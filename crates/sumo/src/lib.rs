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
//! (GUI feature removed - use alc-ui crate instead)

#![deny(missing_docs)]
#![deny(unused_crate_dependencies)]

pub mod autosave;

pub mod decisions;

/// Re-export decision types for convenience and doctest accessibility.
pub use alc_agents::ValidationResult;
pub use crate::decisions::{DecisionProposal, DecisionType};

/// Council of drives: deterministic institutional decision evaluation (R3).
pub mod council;

pub mod drivers;

pub mod map_frame;
/// Re-export map frame types for UI rendering.
pub use crate::map_frame::{MapFrame, PawnPosition, NavEdge};

pub mod observation;
/// Re-export observation types for UI and external consumers.
pub use crate::observation::{Observation, Stats, AgentObservation};

pub mod parole;

pub mod incidents;

/// Re-export structured incident query types for UI/CLI consumers (0.05).
pub use crate::incidents::{filter_incidents, Incident, IncidentFilter};

pub mod invariants;

pub mod state_digest;

pub use alc_world::World;

/// Re-export core types for UI and external consumers.
pub use alc_core::SimTime;
pub use alc_events::WorldEvent;

use alc_agents::Decision;
use alc_core::{GuardId, PrisonerId, TimeSpeed};
use alc_events::{EventBus, EventKind};
use alc_guard::GuardReport;
use alc_persistence::{SaveError, SaveManager};
use alc_prisoner::{
    attendance_probability, current_block, resolve_schedule, AttendanceContext, NeedTag,
    SchedulePolicy, ScheduleTemplate,
};
use alc_social::rehab::{enroll_in_program, RehabPrisonerState, RehabProgram};
use alc_warden::WardenAction;
use std::collections::{BTreeMap, HashSet, HashMap};
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};

/// The prison's policy levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Policy {
    /// Normal operation with standard security.
    Normal,
    /// Enhanced security measures in effect.
    Enhanced,
    /// Prison is under lockdown.
    Lockdown,
}

/// The mode for engine-side decision-making.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
pub enum PolicyMode {
    /// Full determinism: every tick is reproducible given the same seed.
    #[default]
    Deterministic,
    /// Council mode: warden decisions are made by the agent council.
    Council,
    /// Shadow mode: alternative policy path for comparison/A-B testing.
    Shadow,
}

impl Default for Policy {
    fn default() -> Self {
        Policy::Normal
    }
}

/// The prison's policy and lockdown state (H1: policy management).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyState {
    /// Current policy level.
    pub policy: Policy,
    /// Whether the prison is under lockdown.
    pub lockdown: bool,
    /// Security level index (0-5, where 5 is maximum security).
    pub security_level: u32,
    /// Timestamp when policy was last changed.
    pub last_changed: alc_core::SimTime,
}

/// Observer-facing policy summary for UI/control panels (0.05).
#[derive(Debug, Clone, Default)]
pub struct PolicySummary {
    /// Current policy band.
    pub policy: Policy,
    /// Lockdown flag.
    pub lockdown: bool,
    /// Security level 0.0-1.0.
    pub security_level: f32,
    /// Privileges level 0.0-1.0.
    pub privileges_level: f32,
    /// Rehab funding multiplier 0.0-2.0.
    pub rehab_funding_multiplier: f32,
    /// Intake paused by lockdown/max security.
    pub intake_paused: bool,
}

impl Default for PolicyState {
    fn default() -> Self {
        PolicyState {
            policy: Policy::Normal,
            lockdown: false,
            security_level: 1,
            last_changed: alc_core::SimTime::default(),
        }
    }
}

/// Structured decision audit record for observer UI (0.05).
#[derive(Debug, Clone, Default)]
pub struct DecisionAuditRecord {
    /// Simulation time when the decision was recorded.
    pub time: SimTime,
    /// Actor that produced the decision (warden, prisoner:ID, guard:ID).
    pub actor: String,
    /// Decision source/mode (deterministic, council, shadow, agent).
    pub source: String,
    /// Action name or type.
    pub action: String,
    /// Human-readable summary.
    pub summary: String,
    /// Validation outcome (accepted, repaired, rejected).
    pub validation: String,
    /// Reason or engine note.
    pub reason: String,
}

/// Structured incident record for observer UI (0.05).
#[derive(Debug, Clone, Default)]
pub struct IncidentRecord {
    /// Incident id.
    pub id: u64,
    /// Simulation time.
    pub at: SimTime,
    /// Kind label.
    pub kind: String,
    /// Severity 0.0-1.0.
    pub severity: f32,
    /// Location string.
    pub location: String,
    /// Involved prisoner ids.
    pub involved_prisoners: Vec<u64>,
    /// Cause incident id.
    pub cause: Option<u64>,
    /// Consequence incident id.
    pub consequence: Option<u64>,
    /// Resolved flag.
    pub resolved: bool,
}

impl From<&incidents::Incident> for IncidentRecord {
    fn from(incident: &incidents::Incident) -> Self {
        IncidentRecord {
            id: incident.id,
            at: incident.timestamp,
            kind: format!("{:?}", incident.kind),
            severity: incident.severity,
            location: incident.location.clone(),
            involved_prisoners: incident.involved_prisoners.iter().map(|p| p.raw()).collect(),
            cause: incident.cause,
            consequence: incident.consequence,
            resolved: incident.resolved,
        }
    }
}

/// Engine state for persistence (S0: prerequisite save/load).
pub struct EngineState {
    /// Parole ledger (Directive §15).
    pub parole: ParoleLedger,
    /// Population history for invariant checking.
    pub population_history: Vec<(SimTime, u32)>,
    /// Autosave configuration.
    pub autosave: Option<AutosaveScheduler>,
    /// World incident log (R1: incident reports).
    /// Typed world events for the UI Incident Reports panel.
    pub incidents: Vec<alc_events::WorldEvent>,
    /// Sidecar incident log (R1/R6: structured incidents with causality).
    /// Parallel Vec<Incident> that mirrors EventKind events but adds
    /// causality fields (cause/consequence chains) for traceability.
    pub incident_sidecar: Vec<incidents::Incident>,
    /// Decision audit log (R2: decision audit log).
    pub decision_log: Vec<alc_agents::Decision>,
    /// Structured decision audit records for observer UI (0.05).
    pub decision_audit: Vec<DecisionAuditRecord>,
    /// Contraband case IDs manually resolved by observer/control (0.05).
    pub resolved_contraband_cases: std::collections::BTreeSet<u64>,
    /// Post-release tracking (R4: released prisoner outcomes).
    pub post_release: HashMap<alc_core::PrisonerId, bool>,
    /// Policy and lockdown state (H1: policy management).
    pub policy_state: PolicyState,
    /// Policy mode for QA shadow/Deterministic mode (S3).
    pub policy_mode: PolicyMode,
}

/// Convenience: EngineState default with all fields initialized.
impl Default for EngineState {
    fn default() -> Self {
        EngineState {
            parole: ParoleLedger::default(),
            population_history: Vec::new(),
            autosave: None,
            incidents: Vec::new(),
            incident_sidecar: Vec::new(),
            decision_log: Vec::new(),
            decision_audit: Vec::new(),
            resolved_contraband_cases: std::collections::BTreeSet::new(),
            post_release: std::collections::HashMap::new(),
            policy_state: PolicyState::default(),
            policy_mode: PolicyMode::Deterministic,
        }
    }
}

