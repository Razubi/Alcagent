//! World simulation: facility layout, population, tick loop, history (Directive §2, §17, §18, §21).
//!
//! The tick loop is the integration point that makes the prison *alive*. Every
//! step runs prisoners through a schedule resolution, a need-driven utility
//! decision, co-presence encounters, and (on day boundaries) escape, parole and
//! intake resolution. AI deliberation is gated by the orchestrator; the
//! deterministic fallback always keeps the simulation moving (Directive §19).

mod emergent;

use alc_agents::{
    ActionCandidate, Decision, Disposition, Legality, Orchestrator, PendingDecision, Role,
    context::{DecisionRequest, EpistemicContext, hash_validation_context},
    decision::{
        DecisionAction, DecisionOutcomeState, DecisionProvenance, DecisionValidation,
        ValidationContext, ValidationResult, ValidationStage, validate_decision_detailed,
    },
    memory::{EpistemicKind, EpistemicRecord, Memory},
};
use alc_core::config::AiConfig;
use alc_core::{FacilityId, GangId, GuardId, PrisonerId, RngStream, SimTime, TimeSpeed, WardenId};
use alc_economy::{Account, Economy, Money};
use alc_events::{EventBus, EventKind, RehabFailureReason, WorldEvent};
use alc_guard::{
    Guard, GuardInteraction, GuardInteractionKind, GuardObservation, GuardRank, PostType,
};
use alc_prisoner::{
    Activity, DriftActivity, GoalKind, NeedDriftContext, NeedTag, Prisoner, SchedulePolicy,
    ScheduleTemplate, current_block, generate_intake, resolve_schedule,
};
use alc_social::{
    ConflictOutcome, ContrabandItem, DisputeAction, EscapeConsequences, EscapeOutcome, Gang,
    GangHistoryKind, GangPoliticsProfile, GroupDirective, PrisonerGangProfile, Relationship,
    RelationshipDelta, SocialGraph, apply_escape_consequences, apply_outcome,
    detect_internal_disputes, gang_event_cause, gang_pressure, gang_relation_scores,
    leadership_challenger, maybe_form_gang, resolve_conflict, resolve_escape_attempt,
    split_candidates,
};
use alc_warden::{DecisionSource, Warden, WardenAction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Loss-of-privileges status is a sanction with a finite sentence, not a
/// permanent parole bar. Kept as a world rule so old saves (which may carry a
/// status without a dated record) are left untouched.
const DISCIPLINARY_STATUS_DAYS: u64 = 30;

/// Building / facility (Directive §12 expansion catalog).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Facility {
    pub id: FacilityId,
    pub kind: FacilityKind,
    pub wing: u32,
    pub capacity: u32,
    pub condition: f32, // 0..1, degrades with neglect
    pub staff_required: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum FacilityKind {
    CellBlock,
    Dormitory,
    Medical,
    Kitchen,
    Workshop,
    Recreation,
    Education,
    Admin,
    Isolation,
    Visitor,
    Staff,
    Yard,
}

impl FacilityKind {
    pub fn display(&self) -> &'static str {
        match self {
            FacilityKind::CellBlock => "Cell Block",
            FacilityKind::Dormitory => "Dormitory",
            FacilityKind::Medical => "Medical",
            FacilityKind::Kitchen => "Kitchen",
            FacilityKind::Workshop => "Workshop",
            FacilityKind::Recreation => "Recreation",
            FacilityKind::Education => "Education",
            FacilityKind::Admin => "Admin",
            FacilityKind::Isolation => "Isolation",
            FacilityKind::Visitor => "Visiting",
            FacilityKind::Staff => "Staff Quarters",
            FacilityKind::Yard => "Yard",
        }
    }
}

/// Construction project lifecycle status (0.05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstructionStatus {
    Queued,
    InProgress,
    Completed,
    Cancelled,
    Failed,
    /// Approved: prerequisites, staffing and materials were validated.
    ///
    /// Appended last to preserve bincode ordinals of pre-v5 saves.
    Approved,
}

impl Default for ConstructionStatus {
    fn default() -> Self {
        ConstructionStatus::Queued
    }
}

/// A queued construction project (0.05 lifecycle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstructionProject {
    /// Project id.
    pub id: u64,
    /// Facility kind to build.
    pub kind: FacilityKind,
    /// Target wing.
    pub wing: u32,
    /// Capacity when complete.
    pub capacity: u32,
    /// Total build days required.
    pub total_days: u32,
    /// Days of progress completed.
    pub progress_days: u32,
    /// Total cost.
    pub cost: f32,
    /// Day enqueued.
    pub started_day: u64,
    /// Lifecycle status.
    #[serde(default)]
    pub status: ConstructionStatus,
    /// Reason for cancellation/failure, if any.
    #[serde(default)]
    pub terminal_reason: Option<String>,
    /// Whether staffing prerequisite is considered met for this project.
    #[serde(default = "default_true")]
    pub staff_allocated: bool,
    /// Whether materials/funding prerequisite is considered met.
    #[serde(default = "default_true")]
    pub materials_ready: bool,
    /// Whether facility prerequisites are met (e.g. admin/space exists).
    #[serde(default = "default_true")]
    pub prereqs_met: bool,
}

fn default_true() -> bool {
    true
}

/// Bounded heuristic score helper; NaN never leaks into persisted politics state.
fn clamp01(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

/// Basic sentence info attached to a post-release record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PostReleaseSentenceInfo {
    pub sentence_days: u32,
    pub sentence_served_days: u32,
    pub classification: alc_prisoner::Classification,
    pub escape_motivation_at_exit: f32,
}

/// Record of a prisoner who left the institution via parole, escape, death
/// or transfer (0.05 post-release lifecycle, Directive §15/§16).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PostReleaseRecord {
    pub id: u64,
    pub prisoner_id: u64,
    pub name: String,
    /// Why they left: "parole" | "escape" | "death" | "transfer".
    pub reason: String,
    /// Simulation day (`World::days_simulated`) of departure.
    pub day: u64,
    pub sentence_info: PostReleaseSentenceInfo,
    /// Deterministic basic-outcome seed for future reintegration modeling.
    pub outcome_seed: u64,
    /// Post-release reintegration outcomes (recidivism tracking). Driven by
    /// `outcome_seed` so reintegration is deterministic and reproducible.
    #[serde(default)]
    pub employment: bool,
    #[serde(default)]
    pub housing: bool,
    #[serde(default)]
    pub violation: bool,
    #[serde(default)]
    pub return_to_custody: bool,
}

/// World-side record of post-release reintegration outcomes (recidivism
/// tracking). One entry per departed prisoner; persisted with serde defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PostReleaseOutcomeRecord {
    pub id: u64,
    pub post_release_id: u64,
    pub prisoner_id: u64,
    pub name: String,
    /// Simulation day (`World::days_simulated`) of departure.
    pub day: u64,
    /// Deterministic seed that drove the basic reintegration outcome.
    #[serde(default)]
    pub outcome_seed: u64,
    #[serde(default)]
    pub employment: bool,
    #[serde(default)]
    pub housing: bool,
    #[serde(default)]
    pub violation: bool,
    #[serde(default)]
    pub return_to_custody: bool,
}

/// Durable, append-only record of one gang-politics transition (Spec 008).
///
/// Gang state itself remains backwards-compatible: history lives on `World`
/// with a serde default so older saves load as an empty timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GangHistoryEntry {
    pub id: u64,
    pub gang: GangId,
    pub kind: GangHistoryKind,
    pub at: SimTime,
    pub summary: String,
    #[serde(default)]
    pub actors: Vec<u64>,
    #[serde(default)]
    pub causes: Vec<String>,
}

/// Per-prisoner contraband inventory + flags (0.05 lifecycle; world-owned
/// because `Prisoner` is in the immutable-to-us `alc-prisoner` crate).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrisonerContrabandState {
    /// Items held by the prisoner (item kind -> count).
    #[serde(default)]
    pub inventory: std::collections::HashMap<ContrabandItem, u32>,
    /// True when guards have flagged this prisoner for contraband possession.
    #[serde(default)]
    pub flags_confiscated: bool,
    /// Consecutive flagged days while the flag stays set.
    #[serde(default)]
    pub flagged_days: u32,
    /// Times contraband has been discovered on the prisoner.
    #[serde(default)]
    pub discovered_count: u32,
}

/// Explicit rehabilitation program enrollment / progress record (0.05 depth).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RehabProgramRecord {
    pub program: alc_social::RehabProgram,
    pub enrolled_day: u64,
    pub progress_days: u32,
    pub progress_sum: f32,
    pub completed: bool,
    pub dropped: bool,
    /// Days the prisoner attended/made progress in this program.
    #[serde(default)]
    pub attendance_days: u32,
    /// Last day attendance/progress was recorded.
    #[serde(default)]
    pub last_attended_day: Option<u64>,
    /// Terminal failure reason, if dropped/failed.
    #[serde(default)]
    pub failure_reason: Option<RehabFailureReason>,
}

impl RehabProgramRecord {
    /// Bounded 0..1 progress signal used by post-release outcome modeling.
    /// Attendance is accumulated in `u64` with saturation so a large number of
    /// records (or a legacy value of `u32::MAX`) can never overflow.
    fn progress_signal(records: &[RehabProgramRecord]) -> f32 {
        let done = records.iter().filter(|r| r.completed).count() as f32;
        let attended = records.iter().fold(0_u64, |sum, r| {
            sum.saturating_add(u64::from(r.attendance_days))
        });
        (done * 0.6 + (attended.min(180) as f32 / 180.0) * 0.4).clamp(0.0, 1.0)
    }

    fn new(program: alc_social::RehabProgram, day: u64) -> Self {
        Self {
            program,
            enrolled_day: day,
            progress_days: 0,
            progress_sum: 0.0,
            completed: false,
            dropped: false,
            attendance_days: 0,
            last_attended_day: None,
            failure_reason: None,
        }
    }

    fn is_active(&self) -> bool {
        !self.completed && !self.dropped
    }
}

/// Institutional policies (subset, extensible).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policies {
    pub lockdown: bool,
    pub privileges_level: f32,
    pub rehab_funding_multiplier: f32,
    pub security_level: f32,
    /// Manual intake pause (0.05 control panel).
    #[serde(default)]
    pub intake_paused: bool,
}

impl Default for Policies {
    fn default() -> Self {
        Self {
            lockdown: false,
            privileges_level: 0.5,
            rehab_funding_multiplier: 1.0,
            security_level: 0.6,
            intake_paused: false,
        }
    }
}

/// One decision processed by the canonical AI batch gate.
#[derive(Debug, Clone)]
pub struct AiBatchEntry {
    pub prisoner_id: PrisonerId,
    pub result: ValidationResult,
    pub applied: bool,
}

/// Bounded result of one AI decision batch.
#[derive(Debug, Clone, Default)]
pub struct AiBatchOutcome {
    pub entries: Vec<AiBatchEntry>,
    pub accepted: usize,
    pub rejected: usize,
}

/// The full persistent world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub seed: u64,
    pub time: SimTime,
    pub speed: TimeSpeed,
    pub next_facility_id: u64,
    pub next_guard_id: u64,
    pub next_prisoner_id: u64,
    #[serde(default)]
    pub next_construction_id: u64,

    pub facilities: Vec<Facility>,
    pub prisoners: Vec<Prisoner>,
    pub guards: Vec<Guard>,
    pub warden: Warden,

    pub social: SocialGraph,
    /// Gang organizations tracked in world state (Directive 7).
    #[serde(default)]
    pub gangs: Vec<Gang>,
    #[serde(default)]
    pub next_gang_id: u64,
    pub economy: Economy,
    pub policies: Policies,

    pub events: EventBus,

    /// Institutional memory / history (Directive §18).
    pub history: Vec<WorldEvent>,

    /// Orchestration state (Directive §19).
    pub orchestrator: Orchestrator,

    /// Total days simulated (for autosave intervals).
    pub days_simulated: u64,

    // -- Per-prisoner runtime state (Directive §5, §19) ---------------------
    /// Activity each prisoner is currently scheduled to be doing, resolved
    /// from the daily template against live policy/staffing. Stored on `World`
    /// because `alc-prisoner` is an immutable-to-us dependency.
    #[serde(default)]
    pub current_activity: HashMap<PrisonerId, Activity>,
    /// Last structured decision emitted by each prisoner (auditable, §19).
    #[serde(default)]
    pub current_decisions: HashMap<PrisonerId, Decision>,
    /// Where each prisoner currently is; drives co-presence encounters (§5.4).
    #[serde(default)]
    pub prisoner_location: HashMap<PrisonerId, FacilityId>,

    /// Where each guard currently is; drives guard observation and map frame.
    #[serde(default)]
    pub guard_location: HashMap<GuardId, FacilityId>,

    /// Last sim-hour (epoch hours) at which the guard duty sweep ran.
    /// Guards observe once per sim-hour rather than every tick — otherwise
    /// routine reports flood the event bus and evict meaningful events.
    #[serde(default)]
    pub last_guard_duty_hour: u64,
    /// Day lockdown was declared; used to expire emergency measures.
    #[serde(default)]
    pub lockdown_since_day: u64,

    /// Construction queue (0.05). Projects advance daily until completion.
    #[serde(default)]
    pub construction_queue: Vec<ConstructionProject>,

    /// Post-release records: prisoners who left via parole, escape, death or
    /// transfer get an entry here (0.05 lifecycle). `#[serde(default)]` keeps
    /// saves from before this field loadable.
    #[serde(default)]
    pub post_release_records: Vec<PostReleaseRecord>,

    /// World-side reintegration/recidivism outcomes, one per departure.
    #[serde(default)]
    pub post_release_outcomes: Vec<PostReleaseOutcomeRecord>,

    /// Monotonically increasing id for `post_release_records`.
    #[serde(default)]
    pub next_post_release_id: u64,

    /// Per-prisoner contraband inventory + flags (0.05 lifecycle).
    #[serde(default)]
    pub contraband_state: HashMap<PrisonerId, PrisonerContrabandState>,

    /// Contraband case resolution timestamps (case id -> sim time). Persisted with
    /// serde default for 0.05 lifecycle/audit coherence; engine may also track.
    #[serde(default)]
    pub resolved_contraband_at: std::collections::BTreeMap<u64, SimTime>,

    /// World-authoritative contraband case store (Directive §7/§8/§17). Each
    /// discovery creates a durable case record with holder, item, guard,
    /// severity, disposition and disciplinary follow-up. Persisted via serde
    /// default so older saves load with an empty store.
    #[serde(default)]
    pub contraband_cases: Vec<alc_social::contraband::ContrabandCaseRecord>,

    /// Monotonically increasing id allocator for `contraband_cases`.
    #[serde(default)]
    pub next_contraband_case_id: u64,
    /// Explicit per-program rehabilitation enrollment/progress records.
    #[serde(default)]
    pub rehab_records: HashMap<PrisonerId, Vec<RehabProgramRecord>>,

    /// Day the last transfer-out fired (rate-limits the 0.05 pathway).
    #[serde(default)]
    pub last_transfer_day: u64,

    // -- Spec 008 append-only fields (declared last for bincode v5 stability) --
    /// Durable gang timeline. Select transitions are projected here so splits,
    /// leadership changes and conflicts survive the bounded event bus.
    #[serde(default)]
    pub gang_history: Vec<GangHistoryEntry>,
    #[serde(default)]
    pub next_gang_history_id: u64,
}

impl World {
    pub fn new(seed: u64) -> Self {
        Self::new_with_config(seed, &AiConfig::default())
    }

    /// Construct a world with an explicit AI orchestration policy.
    pub fn new_with_config(seed: u64, config: &AiConfig) -> Self {
        let mut w = Self {
            seed,
            time: SimTime::EPOCH,
            speed: TimeSpeed::Paused,
            next_facility_id: 1,
            next_guard_id: 1,
            next_prisoner_id: 1,
            next_construction_id: 1,
            facilities: Vec::new(),
            prisoners: Vec::new(),
            guards: Vec::new(),
            warden: Warden::new(
                WardenId(0),
                "Warden",
                alc_warden::WardenTraits::randomized(&mut RngStream::new(seed, 0)),
            ),
            social: SocialGraph::default(),
            gangs: Vec::new(),
            next_gang_id: 1,
            gang_history: Vec::new(),
            next_gang_history_id: 1,
            economy: Economy::default(),
            policies: Policies::default(),
            events: EventBus::new(50_000),
            history: Vec::new(),
            orchestrator: Orchestrator::from_config(config),
            days_simulated: 0,
            current_activity: HashMap::new(),
            current_decisions: HashMap::new(),
            prisoner_location: HashMap::new(),
            guard_location: HashMap::new(),
            last_guard_duty_hour: 0,
            lockdown_since_day: 0,
            construction_queue: Vec::new(),
            post_release_records: Vec::new(),
            post_release_outcomes: Vec::new(),
            next_post_release_id: 1,
            contraband_state: HashMap::new(),
            resolved_contraband_at: std::collections::BTreeMap::new(),
            contraband_cases: Vec::new(),
            next_contraband_case_id: 1,
            rehab_records: HashMap::new(),
            last_transfer_day: 0,
        };
        w.bootstrap();
        w
    }

    fn bootstrap(&mut self) {
        // Starting layout: 1 cell block, 1 yard, 1 kitchen, 1 medical, 1 admin.
        self.add_facility(FacilityKind::CellBlock, 1, 80);
        self.add_facility(FacilityKind::Yard, 1, 100);
        self.add_facility(FacilityKind::Kitchen, 1, 20);
        self.add_facility(FacilityKind::Medical, 1, 10);
        self.add_facility(FacilityKind::Admin, 1, 10);
        self.add_facility(FacilityKind::Recreation, 1, 30);

        // Starting guards: 4 (1 per essential post + admin).
        let posts = [
            PostType::Tower,
            PostType::Yard,
            PostType::Corridor,
            PostType::Reception,
        ];
        for post in posts {
            let id = self.next_guard_id;
            self.next_guard_id += 1;
            self.guards.push(Guard::new(
                GuardId(id),
                format!("Guard {id}"),
                GuardRank::Officer,
                post,
                &mut RngStream::new(self.seed, id),
            ));
        }

        // Starting population uses the same correlated intake pipeline as all
        // later arrivals (Directive §13).
        for _ in 0..24 {
            let id = self.next_prisoner_id;
            self.next_prisoner_id += 1;
            let prisoner_id = PrisonerId(id);
            self.prisoners.push(generate_intake(self.seed, prisoner_id));
            self.register_ai_agent(prisoner_id, Role::Prisoner);
        }

        // Give observers a consistent view from the very first frame.
        self.resolve_schedules_step();
        self.assign_locations_from_schedule();
    }

    /// Register a prisoner with the AI scheduler using deterministic phase.
    pub fn register_ai_agent(&mut self, id: PrisonerId, role: Role) {
        let interval_secs = match role {
            Role::Warden => self
                .ai_config()
                .decision_interval_mins
                .saturating_mul(60)
                .saturating_mul(4),
            Role::Guard => self.ai_config().decision_interval_mins.saturating_mul(60) / 2,
            Role::Prisoner | Role::Unknown => {
                self.ai_config().decision_interval_mins.saturating_mul(60)
            }
        }
        .max(1);
        let mut rng = RngStream::new(
            self.seed ^ 0xA1CE_0117,
            id.raw() ^ self.time.as_secs() ^ 0x9E37_79B9,
        );
        self.orchestrator.register_agent(
            id.raw(),
            role,
            interval_secs,
            self.time.as_secs(),
            &mut rng,
        );
    }

    /// Runtime AI configuration (not persisted in the v5 save format).
    pub fn ai_config(&self) -> &AiConfig {
        &self.orchestrator.runtime.config
    }

    /// Re-apply AI configuration to runtime state on a loaded world.
    pub fn configure_ai(&mut self, config: &AiConfig) {
        self.orchestrator.configure(config);
    }

    pub fn add_facility(&mut self, kind: FacilityKind, wing: u32, capacity: u32) -> FacilityId {
        let id = self.next_facility_id;
        self.next_facility_id += 1;
        self.facilities.push(Facility {
            id: FacilityId(id),
            kind,
            wing,
            capacity,
            condition: 1.0,
            staff_required: match kind {
                FacilityKind::Admin => 1,
                FacilityKind::Medical => 2,
                FacilityKind::Kitchen => 1,
                _ => 0,
            },
        });
        FacilityId(id)
    }

    pub fn capacity(&self) -> u32 {
        self.facilities
            .iter()
            .filter(|f| matches!(f.kind, FacilityKind::CellBlock | FacilityKind::Dormitory))
            .map(|f| f.capacity)
            .sum()
    }

    pub fn population(&self) -> u32 {
        self.prisoners.len() as u32
    }

    /// Emit an event and append a durable copy carrying the assigned id.
    /// Observers dedupe the durable log and bounded bus by event id, so a
    /// placeholder id would silently collapse unrelated historical events.
    fn emit_history_event(&mut self, event: WorldEvent) -> u64 {
        let id = self.events.emit(event.clone());
        let mut event = event;
        event.id = id;
        self.history.push(event);
        id
    }

    // -----------------------------------------------------------------------
    // Gang membership + durable political history (Spec 008)
    // -----------------------------------------------------------------------

    /// The gang this prisoner is affiliated with, read through the world API.
    /// Decision code should use this rather than raw `Prisoner` fields so the
    /// roster and the affiliation cannot silently diverge.
    #[must_use]
    pub fn prisoner_gang(&self, prisoner: PrisonerId) -> Option<GangId> {
        self.prisoners
            .iter()
            .find(|p| p.id == prisoner)
            .and_then(|p| p.gang_affiliation.map(GangId))
    }

