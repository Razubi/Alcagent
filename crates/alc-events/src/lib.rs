//! Event bus and event taxonomy.
//!
//! Events are the primary integration surface between subsystems. Systems
//! emit typed events; other systems subscribe. Randomness may seed event
//! proposals but never directly overrides simulation state — every event
//! carries the causes that justify it.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use alc_core::SimTime;

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
    ConstructionComplete,
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
        Self { id, at, kind, summary: summary.into(), actors: Vec::new(), causes: Vec::new(), confidence: 1.0 }
    }

    pub fn with_actor(mut self, id: u64) -> Self { self.actors.push(id); self }
    pub fn with_cause(mut self, cause: impl Into<String>) -> Self { self.causes.push(cause.into()); self }
    pub fn with_confidence(mut self, c: f32) -> Self { self.confidence = c.clamp(0.0, 1.0); self }
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
        Self { history: VecDeque::with_capacity(max_history), next_id: 1, max_history }
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

    pub fn recent(&self) -> impl Iterator<Item = &WorldEvent> { self.history.iter() }
    pub fn since(&self, at: SimTime) -> impl Iterator<Item = &WorldEvent> {
        self.history.iter().filter(move |e| e.at >= at)
    }
    pub fn len(&self) -> usize { self.history.len() }
}