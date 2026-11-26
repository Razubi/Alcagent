//! sumo — the simulation engine (the public API of the Alcagent prison simulation).
//!
//! `Sim` owns the [`World`] and drives it through the engine API:
//!
//! - **Time control** — [`Sim::tick`], [`Sim::run_until`], [`Sim::speed`].
//! - **Decision pipeline** — [`Sim::deliver_decision`]: agents propose
//!   [`alc_agents::Decision`]s; the engine validates them (via
//!   [`alc_agents::DecisionValidator`] plus engine preconditions) and applies
//!   them. Agents never mutate world state directly (Directive §19).
//! - **Observation** — [`Sim::observation`], [`Sim::stats_since`], [`Sim::map_frame`]: read-only
//!   snapshots for observers and UI (§20).
//! - **Events** — [`Sim::events`], [`Sim::events_since`]: the world's event
//!   bus (§17 — the integration surface between subsystems).
//! - **Persistence** — [`Sim::save`], [`Sim::load`], [`Sim::enable_autosave`]
//!   / [`Sim::maybe_autosave`] (§23).
//! - **Invariants** — [`Sim::check_invariants`] (§22/§23).
//! - **Report** — [`Sim::report`] (parity with the `alcagent` binary).
//!
//! ## Design
//! The design is documented in `Documentation/Design/Engine/sumo-engine.md`.
//!
//! Observation UIs live in the `alc-ui` and `alc-gui` crates; this engine crate
//! is headless and exposes read-only DTOs for them.

#![deny(missing_docs)]
#![deny(unused_crate_dependencies)]

pub mod autosave;

pub mod decisions;

pub use crate::decisions::{DecisionBatchOutcome, DecisionProposal, DecisionType};
/// Re-export decision types for convenience and doctest accessibility.
pub use alc_agents::{AiRuntimeSnapshot, ValidationResult};

/// Council of drives: deterministic institutional decision evaluation (R3).
pub mod council;

pub mod drivers;

pub mod map_frame;
/// Re-export map frame types for UI rendering.
pub use crate::map_frame::{MapFrame, NavEdge, PawnPosition};

/// Shared graphics primitives used by every observation frontend.
pub mod graphics;
pub use crate::graphics::{
    CONSTRUCTION_KINDS, Color, MapBounds, MapCamera, MapZone, MapZoneKind, Palette, ThemeKind,
    ViewMode, WorldMapLayout,
};

pub mod observation;
/// Re-export observation types for UI and external consumers.
pub use crate::observation::{AgentObservation, Observation, Stats};

pub mod parole;

pub mod incidents;

/// Re-export structured incident query types for UI/CLI consumers (0.05).
pub use crate::incidents::{Incident, IncidentFilter, filter_incidents};

pub mod invariants;

pub mod state_digest;

pub use alc_world::World;

/// Re-export core types for UI and external consumers.
pub use alc_core::SimTime;
pub use alc_events::WorldEvent;

pub mod state;
pub mod views;
pub use state::{
    DecisionAuditRecord, EngineState, IncidentRecord, Policy, PolicyMode, PolicyState,
    PolicySummary,
};
pub use views::*;

// Re-exported for internal modules that historically imported these from crate root.
pub use crate::decisions::DecisionOutcome;
pub use crate::parole::{ParoleConditions, ParoleLedger, ParoleStatus};
pub use alc_agents::Decision;
pub use alc_core::{GuardId, PrisonerId};
pub use alc_events::EventBus;
pub use alc_warden::WardenAction;

use alc_warden::{DecisionSource, StrategyDecision, WardenReportSignal, WardenStrategySnapshot};

use crate::autosave::AutosaveScheduler;
use crate::views::{
    REHAB_PROGRAMS, ReleaseEventInfo, parse_f32_after, parse_plan_flag, rehab_state_for,
    write_back_rehab_state,
};
use alc_core::TimeSpeed;
use alc_core::config::AiConfig;
use alc_events::EventKind;
use alc_guard::{GuardReport, PostType};
use alc_persistence::{SaveError, SaveManager};
use alc_prisoner::NeedTag;
use alc_prisoner::{
    AttendanceContext, SchedulePolicy, ScheduleTemplate, attendance_probability, current_block,
    resolve_schedule,
};
use alc_social::contraband::ContrabandCaseRecord;
use alc_social::rehab::{RehabProgram, enroll_in_program};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Extract a reported field from the structured guard-report cause string.
fn guard_cause_field<'a>(cause: &'a str, key: &str) -> Option<&'a str> {
    cause
        .split_whitespace()
        .find_map(|part| part.strip_prefix(key))
}

fn guard_report_post(cause: &str) -> PostType {
    let value = guard_cause_field(cause, "post=").unwrap_or("Corridor");
    match value {
        "Tower" => PostType::Tower,
        "Wing" => PostType::Wing,
        "Corridor" => PostType::Corridor,
        "Yard" => PostType::Yard,
        "Reception" => PostType::Reception,
        "Medical" => PostType::Medical,
        "Work" => PostType::Work,
        "Visitor" => PostType::Visitor,
        "Admin" => PostType::Admin,
        _ => PostType::Corridor,
    }
}

/// Rebuild the DTO the warden actually received from an event id.
///
/// The warden never reads raw world incidents here: only ids placed in its
/// inbox are resolved, so fog-of-war remains intact in the observer view.
fn guard_report_from_event(event: &alc_events::WorldEvent) -> Option<GuardReport> {
    let cause = event.causes.first()?.as_str();
    if !cause.starts_with("guard duty report") {
        return None;
    }
    let severity = guard_cause_field(cause, "sev=")?
        .parse::<u8>()
        .unwrap_or(0)
        .min(3);
    let witnessed = guard_cause_field(cause, "witness=")
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(0)
        != 0;
    let requested_assistance = guard_cause_field(cause, "assist=")
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(0)
        != 0;
    Some(GuardReport {
        at: event.at,
        post: guard_report_post(cause),
        summary: event.summary.clone(),
        severity,
        credibility: event.confidence.clamp(0.05, 0.99),
        witnessed_directly: witnessed,
        requests_assistance: requested_assistance
            || event.kind == EventKind::SecurityResponse
            || event.summary.starts_with("Assistance requested:"),
    })
}

/// Rebuild the strategy signal for one delivered guard report. Like
/// [`guard_report_from_event`], this only reads event ids already present in
/// the warden inbox; it never reads a raw incident from the world.
fn warden_report_signal_from_event(event: &alc_events::WorldEvent) -> Option<WardenReportSignal> {
    let report = guard_report_from_event(event)?;
    Some(WardenReportSignal {
        id: event.id,
        at: event.at,
        summary: report.summary,
        severity: report.severity,
        credibility: report.credibility,
        requests_assistance: report.requests_assistance,
    })
}

/// The simulation engine: owns the world and drives it over time.
pub struct Sim {
    /// The world state, owned by the engine.
    pub world: World,
    speed: TimeSpeed,
    /// Engine persistence state (S0: save/load, logs, policy mode).
    /// This is the SINGLE SOURCE OF TRUTH for all engine-side state.
    pub engine_state: EngineState,
    last_autosave_error: Option<String>,
}

impl Sim {
    /// Create a new simulation from a seed. The world boots with the starting
    /// facility set, guards and prisoners (Directive §2 bootstrap).
    pub fn new(seed: u64) -> Self {
        Self::new_with_config(seed, &AiConfig::default())
    }

    /// Create a simulation with an explicit AI orchestration policy.
    ///
    /// The config controls provider, model, token budgets, rate limiting,
    /// retries, caching and batching. It is runtime-only and is not added to
    /// the v5 save format.
    pub fn new_with_config(seed: u64, ai: &AiConfig) -> Self {
        Self {
            world: World::new_with_config(seed, ai),
            speed: TimeSpeed::Paused,
            engine_state: EngineState::default(),
            last_autosave_error: None,
        }
    }

    /// Access the parole ledger from engine_state.
    pub fn parole(&self) -> &ParoleLedger {
        &self.engine_state.parole
    }

    /// Access the parole ledger mutably from engine_state.
    pub fn parole_mut(&mut self) -> &mut ParoleLedger {
        &mut self.engine_state.parole
    }

    /// Access population history from engine_state.
    pub fn population_history(&self) -> &Vec<(SimTime, u32)> {
        &self.engine_state.population_history
    }

    /// Access population history mutably from engine_state.
    pub fn population_history_mut(&mut self) -> &mut Vec<(SimTime, u32)> {
        &mut self.engine_state.population_history
    }

    /// Access incidents from engine_state.
    pub fn incidents(&self) -> &Vec<alc_events::WorldEvent> {
        &self.engine_state.incidents
    }

    /// Access incidents mutably from engine_state.
    pub fn incidents_mut(&mut self) -> &mut Vec<alc_events::WorldEvent> {
        &mut self.engine_state.incidents
    }

    /// Access incident_sidecar from engine_state.
    pub fn incident_sidecar(&self) -> &Vec<incidents::Incident> {
        &self.engine_state.incident_sidecar
    }

    /// Access incident_sidecar mutably from engine_state.
    pub fn incident_sidecar_mut(&mut self) -> &mut Vec<incidents::Incident> {
        &mut self.engine_state.incident_sidecar
    }

    /// Access decision_log from engine_state.
    pub fn decision_log(&self) -> &Vec<alc_agents::Decision> {
        &self.engine_state.decision_log
    }

    /// Access decision_log mutably from engine_state.
    pub fn decision_log_mut(&mut self) -> &mut Vec<alc_agents::Decision> {
        &mut self.engine_state.decision_log
    }

    /// Structured decision audit log for observer UI (0.05).
    pub fn decision_audit_log(&self) -> &[DecisionAuditRecord] {
        &self.engine_state.decision_audit
    }

    /// Record a structured decision audit entry, bounded here so callers can
    /// never grow the audit store past the invariant limit.
    pub fn record_decision_audit(&mut self, record: DecisionAuditRecord) {
        self.engine_state.decision_audit.push(record);
        let max = crate::invariants::MAX_LOG_ENTRIES;
        if self.engine_state.decision_audit.len() > max {
            let excess = self.engine_state.decision_audit.len() - max;
            self.engine_state.decision_audit.drain(..excess);
        }
    }

    /// Record a rejected/invalid control attempt. Unlike [`Self::audit_control`],
    /// this marks the record as rejected so observers can distinguish validated
    /// mutations from attempts the engine refused.
    pub fn audit_control_rejected(
        &mut self,
        actor: impl Into<String>,
        action: impl Into<String>,
        summary: impl Into<String>,
        reason: impl Into<String>,
    ) {
        self.record_decision_audit(DecisionAuditRecord {
            time: self.world.time,
            actor: actor.into(),
            source: "control".to_string(),
            action: action.into(),
            summary: summary.into(),
            validation: "rejected".to_string(),
            reason: reason.into(),
            rationale: String::new(),
            repair_reason: String::new(),
            outcome: alc_agents::decision::DecisionOutcomeState::Rejected,
        });
    }

    /// Record a control/observer mutation in the decision audit log (0.05).
    pub fn audit_control(
        &mut self,
        actor: impl Into<String>,
        action: impl Into<String>,
        summary: impl Into<String>,
        validation: impl Into<String>,
        reason: impl Into<String>,
    ) {
        let validation = validation.into();
        let outcome = match validation.as_str() {
            "repaired" => alc_agents::decision::DecisionOutcomeState::Repaired,
            "rejected" => alc_agents::decision::DecisionOutcomeState::Rejected,
            _ => alc_agents::decision::DecisionOutcomeState::Accepted,
        };
        self.record_decision_audit(DecisionAuditRecord {
            time: self.world.time,
            actor: actor.into(),
            source: "control".to_string(),
            action: action.into(),
            summary: summary.into(),
            validation,
            reason: reason.into(),
            rationale: String::new(),
            repair_reason: String::new(),
            outcome,
        });
    }

    /// Access post_release from engine_state.
    pub fn post_release(&self) -> &HashMap<alc_core::PrisonerId, bool> {
        &self.engine_state.post_release
    }

    /// Access post_release mutably from engine_state.
    pub fn post_release_mut(&mut self) -> &mut HashMap<alc_core::PrisonerId, bool> {
        &mut self.engine_state.post_release
    }

    /// Access policy_state from engine_state.
    pub fn policy_state(&self) -> &PolicyState {
        &self.engine_state.policy_state
    }

    /// Access policy_state mutably from engine_state.
    pub fn policy_state_mut(&mut self) -> &mut PolicyState {
        &mut self.engine_state.policy_state
    }

    /// Access policy_mode from engine_state.
    pub fn policy_mode(&self) -> PolicyMode {
        self.engine_state.policy_mode
    }

    /// Set policy_mode in engine_state.
    pub fn set_policy_mode(&mut self, mode: PolicyMode) {
        let old = self.engine_state.policy_mode;
        if old == mode {
            return;
        }
        self.engine_state.policy_mode = mode;
        self.audit_control(
            "system",
            "SetPolicyMode",
            format!("Policy mode {:?} -> {:?}", old, mode),
            "accepted",
            "observer/control policy mode change",
        );
    }

    /// Set the simulation speed.
    pub fn set_speed(&mut self, speed: TimeSpeed) {
        self.speed = speed;
        self.world.speed = speed;
    }
    /// Get the current simulation speed.
    pub fn speed(&self) -> TimeSpeed {
        self.speed
    }

    /// Advance one tick. No-op while paused.
    pub fn tick(&mut self) {
        if self.speed == TimeSpeed::Paused {
            return;
        }
        tracing::debug!(time = ?self.world.time, "tick");
        let last_event_id = self.world.events.recent().last().map(|e| e.id).unwrap_or(0);
        let prev_decisions = self.world.current_decisions.clone();
        self.world.tick();
        // World::tick runs its own warden/guard cycles that mutate
        // world.policies directly; reconcile the engine mirror every tick.
        self.sync_policy_from_world();
        // Audit world-tick decisions (0.05).
        let mut new_audits = Vec::new();
        for (pid, decision) in &self.world.current_decisions {
            if prev_decisions.get(pid) != Some(decision) {
                // Fallback decisions are deterministic/utility choices, not AI
                // proposals. They are validated by the same gate but must not
                // be mislabelled as accepted agent output in the audit trail.
                let fallback =
                    decision.provenance == alc_agents::decision::DecisionProvenance::System;
                let (source, validation, reason) = if fallback {
                    (
                        "world_tick_fallback",
                        "deterministic_fallback",
                        "applied by world tick without AI deliberation",
                    )
                } else {
                    let validation = if decision.outcome
                        == alc_agents::decision::DecisionOutcomeState::Repaired
                    {
                        "repaired"
                    } else {
                        "accepted"
                    };
                    ("world_tick", validation, "applied by world tick")
                };
                new_audits.push(DecisionAuditRecord {
                    time: self.world.time,
                    actor: format!("prisoner:{}", pid.raw()),
                    source: source.to_string(),
                    action: decision.action_name().to_string(),
                    summary: format!("{:?}", decision),
                    validation: validation.to_string(),
                    reason: reason.to_string(),
                    rationale: decision.rationale.clone(),
                    repair_reason: decision.repair_reason.clone(),
                    outcome: decision.outcome,
                });
            }
        }
        for record in new_audits {
            self.record_decision_audit(record);
        }
        // Mirror newly-emitted world events into the engine-side incident log
        // and structured sidecar (C1: logs were never populated at runtime).
        self.pump_events_into_engine_log(last_event_id);
        let pop = self.world.population();
        let history = &mut self.engine_state.population_history;
        if history.last().map(|&(_, p)| p) != Some(pop) {
            history.push((self.world.time, pop));
        }
        // `maybe_autosave` records and logs its own failures, so no caller can
        // silently drop an autosave error.
        let _ = self.maybe_autosave();
    }

    /// Copy newly-emitted conflict events into the engine incident log and
    /// structured sidecar (C1). Non-conflict events stay on the world bus only
    /// so the engine audit store remains bounded and meaningful. Causal links
    /// are derived from the event's `causes` and from prior incidents sharing a
    /// participant or kind — no self-referential placeholders.
    fn pump_events_into_engine_log(&mut self, after_id: u64) {
        // O(k) in the number of newly emitted events; scanning the full
        // bounded history on every tick made long runs needlessly expensive.
        let new = self.world.events.after(after_id);
        for event in &new {
            self.mirror_conflict_event_to_engine_log(event);
        }
        self.mirror_parole_events(&new);
    }