    /// Install a fully formed organization, synchronizing every member's
    /// affiliation and recording its formation. Emergent formation uses this
    /// so no caller can bypass roster/affiliation consistency.
    pub fn register_gang(&mut self, gang: Gang) -> bool {
        let id = gang.id;
        let members = gang.members.clone();
        let name = gang.name.clone();
        let leader = gang.leader;
        if self.gangs.iter().any(|g| g.id == id)
            || members
                .iter()
                .any(|pid| !self.prisoners.iter().any(|p| p.id == *pid))
        {
            return false;
        }
        self.gangs.push(gang);
        for pid in &members {
            if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == *pid) {
                p.gang_affiliation = Some(id.0);
            }
        }
        self.next_gang_id = self.next_gang_id.max(id.0.saturating_add(1));
        self.record_gang_history(
            id,
            GangHistoryKind::Formed,
            format!("Gang '{}' coalesced around {:?}", name, leader),
            &members,
            &[gang_event_cause(id), "emergent formation".into()],
        );
        true
    }

    /// Authoritative gang creation. Sets the founder's affiliation, records the
    /// formation in durable history, and keeps the id allocator ahead of the
    /// supplied identifier.
    pub fn create_gang(
        &mut self,
        id: GangId,
        name: impl Into<String>,
        founder: PrisonerId,
    ) -> bool {
        if !self.prisoners.iter().any(|p| p.id == founder) {
            return false;
        }
        self.register_gang(Gang::with_formed_at(id, name, founder, self.time))
    }

    /// Read-only view of the append-only gang timeline.
    #[must_use]
    pub fn gang_history(&self) -> &[GangHistoryEntry] {
        &self.gang_history
    }

    /// Current institutional pressure exerted by the active gang graph.
    #[must_use]
    pub fn gang_pressure(&self) -> alc_social::GangPressure {
        gang_pressure(&self.gangs, &self.social)
    }

    fn gang_history_event_kind(kind: GangHistoryKind) -> EventKind {
        match kind {
            GangHistoryKind::Formed => EventKind::GangFormed,
            GangHistoryKind::Recruit => EventKind::GangRecruit,
            GangHistoryKind::Loss => EventKind::RelationshipShift,
            GangHistoryKind::LeadershipChange => EventKind::LeadershipDispute,
            GangHistoryKind::Split => EventKind::GangFormed,
            GangHistoryKind::Merge => EventKind::GangRecruit,
            GangHistoryKind::Alliance | GangHistoryKind::Rivalry => EventKind::GangRivalry,
            GangHistoryKind::Dispute => EventKind::LeadershipDispute,
            GangHistoryKind::MajorConflict => EventKind::GangClash,
            GangHistoryKind::Disbanded => EventKind::GangDisbanded,
        }
    }

    /// Append a durable gang-history record without emitting a second event.
    /// Callers that already return a world event to the engine use this path;
    /// all other transitions use [`Self::record_gang_history`].
    fn append_gang_history(
        &mut self,
        gang: GangId,
        kind: GangHistoryKind,
        summary: impl Into<String>,
        actors: &[PrisonerId],
        causes: &[String],
    ) -> u64 {
        let id = self.next_gang_history_id;
        self.next_gang_history_id = self.next_gang_history_id.saturating_add(1);
        self.gang_history.push(GangHistoryEntry {
            id,
            gang,
            kind,
            at: self.time,
            summary: summary.into(),
            actors: actors.iter().map(|p| p.raw()).collect(),
            causes: causes.to_vec(),
        });
        id
    }

    /// Append a durable gang-history record and project it onto the event bus
    /// and institutional history with a stable causal marker.
    fn record_gang_history(
        &mut self,
        gang: GangId,
        kind: GangHistoryKind,
        summary: impl Into<String>,
        actors: &[PrisonerId],
        causes: &[String],
    ) -> u64 {
        let summary = summary.into();
        let id = self.append_gang_history(gang, kind, summary.clone(), actors, causes);
        let mut event = WorldEvent::new(
            0,
            self.time,
            Self::gang_history_event_kind(kind),
            format!("{}: {}", kind.label(), summary),
        );
        for &actor in actors {
            event = event.with_actor(actor.raw());
        }
        for cause in causes {
            event = event.with_cause(cause.clone());
        }
        self.emit_history_event(event);
        id
    }

    /// Remove empty organizations and record their dissolution once.
    fn prune_empty_gangs(&mut self) {
        let empty: Vec<GangId> = self
            .gangs
            .iter()
            .filter(|g| g.members.is_empty())
            .map(|g| g.id)
            .collect();
        for gid in empty {
            self.record_gang_history(
                gid,
                GangHistoryKind::Disbanded,
                format!("Gang {} dissolved after losing all members", gid.0),
                &[],
                &[gang_event_cause(gid)],
            );
        }
        self.gangs.retain(|g| !g.members.is_empty());
    }

    /// Authoritative membership transition. Handles switching gangs, records
    /// recruitment in durable history, and keeps both roster sides in sync.
    pub fn join_gang(&mut self, prisoner: PrisonerId, gang: GangId) -> bool {
        if !self.prisoners.iter().any(|p| p.id == prisoner) {
            return false;
        }
        if !self
            .gangs
            .iter()
            .any(|g| g.id == gang && g.active && !g.members.is_empty())
        {
            return false;
        }
        if self.prisoner_gang(prisoner) == Some(gang)
            && self
                .gangs
                .iter()
                .any(|g| g.id == gang && g.members.contains(&prisoner))
        {
            return true;
        }
        if let Some(previous) = self.prisoner_gang(prisoner) {
            if previous != gang {
                self.leave_gang(prisoner);
            }
        }
        let Some(index) = self.gangs.iter().position(|g| g.id == gang) else {
            return false;
        };
        if !self.gangs[index].members.contains(&prisoner) {
            self.gangs[index].recruit(prisoner);
        }
        if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == prisoner) {
            p.gang_affiliation = Some(gang.0);
        }
        self.record_gang_history(
            gang,
            GangHistoryKind::Recruit,
            format!("Prisoner {} joined gang {}", prisoner.raw(), gang.0),
            &[prisoner],
            &[gang_event_cause(gang), "voluntary recruitment".into()],
        );
        true
    }

    /// Authoritative departure. `Gang::lose` reassigns leadership when needed;
    /// the resulting leadership change and any dissolution are recorded.
    pub fn leave_gang(&mut self, prisoner: PrisonerId) -> bool {
        let Some(gid) = self.prisoner_gang(prisoner) else {
            return false;
        };
        let Some(index) = self.gangs.iter().position(|g| g.id == gid) else {
            if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == prisoner) {
                p.gang_affiliation = None;
            }
            return false;
        };
        let was_member = self.gangs[index].members.contains(&prisoner);
        let old_leader = self.gangs[index].leader;
        self.gangs[index].lose(prisoner);
        let new_leader = self.gangs[index].leader;
        if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == prisoner) {
            p.gang_affiliation = None;
        }
        if was_member {
            self.record_gang_history(
                gid,
                GangHistoryKind::Loss,
                format!("Prisoner {} left gang {}", prisoner.raw(), gid.0),
                &[prisoner],
                &[gang_event_cause(gid), "membership departure".into()],
            );
        }
        if old_leader.is_some() && old_leader != new_leader {
            let actors = new_leader.into_iter().collect::<Vec<_>>();
            self.record_gang_history(
                gid,
                GangHistoryKind::LeadershipChange,
                match new_leader {
                    Some(leader) => format!(
                        "Gang {} leadership passed from {} to {}",
                        gid.0,
                        old_leader.map_or(0, PrisonerId::raw),
                        leader.raw()
                    ),
                    None => format!("Gang {} lost its leader", gid.0),
                },
                &actors,
                &[gang_event_cause(gid), "leader departure".into()],
            );
        }
        self.prune_empty_gangs();
        true
    }

    /// Absorb one organization into another, moving every roster member and
    /// rewriting affiliations. Returns `false` for unknown, inactive or
    /// identical gangs.
    pub fn merge_gangs(&mut self, source: GangId, target: GangId) -> bool {
        if source == target {
            return false;
        }
        let Some(source_index) = self
            .gangs
            .iter()
            .position(|g| g.id == source && g.active && !g.members.is_empty())
        else {
            return false;
        };
        let Some(target_index) = self
            .gangs
            .iter()
            .position(|g| g.id == target && g.active && !g.members.is_empty())
        else {
            return false;
        };
        if source_index == target_index {
            return false;
        }
        let moved = if source_index < target_index {
            let (left, right) = self.gangs.split_at_mut(target_index);
            right[0].merge_from(&mut left[source_index])
        } else {
            let (left, right) = self.gangs.split_at_mut(source_index);
            left[target_index].merge_from(&mut right[0])
        };
        for &pid in &moved {
            if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == pid) {
                p.gang_affiliation = Some(target.0);
            }
        }
        let causes = vec![
            gang_event_cause(source),
            gang_event_cause(target),
            "sustained alliance and shared goals".into(),
        ];
        self.record_gang_history(
            target,
            GangHistoryKind::Merge,
            format!(
                "Gang {} absorbed {} ({} member(s) moved)",
                target.0,
                source.0,
                moved.len()
            ),
            &moved,
            &causes,
        );
        self.record_gang_history(
            source,
            GangHistoryKind::Merge,
            format!("Gang {} merged into {}", source.0, target.0),
            &moved,
            &causes,
        );
        self.gangs.retain(|g| g.id != source);
        true
    }

    /// Repair roster/affiliation drift (including legacy saves and repaired
    /// decisions). Returns the number of repairs applied.
    pub fn reconcile_gang_membership(&mut self) -> usize {
        let live: BTreeSet<u64> = self.prisoners.iter().map(|p| p.id.raw()).collect();
        let valid: BTreeSet<u64> = self
            .gangs
            .iter()
            .filter(|g| g.active)
            .map(|g| g.id.raw())
            .collect();
        let mut repairs = 0usize;

        for p in &mut self.prisoners {
            if let Some(raw) = p.gang_affiliation {
                if !valid.contains(&raw) {
                    p.gang_affiliation = None;
                    repairs += 1;
                }
            }
        }
        for gang in &mut self.gangs {
            let before = gang.members.len();
            gang.members.retain(|p| live.contains(&p.raw()));
            repairs += before.saturating_sub(gang.members.len());
            if let Some(leader) = gang.leader {
                if !gang.members.contains(&leader) {
                    gang.leader = gang.members.first().copied();
                    repairs += 1;
                }
            }
        }

        for raw in live.iter().copied() {
            let pid = PrisonerId(raw);
            // A legacy roster entry without an affiliation is repaired toward
            // the roster; an explicit (different) affiliation is authoritative.
            if self.prisoner_gang(pid).is_none() {
                let roster_gang = self
                    .gangs
                    .iter()
                    .find(|g| g.members.contains(&pid))
                    .map(|g| g.id);
                if let Some(gid) = roster_gang {
                    if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == pid) {
                        p.gang_affiliation = Some(gid.0);
                        repairs += 1;
                    }
                }
            }
            let current = self.prisoner_gang(pid);
            for gang in &mut self.gangs {
                if Some(gang.id) != current && gang.members.contains(&pid) {
                    gang.lose(pid);
                    repairs += 1;
                }
            }
            let Some(gid) = current else { continue };
            let Some(index) = self.gangs.iter().position(|g| g.id == gid) else {
                continue;
            };
            if !self.gangs[index].members.contains(&pid) {
                self.gangs[index].recruit(pid);
                repairs += 1;
            }
        }
        for gang in &self.gangs {
            for &member in &gang.members {
                if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == member) {
                    if p.gang_affiliation != Some(gang.id.0) {
                        p.gang_affiliation = Some(gang.id.0);
                        repairs += 1;
                    }
                }
            }
        }
        self.prune_empty_gangs();
        repairs
    }

    /// Gang-channel throughput for the contraband model. Higher pressure,
    /// resources and member commitment make a gang a more effective channel;
    /// discipline damps it. Always bounded to `0..=1`.
    #[must_use]
    pub fn gang_control_for(&self, prisoner: PrisonerId) -> f32 {
        let Some(gid) = self.prisoner_gang(prisoner) else {
            return 0.0;
        };
        let Some(gang) = self.gangs.iter().find(|g| g.id == gid) else {
            return 0.0;
        };
        let Some(p) = self.prisoners.iter().find(|p| p.id == prisoner) else {
            return 0.0;
        };
        let pressure = self.gang_pressure().total();
        let standing = gang
            .hierarchy
            .iter()
            .find(|record| record.prisoner == prisoner)
            .map_or(0.0, |record| {
                record.commitment * 0.1 + record.influence * 0.1
            });
        clamp01(
            0.16 + gang.resources.clamp(0.0, 1.0) * 0.24
                + pressure * 0.2
                + standing
                + (1.0 - p.traits.discipline.clamp(0.0, 1.0)) * 0.1,
        )
    }

    /// `(rival, friendly)` relation between two prisoners' gangs. Same-gang
    /// dyads count as friendly; declared alliances damp hostile encounters.
    fn gang_tie(&self, a: Option<GangId>, b: Option<GangId>) -> (bool, bool) {
        let (Some(a), Some(b)) = (a, b) else {
            return (false, false);
        };
        if a == b {
            return (false, true);
        }
        let Some(ga) = self.gangs.iter().find(|g| g.id == a) else {
            return (false, false);
        };
        let Some(gb) = self.gangs.iter().find(|g| g.id == b) else {
            return (false, false);
        };
        let rival = ga.rivalries.contains(&b) || gb.rivalries.contains(&a);
        let friendly = ga.alliances.contains(&b) || gb.alliances.contains(&a);
        (rival, friendly)
    }

    /// Local friction signal used by guard perception. Need pressure is a
    /// proxy for tension, not proof of a critical incident.
    fn prisoner_guard_tension(&self, p: &Prisoner) -> f32 {
        let base = p.needs.total_pressure();
        let agitation = p.needs.respect * 0.15 + p.needs.safety * 0.1;
        let compliant = self
            .current_decisions
            .get(&p.id)
            .map(|d| {
                matches!(
                    d.action,
                    alc_agents::decision::DecisionAction::Comply
                        | alc_agents::decision::DecisionAction::None
                )
            })
            .unwrap_or(false);
        let compliance = if compliant { 0.0 } else { 0.08 };
        (base * 0.5 + agitation + compliance).clamp(0.0, 1.0)
    }

    /// Prisoners the guard can actually perceive from its assigned facility.
    pub fn guard_observations(&self, guard_id: GuardId) -> Vec<GuardObservation> {
        let Some(facility) = self.guard_location.get(&guard_id).copied() else {
            return Vec::new();
        };
        self.prisoners
            .iter()
            .filter_map(|p| {
                if self.prisoner_location.get(&p.id).copied() != Some(facility) {
                    return None;
                }
                Some(GuardObservation {
                    prisoner_id: p.id.raw(),
                    facility_id: facility.0,
                    tension: self.prisoner_guard_tension(p),
                })
            })
            .collect()
    }

    /// Current policy context for a guard duty tick.
    pub fn guard_duty_context(&self) -> alc_guard::DutyContext {
        alc_guard::DutyContext {
            at: self.time,
            lockdown: self.policies.lockdown,
            security_level: self.policies.security_level,
        }
    }

    /// Local supervisory deterrence, used to make post assignments change
    /// encounter hostility and incident likelihood.
    pub fn guard_deterrence(&self, facility: FacilityId) -> f32 {
        let local: Vec<&Guard> = self
            .guards
            .iter()
            .filter(|g| self.guard_location.get(&g.id) == Some(&facility))
            .collect();
        if local.is_empty() {
            return 0.0;
        }
        let quality = local
            .iter()
            .map(|g| g.competence * 0.6 + (1.0 - g.fatigue) * 0.25 + (1.0 - g.stress) * 0.15)
            .sum::<f32>()
            / local.len() as f32;
        (quality * 0.18 * local.len().min(3) as f32).clamp(0.0, 0.55)
    }

    /// Reassign a guard to a post and move its pawn atomically, so the
    /// "exactly one place" invariant remains true. Returns false if unknown.
    pub fn assign_guard_post(&mut self, guard_id: GuardId, post: PostType) -> bool {
        let Some(guard) = self.guards.iter_mut().find(|g| g.id == guard_id) else {
            return false;
        };
        if guard.post == post {
            return true;
        }
        guard.post = post;
        guard.current_task = "patrol".into();
        let location = Self::facility_for_guard_post(post);
        let facility = self
            .facility_of_kind(location)
            .or_else(|| self.facilities.first().map(|f| f.id))
            .unwrap_or(FacilityId(0));
        self.guard_location.insert(guard_id, facility);
        self.events.emit(
            WorldEvent::new(
                0,
                self.time,
                EventKind::PolicyChange,
                format!("Guard {} reassigned to {post:?}", guard_id.raw()),
            )
            .with_actor(guard_id.raw())
            .with_cause("guard duty reassignment"),
        );
        true
    }

    /// Hourly guard duty sweep: area-scoped observation → opinion → report →
    /// interaction → escalation. Kept as one world-owned authority; the engine
    /// driver calls the same primitives for explicit duty cycles.
    pub fn guard_duty_sweep(&mut self) {
        let hour = self.time.as_secs() / 3600;
        if hour == self.last_guard_duty_hour {
            return;
        }
        self.last_guard_duty_hour = hour;

        let guard_policy = self.guard_duty_context();
        let guard_ids: Vec<GuardId> = self.guards.iter().map(|g| g.id).collect();
        let mut guard_reports = Vec::new();
        let mut guard_interactions: Vec<GuardInteraction> = Vec::new();

        for gid in guard_ids {
            let observed = self.guard_observations(gid);
            let Some(guard) = self.guards.iter_mut().find(|g| g.id == gid) else {
                continue;
            };
            let mut rng =
                RngStream::new(self.seed, self.time.as_secs() ^ (gid.raw() << 8) ^ 0xA11CE);
            let result = guard.duty_tick_area(&observed, guard_policy, &mut rng);
            guard_interactions.extend(result.interactions);
            guard_reports.extend(result.reports.into_iter().map(|r| (gid, r)));
        }

        // Guard interactions are validated consequences applied by the world,
        // never arbitrary writes from the guard subagent itself.
        for interaction in guard_interactions {
            let intensity = interaction.intensity.clamp(0.0, 1.0);
            let target = interaction.prisoner_id;
            let Some(prisoner) = self.prisoners.iter_mut().find(|p| p.id.raw() == target) else {
                continue;
            };
            match interaction.kind {
                GuardInteractionKind::Observe => {}
                GuardInteractionKind::Directive => {
                    prisoner.needs.impulse(NeedTag::Respect, -0.02 * intensity);
                    prisoner.needs.impulse(NeedTag::Safety, 0.02 * intensity);
                }
                GuardInteractionKind::Restrain => {
                    prisoner.needs.impulse(NeedTag::Safety, 0.05 * intensity);
                    prisoner.needs.impulse(NeedTag::Freedom, 0.04 * intensity);
                    prisoner.needs.impulse(NeedTag::Respect, 0.03 * intensity);
                }
                GuardInteractionKind::Assist => {
                    prisoner.needs.impulse(NeedTag::Safety, 0.03 * intensity);
                }
                GuardInteractionKind::PolicyViolation => {
                    prisoner.needs.impulse(NeedTag::Respect, 0.04 * intensity);
                    prisoner.needs.impulse(NeedTag::Safety, 0.02 * intensity);
                }
            }
        }

        let accepted: Vec<(GuardId, alc_guard::GuardReport)> = guard_reports
            .into_iter()
            .filter(|(_, r)| r.severity == 0 || r.requests_assistance || r.credibility >= 0.25)
            .collect();
        if accepted.is_empty() {
            return;
        }

        self.warden.security_incidents_this_week =
            self.warden.security_incidents_this_week.saturating_add(
                accepted
                    .iter()
                    .filter(|(_, r)| {
                        r.severity >= 2 && (r.requests_assistance || r.credibility >= 0.25)
                    })
                    .count() as u32,
            );
        self.warden.incidents_this_week = self.warden.security_incidents_this_week;

        for (gid, report) in accepted {
            if report.severity == 0 {
                continue;
            }
            // Only critical reports and assistance requests reach the event
            // bus. Lower-severity observations remain in guard history.
            if report.severity < 3 && !report.requests_assistance {
                continue;
            }
            let kind = if report.requests_assistance || report.severity >= 3 {
                EventKind::SecurityResponse
            } else {
                EventKind::GuardReport
            };
            let mistake = u8::from(report.summary.starts_with("Unverified"));
            let witness = u8::from(report.witnessed_directly);
            let cause = format!(
                "guard duty report; post={:?} sev={} cred={:.2} witness={} mistake={}",
                report.post, report.severity, report.credibility, witness, mistake
            );
            let ev = WorldEvent::new(0, self.time, kind, report.summary.clone())
                .with_actor(gid.raw())
                .with_cause(cause)
                .with_confidence(report.credibility);
            let id = self.events.emit(ev);
            if id == 0 {
                continue;
            }
            self.warden.inbox.push(id);
            if self.warden.inbox.len() > 200 {
                let drop = self.warden.inbox.len() - 200;
                self.warden.inbox.drain(0..drop);
            }
        }
    }

    /// Advance the world by one simulation step.
    pub fn tick(&mut self) {
        if self.speed == TimeSpeed::Paused {
            return;
        }
        let dt_mins: u64 = match self.speed {
            TimeSpeed::RealTime => 1,
            TimeSpeed::FastForward => 5,
            TimeSpeed::Extreme => 30,
            TimeSpeed::EventDriven => 1,
            TimeSpeed::Paused => 0,
        };
        let day_before = self.time.as_days();
        self.time = self.time.add_mins(dt_mins);
        let now_secs = self.time.as_secs();

        // 1. Needs drift — now modulated by each prisoner's resolved activity,
        //    traits, crowding and staffing (Directive §4.3). Guards/hunger/
        //    fatigue therefore ripple into every later decision this tick.
        self.drift_needs_step(dt_mins);

        // 2. Guards duty tick: area-scoped observation, opinion formation,
        //    reporting and escalation. Runs once per sim-hour.
        self.guard_duty_sweep();

        // 3. Resolve today's schedule for everyone against live policy.
        self.resolve_schedules_step();

        // 4. Need-driven utility decisions (AI-gated, deterministic fallback).
        self.make_decisions_step(now_secs);

        // 5. Co-presence encounters between prisoners sharing a facility.
        self.resolve_encounters_step();

        // 6. Medical consequences: deterministic death pathway from severe
        //    health/medical neglect, plus a small health decay rate.
        self.resolve_medical_consequences_step();

        // 7. Transfer-out when the institution is dangerously overcrowded
        //    or at near-max security (Deterministic 0.05 pathway).
        self.resolve_transfers_step();

        // 8. Daily boundary: economy, sentence progress, escapes, parole,
        //    warden arbitration and intake. Uses a day-crossing check so fast
        //    speeds can never skip the boundary.
        if self.time.as_days() > day_before {
            self.on_day_boundary();
        }
    }

    // -----------------------------------------------------------------------
    // Step 1 — needs drift (uses last tick's resolved activity)
    // -----------------------------------------------------------------------

    fn drift_needs_step(&mut self, dt_mins: u64) {
        let crowd = (self.population() as f32 / self.capacity().max(1) as f32).min(1.0);
        let staff_coverage = self.staff_coverage();
        let lockdown = self.policies.lockdown;
        let activities = self.current_activity.clone();

        for p in &mut self.prisoners {
            let traits = p.traits.clone();
            let activity = activities.get(&p.id).copied().unwrap_or(Activity::Sleep);
            let relief = relief_for_activity(activity);
            let ctx = NeedDriftContext {
                traits: Some(&traits),
                activity: drift_activity(activity),
                relief: &relief,
                crowd_density: crowd,
                heat: 0.0,
                staff_coverage,
                lockdown,
            };
            p.needs.drift_with(dt_mins, &ctx);
        }
    }

    fn staff_coverage(&self) -> f32 {
        let required: u32 = self.facilities.iter().map(|f| f.staff_required).sum();
        if required == 0 {
            1.0
        } else {
            (self.guards.len() as f32 / required as f32).min(1.0)
        }
    }

    // -----------------------------------------------------------------------
    // Step 3 — schedule resolution (Directive §5)
    // -----------------------------------------------------------------------

    fn schedule_policy(&self) -> SchedulePolicy {
        SchedulePolicy {
            lockdown: self.policies.lockdown,
            privileges_level: self.policies.privileges_level.max(0.05),
            security_level: self.policies.security_level,
            staff_coverage: self.staff_coverage(),
            weather_ok: true,
            rehab_funding_multiplier: self.policies.rehab_funding_multiplier,
            allow_refusal: true,
        }
    }

    fn resolve_schedules_step(&mut self) {
        let template = ScheduleTemplate::default_medium();
        let policy = self.schedule_policy();
        let minute = self.time.minute_of_day() as u16;

        let mut activities = HashMap::with_capacity(self.prisoners.len());
        for p in &self.prisoners {
            let schedule = resolve_schedule(p, &template, &policy);
            let mut activity = current_block(&schedule, minute)
                .map(|b| b.block.activity)
                .unwrap_or(Activity::Sleep);

            // Directive §5: prisoners may refuse scheduled activities. A
            // prisoner whose companionship need is low and whose discipline
            // is low can roll a refusal and take idle time instead.
            if policy.allow_refusal
                && !matches!(
                    activity,
                    Activity::Sleep | Activity::Lockdown | Activity::Medical | Activity::SickCall
                )
                && p.needs.companionship < 0.35
                && p.traits.discipline < 0.35
            {
                let refusal_chance = (0.40 * (1.0 - p.needs.companionship.clamp(0.0, 1.0))
                    + 0.35 * (1.0 - p.traits.discipline.clamp(0.0, 1.0)))
                .clamp(0.0, 0.75);
                let mut rng = RngStream::new(
                    self.seed ^ (minute as u64) ^ 0xA11C_E5ED,
                    p.id.raw() ^ 0x5EED,
                );
                if rng.chance(refusal_chance as f64) {
                    activity = Activity::FreeTime;
                }
            }

            activities.insert(p.id, activity);
        }
        self.current_activity = activities;
    }

    /// Seed locations from the resolved activity (before any MoveTo decision).
    fn assign_locations_from_schedule(&mut self) {
        let mut locations = HashMap::with_capacity(self.prisoners.len());
        for p in &self.prisoners {
            let activity = self
                .current_activity
                .get(&p.id)
                .copied()
                .unwrap_or(Activity::Sleep);
            let kind = facility_for_activity(activity);
            let id = self
                .facility_of_kind(kind)
                .or_else(|| self.facilities.first().map(|f| f.id))
                .unwrap_or(FacilityId(0));
            locations.insert(p.id, id);
        }
        self.prisoner_location = locations;

        // Also assign guard locations based on their posts
        let mut guard_locations = HashMap::with_capacity(self.guards.len());
        for g in &self.guards {
            let kind = Self::facility_for_guard_post(g.post);
            let id = self
                .facility_of_kind(kind)
                .or_else(|| self.facilities.first().map(|f| f.id))
                .unwrap_or(FacilityId(0));
            guard_locations.insert(g.id, id);
        }
        self.guard_location = guard_locations;
    }

    fn facility_for_guard_post(post: alc_guard::PostType) -> FacilityKind {
        use alc_guard::PostType;
        match post {
            PostType::Tower => FacilityKind::Admin,
            PostType::Wing => FacilityKind::CellBlock,
            PostType::Corridor => FacilityKind::CellBlock,
            PostType::Yard => FacilityKind::Yard,
            PostType::Reception => FacilityKind::Admin,
            PostType::Medical => FacilityKind::Medical,
            PostType::Work => FacilityKind::Workshop,
            PostType::Visitor => FacilityKind::Visitor,
            PostType::Admin => FacilityKind::Admin,
        }
    }

    fn facility_of_kind(&self, kind: FacilityKind) -> Option<FacilityId> {
        self.facilities
            .iter()
            .find(|f| f.kind == kind)
            .map(|f| f.id)
    }

    // -----------------------------------------------------------------------
    // Canonical AI decision gate (spec 011)
    // -----------------------------------------------------------------------

    /// Build the single canonical validation context for one prisoner.
    ///
    /// The context is sorted and bounded so provider hashes, dry-runs, batch
    /// validation and delivery all observe exactly the same world slice.
    pub fn validation_context(&self, id: PrisonerId) -> Option<ValidationContext> {
        let prisoner = self.prisoners.iter().find(|p| p.id == id)?;
        let facility = self.prisoner_location.get(&id).copied();
        let guards_nearby = facility
            .map(|f| {
                self.guard_location
                    .values()
                    .filter(|loc| **loc == f)
                    .count() as u32
            })
            .unwrap_or(0);

        let mut visible_entities = Vec::new();
        if let Some(facility) = facility {
            visible_entities.extend(
                self.prisoner_location
                    .iter()
                    .filter(|(_, loc)| **loc == facility)
                    .map(|(pid, _)| pid.raw()),
            );
            visible_entities.extend(
                self.guard_location
                    .iter()
                    .filter(|(_, loc)| **loc == facility)
                    .map(|(gid, _)| gid.raw()),
            );
        }
        if !visible_entities.contains(&id.raw()) {
            visible_entities.push(id.raw());
        }
        visible_entities.sort_unstable();
        visible_entities.dedup();

        let mut known_facilities: Vec<u64> = self.facilities.iter().map(|f| f.id.0).collect();
        known_facilities.sort_unstable();
        known_facilities.dedup();

        let has_contraband = self.contraband_state.get(&id).map_or(false, |state| {
            state.inventory.values().any(|count| *count > 0)
        });
        let tokens_remaining_today = self
            .orchestrator
            .budget
            .agent_budget(id.raw())
            .map_or(self.orchestrator.token_budget_per_agent, |budget| {
                budget.remaining()
            });

        Some(ValidationContext {
            current_facility: facility.map(|f| f.0).unwrap_or(0),
            lockdown: self.policies.lockdown,
            stamina: prisoner.condition.hope,
            tokens_remaining_today,
            guards_nearby,
            agent_id: id.raw(),
            role: Role::Prisoner,
            in_gang: prisoner.gang_affiliation.is_some(),
            gang_id: prisoner.gang_affiliation,
            is_gang_leader: self
                .gangs
                .iter()
                .any(|g| g.leader.map(|leader| leader.raw()) == Some(id.raw())),
            visible_entities,
            known_facilities,
            has_contraband,
            last_decision: self.current_decisions.get(&id).cloned(),
            now_secs: self.time.as_secs(),
        })
    }

    /// Build a bounded provider request for one prisoner using the world's
    /// need-derived action candidates.
    pub fn provider_context(&self, id: PrisonerId) -> Option<DecisionRequest> {
        let candidates = self.decision_candidates_from_world(id);
        self.provider_context_with_candidates(id, candidates)
    }

    /// Build a provider request from an already-computed candidate set.
    pub fn provider_context_with_candidates(
        &self,
        id: PrisonerId,
        candidates: Vec<ActionCandidate>,
    ) -> Option<DecisionRequest> {
        let validation = self.validation_context(id)?;
        let epistemic = self.epistemic_context(id);
        Some(DecisionRequest::new(
            id.raw(),
            Role::Prisoner,
            self.time.as_secs(),
            candidates,
            validation,
            epistemic,
        ))
    }

    /// Retrieve a bounded, epistemic-tagged memory slice for one prisoner.
    ///
    /// Direct observations, rumors, suspicions, beliefs and confirmed facts
    /// are kept in distinct buckets so a provider cannot quietly promote a
    /// rumor into a fact.
    pub fn epistemic_context(&self, id: PrisonerId) -> EpistemicContext {
        let top_k = self.ai_config().max_context_memories.max(1);
        let now_secs = self.time.as_secs();
        let mut memory = Memory::new();
        let mut seen = BTreeMap::new();

        for event in self.history.iter().chain(self.events.recent()) {
            if !event.actors.contains(&id.raw()) {
                continue;
            }
            seen.entry(event.id).or_insert_with(|| event.clone());
        }
        let mut events: Vec<_> = seen.into_values().collect();
        events.sort_by(|a, b| {
            a.at.as_secs()
                .cmp(&b.at.as_secs())
                .then_with(|| a.id.cmp(&b.id))
        });
        // Keep the most recent bounded slice; the memory layer performs the
        // final importance/recency ranking.
        let skip = events.len().saturating_sub(top_k.saturating_mul(2));
        for event in events.into_iter().skip(skip) {
            let kind = Self::epistemic_kind_for(event.kind, event.actors.contains(&id.raw()));
            let source = event
                .actors
                .iter()
                .copied()
                .find(|actor| *actor != id.raw())
                .unwrap_or(event.id);
            let mut record = EpistemicRecord::new(kind, id.raw(), event.summary.clone())
                .with_time(event.at.as_secs());
            record.confidence = event.confidence.clamp(0.0, 1.0);
            record.salience = 0.5;
            record.source = Some(source);
            memory.remember_scored(record);
        }

        if let Some(prisoner) = self.prisoners.iter().find(|p| p.id == id) {
            if !prisoner.memory_summary.trim().is_empty() {
                let mut record = EpistemicRecord::new(
                    EpistemicKind::Belief,
                    id.raw(),
                    prisoner.memory_summary.clone(),
                )
                .with_time(now_secs);
                record.confidence = 0.45;
                record.source = Some(id.raw());
                memory.remember_scored(record);
            }
        }

        EpistemicContext::from_memory(&memory, Some(id.raw()), &[], top_k, now_secs)
    }

    fn epistemic_kind_for(kind: EventKind, witnessed: bool) -> EpistemicKind {
        match kind {
            EventKind::Rumor => EpistemicKind::Rumor,
            EventKind::Investigation | EventKind::EscapeAttempt | EventKind::Theft
                if !witnessed =>
            {
                EpistemicKind::Suspicion
            }
            EventKind::PolicyChange
            | EventKind::ParoleGranted
            | EventKind::ParoleDenied
            | EventKind::Release
            | EventKind::RehabilitationSuccess
            | EventKind::ContrabandConfiscated => EpistemicKind::ConfirmedFact,
            EventKind::Fight
            | EventKind::Argument
            | EventKind::Threat
            | EventKind::RelationshipShift
            | EventKind::Injury
            | EventKind::MedicalEmergency
                if witnessed =>
            {
                EpistemicKind::DirectKnowledge
            }
            _ => EpistemicKind::Belief,
        }
    }

    /// Validate a decision against the canonical context for its agent.
    pub fn validate_decision_for(&self, id: PrisonerId, decision: &Decision) -> DecisionValidation {
        let Some(ctx) = self.validation_context(id) else {
            return DecisionValidation {
                result: ValidationResult::Rejected {
                    reason: format!("unknown prisoner id {}", id.raw()),
                },
                stage: Some(ValidationStage::WorldState),
                reason: Some(format!("unknown prisoner id {}", id.raw())),
                retryable: false,
            };
        };
        self.validate_decision_in_context(id, decision, &ctx)
    }

    /// Validate a decision against an explicitly supplied context. Batch
    /// delivery uses this to validate every proposal in one canonical slice.
    pub fn validate_decision_in_context(
        &self,
        id: PrisonerId,
        decision: &Decision,
        ctx: &ValidationContext,
    ) -> DecisionValidation {
        let validation = validate_decision_detailed(decision, ctx);
        if validation.is_accepted() {
            let validated = validation
                .accepted_decision()
                .cloned()
                .unwrap_or_else(|| decision.clone());
            if let Err(reason) = self.engine_rules_for(id, &validated) {
                return DecisionValidation {
                    result: ValidationResult::Rejected {
                        reason: reason.clone(),
                    },
                    stage: Some(ValidationStage::WorldState),
                    reason: Some(reason),
                    retryable: false,
                };
            }
        }
        validation
    }

    /// Validate a proposal whose pending context hash must still match.
    pub fn validate_decision_for_with_hash(
        &self,
        id: PrisonerId,
        decision: &Decision,
        context_hash: u64,
    ) -> DecisionValidation {
        let Some(ctx) = self.validation_context(id) else {
            return self.validate_decision_for(id, decision);
        };
        if context_hash != 0 && context_hash != hash_validation_context(&ctx) {
            return DecisionValidation {
                result: ValidationResult::Rejected {
                    reason: "stale context".to_string(),
                },
                stage: Some(ValidationStage::WorldState),
                reason: Some("stale context".to_string()),
                retryable: false,
            };
        }
        self.validate_decision_in_context(id, decision, &ctx)
    }

    fn engine_rules_for(&self, id: PrisonerId, decision: &Decision) -> Result<(), String> {
        match &decision.action {
            DecisionAction::MoveTo { target_facility } => {
                if self.policies.lockdown {
                    return Err("lockdown active: movement restricted".to_string());
                }
                if !self.facilities.iter().any(|f| f.id.0 == *target_facility) {
                    return Err(format!("unknown facility {target_facility}"));
                }
            }
            DecisionAction::Socialize { with } | DecisionAction::Trade { with, .. } => {
                if !self.prisoners.iter().any(|p| p.id.raw() == *with) {
                    return Err("unknown target prisoner".to_string());
                }
            }
            DecisionAction::Fight { target } => {
                if !self.prisoners.iter().any(|p| p.id.raw() == *target) {
                    return Err("unknown target prisoner".to_string());
                }
            }
            DecisionAction::SpreadRumor { about, content } => {
                if !self.prisoners.iter().any(|p| p.id.raw() == *about) {
                    return Err("unknown target prisoner".to_string());
                }
                if content.trim().is_empty() {
                    return Err("rumor content is empty".to_string());
                }
            }
            DecisionAction::JoinGang { gang } => {
                if !self.gangs.iter().any(|g| g.id.raw() == *gang) {
                    return Err(format!("unknown gang {gang}"));
                }
            }
            DecisionAction::LeadGang { gang } => {
                match self.gangs.iter().find(|g| g.id.raw() == *gang) {
                    None => return Err(format!("unknown gang {gang}")),
                    Some(g) if !g.members.iter().any(|member| *member == id) => {
                        return Err("prisoner is not a member of the gang".to_string());
                    }
                    Some(_) => {}
                }
            }
            DecisionAction::EscapeAttempt if self.policies.lockdown => {
                return Err("lockdown active: escape attempts blocked".to_string());
            }
            DecisionAction::EscapeAttempt
            | DecisionAction::RequestMedical
            | DecisionAction::Comply
            | DecisionAction::Refuse
            | DecisionAction::LeaveGang
            | DecisionAction::None => {}
        }
        Ok(())
    }

    /// Apply an already-validated decision. Rejected results are never applied.
    ///
    /// Returns generated events without emitting them; the caller (world tick
    /// or engine) remains responsible for bus/incident mirroring.
    pub fn apply_validated_decision(
        &mut self,
        id: PrisonerId,
        result: &ValidationResult,
    ) -> Vec<WorldEvent> {
        let mut decision = match result {
            ValidationResult::Accepted(decision) | ValidationResult::Repaired(decision) => {
                decision.clone()
            }
            ValidationResult::Rejected { .. } => return Vec::new(),
        };
        decision.outcome = match result {
            ValidationResult::Accepted(_) => DecisionOutcomeState::Accepted,
            ValidationResult::Repaired(_) => DecisionOutcomeState::Repaired,
            ValidationResult::Rejected { .. } => return Vec::new(),
        };
        let events = self.apply_decision_action(id, &decision);
        if self.prisoners.iter().any(|p| p.id == id) {
            self.current_decisions.insert(id, decision);
        }
        events
    }

    fn apply_decision_action(&mut self, id: PrisonerId, decision: &Decision) -> Vec<WorldEvent> {
        let mut events = Vec::new();
        match &decision.action {
            DecisionAction::MoveTo { target_facility } => {
                if let Some(facility) = self
                    .facilities
                    .iter()
                    .find(|f| f.id.0 == *target_facility)
                    .cloned()
                {
                    if let Some(prisoner) = self.prisoners.iter_mut().find(|p| p.id == id) {
                        prisoner.schedule.wing = facility.wing;
                    }
                    self.prisoner_location.insert(id, facility.id);
                }
            }
            DecisionAction::Socialize { with } => {
                self.social.record(
                    id,
                    PrisonerId(*with),
                    RelationshipDelta {
                        familiarity: 0.08,
                        friendship: 0.04,
                        trust: 0.02,
                        ..Default::default()
                    },
                );
                events.push(Self::decision_event(
                    self.time,
                    EventKind::RelationshipShift,
                    format!("{} socialized with {with}", id.raw()),
                    vec![id.raw()],
                ));
            }
            DecisionAction::Trade { with, item } => {
                let other = PrisonerId(*with);
                if self.prisoners.iter().any(|p| p.id == id)
                    && self.prisoners.iter().any(|p| p.id == other)
                {
                    let amount = Money::from_major(1);
                    let at = self.time.as_secs();
                    self.economy.record(
                        at,
                        Account::Supplies,
                        amount,
                        format!("trade credit: {} -> {} ({item})", id.raw(), with),
                    );
                    self.economy.record(
                        at,
                        Account::Supplies,
                        Money(-amount.0),
                        format!("trade debit: {} -> {} ({item})", id.raw(), with),
                    );
                }
                self.social.record(
                    id,
                    other,
                    RelationshipDelta {
                        familiarity: 0.12,
                        friendship: 0.03,
                        trust: 0.01,
                        ..Default::default()
                    },
                );
                events.push(Self::decision_event(
                    self.time,
                    EventKind::RelationshipShift,
                    format!("{} traded with {with} ({item})", id.raw()),
                    vec![id.raw(), *with],
                ));
            }
            DecisionAction::Fight { target } => {
                events.extend(self.fight_events(id, PrisonerId(*target)))
            }
            DecisionAction::EscapeAttempt => events.extend(self.escape_events(id)),
            DecisionAction::SpreadRumor { about, content } => {
                events.push(Self::decision_event(
                    self.time,
                    EventKind::Rumor,
                    format!("{} spreads rumor about {about}: {content}", id.raw()),
                    vec![id.raw()],
                ));
            }
            DecisionAction::JoinGang { gang } => {
                // Membership is authoritative in `World`: the API keeps the
                // prisoner field and both gang rosters consistent and writes
                // a durable Recruit history entry.
                self.join_gang(id, GangId(*gang));
            }
            DecisionAction::LeadGang { gang } => {
                let gid = GangId(*gang);
                if let Some(index) = self.gangs.iter().position(|g| g.id == gid) {
                    let old_leader = self.gangs[index].leader;
                    if self.gangs[index].change_leader(id) && old_leader != Some(id) {
                        self.record_gang_history(
                            gid,
                            GangHistoryKind::LeadershipChange,
                            format!("Prisoner {} took leadership of gang {}", id.raw(), gid.0),
                            &[id],
                            &[gang_event_cause(gid), "direct leadership decision".into()],
                        );
                    }
                }
            }
            DecisionAction::LeaveGang => {
                self.leave_gang(id);
            }
            DecisionAction::RequestMedical => {
                // A request is an attempt to obtain care, not a new injury.
                // Relieve the deficit slightly (bounded by staff coverage) so
                // repeated requests cannot create a runaway medical spiral.
                let relief = 0.12 * self.staff_coverage();
                if let Some(prisoner) = self.prisoners.iter_mut().find(|p| p.id == id) {
                    prisoner.needs.relieve(NeedTag::Medical, relief);
                }
            }
            DecisionAction::Comply | DecisionAction::Refuse | DecisionAction::None => {}
        }
        events
    }

    fn decision_event(
        at: SimTime,
        kind: EventKind,
        summary: String,
        actors: Vec<u64>,
    ) -> WorldEvent {
        let mut event = WorldEvent::new(0, at, kind, summary);
        for actor in actors {
            event = event.with_actor(actor);
        }
        event
    }

    fn fight_events(&mut self, a: PrisonerId, b: PrisonerId) -> Vec<WorldEvent> {
        let Some(pa) = self.prisoners.iter().find(|p| p.id == a).cloned() else {
            return Vec::new();
        };
        let Some(pb) = self.prisoners.iter().find(|p| p.id == b).cloned() else {
            return Vec::new();
        };
        let mut rng = RngStream::new(self.seed, self.time.as_secs());
        let tension = 0.5 + (pa.traits.aggression + pb.traits.aggression) * 0.15;
        let outcome = resolve_conflict(
            &mut rng,
            tension,
            0.15,
            pa.aggression_modifier(),
            pa.traits.discipline,
            0.1,
        );
        let gang_a = pa.gang_affiliation.map(GangId);
        let gang_b = pb.gang_affiliation.map(GangId);
        let (rival, _) = self.gang_tie(gang_a, gang_b);
        let context = match (rival, gang_a, gang_b) {
            (true, Some(left), Some(right)) => Some((left, right)),
            _ => None,
        };
        let effects = apply_outcome(
            &mut self.social,
            &mut self.gangs,
            &[a, b],
            outcome,
            0.6,
            context,
        );
        if let Some((left, right)) = context {
            self.append_gang_history(
                left,
                GangHistoryKind::MajorConflict,
                format!(
                    "Gangs {} and {} clashed in a direct confrontation ({:?})",
                    left.0, right.0, effects.incident
                ),
                &[a, b],
                &[gang_event_cause(left), gang_event_cause(right)],
            );
        }
        let kind = match outcome {
            ConflictOutcome::Compliance => None,
            ConflictOutcome::Retreat | ConflictOutcome::Argument => Some(EventKind::Argument),
            ConflictOutcome::Threat | ConflictOutcome::Intimidation | ConflictOutcome::Bullying => {
                Some(EventKind::Threat)
            }
            ConflictOutcome::Theft => Some(EventKind::Theft),
            ConflictOutcome::Fight => Some(EventKind::Fight),
            ConflictOutcome::GangClash | ConflictOutcome::Disturbance => Some(EventKind::GangClash),
        };
        kind.map_or(Vec::new(), |kind| {
            let mut event = Self::decision_event(
                self.time,
                kind,
                format!("conflict resolved: {outcome:?} ({:?})", effects.incident),
                vec![a.raw(), b.raw()],
            );
            if let Some((left, _)) = context {
                event = event.with_cause(gang_event_cause(left));
            }
            vec![event]
        })
    }

    /// Apply the institutional aftermath of an escape attempt. Both the daily
    /// deterministic path and the decision-driven path call this helper so
    /// security audits, lockdowns, investigations, causal notes and prisoner
    /// discipline are identical for every outcome.
    fn apply_escape_consequences_step(
        &mut self,
        consequences: &EscapeConsequences,
        id: PrisonerId,
        event: &mut WorldEvent,
        rng: &mut RngStream,
    ) {
        let sec_old = self.policies.security_level;
        self.policies.security_level = (self.policies.security_level
            + consequences.security_level_delta)
        .clamp(0.0, 0.95);
        self.audit_policy_change("security_level", sec_old, self.policies.security_level);

        if consequences.lockdown_recommended
            && rng.chance(consequences.policy_pressure as f64 * 0.35)
        {
            if !self.policies.lockdown {
                self.lockdown_since_day = self.days_simulated;
                self.audit_policy_change("lockdown", 0.0, 1.0);
            }
            self.policies.lockdown = true;
        }

        if consequences.investigation_opened {
            let investigation = Self::decision_event(
                self.time,
                EventKind::Investigation,
                format!(
                    "Investigation opened after escape attempt by prisoner {} (scope {:.2})",
                    id.raw(),
                    consequences.investigation_scope.clamp(0.0, 1.0)
                ),
                vec![id.raw()],
            )
            .with_cause(format!("escape outcome {:?}", consequences.outcome));
            let investigation_id = self.emit_history_event(investigation);
            event.causes.push(format!(
                "investigation {investigation_id} opened (scope {:.2})",
                consequences.investigation_scope.clamp(0.0, 1.0)
            ));
        }

        for note in &consequences.notes {
            event.causes.push(note.clone());
        }

        // A prevented attempt must still register as a disciplinary incident,
        // but without a permanent privilege/parole bar (Directive §16).
        if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == id) {
            p.needs.impulse(
                NeedTag::Respect,
                consequences.prisoner_discipline_severity * 0.2,
            );
            p.needs.impulse(
                NeedTag::Safety,
                consequences.prisoner_discipline_severity * 0.1,
            );
        }
    }

    fn escape_events(&mut self, id: PrisonerId) -> Vec<WorldEvent> {
        let Some(prisoner) = self.prisoners.iter().find(|p| p.id == id).cloned() else {
            return Vec::new();
        };
        let mut rng = RngStream::new(self.seed, self.time.as_secs());
        let outcome = resolve_escape_attempt(
            &mut rng,
            prisoner.escape_motivation(),
            self.policies.security_level,
            0.5,
            0.2,
        );
        let consequences = apply_escape_consequences(outcome, self.policies.security_level, &[id]);
        let kind = match outcome {
            EscapeOutcome::SuccessfulEscape => EventKind::EscapeSucceeded,
            EscapeOutcome::AttemptPrevented => EventKind::EscapePrevented,
            _ => EventKind::EscapeAttempt,
        };
        let mut event = Self::decision_event(
            self.time,
            kind,
            format!("escape attempt outcome: {outcome:?}"),
            vec![id.raw()],
        );
        self.apply_escape_consequences_step(&consequences, id, &mut event, &mut rng);

        if outcome == EscapeOutcome::SuccessfulEscape {
            self.record_escape_post_release(&prisoner);
            self.purge_gang_members(&[id]);
            self.prisoners.retain(|p| p.id != id);
            self.prune_departed_agent_state(&[id]);
            // `prune_departed_agent_state` skips the O(edges) social retain for
            // cheap routine releases; an escape must not leave stale dyads
            // observable before the next daily sweep.
            let live: std::collections::HashSet<PrisonerId> =
                self.prisoners.iter().map(|p| p.id).collect();
            self.social.retain(&live);
        }

        // The lifecycle Release event is already emitted with an authoritative
        // id by `record_escape_post_release`; returning the outcome event keeps
        // the engine's post-release ledger reconciliation intact.
        vec![event]
    }

    fn record_escape_post_release(&mut self, prisoner: &Prisoner) {
        let outcome_seed = RngStream::new(
            self.seed,
            prisoner.id.raw() ^ (self.days_simulated << 12) ^ 0x0E5C_A9E1,
        )
        .next_u64();
        let rehab_progress = self
            .rehab_records
            .get(&prisoner.id)
            .map(|records| RehabProgramRecord::progress_signal(records))
            .unwrap_or(0.0);
        let (employment, housing, violation, return_to_custody) = Self::post_release_outcomes(
            outcome_seed,
            prisoner.id.raw(),
            "escape",
            prisoner.sentence_days,
            prisoner.sentence_served_days,
            rehab_progress,
        );
        let record_id = self.next_post_release_id;
        self.next_post_release_id += 1;
        let record = PostReleaseRecord {
            id: record_id,
            prisoner_id: prisoner.id.raw(),
            name: prisoner.name.clone(),
            reason: "escape".to_string(),
            day: self.days_simulated,
            sentence_info: PostReleaseSentenceInfo {
                sentence_days: prisoner.sentence_days,
                sentence_served_days: prisoner.sentence_served_days,
                classification: prisoner.classification,
                escape_motivation_at_exit: prisoner.escape_motivation(),
            },
            outcome_seed,
            employment,
            housing,
            violation,
            return_to_custody,
        };
        self.post_release_outcomes.push(PostReleaseOutcomeRecord {
            id: record_id,
            post_release_id: record.id,
            prisoner_id: prisoner.id.raw(),
            name: prisoner.name.clone(),
            day: self.days_simulated,
            outcome_seed,
            employment,
            housing,
            violation,
            return_to_custody,
        });
        self.post_release_records.push(record);
        let event = Self::decision_event(
            self.time,
            EventKind::Release,
            format!(
                "Prisoner {} left the institution: escape",
                prisoner.id.raw()
            ),
            vec![prisoner.id.raw()],
        )
        .with_cause("lifecycle: post-release record");
        self.emit_history_event(event);
    }

    fn post_release_outcomes(
        seed: u64,
        prisoner_id: u64,
        reason: &str,
        sentence_days: u32,
        served_days: u32,
        rehab_progress: f32,
    ) -> (bool, bool, bool, bool) {
        let mut rng = RngStream::new(seed, prisoner_id ^ 0x9E37_79B9_7F4A_7C15);
        let served_fraction = (served_days as f32 / sentence_days.max(1) as f32).clamp(0.0, 1.0);
        let stability =
            (served_fraction * 0.55 + rehab_progress.clamp(0.0, 1.0) * 0.45).clamp(0.0, 0.95);
        let penalty = if reason == "escape" { 0.3 } else { 0.0 };
        let employment = rng.chance((stability * 0.85 + 0.05).min(0.95) as f64);
        let housing = rng.chance((stability * 0.8 + 0.1).min(0.95) as f64);
        let violation = rng.chance((0.32 - stability * 0.27 + penalty).clamp(0.02, 0.9) as f64);
        let return_to_custody = violation && rng.chance(0.55);
        (employment, housing, violation, return_to_custody)
    }

    /// Submit a bounded batch of decision proposals through the canonical
    /// gate. Only accepted/repaired decisions are applied.
    pub fn submit_ai_batch(&mut self, proposals: Vec<(PrisonerId, Decision)>) -> AiBatchOutcome {
        self.submit_ai_batch_with_contexts(
            proposals
                .into_iter()
                .map(|(id, decision)| (id, decision, 0))
                .collect(),
        )
    }

    /// Batch variant that rejects proposals whose pending context hash no
    /// longer matches the canonical validation slice.
    pub fn submit_ai_batch_with_contexts(
        &mut self,
        proposals: Vec<(PrisonerId, Decision, u64)>,
    ) -> AiBatchOutcome {
        let cap = self.ai_config().max_batch_size.max(1);
        let mut selected: BTreeMap<u64, (PrisonerId, Decision, u64)> = BTreeMap::new();
        for (id, decision, context_hash) in proposals {
            if self.prisoners.iter().any(|p| p.id == id) {
                selected.insert(id.raw(), (id, decision, context_hash));
            }
        }
        while selected.len() > cap {
            selected.pop_last();
        }

        let mut outcome = AiBatchOutcome::default();
        // Capture every context before applying anything: one agent's move
        // must not change another agent's validation slice within a batch.
        let mut prepared = Vec::with_capacity(selected.len());
        for (_, (id, decision, context_hash)) in selected {
            match self.validation_context(id) {
                Some(ctx) => prepared.push((id, decision, ctx, context_hash)),
                None => outcome.rejected += 1,
            }
        }
        self.orchestrator.record_batch(prepared.len());

        let mut generated = Vec::new();
        for (id, decision, ctx, context_hash) in prepared {
            let validation = if context_hash != 0 && context_hash != hash_validation_context(&ctx) {
                DecisionValidation {
                    result: ValidationResult::Rejected {
                        reason: "stale context".to_string(),
                    },
                    stage: Some(ValidationStage::WorldState),
                    reason: Some("stale context".to_string()),
                    retryable: false,
                }
            } else {
                self.validate_decision_in_context(id, &decision, &ctx)
            };

            let repaired = matches!(validation.result, ValidationResult::Repaired(_));
            let accepted = validation.is_accepted();
            if accepted {
                let validated = validation
                    .accepted_decision()
                    .cloned()
                    .expect("accepted validation carries a decision");
                let apply_result = if repaired {
                    ValidationResult::Repaired(validated)
                } else {
                    ValidationResult::Accepted(validated)
                };
                generated.extend(self.apply_validated_decision(id, &apply_result));
                self.orchestrator.record_validated(repaired);
                outcome.accepted += 1;
            } else {
                self.orchestrator.record_rejected();
                outcome.rejected += 1;
            }
            outcome.entries.push(AiBatchEntry {
                prisoner_id: id,
                result: validation.result,
                applied: accepted,
            });
            self.orchestrator.pending.retain(|p| p.agent != id.raw());
        }

        self.emit_batch_events(generated);
        outcome
    }

    fn emit_batch_events(&mut self, events: Vec<WorldEvent>) {
        for mut event in events {
            event.at = self.time;
            event.id = self.events.emit(event.clone());
        }
    }

    fn decision_candidates_from_world(&self, id: PrisonerId) -> Vec<ActionCandidate> {
        let Some(prisoner) = self.prisoners.iter().find(|p| p.id == id) else {
            return Vec::new();
        };
        let ids: Vec<PrisonerId> = {
            let mut ids: Vec<PrisonerId> = self.prisoners.iter().map(|p| p.id).collect();
            ids.sort_by_key(|pid| pid.raw());
            ids
        };
        let locations = self.prisoner_location.clone();
        self.decision_candidates(prisoner, &ids, &locations)
    }

    // -----------------------------------------------------------------------
    // Step 4 — need-driven decisions (Directive §3.7, §19)
    // -----------------------------------------------------------------------

    fn make_decisions_step(&mut self, now_secs: u64) {
        // Locations are refreshed from the schedule first, then overridden by
        // any MoveTo decision below, so co-presence reflects actual intent.
        self.assign_locations_from_schedule();

        let ids: Vec<PrisonerId> = self.prisoners.iter().map(|p| p.id).collect();
        let locations = self.prisoner_location.clone();

        // Build candidates from read-only views first (no in-flight borrows).
        // Capture each prisoner's personality as a Disposition so traits shape
        // both AI-ranked and deterministic decisions (Directive §3/§19).
        let plans: Vec<(PrisonerId, Disposition, Vec<ActionCandidate>)> = self
            .prisoners
            .iter()
            .map(|p| {
                (
                    p.id,
                    Disposition::from(p),
                    self.decision_candidates(p, &ids, &locations),
                )
            })
            .collect();

        // AI deliberation is a scheduled, budgeted privilege. Agents that do
        // not receive a slot still decide deterministically, but their
        // decisions are never labelled as AI output.
        let granted: std::collections::BTreeSet<u64> = self
            .orchestrator
            .take_ai_slots(now_secs)
            .into_iter()
            .collect();
        let mut proposals = Vec::with_capacity(plans.len());
        for (id, disposition, candidates) in plans {
            let mut decision = if granted.contains(&id.raw()) {
                match self
                    .provider_context_with_candidates(id, candidates.clone())
                    .and_then(|request| {
                        let validation_hash = request.validation_hash;
                        self.orchestrator
                            .propose(request, now_secs)
                            .map(|decision| (decision, validation_hash))
                    }) {
                    Some((decision, validation_hash)) => {
                        self.orchestrator.submit_pending(PendingDecision::new(
                            id.raw(),
                            now_secs,
                            decision.clone(),
                            validation_hash,
                        ));
                        decision
                    }
                    None => {
                        self.orchestrator.record_fallback();
                        self.orchestrator.telemetry.utility_decisions += 1;
                        self.orchestrator
                            .fallback_decision_with_disposition(&candidates, &disposition)
                    }
                }
            } else {
                self.orchestrator.telemetry.utility_decisions += 1;
                self.orchestrator
                    .fallback_decision_with_disposition(&candidates, &disposition)
            };
            decision.provenance = if granted.contains(&id.raw())
                && decision.provenance != DecisionProvenance::System
            {
                DecisionProvenance::Agent
            } else {
                DecisionProvenance::System
            };
            // Outcome stays Proposed until the canonical gate accepts it.
            decision.outcome = alc_agents::decision::DecisionOutcomeState::Proposed;
            proposals.push((id, decision));
        }

        // Bounded batches, one canonical validation slice per agent, one apply
        // path. Rejected proposals never reach `current_decisions`. Chunking
        // keeps the batch cap configurable without starving large populations.
        let batch_size = self.ai_config().max_batch_size.max(1);
        for chunk in proposals.chunks(batch_size) {
            let _ = self.submit_ai_batch(chunk.to_vec());
        }
    }

    /// Translate a prisoner's dominant need into a ranked action set. The
    /// motivated action outranks the baselines whenever the need is non-trivial.
    fn decision_candidates(
        &self,
        p: &Prisoner,
        all_ids: &[PrisonerId],
        locations: &HashMap<PrisonerId, FacilityId>,
    ) -> Vec<ActionCandidate> {
        let (need, level) = p.needs.dominant();
        let level = level.clamp(0.0, 1.0);
        let my_loc = locations.get(&p.id);
        let nearby = all_ids
            .iter()
            .copied()
            .find(|&other| other != p.id && locations.get(&other) == my_loc);

        let motivated = match need {
            NeedTag::Hunger | NeedTag::Thirst => {
                self.facility_of_kind(FacilityKind::Kitchen).map(|f| {
                    ActionCandidate::new(
                        Decision::new(alc_agents::decision::DecisionAction::MoveTo {
                            target_facility: f.raw(),
                        }),
                        0.2 + level,
                    )
                })
            }
            NeedTag::Sleep => self.facility_of_kind(FacilityKind::CellBlock).map(|f| {
                ActionCandidate::new(
                    Decision::new(alc_agents::decision::DecisionAction::MoveTo {
                        target_facility: f.raw(),
                    }),
                    0.2 + level,
                )
            }),
            NeedTag::Medical => Some(ActionCandidate::new(
                Decision::new(alc_agents::decision::DecisionAction::RequestMedical),
                0.2 + level * 1.3,
            )),
            NeedTag::Companionship => nearby.map(|other| {
                ActionCandidate::new(
                    Decision::new(alc_agents::decision::DecisionAction::Socialize {
                        with: other.raw(),
                    }),
                    0.2 + level,
                )
            }),
            NeedTag::Stimulation => self
                .facility_of_kind(FacilityKind::Recreation)
                .or_else(|| self.facility_of_kind(FacilityKind::Yard))
                .map(|f| {
                    ActionCandidate::new(
                        Decision::new(alc_agents::decision::DecisionAction::MoveTo {
                            target_facility: f.raw(),
                        }), // wander
                        0.2 + level,
                    )
                }),
            NeedTag::Safety => Some(ActionCandidate::new(
                Decision::new(alc_agents::decision::DecisionAction::Comply), // safest visible option: keep close to staff/order
                0.2 + level,
            )),
            NeedTag::Respect => nearby.map(|other| {
                ActionCandidate::new(
                    Decision::new(alc_agents::decision::DecisionAction::Fight {
                        target: other.raw(),
                    }),
                    0.15 + level * 0.5,
                )
                .with_risk(0.85)
                .with_legality(Legality::IllegalButPossible)
            }),
            NeedTag::Fatigue => self.facility_of_kind(FacilityKind::CellBlock).map(|f| {
                ActionCandidate::new(
                    Decision::new(alc_agents::decision::DecisionAction::MoveTo {
                        target_facility: f.raw(),
                    }),
                    0.2 + level,
                )
            }),
            // The dominant-need chooser only surfaces the eight weighted tags
            // above; every other need falls back to routine compliance.
            _ => None,
        };

        let mut out = Vec::with_capacity(4);
        if let Some(c) = motivated {
            out.push(c);
        }
        // Baseline alternatives give the fallback chooser a real ranking and
        // keep routine behaviour from collapsing into a single action.
        out.push(ActionCandidate::new(
            Decision::new(alc_agents::decision::DecisionAction::Comply),
            0.25,
        ));
        if p.needs.medical_need > 0.6 {
            out.push(ActionCandidate::new(
                Decision::new(alc_agents::decision::DecisionAction::RequestMedical),
                0.55,
            ));
        }
        out.push(ActionCandidate::new(
            Decision::new(alc_agents::decision::DecisionAction::None),
            0.05,
        ));
        if p.condition.hope < 0.05 {
            // MoveTo has a stamina cost of 0.05; don't offer proposals that
            // can never pass canonical resource validation.
            out.retain(|c| {
                !matches!(
                    c.decision.action,
                    alc_agents::decision::DecisionAction::MoveTo { .. }
                )
            });
        }
        out
    }

    // -----------------------------------------------------------------------
    // Step 5 — co-presence encounters (Directive §5.4, §6)
    // -----------------------------------------------------------------------

    fn resolve_encounters_step(&mut self) {
        // Cadence: hourly. Daily life is continuous, but relationship changes
        // are discrete and auditable rather than a per-minute dice storm.
        if self.time.as_mins() % 60 != 0 {
            return;
        }

        #[derive(Clone, Copy)]
        struct Snap {
            sociability: f32,
            aggression: f32,
            manipulation: f32,
            respect: f32,
            stimulation: f32,
            gang: Option<GangId>,
        }

        let pressure = self.gang_pressure().total();
        let snaps: HashMap<PrisonerId, Snap> = self
            .prisoners
            .iter()
            .map(|p| {
                (
                    p.id,
                    Snap {
                        sociability: p.traits.sociability,
                        aggression: p.traits.aggression,
                        manipulation: p.traits.manipulation,
                        respect: p.needs.respect,
                        stimulation: p.needs.stimulation,
                        gang: p.gang_affiliation.map(GangId),
                    },
                )
            })
            .collect();

        // Build groups from the prisoner Vec (stable insertion order) and sort
        // facility keys: HashMap iteration order is randomized per process and
        // would otherwise make event traces non-reproducible (Directive §21).
        let mut by_facility: HashMap<FacilityId, Vec<PrisonerId>> = HashMap::new();
        for p in &self.prisoners {
            if let Some(&fid) = self.prisoner_location.get(&p.id) {
                by_facility.entry(fid).or_default().push(p.id);
            }
        }
        let mut facility_ids: Vec<FacilityId> = by_facility.keys().copied().collect();
        facility_ids.sort_by_key(|f| f.0);

        let mut rng = RngStream::new(self.seed, self.time.as_secs() ^ 0xE4C0);
        let mut need_effects: Vec<(PrisonerId, NeedTag, f32, bool)> = Vec::new(); // (id, need, amount, relieve)

        for fid in facility_ids {
            let ids = &by_facility[&fid];
            // Co-located guards materially reduce the chance that a tense
            // dyad escalates; re-posting a guard therefore changes incident
            // likelihood without scripting a specific encounter.
            let deterrence = self.guard_deterrence(fid);
            for (i, &a) in ids.iter().enumerate() {
                for &b in ids.iter().skip(i + 1) {
                    let (Some(sa), Some(sb)) = (snaps.get(&a), snaps.get(&b)) else {
                        continue;
                    };
                    let social = (sa.sociability + sb.sociability) * 0.5;
                    if !rng.chance(0.008 + social as f64 * 0.014) {
                        continue;
                    }

                    let (rival, friendly) = self.gang_tie(sa.gang, sb.gang);
                    let hostility = clamp01(
                        ((sa.aggression + sb.aggression) * 0.5 + (sa.respect + sb.respect) * 0.15)
                            * 0.45
                            * (1.0 - deterrence)
                            + if rival { 0.18 } else { 0.0 }
                            + pressure * 0.05
                            - if friendly { 0.08 } else { 0.0 },
                    );
                    let rumor_bias = 0.12 + (sa.manipulation + sb.manipulation) * 0.12;
                    let roll = rng.next_f64() as f32;
                    let mut delta = RelationshipDelta::default();
                    let mut reverse = RelationshipDelta::default();
                    let mut cause = format!(
                        "co-presence encounter; stimulation {:.2}/{:.2}, gang pressure {:.2}",
                        sa.stimulation, sb.stimulation, pressure
                    );
                    let (kind, summary) = if roll < hostility {
                        if rival {
                            delta.hostility = 0.09;
                            delta.rivalry = 0.05;
                            delta.resentment = 0.04;
                            reverse.hostility = 0.08;
                            reverse.rivalry = 0.05;
                            reverse.resentment = 0.05;
                            if let Some(gid) = sa.gang {
                                cause.push_str(&format!("; {}", gang_event_cause(gid)));
                            }
                            (
                                EventKind::GangClash,
                                format!("Gang clash between {} and {}", a.raw(), b.raw()),
                            )
                        } else {
                            delta.hostility = 0.06;
                            delta.resentment = 0.04;
                            reverse.hostility = 0.05;
                            reverse.resentment = 0.05;
                            (
                                EventKind::Argument,
                                format!("Argument between {} and {}", a.raw(), b.raw()),
                            )
                        }
                    } else if roll < hostility + 0.22 {
                        delta.familiarity = 0.05;
                        delta.trust = 0.02;
                        reverse.familiarity = 0.05;
                        reverse.trust = 0.02;
                        need_effects.push((a, NeedTag::Companionship, 0.05, true));
                        need_effects.push((b, NeedTag::Companionship, 0.05, true));
                        (
                            EventKind::RelationshipShift,
                            format!("Trade between {} and {}", a.raw(), b.raw()),
                        )
                    } else if roll < hostility + 0.22 + rumor_bias {
                        delta.familiarity = 0.03;
                        reverse.familiarity = 0.03;
                        (
                            EventKind::Rumor,
                            format!("Rumor exchanged between {} and {}", a.raw(), b.raw()),
                        )
                    } else {
                        delta.friendship = 0.05;
                        delta.trust = 0.03;
                        delta.familiarity = 0.04;
                        reverse.friendship = 0.05;
                        reverse.trust = 0.03;
                        reverse.familiarity = 0.04;
                        need_effects.push((a, NeedTag::Companionship, 0.08, true));
                        need_effects.push((b, NeedTag::Companionship, 0.08, true));
                        (
                            EventKind::RelationshipShift,
                            format!("Friendship grew between {} and {}", a.raw(), b.raw()),
                        )
                    };

                    if friendly {
                        delta.loyalty = 0.03;
                        reverse.loyalty = 0.03;
                        delta.trust += 0.01;
                        reverse.trust += 0.01;
                    }
                    self.social.record(a, b, delta);
                    self.social.record(b, a, reverse);
                    let ev = WorldEvent::new(0, self.time, kind, summary)
                        .with_actor(a.raw())
                        .with_actor(b.raw())
                        .with_cause(cause);
                    let _ = self.events.emit(ev);
                }
            }
        }

        for (id, need, amount, relieve) in need_effects {
            if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == id) {
                if relieve {
                    p.needs.relieve(need, amount);
                } else {
                    p.needs.impulse(need, amount);
                }
            }
        }
    }

    /// Remove departed prisoners (release, escape, death) from every gang
    /// roster. `Gang::lose` reassigns leadership when the leader leaves; gangs
    /// that lose all their members dissolve.
    fn purge_gang_members(&mut self, departed: &[PrisonerId]) {
        if departed.is_empty() {
            return;
        }
        for index in 0..self.gangs.len() {
            let was_member = departed
                .iter()
                .any(|pid| self.gangs[index].members.contains(pid));
            if !was_member {
                continue;
            }
            let gid = self.gangs[index].id;
            let old_leader = self.gangs[index].leader;
            for &pid in departed {
                self.gangs[index].lose(pid);
            }
            let new_leader = self.gangs[index].leader;
            self.record_gang_history(
                gid,
                GangHistoryKind::Loss,
                format!("Gang {} lost {} departed member(s)", gid.0, departed.len()),
                departed,
                &[gang_event_cause(gid), "prisoner departure".into()],
            );
            if old_leader.is_some() && old_leader != new_leader {
                let summary = match new_leader {
                    Some(leader) => format!(
                        "Gang {} leadership passed to {} after departure",
                        gid.0,
                        leader.raw()
                    ),
                    None => format!("Gang {} lost its leader after departure", gid.0),
                };
                let actors = new_leader.into_iter().collect::<Vec<_>>();
                self.record_gang_history(
                    gid,
                    GangHistoryKind::LeadershipChange,
                    summary,
                    &actors,
                    &[gang_event_cause(gid), "leader departure".into()],
                );
            }
        }
        self.prune_empty_gangs();
    }

    /// Build one politics profile per gang member, directly from traits/needs
    /// and the observed leader relationship (no omniscient world knowledge).
    fn politics_profiles(&self, gang_index: usize) -> Vec<GangPoliticsProfile> {
        let leader = self.gangs[gang_index].leader;
        let mut members = self.gangs[gang_index].members.clone();
        members.sort_unstable_by_key(|p| p.0);
        members
            .into_iter()
            .filter_map(|id| {
                let p = self.prisoners.iter().find(|p| p.id == id)?;
                let rel = leader.map_or_else(Relationship::default, |leader| {
                    self.social.get(id, leader).clone()
                });
                Some(GangPoliticsProfile {
                    prisoner: id,
                    loyalty: clamp01(
                        p.traits.loyalty * 0.65
                            + rel.loyalty * 0.2
                            + rel.trust * 0.1
                            + p.needs.safety * 0.05,
                    ),
                    autonomy: clamp01(
                        0.2 + p.needs.autonomy * 0.45
                            + (1.0 - p.traits.discipline) * 0.2
                            + p.needs.freedom * 0.15,
                    ),
                    discipline: clamp01(p.traits.discipline),
                    leadership: clamp01(p.traits.leadership),
                    ambition: clamp01(p.traits.ambition),
                    goal_conflict: clamp01(
                        (1.0 - p.needs.stability) * 0.25
                            + p.needs.freedom * 0.2
                            + p.needs.autonomy * 0.15
                            + (1.0 - p.condition.hope) * 0.15
                            + (1.0 - p.traits.loyalty) * 0.25,
                    ),
                })
            })
            .collect()
    }

    fn gang_directive(
        &self,
        gang_index: usize,
        pressure: alc_social::GangPressure,
    ) -> GroupDirective {
        let gang = &self.gangs[gang_index];
        GroupDirective {
            description: format!("weekly expectations of {}", gang.name),
            pressure: clamp01(0.26 + pressure.total() * 0.34 + self.policies.security_level * 0.16),
            expected_gain: clamp01(gang.resources * 0.3 + self.policies.privileges_level * 0.2),
            personal_risk: clamp01(
                self.policies.security_level * 0.4
                    + gang.rivalries.len() as f32 * 0.06
                    + pressure.rivalry * 0.18,
            ),
        }
    }

    /// Weekly gang-politics pass (Spec 008). Pure helpers decide disputes,
    /// leadership challenges, fractures and inter-gang relations; only this
    /// world-owned method mutates state and records explainable causes.
    pub fn gang_politics_step(&mut self) {
        self.reconcile_gang_membership();
        if self.gangs.is_empty() {
            return;
        }
        let pressure = self.gang_pressure();
        let mut rng = RngStream::new(self.seed, self.days_simulated ^ 0x6011_C5A7);
        let mut gang_ids: Vec<GangId> = self
            .gangs
            .iter()
            .filter(|g| g.active)
            .map(|g| g.id)
            .collect();
        gang_ids.sort_unstable_by_key(|g| g.0);

        // Internal disagreement is resolved per gang, in id order.
        for gid in &gang_ids {
            let Some(index) = self.gangs.iter().position(|g| g.id == *gid) else {
                continue;
            };
            if !self.gangs[index].active {
                continue;
            }
            let directive = self.gang_directive(index, pressure);
            let profiles = self.politics_profiles(index);
            let disputes =
                detect_internal_disputes(&self.gangs[index], &self.social, &profiles, &directive);
            if disputes.is_empty() {
                continue;
            }

            for dispute in &disputes {
                if let Some(record) = self.gangs[index]
                    .hierarchy
                    .iter_mut()
                    .find(|r| r.prisoner == dispute.prisoner)
                {
                    let delta = if dispute.action == DisputeAction::Negotiate {
                        -0.05
                    } else {
                        dispute.grievance_delta
                    };
                    record.grievances = clamp01(record.grievances + delta);
                }
                match dispute.action {
                    DisputeAction::Refuse => {
                        self.gangs[index].reputation = clamp01(self.gangs[index].reputation - 0.02);
                        if let Some(p) =
                            self.prisoners.iter_mut().find(|p| p.id == dispute.prisoner)
                        {
                            p.needs.impulse(NeedTag::Respect, -0.02);
                        }
                    }
                    DisputeAction::Covert => {
                        self.gangs[index].resources += 0.02;
                        if let Some(p) =
                            self.prisoners.iter_mut().find(|p| p.id == dispute.prisoner)
                        {
                            p.traits.manipulation = clamp01(p.traits.manipulation + 0.01);
                            p.needs.relieve(NeedTag::Freedom, 0.01);
                        }
                    }
                    DisputeAction::Negotiate => {
                        if let Some(p) =
                            self.prisoners.iter_mut().find(|p| p.id == dispute.prisoner)
                        {
                            p.needs.relieve(NeedTag::Respect, 0.02);
                            p.needs.relieve(NeedTag::Safety, 0.02);
                        }
                    }
                    DisputeAction::ChallengeLeader => {
                        if let Some(p) =
                            self.prisoners.iter_mut().find(|p| p.id == dispute.prisoner)
                        {
                            p.needs.impulse(NeedTag::Respect, 0.02);
                        }
                    }
                }
            }

            let actors: Vec<PrisonerId> = disputes.iter().map(|d| d.prisoner).collect();
            let summary = format!(
                "{} member dispute(s) in gang {}: {}",
                disputes.len(),
                gid.0,
                disputes
                    .iter()
                    .map(|d| format!("{} ({})", d.prisoner.raw(), d.reason))
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            self.gangs[index].record_dispute(summary.clone());
            self.record_gang_history(
                *gid,
                GangHistoryKind::Dispute,
                summary,
                &actors,
                &[gang_event_cause(*gid), "weekly politics review".into()],
            );

            let challenger = leadership_challenger(&self.gangs[index], &profiles, &mut rng);
            if let Some(challenger) = challenger {
                let challenges = disputes.iter().any(|d| {
                    d.prisoner == challenger && d.action == DisputeAction::ChallengeLeader
                });
                if challenges && rng.chance(0.55) {
                    let old_leader = self.gangs[index].leader;
                    if self.gangs[index].change_leader(challenger) && old_leader != Some(challenger)
                    {
                        self.record_gang_history(
                            *gid,
                            GangHistoryKind::LeadershipChange,
                            format!(
                                "Gang {} leadership changed from {:?} to {}",
                                gid.0,
                                old_leader.map(PrisonerId::raw),
                                challenger.raw()
                            ),
                            &[challenger],
                            &[gang_event_cause(*gid), "leadership challenge".into()],
                        );
                    }
                }
            }

            if let Some(split) = split_candidates(&self.gangs[index], &disputes) {
                if rng.chance((0.22 + pressure.total() * 0.25) as f64) {
                    let new_id = GangId(self.next_gang_id);
                    self.next_gang_id = self.next_gang_id.saturating_add(1);
                    if let Some(splinter) = self.gangs[index].fracture(&split, new_id) {
                        // Leadership rank in the splinter is already authoritative;
                        // mirror the roster onto every affected affiliation.
                        let new_members = splinter.members.clone();
                        let new_name = splinter.name.clone();
                        self.gangs.push(splinter);
                        for pid in &new_members {
                            if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == *pid) {
                                p.gang_affiliation = Some(new_id.0);
                            }
                        }
                        self.record_gang_history(
                            *gid,
                            GangHistoryKind::Split,
                            format!("Gang {} fractured into {}", gid.0, new_name),
                            &new_members,
                            &[gang_event_cause(*gid), "shared member grievances".into()],
                        );
                        self.record_gang_history(
                            new_id,
                            GangHistoryKind::Formed,
                            format!("Splinter gang {} formed from {}", new_name, gid.0),
                            &new_members,
                            &[gang_event_cause(new_id), gang_event_cause(*gid)],
                        );
                    }
                }
            }
        }

        // Inter-gang relations: affinity/alliance vs hostility/rivalry. Scores
        // come only from observed relationships, goals and declared ties.
        let active: Vec<(usize, GangId)> = self
            .gangs
            .iter()
            .enumerate()
            .filter(|(_, gang)| gang.active)
            .map(|(index, gang)| (index, gang.id))
            .collect();
        let mut pairs: Vec<(usize, usize, GangId, GangId)> = Vec::new();
        for (offset, (left_index, left_id)) in active.iter().enumerate() {
            for (right_index, right_id) in active.iter().skip(offset + 1) {
                pairs.push((*left_index, *right_index, *left_id, *right_id));
            }
        }
        pairs.sort_unstable_by_key(|(_, _, a, b)| (a.0, b.0));
        let mut merge_candidates: Vec<(GangId, GangId)> = Vec::new();
        for (i, j, gid_a, gid_b) in pairs {
            let (affinity, hostility) =
                gang_relation_scores(&self.gangs[i], &self.gangs[j], &self.social);
            let causes = vec![
                gang_event_cause(gid_a),
                gang_event_cause(gid_b),
                format!("affinity {affinity:.2}, hostility {hostility:.2}"),
            ];
            let already_rival = self.gangs[i].rivalries.contains(&gid_b)
                || self.gangs[j].rivalries.contains(&gid_a);
            let already_allied = self.gangs[i].alliances.contains(&gid_b)
                || self.gangs[j].alliances.contains(&gid_a);
            if hostility > 0.55
                && affinity < 0.5
                && !already_rival
                && rng.chance((0.12 + pressure.rivalry * 0.3) as f64)
            {
                {
                    let (left, right) = self.gangs.split_at_mut(j);
                    left[i].form_rivalry(&mut right[0]);
                }
                self.record_gang_history(
                    gid_a,
                    GangHistoryKind::Rivalry,
                    format!("Gang {} became rivals with {}", gid_a.0, gid_b.0),
                    &[],
                    &causes,
                );
            } else if affinity > 0.6
                && hostility < 0.4
                && !already_allied
                && rng.chance((0.1 + (1.0 - pressure.rivalry) * 0.16) as f64)
            {
                {
                    let (left, right) = self.gangs.split_at_mut(j);
                    left[i].form_alliance(&mut right[0]);
                }
                self.record_gang_history(
                    gid_a,
                    GangHistoryKind::Alliance,
                    format!("Gang {} formed an alliance with {}", gid_a.0, gid_b.0),
                    &[],
                    &causes,
                );
            } else if already_allied
                && affinity > 0.78
                && hostility < 0.2
                && self.gangs[i].members.len() + self.gangs[j].members.len() <= 10
                && rng.chance((0.04 + (1.0 - pressure.rivalry) * 0.04) as f64)
            {
                let (source, target) = if self.gangs[i].members.len() <= self.gangs[j].members.len()
                {
                    (gid_a, gid_b)
                } else {
                    (gid_b, gid_a)
                };
                merge_candidates.push((source, target));
            }
        }
        merge_candidates.sort_unstable_by_key(|(source, target)| (source.0, target.0));
        for (source, target) in merge_candidates {
            self.merge_gangs(source, target);
        }

        // A high-pressure rivalry can escalate into a major conflict. Outcomes
        // are resolved by the social layer with explicit gang context, then
        // reflected in institutional security posture.
        if pressure.total() > 0.4 && rng.chance((0.08 + pressure.total() * 0.2) as f64) {
            let rival_pair = self.gangs.iter().enumerate().find_map(|(i, a)| {
                if !a.active {
                    return None;
                }
                self.gangs
                    .iter()
                    .enumerate()
                    .skip(i + 1)
                    .find(|(_, b)| {
                        b.active && (a.rivalries.contains(&b.id) || b.rivalries.contains(&a.id))
                    })
                    .map(|(j, b)| (i, j, a.id, b.id))
            });
            if let Some((i, j, gid_a, gid_b)) = rival_pair {
                if let (Some(&a), Some(&b)) =
                    (self.gangs[i].members.first(), self.gangs[j].members.first())
                {
                    let severity = clamp01(0.4 + pressure.total() * 0.5);
                    let effects = apply_outcome(
                        &mut self.social,
                        &mut self.gangs,
                        &[a, b],
                        ConflictOutcome::Fight,
                        severity,
                        Some((gid_a, gid_b)),
                    );
                    let causes = vec![
                        gang_event_cause(gid_a),
                        gang_event_cause(gid_b),
                        format!("gang pressure {:.2}", pressure.total()),
                    ];
                    self.append_gang_history(
                        gid_a,
                        GangHistoryKind::MajorConflict,
                        format!("Gang {} clashed with {}", gid_a.0, gid_b.0),
                        &[a, b],
                        &causes,
                    );
                    self.record_gang_history(
                        gid_b,
                        GangHistoryKind::MajorConflict,
                        format!(
                            "Gang {} clashes with {} (severity {:.2}{})",
                            gid_b.0,
                            gid_a.0,
                            severity,
                            if effects.investigation_recommended {
                                ", investigation recommended"
                            } else {
                                ""
                            }
                        ),
                        &[a, b],
                        &causes,
                    );
                    self.warden.security_incidents_this_week =
                        self.warden.security_incidents_this_week.saturating_add(1);
                    let old_security = self.policies.security_level;
                    self.policies.security_level = clamp01(old_security + 0.04 * severity);
                    self.audit_policy_change(
                        "security_level",
                        old_security,
                        self.policies.security_level,
                    );
                }
            }
        }
    }

    /// Emergent gang formation scan (Directive §7). Periodically looks for a
    /// cluster of unaffiliated prisoners whose relationships, shared background
    /// and protection needs justify a new organization. Nothing is scripted.
    fn maybe_form_gang_step(&mut self) {
        let profiles: Vec<PrisonerGangProfile> = self
            .prisoners
            .iter()
            .map(|p| PrisonerGangProfile {
                id: p.id,
                gang_affiliation: p.gang_affiliation,
                background: p.background.clone(),
                protection_need: p.needs.safety.clamp(0.0, 1.0),
                safety_need: p.needs.safety.clamp(0.0, 1.0),
                ambition: p.traits.ambition.clamp(0.0, 1.0),
                sociability: p.traits.sociability.clamp(0.0, 1.0),
            })
            .collect();

        let mut rng = RngStream::new(self.seed, self.days_simulated ^ 0x5A17);
        if let Some(gang) = maybe_form_gang(&profiles, &self.social, &self.gangs, &mut rng) {
            self.register_gang(gang);
        }
    }

    // -----------------------------------------------------------------------
    // Step 6 — daily boundary (economy, escapes, parole, warden, intake)
    // -----------------------------------------------------------------------

    fn on_day_boundary(&mut self) {
        self.days_simulated += 1;

        // Daily funding + expenses.
        let pop = self.population();
        let buildings = self.facilities.len() as u32;
        self.economy.accrue_daily_funding(self.time.as_secs(), pop);
        self.economy.accrue_daily_expenses(
            self.time.as_secs(),
            pop,
            self.guards.len() as u32,
            buildings,
        );

        // Facility degradation.
        for f in &mut self.facilities {
            f.condition = (f.condition - 0.001).max(0.0);
        }

        // Sentence progress and finite disciplinary sanctions. A dated
        // loss-of-privileges record expires after the configured window; this
        // prevents a single contraband finding from barring parole forever.
        for p in &mut self.prisoners {
            p.sentence_served_days += 1;
            if p.schedule.disciplinary_status {
                if let Some(last) = p.discipline_history.last() {
                    if self.days_simulated.saturating_sub(u64::from(last.day))
                        >= DISCIPLINARY_STATUS_DAYS
                    {
                        p.schedule.disciplinary_status = false;
                    }
                }
            }
        }

        // Escape attempts for the genuinely desperate (Directive §16).
        self.resolve_escape_step();

        // Parole hearings (Directive §15).
        self.resolve_parole_step();

        // Rehabilitation programs (Directive §14).
        self.rehab_step();

        // Contraband market and discovery (Directive §7, §17).
        self.contraband_step();

        // Construction progress (0.05 lifecycle).
        self.construction_step();

        // Condition-driven emergent events (Spec 007).
        self.emergent_events_step();

        // Refresh warden context and reset trailing-week incident count weekly.
        if self.days_simulated % 7 == 0 {
            // Gang politics runs before de-escalation so leadership disputes,
            // splits and rivalries can shape the new security posture.
            self.gang_politics_step();
            let gang_pressure = self.gang_pressure().total();
            let quiet_week = self.warden.security_incidents_this_week <= 2 && gang_pressure < 0.5;
            self.warden.reset_weekly_incidents();
            // Sustained gang pressure keeps the institution on edge even when
            // no single incident crossed the reporting threshold.
            if gang_pressure > 0.6 {
                let old = self.policies.security_level;
                self.policies.security_level = clamp01(old + 0.03 * gang_pressure);
                self.audit_policy_change("security_level", old, self.policies.security_level);
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::PolicyChange,
                    format!(
                        "Security tightened to {:.2} under gang pressure {:.2}",
                        self.policies.security_level, gang_pressure
                    ),
                ));
            }
            // De-escalation: security posture relaxes after a quiet week —
            // otherwise incident spikes ratchet it to 1.0 permanently.
            if quiet_week && self.policies.security_level > 0.45 {
                let old = self.policies.security_level;
                self.policies.security_level = (self.policies.security_level - 0.1).max(0.45);
                self.audit_policy_change("security_level", old, self.policies.security_level);
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::PolicyChange,
                    format!(
                        "Security posture relaxed to {:.2} after a quiet week",
                        self.policies.security_level
                    ),
                ));
            }
            // Emergency measures expire rather than trapping the institution
            // in a permanent lockdown that blocks all intake.
            if self.policies.lockdown
                && self.days_simulated.saturating_sub(self.lockdown_since_day) >= 14
            {
                self.policies.lockdown = false;
                self.audit_policy_change("lockdown", 1.0, 0.0);
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::PolicyChange,
                    "Lockdown lifted after two weeks",
                ));
            }
            // Staff right-sizing: don't carry a full roster for a shrunken
            // population (payroll was a major insolvency driver).
            let need = ((self.population() + 9) / 10).max(1) as usize;
            if self.guards.len() > need + 1 && self.economy.cash.0 < 5_000_000 {
                self.guards.pop();
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::PolicyChange,
                    "Guard roster reduced to match population",
                ));
            }
            // Last-resort emergency funding: institutions do not simply
            // vanish when cash runs out, but the bailout costs privileges.
            if self.economy.cash.0 < 0 {
                let deficit = -self.economy.cash.0;
                self.economy.accrue_bonus(
                    self.time.as_secs(),
                    alc_economy::Money(deficit),
                    "emergency government funding (insolvency bailout)",
                );
                let priv_old = self.policies.privileges_level;
                self.policies.privileges_level = (self.policies.privileges_level - 0.1).max(0.0);
                self.audit_policy_change(
                    "privileges_level",
                    priv_old,
                    self.policies.privileges_level,
                );
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::BudgetChange,
                    "Emergency funding received; privileges reduced",
                ));
            }
            // Social organizations emerge from relationships, not scripts.
            self.maybe_form_gang_step();
        }
        self.warden
            .set_context(self.capacity(), self.warden.security_incidents_this_week);

        // Warden arbitration: utility-based decision (deterministic core).
        let pop = self.population();
        let action = alc_warden::arbitrate(
            &mut self.warden,
            &self.economy,
            pop,
            &mut RngStream::new(self.seed, self.days_simulated),
        );
        self.warden.record_decision(
            self.time,
            action.clone(),
            action.default_reason(),
            DecisionSource::AiArbitration,
            true,
        );
        self.apply_warden_action(action);

        // Orchestrator reset.
        self.orchestrator.reset_day();

        // Population dynamics: accepted intakes respect capacity (Directive §13).
        self.accept_intake_step();

        // Drop runtime state for prisoners who left the population.
        self.prune_agent_state();

        // Autosave hint via event.
        if self.days_simulated % 7 == 0 {
            self.events.emit(WorldEvent::new(
                0,
                self.time,
                EventKind::BudgetChange,
                "Weekly autosave checkpoint",
            ));
        }
    }

    /// Once per day, high-motivation prisoners roll an abstract escape attempt.
    fn resolve_escape_step(&mut self) {
        // Gang pressure raises the institutional response to escape attempts
        // (more scrutiny after unrest), while the base still depends on real
        // staffing and the warden's security posture.
        let gang_pressure = self.gang_pressure().total();
        let staff_response = clamp01(
            self.staff_coverage() * 0.6 + self.policies.security_level * 0.4 + gang_pressure * 0.08,
        );
        let candidates: Vec<(PrisonerId, f32, f32)> = self
            .prisoners
            .iter()
            .filter_map(|p| {
                let motivation = p.escape_motivation();
                if motivation <= 0.7 {
                    return None;
                }
                // Attempts are rare, deliberate acts: even the desperate only
                // try occasionally rather than every night.
                let mut gate =
                    RngStream::new(self.seed, p.id.raw() ^ (self.days_simulated << 16) ^ 0x35C4);
                if !gate.chance((motivation - 0.7) as f64 * 1.5) {
                    return None;
                }
                let determination = (p.traits.risk_tolerance * 0.5
                    + (1.0 - p.traits.discipline) * 0.3
                    + p.traits.leadership * 0.2)
                    .clamp(0.0, 1.0);
                Some((p.id, motivation, determination))
            })
            .collect();

        let mut escaped = Vec::new();
        for (id, motivation, determination) in candidates {
            let mut rng =
                RngStream::new(self.seed, id.raw() ^ (self.days_simulated << 24) ^ 0x35C4B);
            let outcome = resolve_escape_attempt(
                &mut rng,
                motivation,
                self.policies.security_level,
                determination,
                staff_response,
            );
            let consequences =
                apply_escape_consequences(outcome, self.policies.security_level, &[id]);

            let kind = match outcome {
                EscapeOutcome::SuccessfulEscape => EventKind::EscapeSucceeded,
                EscapeOutcome::PartialBreachContained => EventKind::EscapePrevented,
                _ => EventKind::EscapeAttempt,
            };
            let summary = format!("Prisoner {} escape attempt: {outcome:?}", id.raw());
            let mut ev = WorldEvent::new(0, self.time, kind, summary)
                .with_actor(id.raw())
                .with_cause(format!("escape motivation {motivation:.2}; determination {determination:.2}; security {:.2}", self.policies.security_level));
            self.apply_escape_consequences_step(&consequences, id, &mut ev, &mut rng);
            self.events.emit(ev);

            if outcome == EscapeOutcome::SuccessfulEscape {
                escaped.push(id);
            }
        }

        if !escaped.is_empty() {
            for &id in &escaped {
                self.record_post_release(id, "escape");
            }
            self.purge_gang_members(&escaped);
            self.prisoners.retain(|p| !escaped.contains(&p.id));
        }
    }

    /// Daily parole hearings: approval is a function of conduct, rehabilitation
    /// and residual risk — never a coin flip alone (Directive §15).
    fn resolve_parole_step(&mut self) {
        let eligible: Vec<PrisonerId> = self
            .prisoners
            .iter()
            .filter(|p| p.is_eligible_for_parole())
            .map(|p| p.id)
            .collect();
        let mut released = Vec::new();

        for id in eligible {
            self.events.emit(
                WorldEvent::new(
                    0,
                    self.time,
                    EventKind::ParoleHearing,
                    format!("Parole hearing held for prisoner {}", id.raw()),
                )
                .with_actor(id.raw()),
            );
            let Some(p) = self.prisoners.iter().find(|p| p.id == id) else {
                continue;
            };
            let behavior = p.reputation.clamp(0.0, 1.0) * 0.4
                + (if p.schedule.disciplinary_status {
                    0.0
                } else {
                    0.3
                })
                + (1.0 - p.needs.total_pressure()) * 0.3;
            let rehab_progress = p
                .goals
                .iter()
                .find(|g| g.kind == GoalKind::Rehabilitation)
                .map_or(0.0, |g| g.progress.clamp(0.0, 1.0));
            let rehab = p.rehab_motivation() * 0.5 + rehab_progress * 0.5;
            let risk = p.escape_motivation() * 0.5
                + p.traits.aggression * 0.25
                + (1.0 - p.condition.mental_health.clamp(0.0, 1.0)) * 0.25;
            let chance = (0.25 + behavior * 0.35 + rehab * 0.25 - risk * 0.25).clamp(0.02, 0.95);

            let mut rng =
                RngStream::new(self.seed, id.raw() ^ (self.days_simulated << 16) ^ 0xBA20);
            if rng.chance(chance as f64) {
                released.push(id);
                self.events.emit(
                    WorldEvent::new(
                        0,
                        self.time,
                        EventKind::ParoleGranted,
                        format!("Parole granted to prisoner {}", id.raw()),
                    )
                    .with_actor(id.raw())
                    .with_cause(format!(
                        "behavior {behavior:.2}, rehab {rehab:.2}, risk {risk:.2}"
                    )),
                );
            } else {
                self.events.emit(
                    WorldEvent::new(
                        0,
                        self.time,
                        EventKind::ParoleDenied,
                        format!("Parole denied to prisoner {}", id.raw()),
                    )
                    .with_actor(id.raw())
                    .with_cause(format!(
                        "behavior {behavior:.2}, rehab {rehab:.2}, risk {risk:.2}"
                    )),
                );
            }
        }

        if !released.is_empty() {
            self.release_on_parole(&released);
        }
    }

    /// Release granted prisoners from custody through the single authoritative
    /// world path: post-release records, gang cleanup, population update and a
    /// `Release` event. Returns the ids that were actually released (an id that
    /// is no longer present is silently ignored).
    pub fn release_on_parole(&mut self, ids: &[PrisonerId]) -> Vec<PrisonerId> {
        let present: Vec<PrisonerId> = ids
            .iter()
            .copied()
            .filter(|id| self.prisoners.iter().any(|p| p.id == *id))
            .collect();
        for &id in &present {
            self.record_post_release(id, "parole");
        }
        if !present.is_empty() {
            self.purge_gang_members(&present);
            self.prisoners.retain(|p| !present.contains(&p.id));
            // Daily releases are followed by the normal end-of-day
            // `prune_agent_state` sweep. Out-of-band callers (validated
            // controls) should call `prune_departed_agent_state` explicitly.
        }
        present
    }

    /// Deterministically derive high-level reintegration outcomes from a
    /// departure seed. Sentence progress and rehabilitation improve the odds;
    /// escape departures carry an additional violation risk. Kept pure so the
    /// engine can recompute outcomes without consulting an AI model.
    fn derive_post_release_outcomes(
        seed: u64,
        prisoner_id: u64,
        reason: &str,
        sentence_days: u32,
        served_days: u32,
        rehab_progress: f32,
    ) -> (bool, bool, bool, bool) {
        let mut rng = RngStream::new(seed, prisoner_id ^ 0x9E37_79B9_7F4A_7C15);
        let served_fraction = (served_days as f32 / sentence_days.max(1) as f32).clamp(0.0, 1.0);
        let stability =
            (served_fraction * 0.55 + rehab_progress.clamp(0.0, 1.0) * 0.45).clamp(0.0, 0.95);
        let penalty = if reason == "escape" { 0.3 } else { 0.0 };
        let employment = rng.chance((stability * 0.85 + 0.05).min(0.95) as f64);
        let housing = rng.chance((stability * 0.8 + 0.1).min(0.95) as f64);
        let violation = rng.chance((0.32 - stability * 0.27 + penalty).clamp(0.02, 0.9) as f64);
        let return_to_custody = violation && rng.chance(0.55);
        (employment, housing, violation, return_to_custody)
    }

    /// Record a post-release entry (id, name, reason, day, sentence info,
    /// basic outcome seed) and emit an auditable `Release` event + history
    /// entry. Called when prisoners leave via parole, escape, death or
    /// transfer (0.05 lifecycle).
    fn record_post_release(&mut self, pid: PrisonerId, reason: &str) {
        let Some(p) = self.prisoners.iter().find(|p| p.id == pid) else {
            return;
        };
        let outcome_seed = RngStream::new(
            self.seed,
            pid.raw() ^ (self.days_simulated << 12) ^ 0x0E5C_A9E1,
        )
        .next_u64();
        let rehab_progress = self
            .rehab_records
            .get(&pid)
            .map(|records| RehabProgramRecord::progress_signal(records))
            .unwrap_or(0.0);
        let (employment, housing, violation, return_to_custody) =
            Self::derive_post_release_outcomes(
                outcome_seed,
                pid.raw(),
                reason,
                p.sentence_days,
                p.sentence_served_days,
                rehab_progress,
            );
        let rec = PostReleaseRecord {
            id: self.next_post_release_id,
            prisoner_id: pid.raw(),
            name: p.name.clone(),
            reason: reason.to_string(),
            day: self.days_simulated,
            sentence_info: PostReleaseSentenceInfo {
                sentence_days: p.sentence_days,
                sentence_served_days: p.sentence_served_days,
                classification: p.classification,
                escape_motivation_at_exit: p.escape_motivation(),
            },
            outcome_seed,
            employment,
            housing,
            violation,
            return_to_custody,
        };
        self.post_release_outcomes.push(PostReleaseOutcomeRecord {
            id: self.next_post_release_id,
            post_release_id: rec.id,
            prisoner_id: pid.raw(),
            name: p.name.clone(),
            day: self.days_simulated,
            outcome_seed,
            employment,
            housing,
            violation,
            return_to_custody,
        });
        self.next_post_release_id += 1;
        self.post_release_records.push(rec.clone());
        let ev = WorldEvent::new(
            0,
            self.time,
            EventKind::Release,
            format!("Prisoner {} left the institution: {reason}", pid.raw()),
        )
        .with_actor(pid.raw())
        .with_cause("lifecycle: post-release record");
        self.emit_history_event(ev);
    }

    /// Deterministic medical-neglect death pathway (0.05 lifecycle).
    /// Condition: severe health/medical neglect drives health down; once a
    /// prisoner is very low and still neglecting, they die and leave the
    /// population with a `Death` event and a post-release record.
    fn resolve_medical_consequences_step(&mut self) {
        for p in &mut self.prisoners {
            if p.needs.medical_neglect() >= 0.75 {
                p.condition.health = (p.condition.health - 0.008).max(0.0);
            }
        }
        let dead: Vec<PrisonerId> = self
            .prisoners
            .iter()
            .filter(|p| {
                p.condition.health.clamp(0.0, 1.0) < 0.02 && p.needs.medical_neglect() > 0.7
            })
            .map(|p| p.id)
            .collect();
        for pid in dead.clone() {
            let ev = WorldEvent::new(
                0,
                self.time,
                EventKind::Death,
                format!(
                    "Prisoner {} died (severe health/medical neglect)",
                    pid.raw()
                ),
            )
            .with_actor(pid.raw())
            .with_cause("condition.health < 0.02 while medical_need high; unmonitored death");
            self.emit_history_event(ev);
            self.record_post_release(pid, "death");
        }
        if !dead.is_empty() {
            self.purge_gang_members(&dead);
            self.prisoners.retain(|p| !dead.contains(&p.id));
            self.prune_agent_state();
        }
    }

    /// Deterministic transfer-out pathway: extreme crowding (security
    /// posture feeds this indirectly through the escape gate) transfers the
    /// lowest-id prisoner away with a `TransferOut` event + post-release
    /// record. Rate-limited to one per couple of days.
    fn resolve_transfers_step(&mut self) {
        let cap = self.capacity();
        let pop = self.population();
        let over_crowded = pop as f32 > cap as f32 * 1.2;
        if !over_crowded {
            return;
        }
        // Rate-limit the pathway so it stays observable without churning
        // the population (Extreme speeds advance days fast).
        if self.days_simulated.saturating_sub(self.last_transfer_day) < 2 {
            return;
        }
        let Some(id) = self.prisoners.iter().min_by_key(|p| p.id.0).map(|p| p.id) else {
            return;
        };
        let ev = WorldEvent::new(
            0,
            self.time,
            EventKind::TransferOut,
            format!(
                "Prisoner {} transferred out (overcrowded={} security={:.2})",
                id.raw(),
                over_crowded,
                self.policies.security_level
            ),
        )
        .with_actor(id.raw());
        self.emit_history_event(ev);
        self.record_post_release(id, "transfer");
        self.purge_gang_members(&[id]);
        self.last_transfer_day = self.days_simulated;
        self.prisoners.retain(|p| p.id != id);
        self.prune_agent_state();
    }

    /// Accept new arrivals only when the facility has spare capacity and
    /// intake is permitted by policy (no lockdown, moderate security).
    fn accept_intake_step(&mut self) {
        if self.days_simulated % 3 != 0 {
            return;
        }
        // Lockdown / max-security posture halts admissions (Directive §13).
        let lockdown_active = self.policies.lockdown
            && self
                .days_simulated
                .saturating_sub(self.lockdown_since_day)
                < 14;
        if lockdown_active
            || self.policies.security_level >= 1.0
            || self.policies.intake_paused
        {
            return;
        }
        let cap = self.capacity();
        let free = cap.saturating_sub(self.population());
        if free == 0 {
            return;
        }
        // One admission per sweep keeps growth paced; never exceed capacity.
        let accepted = 1.min(free);
        // A day-boundary admission may be the last state change of a run, so
        // place the pawn atomically rather than waiting for the next tick's
        // schedule assignment.
        let intake_facility = self
            .facility_of_kind(FacilityKind::CellBlock)
            .or_else(|| self.facilities.first().map(|f| f.id))
            .unwrap_or(FacilityId(0));
        for _ in 0..accepted {
            let id = self.next_prisoner_id;
            self.next_prisoner_id += 1;
            let pid = PrisonerId(id);
            let p = generate_intake(self.seed, pid);
            self.prisoner_location.insert(pid, intake_facility);
            self.events.emit(
                WorldEvent::new(
                    0,
                    self.time,
                    EventKind::Admission,
                    format!("New arrival: {}", p.name),
                )
                .with_actor(id)
                .with_cause(format!(
                    "daily intake sweep; {free} bed(s) available of {cap}"
                )),
            );
            self.prisoners.push(p);
            self.register_ai_agent(pid, Role::Prisoner);
        }
    }

    /// Drop auxiliary state belonging only to departed prisoners. Kept O(n)
    /// in the live population so frequent parole releases stay cheap.
    pub fn prune_departed_agent_state(&mut self, departed: &[PrisonerId]) {
        let gone: std::collections::HashSet<PrisonerId> = departed.iter().copied().collect();
        self.current_activity.retain(|id, _| !gone.contains(id));
        self.current_decisions.retain(|id, _| !gone.contains(id));
        self.prisoner_location.retain(|id, _| !gone.contains(id));
        self.contraband_state.retain(|id, _| !gone.contains(id));
        self.rehab_records.retain(|id, _| !gone.contains(id));
        let departed: Vec<u64> = departed.iter().map(|id| id.raw()).collect();
        self.orchestrator.prune_departed(&departed);
        // Social dyads are pruned by the daily `prune_agent_state` sweep; doing
        // an O(edges) retain here would make frequent parole releases costly.
    }

    fn prune_agent_state(&mut self) {
        let live: Vec<PrisonerId> = self.prisoners.iter().map(|p| p.id).collect();
        self.current_activity.retain(|id, _| live.contains(id));
        self.current_decisions.retain(|id, _| live.contains(id));
        self.prisoner_location.retain(|id, _| live.contains(id));
        self.contraband_state.retain(|id, _| live.contains(id));
        self.rehab_records.retain(|id, _| live.contains(id));
        let departed: Vec<u64> = self
            .orchestrator
            .scheduler
            .agents
            .keys()
            .copied()
            .filter(|id| !live.iter().any(|p| p.raw() == *id))
            .collect();
        self.orchestrator.prune_departed(&departed);
        // Also prune social dyads so stale relationships never survive a
        // release/escape (R6 invariant: claims must reference existing pawns).
        let live_set: std::collections::HashSet<PrisonerId> = live.into_iter().collect();
        self.social.retain(&live_set);
    }

    /// Seed permanent completion markers from persisted rehab records.
    ///
    /// Legacy saves may contain a completed `RehabProgramRecord` without the
    /// `completed:<program key>` skill marker now used to block re-enrollment.
    /// Deriving the marker from durable records keeps those saves from gaining
    /// repeated hope/purpose bumps and zombie active records. Mixed
    /// active/completed duplicates are closed here as well: otherwise the
    /// completion marker would strip the active skill key while the stale
    /// record kept consuming capacity and blocked every later enrollment.
    fn reconcile_rehab_completion_markers(&mut self) {
        let markers: Vec<(PrisonerId, String, String)> = self
            .rehab_records
            .iter()
            .flat_map(|(pid, records)| {
                records.iter().filter(|r| r.completed).map(move |r| {
                    let program_key = r.program.key().to_string();
                    (*pid, format!("completed:{program_key}"), program_key)
                })
            })
            .collect();
        for (pid, marker, program_key) in markers {
            if let Some(records) = self.rehab_records.get_mut(&pid) {
                for record in records
                    .iter_mut()
                    .filter(|r| r.program.key() == program_key && r.is_active())
                {
                    record.dropped = true;
                    record.failure_reason = Some(RehabFailureReason::ProgramCompleted);
                }
            }
            if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == pid) {
                p.skills.insert(marker, 1.0);
                p.skills.remove(&program_key);
            }
        }
    }

    fn rehab_step(&mut self) {
        use alc_social::rehab::{
            RehabOutcome, RehabPrisonerState, RehabProgram, apply_program_progress,
            enroll_in_program, has_completed_program,
        };
        self.reconcile_rehab_completion_markers();
        let programs = [
            RehabProgram::Education,
            RehabProgram::Vocational,
            RehabProgram::Counseling,
            RehabProgram::AddictionRecovery,
            RehabProgram::AngerManagement,
        ];
        // Policy tie-in (Directive §14): funding multiplier scales daily progress.
        let multiplier = self.policies.rehab_funding_multiplier.clamp(0.0, 10.0);
        let day = self.days_simulated;
        let dt = 60u64;
        let dt_eff = (dt as f32 * multiplier).round() as u64;

        // Simple deterministic capacity: based on education/workshop/medical facilities, min 8.
        let cap_facilities: usize = self
            .facilities
            .iter()
            .filter(|f| {
                matches!(
                    f.kind,
                    FacilityKind::Education | FacilityKind::Workshop | FacilityKind::Medical
                )
            })
            .map(|f| (f.capacity.max(1) / 4) as usize)
            .sum();
        let rehab_capacity = cap_facilities.max(8);

        let mut events = Vec::new();

        for i in 0..self.prisoners.len() {
            let pid = self.prisoners[i].id;
            let name = self.prisoners[i].name.clone();
            let mut state = RehabPrisonerState {
                disciplinary_status: self.prisoners[i].schedule.disciplinary_status,
                education: self.prisoners[i].schedule.education,
                skills: self.prisoners[i].skills.clone(),
                hope: self.prisoners[i].condition.hope,
                mental_health: self.prisoners[i].condition.mental_health,
                health: self.prisoners[i].condition.health,
                aggression: self.prisoners[i].traits.aggression,
                emotional_stability: self.prisoners[i].traits.emotional_stability,
                impulsiveness: self.prisoners[i].traits.impulsiveness,
                discipline: self.prisoners[i].traits.discipline,
                intelligence: self.prisoners[i].traits.intelligence,
                reputation: self.prisoners[i].reputation,
                fatigue: self.prisoners[i].needs.fatigue,
                purpose_need: self.prisoners[i].needs.purpose,
                stability_need: self.prisoners[i].needs.stability,
                respect_need: self.prisoners[i].needs.respect,
                rehab_motivation: self.prisoners[i].rehab_motivation(),
                desperation: self.prisoners[i].needs.desperation(),
            };

            // Sync pre-existing skill enrollments into explicit records.
            for program in programs {
                if state.skills.contains_key(program.key()) {
                    let has_record = self
                        .rehab_records
                        .get(&pid)
                        .is_some_and(|recs| recs.iter().any(|r| r.program == program));
                    if !has_record {
                        self.rehab_records
                            .entry(pid)
                            .or_default()
                            .push(RehabProgramRecord::new(program, day));
                    }
                }
            }

            // Disciplinary removal: mark active records as failed.
            if state.disciplinary_status {
                if let Some(recs) = self.rehab_records.get_mut(&pid) {
                    for rec in recs.iter_mut().filter(|r| r.is_active()) {
                        rec.dropped = true;
                        rec.failure_reason = Some(RehabFailureReason::DisciplinaryRemoval);
                        state.skills.remove(rec.program.key());
                        self.events.emit_rehab_failure(
                            self.time,
                            pid.raw(),
                            rec.program.key(),
                            RehabFailureReason::DisciplinaryRemoval,
                        );
                    }
                }
            }

            let enrolled_any = programs.iter().any(|pr| {
                state.skills.contains_key(pr.key())
                    || self
                        .rehab_records
                        .get(&pid)
                        .is_some_and(|recs| recs.iter().any(|r| r.program == *pr && r.is_active()))
            });
            let current_active = self
                .rehab_records
                .values()
                .map(|recs| recs.iter().filter(|r| r.is_active()).count())
                .sum::<usize>();
            let has_capacity = current_active < rehab_capacity;

            if !enrolled_any
                && !state.disciplinary_status
                && state.rehab_motivation > 0.45
                && has_capacity
            {
                let program = if state.aggression > 0.6 {
                    RehabProgram::AngerManagement
                } else if state.mental_health < 0.5 {
                    RehabProgram::Counseling
                } else if state.purpose_need > 0.6 {
                    RehabProgram::Education
                } else {
                    RehabProgram::Vocational
                };
                if enroll_in_program(&mut state, program) {
                    self.rehab_records
                        .entry(pid)
                        .or_default()
                        .push(RehabProgramRecord::new(program, day));
                    events.push(
                        WorldEvent::new(
                            0,
                            self.time,
                            EventKind::ProgramEnrolled,
                            format!("{name} enrolled in {program:?}"),
                        )
                        .with_actor(pid.raw())
                        .with_cause(format!(
                            "eligibility met; capacity {current_active}/{rehab_capacity}"
                        )),
                    );
                }
            } else if !enrolled_any
                && !state.disciplinary_status
                && state.rehab_motivation > 0.45
                && !has_capacity
            {
                events.push(WorldEvent::new(0, self.time, EventKind::RehabilitationFailure,
                    format!("{name} rehab enrollment deferred: no capacity ({current_active}/{rehab_capacity})"))
                    .with_actor(pid.raw())
                    .with_cause("eligibility met but rehab capacity exhausted"));
            }

            for program in programs {
                if !state.skills.contains_key(program.key()) {
                    continue;
                }
                let has_record = self
                    .rehab_records
                    .get(&pid)
                    .is_some_and(|recs| recs.iter().any(|r| r.program == program));
                if !has_record {
                    self.rehab_records
                        .entry(pid)
                        .or_default()
                        .push(RehabProgramRecord::new(program, day));
                }

                // Legacy saves can carry duplicate records for one program.
                // Progress belongs to the newest *active* record, never to an
                // older completed/dropped entry that happens to come first.
                let has_active_record = self
                    .rehab_records
                    .get(&pid)
                    .is_some_and(|recs| recs.iter().any(|r| r.program == program && r.is_active()));
                if !has_active_record {
                    state.skills.remove(program.key());
                    continue;
                }

                match apply_program_progress(&mut state, program, dt_eff) {
                    RehabOutcome::Progress(delta) if delta > 0.0 => {
                        if let Some(rec) = self.rehab_records.get_mut(&pid).and_then(|recs| {
                            recs.iter_mut()
                                .rev()
                                .find(|r| r.program == program && r.is_active())
                        }) {
                            rec.progress_days += 1;
                            rec.attendance_days += 1;
                            rec.last_attended_day = Some(day);
                            rec.progress_sum += delta;
                        }
                        let progress = state.skills.get(program.key()).copied().unwrap_or(0.0);
                        events.push(
                            WorldEvent::new(
                                0,
                                self.time,
                                EventKind::RehabilitationProgress,
                                format!(
                                    "Rehab progress: {} +{delta:.2} (day {progress:.2})",
                                    program.key()
                                ),
                            )
                            .with_actor(pid.raw())
                            .with_cause(format!("rehab funding multiplier {multiplier:.2}")),
                        );
                    }
                    RehabOutcome::Progress(_) => {
                        if let Some(rec) = self.rehab_records.get_mut(&pid).and_then(|recs| {
                            recs.iter_mut()
                                .rev()
                                .find(|r| r.program == program && r.is_active())
                        }) {
                            let idle_days = day
                                .saturating_sub(rec.last_attended_day.unwrap_or(rec.enrolled_day));
                            if idle_days > 21 && rec.attendance_days < 2 {
                                rec.dropped = true;
                                rec.failure_reason =
                                    Some(RehabFailureReason::InsufficientEngagement);
                                state.skills.remove(program.key());
                                self.events.emit_rehab_failure(
                                    self.time,
                                    pid.raw(),
                                    program.key(),
                                    RehabFailureReason::InsufficientEngagement,
                                );
                            }
                        }
                    }
                    RehabOutcome::Completed => {
                        if let Some(rec) = self.rehab_records.get_mut(&pid).and_then(|recs| {
                            recs.iter_mut()
                                .rev()
                                .find(|r| r.program == program && r.is_active())
                        }) {
                            rec.completed = true;
                            rec.failure_reason = Some(RehabFailureReason::ProgramCompleted);
                            rec.last_attended_day = Some(day);
                        }
                        events.push(
                            WorldEvent::new(
                                0,
                                self.time,
                                EventKind::RehabilitationSuccess,
                                format!("{name} completed {program:?}"),
                            )
                            .with_actor(pid.raw()),
                        );
                        self.events.emit_rehab_failure(
                            self.time,
                            pid.raw(),
                            program.key(),
                            RehabFailureReason::ProgramCompleted,
                        );
                    }
                    RehabOutcome::Dropped => {
                        let reason = if state.disciplinary_status {
                            RehabFailureReason::DisciplinaryRemoval
                        } else {
                            RehabFailureReason::DroppedOut
                        };
                        if let Some(rec) = self.rehab_records.get_mut(&pid).and_then(|recs| {
                            recs.iter_mut()
                                .rev()
                                .find(|r| r.program == program && r.is_active())
                        }) {
                            rec.dropped = true;
                            rec.failure_reason = Some(reason);
                        }
                        self.events
                            .emit_rehab_failure(self.time, pid.raw(), program.key(), reason);
                    }
                    RehabOutcome::NoEnrollment => {
                        // A completion marker or a legacy skill-only record can
                        // leave an active record with no live enrollment. Close
                        // it so it cannot consume capacity forever.
                        let reason = if has_completed_program(&state, program) {
                            RehabFailureReason::ProgramCompleted
                        } else {
                            RehabFailureReason::DroppedOut
                        };
                        if let Some(recs) = self.rehab_records.get_mut(&pid) {
                            for rec in recs
                                .iter_mut()
                                .filter(|r| r.program == program && r.is_active())
                            {
                                rec.dropped = true;
                                rec.failure_reason = Some(reason);
                            }
                        }
                    }
                }
            }

            // Write back.
            let p = &mut self.prisoners[i];
            p.skills = state.skills;
            p.condition.hope = state.hope.clamp(0.0, 1.0);
            p.condition.mental_health = state.mental_health.clamp(0.0, 1.0);
            p.condition.health = state.health.clamp(0.0, 1.0);
            p.traits.aggression = state.aggression.clamp(0.0, 1.0);
            p.traits.emotional_stability = state.emotional_stability.clamp(0.0, 1.0);
            p.traits.impulsiveness = state.impulsiveness.clamp(0.0, 1.0);
            p.reputation = state.reputation.clamp(0.0, 1.0);
            p.needs.purpose = state.purpose_need.clamp(0.0, 1.0);
            p.needs.stability = state.stability_need.clamp(0.0, 1.0);
            p.needs.respect = state.respect_need.clamp(0.0, 1.0);
            if state.education {
                p.schedule.education = true;
            }
        }

        for ev in events {
            self.events.emit(ev);
        }
    }

    fn contraband_step(&mut self) {
        use alc_social::contraband::{
            ContrabandPrisonerProfile, contraband_discovery, contraband_market,
        };
        if self.prisoners.is_empty() || self.guards.is_empty() {
            return;
        }
        let profiles: Vec<ContrabandPrisonerProfile> = self
            .prisoners
            .iter()
            .map(|p| ContrabandPrisonerProfile {
                id: p.id,
                discipline: p.traits.discipline,
                risk_tolerance: p.traits.risk_tolerance,
                manipulation: p.traits.manipulation,
                aggression: p.traits.aggression,
                stimulation: p.needs.stimulation,
                freedom: p.needs.freedom,
                safety: p.needs.safety,
                desperation: p.needs.desperation(),
                // Gang-channel throughput is derived from live political state:
                // pressure, resources, standing and personal discipline all
                // change how effectively a member can move contraband.
                gang_control: self.gang_control_for(p.id),
            })
            .collect();

        let mut rng = RngStream::new(self.seed, self.days_simulated ^ 0xC047_2ABA);
        let offers = contraband_market(&profiles, &mut rng);
        for offer in offers.into_iter().take(3) {
            // Track the trade in per-prisoner inventories (0.05 depth):
            // the seller loses one, the buyer gains one.
            self.move_contraband(offer.seller, &offer.item, -1);
            self.move_contraband(offer.buyer, &offer.item, 1);
            let flags = (
                self.contraband_state
                    .get(&offer.seller)
                    .map_or(false, |s| s.flags_confiscated),
                self.contraband_state
                    .get(&offer.buyer)
                    .map_or(false, |s| s.flags_confiscated),
            );
            self.events.emit(WorldEvent::new(0, self.time, EventKind::ContrabandFound,
                format!("Contraband trade suspected: {:?} from {} to {} (price {}, seller_flagged={}, buyer_flagged={})",
                    offer.item, offer.seller.raw(), offer.buyer.raw(), offer.price, flags.0, flags.1))
                .with_actor(offer.seller.raw())
                .with_cause("contraband market activity"));
        }

        let mut rng2 = RngStream::new(self.seed, self.days_simulated ^ 0xD15C_0BE2);
        if let Some(found) = contraband_discovery(&self.guards, &profiles, &mut rng2) {
            self.record_contraband_discovery(found.clone());
            let sec_old = self.policies.security_level;
            self.policies.security_level =
                (self.policies.security_level + 0.02 * found.severity).min(1.0);
            self.audit_policy_change("security_level", sec_old, self.policies.security_level);
        }
    }

    /// Record a guard discovery of contraband on a prisoner: applies
    /// discipline/reputation/safety effects, records the item in world-owned
    /// inventory/state, sets confiscation flags, and emits consistent
    /// ContrabandFound + ContrabandConfiscated events (0.05 lifecycle).
    fn record_contraband_discovery(&mut self, found: alc_social::ContrabandFound) {
        let holder = found.holder;
        if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == holder) {
            p.schedule.disciplinary_status = true;
            p.record_discipline(
                u32::try_from(self.days_simulated).unwrap_or(u32::MAX),
                "contraband",
                "loss-of-privileges",
                found.severity,
            );
            p.reputation = (p.reputation - 0.05).max(0.0);
            p.traits.discipline = (p.traits.discipline - 0.04).max(0.05);
            p.needs.safety = (p.needs.safety + 0.1).min(1.0);
        }
        let st = self.contraband_state_entry(holder);
        *st.inventory.entry(found.item).or_insert(0) += 1;
        st.flags_confiscated = true;
        st.discovered_count += 1;
        st.flagged_days += 1;
        let inv_count = st.inventory.get(&found.item).copied().unwrap_or(0);

        // World-authoritative case record: each discovery produces a durable
        // audit entry with disposition and a severity-scaled sanction.
        use alc_social::contraband::{
            ConfiscationDisposition, ContrabandCaseRecord, DisciplinaryAction,
        };
        let disposition = if found.severity >= 0.8 {
            ConfiscationDisposition::Evidence
        } else if found.severity >= 0.4 {
            ConfiscationDisposition::Seized
        } else {
            ConfiscationDisposition::Destroyed
        };
        let disciplinary_action = if found.severity >= 0.8 {
            Some(DisciplinaryAction::Isolation {
                duration_days: (3.0 + found.severity * 7.0) as u32,
            })
        } else if found.severity >= 0.5 {
            Some(DisciplinaryAction::LossOfPrivileges {
                level: found.severity,
                duration_days: 7,
            })
        } else {
            Some(DisciplinaryAction::Warning)
        };
        let case_id = self.next_contraband_case_id;
        self.next_contraband_case_id = self.next_contraband_case_id.saturating_add(1);
        let summary = format!(
            "Case {case_id}: {:?} found on prisoner {} by guard {} (severity {:.2}, {} in inventory)",
            found.item,
            holder.raw(),
            found.guard.raw(),
            found.severity,
            inv_count
        );
        self.contraband_cases.push(ContrabandCaseRecord {
            case_id,
            discovered_at: self.time,
            discovering_guard: found.guard,
            holder,
            item: found.item,
            severity: found.severity,
            disposition,
            disciplinary_action,
            summary,
        });
        // Keep the durable case log bounded for long-running simulations.
        if self.contraband_cases.len() > 512 {
            let drain = self.contraband_cases.len() - 512;
            self.contraband_cases.drain(..drain);
        }

        let found_ev = WorldEvent::new(0, self.time, EventKind::ContrabandFound,
            format!("Guard {} found {:?} on prisoner {} [flags: true; {} {:?} in inventory; severity {:.2}]",
                found.guard.raw(), found.item, holder.raw(), inv_count, found.item, found.severity))
            .with_actor(holder.raw())
            .with_cause(format!("discovery by guard {}", found.guard.raw()));
        self.emit_history_event(found_ev);

        let conf_ev = WorldEvent::new(
            0,
            self.time,
            EventKind::ContrabandConfiscated,
            format!(
                "Contraband confiscated: {:?} from prisoner {} by guard {}",
                found.item,
                holder.raw(),
                found.guard.raw()
            ),
        )
        .with_actor(holder.raw())
        .with_cause("confiscation following discovery");
        self.emit_history_event(conf_ev);
    }

    fn contraband_state_entry(&mut self, id: PrisonerId) -> &mut PrisonerContrabandState {
        self.contraband_state.entry(id).or_default()
    }

    fn contraband_inventory_count(&self, id: PrisonerId, item: &ContrabandItem) -> u32 {
        self.contraband_state
            .get(&id)
            .and_then(|s| s.inventory.get(item))
            .copied()
            .unwrap_or(0)
    }

    fn move_contraband(&mut self, id: PrisonerId, item: &ContrabandItem, delta: i32) {
        let count = self.contraband_inventory_count(id, item) as i64;
        let new_count = if delta > 0 {
            count + delta as i64
        } else {
            count - (-delta as i64)
        };
        let new_count = new_count.max(0) as u32;
        let st = self.contraband_state_entry(id);
        if new_count == 0 {
            st.inventory.remove(item);
        } else {
            st.inventory.insert(*item, new_count);
        }
    }

    fn construction_step(&mut self) {
        if self.construction_queue.is_empty() {
            return;
        }

        // Evaluate prerequisites/staff/materials deterministically by project id order.
        let staff_available = self.staff_coverage() >= 0.2 || self.guards.len() >= 2;
        let materials_available = self.economy.cash.0 > -50_000_00; // not catastrophically insolvent
        let has_admin = self
            .facilities
            .iter()
            .any(|f| matches!(f.kind, FacilityKind::Admin));

        let mut approved: Vec<(u64, FacilityKind)> = Vec::new();
        let mut completed: Vec<ConstructionProject> = Vec::new();
        let mut cancelled: Vec<(u64, String, FacilityKind)> = Vec::new();
        let mut failed: Vec<(u64, String, FacilityKind)> = Vec::new();

        for p in &mut self.construction_queue {
            if matches!(
                p.status,
                ConstructionStatus::Completed
                    | ConstructionStatus::Cancelled
                    | ConstructionStatus::Failed
            ) {
                continue;
            }
            // Prerequisites: admin required for non-admin builds; wing must be plausible.
            p.prereqs_met = has_admin || matches!(p.kind, FacilityKind::Admin);
            p.staff_allocated = staff_available;
            p.materials_ready = materials_available;

            if !p.prereqs_met {
                p.status = ConstructionStatus::Failed;
                p.terminal_reason = Some("missing prerequisite: admin facility".to_string());
                failed.push((p.id, p.terminal_reason.clone().unwrap_or_default(), p.kind));
                continue;
            }
            if !p.materials_ready
                && p.progress_days == 0
                && self.days_simulated.saturating_sub(p.started_day) > 7
            {
                p.status = ConstructionStatus::Cancelled;
                p.terminal_reason = Some("materials/funding unavailable".to_string());
                cancelled.push((p.id, p.terminal_reason.clone().unwrap_or_default(), p.kind));
                continue;
            }
            if !p.staff_allocated
                && self.days_simulated.saturating_sub(p.started_day) > 14
                && p.progress_days < p.total_days / 2
            {
                p.status = ConstructionStatus::Failed;
                p.terminal_reason = Some("insufficient staff allocation".to_string());
                failed.push((p.id, p.terminal_reason.clone().unwrap_or_default(), p.kind));
                continue;
            }

            // request -> approved: a queued request is reviewed before work
            // starts, so the approval is an auditable state of its own.
            if p.status == ConstructionStatus::Queued {
                p.status = ConstructionStatus::Approved;
                approved.push((p.id, p.kind));
            }
            // approved -> active (same day-boundary once prerequisites hold)
            p.status = ConstructionStatus::InProgress;
            // Progress only when staff/materials are available; deterministic stall otherwise.
            if p.staff_allocated && p.materials_ready {
                p.progress_days = p.progress_days.saturating_add(1);
            }
            if p.progress_days >= p.total_days {
                p.status = ConstructionStatus::Completed;
                completed.push(p.clone());
            }
        }

        for (id, kind) in approved {
            let ev = WorldEvent::new(
                0,
                self.time,
                EventKind::Expansion,
                format!("Construction approved: {} (project {})", kind.display(), id),
            )
            .with_cause(format!(
                "prerequisites, staffing and materials validated for project {id}"
            ));
            self.emit_history_event(ev);
        }
        for (id, reason, kind) in cancelled {
            let ev = WorldEvent::new(
                0,
                self.time,
                EventKind::ConstructionCancelled,
                format!(
                    "Construction cancelled: {} (project {}) - {}",
                    kind.display(),
                    id,
                    reason
                ),
            )
            .with_cause(reason.clone());
            self.emit_history_event(ev);
        }
        for (id, reason, kind) in failed {
            let ev = WorldEvent::new(
                0,
                self.time,
                EventKind::ConstructionFailed,
                format!(
                    "Construction failed: {} (project {}) - {}",
                    kind.display(),
                    id,
                    reason
                ),
            )
            .with_cause(reason.clone());
            self.emit_history_event(ev);
        }
        for proj in completed {
            let id = self.add_facility(proj.kind, proj.wing, proj.capacity);
            let ev = WorldEvent::new(
                0,
                self.time,
                EventKind::ConstructionComplete,
                format!(
                    "Construction complete: {} (facility {})",
                    proj.kind.display(),
                    id.0
                ),
            )
            .with_cause(format!(
                "project {} completed after {} days",
                proj.id, proj.total_days
            ));
            self.emit_history_event(ev);
        }

        // Keep terminal projects for auditability; prune only very old terminal entries to avoid unbounded growth.
        let cutoff = self.days_simulated.saturating_sub(365);
        self.construction_queue.retain(|p| {
            matches!(
                p.status,
                ConstructionStatus::Queued
                    | ConstructionStatus::Approved
                    | ConstructionStatus::InProgress
            ) || p.started_day >= cutoff
        });
    }

    /// Public entry point for expansion queueing (0.05 lifecycle: makes the
    /// construction queue a visible, inspectable part of world state).
    pub fn enqueue_construction(&mut self, kind: FacilityKind, wing: u32, capacity: u32) -> u64 {
        let id = self.next_construction_id;
        self.next_construction_id += 1;
        let total_days = match kind {
            FacilityKind::CellBlock | FacilityKind::Dormitory => 14,
            FacilityKind::Medical | FacilityKind::Education => 10,
            FacilityKind::Isolation | FacilityKind::Visitor | FacilityKind::Staff => 8,
            _ => 7,
        };
        let cost = (capacity as f32 * 50.0) + (total_days as f32 * 200.0);
        // Charge upfront if possible; still allow construction (debt pressure is part of sim).
        let _ = self.economy.spend(
            self.time.as_secs(),
            alc_economy::Money::from_major(cost as i64),
            alc_economy::Account::Construction,
            format!("Construction: {}", kind.display()),
        );
        let proj = ConstructionProject {
            id,
            kind,
            wing,
            capacity,
            total_days,
            progress_days: 0,
            cost,
            started_day: self.days_simulated,
            status: ConstructionStatus::Queued,
            terminal_reason: None,
            staff_allocated: self.staff_coverage() >= 0.2 || self.guards.len() >= 2,
            materials_ready: self.economy.cash.0 > -50_000_00,
            prereqs_met: self
                .facilities
                .iter()
                .any(|f| matches!(f.kind, FacilityKind::Admin))
                || matches!(kind, FacilityKind::Admin),
        };
        self.events.emit(
            WorldEvent::new(
                0,
                self.time,
                EventKind::Expansion,
                format!(
                    "Construction started: {} ({} days, ${:.0})",
                    kind.display(),
                    total_days,
                    cost
                ),
            )
            .with_cause(format!("project {id}")),
        );
        self.construction_queue.push(proj);
        id
    }

    /// Approve a requested construction project explicitly. Approval validates
    /// the recorded prerequisites and emits an auditable `Expansion` event;
    /// work only begins once a project is `Approved`.
    pub fn approve_construction(&mut self, project_id: u64, reason: impl Into<String>) -> bool {
        let reason = reason.into();
        let mut approved = None;
        for p in &mut self.construction_queue {
            if p.id == project_id && p.status == ConstructionStatus::Queued {
                p.status = ConstructionStatus::Approved;
                approved = Some(p.kind);
                break;
            }
        }
        if let Some(kind) = approved {
            let ev = WorldEvent::new(
                0,
                self.time,
                EventKind::Expansion,
                format!(
                    "Construction approved: {} (project {}) - {}",
                    kind.display(),
                    project_id,
                    reason
                ),
            )
            .with_cause(reason);
            self.emit_history_event(ev);
            return true;
        }
        false
    }

    /// Cancel an active construction project. Emits ConstructionCancelled and keeps
    /// the terminal record in queue history for auditability.
    pub fn cancel_construction(&mut self, project_id: u64, reason: impl Into<String>) -> bool {
        let reason = reason.into();
        let mut found = None;
        for p in &mut self.construction_queue {
            if p.id == project_id
                && matches!(
                    p.status,
                    ConstructionStatus::Queued
                        | ConstructionStatus::Approved
                        | ConstructionStatus::InProgress
                )
            {
                p.status = ConstructionStatus::Cancelled;
                p.terminal_reason = Some(reason.clone());
                found = Some(p.kind);
                break;
            }
        }
        if let Some(kind) = found {
            let ev = WorldEvent::new(
                0,
                self.time,
                EventKind::ConstructionCancelled,
                format!(
                    "Construction cancelled: {} (project {}) - {}",
                    kind.display(),
                    project_id,
                    reason
                ),
            )
            .with_cause(reason);
            self.emit_history_event(ev);
            return true;
        }
        false
    }

    /// Mark a construction project as failed (world-side lifecycle API for engine delegation).
    pub fn fail_construction(&mut self, project_id: u64, reason: impl Into<String>) -> bool {
        let reason = reason.into();
        let mut found = None;
        for p in &mut self.construction_queue {
            if p.id == project_id
                && matches!(
                    p.status,
                    ConstructionStatus::Queued
                        | ConstructionStatus::Approved
                        | ConstructionStatus::InProgress
                )
            {
                p.status = ConstructionStatus::Failed;
                p.terminal_reason = Some(reason.clone());
                found = Some(p.kind);
                break;
            }
        }
        if let Some(kind) = found {
            let ev = WorldEvent::new(
                0,
                self.time,
                EventKind::ConstructionFailed,
                format!(
                    "Construction failed: {} (project {}) - {}",
                    kind.display(),
                    project_id,
                    reason
                ),
            )
            .with_cause(reason);
            self.emit_history_event(ev);
            return true;
        }
        false
    }

    /// Advance a construction project by `days` (world-side lifecycle API for engine delegation).
    /// Returns true if the project advanced or completed.
    pub fn advance_construction(&mut self, project_id: u64, days: u32) -> bool {
        let days = days.max(1);
        let mut completed_proj: Option<ConstructionProject> = None;
        let mut approved: Option<(u64, FacilityKind)> = None;
        for p in &mut self.construction_queue {
            // Advancing a raw request implies approval: the request transitions
            // through `Approved` into `InProgress` in one validated step.
            if p.id == project_id && p.status == ConstructionStatus::Queued {
                p.status = ConstructionStatus::Approved;
                approved = Some((p.id, p.kind));
            }
            if p.id == project_id
                && matches!(
                    p.status,
                    ConstructionStatus::Approved | ConstructionStatus::InProgress
                )
            {
                p.status = ConstructionStatus::InProgress;
                // Clamp to the remaining work so observer-driven advances can
                // never push progress past the project's declared duration.
                p.progress_days = (p.progress_days.saturating_add(days)).min(p.total_days);
                if p.progress_days >= p.total_days {
                    p.status = ConstructionStatus::Completed;
                    completed_proj = Some(p.clone());
                }
                break;
            }
        }
        if let Some((id, kind)) = approved {
            let ev = WorldEvent::new(
                0,
                self.time,
                EventKind::Expansion,
                format!("Construction approved: {} (project {})", kind.display(), id),
            )
            .with_cause("observer advance approved the queued request".to_string());
            self.emit_history_event(ev);
        }
        if let Some(proj) = completed_proj {
            let id = self.add_facility(proj.kind, proj.wing, proj.capacity);
            let ev = WorldEvent::new(
                0,
                self.time,
                EventKind::ConstructionComplete,
                format!(
                    "Construction complete: {} (facility {})",
                    proj.kind.display(),
                    id.0
                ),
            )
            .with_cause(format!(
                "project {} completed after {} days",
                proj.id, proj.total_days
            ));
            self.emit_history_event(ev);
        }
        self.construction_queue.iter().any(|p| p.id == project_id)
    }

    /// Record contraband case resolution timestamp for persistence/audit coherence.
    pub fn resolve_contraband_case(&mut self, case_id: u64, at: SimTime) {
        self.resolved_contraband_at.entry(case_id).or_insert(at);
    }

    /// Auditable policy mutation: emits a `PolicyChange` event (and history
    /// entry) when the value actually changes.
    pub fn audit_policy_change(&mut self, key: &str, old: f32, new: f32) {
        if (old - new).abs() <= f32::EPSILON {
            return;
        }
        let ev = WorldEvent::new(
            0,
            self.time,
            EventKind::PolicyChange,
            format!("Policy changed: {key}: {old:.2} -> {new:.2}"),
        )
        .with_cause("warden/policy mutation audit");
        self.emit_history_event(ev);
    }

    /// Apply a warden action inside the authoritative world state. All engine
    /// and UI paths delegate here so policy, staffing, construction and budget
    /// consequences have a single implementation.
    pub fn apply_warden_action(&mut self, action: WardenAction) {
        match action {
            WardenAction::Expand { building } => {
                let kind = match building.as_str() {
                    "medical" => FacilityKind::Medical,
                    "education" => FacilityKind::Education,
                    "isolation" => FacilityKind::Isolation,
                    "visitor" => FacilityKind::Visitor,
                    "workshop" => FacilityKind::Workshop,
                    "recreation" => FacilityKind::Recreation,
                    "dormitory" => FacilityKind::Dormitory,
                    _ => FacilityKind::CellBlock,
                };
                // Unknown keys fall back to the canonical cell block; known
                // catalog entries carry their designed bed capacity.
                let capacity = alc_warden::expansion::find_building_spec(&building)
                    .map(|spec| spec.capacity)
                    .unwrap_or(40);
                let pid = self.enqueue_construction(kind, 1, capacity);
                self.events.emit(
                    WorldEvent::new(
                        0,
                        self.time,
                        EventKind::Expansion,
                        format!(
                            "Warden queued expansion: {} (project {})",
                            kind.display(),
                            pid
                        ),
                    )
                    .with_cause("warden action"),
                );
            }
            WardenAction::HireGuards { count } => {
                // Engine-side bound: observer controls must not be able to add
                // an unbounded payroll in one action.
                let count = count.min(16);
                let recruitment = Money::from_major(2_000_i64 * count as i64);
                self.economy.spend(
                    self.time.as_secs(),
                    recruitment,
                    Account::Salaries,
                    format!("Warden hired {count} guards"),
                );
                // Seed the pawn location immediately: a day-boundary hire may
                // happen after the last schedule assignment of the run, and a
                // guard without a location violates the one-place invariant.
                let home_facility = self
                    .facilities
                    .first()
                    .map(|f| f.id)
                    .unwrap_or(FacilityId(0));
                for _ in 0..count {
                    let id = self.next_guard_id;
                    self.next_guard_id += 1;
                    let guard_id = GuardId(id);
                    self.guards.push(Guard::new(
                        guard_id,
                        format!("Guard {id}"),
                        GuardRank::Officer,
                        PostType::Corridor,
                        &mut RngStream::new(self.seed, id),
                    ));
                    self.guard_location.insert(guard_id, home_facility);
                }
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::PolicyChange,
                    format!("Hired {count} guards"),
                ));
            }
            WardenAction::IncreaseSecurity => {
                let old = self.policies.security_level;
                self.policies.security_level = (self.policies.security_level + 0.1).min(0.95);
                self.audit_policy_change("security_level", old, self.policies.security_level);
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::PolicyChange,
                    "Security increased",
                ));
            }
            WardenAction::ReduceExpenses => {
                let days_since = self
                    .days_simulated
                    .saturating_sub(self.warden.last_action_day);
                // Cooldown: only act every 7 days, and only when financial health is poor.
                if self.economy.health() < 0.4 && days_since >= 7 {
                    let old = self.policies.rehab_funding_multiplier;
                    self.policies.rehab_funding_multiplier =
                        (self.policies.rehab_funding_multiplier - 0.1).max(0.2);
                    self.warden.last_action_day = self.days_simulated;
                    self.warden.last_decision_at = self.time;
                    self.audit_policy_change(
                        "rehab_funding_multiplier",
                        old,
                        self.policies.rehab_funding_multiplier,
                    );
                }
                // Otherwise defer: no death-spiral cuts when healthy or on cooldown.
            }
            WardenAction::ChangePolicy { policy, value } => {
                let value = value as f32;
                match policy.as_str() {
                    "privileges" => {
                        let old = self.policies.privileges_level;
                        self.policies.privileges_level = value.clamp(0.0, 1.0);
                        self.audit_policy_change(
                            "privileges_level",
                            old,
                            self.policies.privileges_level,
                        );
                    }
                    "security" => {
                        let old = self.policies.security_level;
                        self.policies.security_level = value.clamp(0.0, 1.0);
                        self.audit_policy_change(
                            "security_level",
                            old,
                            self.policies.security_level,
                        );
                    }
                    "rehab_funding" => {
                        let old = self.policies.rehab_funding_multiplier;
                        self.policies.rehab_funding_multiplier = value.clamp(0.2, 3.0);
                        self.audit_policy_change(
                            "rehab_funding_multiplier",
                            old,
                            self.policies.rehab_funding_multiplier,
                        );
                    }
                    "lockdown" => {
                        let enabled = value >= 0.5;
                        let old = if self.policies.lockdown { 1.0 } else { 0.0 };
                        self.policies.lockdown = enabled;
                        self.lockdown_since_day = self.days_simulated;
                        self.audit_policy_change("lockdown", old, if enabled { 1.0 } else { 0.0 });
                    }
                    _ => {}
                }
            }
            WardenAction::NegotiateWithRepresentatives => {
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::PolicyChange,
                    "Warden met with prisoner representatives",
                ));
            }
            WardenAction::FireGuards { count } => {
                let count = count.min(16).min(self.guards.len() as u32);
                for _ in 0..count {
                    if let Some(guard) = self.guards.pop() {
                        self.guard_location.remove(&guard.id);
                    }
                }
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::PolicyChange,
                    format!("Fired {count} guards"),
                ));
            }
            WardenAction::PromoteStaff { guard } => {
                if let Some(g) = self.guards.iter_mut().find(|g| g.id.raw() == guard) {
                    g.rank = match g.rank {
                        GuardRank::Officer => GuardRank::Sergeant,
                        GuardRank::Sergeant => GuardRank::Lieutenant,
                        GuardRank::Lieutenant => GuardRank::Captain,
                        GuardRank::Captain => GuardRank::Captain,
                    };
                    self.events.emit(WorldEvent::new(
                        0,
                        self.time,
                        EventKind::PolicyChange,
                        format!("Guard {guard} promoted to {:?}", g.rank),
                    ));
                }
            }
            WardenAction::ImproveInfrastructure { facility } => {
                if let Some(f) = self.facilities.iter_mut().find(|f| f.id.0 == facility) {
                    f.condition = (f.condition + 0.05).min(1.0);
                    self.events.emit(WorldEvent::new(
                        0,
                        self.time,
                        EventKind::PolicyChange,
                        format!(
                            "Facility {facility} condition improved to {:.2}",
                            f.condition
                        ),
                    ));
                }
            }
            WardenAction::RespondToDisturbance { event_id } => {
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::Investigation,
                    format!("Warden responded to disturbance (event {event_id})"),
                ));
            }
            WardenAction::InvestigateMisconduct { target } => {
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::Investigation,
                    format!("Misconduct investigation initiated: {target}"),
                ));
            }
            WardenAction::AdjustParoleRecommendation { prisoner, lean } => {
                self.events.emit(WorldEvent::new(
                    0,
                    self.time,
                    EventKind::ParoleBoardDecision,
                    format!("Parole recommendation adjusted: prisoner {prisoner} lean={lean:.2}"),
                ));
            }
            WardenAction::RequestFunding { amount_major } => {
                // Funding is a real budget inflow, not just an event. A 30-day
                // ledger cooldown keeps repeated strategy cycles from turning
                // the request action into an infinite money source.
                let amount = amount_major.clamp(1_000, 500_000);
                let cutoff = self.time.as_secs().saturating_sub(30 * 86_400);
                let recent_request = self.economy.ledger.iter().any(|entry| {
                    entry.at_secs >= cutoff && entry.note.starts_with("Warden funding request")
                });
                if recent_request {
                    self.events.emit(WorldEvent::new(
                        0,
                        self.time,
                        EventKind::BudgetChange,
                        format!("Funding request deferred: previous request within 30 days"),
                    ));
                } else {
                    self.economy.record(
                        self.time.as_secs(),
                        Account::GovGrant,
                        Money::from_major(amount),
                        format!("Warden funding request: ${amount}"),
                    );
                    self.events.emit(WorldEvent::new(
                        0,
                        self.time,
                        EventKind::BudgetChange,
                        format!("Funding request granted: ${amount}"),
                    ));
                }
                self.warden.attention = (self.warden.attention - 0.05).max(0.0);
            }
            WardenAction::NoAction { .. } => {}
        }
    }
}

