//! Engine state types and persistence converters (split from `lib.rs`).
use crate::autosave::AutosaveScheduler;
use crate::incidents;
use crate::parole::{ParoleLedger, ParoleStatus};
use alc_core::{PrisonerId, SimTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

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
    /// Detailed rationale for the decision.
    pub rationale: String,
    /// If repaired/clamped, the reason for the repair.
    pub repair_reason: String,
    /// Outcome state (Accepted, Repaired, Rejected, Proposed).
    pub outcome: alc_agents::decision::DecisionOutcomeState,
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
    /// Disposition.
    pub disposition: String,
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
            disposition: format!("{:?}", incident.disposition),
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
    /// Engine-only resolution timestamps for contraband cases (not persisted in
    /// alc-persistence format; coordinated persistence change required to keep
    /// across save/load).
    pub resolved_contraband_at: std::collections::BTreeMap<u64, SimTime>,
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
            resolved_contraband_at: std::collections::BTreeMap::new(),
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
                    causal_chain: inc.causal_chain.iter().map(|link| alc_persistence::PersistedCausalLink {
                        source_id: link.source_id,
                        is_incident: link.is_incident,
                        description: link.description.clone(),
                    }).collect(),
                    timestamp: inc.timestamp,
                    location: inc.location.clone(),
                    involved_prisoners: inc.involved_prisoners.iter().map(|p| p.raw()).collect(),
                    involved_guards: Vec::new(), // sumo's Incident doesn't track guards
                    witnesses: inc.witnesses.iter().map(|p| p.raw()).collect(),
                    disposition: incident_disposition_to_persist(inc.disposition),
                    response_actions: inc.response_actions.iter().map(|a| alc_persistence::PersistedResponseAction {
                        action: format!("{:?}", a.action),
                        responder: match a.responder { incidents::Responder::Guard(id) => id, _ => 0 },
                        responder_kind: responder_kind(&a.responder).to_string(),
                        at: a.at,
                        description: a.description.clone(),
                        success: a.success,
                    }).collect(),
                    resolved: inc.resolved,
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
                rationale: r.rationale.clone(),
                repair_reason: r.repair_reason.clone(),
                outcome: r.outcome,
            }).collect(),
            resolved_contraband_cases: self.resolved_contraband_cases.iter().copied().collect(),
            resolved_contraband_at: self.resolved_contraband_at.clone(),
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
                    causal_chain: inc.causal_chain.into_iter().map(|link| incidents::CausalLink {
                        source_id: link.source_id,
                        is_incident: link.is_incident,
                        description: link.description,
                    }).collect(),
                    timestamp: inc.timestamp,
                    location: inc.location,
                    involved_prisoners: inc.involved_prisoners.into_iter().map(PrisonerId).collect(),
                    witnesses: inc.witnesses.into_iter().map(PrisonerId).collect(),
                    severity: inc.severity,
                    disposition: incident_disposition_from_persist(inc.disposition),
                    response_actions: inc.response_actions.into_iter().map(|a| incidents::ResponseAction {
                        action: response_action_kind_from_persist(&a.action),
                        responder: responder_from_persist(&a),
                        at: a.at,
                        description: a.description,
                        success: a.success,
                    }).collect(),
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
                rationale: r.rationale,
                repair_reason: r.repair_reason,
                outcome: r.outcome,
            }).collect(),
            resolved_contraband_cases: persist.resolved_contraband_cases.into_iter().collect(),
            resolved_contraband_at: persist.resolved_contraband_at.clone(),
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

/// Helper to convert an incident disposition to its persistence mirror.
fn incident_disposition_to_persist(d: incidents::IncidentDisposition) -> alc_persistence::PersistedIncidentDisposition {
    match d {
        incidents::IncidentDisposition::None => alc_persistence::PersistedIncidentDisposition::None,
        incidents::IncidentDisposition::Investigated => alc_persistence::PersistedIncidentDisposition::Investigated,
        incidents::IncidentDisposition::Disciplinary => alc_persistence::PersistedIncidentDisposition::Disciplinary,
        incidents::IncidentDisposition::Medical => alc_persistence::PersistedIncidentDisposition::Medical,
        incidents::IncidentDisposition::Resolved => alc_persistence::PersistedIncidentDisposition::Resolved,
    }
}

/// Helper to convert a persisted incident disposition back to the engine type.
fn incident_disposition_from_persist(d: alc_persistence::PersistedIncidentDisposition) -> incidents::IncidentDisposition {
    match d {
        alc_persistence::PersistedIncidentDisposition::None => incidents::IncidentDisposition::None,
        alc_persistence::PersistedIncidentDisposition::Investigated => incidents::IncidentDisposition::Investigated,
        alc_persistence::PersistedIncidentDisposition::Disciplinary => incidents::IncidentDisposition::Disciplinary,
        alc_persistence::PersistedIncidentDisposition::Medical => incidents::IncidentDisposition::Medical,
        alc_persistence::PersistedIncidentDisposition::Resolved => incidents::IncidentDisposition::Resolved,
    }
}

/// Stable label for a responder kind used in the persistence mirror.
fn responder_kind(r: &incidents::Responder) -> &'static str {
    match r {
        incidents::Responder::Guard(_) => "Guard",
        incidents::Responder::Warden => "Warden",
        incidents::Responder::System => "System",
        incidents::Responder::MedicalStaff => "MedicalStaff",
        incidents::Responder::AdminStaff => "AdminStaff",
    }
}

/// Parse a persisted response-action kind back to the engine enum, falling back
/// to a safe default for unknown strings from future versions.
fn response_action_kind_from_persist(s: &str) -> incidents::ResponseActionKind {
    use incidents::ResponseActionKind::*;
    match s {
        "GuardIntervention" => GuardIntervention,
        "MedicalResponse" => MedicalResponse,
        "DisciplinaryHearing" => DisciplinaryHearing,
        "Segregation" => Segregation,
        "PrivilegeRevocation" => PrivilegeRevocation,
        "InvestigationOpened" => InvestigationOpened,
        "Lockdown" => Lockdown,
        "Transfer" => Transfer,
        "CounselingReferral" => CounselingReferral,
        "ContrabandSearch" => ContrabandSearch,
        "VerbalWarning" => VerbalWarning,
        "WrittenWarning" => WrittenWarning,
        _ => VerbalWarning,
    }
}

/// Parse a persisted responder back to the engine enum.
fn responder_from_persist(p: &alc_persistence::PersistedResponseAction) -> incidents::Responder {
    match p.responder_kind.as_str() {
        "Guard" => incidents::Responder::Guard(p.responder),
        "Warden" => incidents::Responder::Warden,
        "System" => incidents::Responder::System,
        "MedicalStaff" => incidents::Responder::MedicalStaff,
        "AdminStaff" => incidents::Responder::AdminStaff,
        _ => incidents::Responder::System,
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