    /// Reconcile the engine parole ledger with world-driven hearings. The
    /// world runs deterministic daily hearings; without this mirror its
    /// decisions would never reach the persisted ledger and observers would see
    /// a prisoner as perpetually "due" after a world-level grant or denial.
    fn mirror_parole_events(&mut self, events: &[WorldEvent]) {
        for event in events {
            let Some(&raw_id) = event.actors.first() else {
                continue;
            };
            let id = PrisonerId(raw_id);
            match event.kind {
                EventKind::ParoleHearing => {
                    if !self.engine_state.parole.has(id) {
                        self.engine_state
                            .parole
                            .set(id, ParoleStatus::Scheduled, event.at);
                    }
                    if self.engine_state.parole.hearing_at(id).is_none() {
                        self.engine_state.parole.set_hearing_at(id, event.at);
                    }
                }
                EventKind::ParoleGranted => {
                    if !matches!(
                        self.engine_state.parole.status(id),
                        Some(ParoleStatus::Denied)
                    ) {
                        self.engine_state
                            .parole
                            .set(id, ParoleStatus::Granted, event.at);
                        if self.engine_state.parole.conditions(id).is_none() {
                            let conditions = ParoleConditions::deterministic(id, 0.5, 0.5);
                            self.engine_state.parole.set_conditions(id, conditions);
                        }
                        if self.engine_state.parole.decision_reason(id).is_none() {
                            let reason = event.causes.first().cloned().unwrap_or_else(|| {
                                "world parole board granted release".to_string()
                            });
                            self.engine_state.parole.set_decision_reason(id, reason);
                        }
                        if !self.world.prisoners.iter().any(|p| p.id == id)
                            && self.engine_state.parole.released_at(id).is_none()
                        {
                            self.engine_state.parole.set_released_at(id, event.at);
                        }
                    }
                }
                EventKind::ParoleDenied => {
                    if !matches!(
                        self.engine_state.parole.status(id),
                        Some(ParoleStatus::Granted)
                    ) {
                        self.engine_state
                            .parole
                            .set(id, ParoleStatus::Denied, event.at);
                        let reason = event
                            .causes
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "world parole board denied release".to_string());
                        self.engine_state.parole.set_decision_reason(id, reason);
                    }
                }
                _ => {}
            }
        }
    }

    fn is_engine_mirrored_kind(kind: alc_events::EventKind) -> bool {
        use alc_events::EventKind;
        matches!(
            kind,
            EventKind::Fight
                | EventKind::Argument
                | EventKind::Threat
                | EventKind::Theft
                | EventKind::GangClash
                | EventKind::Protests
                | EventKind::PropertyDamage
                | EventKind::MedicalEmergency
                | EventKind::Rumor
                | EventKind::FamilyCorrespondence
                | EventKind::StaffMisconduct
                | EventKind::EquipmentFailure
                | EventKind::FoodShortage
                | EventKind::EscapeAttempt
                | EventKind::EscapePrevented
                | EventKind::EscapeSucceeded
                | EventKind::ContrabandFound
                | EventKind::Death
                | EventKind::TransferOut
                | EventKind::ConstructionComplete
                | EventKind::ConstructionCancelled
                | EventKind::ConstructionFailed
        )
    }

    /// True when a world event has already been mirrored into either engine
    /// incident store. The primary log is bounded, so old events can survive
    /// only as `CausalLink::world_event` provenance on a sidecar incident.
    fn world_event_was_mirrored(&self, event_id: u64) -> bool {
        if self.engine_state.incidents.iter().any(|e| e.id == event_id) {
            return true;
        }
        self.engine_state.incident_sidecar.iter().any(|inc| {
            inc.causal_chain
                .iter()
                .any(|link| !link.is_incident && link.source_id == event_id)
        })
    }

    /// Mirror one conflict/incident-kind event into engine incident stores with
    /// dedupe by world event id. Public emission and tick pumping both route
    /// here so mirrored kinds are consistently recorded.
    pub(crate) fn mirror_conflict_event_to_engine_log(&mut self, event: &WorldEvent) {
        if !Self::is_engine_mirrored_kind(event.kind) {
            return;
        }
        if self.world_event_was_mirrored(event.id) {
            return;
        }
        let next_id = self
            .engine_state
            .incident_sidecar
            .iter()
            .map(|inc| inc.id)
            .max()
            .unwrap_or(0)
            .max(self.engine_state.incident_sidecar.len() as u64)
            + 1;
        let severity = match event.kind {
            alc_events::EventKind::Fight
            | alc_events::EventKind::GangClash
            | alc_events::EventKind::EscapeSucceeded
            | alc_events::EventKind::Death => 0.9,
            alc_events::EventKind::Protests => 0.75,
            alc_events::EventKind::EscapeAttempt
            | alc_events::EventKind::MedicalEmergency
            | alc_events::EventKind::TransferOut => 0.7,
            alc_events::EventKind::StaffMisconduct => 0.65,
            alc_events::EventKind::Threat
            | alc_events::EventKind::Theft
            | alc_events::EventKind::ContrabandFound => 0.6,
            alc_events::EventKind::FoodShortage => 0.55,
            alc_events::EventKind::EquipmentFailure => 0.4,
            alc_events::EventKind::Rumor => 0.25,
            alc_events::EventKind::FamilyCorrespondence => 0.2,
            alc_events::EventKind::ConstructionComplete => 0.2,
            _ => 0.4,
        };
        let location = format!("facility {}", event.actors.first().copied().unwrap_or(0));
        let known_prisoners: Vec<alc_core::PrisonerId> =
            self.world.prisoners.iter().map(|p| p.id).collect();
        let prior = &self.engine_state.incident_sidecar[..];
        let incident = incidents::incident_from_world_event(
            event,
            next_id,
            prior,
            &known_prisoners,
            severity,
            location,
        );
        self.engine_state.incident_sidecar.push(incident);
        self.engine_state.incidents.push(event.clone());
        self.trim_engine_logs();
    }

    /// Keep engine audit stores within the bounded invariant limit (R6).
    fn trim_engine_logs(&mut self) {
        const MAX: usize = crate::invariants::MAX_LOG_ENTRIES;
        let events_trimmed = self.engine_state.incidents.len() > MAX;
        if events_trimmed {
            let excess = self.engine_state.incidents.len() - MAX;
            self.engine_state.incidents.drain(..excess);
        }
        let mut sidecar_trimmed = false;
        if self.engine_state.incident_sidecar.len() > MAX {
            let excess = self.engine_state.incident_sidecar.len() - MAX;
            self.engine_state.incident_sidecar.drain(..excess);
            sidecar_trimmed = true;
        }
        if sidecar_trimmed || events_trimmed {
            // B2: drop cause/consequence and chain references to incidents or
            // world events evicted by a bounded log, so causality stays honest
            // instead of accumulating dangling ids on long runs. Both live id
            // sets are built once per trim, never per link.
            let live_incidents: std::collections::HashSet<u64> = self
                .engine_state
                .incident_sidecar
                .iter()
                .map(|inc| inc.id)
                .collect();
            let live_events: std::collections::HashSet<u64> = self
                .engine_state
                .incidents
                .iter()
                .map(|event| event.id)
                .collect();
            for inc in &mut self.engine_state.incident_sidecar {
                if inc.cause.is_some_and(|id| !live_incidents.contains(&id)) {
                    inc.cause = None;
                }
                if inc
                    .consequence
                    .is_some_and(|id| !live_incidents.contains(&id))
                {
                    inc.consequence = None;
                }
                if !inc.causal_chain.is_empty() {
                    inc.causal_chain.retain(|link| {
                        if link.is_incident {
                            live_incidents.contains(&link.source_id)
                        } else {
                            live_events.contains(&link.source_id)
                        }
                    });
                }
            }
        }
        if self.engine_state.decision_log.len() > MAX {
            let excess = self.engine_state.decision_log.len() - MAX;
            self.engine_state.decision_log.drain(..excess);
        }
        if self.engine_state.decision_audit.len() > MAX {
            let excess = self.engine_state.decision_audit.len() - MAX;
            self.engine_state.decision_audit.drain(..excess);
        }
        if self.engine_state.resolved_contraband_cases.len() > MAX {
            // Keep highest ids (most recent) when trimming.
            while self.engine_state.resolved_contraband_cases.len() > MAX {
                if let Some(first) = self
                    .engine_state
                    .resolved_contraband_cases
                    .iter()
                    .next()
                    .copied()
                {
                    self.engine_state.resolved_contraband_cases.remove(&first);
                    self.engine_state.resolved_contraband_at.remove(&first);
                } else {
                    break;
                }
            }
        }
        if self.engine_state.resolved_contraband_at.len() > MAX {
            while self.engine_state.resolved_contraband_at.len() > MAX {
                if let Some(first) = self
                    .engine_state
                    .resolved_contraband_at
                    .keys()
                    .next()
                    .copied()
                {
                    self.engine_state.resolved_contraband_at.remove(&first);
                    self.engine_state.resolved_contraband_cases.remove(&first);
                } else {
                    break;
                }
            }
        }
    }

    /// Advance until `target` time or until the simulation pauses.
    /// Returns the number of ticks run.
    pub fn run_until(&mut self, target: SimTime) -> u64 {
        let mut n = 0;
        while self.world.time < target && self.speed != TimeSpeed::Paused {
            let before = self.world.time;
            self.tick();
            n += 1;
            if self.world.time == before {
                break; // No progress possible (e.g. a day boundary loop).
            }
        }
        n
    }

    /// Get a reference to the world state.
    pub fn world(&self) -> &World {
        &self.world
    }
    /// Mutable world access for engine tests and fixture setup only.
    ///
    /// This bypasses the validated control boundary and is not available to
    /// observers or agents: it is gated behind the `test-support` feature (and
    /// unit-test builds). Frontends must use the validated `Sim` controls.
    #[cfg(any(test, feature = "test-support"))]
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }
    /// Get the number of days simulated.
    pub fn days_simulated(&self) -> u64 {
        self.world.days_simulated
    }

    /// Get the world's event bus.
    pub fn events(&self) -> &EventBus {
        &self.world.events
    }

    /// Get events since a point in time.
    pub fn events_since(&self, at: SimTime) -> Vec<WorldEvent> {
        self.world.events.since(at).cloned().collect()
    }

    /// Validated decision pipeline: agents propose, the engine disposes.
    pub fn deliver_decision(&mut self, id: PrisonerId, decision: Decision) -> DecisionOutcome {
        decisions::deliver(self, id, decision)
    }

    /// Deliver a bounded batch through the same canonical validation gate.
    ///
    /// Proposals are de-duplicated by prisoner id, capped by
    /// [`AiConfig::max_batch_size`], validated against contexts captured before
    /// any state mutation, and applied only when accepted or repaired.
    ///
    /// [`AiConfig::max_batch_size`]: alc_core::config::AiConfig::max_batch_size
    pub fn deliver_decision_batch(
        &mut self,
        requests: Vec<(PrisonerId, Decision)>,
    ) -> DecisionBatchOutcome {
        decisions::deliver_batch(self, requests)
    }

    /// Snapshot the configured AI runtime policy and telemetry for observers.
    pub fn ai_runtime_snapshot(&self) -> AiRuntimeSnapshot {
        self.world.orchestrator.runtime_snapshot()
    }

    /// Propose a decision without mutating world state.
    ///
    /// This generates a [`DecisionProposal`] by validating the decision
    /// against the current world state, but does not apply it, emit events,
    /// or modify budget/trace_hash. Useful for AI orchestration, policy
    /// simulation, and dry-runs.
    ///
    /// Returns `None` if the prisoner is unavailable (locked down, in hearing,
    /// or otherwise unable to participate in decisions).
    pub fn propose_decision(
        &self,
        prisoner_id: PrisonerId,
        decision_type: crate::decisions::DecisionType,
        target: Option<PrisonerId>,
    ) -> Option<crate::decisions::DecisionProposal> {
        // Check prisoner availability: must exist and not be in a state
        // that blocks decision-making.
        let prisoner = self.world.prisoners.iter().find(|p| p.id == prisoner_id)?;
        // Lockdown restricts most decisions; prisoners in hearing are
        // also unavailable.
        if self.world.policies.lockdown
            && !matches!(
                decision_type,
                crate::decisions::DecisionType::Comply
                    | crate::decisions::DecisionType::Refuse
                    | crate::decisions::DecisionType::None
            )
        {
            return None;
        }
        // If the prisoner currently has a pending AI deliberation, they
        // are considered unavailable for a new proposal.
        if self.world.current_decisions.contains_key(&prisoner_id) {
            return None;
        }
        // Validation uses the single authoritative context inside
        // `validate_with_engine_rules`, so dry-runs agree with delivery (R2).

        // Generate a rationale based on the prisoner's traits and needs.
        let rationale = self.generate_rationale(prisoner, decision_type, target);

        // Build a temporary Decision from the DecisionType and target,
        // then validate it through the pipeline.
        let decision = decision_type.to_decision(target);
        let estimated_outcome =
            crate::decisions::validate_with_engine_rules(&self.world, prisoner_id, &decision);
        let trace_hash_before = self.trace_hash();

        // Brain-inspector enrichment: attach evaluator drives, confidence and
        // cited memories so the UI can explain the proposal.
        let scores = crate::decisions::score_for(prisoner, decision_type);
        let memory_records = crate::decisions::memory_citations(&self.world, prisoner);
        let confidence = crate::decisions::proposal_confidence(
            &scores,
            &estimated_outcome,
            prisoner.needs.total_pressure(),
        );

        Some(crate::decisions::DecisionProposal {
            prisoner_id,
            decision_type,
            target,
            rationale,
            estimated_outcome,
            trace_hash_before,
            scores,
            confidence,
            memory_records,
        })
    }

    /// Dry-run validation of a [`DecisionProposal`].
    ///
    /// Uses real world data for validation (the same context that
    /// `propose_decision` builds) and returns `ValidationResult::Accepted`
    /// or `ValidationResult::Rejected` with a reason. Does not modify any
    /// world state, emit events, or change budget or trace_hash.
    pub fn validate_only(
        &self,
        prisoner_id: PrisonerId,
        proposal: &crate::decisions::DecisionProposal,
    ) -> ValidationResult {
        // Verify the prisoner exists before building a context.
        if !self.world.prisoners.iter().any(|p| p.id == prisoner_id) {
            return ValidationResult::Rejected {
                reason: format!("unknown prisoner id {}", prisoner_id.raw()),
            };
        }

        // Validation uses the single authoritative context inside
        // `validate_with_engine_rules`, so dry-runs agree with delivery.
        let decision = proposal.decision_type.to_decision(proposal.target);
        crate::decisions::validate_with_engine_rules(&self.world, prisoner_id, &decision)
    }

    /// Generate a free-text rationale for why a prisoner would choose a
    /// given decision type. This is deterministic given the prisoner's
    /// traits and the current world state, used by AI orchestration.
    fn generate_rationale(
        &self,
        prisoner: &alc_prisoner::Prisoner,
        decision_type: crate::decisions::DecisionType,
        target: Option<PrisonerId>,
    ) -> String {
        let need_tags: Vec<alc_prisoner::NeedTag> = match decision_type {
            crate::decisions::DecisionType::MoveTo => {
                if prisoner.needs.hunger > 0.7 {
                    vec![alc_prisoner::NeedTag::Hunger]
                } else if prisoner.needs.thirst > 0.7 {
                    vec![alc_prisoner::NeedTag::Thirst]
                } else {
                    vec![]
                }
            }
            crate::decisions::DecisionType::Socialize => {
                if prisoner.traits.sociability > 0.6 {
                    vec![alc_prisoner::NeedTag::Companionship]
                } else {
                    vec![]
                }
            }
            crate::decisions::DecisionType::Fight => {
                if prisoner.needs.respect < 0.3 {
                    vec![alc_prisoner::NeedTag::Respect]
                } else {
                    vec![]
                }
            }
            crate::decisions::DecisionType::RequestMedical => {
                if prisoner.needs.medical_need > 0.6 {
                    vec![alc_prisoner::NeedTag::Medical]
                } else {
                    vec![]
                }
            }
            crate::decisions::DecisionType::Comply => vec![alc_prisoner::NeedTag::Safety],
            crate::decisions::DecisionType::Refuse => vec![alc_prisoner::NeedTag::Autonomy],
            crate::decisions::DecisionType::EscapeAttempt => {
                if prisoner.needs.safety < 0.3 {
                    vec![alc_prisoner::NeedTag::Safety]
                } else {
                    vec![]
                }
            }
            crate::decisions::DecisionType::JoinGang => {
                // ambition not in NeedTag; fall back to no specific tag
                vec![]
            }
            crate::decisions::DecisionType::LeaveGang => {
                if prisoner.needs.safety > 0.7 {
                    vec![alc_prisoner::NeedTag::Safety]
                } else {
                    vec![]
                }
            }
            crate::decisions::DecisionType::SpreadRumor => {
                // manipulation not in NeedTag; fall back to no specific tag
                vec![]
            }
            crate::decisions::DecisionType::None => vec![],
            DecisionType::Trade | DecisionType::LeadGang => vec![],
        };

        let target_note = match target {
            Some(t) => format!(" toward {}", t.raw()),
            None => String::new(),
        };
        if need_tags.is_empty() {
            format!(
                "Prisoner {} has no specific need driving this decision{target_note}",
                prisoner.id.raw()
            )
        } else {
            let tags: Vec<String> = need_tags.iter().map(|t| format!("{:?}", t)).collect();
            format!(
                "Prisoner {} decision driven by needs: {:?}{target_note}",
                prisoner.id.raw(),
                tags
            )
        }
    }

    /// Get a read-only snapshot of the world for observers and UI.
    pub fn observation(&self) -> Observation {
        Observation::capture(&self.world)
    }

    /// Get a point-in-time map frame of all pawn positions for the live map (R1).
    ///
    /// This is the documented entry point for UI map rendering; callers do not
    /// need mutable `World` access to draw pawns.
    pub fn map_frame(&self) -> crate::map_frame::MapFrame {
        crate::map_frame::map_frame(self)
    }

    /// Metrics derived from the event log since `at`.
    pub fn stats_since(&self, at: SimTime) -> Stats {
        let max_population = self
            .population_history()
            .iter()
            .filter(|(t, _)| *t >= at)
            .map(|(_, p)| *p)
            .max()
            .unwrap_or_else(|| self.world.population());
        Stats::from_events(
            at,
            self.world.events.since(at).cloned(),
            self.world.population(),
            self.avg_population_since(at),
            self.world.capacity(),
            max_population,
        )
    }

    // --- Summary types for UI (Wave 5) ---

    /// Get all prisoners as summary types for the prisoner view.
    pub fn prisoners(&self) -> Vec<PrisonerSummary> {
        self.world
            .prisoners
            .iter()
            .map(|p| PrisonerSummary {
                id: p.id,
                name: p.name.clone(),
                age: p.age,
                classification: p.classification,
                sentence_served_days: p.sentence_served_days,
                sentence_days: p.sentence_days,
                gang_affiliation: p.gang_affiliation,
                reputation: p.reputation,
                condition: p.condition.clone(),
                traits: p.traits.clone(),
                needs: p.needs.clone(),
                goals: p.goals.clone(),
                schedule: p.schedule.clone(),
                skills: p.skills.clone(),
                memory_summary: p.memory_summary.clone(),
            })
            .collect()
    }

    /// Get all gangs as summary types for the gang view.
    ///
    /// Relation intensities are computed from observed social ties, goals and
    /// declared alliances/rivalries (not placeholder `1.0` values), and each
    /// summary carries the durable gang timeline entries that mention it.
    pub fn gangs(&self) -> Vec<GangSummary> {
        let mut gangs: Vec<_> = self.world.gangs.iter().collect();
        gangs.sort_by_key(|g| g.id.0);
        let mut scores: BTreeMap<(u64, u64), (f32, f32)> = BTreeMap::new();
        for (index, left) in gangs.iter().enumerate() {
            for right in gangs.iter().skip(index + 1) {
                let (affinity, hostility) =
                    alc_social::gang_relation_scores(left, right, &self.world.social);
                scores.insert((left.id.0, right.id.0), (affinity, hostility));
            }
        }
        let pressure = self.world.gang_pressure();

        gangs
            .iter()
            .copied()
            .map(|g| {
                let score_for = |other: u64| -> (f32, f32) {
                    if g.id.0 <= other {
                        scores.get(&(g.id.0, other)).copied().unwrap_or((0.5, 0.0))
                    } else {
                        scores
                            .get(&(other, g.id.0))
                            .copied()
                            .map(|(affinity, hostility)| (affinity, hostility))
                            .unwrap_or((0.5, 0.0))
                    }
                };
                let mut rivalries: Vec<(u64, f32)> = g
                    .rivalries
                    .iter()
                    .map(|id| (id.0, score_for(id.0).1.max(0.25)))
                    .collect();
                rivalries.sort_by_key(|(id, _)| *id);
                let mut alliances: Vec<(u64, f32)> = g
                    .alliances
                    .iter()
                    .map(|id| (id.0, score_for(id.0).0.max(0.25)))
                    .collect();
                alliances.sort_by_key(|(id, _)| *id);
                let relations: Vec<GangRelationSummary> = gangs
                    .iter()
                    .copied()
                    .filter(|other| other.id != g.id)
                    .map(|other| {
                        let (affinity, hostility) = score_for(other.id.0);
                        GangRelationSummary {
                            gang: other.id.0,
                            affinity,
                            hostility,
                        }
                    })
                    .collect();
                let history = self
                    .world
                    .gang_history()
                    .iter()
                    .filter(|entry| entry.gang == g.id)
                    .cloned()
                    .collect();
                GangSummary {
                    id: g.id.0,
                    name: g.name.clone(),
                    leader: g.leader,
                    members: g.members.clone(),
                    reputation: g.reputation,
                    resources: g.resources,
                    culture: g.culture.clone(),
                    formed_at: g.formed_at,
                    active: g.active,
                    hierarchy: g.hierarchy.clone(),
                    rivalries,
                    alliances,
                    internal_disputes: g.internal_disputes.clone(),
                    goals: g.goals.clone(),
                    pressure,
                    history,
                    relations,
                }
            })
            .collect()
    }

    /// Get all guards as summary types for the staff view.
    pub fn guards(&self) -> Vec<GuardSummary> {
        self.world
            .guards
            .iter()
            .map(|g| GuardSummary {
                id: g.id.0,
                name: g.name.clone(),
                rank: g.rank,
                post: g.post,
                authority: g.authority,
                shift: g.shift,
                current_task: g.current_task.clone(),
                competence: g.competence,
                experience: g.experience,
                morale: g.morale,
                fatigue: g.fatigue,
                stress: g.stress,
                loyalty: g.loyalty,
                integrity: g.integrity,
                personality: g.personality,
                credibility: g.credibility(),
                mistakes: g.mistake_count(),
                violations: g.violation_count(),
                tracked_opinions: g.opinion_count(),
                history: g.history.iter().rev().take(16).rev().cloned().collect(),
            })
            .collect()
    }

    /// Get the warden as summary for the warden view.
    pub fn warden(&self) -> WardenSummary {
        let events: Vec<&alc_events::WorldEvent> = self.world.events.recent().collect();
        let inbox = self
            .world
            .warden
            .inbox
            .iter()
            .filter_map(|id| events.iter().find(|event| event.id == *id))
            .filter_map(|event| guard_report_from_event(event))
            .take(50)
            .collect();
        let health = self.world.economy.health();
        WardenSummary {
            name: self.world.warden.name.clone(),
            philosophy: self.world.warden.philosophy.clone(),
            traits: self.world.warden.traits.clone(),
            attention: self.world.warden.attention,
            inbox,
            pending_actions: self.world.warden.pending_actions.clone(),
            last_decision_at: self.world.warden.last_decision_at,
            control: self.world.warden.control_panel_summary(health),
            recent_decisions: self
                .world
                .warden
                .audit_log
                .recent(16)
                .into_iter()
                .cloned()
                .collect(),
        }
    }

    /// Build the report-only decision context for warden strategy (0.05 §010).
    ///
    /// Every value comes from an observer DTO (warden summary, budget snapshot,
    /// construction queue, economy/guard/policy summaries). No prisoner,
    /// gang, or raw world-event internals are consulted here.
    pub fn warden_strategy_snapshot(&self) -> WardenStrategySnapshot {
        let summary = self.warden();
        let budget = self.budget_snapshot_window(30);
        let queue = self.construction_queue();
        let economy = self.economy();
        let guards = self.guards();
        let policy = self.policy_summary();

        let recent: Vec<&DecisionAuditRecord> = self
            .engine_state
            .decision_audit
            .iter()
            .rev()
            .take(20)
            .collect();
        let mut warnings = budget.warnings.clone();
        if policy.lockdown {
            warnings.push("lockdown limits routine activity".to_string());
        }
        if policy.intake_paused {
            warnings.push("intake paused by policy".to_string());
        }
        if recent
            .iter()
            .any(|r| r.reason.contains("warden funding request"))
        {
            warnings.push("recent warden funding request on ledger".to_string());
        }

        let events: Vec<&alc_events::WorldEvent> = self.world.events.recent().collect();
        let reports = self
            .world
            .warden
            .inbox
            .iter()
            .filter_map(|id| events.iter().find(|event| event.id == *id))
            .filter_map(|event| warden_report_signal_from_event(event))
            .take(50)
            .collect();

        let active: Vec<_> = queue
            .iter()
            .filter(|p| {
                matches!(
                    p.status,
                    alc_world::ConstructionStatus::Queued
                        | alc_world::ConstructionStatus::Approved
                        | alc_world::ConstructionStatus::InProgress
                )
            })
            .collect();
        let future_capacity: u32 = active.iter().map(|p| p.capacity).sum();

        WardenStrategySnapshot::new(
            budget.population,
            budget.capacity,
            economy.cash_major,
            budget.health as f64,
            budget.runway_days,
            summary.control.security_incidents_this_week,
            summary.attention,
            reports,
            active.len() as u32,
            future_capacity,
            guards.len() as u32,
            warnings,
        )
    }

    /// Decide from the report-only strategy context. The decision is returned
    /// without mutating the world; callers apply it through the engine API.
    pub fn decide_warden_strategy(&self) -> StrategyDecision {
        let snapshot = self.warden_strategy_snapshot();
        let seed = self.world.seed;
        let secs = self.world.time.as_secs();
        alc_warden::decide_strategy(
            &snapshot,
            &self.world.warden.traits,
            &mut alc_core::RngStream::new(seed ^ 0x5741_5245, secs),
        )
    }

    /// Get the economy summary for the financial view.
    pub fn economy(&self) -> EconomySummary {
        EconomySummary {
            cash_major: self.world.economy.cash.as_major(),
            health: self.world.economy.health(),
            funding_per_prisoner_per_day: self
                .world
                .economy
                .funding_per_prisoner_per_day
                .as_major(),
            crowding_threshold: self.world.economy.crowding_threshold,
            ledger: self.world.economy.ledger.clone(),
        }
    }

    /// Get social graph edges for the social network view.
    pub fn social_edges(&self) -> Vec<SocialEdge> {
        // Build edges from the social graph using iter()
        let mut edges = Vec::new();
        for ((a, b), edge) in self.world.social.iter() {
            edges.push(SocialEdge {
                prisoner_a: PrisonerId(a),
                prisoner_b: PrisonerId(b),
                friendship: edge.friendship,
                trust: edge.trust,
                fear: edge.fear,
                hostility: edge.hostility,
                rivalry: edge.rivalry,
                bond_score: (edge.friendship + edge.trust) / 2.0,
                threat_score: (edge.hostility + edge.rivalry + edge.fear) / 3.0,
            });
        }
        edges
    }

    /// Get detected emergent social roles for the Social Network view.
    ///
    /// Recomputed from the authoritative social graph and gang roster. Rows
    /// are ordered by prisoner id then role label, and every strength is
    /// clamped to `0.0..=1.0`.
    pub fn social_roles(&self) -> Vec<SocialRoleSummary> {
        let prisoner_ids: Vec<PrisonerId> = self.world.prisoners.iter().map(|p| p.id).collect();
        let detected =
            alc_social::detect_roles(&self.world.social, &prisoner_ids, &self.world.gangs);

        let mut rows: Vec<SocialRoleSummary> = detected
            .into_iter()
            .flat_map(|(prisoner, roles)| {
                roles
                    .into_iter()
                    .map(move |(role, strength)| SocialRoleSummary {
                        prisoner,
                        role: format!("{role:?}"),
                        strength: strength.clamp(0.0, 1.0),
                    })
            })
            .collect();

        // Dyad classifiers are part of the social-network analysis surface too:
        // expose them beside the aggregate roles so the observer can see who
        // is a friend, rival, or enemy of whom without reading raw dyads.
        let denom = prisoner_ids.len().saturating_sub(1).max(1) as f32;
        for &prisoner in &prisoner_ids {
            let graph = &self.world.social;
            for (label, matches) in [
                (
                    "Friend",
                    alc_social::friends_of(graph, prisoner, &prisoner_ids),
                ),
                (
                    "Rival",
                    alc_social::rivals_of(graph, prisoner, &prisoner_ids),
                ),
                (
                    "Enemy",
                    alc_social::enemies_of(graph, prisoner, &prisoner_ids),
                ),
            ] {
                if matches.is_empty() {
                    continue;
                }
                rows.push(SocialRoleSummary {
                    prisoner,
                    role: label.to_string(),
                    strength: (matches.len() as f32 / denom).clamp(0.0, 1.0),
                });
            }
        }

        rows.sort_by(|a, b| {
            a.prisoner
                .raw()
                .cmp(&b.prisoner.raw())
                .then_with(|| a.role.cmp(&b.role))
        });
        rows
    }

    /// Get facilities for the map view.
    pub fn facilities(&self) -> Vec<FacilitySummary> {
        self.world
            .facilities
            .iter()
            .map(|f| FacilitySummary {
                id: f.id.0,
                kind: f.kind,
                wing: 0, // Would need to derive from location
                capacity: f.capacity,
                condition: f.condition,
                staff_required: 0, // Would need to calculate
            })
            .collect()
    }

    /// Construction queue for observer UI (0.05).
    ///
    /// Backed by `World::construction_queue`. Terminal projects are retained for
    /// audit and pruned only after 365 simulated days; completed facilities also
    /// appear in [`Sim::facilities`].
    pub fn construction_queue(&self) -> Vec<ConstructionSummary> {
        self.world
            .construction_queue
            .iter()
            .map(|p| ConstructionSummary {
                id: p.id,
                kind: p.kind,
                wing: p.wing,
                capacity: p.capacity,
                total_days: p.total_days,
                progress_days: p.progress_days,
                cost: p.cost,
                started_day: p.started_day,
                status: p.status,
                terminal_reason: p.terminal_reason.clone(),
                staff_allocated: p.staff_allocated,
                materials_ready: p.materials_ready,
                prereqs_met: p.prereqs_met,
            })
            .collect()
    }

    /// Queue a construction project through the world API (observer/control, 0.05).
    ///
    /// Returns the project id. Emits construction start through the world and
    /// records a decision-audit entry.
    pub fn enqueue_construction(
        &mut self,
        kind: alc_world::FacilityKind,
        wing: u32,
        capacity: u32,
    ) -> u64 {
        let id = self.world.enqueue_construction(kind, wing, capacity);
        self.audit_control(
            "warden/control",
            "EnqueueConstruction",
            format!(
                "Queued construction {:?} wing={} cap={} (project {})",
                kind, wing, capacity, id
            ),
            "accepted",
            "delegated to World::enqueue_construction",
        );
        id
    }

    /// Approve a requested construction project (observer/control, 0.05).
    ///
    /// Validates the request through `World::approve_construction` and records
    /// both a decision-audit entry and the world's `Expansion` approval event.
    pub fn approve_construction(&mut self, project_id: u64, reason: impl Into<String>) -> bool {
        let reason = reason.into();
        let before = self.world.events.recent().map(|e| e.id).max().unwrap_or(0);
        let ok = self.world.approve_construction(project_id, reason.clone());
        self.pump_events_into_engine_log(before);
        self.audit_control(
            "observer/control",
            "ApproveConstruction",
            format!(
                "Construction project {project_id} {}",
                if ok { "approved" } else { "not approvable" }
            ),
            if ok { "accepted" } else { "rejected" },
            if ok {
                reason
            } else {
                "unknown or non-requested construction project".to_string()
            },
        );
        ok
    }

    /// Advance a queued construction project by `days` (observer/control, 0.05).
    ///
    /// Delegates to the authoritative `World::advance_construction` so manual
    /// work uses the same completion effects as the daily build step (add
    /// facility + `ConstructionComplete` event). Returns `true` if the project
    /// existed and was advanceable.
    pub fn advance_construction(&mut self, project_id: u64, days: u32) -> bool {
        if days == 0 {
            self.audit_control_rejected(
                "observer/control",
                "AdvanceConstruction",
                format!("Construction project {project_id} advance ignored"),
                "days must be > 0",
            );
            return false;
        }
        let before = self.world.events.recent().map(|e| e.id).max().unwrap_or(0);
        let proj = self
            .world
            .construction_queue
            .iter()
            .find(|p| p.id == project_id)
            .cloned();
        let Some(proj) = proj else {
            self.audit_control_rejected(
                "observer/control",
                "AdvanceConstruction",
                format!("Construction project {project_id} not found"),
                "unknown construction project",
            );
            return false;
        };
        let prev_progress = proj.progress_days;
        let total = proj.total_days;
        let ok = self.world.advance_construction(project_id, days);
        self.pump_events_into_engine_log(before);
        let after_progress = self
            .world
            .construction_queue
            .iter()
            .find(|p| p.id == project_id)
            .map(|p| p.progress_days)
            .unwrap_or(total);
        let completed = self
            .world
            .construction_queue
            .iter()
            .find(|p| p.id == project_id)
            .is_some_and(|p| p.status == alc_world::ConstructionStatus::Completed);
        self.audit_control(
            "observer/control",
            "AdvanceConstruction",
            format!("Construction project {project_id} advanced {prev_progress}->{after_progress}/{total}{}", if completed { " (completed)" } else { "" }),
            if ok { "accepted" } else { "rejected" },
            if ok { "world construction lifecycle" } else { "project not advanceable" },
        );
        ok
    }

    /// Cancel a queued construction project (observer/control, 0.05).
    pub fn cancel_construction(&mut self, project_id: u64, reason: impl Into<String>) -> bool {
        let reason = reason.into();
        let before = self.world.events.recent().map(|e| e.id).max().unwrap_or(0);
        let ok = self.world.cancel_construction(project_id, reason.clone());
        self.pump_events_into_engine_log(before);
        self.audit_control(
            "observer/control",
            "CancelConstruction",
            format!(
                "Construction project {project_id} {}",
                if ok { "cancelled" } else { "not cancelled" }
            ),
            if ok { "accepted" } else { "rejected" },
            if ok {
                reason
            } else {
                "unknown or terminal construction project".to_string()
            },
        );
        ok
    }

    /// Mark a queued construction project as failed (observer/control, 0.05).
    pub fn fail_construction(&mut self, project_id: u64, reason: impl Into<String>) -> bool {
        let reason = reason.into();
        let before = self.world.events.recent().map(|e| e.id).max().unwrap_or(0);
        let ok = self.world.fail_construction(project_id, reason.clone());
        self.pump_events_into_engine_log(before);
        self.audit_control(
            "observer/control",
            "FailConstruction",
            format!(
                "Construction project {project_id} {}",
                if ok { "failed" } else { "not failed" }
            ),
            if ok { "accepted" } else { "rejected" },
            if ok {
                reason
            } else {
                "unknown or terminal construction project".to_string()
            },
        );
        ok
    }

    // ---------------------------------------------------------------------
    // 0.05 observer/control DTOs
    // ---------------------------------------------------------------------

    /// Post-release records for every prisoner tracked in the engine ledger.
    ///
    /// Built from `EngineState::post_release` (persisted) plus the release
    /// events still present on the bounded world event bus. Release type and
    /// release time come from the latest matching event; if the event has aged
    /// out of the bus, `release_type` is `"unknown"` and `released_at` is
    /// `None` rather than fabricated.
    pub fn post_release_records(&self) -> Vec<PostReleaseSummary> {
        let mut rows: Vec<PostReleaseSummary> = self
            .engine_state
            .post_release
            .iter()
            .map(|(&id, &success)| {
                let info = self.release_event_info(id);
                let (release_type, released_at, recidivism_risk, has_plan) = match info {
                    Some(info) => (
                        info.release_type,
                        Some(info.at),
                        info.recidivism_risk,
                        info.has_reintegration_plan,
                    ),
                    None => ("unknown".to_string(), None, None, None),
                };
                PostReleaseSummary {
                    prisoner_id: id,
                    prisoner_name: self
                        .world
                        .prisoners
                        .iter()
                        .find(|p| p.id == id)
                        .map(|p| p.name.clone()),
                    release_type,
                    released_at,
                    successful_reintegration: success,
                    recidivism_risk,
                    has_reintegration_plan: has_plan,
                    follow_up_required: !success,
                }
            })
            .collect();
        rows.sort_by_key(|r| {
            (
                r.released_at.map(|t| t.secs).unwrap_or(u64::MAX),
                r.prisoner_id.raw(),
            )
        });
        rows
    }

    /// Latest release-shaped event for a prisoner, if the event bus still has it.
    fn release_event_info(&self, prisoner_id: PrisonerId) -> Option<ReleaseEventInfo> {
        let mut best: Option<ReleaseEventInfo> = None;
        for event in self.world.events.recent() {
            let release_type = match event.kind {
                EventKind::PostRelease => "post_release",
                EventKind::Release => "release",
                EventKind::ParoleGranted => "parole",
                EventKind::EscapeSucceeded => "escape",
                EventKind::TransferOut => "transfer",
                _ => continue,
            };
            if !event.actors.contains(&prisoner_id.raw()) {
                continue;
            }
            let info = ReleaseEventInfo {
                release_type: release_type.to_string(),
                at: event.at,
                recidivism_risk: parse_f32_after(&event.causes, "risk "),
                has_reintegration_plan: parse_plan_flag(&event.causes),
            };
            if best.as_ref().map(|b| b.at <= info.at).unwrap_or(true) {
                best = Some(info);
            }
        }
        best
    }

    /// Rehab enrollment rows derived from prisoner skill state (0.05).
    ///
    /// `alc-social` records enrollment as a `rehab_*` skill key on the prisoner
    /// whose value is program progress 0..1, so no new world state is needed.
    /// Completed programs remove the key and therefore drop out of this list
    /// (completion counts live in [`Sim::rehab_overview`]).
    pub fn rehab_summaries(&self) -> Vec<RehabEnrollmentSummary> {
        let mut rows: Vec<RehabEnrollmentSummary> = Vec::new();
        for p in &self.world.prisoners {
            for program in REHAB_PROGRAMS {
                let Some(progress) = p.skills.get(program.key()) else {
                    continue;
                };
                rows.push(RehabEnrollmentSummary {
                    prisoner_id: p.id,
                    prisoner_name: p.name.clone(),
                    program: format!("{program:?}"),
                    program_key: program.key().to_string(),
                    progress: progress.clamp(0.0, 1.0),
                    education_track: matches!(
                        program,
                        RehabProgram::Education | RehabProgram::Vocational
                    ),
                    disciplinary: p.schedule.disciplinary_status,
                });
            }
        }
        rows.sort_by_key(|r| (r.prisoner_id.raw(), r.program_key.clone()));
        rows
    }

    /// Static rehab program catalog for enrollment controls (0.05).
    pub fn rehab_program_catalog(&self) -> Vec<RehabProgramInfo> {
        REHAB_PROGRAMS
            .iter()
            .map(|program| RehabProgramInfo {
                name: format!("{program:?}"),
                key: program.key().to_string(),
                difficulty: program.difficulty(),
            })
            .collect()
    }

    /// Aggregate rehab overview: enrollments, completions, dropouts by program.
    ///
    /// Completion/dropout counts are event-derived (bounded world bus), so they
    /// cover roughly the retained event window rather than all time.
    pub fn rehab_overview(&self) -> RehabOverview {
        let enrollments = self.rehab_summaries();
        let mut enrolled_prisoners: Vec<PrisonerId> = Vec::new();
        let mut progress_sum = 0.0_f32;
        let mut rollups: Vec<RehabProgramRollup> = REHAB_PROGRAMS
            .iter()
            .map(|program| RehabProgramRollup {
                program: format!("{program:?}"),
                key: program.key().to_string(),
                enrolled: 0,
                average_progress: 0.0,
            })
            .collect();
        for row in &enrollments {
            if !enrolled_prisoners.contains(&row.prisoner_id) {
                enrolled_prisoners.push(row.prisoner_id);
            }
            progress_sum += row.progress;
            if let Some(slot) = rollups.iter_mut().find(|r| r.key == row.program_key) {
                slot.enrolled += 1;
                slot.average_progress += row.progress;
            }
        }
        for slot in &mut rollups {
            if slot.enrolled > 0 {
                slot.average_progress /= slot.enrolled as f32;
            }
        }

        let mut completions = 0u32;
        let mut dropouts = 0u32;
        for event in self.world.events.recent() {
            match event.kind {
                EventKind::RehabilitationSuccess => completions += 1,
                EventKind::RehabilitationFailure => dropouts += 1,
                EventKind::ProgramEnrolled if event.summary.contains("dropped") => dropouts += 1,
                _ => {}
            }
        }

        RehabOverview {
            enrollments: enrollments.len() as u32,
            enrolled_prisoners: enrolled_prisoners.len() as u32,
            average_progress: if enrollments.is_empty() {
                0.0
            } else {
                progress_sum / enrollments.len() as f32
            },
            completions,
            dropouts,
            programs: rollups,
        }
    }

    /// Contraband cases from the world-owned case store (0.05).
    ///
    /// Reads `World::contraband_cases` directly; summaries never parse event
    /// text. Standalone legacy `ContrabandFound` events are ingested once by
    /// [`Self::emit_event`] or during [`Self::load_with_config`], after which
    /// case ids, item, holder, guard, severity and summary are authoritative
    /// case fields. Resolution merges the world store with the persisted
    /// engine mirror. Rows stay sorted by case id.
    pub fn contraband_summaries(&self) -> Vec<ContrabandCaseSummary> {
        let mut cases: Vec<ContrabandCaseSummary> = self
            .world
            .contraband_cases
            .iter()
            .map(|case| {
                let mut causes = vec![format!(
                    "discovery by guard {}",
                    case.discovering_guard.raw()
                )];
                causes.push(format!("disposition: {:?}", case.disposition));
                if let Some(action) = &case.disciplinary_action {
                    causes.push(format!("disciplinary action: {action:?}"));
                }
                let resolved_at = self
                    .world
                    .resolved_contraband_at
                    .get(&case.case_id)
                    .copied()
                    .or_else(|| {
                        self.engine_state
                            .resolved_contraband_at
                            .get(&case.case_id)
                            .copied()
                    });
                ContrabandCaseSummary {
                    case_id: case.case_id,
                    at: case.discovered_at,
                    item: Some(format!("{:?}", case.item)),
                    holder: Some(case.holder),
                    guard: Some(case.discovering_guard.raw()),
                    severity: case.severity,
                    summary: case.summary.clone(),
                    causes,
                    resolved: self
                        .engine_state
                        .resolved_contraband_cases
                        .contains(&case.case_id)
                        || self
                            .world
                            .resolved_contraband_at
                            .contains_key(&case.case_id),
                    resolved_at,
                }
            })
            .collect();
        cases.sort_by_key(|c| c.case_id);
        cases
    }

    /// Mark a known contraband case resolved (observer/control, 0.05).
    ///
    /// Validated against the world case store: unknown ids are rejected and
    /// leave both resolution maps untouched. Returns `true` only when the case
    /// was newly resolved. The world store is authoritative; the engine sets
    /// are kept as a save-compatible mirror.
    pub fn resolve_contraband_case(&mut self, case_id: u64) -> bool {
        if !self
            .world
            .contraband_cases
            .iter()
            .any(|case| case.case_id == case_id)
        {
            self.audit_control_rejected(
                "observer/control",
                "ResolveContrabandCase",
                format!("Contraband case {case_id} not found"),
                "unknown contraband case id",
            );
            return false;
        }
        let at = self.world.time;
        let already_resolved = self
            .engine_state
            .resolved_contraband_cases
            .contains(&case_id)
            || self.world.resolved_contraband_at.contains_key(&case_id);
        if already_resolved {
            // Heal either side if only one copy recorded the resolution: the
            // world store is authoritative, but older saves may carry only the
            // engine mirror. Use the earliest known timestamp via `or_insert`.
            let resolved_at = self
                .world
                .resolved_contraband_at
                .get(&case_id)
                .copied()
                .or_else(|| {
                    self.engine_state
                        .resolved_contraband_at
                        .get(&case_id)
                        .copied()
                })
                .unwrap_or(at);
            self.engine_state.resolved_contraband_cases.insert(case_id);
            self.engine_state
                .resolved_contraband_at
                .entry(case_id)
                .or_insert(resolved_at);
            self.world.resolve_contraband_case(case_id, resolved_at);
            self.audit_control(
                "observer/control",
                "ResolveContrabandCase",
                format!("Contraband case {case_id} already resolved"),
                "rejected",
                "case already resolved",
            );
            return false;
        }
        self.engine_state.resolved_contraband_cases.insert(case_id);
        self.engine_state.resolved_contraband_at.insert(case_id, at);
        self.world.resolve_contraband_case(case_id, at);
        self.audit_control(
            "observer/control",
            "ResolveContrabandCase",
            format!("Contraband case {case_id} marked resolved"),
            "accepted",
            "contraband case resolution",
        );
        true
    }

    /// Contraband inventory rollup by item category (0.05).
    ///
    /// Aggregates [`Sim::contraband_summaries`] from the world case store.
    pub fn contraband_inventory(&self) -> Vec<ContrabandInventorySummary> {
        let mut map: BTreeMap<String, ContrabandInventorySummary> = BTreeMap::new();
        for case in self.contraband_summaries() {
            let label = case.item.clone().unwrap_or_else(|| "Unknown".to_string());
            let slot = map
                .entry(label.clone())
                .or_insert_with(|| ContrabandInventorySummary {
                    item: label,
                    cases: 0,
                    holders: Vec::new(),
                    last_seen: None,
                    max_severity: 0.0,
                });
            slot.cases += 1;
            if let Some(holder) = case.holder {
                if !slot.holders.contains(&holder) {
                    slot.holders.push(holder);
                }
            }
            slot.last_seen = Some(match slot.last_seen {
                Some(prev) if prev >= case.at => prev,
                _ => case.at,
            });
            slot.max_severity = slot.max_severity.max(case.severity);
        }
        map.into_values().collect()
    }

    /// Resolved daily schedule timeline for one prisoner (0.05).
    ///
    /// Recomputes `resolve_schedule` from the current policy/staffing (the
    /// world itself only persists the *current* activity, so this is the
    /// documented source of truth for the whole day). Attendance outcomes are
    /// not persisted per block: `ScheduleBlockView::attendance` is `None` and
    /// `attendance_probability` is the deterministic probability model the
    /// world uses — TODO(world-state): store per-block attendance history.
    pub fn schedule_timeline(&self, prisoner_id: PrisonerId) -> Option<ScheduleTimeline> {
        let prisoner = self.world.prisoners.iter().find(|p| p.id == prisoner_id)?;
        let policy = self.schedule_policy_snapshot();
        let template = ScheduleTemplate::default_medium();
        let resolved = resolve_schedule(prisoner, &template, &policy);
        let minute = self.world.time.minute_of_day() as u16;
        let current_block_id = current_block(&resolved, minute).map(|rb| rb.block.id);

        let blocks: Vec<ScheduleBlockView> = resolved
            .iter()
            .map(|rb| ScheduleBlockView {
                block_id: rb.block.id,
                activity: format!("{:?}", rb.block.activity),
                label: rb.block.label.clone(),
                start_minute: rb.start_minute,
                end_minute: rb.end_minute,
                cancelled: rb.cancelled,
                reason: rb.reason.map(|r| format!("{r:?}")),
                mandatory: rb.block.attendance.mandatory,
                staff_required: rb.block.staff_requirement,
                current: current_block_id == Some(rb.block.id),
                // TODO(world-state): attendance outcomes are not persisted.
                attendance: None,
            })
            .collect();

        Some(ScheduleTimeline {
            prisoner_id,
            prisoner_name: prisoner.name.clone(),
            day: self.world.days_simulated,
            current_minute: minute,
            current_activity: self
                .world
                .current_activity
                .get(&prisoner_id)
                .map(|a| format!("{a:?}")),
            lockdown: policy.lockdown,
            staff_coverage: policy.staff_coverage,
            attendance_probability: attendance_probability(&AttendanceContext::from_prisoner(
                prisoner,
            )),
            cancelled_blocks: blocks.iter().filter(|b| b.cancelled).count() as u32,
            blocks,
        })
    }

    /// Policy/staffing context for schedule resolution (mirrors the world's
    /// private `schedule_policy` / `staff_coverage` so the observer timeline
    /// matches what the simulation actually resolved).
    fn schedule_policy_snapshot(&self) -> SchedulePolicy {
        SchedulePolicy {
            lockdown: self.world.policies.lockdown,
            privileges_level: self.world.policies.privileges_level.max(0.05),
            security_level: self.world.policies.security_level,
            staff_coverage: self.staff_coverage_estimate(),
            weather_ok: true,
            rehab_funding_multiplier: self.world.policies.rehab_funding_multiplier,
            allow_refusal: true,
        }
    }

    /// Staff coverage estimate (guards / required facility posts), clamped 0..1.
    fn staff_coverage_estimate(&self) -> f32 {
        let required: u32 = self.world.facilities.iter().map(|f| f.staff_required).sum();
        if required == 0 {
            1.0
        } else {
            (self.world.guards.len() as f32 / required as f32).min(1.0)
        }
    }

    /// Budget snapshot over the trailing 30 days (0.05).
    pub fn budget_snapshot(&self) -> BudgetSnapshot {
        self.budget_snapshot_window(30)
    }

    /// Budget snapshot over `window_days` of ledger history (0.05).
    ///
    /// Aggregates the persisted `alc-economy` ledger by account, derives
    /// daily revenue/expense projections and a cash runway, and explains the
    /// financial health index with explicit warnings.
    pub fn budget_snapshot_window(&self, window_days: u64) -> BudgetSnapshot {
        let window_days = window_days.max(1);
        let cutoff = self
            .world
            .time
            .as_secs()
            .saturating_sub(window_days.saturating_mul(86_400));

        let mut by_account: BTreeMap<String, (f64, f64, u32)> = BTreeMap::new();
        for entry in &self.world.economy.ledger {
            if entry.at_secs < cutoff {
                continue;
            }
            let slot = by_account
                .entry(format!("{:?}", entry.account))
                .or_insert((0.0, 0.0, 0));
            let major = entry.amount.as_major();
            if major >= 0.0 {
                slot.0 += major;
            } else {
                slot.1 += -major;
            }
            slot.2 += 1;
        }
        let lines: Vec<BudgetLine> = by_account
            .into_iter()
            .map(|(account, (inflow, outflow, entries))| BudgetLine {
                account,
                inflow_major: inflow,
                outflow_major: outflow,
                net_major: inflow - outflow,
                entries,
            })
            .collect();

        let inflow_total: f64 = lines.iter().map(|l| l.inflow_major).sum();
        let outflow_total: f64 = lines.iter().map(|l| l.outflow_major).sum();
        let span = (window_days as f64)
            .min(self.world.days_simulated.max(1) as f64)
            .max(1.0);
        let trailing_daily_expense_avg =
            self.world.economy.trailing_daily_expense_avg() as f64 / 100.0;
        let projected_daily_revenue = if inflow_total > 0.0 {
            inflow_total / span
        } else {
            0.0
        };
        let projected_daily_expense = if outflow_total > 0.0 {
            outflow_total / span
        } else {
            trailing_daily_expense_avg
        };
        let cash_major = self.world.economy.cash.as_major();
        let runway_days = if cash_major <= 0.0 {
            0.0
        } else if projected_daily_expense <= 0.0 {
            9_999.0
        } else {
            cash_major / projected_daily_expense
        };

        let mut warnings = Vec::new();
        if cash_major < 0.0 {
            warnings.push("cash is negative (institution in deficit)".to_string());
        }
        if self.world.economy.health() < 0.25 {
            warnings.push("financial health below 25% of one month of expenses".to_string());
        }
        if projected_daily_expense > projected_daily_revenue {
            warnings.push(format!(
                "structural deficit: daily expenses ${:.0} exceed revenue ${:.0}",
                projected_daily_expense, projected_daily_revenue
            ));
        }
        let payroll = lines
            .iter()
            .find(|l| l.account == "Salaries")
            .map(|l| l.outflow_major)
            .unwrap_or(0.0);
        if outflow_total > 0.0 && payroll / outflow_total > 0.6 {
            warnings.push(format!(
                "payroll is {:.0}% of expenses",
                payroll / outflow_total * 100.0
            ));
        }
        if self.world.population() > self.world.economy.crowding_threshold {
            warnings.push(format!(
                "population {} above crowding threshold {}",
                self.world.population(),
                self.world.economy.crowding_threshold
            ));
        }
        let projects = self.world.construction_queue.len();
        if projects > 0 {
            warnings.push(format!("{projects} construction project(s) in progress"));
        }
        if self.world.policies.intake_paused {
            warnings.push("intake is paused by policy".to_string());
        }

        BudgetSnapshot {
            day: self.world.days_simulated,
            at: self.world.time,
            window_days,
            cash_major,
            health: self.world.economy.health() as f32,
            funding_per_prisoner_per_day: self
                .world
                .economy
                .funding_per_prisoner_per_day
                .as_major(),
            base_daily_grant: self.world.economy.base_daily_grant.as_major(),
            projected_daily_revenue,
            projected_daily_expense,
            trailing_daily_expense_avg,
            runway_days,
            population: self.world.population(),
            capacity: self.world.capacity(),
            crowding_threshold: self.world.economy.crowding_threshold,
            lines,
            warnings,
        }
    }

    /// Structured incident records matching `filter` (0.05).
    ///
    /// Reads the persisted sidecar incident store; filters are applied by
    /// [`incidents::IncidentFilter`] so UI and CLI share one query semantic.
    pub fn incident_records_filtered(
        &self,
        filter: &incidents::IncidentFilter,
    ) -> Vec<IncidentRecord> {
        incidents::filter_incidents(&self.engine_state.incident_sidecar, filter)
            .iter()
            .map(IncidentRecord::from)
            .collect()
    }

    /// Raw structured incidents matching `filter` (0.05).
    pub fn incidents_filtered(
        &self,
        filter: &incidents::IncidentFilter,
    ) -> Vec<incidents::Incident> {
        incidents::filter_incidents(&self.engine_state.incident_sidecar, filter)
    }

    // --- 0.05 validated observer controls ---

    /// Enroll a prisoner in a rehabilitation program (observer control, 0.05).
    ///
    /// Validated: returns `false` (and emits nothing) when the prisoner does
    /// not exist, is already enrolled in that program, or the program's
    /// willingness/eligibility gate rejects enrollment. On success the
    /// enrollment and its immediate purpose/hope lift are written back to the
    /// prisoner and a `ProgramEnrolled` event is emitted.
    pub fn enroll_rehab(&mut self, prisoner_id: PrisonerId, program: RehabProgram) -> bool {
        let at = self.world.time;
        let Some(prisoner) = self
            .world
            .prisoners
            .iter_mut()
            .find(|p| p.id == prisoner_id)
        else {
            return false;
        };
        if prisoner.skills.contains_key(program.key()) {
            return false;
        }
        let mut state = rehab_state_for(prisoner);
        if !enroll_in_program(&mut state, program) {
            return false;
        }
        write_back_rehab_state(prisoner, state);
        let name = prisoner.name.clone();
        let day = self.world.days_simulated;
        let recs = self.world.rehab_records.entry(prisoner_id).or_default();
        if !recs
            .iter()
            .any(|r| r.program == program && !r.completed && !r.dropped)
        {
            recs.push(alc_world::RehabProgramRecord {
                program,
                enrolled_day: day,
                progress_days: 0,
                progress_sum: 0.0,
                completed: false,
                dropped: false,
                attendance_days: 0,
                last_attended_day: None,
                failure_reason: None,
            });
        }
        let event = WorldEvent::new(
            0,
            at,
            EventKind::ProgramEnrolled,
            format!("{name} enrolled in {program:?} (observer control)"),
        )
        .with_actor(prisoner_id.raw())
        .with_cause("manual rehabilitation enrollment");
        self.emit_event(event);
        self.audit_control(
            "observer/control",
            "EnrollRehab",
            format!("Prisoner {} enrolled in {:?}", prisoner_id.raw(), program),
            "accepted",
            "rehab enrollment control",
        );
        true
    }

    /// Enroll by program key (UI/control convenience).
    pub fn enroll_rehab_by_key(&mut self, prisoner_id: PrisonerId, key: &str) -> bool {
        let Some(program) = REHAB_PROGRAMS.iter().copied().find(|p| p.key() == key) else {
            return false;
        };
        self.enroll_rehab(prisoner_id, program)
    }

    /// Drop a rehab program by key (UI/control convenience).
    pub fn drop_rehab_by_key(&mut self, prisoner_id: PrisonerId, key: &str) -> bool {
        let Some(program) = REHAB_PROGRAMS.iter().copied().find(|p| p.key() == key) else {
            return false;
        };
        self.drop_rehab(prisoner_id, program)
    }

    /// Remove a prisoner from a rehabilitation program (observer control, 0.05).
    ///
    /// Validated: returns `false` when the prisoner does not exist or is not
    /// enrolled. On success emits a `RehabilitationFailure` event with the
    /// manual-removal cause.
    pub fn drop_rehab(&mut self, prisoner_id: PrisonerId, program: RehabProgram) -> bool {
        let at = self.world.time;
        let Some(prisoner) = self
            .world
            .prisoners
            .iter_mut()
            .find(|p| p.id == prisoner_id)
        else {
            return false;
        };
        if prisoner.skills.remove(program.key()).is_none() {
            return false;
        }
        let name = prisoner.name.clone();
        if let Some(recs) = self.world.rehab_records.get_mut(&prisoner_id) {
            if let Some(rec) = recs
                .iter_mut()
                .rev()
                .find(|r| r.program == program && !r.completed && !r.dropped)
            {
                rec.dropped = true;
            }
        }
        let event = WorldEvent::new(
            0,
            at,
            EventKind::RehabilitationFailure,
            format!("{name} removed from {program:?} (observer control)"),
        )
        .with_actor(prisoner_id.raw())
        .with_cause("manual removal by observer");
        self.emit_event(event);
        self.audit_control(
            "observer/control",
            "DropRehab",
            format!("Prisoner {} removed from {:?}", prisoner_id.raw(), program),
            "accepted",
            "rehab removal control",
        );
        true
    }

    /// Mark a structured sidecar incident resolved (observer/control, 0.05).
    ///
    /// Validated: returns `false` for an unknown id or an already-resolved
    /// incident. Resolution persists with the sidecar incident, emits an
    /// `Investigation` event, and records a decision-audit entry.
    pub fn resolve_incident(&mut self, incident_id: u64) -> bool {
        let at = self.world.time;
        let (kind, already_resolved) = {
            let Some(incident) = self
                .engine_state
                .incident_sidecar
                .iter()
                .find(|inc| inc.id == incident_id)
            else {
                self.audit_control(
                    "observer/control",
                    "ResolveIncident",
                    format!("Incident {incident_id} not found"),
                    "rejected",
                    "unknown incident id",
                );
                return false;
            };
            (incident.kind, incident.resolved)
        };
        if already_resolved {
            self.audit_control(
                "observer/control",
                "ResolveIncident",
                format!("Incident {incident_id} ({kind:?}) already resolved"),
                "rejected",
                "incident already resolved",
            );
            return false;
        }
        if let Some(incident) = self
            .engine_state
            .incident_sidecar
            .iter_mut()
            .find(|inc| inc.id == incident_id)
        {
            incident.resolved = true;
        }
        let event = WorldEvent::new(
            0,
            at,
            EventKind::Investigation,
            format!("Incident {incident_id} ({kind:?}) marked resolved"),
        )
        .with_cause("incident resolution by observer");
        self.emit_event(event);
        self.audit_control(
            "observer/control",
            "ResolveIncident",
            format!("Incident {incident_id} ({kind:?}) marked resolved"),
            "accepted",
            "structured sidecar incident resolved by observer/control",
        );
        true
    }

    /// Record a post-release outcome (observer/engine control, 0.05).
    ///
    /// Stores the reintegration result in the engine ledger (persisted) and
    /// emits a `PostRelease` event carrying the recidivism risk and whether a
    /// reintegration plan existed, so [`Sim::post_release_records`] can report
    /// both. Returns `true` once recorded.
    pub fn record_post_release(
        &mut self,
        prisoner_id: PrisonerId,
        release_type: &str,
        recidivism_risk: f32,
        has_reintegration_plan: bool,
    ) -> bool {
        let at = self.world.time;
        let risk = recidivism_risk.clamp(0.0, 1.0);
        let success = has_reintegration_plan && risk < 0.5;
        self.engine_state.post_release.insert(prisoner_id, success);
        let event = WorldEvent::new(
            0,
            at,
            EventKind::PostRelease,
            format!(
                "Post-release: prisoner {} ({release_type}) risk {risk:.2}{}",
                prisoner_id.raw(),
                if has_reintegration_plan {
                    ", plan in place"
                } else {
                    ", no plan"
                }
            ),
        )
        .with_actor(prisoner_id.raw())
        .with_cause(format!("recidivism risk {risk:.2}"))
        .with_cause(format!(
            "reintegration plan {}",
            if has_reintegration_plan { "yes" } else { "no" }
        ));
        self.emit_event(event);
        self.audit_control(
            "observer/control",
            "RecordPostRelease",
            format!(
                "Prisoner {} post-release recorded ({} risk {:.2}, plan {})",
                prisoner_id.raw(),
                release_type,
                risk,
                if has_reintegration_plan { "yes" } else { "no" }
            ),
            "accepted",
            "post-release outcome recorded in engine ledger",
        );
        true
    }

    /// Compute the average population since a given time.
    fn avg_population_since(&self, at: SimTime) -> f64 {
        let pts: Vec<&(SimTime, u32)> = self
            .population_history()
            .iter()
            .filter(|(t, _)| *t >= at)
            .collect();
        if pts.is_empty() {
            f64::from(self.world.population())
        } else {
            pts.iter().map(|(_, p)| f64::from(*p)).sum::<f64>() / pts.len() as f64
        }
    }

    /// Save the world via alc-persistence (includes EngineState).
    pub fn save(&self, dir: impl AsRef<Path>) -> Result<(), SaveError> {
        let manager = SaveManager::new(dir);
        // Convert sumo's EngineState to alc_persistence's EngineState
        let persist_state = self.engine_state.to_persistence();
        manager.save_with_engine(&self.world, Some(persist_state))
    }

    /// Load a saved world with full EngineState restoration.
    pub fn load(dir: impl AsRef<Path>) -> Result<Sim, SaveError> {
        Self::load_with_config(dir, &AiConfig::default())
    }

    /// Load a saved world and re-apply runtime-only AI configuration.
    pub fn load_with_config(dir: impl AsRef<Path>, ai: &AiConfig) -> Result<Sim, SaveError> {
        let dir = dir.as_ref().to_path_buf();
        let manager = SaveManager::new(&dir);
        let (mut world, engine_state) = manager.load_with_engine()?;
        // Runtime provider/cache/config state is deliberately skipped by the
        // v5 save format. Re-apply the startup policy after deserialization.
        world.configure_ai(ai);

        let speed = world.speed;
        let mut sim = Sim {
            world,
            speed,
            engine_state: engine_state
                .map(|es| EngineState::from_persistence(es, &dir))
                .unwrap_or_default(),
            last_autosave_error: None,
        };
        sim.migrate_legacy_contraband_on_load();
        for guard in &mut sim.world.guards {
            guard.restore_legacy_counters_from_history();
        }
        Ok(sim)
    }

    /// Enable autosave: write the world every `interval_days` simulated days.
    pub fn enable_autosave(&mut self, dir: impl Into<PathBuf>, interval_days: u64) {
        let dir = dir.into();
        self.engine_state.autosave = Some(AutosaveScheduler::new(dir.clone(), interval_days));
        self.audit_control(
            "observer/control",
            "EnableAutosave",
            format!(
                "Autosave enabled every {} day(s) at {}",
                interval_days,
                dir.display()
            ),
            "accepted",
            "autosave scheduler configured",
        );
    }

    /// Disable autosave.
    pub fn disable_autosave(&mut self) {
        if self.engine_state.autosave.is_none() {
            return;
        }
        self.engine_state.autosave = None;
        self.audit_control(
            "observer/control",
            "DisableAutosave",
            "Autosave disabled".to_string(),
            "accepted",
            "autosave scheduler cleared",
        );
    }

    /// Last autosave/save error surfaced by tick or manual autosave.
    pub fn last_autosave_error(&self) -> Option<&str> {
        self.last_autosave_error.as_deref()
    }

    /// Save when the autosave interval has elapsed. Returns true when written.
    ///
    /// Failures are always recorded in [`Self::last_autosave_error`] and emitted
    /// through `tracing::warn!` before being returned, so they are never silent
    /// even when a caller ignores the `Result`.
    pub fn maybe_autosave(&mut self) -> Result<bool, SaveError> {
        let (due, dir) = match &self.engine_state.autosave {
            Some(a) if a.due(self.world.days_simulated) => (true, a.dir.clone()),
            _ => (false, PathBuf::new()),
        };
        if !due {
            return Ok(false);
        }
        match self.save(&dir) {
            Ok(()) => {
                self.last_autosave_error = None;
                if let Some(a) = &mut self.engine_state.autosave {
                    a.mark_saved(self.world.days_simulated);
                }
                Ok(true)
            }
            Err(e) => {
                tracing::warn!(error = %e, "autosave failed");
                self.last_autosave_error = Some(e.to_string());
                Err(e)
            }
        }
    }

    /// Engine invariants: population ≤ capacity, integral non-negative money, unique entity ids,
    /// and S6 state consistency invariants.
    pub fn check_invariants(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let w = &self.world;

        // Existing invariants
        if !alc_testing::invariant_capacity(w.population(), w.capacity()) {
            errs.push("population exceeds facility capacity".to_string());
        }
        if !alc_testing::invariant_money_integral(w.economy.cash.0) {
            errs.push("economy.cash is not integral".to_string());
        }
        if w.economy.cash.0 < 0 {
            errs.push("economy.cash is negative".to_string());
        }
        let mut prisoners_seen = HashSet::new();
        for p in &w.prisoners {
            if !prisoners_seen.insert(p.id.raw()) {
                errs.push(format!("duplicate prisoner id {}", p.id.raw()));
            }
        }
        let mut guards_seen = HashSet::new();
        for g in &w.guards {
            if !guards_seen.insert(g.id.raw()) {
                errs.push(format!("duplicate guard id {}", g.id.raw()));
            }
        }
        let mut facilities_seen = HashSet::new();
        for f in &w.facilities {
            if !facilities_seen.insert(f.id.0) {
                errs.push(format!("duplicate facility id {}", f.id.0));
            }
        }

        // S6: Integrated Invariants from invariants.rs
        if !crate::invariants::invariant_pawn_in_exactly_one_place(w) {
            errs.push("pawn_in_exactly_one_place violation".to_string());
        }
        if !crate::invariants::invariant_intention_target_exists_and_is_legal(w) {
            errs.push("intention_target_exists_and_is_legal violation".to_string());
        }
        if !crate::invariants::invariant_every_incident_has_cause_and_consequence(self) {
            errs.push("every_incident_has_cause_and_consequence violation".to_string());
        }
        if !crate::invariants::invariant_suspicion_claim_records_reference_existing_pawns(w) {
            errs.push("suspicion_claim_records_reference_existing_pawns violation".to_string());
        }
        if !crate::invariants::invariant_gang_membership_and_history(w) {
            errs.push("gang_membership_and_history violation".to_string());
        }
        if !crate::invariants::invariant_policy_state_matches_world_policies(&self.engine_state, w)
        {
            errs.push("policy_state_matches_world_policies violation".to_string());
        }
        if !crate::invariants::invariant_logs_and_incident_store_bounded(self) {
            errs.push("logs_and_incident_store_bounded violation".to_string());
        }

        errs
    }

    /// Record an incident in the world incident log (R1: incident reports).
    ///
    /// Incidents are stored for the UI Incident Reports panel to display.
    /// Typical events include: arguments, fights, thefts, medical emergencies,
    /// security incidents, and parole hearings.
    pub fn record_incident(&mut self, event: alc_events::WorldEvent) {
        self.engine_state.incidents.push(event);
        self.trim_engine_logs();
    }

    /// Record a structured incident in the sidecar log (R6: causality).
    ///
    /// Adds an Incident with cause/consequence traceability to the sidecar
    /// storage alongside the WorldEvent in the primary incident log.
    /// The incident sidecar enables causal chain inspection for traceability
    /// and determinism checks, without modifying the EventKind enum.
    pub fn record_sidecar_incident(&mut self, incident: incidents::Incident) {
        self.engine_state.incident_sidecar.push(incident);
        self.trim_engine_logs();
    }

    /// Get the world incident log for the UI Incident Reports panel.
    ///
    /// Returns a copy of all recorded incidents since simulation start.
    pub fn incident_log(&self) -> Vec<alc_events::WorldEvent> {
        self.engine_state.incidents.clone()
    }

    /// Get incidents since a given simulation time from the sidecar storage.
    ///
    /// Returns incidents after the given time, for traceability and
    /// causality chain inspection. This reads from the sidecar storage,
    /// not from the EventKind-enriched world event bus.
    pub fn incidents_since(&self, at: SimTime) -> Vec<incidents::Incident> {
        incidents::incidents_since(&self.engine_state.incident_sidecar, at)
    }

    /// Structured incident records for observer UI (0.05).
    pub fn incident_records(&self) -> Vec<IncidentRecord> {
        self.engine_state
            .incident_sidecar
            .iter()
            .map(IncidentRecord::from)
            .collect()
    }

    /// Record a delivered decision in the audit log (R2: decision audit log).
    ///
    /// Persists every decision delivered to the engine for later review
    /// and replay. Used by the decision audit log panel and determinism checks.
    pub fn record_decision(&mut self, decision: alc_agents::Decision) {
        self.engine_state.decision_log.push(decision);
        self.trim_engine_logs();
    }

    /// Track a prisoner's post-release outcome (R4: post-release tracking).
    ///
    /// Marks a released prisoner's reintegration status. Returns the previous
    /// status if the prisoner was already tracked.
    pub fn set_post_release(
        &mut self,
        prisoner: alc_core::PrisonerId,
        success: bool,
    ) -> Option<bool> {
        let prev = self.engine_state.post_release.insert(prisoner, success);
        self.audit_control(
            "observer/control",
            "SetPostRelease",
            format!(
                "Prisoner {} post-release success={}",
                prisoner.raw(),
                success
            ),
            "accepted",
            "post-release status updated",
        );
        prev
    }

    /// Get the post-release status for a prisoner.
    ///
    /// Returns `None` if the prisoner has not been tracked, or `Some(true/false)`
    /// for their reintegration success status.
    pub fn post_release_status(&self, prisoner: alc_core::PrisonerId) -> Option<bool> {
        self.engine_state.post_release.get(&prisoner).copied()
    }

    /// Set the prison's policy level (H1: policy management).
    ///
    /// Changes the policy level and emits a `PolicyChange` event if the level
    /// actually changes. Valid policy levels are `Normal`, `Enhanced`, and `Lockdown`.
    /// Also updates the world's policies to affect simulation behavior.
    pub fn set_policy(&mut self, policy: Policy) {
        if self.engine_state.policy_state.policy != policy {
            let old_security = self.world.policies.security_level;
            self.engine_state.policy_state.policy = policy;
            self.engine_state.policy_state.last_changed = self.world.time;

            // Sync with world.policies to affect simulation behavior
            match policy {
                Policy::Normal => {
                    self.world.policies.lockdown = false;
                    self.world.policies.security_level = 0.6;
                    self.world.policies.privileges_level = 0.5;
                    self.engine_state.policy_state.lockdown = false;
                    self.engine_state.policy_state.security_level = 1;
                }
                Policy::Enhanced => {
                    self.world.policies.lockdown = false;
                    self.world.policies.security_level = 0.8;
                    self.world.policies.privileges_level = 0.3;
                    self.engine_state.policy_state.lockdown = false;
                    self.engine_state.policy_state.security_level = 3;
                }
                Policy::Lockdown => {
                    self.world.policies.lockdown = true;
                    self.world.lockdown_since_day = self.world.days_simulated;
                    self.world.policies.security_level = 1.0;
                    self.world.policies.privileges_level = 0.1;
                    self.engine_state.policy_state.lockdown = true;
                    self.engine_state.policy_state.security_level = 5;
                }
            }
            // Institutional memory: controls must be auditable in world.history,
            // not only in the engine decision-audit log.
            self.world.audit_policy_change(
                &format!("policy={policy:?}"),
                old_security,
                self.world.policies.security_level,
            );
            self.audit_control(
                "warden/control",
                "SetPolicy",
                format!("Policy changed to {:?}", policy),
                "accepted",
                "applied to engine_state and world.policies",
            );
        }
    }

    /// Set the prison lockdown status (H1: policy management).
    ///
    /// Toggles lockdown status and emits a `PolicyChange` event. When setting
    /// lockdown to `true`, the security level is automatically increased.
    /// Also updates the world's policies to affect simulation behavior.
    pub fn set_lockdown(&mut self, lockdown: bool) {
        if self.engine_state.policy_state.lockdown != lockdown {
            self.engine_state.policy_state.lockdown = lockdown;
            self.engine_state.policy_state.last_changed = self.world.time;

            // Anchor the 14-day emergency-expiry clock to the observer action
            // so an imposed lockdown does not expire on the next weekly sweep.
            self.world.lockdown_since_day = self.world.days_simulated;
            // Sync with world.policies to affect simulation behavior
            self.world.policies.lockdown = lockdown;
            if lockdown {
                self.world.policies.security_level =
                    (self.world.policies.security_level + 0.2).min(1.0);
                self.world.policies.privileges_level =
                    (self.world.policies.privileges_level - 0.2).max(0.1);
                self.engine_state.policy_state.security_level =
                    std::cmp::min(5, self.engine_state.policy_state.security_level + 1);
            } else {
                self.world.policies.security_level =
                    (self.world.policies.security_level - 0.1).max(0.3);
                self.world.policies.privileges_level =
                    (self.world.policies.privileges_level + 0.1).min(0.5);
            }

            self.world.audit_policy_change(
                "lockdown",
                if lockdown { 0.0 } else { 1.0 },
                if lockdown { 1.0 } else { 0.0 },
            );
            self.audit_control(
                "warden/control",
                "SetLockdown",
                format!("Lockdown {}", if lockdown { "enabled" } else { "disabled" }),
                "accepted",
                "applied to engine_state and world.policies",
            );
        }
    }

    /// Observer-facing policy summary for control panels.
    pub fn policy_summary(&self) -> PolicySummary {
        PolicySummary {
            policy: self.engine_state.policy_state.policy,
            lockdown: self.world.policies.lockdown,
            security_level: self.world.policies.security_level,
            privileges_level: self.world.policies.privileges_level,
            rehab_funding_multiplier: self.world.policies.rehab_funding_multiplier,
            intake_paused: self.world.policies.lockdown
                || self.world.policies.security_level >= 0.9
                || self.world.policies.intake_paused,
        }
    }

    /// Set fine-grained security level (0.0-1.0) with validation and event feedback.
    pub fn set_security_level(&mut self, level: f32) {
        let clamped = level.clamp(0.0, 1.0);
        let repaired = !level.is_finite() || (level - clamped).abs() > f32::EPSILON;
        let level = clamped;
        let old = self.world.policies.security_level;
        if (old - level).abs() < f32::EPSILON {
            return;
        }
        self.world.policies.security_level = level;
        self.sync_policy_band_from_world();
        self.engine_state.policy_state.last_changed = self.world.time;
        self.world
            .audit_policy_change("security_level", old, self.world.policies.security_level);
        self.audit_control(
            "warden/control",
            "SetSecurityLevel",
            format!("Security level {:.2} -> {:.2}", old, level),
            if repaired { "repaired" } else { "accepted" },
            if repaired {
                "input clamped to 0.0..=1.0; policy band synced"
            } else {
                "world.policies.security_level updated; policy band synced"
            },
        );
    }

    /// Set privileges level (0.0-1.0) with validation and event feedback.
    pub fn set_privileges_level(&mut self, level: f32) {
        let clamped = level.clamp(0.0, 1.0);
        let repaired = !level.is_finite() || (level - clamped).abs() > f32::EPSILON;
        let level = clamped;
        let old = self.world.policies.privileges_level;
        if (old - level).abs() < f32::EPSILON {
            return;
        }
        self.world.policies.privileges_level = level;
        self.engine_state.policy_state.last_changed = self.world.time;
        self.world
            .audit_policy_change("privileges_level", old, level);
        self.audit_control(
            "warden/control",
            "SetPrivilegesLevel",
            format!("Privileges {:.2} -> {:.2}", old, level),
            if repaired { "repaired" } else { "accepted" },
            if repaired {
                "input clamped to 0.0..=1.0"
            } else {
                "world.policies.privileges_level updated"
            },
        );
    }

    /// Set rehabilitation funding multiplier (0.0-3.0) with validation and
    /// event feedback. The range matches the warden's `ChangePolicy` applier.
    pub fn set_rehab_funding(&mut self, multiplier: f32) {
        let clamped = multiplier.clamp(0.0, 3.0);
        let repaired = !multiplier.is_finite() || (multiplier - clamped).abs() > f32::EPSILON;
        let multiplier = clamped;
        let old = self.world.policies.rehab_funding_multiplier;
        if (old - multiplier).abs() < f32::EPSILON {
            return;
        }
        self.world.policies.rehab_funding_multiplier = multiplier;
        self.engine_state.policy_state.last_changed = self.world.time;
        self.world
            .audit_policy_change("rehab_funding_multiplier", old, multiplier);
        self.audit_control(
            "warden/control",
            "SetRehabFunding",
            format!("Rehab funding {:.2} -> {:.2}", old, multiplier),
            if repaired { "repaired" } else { "accepted" },
            if repaired {
                "input clamped to 0.0..=3.0"
            } else {
                "world.policies.rehab_funding_multiplier updated"
            },
        );
    }

    /// Manually pause or resume intake (0.05 policy control).
    pub fn set_intake_paused(&mut self, paused: bool) {
        if self.world.policies.intake_paused == paused {
            return;
        }
        self.world.policies.intake_paused = paused;
        self.engine_state.policy_state.last_changed = self.world.time;
        self.world.audit_policy_change(
            "intake_paused",
            if paused { 0.0 } else { 1.0 },
            if paused { 1.0 } else { 0.0 },
        );
        self.audit_control(
            "warden/control",
            "SetIntakePaused",
            format!("Intake {}", if paused { "paused" } else { "resumed" }),
            "accepted",
            "world.policies.intake_paused updated",
        );
    }

    /// Sync policy band/engine security from world security while preserving fine-grained value.
    fn sync_policy_band_from_world(&mut self) {
        let wsec = self.world.policies.security_level;
        let lockdown = self.world.policies.lockdown;
        let (policy, base_engine) = if wsec >= 0.9 || lockdown {
            (Policy::Lockdown, 5_u32)
        } else if wsec >= 0.7 {
            (Policy::Enhanced, 3_u32)
        } else {
            (Policy::Normal, 1_u32)
        };
        self.engine_state.policy_state.policy = policy;
        self.engine_state.policy_state.lockdown = lockdown;
        self.engine_state.policy_state.security_level = base_engine;
    }

    /// Human-readable summary (parity with the `alcagent` binary).
    pub fn report(&self) -> String {
        let w = &self.world;
        let mut s = String::from("sumo — prison world\n");
        s.push_str(&format!("time: {}  days: {}\n", w.time, w.days_simulated));
        s.push_str(&format!(
            "population: {} / {}\n",
            w.population(),
            w.capacity()
        ));
        s.push_str(&format!(
            "guards: {}  facilities: {}\n",
            w.guards.len(),
            w.facilities.len()
        ));
        s.push_str(&format!(
            "warden: {} ({})\n",
            w.warden.name, w.warden.philosophy
        ));
        s.push_str(&format!(
            "cash: ${:.2}  health: {:.2}\n",
            w.economy.cash.as_major(),
            w.economy.health()
        ));
        s.push_str(&format!(
            "events: {}  speed: {:?}\n",
            w.events.len(),
            self.speed
        ));
        s
    }

    /// Emit engine events with authoritative world event ids, then mirror
    /// conflict kinds into the incident stores.
    ///
    /// IDs are assigned back into `events` before mirroring so dedupe and
    /// causal provenance use the same ids observers see on the world bus.
    pub(crate) fn emit_events(&mut self, events: &mut [WorldEvent]) {
        for e in events.iter_mut() {
            e.at = self.world.time;
            e.id = self.world.events.emit(e.clone());
        }
        for e in events.iter() {
            self.mirror_conflict_event_to_engine_log(e);
        }
    }

    // ---------------------------------------------------------------------
    // Expansion API (Documentation/Design/Engine/sumo-expansion.md §4)
    // ---------------------------------------------------------------------

    // --- 4.1 Time control (extends §3) ---

    /// Advance `secs` seconds of simulation; ticks while the world advances.
    pub fn advance_by_secs(&mut self, secs: u64) -> u64 {
        let target = self.world.time.add_secs(secs);
        let mut n = 0u64;
        while self.world.time < target && self.speed != TimeSpeed::Paused {
            let before = self.world.time;
            self.tick();
            n += 1;
            if self.world.time == before {
                break;
            }
        }
        n
    }

    /// Advance `days` simulated days (86,400 s each).
    pub fn run_days(&mut self, days: u64) -> u64 {
        self.advance_by_secs(days.saturating_mul(86_400))
    }

    /// Set the simulation speed to paused.
    pub fn pause(&mut self) {
        self.set_speed(TimeSpeed::Paused);
    }
    /// Set the simulation speed to real time.
    pub fn resume(&mut self) {
        self.set_speed(TimeSpeed::RealTime);
    }

    /// Event-driven advance: tick until an event matching `pred` appears
    /// after `after`, or the simulation pauses. Returns the event if found.
    pub fn wait_for_event(
        &mut self,
        after: SimTime,
        pred: impl Fn(&WorldEvent) -> bool,
    ) -> Option<WorldEvent> {
        loop {
            if let Some(e) = self.events_since(after).into_iter().find(|e| pred(e)) {
                return Some(e);
            }
            if self.speed == TimeSpeed::Paused {
                return None;
            }
            let before = self.world.time;
            self.tick();
            if self.world.time == before {
                return None;
            }
        }
    }

    // --- 4.2 Events ---

    /// Public engine event emission.
    ///
    /// Conflict-kind events are mirrored into the engine incident log and
    /// structured sidecar so observers see them consistently regardless of
    /// which path emitted them. Standalone `ContrabandFound` events are ingested
    /// into the world-owned case store once so summaries stay store-backed.
    pub fn emit_event(&mut self, e: WorldEvent) {
        let at = self.world.time;
        let mut ev = e;
        if ev.at.secs == 0 {
            ev.at = at;
        }
        let id = self.world.events.emit(ev.clone());
        let mut emitted = ev;
        emitted.id = id;
        // Compatibility bridge for callers that inject a legacy standalone
        // `ContrabandFound` event. The durable case store remains the single
        // source of truth for summaries and resolution; ingestion just ensures
        // such events are representable there instead of parsed on every read.
        self.ingest_legacy_contraband_event(&emitted);
        self.mirror_conflict_event_to_engine_log(&emitted);
    }

    /// Parse a legacy standalone `ContrabandFound` event into a durable case.
    fn legacy_contraband_case_from_event(
        event: &WorldEvent,
        case_id: u64,
    ) -> Option<ContrabandCaseRecord> {
        use alc_social::ContrabandItem;
        use alc_social::contraband::ConfiscationDisposition;

        if event.kind != EventKind::ContrabandFound {
            return None;
        }
        let holder = event.actors.first().copied().map(PrisonerId)?;
        let candidates = [
            ("Weapons", ContrabandItem::Weapons, 0.95_f32),
            ("Drugs", ContrabandItem::Drugs, 0.75),
            ("Tools", ContrabandItem::Tools, 0.7),
            ("Phones", ContrabandItem::Phones, 0.55),
            ("Money", ContrabandItem::Money, 0.35),
        ];
        let (_, item, severity) = candidates
            .into_iter()
            .find(|(label, _, _)| event.summary.contains(*label))?;
        let guard = event
            .causes
            .iter()
            .find_map(|cause| {
                let lower = cause.to_ascii_lowercase();
                let start = lower.find("guard")? + "guard".len();
                let digits: String = cause[start..]
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                digits.parse::<u64>().ok()
            })
            .unwrap_or(0);
        let disposition = if severity >= 0.8 {
            ConfiscationDisposition::Evidence
        } else if severity >= 0.4 {
            ConfiscationDisposition::Seized
        } else {
            ConfiscationDisposition::Destroyed
        };
        Some(ContrabandCaseRecord {
            case_id,
            discovered_at: event.at,
            discovering_guard: GuardId(guard),
            holder,
            item,
            severity,
            disposition,
            disciplinary_action: None,
            summary: event.summary.clone(),
        })
    }

    /// Allocate a case id that cannot alias any existing case or persisted
    /// resolution id. Legacy engine-only resolutions have no case row, so the
    /// allocator must reserve them explicitly.
    fn allocate_contraband_case_id(&self) -> u64 {
        let mut id = self.world.next_contraband_case_id.max(1);
        while id < u64::MAX
            && (self.world.contraband_cases.iter().any(|c| c.case_id == id)
                || self.engine_state.resolved_contraband_cases.contains(&id)
                || self.world.resolved_contraband_at.contains_key(&id))
        {
            id = id.saturating_add(1);
        }
        id
    }

    /// Ingest an externally emitted `ContrabandFound` event into the world case
    /// store once. Real discoveries already write a case record before emitting
    /// their event, so the holder/item/time guard prevents duplicates.
    fn ingest_legacy_contraband_event(&mut self, event: &WorldEvent) {
        let case_id = self.allocate_contraband_case_id();
        let Some(case) = Self::legacy_contraband_case_from_event(event, case_id) else {
            return;
        };
        if self.world.contraband_cases.iter().any(|existing| {
            existing.holder == case.holder
                && existing.item == case.item
                && existing.discovered_at == case.discovered_at
        }) {
            return;
        }
        self.world.next_contraband_case_id = case_id.saturating_add(1);
        self.world.contraband_cases.push(case);
        // Match the world's bounded case log so external event injection cannot
        // grow the durable store without limit.
        if self.world.contraband_cases.len() > 512 {
            let excess = self.world.contraband_cases.len() - 512;
            self.world.contraband_cases.drain(..excess);
        }
    }

    /// Migrate `ContrabandFound` events from older saves into the durable case
    /// store and heal resolution mirrors keyed by their original event ids.
    ///
    /// Old saves had no `world.contraband_cases` but did persist discovery
    /// events and resolution ids. Preserving the event id as the case id keeps
    /// those resolutions valid. Engine-only resolutions with no case row remain
    /// part of the persistence contract, so they are preserved too; their ids
    /// are reserved before future allocation so they cannot mislabel new cases.
    fn migrate_legacy_contraband_on_load(&mut self) {
        use std::collections::{BTreeMap, HashSet};

        const MAX_CASES: usize = 512;
        let had_case_store = !self.world.contraband_cases.is_empty();
        let mut events: Vec<WorldEvent> = self
            .world
            .events
            .recent()
            .cloned()
            .chain(self.engine_state.incidents.iter().cloned())
            .filter(|event| event.kind == EventKind::ContrabandFound)
            .collect();
        events.sort_by_key(|event| event.id);
        let mut seen_event_ids = HashSet::new();
        events.retain(|event| seen_event_ids.insert(event.id));

        let mut used_ids: HashSet<u64> = self
            .world
            .contraband_cases
            .iter()
            .map(|case| case.case_id)
            .collect();
        let mut remap: BTreeMap<u64, u64> = BTreeMap::new();
        let reserved_ids: HashSet<u64> = self
            .engine_state
            .resolved_contraband_cases
            .iter()
            .copied()
            .chain(self.engine_state.resolved_contraband_at.keys().copied())
            .chain(self.world.resolved_contraband_at.keys().copied())
            .collect();
        let mut next_id = self.world.next_contraband_case_id.max(1);

        for event in events {
            let Some(mut case) = Self::legacy_contraband_case_from_event(&event, 0) else {
                continue;
            };
            if let Some(existing) = self.world.contraband_cases.iter().find(|existing| {
                existing.holder == case.holder
                    && existing.item == case.item
                    && existing.discovered_at == case.discovered_at
            }) {
                remap.insert(event.id, existing.case_id);
                continue;
            }
            let case_id = if !had_case_store && !used_ids.contains(&event.id) {
                event.id
            } else {
                while (used_ids.contains(&next_id) || reserved_ids.contains(&next_id))
                    && next_id < u64::MAX
                {
                    next_id = next_id.saturating_add(1);
                }
                let id = next_id;
                next_id = next_id.saturating_add(1);
                id
            };
            case.case_id = case_id;
            self.world.contraband_cases.push(case);
            used_ids.insert(case_id);
            remap.insert(event.id, case_id);
        }

        if self.world.contraband_cases.len() > MAX_CASES {
            self.world.contraband_cases.sort_by_key(|case| case.case_id);
            let excess = self.world.contraband_cases.len() - MAX_CASES;
            self.world.contraband_cases.drain(..excess);
        }
        used_ids = self
            .world
            .contraband_cases
            .iter()
            .map(|case| case.case_id)
            .collect();

        let remapped_id = |id: u64| remap.get(&id).copied().unwrap_or(id);
        let engine_ids: Vec<u64> = self
            .engine_state
            .resolved_contraband_cases
            .iter()
            .copied()
            .collect();
        self.engine_state.resolved_contraband_cases.clear();
        for id in engine_ids {
            self.engine_state
                .resolved_contraband_cases
                .insert(remapped_id(id));
        }
        let engine_at = std::mem::take(&mut self.engine_state.resolved_contraband_at);
        for (id, at) in engine_at {
            self.engine_state
                .resolved_contraband_at
                .entry(remapped_id(id))
                .or_insert(at);
        }
        let world_at = std::mem::take(&mut self.world.resolved_contraband_at);
        for (id, at) in world_at {
            self.world
                .resolved_contraband_at
                .entry(remapped_id(id))
                .or_insert(at);
        }

        let max_case_id = used_ids.iter().copied().max().unwrap_or(0);
        let max_resolution_id = self
            .engine_state
            .resolved_contraband_cases
            .iter()
            .copied()
            .chain(self.engine_state.resolved_contraband_at.keys().copied())
            .chain(self.world.resolved_contraband_at.keys().copied())
            .max()
            .unwrap_or(0);
        self.world.next_contraband_case_id = self
            .world
            .next_contraband_case_id
            .max(next_id)
            .max(max_case_id.saturating_add(1))
            .max(max_resolution_id.saturating_add(1))
            .max(1);
    }

    // --- 4.3 Guard duty ---

    /// Run the duty loop of every guard once (§10): tick + observe + report +
    /// enforce + request_assistance; reports are collected.
    pub fn guard_duty_cycle(&mut self) -> Vec<(GuardId, GuardReport)> {
        drivers::guard_duty_cycle(self)
    }

    /// Run one guard's duty loop for `minutes` minutes; reports collected.
    pub fn run_guard_duty(&mut self, guard_id: GuardId, minutes: u64) -> Vec<GuardReport> {
        drivers::run_guard_duty(self, guard_id, minutes)
    }

    /// Deliver a guard duty report to the world bus; returns the event id.
    pub fn deliver_guard_report(&mut self, guard_id: GuardId, report: GuardReport) -> u64 {
        drivers::deliver_guard_report(self, guard_id, report)
    }

    /// Reassign a guard to a post. The world moves the pawn atomically; an
    /// unknown guard is rejected without mutating the location map.
    pub fn assign_guard_post(&mut self, guard_id: GuardId, post: PostType) -> bool {
        self.world.assign_guard_post(guard_id, post)
    }

    // --- 4.4 Warden ---

    /// Warden cycle: arbitrate via `alc_warden::arbitrate` + apply.
    pub fn warden_cycle(&mut self) -> Vec<WardenAction> {
        drivers::warden_cycle(self)
    }

    /// Explicit strategy cycle: decide from reports/summaries and apply the
    /// audited result without changing the configured policy mode.
    pub fn warden_strategy_cycle(&mut self) -> Vec<WardenAction> {
        drivers::warden_strategy_cycle(self)
    }

    /// Apply a WardenAction with the engine-side applier.
    pub fn apply_warden_action(&mut self, action: WardenAction) {
        self.apply_warden_action_with_rationale(
            action,
            String::new(),
            DecisionSource::AiArbitration,
        );
    }

    /// Apply a strategy decision and persist its rationale in both audit
    /// stores. Returns the action actually applied (possibly `NoAction` when
    /// attention has been exhausted since deliberation).
    pub fn apply_strategy_decision(
        &mut self,
        decision: StrategyDecision,
        source: DecisionSource,
    ) -> WardenAction {
        let action = decision.action.clone();
        let cost = action.attention_cost();
        if cost > 0.0 && !self.world.warden.spend_attention(cost) && !action.is_emergency() {
            let deferred = WardenAction::NoAction {
                rationale: "attention exhausted before application".into(),
            };
            let rationale = format!(
                "{} | deferred: attention budget exhausted before application",
                decision.rationale
            );
            self.record_warden_decision(deferred.clone(), rationale.clone(), source, false);
            return deferred;
        }

        let rationale = format!(
            "{} | constraints: {} | finance: {}",
            decision.rationale,
            if decision.conflicts.is_empty() {
                "none".to_string()
            } else {
                decision.conflicts.join("; ")
            },
            decision.finance_state.label()
        );
        self.apply_warden_action_with_rationale(action, rationale, source)
    }

    fn apply_warden_action_with_rationale(
        &mut self,
        action: WardenAction,
        rationale: String,
        source: DecisionSource,
    ) -> WardenAction {
        let at = self.world.time;
        let summary = format!("{:?}", action);
        let last_event_id = self.world.events.recent().last().map(|e| e.id).unwrap_or(0);
        let _events = drivers::apply_warden_action(&mut self.world, at, action.clone());
        // The world applier emits authoritative event ids directly; mirror the
        // new events into engine logs instead of emitting a second copy.
        self.pump_events_into_engine_log(last_event_id);
        // The driver mutates `world.policies` directly (it only has &mut World),
        // so reconcile the engine-side policy mirror afterwards (R6: drift
        // between the two is an invariant violation).
        self.sync_policy_from_world();
        if let WardenAction::AdjustParoleRecommendation { prisoner, lean } = &action {
            self.engine_state
                .parole
                .set_adjustment(PrisonerId(*prisoner), *lean);
        }
        self.record_warden_decision(action.clone(), rationale.clone(), source, true);
        self.record_decision_audit(DecisionAuditRecord {
            time: at,
            actor: "warden".to_string(),
            source: format!("{source:?}"),
            action: "WardenAction".to_string(),
            summary,
            validation: "accepted".to_string(),
            reason: "applied by engine".to_string(),
            rationale,
            repair_reason: String::new(),
            outcome: alc_agents::decision::DecisionOutcomeState::Accepted,
        });
        action
    }

    /// Record a warden decision in the world-owned bounded audit log.
    fn record_warden_decision(
        &mut self,
        action: WardenAction,
        rationale: String,
        source: DecisionSource,
        applied: bool,
    ) {
        let at = self.world.time;
        let reason = if rationale.is_empty() {
            action.default_reason().to_string()
        } else {
            rationale
        };
        self.world
            .warden
            .record_decision(at, action, reason, source, applied);
    }

    /// Reconcile `engine_state.policy_state` with the authoritative
    /// `world.policies` after a driver mutated the world directly.
    fn sync_policy_from_world(&mut self) {
        let wsec = self.world.policies.security_level;
        let lockdown = self.world.policies.lockdown;
        let ps = &mut self.engine_state.policy_state;
        ps.lockdown = lockdown;
        // Derive the nearest policy band from world security so the invariant's
        // tolerance check around each band base stays satisfied.
        let (policy, base_engine, base_world) = if wsec >= 0.9 || lockdown {
            (Policy::Lockdown, 5_u32, 1.0_f32)
        } else if wsec >= 0.7 {
            (Policy::Enhanced, 3_u32, 0.8_f32)
        } else {
            (Policy::Normal, 1_u32, 0.6_f32)
        };
        ps.policy = policy;
        ps.security_level = base_engine;
        ps.last_changed = self.world.time;
        if (wsec - base_world).abs() > 0.25 {
            self.world.policies.security_level = base_world;
        }
    }

    /// Warden actions still pending.
    pub fn warden_pending(&self) -> Vec<WardenAction> {
        drivers::warden_pending(self)
    }

    // --- 4.5 Parole (§15) ---

    /// Prisoners currently eligible and without an engine record.
    pub fn parole_due(&self) -> Vec<PrisonerId> {
        self.world
            .prisoners
            .iter()
            .filter(|p| p.is_eligible_for_parole() && !self.engine_state.parole.has(p.id))
            .map(|p| p.id)
            .collect()
    }

    /// Schedule a parole hearing (§15 step 8); returns the new event id.
    ///
    /// Validated: the prisoner must exist, be eligible and have no prior
    /// decision. Rejections are audited rather than silently returning `None`.
    pub fn start_parole_hearing(&mut self, id: PrisonerId) -> Option<u64> {
        let eligible = self
            .world
            .prisoners
            .iter()
            .any(|p| p.id == id && p.is_eligible_for_parole());
        if !eligible || self.engine_state.parole.status(id).is_some() {
            self.audit_control_rejected(
                "observer/control",
                "StartParoleHearing",
                format!("Parole hearing for prisoner {} refused", id.raw()),
                if eligible {
                    "prisoner already has a parole record"
                } else {
                    "prisoner is absent or ineligible"
                },
            );
            return None;
        }
        self.engine_state
            .parole
            .set(id, ParoleStatus::Scheduled, self.world.time);
        self.engine_state.parole.set_hearing_at(id, self.world.time);
        self.world.events.emit(
            WorldEvent::new(
                0,
                self.world.time,
                EventKind::ParoleHearing,
                format!("Parole hearing scheduled for prisoner {}", id.raw()),
            )
            .with_actor(id.raw()),
        );
        let event_id = self.world.events.recent().last().map(|e| e.id);
        self.audit_control(
            "observer/control",
            "StartParoleHearing",
            format!("Parole hearing scheduled for prisoner {}", id.raw()),
            "accepted",
            "eligibility verified; hearing date recorded",
        );
        event_id
    }

    /// Deterministic risk/rehabilitation inputs for a parole decision.
    fn parole_decision_inputs(&self, id: PrisonerId) -> (f32, f32) {
        let Some(p) = self.world.prisoners.iter().find(|p| p.id == id) else {
            return (0.5, 0.5);
        };
        let risk = (p.escape_motivation() * 0.5
            + p.traits.aggression * 0.25
            + (1.0 - p.condition.mental_health.clamp(0.0, 1.0)) * 0.25)
            .clamp(0.0, 1.0);
        let rehab = p.rehab_motivation().clamp(0.0, 1.0);
        (risk, rehab)
    }

    /// Grant parole (§15 step 10); returns the new event id.
    ///
    /// A grant records deterministic conditions and a decision reason before
    /// emitting `ParoleGranted`. Use [`Self::release_on_parole`] to carry the
    /// grant through to an actual release from custody.
    pub fn grant_parole(&mut self, id: PrisonerId) -> Option<u64> {
        let eligible = self
            .world
            .prisoners
            .iter()
            .any(|p| p.id == id && p.is_eligible_for_parole());
        if !eligible
            || matches!(
                self.engine_state.parole.status(id),
                Some(ParoleStatus::Granted) | Some(ParoleStatus::Denied)
            )
        {
            self.audit_control_rejected(
                "observer/control",
                "GrantParole",
                format!("Parole grant for prisoner {} refused", id.raw()),
                if eligible {
                    "prisoner already has a terminal parole decision"
                } else {
                    "prisoner is absent or ineligible"
                },
            );
            return None;
        }
        let (risk, rehab) = self.parole_decision_inputs(id);
        if self.engine_state.parole.status(id).is_none() {
            self.engine_state
                .parole
                .set(id, ParoleStatus::InHearing, self.world.time);
        }
        self.engine_state
            .parole
            .set(id, ParoleStatus::Granted, self.world.time);
        self.engine_state
            .parole
            .set_conditions(id, ParoleConditions::deterministic(id, risk, rehab));
        let reason = format!("deterministic board decision: risk {risk:.2}, rehab {rehab:.2}");
        self.engine_state
            .parole
            .set_decision_reason(id, reason.clone());
        self.world.events.emit(
            WorldEvent::new(
                0,
                self.world.time,
                EventKind::ParoleGranted,
                format!("Parole granted to prisoner {}", id.raw()),
            )
            .with_actor(id.raw())
            .with_cause(reason.clone()),
        );
        let event_id = self.world.events.recent().last().map(|e| e.id);
        self.audit_control(
            "observer/control",
            "GrantParole",
            reason,
            "accepted",
            "parole granted by validated engine control",
        );
        event_id
    }

    /// Deny parole (§15); returns the new event id.
    pub fn deny_parole(&mut self, id: PrisonerId, reason: impl Into<String>) -> Option<u64> {
        let eligible = self
            .world
            .prisoners
            .iter()
            .any(|p| p.id == id && p.is_eligible_for_parole());
        if !eligible
            || matches!(
                self.engine_state.parole.status(id),
                Some(ParoleStatus::Granted) | Some(ParoleStatus::Denied)
            )
        {
            self.audit_control_rejected(
                "observer/control",
                "DenyParole",
                format!("Parole denial for prisoner {} refused", id.raw()),
                if eligible {
                    "prisoner already has a terminal parole decision"
                } else {
                    "prisoner is absent or ineligible"
                },
            );
            return None;
        }
        let reason = reason.into();
        self.engine_state
            .parole
            .set(id, ParoleStatus::Denied, self.world.time);
        self.engine_state
            .parole
            .set_decision_reason(id, reason.clone());
        self.world.events.emit(
            WorldEvent::new(
                0,
                self.world.time,
                EventKind::ParoleDenied,
                format!("Parole denied for prisoner {}: {}", id.raw(), reason),
            )
            .with_actor(id.raw())
            .with_cause(reason.clone()),
        );
        let event_id = self.world.events.recent().last().map(|e| e.id);
        self.audit_control(
            "observer/control",
            "DenyParole",
            format!("Parole denied for prisoner {}", id.raw()),
            "accepted",
            reason,
        );
        event_id
    }

    /// Attach or replace validated parole conditions for a decided prisoner.
    pub fn set_parole_conditions(
        &mut self,
        id: PrisonerId,
        mut conditions: ParoleConditions,
    ) -> bool {
        let decidable = matches!(
            self.engine_state.parole.status(id),
            Some(ParoleStatus::Granted)
                | Some(ParoleStatus::Scheduled)
                | Some(ParoleStatus::InHearing)
        );
        if !decidable {
            self.audit_control_rejected(
                "observer/control",
                "SetParoleConditions",
                format!("Conditions for prisoner {} refused", id.raw()),
                "no scheduled or granted parole decision",
            );
            return false;
        }
        let repaired = !conditions.is_normalized();
        conditions.normalize();
        self.engine_state.parole.set_conditions(id, conditions);
        self.audit_control(
            "observer/control",
            "SetParoleConditions",
            format!("Parole conditions set for prisoner {}", id.raw()),
            if repaired { "repaired" } else { "accepted" },
            if repaired {
                "conditions clamped to validated ranges"
            } else {
                "validated conditions persisted"
            },
        );
        true
    }

    /// Release a granted prisoner from custody through the authoritative world
    /// release path. Returns `false` unless the prisoner is still present and
    /// has a stored grant. The release time is recorded in the ledger.
    pub fn release_on_parole(&mut self, id: PrisonerId) -> bool {
        if !matches!(
            self.engine_state.parole.status(id),
            Some(ParoleStatus::Granted)
        ) || self.engine_state.parole.released_at(id).is_some()
        {
            self.audit_control_rejected(
                "observer/control",
                "ReleaseOnParole",
                format!("Release for prisoner {} refused", id.raw()),
                "no unexecuted parole grant",
            );
            return false;
        }
        let before = self.world.events.recent().map(|e| e.id).max().unwrap_or(0);
        let released = self.world.release_on_parole(&[id]);
        if !released.is_empty() {
            // A control-driven release happens mid-day, so clean the departed
            // pawn's auxiliary state now instead of waiting for the sweep.
            self.world.prune_departed_agent_state(&released);
        }
        self.pump_events_into_engine_log(before);
        let ok = released.first() == Some(&id);
        if ok {
            self.engine_state
                .parole
                .set_released_at(id, self.world.time);
            // Keep the engine post-release ledger coherent with the world's
            // authoritative release record (same prisoner, same reason).
            let risk = self.engine_state.parole.conditions(id).map_or(0.25, |c| {
                (0.1 + (c.reporting_per_month as f32 / 12.0) * 0.2
                    + if c.no_contact_order { 0.2 } else { 0.0 })
                .clamp(0.0, 1.0)
            });
            let plan = self
                .engine_state
                .parole
                .conditions(id)
                .is_some_and(|c| c.treatment_required || c.supervision_months >= 6);
            self.record_post_release(id, "parole", risk, plan);
            self.audit_control(
                "observer/control",
                "ReleaseOnParole",
                format!("Prisoner {} released on parole", id.raw()),
                "accepted",
                "grant executed through World::release_on_parole",
            );
        } else {
            self.audit_control_rejected(
                "observer/control",
                "ReleaseOnParole",
                format!("Release for prisoner {} refused", id.raw()),
                "prisoner not present in custody",
            );
        }
        ok
    }

    /// Get the parole status for a prisoner.
    pub fn parole_status(&self, id: PrisonerId) -> Option<ParoleStatus> {
        self.engine_state.parole.status(id)
    }

    /// Persisted conditions for a decided prisoner, if any.
    pub fn parole_conditions(&self, id: PrisonerId) -> Option<&ParoleConditions> {
        self.engine_state.parole.conditions(id)
    }

    /// Persisted decision reason for a prisoner, if any.
    pub fn parole_decision_reason(&self, id: PrisonerId) -> Option<&str> {
        self.engine_state.parole.decision_reason(id)
    }

    /// Sim time at which a granted prisoner left custody, if released.
    pub fn parole_released_at(&self, id: PrisonerId) -> Option<SimTime> {
        self.engine_state.parole.released_at(id)
    }

    // --- 4.6 Rehab (§14) ---

    /// Deterministic rehab progress: relief moves per minute of program
    /// participation (purpose, respect, stimulation, safety, companionship).
    pub fn apply_rehab_progress(&mut self, id: PrisonerId, minutes: u64) -> f32 {
        let Some(p) = self.world.prisoners.iter_mut().find(|p| p.id == id) else {
            return 0.0;
        };
        let m = minutes as f32;
        p.needs.relieve(NeedTag::Purpose, 0.05 * m);
        p.needs.relieve(NeedTag::Respect, 0.04 * m);
        p.needs.relieve(NeedTag::Stimulation, 0.04 * m);
        p.needs.relieve(NeedTag::Safety, 0.03 * m);
        p.needs.relieve(NeedTag::Companionship, 0.03 * m);
        self.rehab_score(id)
    }

    /// Rehab progress score 0..1 (aggregate of key positive needs).
    pub fn rehab_score(&self, id: PrisonerId) -> f32 {
        let Some(p) = self.world.prisoners.iter().find(|p| p.id == id) else {
            return 0.0;
        };
        let n = &p.needs;
        let deficit = (1.0 - n.purpose)
            + (1.0 - n.respect)
            + (1.0 - n.stimulation)
            + (1.0 - n.safety)
            + (1.0 - n.companionship);
        (1.0 - deficit / 5.0).clamp(0.0, 1.0)
    }

    // --- 4.7 Observation ---

    /// Per-agent read-only observation (§20, individual-agent mode).
    pub fn agent_observation(&self, id: PrisonerId) -> Option<AgentObservation> {
        let p = self.world.prisoners.iter().find(|p| p.id == id)?;
        Some(AgentObservation {
            id: p.id,
            name: p.name.clone(),
            age: p.age,
            classification: p.classification,
            needs: p.needs.clone(),
            condition: p.condition.clone(),
            goals: p.goals.clone(),
            gang_affiliation: p.gang_affiliation,
            aggression_modifier: p.aggression_modifier(),
            social_drive: p.social_drive(),
            memory_summary: p.memory_summary.clone(),
            current_intention: self
                .world
                .current_decisions
                .get(&id)
                .map(|d| format!("{:?}", d)),
        })
    }

    /// Leaderboard: top `n` by reputation.
    pub fn leader_board(&self, n: usize) -> Vec<(String, f32)> {
        let mut v: Vec<(String, f32)> = self
            .world
            .prisoners
            .iter()
            .map(|p| (p.name.clone(), p.reputation))
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(n);
        v
    }

    // --- 4.8 Determinism (§22) ---

    /// FNV-1a hash of (time, population, recent event summaries, incident sidecar) —
    /// a legacy trace fingerprint AI determinism/replay checks can compare across runs.
    ///
    /// For a fingerprint that also covers the full engine state, use
    /// [`Self::trace_hash_v2`], which mixes in `state_digest()`. Keeping the two
    /// separate preserves the historical trace format for existing seeds.
    pub fn trace_hash(&self) -> u64 {
        // Original trace hash components (backward compatible)
        let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a offset basis

        let mut mix = |s: &str| {
            for b in s.bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };

        mix(&format!(
            "t:{}p:{}",
            self.world.time.secs,
            self.world.population()
        ));
        for e in self.world.events.recent() {
            mix(&format!("{:?}:{}@{}", e.kind, e.summary, e.at.secs));
        }

        // Include sidecar incident causality for trace_hash compatibility
        for inc in &self.engine_state.incident_sidecar {
            let cause_str = inc.cause.map(|c| c.to_string()).unwrap_or_default();
            let conj_str = inc.consequence.map(|c| c.to_string()).unwrap_or_default();
            let kind_str = format!("{:?}", inc.kind);
            mix(&format!(
                "inc:{}:{}:{}:{}@{}",
                kind_str,
                inc.severity.to_bits(),
                cause_str,
                conj_str,
                inc.timestamp.secs
            ));
            for pid in &inc.involved_prisoners {
                mix(&format!("prisoner:{}", pid.raw()));
            }
        }

        h
    }

    /// Richer trace fingerprint that additionally mixes in the full state digest (R6).
    ///
    /// Use this when comparing full world state across runs. [`Self::trace_hash`]
    /// remains the legacy FNV fingerprint of time/population/events/sidecar.
    pub fn trace_hash_v2(&self) -> u64 {
        let mut h = self.trace_hash();
        let state_digest = crate::state_digest::compute_full_state_digest(self);
        h ^= state_digest;
        h.wrapping_mul(0x0000_0100_0000_01b3)
    }
}