impl EngineState {
    /// Convert sumo's EngineState to alc_persistence's EngineState for saving.
    pub fn to_persistence(&self) -> alc_persistence::EngineState {
        alc_persistence::EngineState {
            parole: alc_persistence::ParoleLedger {
                status: self.parole.status.iter().map(|(k, v)| (k.raw(), parole_status_to_persist(*v))).collect(),
                adjustments: self.parole.adjustments.iter().map(|(k, v)| (k.raw(), *v)).collect(),
                hearing_at: self.parole.hearing_at.iter().map(|(k, v)| (k.raw(), *v)).collect(),
                last_decision_at: self.parole.last_decision_at,
            },
            population_history: self.population_history.iter().map(|(t, p)| (t.secs, *p)).collect(),
            incidents: self.incidents.clone(),
            incident_sidecar: self.incident_sidecar.iter().map(|inc| {
                alc_persistence::Incident {
                    id: inc.id,
                    kind: inc.kind,
                    severity: inc.severity,
                    cause: inc.cause,
                    consequence: inc.consequence,
                    timestamp: inc.timestamp,
                    location: inc.location.clone(),
                    resolved: inc.resolved,
                    involved_prisoners: inc.involved_prisoners.iter().map(|p| p.raw()).collect(),
                    involved_guards: Vec::new(), // sumo's Incident doesn't track guards
                }
            }).collect(),
            decision_log: self.decision_log.clone(),
            decision_audit: self.decision_audit.iter().map(|r| alc_persistence::DecisionAuditRecord {
                time: r.time,
                actor: r.actor.clone(),
                source: r.source.clone(),
                action: r.action.clone(),
                summary: r.summary.clone(),
                validation: r.validation.clone(),
                reason: r.reason.clone(),
            }).collect(),
            resolved_contraband_cases: self.resolved_contraband_cases.iter().copied().collect(),
            post_release: self.post_release.iter().map(|(k, v)| (k.raw(), *v)).collect(),
            autosave_interval_days: self.autosave.as_ref().map(|a| a.interval_days),
            last_autosave_day: self.autosave.as_ref().map(|a| a.last_save_day()),
            policy_state: alc_persistence::PolicyState {
                policy: policy_to_persist(self.policy_state.policy),
                lockdown: self.policy_state.lockdown,
                security_level: self.policy_state.security_level,
                last_changed: self.policy_state.last_changed,
            },
            policy_mode: policy_mode_to_persist(self.policy_mode),
        }
    }

    /// Convert alc_persistence's EngineState to sumo's EngineState for loading.
    pub fn from_persistence(persist: alc_persistence::EngineState, save_dir: &Path) -> Self {
        EngineState {
            parole: ParoleLedger {
                status: persist.parole.status.into_iter().map(|(k, v)| (PrisonerId(k), parole_status_from_persist(v))).collect(),
                adjustments: persist.parole.adjustments.into_iter().map(|(k, v)| (PrisonerId(k), v)).collect(),
                hearing_at: persist.parole.hearing_at.into_iter().map(|(k, v)| (PrisonerId(k), v)).collect(),
                last_decision_at: persist.parole.last_decision_at,
            },
            population_history: persist.population_history.into_iter().map(|(t, p)| (SimTime::from_secs(t), p)).collect(),
            autosave: persist.autosave_interval_days.map(|days| {
                let mut s = AutosaveScheduler::new(save_dir.to_path_buf(), days);
                if let Some(d) = persist.last_autosave_day { s = s.with_last_save_day(d); }
                s
            }),
            incidents: persist.incidents,
            incident_sidecar: persist.incident_sidecar.into_iter().map(|inc| {
                incidents::Incident {
                    id: inc.id,
                    kind: inc.kind,
                    cause: inc.cause,
                    consequence: inc.consequence,
                    timestamp: inc.timestamp,
                    location: inc.location,
                    involved_prisoners: inc.involved_prisoners.into_iter().map(PrisonerId).collect(),
                    severity: inc.severity,
                    resolved: inc.resolved,
                }
            }).collect(),
            decision_log: persist.decision_log,
            decision_audit: persist.decision_audit.into_iter().map(|r| DecisionAuditRecord {
                time: r.time,
                actor: r.actor,
                source: r.source,
                action: r.action,
                summary: r.summary,
                validation: r.validation,
                reason: r.reason,
            }).collect(),
            resolved_contraband_cases: persist.resolved_contraband_cases.into_iter().collect(),
            post_release: persist.post_release.into_iter().map(|(k, v)| (PrisonerId(k), v)).collect(),
            policy_state: PolicyState {
                policy: policy_from_persist(persist.policy_state.policy),
                lockdown: persist.policy_state.lockdown,
                security_level: persist.policy_state.security_level,
                last_changed: persist.policy_state.last_changed,
            },
            policy_mode: policy_mode_from_persist(persist.policy_mode),
        }
    }
}

/// Helper to convert ParoleStatus to persistence enum.
fn parole_status_to_persist(status: ParoleStatus) -> alc_persistence::ParoleStatus {
    match status {
        ParoleStatus::None => alc_persistence::ParoleStatus::None,
        ParoleStatus::Scheduled => alc_persistence::ParoleStatus::Scheduled,
        ParoleStatus::Granted => alc_persistence::ParoleStatus::Granted,
        ParoleStatus::Denied => alc_persistence::ParoleStatus::Denied,
        ParoleStatus::InHearing => alc_persistence::ParoleStatus::InHearing,
    }
}

/// Helper to convert ParoleStatus from persistence enum.
fn parole_status_from_persist(status: alc_persistence::ParoleStatus) -> ParoleStatus {
    match status {
        alc_persistence::ParoleStatus::None => ParoleStatus::None,
        alc_persistence::ParoleStatus::Scheduled => ParoleStatus::Scheduled,
        alc_persistence::ParoleStatus::Granted => ParoleStatus::Granted,
        alc_persistence::ParoleStatus::Denied => ParoleStatus::Denied,
        alc_persistence::ParoleStatus::InHearing => ParoleStatus::InHearing,
    }
}

/// Helper to convert Policy to persistence enum.
fn policy_to_persist(policy: Policy) -> alc_persistence::Policy {
    match policy {
        Policy::Normal => alc_persistence::Policy::Normal,
        Policy::Enhanced => alc_persistence::Policy::Enhanced,
        Policy::Lockdown => alc_persistence::Policy::Lockdown,
    }
}

/// Helper to convert Policy from persistence enum.
fn policy_from_persist(policy: alc_persistence::Policy) -> Policy {
    match policy {
        alc_persistence::Policy::Normal => Policy::Normal,
        alc_persistence::Policy::Enhanced => Policy::Enhanced,
        alc_persistence::Policy::Lockdown => Policy::Lockdown,
    }
}

/// Helper to convert PolicyMode to persistence enum.
fn policy_mode_to_persist(mode: PolicyMode) -> alc_persistence::PolicyMode {
    match mode {
        PolicyMode::Deterministic => alc_persistence::PolicyMode::Deterministic,
        PolicyMode::Council => alc_persistence::PolicyMode::Council,
        PolicyMode::Shadow => alc_persistence::PolicyMode::Shadow,
    }
}

/// Helper to convert PolicyMode from persistence enum.
fn policy_mode_from_persist(mode: alc_persistence::PolicyMode) -> PolicyMode {
    match mode {
        alc_persistence::PolicyMode::Deterministic => PolicyMode::Deterministic,
        alc_persistence::PolicyMode::Council => PolicyMode::Council,
        alc_persistence::PolicyMode::Shadow => PolicyMode::Shadow,
    }
}

