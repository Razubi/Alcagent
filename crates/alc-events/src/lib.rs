//! Event bus and event taxonomy.
//!
//! Events are the primary integration surface between subsystems. Systems
//! emit typed events; other systems subscribe. Randomness may seed event
//! proposals but never directly overrides simulation state — every event
//! carries the causes that justify it.

use alc_core::SimTime;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub mod generators;

pub use generators::{
    DailyEventEffect, EventConditions, GeneratedEvent, MAX_EVENTS_PER_DAY, generate_daily_events,
};

/// Category of a simulated event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum EventKind {
    // Population / lifecycle
    Admission,
    TransferOut,
    Release,
    ParoleHearing,
    ParoleGranted,
    ParoleDenied,
    Death,
    // Rehab / reintegration
    RehabilitationProgress,
    RehabilitationFailure,
    PostRelease,
    // Construction
    ConstructionComplete,
    ConstructionCancelled,
    ConstructionFailed,
    // Social
    RelationshipShift,
    GangFormed,
    GangDisbanded,
    GangRecruit,
    GangRivalry,
    // Conflict / discipline
    Argument,
    Threat,
    Theft,
    Fight,
    GangClash,
    Protests,
    PropertyDamage,
    ContrabandFound,
    // Health
    MedicalEmergency,
    Illness,
    Injury,
    // Security
    EscapeAttempt,
    EscapePrevented,
    EscapeSucceeded,
    StaffMisconduct,
    Investigation,
    SecurityResponse,
    GuardReport,
    // Institution
    PolicyChange,
    Expansion,
    FoodShortage,
    EquipmentFailure,
    BudgetChange,
    ParoleBoardDecision,
    // Narrative
    Rumor,
    FamilyCorrespondence,
    Visitor,
    LeadershipDispute,
    RehabilitationSuccess,
    ProgramEnrolled,
    // NPC-NACK interactions (models in `alc_world::Interaction`; the two
    // kinds are appended at the end so existing bincode enum indices in
    // saved worlds stay stable).
    Dialogue,
    Interaction,
    // 0.05 lifecycle additions (append-only for bincode stability).
    ContrabandConfiscated,
}

/// A recorded world event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldEvent {
    pub id: u64,
    pub at: SimTime,
    pub kind: EventKind,
    /// Short human-readable description.
    pub summary: String,
    /// Structured actor ids involved (when applicable).
    pub actors: Vec<u64>,
    /// Free-form causal chain explaining the event (Directive §17:
    /// "Events should be explainable by their underlying causes").
    pub causes: Vec<String>,
    /// Confidence 0..1 that the reported facts are accurate (lower when
    /// relayed via rumors or low-credibility guards).
    pub confidence: f32,
}

impl WorldEvent {
    pub fn new(id: u64, at: SimTime, kind: EventKind, summary: impl Into<String>) -> Self {
        Self {
            id,
            at,
            kind,
            summary: summary.into(),
            actors: Vec::new(),
            causes: Vec::new(),
            confidence: 1.0,
        }
    }

    pub fn with_actor(mut self, id: u64) -> Self {
        self.actors.push(id);
        self
    }
    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.causes.push(cause.into());
        self
    }
    pub fn with_confidence(mut self, c: f32) -> Self {
        self.confidence = c.clamp(0.0, 1.0);
        self
    }
}

/// Helper structs for event payloads.

/// Progress update from a rehabilitation program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RehabProgressEvent {
    /// Prisoner who received progress.
    pub prisoner_id: u64,
    /// Program ID.
    pub program: String,
    /// Progress delta (0..1, positive for progress, negative for regression).
    pub delta: f32,
    /// Current progress (0..1).
    pub progress: f32,
}

/// Rehabilitation failure reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RehabFailureReason {
    DroppedOut,
    DisciplinaryRemoval,
    InsufficientEngagement,
    ProgramCompleted,
}

/// Failure event from rehab progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RehabFailureEvent {
    /// Prisoner who failed.
    pub prisoner_id: u64,
    /// Program ID.
    pub program: String,
    /// Reason for failure.
    pub reason: RehabFailureReason,
}

/// Post-release reintegration record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostReleaseRecord {
    /// Prisoner who was released.
    pub prisoner_id: u64,
    /// Release type: parole, escape, transfer, etc.
    pub release_type: String,
    /// Date of release.
    pub at: SimTime,
    /// Recidivism risk score (0..1, higher = riskier).
    pub recidivism_risk: f32,
    /// Whether the prisoner had a plan/arrangements.
    pub has_reintegration_plan: bool,
    /// Institutional follow-up flag.
    pub follow_up_required: bool,
}