/// Map a scheduled activity to the facility that hosts it (Directive §5.1).
fn facility_for_activity(activity: Activity) -> FacilityKind {
    match activity {
        Activity::Breakfast | Activity::Lunch | Activity::Dinner => FacilityKind::Kitchen,
        Activity::Work | Activity::Program => FacilityKind::Workshop,
        Activity::Education => FacilityKind::Education,
        Activity::Yard => FacilityKind::Yard,
        Activity::Medical | Activity::SickCall => FacilityKind::Medical,
        Activity::Visits => FacilityKind::Visitor,
        Activity::Recreation | Activity::Showers | Activity::Religious | Activity::FreeTime => {
            FacilityKind::Recreation
        }
        Activity::Wake | Activity::Lockdown | Activity::Sleep | Activity::Count => {
            FacilityKind::CellBlock
        }
    }
}

/// Map a scheduled activity to the need-drift activity used by the needs engine.
fn drift_activity(activity: Activity) -> DriftActivity {
    match activity {
        Activity::Breakfast | Activity::Lunch | Activity::Dinner => DriftActivity::Eat,
        Activity::Sleep => DriftActivity::Sleep,
        Activity::Work | Activity::Education | Activity::Program => DriftActivity::Work,
        Activity::Yard => DriftActivity::Yard,
        Activity::Recreation | Activity::Religious | Activity::FreeTime => {
            DriftActivity::Recreation
        }
        Activity::Medical | Activity::SickCall => DriftActivity::Medical,
        Activity::Visits => DriftActivity::Visitor,
        Activity::Lockdown | Activity::Count => DriftActivity::Lockdown,
        Activity::Wake | Activity::Showers => DriftActivity::None,
    }
}