/// The simulation engine: owns the world and drives it over time.
pub struct Sim {
    /// The world state, owned by the engine.
    pub world: World,
    speed: TimeSpeed,
    /// Engine persistence state (S0: save/load, logs, policy mode).
    /// This is the SINGLE SOURCE OF TRUTH for all engine-side state.
    pub engine_state: EngineState,
}

impl Sim {
    /// Create a new simulation from a seed. The world boots with the starting
    /// facility set, guards and prisoners (Directive §2 bootstrap).
    pub fn new(seed: u64) -> Self {
        Self {
            world: World::new(seed),
            speed: TimeSpeed::Paused,
            engine_state: EngineState::default(),
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

    /// Record a structured decision audit entry.
    pub fn record_decision_audit(&mut self, record: DecisionAuditRecord) {
        self.engine_state.decision_audit.push(record);
        self.trim_engine_logs();
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
        self.engine_state.policy_mode = mode;
    }

    

    /// Set the simulation speed.
    pub fn set_speed(&mut self, speed: TimeSpeed) {
        self.speed = speed;
        self.world.speed = speed;
    }
    /// Get the current simulation speed.
    pub fn speed(&self) -> TimeSpeed { self.speed }

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
                new_audits.push(DecisionAuditRecord {
                    time: self.world.time,
                    actor: format!("prisoner:{}", pid.raw()),
                    source: "world_tick".to_string(),
                    action: decision.action_name().to_string(),
                    summary: format!("{:?}", decision),
                    validation: "accepted".to_string(),
                    reason: "applied by world tick".to_string(),
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
        let _ = self.maybe_autosave();
    }

    /// Copy newly-emitted conflict events into the engine incident log and
    /// structured sidecar (C1). Non-conflict events stay on the world bus only
    /// so the engine audit store remains bounded and meaningful. Conflict
    /// incidents use a self-referential cause/consequence pair until a real
    /// chain is built (same convention as `decisions::deliver`).
    fn pump_events_into_engine_log(&mut self, after_id: u64) {
        use alc_events::EventKind;
        let new: Vec<WorldEvent> = self
            .world
            .events
            .recent()
            .filter(|e| e.id > after_id)
            .cloned()
            .collect();
        for event in new {
            if !matches!(
                event.kind,
                EventKind::Fight
                    | EventKind::Argument
                    | EventKind::Threat
                    | EventKind::Theft
                    | EventKind::GangClash
                    | EventKind::Protests
                    | EventKind::PropertyDamage
                    | EventKind::MedicalEmergency
                    | EventKind::EscapeAttempt
                    | EventKind::EscapePrevented
                    | EventKind::EscapeSucceeded
                    | EventKind::ContrabandFound
                    | EventKind::Death
                    | EventKind::TransferOut
                    | EventKind::ConstructionComplete
            ) {
                continue;
            }
            let id = self
                .engine_state
                .incident_sidecar
                .iter()
                .map(|inc| inc.id)
                .max()
                .unwrap_or(0)
                + 1;
            let involved_prisoners = event
                .actors
                .iter()
                .copied()
                .filter(|&a| self.world.prisoners.iter().any(|p| p.id.raw() == a))
                .map(alc_core::PrisonerId)
                .collect();
            let severity = match event.kind {
                EventKind::Fight | EventKind::GangClash | EventKind::EscapeSucceeded | EventKind::Death => 0.9,
                EventKind::EscapeAttempt | EventKind::MedicalEmergency | EventKind::TransferOut => 0.7,
                EventKind::Threat | EventKind::Theft | EventKind::ContrabandFound => 0.6,
                EventKind::ConstructionComplete => 0.2,
                _ => 0.4,
            };
            self.engine_state.incident_sidecar.push(incidents::Incident {
                id,
                kind: event.kind,
                cause: Some(id),
                consequence: Some(id),
                timestamp: event.at,
                location: format!("facility {}", event.actors.first().copied().unwrap_or(0)),
                involved_prisoners,
                severity,
                resolved: false,
            });
            self.engine_state.incidents.push(event);
        }
        self.trim_engine_logs();
    }

    /// Keep engine audit stores within the bounded invariant limit (R6).
    fn trim_engine_logs(&mut self) {
        const MAX: usize = crate::invariants::MAX_LOG_ENTRIES;
        if self.engine_state.incidents.len() > MAX {
            let excess = self.engine_state.incidents.len() - MAX;
            self.engine_state.incidents.drain(..excess);
        }
        if self.engine_state.incident_sidecar.len() > MAX {
            let excess = self.engine_state.incident_sidecar.len() - MAX;
            self.engine_state.incident_sidecar.drain(..excess);
        }
        if self.engine_state.decision_log.len() > MAX {
            let excess = self.engine_state.decision_log.len() - MAX;
            self.engine_state.decision_log.drain(..excess);
        }
        if self.engine_state.decision_audit.len() > MAX {
            let excess = self.engine_state.decision_audit.len() - MAX;
            self.engine_state.decision_audit.drain(..excess);
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
    pub fn world(&self) -> &World { &self.world }
    /// Get a mutable reference to the world state.
    pub fn world_mut(&mut self) -> &mut World { &mut self.world }
    /// Get the number of days simulated.
    pub fn days_simulated(&self) -> u64 { self.world.days_simulated }

    /// Get the world's event bus.
    pub fn events(&self) -> &EventBus { &self.world.events }

    /// Get events since a point in time.
    pub fn events_since(&self, at: SimTime) -> Vec<WorldEvent> {
        self.world.events.since(at).cloned().collect()
    }

    /// Validated decision pipeline: agents propose, the engine disposes.
    pub fn deliver_decision(&mut self, id: PrisonerId, decision: Decision) -> DecisionOutcome {
        decisions::deliver(self, id, decision)
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
        // Use the single authoritative validation context so dry-runs agree
        // with the real decision pipeline (R2 acceptance).
        let ctx = crate::decisions::validation_context(&self.world, prisoner_id);

        // Generate a rationale based on the prisoner's traits and needs.
        let rationale = self.generate_rationale(prisoner, decision_type, target);

        // Build a temporary Decision from the DecisionType and target,
        // then validate it through the pipeline.
        let decision = decision_type.to_decision(target);
        let estimated_outcome = alc_agents::validate_decision(&decision, &ctx);
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
    pub fn validate_only(&self, prisoner_id: PrisonerId, proposal: &crate::decisions::DecisionProposal) -> ValidationResult {
        // Verify the prisoner exists before building a context.
        if !self.world.prisoners.iter().any(|p| p.id == prisoner_id) {
            return ValidationResult::Rejected {
                reason: format!("unknown prisoner id {}", prisoner_id.raw()),
            };
        }

        // Use the single authoritative validation context.
        let ctx = crate::decisions::validation_context(&self.world, prisoner_id);

        // Build the decision from the proposal and validate.
        let decision = proposal.decision_type.to_decision(proposal.target);
        alc_agents::validate_decision(&decision, &ctx)
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
            format!("Prisoner {} has no specific need driving this decision{target_note}", prisoner.id.raw())
        } else {
            let tags: Vec<String> = need_tags.iter().map(|t| format!("{:?}", t)).collect();
            format!("Prisoner {} decision driven by needs: {:?}{target_note}", prisoner.id.raw(), tags)
        }
    }

    /// Get a read-only snapshot of the world for observers and UI.
    pub fn observation(&self) -> Observation { Observation::capture(&self.world) }

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
        self.world.prisoners.iter().map(|p| PrisonerSummary {
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
        }).collect()
    }

    /// Get all gangs as summary types for the gang view.
    pub fn gangs(&self) -> Vec<GangSummary> {
        self.world.gangs.iter().map(|g| GangSummary {
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
            rivalries: g.rivalries.iter().map(|id| (id.0, 1.0)).collect(), // simplified
            alliances: g.alliances.iter().map(|id| (id.0, 1.0)).collect(), // simplified
            internal_disputes: g.internal_disputes.clone(),
            goals: g.goals.clone(),
        }).collect()
    }

    /// Get all guards as summary types for the staff view.
    pub fn guards(&self) -> Vec<GuardSummary> {
        self.world.guards.iter().map(|g| GuardSummary {
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
            history: g.history.clone(),
        }).collect()
    }

    /// Get the warden as summary for the warden view.
    pub fn warden(&self) -> WardenSummary {
        WardenSummary {
            name: self.world.warden.name.clone(),
            philosophy: self.world.warden.philosophy.clone(),
            traits: self.world.warden.traits.clone(),
            attention: self.world.warden.attention,
            inbox: Vec::new(), // Would need conversion from event ids to reports
            pending_actions: self.world.warden.pending_actions.clone(),
            last_decision_at: self.world.warden.last_decision_at,
        }
    }

    /// Get the economy summary for the financial view.
    pub fn economy(&self) -> EconomySummary {
        EconomySummary {
            cash_major: self.world.economy.cash.as_major(),
            health: self.world.economy.health(),
            funding_per_prisoner_per_day: self.world.economy.funding_per_prisoner_per_day.as_major(),
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

    /// Get facilities for the map view.
    pub fn facilities(&self) -> Vec<FacilitySummary> {
        self.world.facilities.iter().map(|f| FacilitySummary {
            id: f.id.0,
            kind: f.kind,
            wing: 0, // Would need to derive from location
            capacity: f.capacity,
            condition: f.condition,
            staff_required: 0, // Would need to calculate
        }).collect()
    }

    /// Construction queue for observer UI (0.05).
    ///
    /// Backed by `World::construction_queue`; entries disappear when a project
    /// completes (the facility is then in [`Sim::facilities`]).
    pub fn construction_queue(&self) -> Vec<ConstructionSummary> {
        self.world.construction_queue.iter().map(|p| ConstructionSummary {
            id: p.id,
            kind: p.kind,
            wing: p.wing,
            capacity: p.capacity,
            total_days: p.total_days,
            progress_days: p.progress_days,
            cost: p.cost,
            started_day: p.started_day,
        }).collect()
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

    /// Contraband cases derived from the event record (0.05).
    ///
    /// TODO(world-state): there is no persisted contraband inventory/case
    /// store yet — `alc-world` only emits `EventKind::ContrabandFound` events.
    /// This accessor therefore reconstructs cases from the bounded world event
    /// bus (plus the engine incident log), deduplicated by event id. Item and
    /// guard are best-effort parsed from the event summary/causes; `resolved`
    /// is always `false` until a real case store exists.
    pub fn contraband_summaries(&self) -> Vec<ContrabandCaseSummary> {
        let mut cases: Vec<ContrabandCaseSummary> = Vec::new();
        let mut seen: HashSet<u64> = HashSet::new();
        let bus_events = self.world.events.recent();
        let log_events = self.engine_state.incidents.iter();
        for event in bus_events.chain(log_events) {
            if event.kind != EventKind::ContrabandFound || !seen.insert(event.id) {
                continue;
            }
            let item = detect_contraband_item(&event.summary);
            let severity = item
                .as_deref()
                .map(contraband_severity_for_label)
                .unwrap_or(0.5);
            cases.push(ContrabandCaseSummary {
                case_id: event.id,
                at: event.at,
                item,
                holder: event.actors.first().copied().map(PrisonerId),
                guard: event.causes.iter().find_map(|c| parse_guard_cause(c)),
                severity,
                summary: event.summary.clone(),
                causes: event.causes.clone(),
                resolved: self.engine_state.resolved_contraband_cases.contains(&event.id),
            });
        }
        cases.sort_by_key(|c| c.case_id);
        cases
    }

    /// Mark a contraband case resolved (observer/control, 0.05).
    pub fn resolve_contraband_case(&mut self, case_id: u64) -> bool {
        self.engine_state.resolved_contraband_cases.insert(case_id)
    }

    /// Contraband inventory rollup by item category (0.05).
    ///
    /// Aggregates [`Sim::contraband_summaries`]; again event-derived only.
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
        if !recs.iter().any(|r| r.program == program && !r.completed && !r.dropped) {
            recs.push(alc_world::RehabProgramRecord {
                program,
                enrolled_day: day,
                progress_days: 0,
                progress_sum: 0.0,
                completed: false,
                dropped: false,
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
        true
    }

    /// Enroll by program key (UI/control convenience).
    pub fn enroll_rehab_by_key(&mut self, prisoner_id: PrisonerId, key: &str) -> bool {
        let Some(program) = REHAB_PROGRAMS.iter().copied().find(|p| p.key() == key) else { return false; };
        self.enroll_rehab(prisoner_id, program)
    }

    /// Drop a rehab program by key (UI/control convenience).
    pub fn drop_rehab_by_key(&mut self, prisoner_id: PrisonerId, key: &str) -> bool {
        let Some(program) = REHAB_PROGRAMS.iter().copied().find(|p| p.key() == key) else { return false; };
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
            if let Some(rec) = recs.iter_mut().rev().find(|r| r.program == program && !r.completed && !r.dropped) {
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
        true
    }

    /// Mark a structured sidecar incident resolved (observer control, 0.05).
    ///
    /// Validated: returns `false` for an unknown id or an already-resolved
    /// incident. Resolution persists with the sidecar incident and emits an
    /// `Investigation` event for the audit trail.
    pub fn resolve_incident(&mut self, incident_id: u64) -> bool {
        let at = self.world.time;
        let Some(incident) = self
            .engine_state
            .incident_sidecar
            .iter_mut()
            .find(|inc| inc.id == incident_id)
        else {
            return false;
        };
        if incident.resolved {
            return false;
        }
        incident.resolved = true;
        let kind = incident.kind;
        let event = WorldEvent::new(
            0,
            at,
            EventKind::Investigation,
            format!("Incident {incident_id} ({kind:?}) marked resolved"),
        )
        .with_cause("incident resolution by observer");
        self.emit_event(event);
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
        true
    }

    /// Compute the average population since a given time.
    fn avg_population_since(&self, at: SimTime) -> f64 {
        let pts: Vec<&(SimTime, u32)> = self.population_history().iter().filter(|(t, _)| *t >= at).collect();
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
        let dir = dir.as_ref().to_path_buf();
        let manager = SaveManager::new(&dir);
        let (world, engine_state) = manager.load_with_engine()?;

        Ok(Sim {
            world,
            speed: TimeSpeed::Paused,
            engine_state: engine_state.map(|es| EngineState::from_persistence(es, &dir)).unwrap_or_default(),
        })
    }

    /// Enable autosave: write the world every `interval_days` simulated days.
    pub fn enable_autosave(&mut self, dir: impl Into<PathBuf>, interval_days: u64) {
        self.engine_state.autosave = Some(AutosaveScheduler::new(dir, interval_days));
    }

    /// Disable autosave.
    pub fn disable_autosave(&mut self) { self.engine_state.autosave = None; }

    /// Save when the autosave interval has elapsed. Returns true when written.
    pub fn maybe_autosave(&mut self) -> Result<bool, SaveError> {
        let (due, dir) = match &self.engine_state.autosave {
            Some(a) if a.due(self.world.days_simulated) => (true, a.dir.clone()),
            _ => (false, PathBuf::new()),
        };
        if !due {
            return Ok(false);
        }
        self.save(&dir)?;
        if let Some(a) = &mut self.engine_state.autosave {
            a.mark_saved(self.world.days_simulated);
        }
        Ok(true)
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
        if !crate::invariants::invariant_policy_state_matches_world_policies(&self.engine_state, w) {
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
    pub fn set_post_release(&mut self, prisoner: alc_core::PrisonerId, success: bool) -> Option<bool> {
        self.engine_state.post_release.insert(prisoner, success)
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
                    self.world.policies.security_level = 1.0;
                    self.world.policies.privileges_level = 0.1;
                    self.engine_state.policy_state.lockdown = true;
                    self.engine_state.policy_state.security_level = 5;
                }
            }
            
            self.emit_event(WorldEvent::new(
                0,
                self.world.time,
                EventKind::PolicyChange,
                format!("Policy changed to {:?}", policy),
            ));
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
            
            // Sync with world.policies to affect simulation behavior
            self.world.policies.lockdown = lockdown;
            if lockdown {
                self.world.policies.security_level = (self.world.policies.security_level + 0.2).min(1.0);
                self.world.policies.privileges_level = (self.world.policies.privileges_level - 0.2).max(0.1);
                self.engine_state.policy_state.security_level = std::cmp::min(5, self.engine_state.policy_state.security_level + 1);
            } else {
                self.world.policies.security_level = (self.world.policies.security_level - 0.1).max(0.3);
                self.world.policies.privileges_level = (self.world.policies.privileges_level + 0.1).min(0.5);
            }
            
            self.emit_event(WorldEvent::new(
                0,
                self.world.time,
                EventKind::PolicyChange,
                format!("Lockdown {} to {:?}", if lockdown { "enabled" } else { "disabled" }, self.engine_state.policy_state.policy),
            ));
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
            intake_paused: self.world.policies.lockdown || self.world.policies.security_level >= 0.9 || self.world.policies.intake_paused,
        }
    }

    /// Set fine-grained security level (0.0-1.0) with validation and event feedback.
    pub fn set_security_level(&mut self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        let old = self.world.policies.security_level;
        if (old - level).abs() < f32::EPSILON { return; }
        self.world.policies.security_level = level;
        self.sync_policy_band_from_world();
        self.engine_state.policy_state.last_changed = self.world.time;
        self.emit_event(WorldEvent::new(0, self.world.time, EventKind::PolicyChange,
            format!("Security level {:.0}% -> {:.0}%", old * 100.0, level * 100.0)));
    }

    /// Set privileges level (0.0-1.0) with validation and event feedback.
    pub fn set_privileges_level(&mut self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        let old = self.world.policies.privileges_level;
        if (old - level).abs() < f32::EPSILON { return; }
        self.world.policies.privileges_level = level;
        self.engine_state.policy_state.last_changed = self.world.time;
        self.emit_event(WorldEvent::new(0, self.world.time, EventKind::PolicyChange,
            format!("Privileges {:.0}% -> {:.0}%", old * 100.0, level * 100.0)));
    }

    /// Set rehabilitation funding multiplier (0.0-2.0) with validation and event feedback.
    pub fn set_rehab_funding(&mut self, multiplier: f32) {
        let multiplier = multiplier.clamp(0.0, 2.0);
        let old = self.world.policies.rehab_funding_multiplier;
        if (old - multiplier).abs() < f32::EPSILON { return; }
        self.world.policies.rehab_funding_multiplier = multiplier;
        self.engine_state.policy_state.last_changed = self.world.time;
        self.emit_event(WorldEvent::new(0, self.world.time, EventKind::PolicyChange,
            format!("Rehab funding {:.2}x -> {:.2}x", old, multiplier)));
    }

    /// Manually pause or resume intake (0.05 policy control).
    pub fn set_intake_paused(&mut self, paused: bool) {
        if self.world.policies.intake_paused == paused { return; }
        self.world.policies.intake_paused = paused;
        self.engine_state.policy_state.last_changed = self.world.time;
        self.emit_event(WorldEvent::new(0, self.world.time, EventKind::PolicyChange,
            format!("Intake {}", if paused { "paused" } else { "resumed" })));
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
        s.push_str(&format!("population: {} / {}\n", w.population(), w.capacity()));
        s.push_str(&format!("guards: {}  facilities: {}\n", w.guards.len(), w.facilities.len()));
        s.push_str(&format!("warden: {} ({})\n", w.warden.name, w.warden.philosophy));
        s.push_str(&format!("cash: ${:.2}  health: {:.2}\n", w.economy.cash.as_major(), w.economy.health()));
        s.push_str(&format!("events: {}  speed: {:?}\n", w.events.len(), self.speed));
        s
    }

    /// Emit engine events with the current simulation time.
    pub(crate) fn emit_events(&mut self, events: Vec<WorldEvent>) {
        for mut e in events {
            e.at = self.world.time;
            self.world.events.emit(e);
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
    pub fn pause(&mut self) { self.set_speed(TimeSpeed::Paused); }
    /// Set the simulation speed to real time.
    pub fn resume(&mut self) { self.set_speed(TimeSpeed::RealTime); }

    /// Event-driven advance: tick until an event matching `pred` appears
    /// after `after`, or the simulation pauses. Returns the event if found.
    pub fn wait_for_event(&mut self, after: SimTime, pred: impl Fn(&WorldEvent) -> bool) -> Option<WorldEvent> {
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
    pub fn emit_event(&mut self, e: WorldEvent) {
        self.world.events.emit(e);
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

    // --- 4.4 Warden ---

    /// Warden cycle: arbitrate via `alc_warden::arbitrate` + apply.
    pub fn warden_cycle(&mut self) -> Vec<WardenAction> {
        drivers::warden_cycle(self)
    }

    /// Apply a WardenAction with the engine-side applier.
    pub fn apply_warden_action(&mut self, action: WardenAction) {
        let at = self.world.time;
        let summary = format!("{:?}", action);
        let events = drivers::apply_warden_action(&mut self.world, at, action.clone());
        self.emit_events(events);
        // The driver mutates `world.policies` directly (it only has &mut World),
        // so reconcile the engine-side policy mirror afterwards (R6: drift
        // between the two is an invariant violation).
        self.sync_policy_from_world();
        if let WardenAction::AdjustParoleRecommendation { prisoner, lean } = &action {
            self.engine_state.parole.set_adjustment(PrisonerId(*prisoner), *lean);
        }
        self.record_decision_audit(DecisionAuditRecord {
            time: at,
            actor: "warden".to_string(),
            source: format!("{:?}", self.engine_state.policy_mode),
            action: "WardenAction".to_string(),
            summary,
            validation: "accepted".to_string(),
            reason: "applied by engine".to_string(),
        });
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
    pub fn start_parole_hearing(&mut self, id: PrisonerId) -> Option<u64> {
        if !self.world.prisoners.iter().any(|p| p.id == id) {
            return None;
        }
        if self.engine_state.parole.status(id).is_some() {
            return None;
        }
        self.engine_state.parole.set(id, ParoleStatus::Scheduled, self.world.time);
        self.world.events.emit(
            WorldEvent::new(0, self.world.time, EventKind::ParoleHearing, format!("Parole hearing scheduled for prisoner {}", id.raw()))
                .with_actor(id.raw()),
        );
        self.world.events.recent().last().map(|e| e.id)
    }

    /// Grant parole (§15 step 10); returns the new event id.
    pub fn grant_parole(&mut self, id: PrisonerId) -> Option<u64> {
        if !self.world.prisoners.iter().any(|p| p.id == id) {
            return None;
        }
        if matches!(self.engine_state.parole.status(id), Some(ParoleStatus::Granted) | Some(ParoleStatus::Denied)) {
            return None;
        }
        if self.engine_state.parole.status(id).is_none() {
            self.engine_state.parole.set(id, ParoleStatus::InHearing, self.world.time);
        }
        self.engine_state.parole.set(id, ParoleStatus::Granted, self.world.time);
        self.world.events.emit(
            WorldEvent::new(0, self.world.time, EventKind::ParoleGranted, format!("Parole granted to prisoner {}", id.raw())).with_actor(id.raw()),
        );
        self.world.events.recent().last().map(|e| e.id)
    }

    /// Deny parole (§15); returns the new event id.
    pub fn deny_parole(&mut self, id: PrisonerId, reason: impl Into<String>) -> Option<u64> {
        if !self.world.prisoners.iter().any(|p| p.id == id) {
            return None;
        }
        if matches!(self.engine_state.parole.status(id), Some(ParoleStatus::Granted) | Some(ParoleStatus::Denied)) {
            return None;
        }
        self.engine_state.parole.set(id, ParoleStatus::Denied, self.world.time);
        self.world.events.emit(
            WorldEvent::new(0, self.world.time, EventKind::ParoleDenied, format!("Parole denied for prisoner {}: {}", id.raw(), reason.into())).with_actor(id.raw()),
        );
        self.world.events.recent().last().map(|e| e.id)
    }

    /// Get the parole status for a prisoner.
    pub fn parole_status(&self, id: PrisonerId) -> Option<ParoleStatus> {
        self.engine_state.parole.status(id)
    }

    // --- 4.6 Rehab (§14) ---

    /// Deterministic rehab progress: relief moves per minute of program
    /// participation (purpose, respect, stimulation, safety, companionship).
    pub fn apply_rehab_progress(&mut self, id: PrisonerId, minutes: u64) -> f32 {
        let Some(p) = self.world.prisoners.iter_mut().find(|p| p.id == id) else { return 0.0 };
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
        let Some(p) = self.world.prisoners.iter().find(|p| p.id == id) else { return 0.0 };
        let n = &p.needs;
        let deficit = (1.0 - n.purpose) + (1.0 - n.respect) + (1.0 - n.stimulation) + (1.0 - n.safety) + (1.0 - n.companionship);
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
        let mut v: Vec<(String, f32)> = self.world.prisoners.iter().map(|p| (p.name.clone(), p.reputation)).collect();
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

        mix(&format!("t:{}p:{}", self.world.time.secs, self.world.population()));
        for e in self.world.events.recent() {
            mix(&format!("{:?}:{}@{}", e.kind, e.summary, e.at.secs));
        }

        // Include sidecar incident causality for trace_hash compatibility
        for inc in &self.engine_state.incident_sidecar {
            let cause_str = inc.cause.map(|c| c.to_string()).unwrap_or_default();
            let conj_str = inc.consequence.map(|c| c.to_string()).unwrap_or_default();
            let kind_str = format!("{:?}", inc.kind);
            mix(&format!("inc:{}:{}:{}:{}@{}", kind_str, inc.severity.to_bits(), cause_str, conj_str, inc.timestamp.secs));
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

use crate::autosave::AutosaveScheduler;
use crate::decisions::DecisionOutcome;
use crate::parole::{ParoleLedger, ParoleStatus};

// Summary types for UI observation (defined below)

/// Summary of a prisoner for the UI prisoner view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrisonerSummary {
    /// Unique prisoner identifier.
    pub id: alc_core::PrisonerId,
    /// Display name.
    pub name: String,
    /// Age in years.
    pub age: u32,
    /// Security classification.
    pub classification: alc_prisoner::Classification,
    /// Days served of sentence.
    pub sentence_served_days: u32,
    /// Total sentence in days.
    pub sentence_days: u32,
    /// Gang ID if affiliated.
    pub gang_affiliation: Option<u64>,
    /// Reputation score.
    pub reputation: f32,
    /// Physical and mental condition.
    pub condition: alc_prisoner::Condition,
    /// Personality traits.
    pub traits: alc_prisoner::Traits,
    /// Current needs state.
    pub needs: alc_prisoner::Needs,
    /// Active goals.
    pub goals: Vec<alc_prisoner::Goal>,
    /// Daily schedule assignment.
    pub schedule: alc_prisoner::ScheduleAssignment,
    /// Skill proficiencies.
    pub skills: std::collections::HashMap<String, f32>,
    /// Brief memory summary.
    pub memory_summary: String,
}

/// Summary of a gang for the UI gang view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GangSummary {
    /// Unique gang identifier.
    pub id: u64,
    /// Gang name.
    pub name: String,
    /// Leader prisoner ID if any.
    pub leader: Option<alc_core::PrisonerId>,
    /// Member prisoner IDs.
    pub members: Vec<alc_core::PrisonerId>,
    /// Reputation score.
    pub reputation: f32,
    /// Resources controlled.
    pub resources: f32,
    /// Cultural descriptor.
    pub culture: String,
    /// Formation time.
    pub formed_at: SimTime,
    /// Whether the gang is active.
    pub active: bool,
    /// Hierarchy of members.
    pub hierarchy: Vec<alc_social::MemberRecord>,
    /// Rival gangs with intensity.
    pub rivalries: Vec<(u64, f32)>,
    /// Allied gangs with intensity.
    pub alliances: Vec<(u64, f32)>,
    /// Internal dispute descriptions.
    pub internal_disputes: Vec<String>,
    /// Current goals.
    pub goals: Vec<String>,
}

/// Summary of a guard for the UI staff view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardSummary {
    /// Unique guard identifier.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Rank.
    pub rank: alc_guard::GuardRank,
    /// Current post assignment.
    pub post: alc_guard::PostType,
    /// Current authority level.
    pub authority: alc_guard::AuthorityLevel,
    /// Current shift index.
    pub shift: u8,
    /// Current task description.
    pub current_task: String,
    /// Competence rating 0..1.
    pub competence: f32,
    /// Experience rating 0..1.
    pub experience: f32,
    /// Morale rating 0..1.
    pub morale: f32,
    /// Fatigue level 0..1.
    pub fatigue: f32,
    /// Stress level 0..1.
    pub stress: f32,
    /// Loyalty rating 0..1.
    pub loyalty: f32,
    /// Integrity rating 0..1.
    pub integrity: f32,
    /// Personality trait 0..1 (rigid vs lenient).
    pub personality: f32,
    /// Historical event log.
    pub history: Vec<String>,
}

/// Summary of the warden for the UI warden view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WardenSummary {
    /// Warden name.
    pub name: String,
    /// Leadership philosophy.
    pub philosophy: String,
    /// Personality traits.
    pub traits: alc_warden::WardenTraits,
    /// Attention budget 0..1.
    pub attention: f32,
    /// Incoming guard reports.
    pub inbox: Vec<alc_guard::GuardReport>,
    /// Pending warden actions.
    pub pending_actions: Vec<alc_warden::WardenAction>,
    /// Time of last decision.
    pub last_decision_at: SimTime,
}

/// Summary of the economy for the UI financial view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomySummary {
    /// Cash in major currency units.
    pub cash_major: f64,
    /// Financial health index 0.0..1.0.
    pub health: f64,
    /// Per-prisoner daily funding in major units.
    pub funding_per_prisoner_per_day: f64,
    /// Crowding threshold.
    pub crowding_threshold: u32,
    /// Ledger entries.
    pub ledger: Vec<alc_economy::LedgerEntry>,
}

/// Summary of a social relationship edge for the UI social network view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialEdge {
    /// First prisoner.
    pub prisoner_a: alc_core::PrisonerId,
    /// Second prisoner.
    pub prisoner_b: alc_core::PrisonerId,
    /// Friendship level 0..1.
    pub friendship: f32,
    /// Trust level 0..1.
    pub trust: f32,
    /// Fear level 0..1.
    pub fear: f32,
    /// Hostility level 0..1.
    pub hostility: f32,
    /// Rivalry level 0..1.
    pub rivalry: f32,
    /// Combined bond score.
    pub bond_score: f32,
    /// Combined threat score.
    pub threat_score: f32,
}

/// Summary of a facility for the UI map view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacilitySummary {
    /// Unique facility identifier.
    pub id: u64,
    /// Facility kind.
    pub kind: alc_world::FacilityKind,
    /// Wing number.
    pub wing: u32,
    /// Capacity in prisoners.
    pub capacity: u32,
    /// Condition 0..1.
    pub condition: f32,
    /// Required staff.
    pub staff_required: u32,
}

/// Summary of a construction project for the UI (0.05).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstructionSummary {
    /// Project id.
    pub id: u64,
    /// Facility kind.
    pub kind: alc_world::FacilityKind,
    /// Wing.
    pub wing: u32,
    /// Capacity on completion.
    pub capacity: u32,
    /// Total build days.
    pub total_days: u32,
    /// Progress days.
    pub progress_days: u32,
    /// Cost.
    pub cost: f32,
    /// Start day.
    pub started_day: u64,
}

/// Post-release outcome row for the observer UI (0.05).
///
/// Built from the persisted `EngineState::post_release` ledger, enriched with
/// release events that are still on the bounded world event bus.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PostReleaseSummary {
    /// Released prisoner.
    pub prisoner_id: PrisonerId,
    /// Name when the prisoner is still known to the world (released prisoners
    /// are removed from the roster, so this is usually `None`).
    pub prisoner_name: Option<String>,
    /// `post_release`, `release`, `parole`, `escape`, `transfer`, or `unknown`.
    pub release_type: String,
    /// Release time when the matching event is still retained.
    pub released_at: Option<SimTime>,
    /// Whether reintegration was recorded as successful.
    pub successful_reintegration: bool,
    /// Recidivism risk 0..1 parsed from the release event, when present.
    pub recidivism_risk: Option<f32>,
    /// Whether a reintegration plan existed, when the release event recorded it.
    pub has_reintegration_plan: Option<bool>,
    /// Whether the institution still needs to follow up (i.e. not successful).
    pub follow_up_required: bool,
}

/// Rehab enrollment row for the observer UI (0.05).
///
/// Derived from `rehab_*` skill keys on the prisoner, which is where
/// `alc-social::rehab` stores enrollment and 0..1 program progress.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RehabEnrollmentSummary {
    /// Enrolled prisoner.
    pub prisoner_id: PrisonerId,
    /// Prisoner display name.
    pub prisoner_name: String,
    /// Program display name (`Education`, `Vocational`, ...).
    pub program: String,
    /// Stable program key (`rehab_education`, ...).
    pub program_key: String,
    /// Progress 0..1.
    pub progress: f32,
    /// Whether the program counts as an education/vocational track.
    pub education_track: bool,
    /// Whether the prisoner is currently under disciplinary status.
    pub disciplinary: bool,
}

/// Static rehabilitation program catalog entry (0.05).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RehabProgramInfo {
    /// Program display name.
    pub name: String,
    /// Stable program key.
    pub key: String,
    /// Difficulty 0..1 (higher = slower progress, harder enrollment).
    pub difficulty: f32,
}

/// Per-program rehabilitation rollup (0.05).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RehabProgramRollup {
    /// Program display name.
    pub program: String,
    /// Stable program key.
    pub key: String,
    /// Number of current enrollments.
    pub enrolled: u32,
    /// Average progress 0..1 across current enrollments.
    pub average_progress: f32,
}

/// Aggregate rehabilitation overview (0.05).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RehabOverview {
    /// Total current enrollment rows (prisoner × program).
    pub enrollments: u32,
    /// Distinct enrolled prisoners.
    pub enrolled_prisoners: u32,
    /// Average progress across current enrollments 0..1.
    pub average_progress: f32,
    /// Completions seen on the retained event window.
    pub completions: u32,
    /// Dropouts/failures seen on the retained event window.
    pub dropouts: u32,
    /// Per-program rollups.
    pub programs: Vec<RehabProgramRollup>,
}

/// Contraband case derived from the event record (0.05).
///
/// TODO(world-state): no persisted contraband case/inventory store exists yet;
/// remove the event-derived reconstruction once `alc-world` owns one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContrabandCaseSummary {
    /// Case id (the originating world event id).
    pub case_id: u64,
    /// Simulation time of the case.
    pub at: SimTime,
    /// Item label when detectable (`Weapons`, `Drugs`, `Tools`, `Phones`,
    /// `Money`), otherwise `None`.
    pub item: Option<String>,
    /// Prisoner holding the contraband, when reported.
    pub holder: Option<PrisonerId>,
    /// Guard who discovered it, when reported.
    pub guard: Option<u64>,
    /// Severity 0..1 (derived from item class, default 0.5).
    pub severity: f32,
    /// Original event summary.
    pub summary: String,
    /// Original causal notes.
    pub causes: Vec<String>,
    /// Always `false` until a real case store can carry disposition state.
    pub resolved: bool,
}

/// Contraband inventory rollup by item category (0.05).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContrabandInventorySummary {
    /// Item label (`Weapons`, `Drugs`, `Tools`, `Phones`, `Money`, `Unknown`).
    pub item: String,
    /// Number of recorded cases for this item.
    pub cases: u32,
    /// Distinct holders seen.
    pub holders: Vec<PrisonerId>,
    /// Most recent case time.
    pub last_seen: Option<SimTime>,
    /// Worst severity seen 0..1.
    pub max_severity: f32,
}

/// One resolved schedule block for the observer timeline (0.05).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScheduleBlockView {
    /// Template block id.
    pub block_id: u32,
    /// Activity label (`Work`, `Yard`, ...).
    pub activity: String,
    /// Human-readable block label.
    pub label: String,
    /// Start minute-of-day.
    pub start_minute: u16,
    /// End minute-of-day.
    pub end_minute: u16,
    /// Whether the block was cancelled for this prisoner today.
    pub cancelled: bool,
    /// Cancel reason label (`Lockdown`, `StaffShortage`, ...).
    pub reason: Option<String>,
    /// Whether attendance is mandatory.
    pub mandatory: bool,
    /// Guards/teachers the block requires.
    pub staff_required: u8,
    /// Whether this is the block active at the current minute.
    pub current: bool,
    /// Attendance outcome.
    ///
    /// TODO(world-state): per-block attendance is not persisted, so this is
    /// `None`; `ScheduleTimeline::attendance_probability` is the deterministic
    /// compliance probability the world uses.
    pub attendance: Option<String>,
}

/// A prisoner's resolved daily schedule timeline (0.05).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScheduleTimeline {
    /// Prisoner.
    pub prisoner_id: PrisonerId,
    /// Prisoner display name.
    pub prisoner_name: String,
    /// Simulation day the timeline was resolved for.
    pub day: u64,
    /// Current minute-of-day (0..1439).
    pub current_minute: u16,
    /// Activity the world currently has this prisoner in.
    pub current_activity: Option<String>,
    /// Whether lockdown drove the resolution.
    pub lockdown: bool,
    /// Staff coverage used for the resolution 0..1.
    pub staff_coverage: f32,
    /// Deterministic attendance probability for this prisoner 0..1.
    pub attendance_probability: f32,
    /// Number of cancelled blocks today.
    pub cancelled_blocks: u32,
    /// Resolved blocks in start-time order.
    pub blocks: Vec<ScheduleBlockView>,
}