#[cfg(test)]
mod dto_tests {
    use super::*;

    #[test]
    fn schedule_timeline_returns_resolved_blocks() {
        let sim = Sim::new(41);
        let id = sim.world.prisoners[0].id;
        let timeline = sim.schedule_timeline(id).expect("prisoner exists");
        assert_eq!(timeline.prisoner_id, id);
        assert!(!timeline.blocks.is_empty());
        assert!(timeline.blocks.iter().any(|b| b.activity == "Sleep"));
        let cancelled = timeline.blocks.iter().filter(|b| b.cancelled).count() as u32;
        assert_eq!(timeline.cancelled_blocks, cancelled);
    }

    #[test]
    fn budget_snapshot_matches_economy_cash() {
        let sim = Sim::new(42);
        let snapshot = sim.budget_snapshot();
        assert!((snapshot.cash_major - sim.world.economy.cash.as_major()).abs() < f64::EPSILON);
        assert!((0.0..=1.0).contains(&snapshot.health));
        assert_eq!(snapshot.population, sim.world.population());
        assert_eq!(snapshot.window_days, 30);
    }

    #[test]
    fn rehab_controls_are_validated() {
        let mut sim = Sim::new(43);
        let id = sim.world.prisoners[0].id;
        // Dropping when not enrolled must be a validating no-op.
        assert!(!sim.drop_rehab(id, RehabProgram::Education));
        // Enrolling is willingness-gated; if it succeeds, dropping must work once.
        if sim.enroll_rehab(id, RehabProgram::Education) {
            assert!(sim.rehab_summaries().iter().any(|r| r.prisoner_id == id));
            assert!(sim.drop_rehab(id, RehabProgram::Education));
            assert!(!sim.drop_rehab(id, RehabProgram::Education));
        }
        // Unknown prisoner is always rejected.
        assert!(!sim.enroll_rehab(PrisonerId(99_999), RehabProgram::Counseling));
    }