/// In-memory event bus with bounded history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBus {
    history: VecDeque<WorldEvent>,
    next_id: u64,
    max_history: usize,
}

impl EventBus {
    pub fn new(max_history: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(max_history),
            next_id: 1,
            max_history,
        }
    }

    /// Emit an event; returns the assigned event id.
    pub fn emit(&mut self, mut e: WorldEvent) -> u64 {
        e.id = self.next_id;
        self.next_id += 1;
        let id = e.id;
        if self.history.len() >= self.max_history {
            self.history.pop_front();
        }
        self.history.push_back(e);
        id
    }

    pub fn recent(&self) -> impl Iterator<Item = &WorldEvent> {
        self.history.iter()
    }

    /// Events with an id greater than `after_id`, in chronological order.
    ///
    /// Event ids increase monotonically and the deque is append-only, so this
    /// walks only the `k` new events from the back instead of scanning the
    /// entire bounded history on every tick.
    pub fn after(&self, after_id: u64) -> Vec<WorldEvent> {
        let mut out: Vec<WorldEvent> = self
            .history
            .iter()
            .rev()
            .take_while(|e| e.id > after_id)
            .cloned()
            .collect();
        out.reverse();
        out
    }

    /// The most recent `n` events in chronological order.
    ///
    /// Clones only the requested tail instead of materialising the whole
    /// bounded history, which matters for observer snapshots on long runs.
    pub fn last(&self, n: usize) -> Vec<WorldEvent> {
        let mut out: Vec<WorldEvent> = self.history.iter().rev().take(n).cloned().collect();
        out.reverse();
        out
    }

    /// The most recent `n` events, newest first (ready for display).
    pub fn last_reversed(&self, n: usize) -> Vec<WorldEvent> {
        self.history.iter().rev().take(n).cloned().collect()
    }

    pub fn since(&self, at: SimTime) -> impl Iterator<Item = &WorldEvent> {
        self.history.iter().filter(move |e| e.at >= at)
    }
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Emit a rehabilitation progress event.
    pub fn emit_rehab_progress(
        &mut self,
        at: SimTime,
        prisoner_id: u64,
        program: &str,
        delta: f32,
        _progress: f32,
    ) -> u64 {
        let e = WorldEvent::new(
            1,
            at,
            EventKind::RehabilitationProgress,
            format!("Rehab progress: {} {:+.2}", program, delta),
        )
        .with_actor(prisoner_id)
        .with_cause(format!("Rehab progress delta {:+.2} over step", delta))
        .with_confidence(1.0);
        self.emit(e)
    }

    /// Emit a rehabilitation failure event.
    pub fn emit_rehab_failure(
        &mut self,
        at: SimTime,
        prisoner_id: u64,
        program: &str,
        reason: RehabFailureReason,
    ) -> u64 {
        let reason_str = match reason {
            RehabFailureReason::DroppedOut => "dropped out",
            RehabFailureReason::DisciplinaryRemoval => "removed due to discipline",
            RehabFailureReason::InsufficientEngagement => "insufficient engagement",
            RehabFailureReason::ProgramCompleted => "program completed",
        };
        let e = WorldEvent::new(
            1,
            at,
            EventKind::RehabilitationFailure,
            format!("Rehab failure: {} ({})", program, reason_str),
        )
        .with_actor(prisoner_id)
        .with_cause(format!("Rehab failure reason: {:?}", reason))
        .with_confidence(1.0);
        self.emit(e)
    }

    /// Emit a post-release record event.
    pub fn emit_post_release(
        &mut self,
        at: SimTime,
        prisoner_id: u64,
        release_type: &str,
        recidivism_risk: f32,
        _has_reintegration_plan: bool,
    ) -> u64 {
        let e = WorldEvent::new(
            1,
            at,
            EventKind::PostRelease,
            format!(
                "Post-release: {} (risk {:.2})",
                release_type, recidivism_risk
            ),
        )
        .with_actor(prisoner_id)
        .with_cause(format!(
            "Post-release recidivism risk {:.2}",
            recidivism_risk
        ))
        .with_confidence(1.0);
        self.emit(e)
    }

    /// Emit a construction complete event.
    pub fn emit_construction_complete(&mut self, at: SimTime, facility: &str) -> u64 {
        let e = WorldEvent::new(
            1,
            at,
            EventKind::ConstructionComplete,
            format!("Construction complete: {}", facility),
        )
        .with_cause(format!("Construction of {} completed", facility))
        .with_confidence(1.0);
        self.emit(e)
    }

    /// Emit a construction cancelled event.
    pub fn emit_construction_cancelled(
        &mut self,
        at: SimTime,
        facility: &str,
        reason: &str,
    ) -> u64 {
        let e = WorldEvent::new(
            1,
            at,
            EventKind::ConstructionCancelled,
            format!("Construction cancelled: {} ({})", facility, reason),
        )
        .with_cause(reason.to_string())
        .with_confidence(1.0);
        self.emit(e)
    }

    /// Emit a construction failed event.
    pub fn emit_construction_failed(&mut self, at: SimTime, facility: &str, reason: &str) -> u64 {
        let e = WorldEvent::new(
            1,
            at,
            EventKind::ConstructionFailed,
            format!("Construction failed: {} ({})", facility, reason),
        )
        .with_cause(reason.to_string())
        .with_confidence(1.0);
        self.emit(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::SimTime;

    #[test]
    fn event_kind_has_new_kinds() {
        // Verify the new event kinds are accessible
        let _ = EventKind::RehabilitationProgress;
        let _ = EventKind::RehabilitationFailure;
        let _ = EventKind::PostRelease;
    }

    #[test]
    fn last_returns_only_the_requested_tail_in_both_orders() {
        let mut bus = EventBus::new(16);
        for i in 0..8 {
            bus.emit(WorldEvent::new(
                1,
                SimTime { secs: i },
                EventKind::Admission,
                format!("event {i}"),
            ));
        }
        let chronological = bus.last(3);
        assert_eq!(
            chronological.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![6, 7, 8]
        );
        assert_eq!(
            bus.last_reversed(3)
                .iter()
                .map(|e| e.id)
                .collect::<Vec<_>>(),
            vec![8, 7, 6]
        );
        assert!(bus.last(0).is_empty());
        assert_eq!(bus.last(100).len(), 8);
    }

    #[test]
    fn world_event_new_and_modifiers() {
        let e = WorldEvent::new(
            1,
            SimTime { secs: 0 },
            EventKind::Admission,
            "test admission",
        );
        assert_eq!(e.id, 1);
        assert_eq!(e.at.secs, 0);
        assert_eq!(e.kind, EventKind::Admission);
        assert_eq!(e.summary, "test admission");
        assert_eq!(e.actors, []);
        assert!(e.causes.is_empty());
        assert_eq!(e.confidence, 1.0);

        let e2 = e
            .with_actor(42)
            .with_cause("test cause")
            .with_confidence(0.8);
        assert_eq!(e2.actors, [42]);
        assert_eq!(e2.causes, ["test cause"]);
        assert_eq!(e2.confidence, 0.8);
    }

    #[test]
    fn emit_rehab_progress() {
        let mut bus = EventBus::new(100);
        let id = bus.emit_rehab_progress(SimTime { secs: 10 }, 42, "education", 0.05, 0.3);
        assert!(id > 0);
        let events = bus.recent().collect::<Vec<_>>();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, EventKind::RehabilitationProgress));
        assert_eq!(events[0].actors, [42]);
    }

    #[test]
    fn emit_rehab_failure() {
        let mut bus = EventBus::new(100);
        let id = bus.emit_rehab_failure(
            SimTime { secs: 20 },
            42,
            "anger_mgmt",
            RehabFailureReason::DroppedOut,
        );
        assert!(id > 0);
        let events = bus.recent().collect::<Vec<_>>();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, EventKind::RehabilitationFailure));
        assert_eq!(events[0].actors, [42]);
    }

    #[test]
    fn emit_post_release() {
        let mut bus = EventBus::new(100);
        let id = bus.emit_post_release(SimTime { secs: 30 }, 42, "parole", 0.15, true);
        assert!(id > 0);
        let events = bus.recent().collect::<Vec<_>>();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, EventKind::PostRelease));
        assert_eq!(events[0].actors, [42]);
    }

    #[test]
    fn eventbus_bounded_history() {
        let mut bus = EventBus::new(3);
        bus.emit(WorldEvent::new(
            1,
            SimTime { secs: 0 },
            EventKind::Admission,
            "first",
        ));
        bus.emit(WorldEvent::new(
            2,
            SimTime { secs: 1 },
            EventKind::Admission,
            "second",
        ));
        bus.emit(WorldEvent::new(
            3,
            SimTime { secs: 2 },
            EventKind::Admission,
            "third",
        ));
        // Adding 4th should pop the first
        bus.emit(WorldEvent::new(
            4,
            SimTime { secs: 3 },
            EventKind::Admission,
            "fourth",
        ));
        let recent: Vec<_> = bus.recent().collect();
        assert_eq!(recent.len(), 3);
        // The first should have been popped
        assert!(!recent.iter().any(|e| e.summary == "first"));
    }
}