/// One budget account line in a [`BudgetSnapshot`] (0.05).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetLine {
    /// Account name (from `alc_economy::Account`).
    pub account: String,
    /// Inflow in major currency units over the window.
    pub inflow_major: f64,
    /// Outflow magnitude in major currency units over the window.
    pub outflow_major: f64,
    /// Net (inflow − outflow) in major units.
    pub net_major: f64,
    /// Number of ledger entries in the window.
    pub entries: u32,
}

/// Budget/finance snapshot for the observer finance view (0.05).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    /// Simulation day.
    pub day: u64,
    /// Simulation time.
    pub at: SimTime,
    /// Ledger window used, in days.
    pub window_days: u64,
    /// Cash on hand in major units.
    pub cash_major: f64,
    /// `Economy::health()` 0..1 (cash ÷ one month of trailing expenses).
    pub health: f32,
    /// Per-prisoner daily funding in major units.
    pub funding_per_prisoner_per_day: f64,
    /// Fixed daily institutional grant in major units.
    pub base_daily_grant: f64,
    /// Revenue per day projected from the window.
    pub projected_daily_revenue: f64,
    /// Expenses per day projected from the window.
    pub projected_daily_expense: f64,
    /// 30-day trailing average daily expense (major units).
    pub trailing_daily_expense_avg: f64,
    /// Estimated cash runway in days at the projected expense rate.
    pub runway_days: f64,
    /// Current population.
    pub population: u32,
    /// Current capacity.
    pub capacity: u32,
    /// Crowding threshold.
    pub crowding_threshold: u32,
    /// Per-account breakdown.
    pub lines: Vec<BudgetLine>,
    /// Human-readable warnings explaining financial pressure.
    pub warnings: Vec<String>,
}