    #[test]
    fn contraband_summaries_come_from_world_case_store() {
        use alc_social::contraband::{
            ConfiscationDisposition, ContrabandCaseRecord, DisciplinaryAction,
        };

        let mut sim = Sim::new(48);
        let holder = sim.world.prisoners[0].id;
        sim.world.contraband_cases.push(ContrabandCaseRecord {
            case_id: 77,
            discovered_at: sim.world.time,
            discovering_guard: GuardId(7),
            holder,
            item: alc_social::ContrabandItem::Phones,
            severity: 0.55,
            disposition: ConfiscationDisposition::Seized,
            disciplinary_action: Some(DisciplinaryAction::Warning),
            summary: "Case 77: Phones found".to_string(),
        });

        let rows = sim.contraband_summaries();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].case_id, 77);
        assert_eq!(rows[0].item.as_deref(), Some("Phones"));
        assert_eq!(rows[0].holder, Some(holder));
        assert_eq!(rows[0].guard, Some(7));
        assert!(!rows[0].resolved);
        assert!(rows[0].causes.iter().any(|c| c == "discovery by guard 7"));

        assert!(
            !sim.resolve_contraband_case(999),
            "unknown case must be rejected"
        );
        assert!(sim.resolve_contraband_case(77));
        assert!(!sim.resolve_contraband_case(77));
        let resolved = sim.contraband_summaries();
        assert!(resolved[0].resolved);
        assert_eq!(resolved[0].resolved_at, Some(sim.world.time));
        assert!(sim.world.resolved_contraband_at.contains_key(&77));
    }

    #[test]
    fn load_migrates_legacy_contraband_cases_and_resolution_ids() {
        let dir = std::env::temp_dir().join(format!(
            "alcagent-contraband-legacy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let mut sim = Sim::new(49);
        let holder = sim.world.prisoners[0].id;
        sim.emit_event(
            WorldEvent::new(
                0,
                sim.world.time,
                EventKind::ContrabandFound,
                "Phones found in cell",
            )
            .with_actor(holder.raw())
            .with_cause("guard 7 search"),
        );
        let event_id = sim
            .world
            .events
            .recent()
            .last()
            .expect("discovery event")
            .id;
        // Reshape the save into the pre-case-store format: events and engine
        // resolution ids exist, but the durable case store does not.
        sim.world.contraband_cases.clear();
        sim.world.next_contraband_case_id = 1;
        sim.world
            .resolved_contraband_at
            .insert(event_id, sim.world.time);
        sim.engine_state.resolved_contraband_cases.insert(event_id);
        sim.engine_state
            .resolved_contraband_at
            .insert(event_id, sim.world.time);
        sim.engine_state.resolved_contraband_cases.insert(999_999);
        sim.engine_state
            .resolved_contraband_at
            .insert(999_999, sim.world.time);
        sim.save(&dir).expect("save legacy-shaped world");

        let mut loaded = Sim::load(&dir).expect("load legacy-shaped world");
        let rows = loaded.contraband_summaries();
        assert_eq!(rows.len(), 1, "legacy event must become a durable case");
        assert_eq!(rows[0].case_id, event_id);
        assert!(rows[0].resolved, "legacy resolution must survive load");
        assert!(
            loaded
                .engine_state
                .resolved_contraband_cases
                .contains(&999_999),
            "engine-only resolutions remain part of the save contract"
        );
        assert!(loaded.world.next_contraband_case_id > 999_999);

        loaded.emit_event(
            WorldEvent::new(
                0,
                loaded.world.time,
                EventKind::ContrabandFound,
                "Money found in cell",
            )
            .with_actor(holder.raw())
            .with_cause("guard 8 search"),
        );
        let new_case = loaded
            .world
            .contraband_cases
            .last()
            .expect("new legacy bridge case");
        assert_ne!(new_case.case_id, event_id);
        assert!(
            new_case.case_id > 999_999,
            "new allocation must skip reserved legacy ids"
        );
        assert!(
            !loaded
                .contraband_summaries()
                .iter()
                .any(|row| row.case_id == new_case.case_id && row.resolved),
            "fresh case must not inherit a stale resolution"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incident_filter_end_to_end() {
        let mut sim = Sim::new(44);
        let id = sim.world.prisoners[0].id;
        sim.record_sidecar_incident(
            incidents::Incident::new(
                EventKind::Fight,
                sim.world.time,
                "Yard A".to_string(),
                vec![id],
            )
            .severity(0.9),
        );
        let filter = incidents::IncidentFilter::all()
            .with_kinds(vec![EventKind::Fight])
            .with_prisoner(id)
            .with_min_severity(0.5);
        let hits = sim.incident_records_filtered(&filter);
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].resolved);
        let incident_id = hits[0].id;
        assert!(sim.resolve_incident(incident_id));
        assert!(!sim.resolve_incident(incident_id));
        assert!(sim.incidents_filtered(&filter)[0].resolved);
    }

    #[test]
    fn post_release_record_roundtrip() {
        let mut sim = Sim::new(45);
        let id = sim.world.prisoners[0].id;
        assert!(sim.record_post_release(id, "parole", 0.2, true));
        let rows = sim.post_release_records();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].release_type, "post_release");
        assert!(rows[0].successful_reintegration);
        assert_eq!(rows[0].recidivism_risk, Some(0.2));
        assert_eq!(rows[0].has_reintegration_plan, Some(true));
        assert!(!rows[0].follow_up_required);
    }

    #[test]
    fn public_conflict_emission_uses_real_world_event_ids_and_dedupes() {
        let mut sim = Sim::new(46);
        let actor = sim.world.prisoners[0].id.raw();
        let sidecar_before = sim.incident_sidecar().len();
        let log_before = sim.incident_log().len();

        let mut events = vec![
            WorldEvent::new(0, sim.world.time, EventKind::Fight, "audit: yard fight")
                .with_actor(actor),
        ];
        sim.emit_events(&mut events);

        let first_id = events[0].id;
        assert!(first_id > 0, "world bus must assign an authoritative id");
        assert!(sim.events().recent().any(|e| e.id == first_id));
        assert_eq!(sim.incident_sidecar().len(), sidecar_before + 1);
        assert_eq!(sim.incident_log().len(), log_before + 1);

        let first_incident = sim
            .incident_sidecar()
            .iter()
            .find(|inc| {
                inc.causal_chain
                    .iter()
                    .any(|link| !link.is_incident && link.source_id == first_id)
            })
            .expect("mirrored incident must reference its world event id");
        let first_incident_id = first_incident.id;

        // Replaying the same event through the mirror path is a no-op: dedupe
        // is by world event id, not by free-text summary or actor list.
        let sidecar_len = sim.incident_sidecar().len();
        let log_len = sim.incident_log().len();
        sim.mirror_conflict_event_to_engine_log(&events[0]);
        assert_eq!(sim.incident_sidecar().len(), sidecar_len);
        assert_eq!(sim.incident_log().len(), log_len);

        // A distinct public emission gets a new id and a distinct incident.
        let mut second = vec![
            WorldEvent::new(0, sim.world.time, EventKind::Threat, "audit: cell threat")
                .with_actor(actor),
        ];
        sim.emit_events(&mut second);
        assert_ne!(second[0].id, first_id);
        assert_eq!(sim.incident_sidecar().len(), sidecar_len + 1);
        assert!(sim.incident_sidecar().iter().any(|inc| {
            inc.causal_chain
                .iter()
                .any(|link| !link.is_incident && link.source_id == second[0].id)
        }));
        assert_ne!(sim.incident_sidecar().last().unwrap().id, first_incident_id);
    }

    #[test]
    fn incident_trim_scrubs_dangling_world_event_links() {
        let mut sim = Sim::new(47);
        let live_event = WorldEvent::new(7_001, sim.world.time, EventKind::Fight, "live event");
        sim.engine_state.incidents.push(live_event.clone());

        let max = crate::invariants::MAX_LOG_ENTRIES as u64;
        for id in 2..=(max + 10) {
            let causal_chain = if id == max + 10 {
                vec![
                    incidents::CausalLink::world_event(live_event.id, "live evidence"),
                    incidents::CausalLink::world_event(9_999, "gone evidence"),
                ]
            } else {
                Vec::new()
            };
            sim.engine_state.incident_sidecar.push(incidents::Incident {
                id,
                causal_chain,
                timestamp: sim.world.time,
                ..Default::default()
            });
        }

        // Trigger the bounded-log cleanup through a public recording path.
        sim.record_incident(WorldEvent::new(
            0,
            sim.world.time,
            EventKind::Argument,
            "trim",
        ));

        let live_events: std::collections::HashSet<u64> = sim
            .engine_state
            .incidents
            .iter()
            .map(|event| event.id)
            .collect();
        let mut kept_live_link = false;
        for inc in &sim.engine_state.incident_sidecar {
            for link in &inc.causal_chain {
                if link.is_incident {
                    continue;
                }
                assert!(
                    live_events.contains(&link.source_id),
                    "dangling world-event link {} in incident {}",
                    link.source_id,
                    inc.id
                );
                kept_live_link |= link.source_id == live_event.id;
            }
        }
        assert!(
            kept_live_link,
            "resolvable event provenance must be preserved"
        );
        assert!(sim.check_invariants().is_empty());
    }
}