/// Needs relieved by a successfully-attended activity (Directive §4.3).
fn relief_for_activity(activity: Activity) -> Vec<NeedTag> {
    match activity {
        Activity::Breakfast | Activity::Lunch | Activity::Dinner => {
            vec![NeedTag::Hunger, NeedTag::Thirst]
        }
        Activity::Sleep => vec![NeedTag::Sleep],
        Activity::Work | Activity::Education | Activity::Program => vec![NeedTag::Purpose],
        Activity::Yard => vec![NeedTag::Exercise],
        Activity::Recreation | Activity::FreeTime => {
            vec![NeedTag::Stimulation, NeedTag::Companionship]
        }
        Activity::Medical | Activity::SickCall => vec![NeedTag::Medical],
        Activity::Visits => vec![NeedTag::FamilyContact],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(seed: u64, speed: TimeSpeed) -> World {
        let mut w = World::new(seed);
        w.speed = speed;
        w
    }

    #[test]
    fn tick_populates_schedule_decisions_and_locations() {
        let mut w = running(1, TimeSpeed::FastForward);
        assert_eq!(w.current_activity.len(), w.prisoners.len());
        for _ in 0..12 {
            w.tick();
        }
        assert_eq!(w.current_activity.len(), w.prisoners.len());
        assert_eq!(w.current_decisions.len(), w.prisoners.len());
        assert_eq!(w.prisoner_location.len(), w.prisoners.len());
        assert!(
            w.current_decisions
                .values()
                .any(|d| d.action != alc_agents::decision::DecisionAction::None)
        );
    }

    #[test]
    fn schedule_changes_across_the_day() {
        let mut w = running(2, TimeSpeed::Extreme);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..48 {
            w.tick();
            for a in w.current_activity.values() {
                seen.insert(*a);
            }
        }
        assert!(
            seen.len() >= 3,
            "expected a varied daily rhythm, saw {seen:?}"
        );
    }

    #[test]
    fn long_run_produces_emergent_life_events_and_respects_capacity() {
        let mut w = running(3, TimeSpeed::Extreme);
        let mut saw_social = false;
        let mut saw_parole = false;
        // Sample every day: the event bus is bounded, so older events are
        // evicted and cannot be asserted from the final history alone.
        for _ in 0..120 {
            for _ in 0..48 {
                w.tick();
                assert!(w.population() <= w.capacity());
            }
            for kind in w.events.recent().map(|e| e.kind) {
                saw_social |= matches!(
                    kind,
                    EventKind::RelationshipShift | EventKind::Argument | EventKind::Rumor
                );
                saw_parole |= matches!(
                    kind,
                    EventKind::ParoleGranted | EventKind::ParoleDenied | EventKind::ParoleHearing
                );
            }
        }
        assert!(saw_social, "no social encounters over 120 days");
        assert!(saw_parole, "no parole activity over 120 days");
        assert!(!w.prisoners.is_empty(), "population collapsed");
    }

    #[test]
    fn gangs_emerge_in_live_world() {
        let mut w = running(11, TimeSpeed::Extreme);
        for _ in 0..180 {
            for _ in 0..48 {
                w.tick();
            }
        }
        assert!(!w.gangs.is_empty(), "no emergent gangs after 180 live days");
        for g in &w.gangs {
            // Gangs form from 3+ prisoners but may shrink as members leave;
            // every remaining member must still carry the affiliation.
            for m in &g.members {
                let affiliation = w
                    .prisoners
                    .iter()
                    .find(|p| p.id == *m)
                    .and_then(|p| p.gang_affiliation);
                assert_eq!(
                    affiliation,
                    Some(g.id.0),
                    "member {m:?} not affiliated with {:?}",
                    g.id
                );
            }
        }
    }

    #[test]
    fn world_round_trips_with_runtime_state() {
        let mut w = running(4, TimeSpeed::FastForward);
        for _ in 0..8 {
            w.tick();
        }
        let json = serde_json::to_string(&w).expect("serialize");
        let back: World = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.time, w.time);
        assert_eq!(back.current_activity.len(), w.current_activity.len());
        assert_eq!(back.current_decisions.len(), w.current_decisions.len());
        assert_eq!(back.prisoner_location.len(), w.prisoner_location.len());
    }

    // -- 0.05 lifecycle depth ---------------------------------------------

    #[test]
    fn post_release_records_capture_reason_and_sentence_info() {
        let mut w = running(21, TimeSpeed::Paused);
        let id = w.prisoners[0].id;
        let (name, sentence_days, served, classification) = {
            let p = &w.prisoners[0];
            (
                p.name.clone(),
                p.sentence_days,
                p.sentence_served_days,
                p.classification,
            )
        };
        w.record_post_release(id, "parole");
        assert_eq!(w.post_release_records.len(), 1);
        let rec = &w.post_release_records[0];
        assert_eq!(rec.id, 1);
        assert_eq!(rec.prisoner_id, id.raw());
        assert_eq!(rec.name, name);
        assert_eq!(rec.reason, "parole");
        assert_eq!(rec.day, w.days_simulated);
        assert_eq!(rec.sentence_info.sentence_days, sentence_days);
        assert_eq!(rec.sentence_info.sentence_served_days, served);
        assert_eq!(rec.sentence_info.classification, classification);
        assert!(w.events.recent().any(|e| e.kind == EventKind::Release));
        assert!(w.history.iter().any(|e| e.kind == EventKind::Release));
        // Deterministic reintegration outcome is recorded alongside the exit.
        assert_eq!(w.post_release_outcomes.len(), 1);
        let outcome = &w.post_release_outcomes[0];
        assert_eq!(outcome.post_release_id, rec.id);
        assert_eq!(outcome.prisoner_id, id.raw());
        assert_eq!(outcome.outcome_seed, rec.outcome_seed);
        assert_eq!(outcome.employment, rec.employment);
        assert_eq!(outcome.housing, rec.housing);
        assert_eq!(outcome.violation, rec.violation);
        assert_eq!(outcome.return_to_custody, rec.return_to_custody);
        assert!(!outcome.return_to_custody || outcome.violation);
    }

    #[test]
    fn death_pathway_emits_and_removes() {
        let mut w = running(22, TimeSpeed::Paused);
        let before = w.population();
        let id = w.prisoners[0].id;
        {
            let p = w.prisoners.iter_mut().find(|p| p.id == id).unwrap();
            p.condition.health = 0.0;
            p.needs.medical_need = 1.0;
            p.needs.hygiene = 0.9;
            p.needs.fatigue = 0.9;
        }
        w.resolve_medical_consequences_step();
        assert!(w.population() < before);
        assert!(w.events.recent().any(|e| e.kind == EventKind::Death));
        assert!(w.history.iter().any(|e| e.kind == EventKind::Death));
        assert!(w.post_release_records.iter().any(|r| r.reason == "death"));
    }

    #[test]
    fn transfer_out_pathway_on_overcrowding() {
        let mut w = running(23, TimeSpeed::Paused);
        for f in &mut w.facilities {
            f.capacity = 2;
        }
        // Bypass the rate limit for the deterministic pathway test.
        w.days_simulated = 10;
        w.last_transfer_day = 0;
        let before = w.population();
        w.resolve_transfers_step();
        assert!(w.population() < before);
        assert!(w.events.recent().any(|e| e.kind == EventKind::TransferOut));
        assert!(w.history.iter().any(|e| e.kind == EventKind::TransferOut));
        assert!(
            w.post_release_records
                .iter()
                .any(|r| r.reason == "transfer")
        );
    }

    #[test]
    fn contraband_inventory_and_discovery_tracked() {
        use alc_social::ContrabandFound;
        let mut w = running(24, TimeSpeed::Paused);
        let id = w.prisoners[0].id;
        assert_eq!(
            w.contraband_inventory_count(id, &ContrabandItem::Weapons),
            0
        );

        w.record_contraband_discovery(ContrabandFound {
            guard: w.guards[0].id,
            holder: id,
            item: ContrabandItem::Weapons,
            severity: 0.9,
        });
        assert!(w.contraband_state.get(&id).unwrap().flags_confiscated);
        assert_eq!(
            w.contraband_inventory_count(id, &ContrabandItem::Weapons),
            1
        );
        assert!(
            w.events
                .recent()
                .any(|e| e.kind == EventKind::ContrabandFound)
        );
        // A durable world case record is created for every discovery.
        assert_eq!(w.contraband_cases.len(), 1);
        let case_id = {
            let case = &w.contraband_cases[0];
            assert_eq!(case.case_id, 1);
            assert_eq!(case.holder, id);
            assert_eq!(case.item, ContrabandItem::Weapons);
            assert!(case.summary.contains("Case 1"));
            assert!(case.disciplinary_action.is_some());
            case.case_id
        };
        w.resolve_contraband_case(case_id, w.time);
        assert!(w.resolved_contraband_at.contains_key(&case_id));

        // Inventory movement through the world state.
        w.move_contraband(id, &ContrabandItem::Weapons, -1);
        assert_eq!(
            w.contraband_inventory_count(id, &ContrabandItem::Weapons),
            0
        );
        w.move_contraband(id, &ContrabandItem::Weapons, 2);
        assert_eq!(
            w.contraband_inventory_count(id, &ContrabandItem::Weapons),
            2
        );

        // The daily step runs without panicking.
        w.contraband_step();
        assert!(
            w.events
                .recent()
                .any(|e| e.kind == EventKind::ContrabandFound)
        );
    }

    #[test]
    fn rehab_records_track_progress_and_honor_funding_multiplier() {
        let mut w = running(26, TimeSpeed::Paused);
        let id = w.prisoners[0].id;
        {
            let p = w.prisoners.iter_mut().find(|p| p.id == id).unwrap();
            p.skills
                .insert(alc_social::RehabProgram::Education.key().to_string(), 0.0);
        }
        w.policies.rehab_funding_multiplier = 2.0;
        w.rehab_step();
        assert!(w.rehab_records.get(&id).is_some_and(|recs| {
            recs.iter()
                .any(|r| r.program == alc_social::RehabProgram::Education)
        }));
        assert!(
            w.events
                .recent()
                .any(|e| e.kind == EventKind::RehabilitationProgress)
        );

        // Zero funding multiplier => no progress events.
        let mut w0 = running(26, TimeSpeed::Paused);
        let id0 = w0.prisoners[0].id;
        w0.prisoners
            .iter_mut()
            .find(|p| p.id == id0)
            .unwrap()
            .skills
            .insert(alc_social::RehabProgram::Education.key().to_string(), 0.0);
        w0.policies.rehab_funding_multiplier = 0.0;
        w0.rehab_step();
        assert!(
            w0.events
                .recent()
                .all(|e| e.kind != EventKind::RehabilitationProgress)
        );
    }

    #[test]
    fn policy_changes_emit_auditable_events() {
        let mut w = running(27, TimeSpeed::Paused);
        w.audit_policy_change("security_level", 0.5, 0.7);
        assert!(w.events.recent().any(|e| e.kind == EventKind::PolicyChange));
        assert!(w.history.iter().any(|e| e.kind == EventKind::PolicyChange));
    }

    #[test]
    fn durable_history_events_carry_bus_ids() {
        let mut w = running(30, TimeSpeed::Paused);
        w.audit_policy_change("security_level", 0.5, 0.7);
        assert!(w.history.iter().all(|e| e.id > 0));
        let event = w.history.last().expect("durable history event");
        assert!(
            w.events.recent().any(|e| e.id == event.id),
            "durable id {} missing from bus",
            event.id
        );
    }

    #[test]
    fn transfer_out_purges_gang_roster() {
        let mut w = running(31, TimeSpeed::Paused);
        let id = w
            .prisoners
            .iter()
            .min_by_key(|p| p.id.0)
            .map(|p| p.id)
            .expect("prisoner");
        assert!(w.create_gang(GangId(1), "Transfer Gang", id));
        assert!(w.gangs.iter().any(|g| g.members.contains(&id)));
        for f in &mut w.facilities {
            f.capacity = 2;
        }
        w.days_simulated = 10;
        w.last_transfer_day = 0;
        w.resolve_transfers_step();
        assert!(w.prisoners.iter().all(|p| p.id != id));
        assert!(w.gangs.iter().all(|g| !g.members.contains(&id)));
        assert!(w.prisoners.iter().all(|p| p.gang_affiliation != Some(1)));
    }

    #[test]
    fn escape_consequence_helper_audits_security_and_investigation() {
        let mut w = running(32, TimeSpeed::Paused);
        let id = w.prisoners[0].id;
        let consequences = EscapeConsequences {
            outcome: EscapeOutcome::DiscoveredDuring,
            security_level_delta: 0.2,
            investigation_opened: true,
            investigation_scope: 0.8,
            lockdown_recommended: true,
            policy_pressure: 1.0,
            prisoner_discipline_severity: 0.7,
            notes: vec!["test note".into()],
            ..Default::default()
        };
        let mut event =
            WorldEvent::new(0, w.time, EventKind::EscapeAttempt, "escape").with_actor(id.raw());
        let mut rng = RngStream::new(3, 3);
        w.apply_escape_consequences_step(&consequences, id, &mut event, &mut rng);
        assert!((w.policies.security_level - 0.8).abs() < 1e-6);
        assert!(event.causes.iter().any(|c| c == "test note"));
        assert!(event.causes.iter().any(|c| c.contains("investigation")));
        assert!(
            w.events
                .recent()
                .any(|e| e.kind == EventKind::Investigation)
        );
        assert!(
            w.history
                .iter()
                .any(|e| e.kind == EventKind::Investigation && e.id > 0)
        );
    }

    fn rehab_record(
        program: alc_social::RehabProgram,
        completed: bool,
        attendance_days: u32,
    ) -> RehabProgramRecord {
        RehabProgramRecord {
            program,
            enrolled_day: 0,
            progress_days: 1,
            progress_sum: 1.0,
            completed,
            dropped: false,
            attendance_days,
            last_attended_day: Some(1),
            failure_reason: None,
        }
    }

    #[test]
    fn legacy_rehab_completion_records_seed_markers_and_close_stale_active_duplicates() {
        let mut w = running(33, TimeSpeed::Paused);
        let id = w.prisoners[0].id;
        let program = alc_social::RehabProgram::Education;
        let marker = format!("completed:{}", program.key());
        w.rehab_records.insert(
            id,
            vec![
                rehab_record(program, true, 30),
                rehab_record(program, false, 2),
            ],
        );
        w.prisoners[0].skills.insert(program.key().to_string(), 0.2);
        // Motivation cannot re-enroll anything; this test isolates the marker.
        w.prisoners[0].condition.hope = 0.0;
        w.policies.rehab_funding_multiplier = 0.0;
        w.rehab_step();
        assert_eq!(w.prisoners[0].skills.get(&marker), Some(&1.0));
        assert!(!w.prisoners[0].skills.contains_key(program.key()));
        let records = &w.rehab_records[&id];
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].attendance_days, 30);
        assert!(records[0].completed);
        assert!(records[1].dropped, "mixed active duplicate must be closed");
        assert_eq!(
            records[1].failure_reason,
            Some(RehabFailureReason::ProgramCompleted)
        );
        assert_eq!(
            records.iter().filter(|r| r.is_active()).count(),
            0,
            "completion must not strand capacity-consuming active records"
        );
    }

    #[test]
    fn rehab_progress_updates_newest_active_duplicate_record() {
        let mut w = running(34, TimeSpeed::Paused);
        let id = w.prisoners[0].id;
        let program = alc_social::RehabProgram::Education;
        w.rehab_records.insert(
            id,
            vec![
                rehab_record(program, false, 2),
                rehab_record(program, false, 2),
            ],
        );
        w.prisoners[0].skills.insert(program.key().to_string(), 0.2);
        w.prisoners[0].condition.hope = 0.0;
        w.policies.rehab_funding_multiplier = 2.0;
        w.rehab_step();
        let records = &w.rehab_records[&id];
        assert_eq!(records[0].attendance_days, 2);
        assert!(
            records[1].attendance_days > 2,
            "newest active record must receive progress"
        );
    }

    #[test]
    fn rehab_progress_signal_saturates_large_attendance() {
        let mut record = rehab_record(alc_social::RehabProgram::Education, false, u32::MAX);
        record.completed = true;
        let signal = RehabProgramRecord::progress_signal(&[record]);
        assert!((0.0..=1.0).contains(&signal));
    }
}