/// The five rehabilitation programs supported by `alc-social::rehab`.
const REHAB_PROGRAMS: [RehabProgram; 5] = [
    RehabProgram::Education,
    RehabProgram::Vocational,
    RehabProgram::Counseling,
    RehabProgram::AddictionRecovery,
    RehabProgram::AngerManagement,
];

/// Release information recovered from the event record.
struct ReleaseEventInfo {
    release_type: String,
    at: SimTime,
    recidivism_risk: Option<f32>,
    has_reintegration_plan: Option<bool>,
}

/// Build the `alc-social` rehab view of a prisoner (same mapping the world uses).
fn rehab_state_for(prisoner: &alc_prisoner::Prisoner) -> RehabPrisonerState {
    RehabPrisonerState {
        disciplinary_status: prisoner.schedule.disciplinary_status,
        education: prisoner.schedule.education,
        skills: prisoner.skills.clone(),
        hope: prisoner.condition.hope,
        mental_health: prisoner.condition.mental_health,
        health: prisoner.condition.health,
        aggression: prisoner.traits.aggression,
        emotional_stability: prisoner.traits.emotional_stability,
        impulsiveness: prisoner.traits.impulsiveness,
        discipline: prisoner.traits.discipline,
        intelligence: prisoner.traits.intelligence,
        reputation: prisoner.reputation,
        fatigue: prisoner.needs.fatigue,
        purpose_need: prisoner.needs.purpose,
        stability_need: prisoner.needs.stability,
        respect_need: prisoner.needs.respect,
        rehab_motivation: prisoner.rehab_motivation(),
        desperation: prisoner.needs.desperation(),
    }
}

