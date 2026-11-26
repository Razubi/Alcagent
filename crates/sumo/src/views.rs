//! Read-only summary/view DTOs for observers and UIs (split from `lib.rs`).
use alc_core::{PrisonerId, SimTime};
use alc_social::rehab::{RehabPrisonerState, RehabProgram};
use serde::{Deserialize, Serialize};

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

/// Pairwise political score between two gangs (Spec 008).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GangRelationSummary {
    /// Other gang's identifier.
    pub gang: u64,
    /// Relationship-driven likelihood of cooperation (`0..=1`).
    pub affinity: f32,
    /// Relationship-driven likelihood of hostility (`0..=1`).
    pub hostility: f32,
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
    /// Rival gangs with observed hostility intensity.
    pub rivalries: Vec<(u64, f32)>,
    /// Allied gangs with observed affinity intensity.
    pub alliances: Vec<(u64, f32)>,
    /// Internal dispute descriptions.
    pub internal_disputes: Vec<String>,
    /// Current goals.
    pub goals: Vec<String>,
    /// Institutional pressure exerted by the current gang graph.
    pub pressure: alc_social::GangPressure,
    /// Durable political transitions involving this gang, oldest first.
    pub history: Vec<alc_world::GangHistoryEntry>,
    /// Relationship-derived scores for every other active gang.
    pub relations: Vec<GangRelationSummary>,
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
    /// Current credibility of this guard's reports.
    pub credibility: f32,
    /// Cumulative perceptual/enforcement mistakes (spec 009).
    pub mistakes: u32,
    /// Cumulative policy violations (spec 009).
    pub violations: u32,
    /// Distinct prisoners this guard has formed an opinion about.
    pub tracked_opinions: usize,
    /// Current guard needs (0..1, higher = more unmet).
    #[serde(default)]
    pub needs: alc_guard::GuardNeeds,
    /// Whether the guard is in an off-duty block right now.
    #[serde(default)]
    pub off_duty: bool,
    /// Activity for the current/last off-duty block.
    #[serde(default)]
    pub off_duty_activity: alc_guard::OffDutyActivity,
    /// Composite performance score used for promotion and reduction.
    #[serde(default)]
    pub performance: f32,
    /// Off-duty social bonds (bounded).
    #[serde(default)]
    pub bonds: Vec<alc_guard::GuardBond>,
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
    /// Administrative affordances and incident pressure (0.05 spec 010).
    pub control: alc_warden::WardenControlSummary,
    /// Newest-first warden decision history with rationale (0.05 spec 010).
    pub recent_decisions: Vec<alc_warden::WardenDecisionRecord>,
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

/// Emergent social role row for the observer UI (Spec 012).
///
/// The role is a string label so UIs do not depend directly on
/// `alc_social::SocialRole`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocialRoleSummary {
    /// Prisoner carrying the role.
    pub prisoner: PrisonerId,
    /// Stable role label (`Leader`, `InformantSuspect`, ...).
    pub role: String,
    /// Detection strength, clamped to 0.0..=1.0.
    pub strength: f32,
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
    /// Lifecycle status (0.05).
    #[serde(default)]
    pub status: alc_world::ConstructionStatus,
    /// Terminal reason if cancelled/failed.
    #[serde(default)]
    pub terminal_reason: Option<String>,
    /// Staffing prerequisite met.
    #[serde(default = "default_true_sumo")]
    pub staff_allocated: bool,
    /// Materials/funding prerequisite met.
    #[serde(default = "default_true_sumo")]
    pub materials_ready: bool,
    /// Facility prerequisites met.
    #[serde(default = "default_true_sumo")]
    pub prereqs_met: bool,
}

fn default_true_sumo() -> bool {
    true
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

/// Contraband case from the world-owned durable case store (0.05).
///
/// Case ids, discovery data, item, severity and summary come from
/// `World::contraband_cases`; resolution comes from `World::resolved_contraband_at`
/// with the engine mirror retained for save-format compatibility. Rows are
/// never derived by parsing free-text events on read; standalone legacy events
/// are ingested into the store once by the engine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContrabandCaseSummary {
    /// World-assigned case id.
    pub case_id: u64,
    /// Simulation time of the case.
    pub at: SimTime,
    /// Item label (`Weapons`, `Drugs`, `Tools`, `Phones`, `Money`).
    pub item: Option<String>,
    /// Prisoner holding the contraband.
    pub holder: Option<PrisonerId>,
    /// Guard who discovered it.
    pub guard: Option<u64>,
    /// Severity 0..1 recorded on the case.
    pub severity: f32,
    /// Case summary recorded by the world.
    pub summary: String,
    /// Structured causal notes derived from the case record.
    pub causes: Vec<String>,
    /// Resolved in the world store (or its engine mirror).
    pub resolved: bool,
    /// Resolution timestamp from the world store or engine mirror.
    #[serde(default)]
    pub resolved_at: Option<SimTime>,
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
pub(crate) const REHAB_PROGRAMS: [RehabProgram; 5] = [
    RehabProgram::Education,
    RehabProgram::Vocational,
    RehabProgram::Counseling,
    RehabProgram::AddictionRecovery,
    RehabProgram::AngerManagement,
];

/// Release information recovered from the event record.
pub(crate) struct ReleaseEventInfo {
    pub(crate) release_type: String,
    pub(crate) at: SimTime,
    pub(crate) recidivism_risk: Option<f32>,
    pub(crate) has_reintegration_plan: Option<bool>,
}

/// Build the `alc-social` rehab view of a prisoner (same mapping the world uses).
pub(crate) fn rehab_state_for(prisoner: &alc_prisoner::Prisoner) -> RehabPrisonerState {
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
pub(crate) fn write_back_rehab_state(
    prisoner: &mut alc_prisoner::Prisoner,
    state: RehabPrisonerState,
) {
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
pub(crate) fn parse_f32_after(fragments: &[String], marker: &str) -> Option<f32> {
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
pub(crate) fn parse_plan_flag(fragments: &[String]) -> Option<bool> {
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