/// Write a processed rehab state back onto the prisoner (world-compatible).
fn write_back_rehab_state(prisoner: &mut alc_prisoner::Prisoner, state: RehabPrisonerState) {
    prisoner.skills = state.skills;
    prisoner.condition.hope = state.hope.clamp(0.0, 1.0);
    prisoner.condition.mental_health = state.mental_health.clamp(0.0, 1.0);
    prisoner.condition.health = state.health.clamp(0.0, 1.0);
    prisoner.traits.aggression = state.aggression.clamp(0.0, 1.0);
    prisoner.traits.emotional_stability = state.emotional_stability.clamp(0.0, 1.0);
    prisoner.traits.impulsiveness = state.impulsiveness.clamp(0.0, 1.0);
    prisoner.reputation = state.reputation.clamp(0.0, 1.0);
    prisoner.needs.purpose = state.purpose_need.clamp(0.0, 1.0);
    prisoner.needs.stability = state.stability_need.clamp(0.0, 1.0);
    prisoner.needs.respect = state.respect_need.clamp(0.0, 1.0);
    if state.education {
        prisoner.schedule.education = true;
    }
}

/// Parse a `f32` immediately following `marker` in any of the text fragments.
fn parse_f32_after(fragments: &[String], marker: &str) -> Option<f32> {
    for fragment in fragments {
        let lower = fragment.to_lowercase();
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = &fragment[idx + marker.len()..];
        let token: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
            .collect();
        if let Ok(value) = token.parse::<f32>() {
            return Some(value);
        }
    }
    None
}

/// Detect an explicit reintegration-plan flag in causal notes.
fn parse_plan_flag(fragments: &[String]) -> Option<bool> {
    for fragment in fragments {
        let lower = fragment.to_lowercase();
        if lower.contains("plan yes") {
            return Some(true);
        }
        if lower.contains("plan no") {
            return Some(false);
        }
    }
    None
}

/// Detect a contraband item label inside a free-form event summary.
fn detect_contraband_item(summary: &str) -> Option<String> {
    const ITEMS: [&str; 5] = ["Weapons", "Drugs", "Tools", "Phones", "Money"];
    ITEMS
        .iter()
        .find(|label| summary.contains(*label))
        .map(|label| (*label).to_string())
}

/// Severity by contraband item label (matches `alc-social::contraband`).
fn contraband_severity_for_label(label: &str) -> f32 {
    match label {
        "Weapons" => 0.95,
        "Drugs" => 0.75,
        "Tools" => 0.7,
        "Phones" => 0.55,
        "Money" => 0.35,
        _ => 0.5,
    }
}

/// Parse a guard id out of a `discovery by guard N` style cause.
fn parse_guard_cause(cause: &str) -> Option<u64> {
    let lower = cause.to_lowercase();
    let idx = lower.find("guard")? + "guard".len();
    let digits: String = cause[idx..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u64>().ok()
}

// Same modules as above, but conditionally compiled.
// The `gui` feature gate ensures these types only exist when the feature is active.
//
// Enable with `cargo build -p sumo --features gui`.
//
// ## GUI types
// - `SumoApp` — the main GUI application window
// - `SumoGui` — the GUI backend instance
// - `Panel` — a UI panel within the application
// - `launch_gui` — launch the egui/eframe application

// Optional GUI rendering backend (feature `gui`).
/// Enable with `cargo build -p sumo --features gui`.
/// This module and its types are compiled only when the `gui` feature is active.

#[cfg(feature = "gui")]
mod gui;

/// Re-export GUI types when the `gui` feature is active.
#[cfg(feature = "gui")]
pub use gui::{SumoApp, launch_gui};

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
}
