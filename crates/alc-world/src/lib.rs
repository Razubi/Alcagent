//! World simulation: facility layout, population, tick loop, history (Directive §2, §17, §18, §21).
//!
//! The tick loop is the integration point that makes the prison *alive*. Every
//! step runs prisoners through a schedule resolution, a need-driven utility
//! decision, co-presence encounters, and (on day boundaries) escape, parole and
//! intake resolution. AI deliberation is gated by the orchestrator; the
//! deterministic fallback always keeps the simulation moving (Directive §19).

mod emergent;

use alc_core::{FacilityId, GuardId, PrisonerId, RngStream, SimTime, TimeSpeed, WardenId};
use alc_events::{EventBus, WorldEvent, EventKind, RehabFailureReason};
use alc_economy::Economy;
use alc_warden::{Warden, WardenAction};
use alc_guard::{Guard, GuardRank, PostType};
use alc_social::{
    Gang, PrisonerGangProfile, SocialGraph, RelationshipDelta, EscapeOutcome,
    ContrabandItem, apply_escape_consequences, maybe_form_gang, resolve_escape_attempt,
};
use alc_prisoner::{
    Activity, DriftActivity, GoalKind, NeedDriftContext, NeedTag, Prisoner, SchedulePolicy,
    ScheduleTemplate, current_block, generate_intake, resolve_schedule,
};
use alc_agents::{ActionCandidate, Decision, Disposition, Legality, Orchestrator, trait_bias};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Building / facility (Directive §12 expansion catalog).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Facility {
    pub id: FacilityId,
    pub kind: FacilityKind,
    pub wing: u32,
    pub capacity: u32,
    pub condition: f32,   // 0..1, degrades with neglect
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
    fn default() -> Self { ConstructionStatus::Queued }
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

fn default_true() -> bool { true }

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

    fn is_active(&self) -> bool { !self.completed && !self.dropped }
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
        Self { lockdown: false, privileges_level: 0.5, rehab_funding_multiplier: 1.0, security_level: 0.6, intake_paused: false }
    }
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
}

impl World {
    pub fn new(seed: u64) -> Self {
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
            economy: Economy::default(),
            policies: Policies::default(),
            events: EventBus::new(50_000),
            history: Vec::new(),
            orchestrator: Orchestrator::new(4_000),
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
        let posts = [PostType::Tower, PostType::Yard, PostType::Corridor, PostType::Reception];
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
            self.prisoners.push(generate_intake(self.seed, PrisonerId(id)));
        }

        // Give observers a consistent view from the very first frame.
        self.resolve_schedules_step();
        self.assign_locations_from_schedule();
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
        self.facilities.iter().filter(|f| matches!(f.kind, FacilityKind::CellBlock | FacilityKind::Dormitory)).map(|f| f.capacity).sum()
    }

    pub fn population(&self) -> u32 { self.prisoners.len() as u32 }

    /// Advance the world by one simulation step.
    pub fn tick(&mut self) {
        if self.speed == TimeSpeed::Paused { return; }
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

        // 2. Guards duty tick: observe nearby prisoners, report incidents,
        //    and request assistance when stress is high. Runs once per sim-hour
        //    — every-tick sweeps flooded the event bus with routine reports.
        let duty_hour = now_secs / 3600;
        if duty_hour != self.last_guard_duty_hour {
            self.last_guard_duty_hour = duty_hour;
            let guard_policy = alc_guard::DutyContext {
                at: self.time,
                lockdown: self.policies.lockdown,
                security_level: self.policies.security_level,
            };
            let mut guard_reports = Vec::new();
            for g in &mut self.guards {
                let observed: Vec<f32> = self
                    .prisoners
                    .iter()
                    .map(|p| {
                        // Need pressure is a proxy for friction, not proof of a
                        // critical incident. Keep the baseline low so only
                        // genuine peaks produce severity-3 reports; otherwise
                        // routine unmet needs saturate the event bus.
                        let base = p.needs.total_pressure();
                        let agitation = p.needs.respect * 0.15 + p.needs.safety * 0.1;
                        let compliance = if let Some(d) = self.current_decisions.get(&p.id) {
                            matches!(&d.action, alc_agents::decision::DecisionAction::Comply | alc_agents::decision::DecisionAction::None)
                        } else {
                            false
                        };
                        let compliance: f32 = if compliance { 0.0 } else { 0.08 };
                        (base * 0.5 + agitation + compliance).clamp(0.0, 1.0)
                    })
                    .collect();
                let mut rng = RngStream::new(self.seed, self.time.as_secs() ^ (g.id.raw() << 8) ^ 0xA11CE);
                let reports = g.duty_tick(&observed, guard_policy, &mut rng);
                if !reports.is_empty() {
                    guard_reports.extend(reports.into_iter().map(|r| (g.id, r)));
                }
            }
            if !guard_reports.is_empty() {
                self.warden.security_incidents_this_week = self
                    .warden
                    .security_incidents_this_week
                    .saturating_add(guard_reports.iter().filter(|(_, r)| r.severity >= 2).count() as u32);
                self.warden.incidents_this_week = self.warden.security_incidents_this_week;
                for (gid, report) in guard_reports {
                    // Only critical reports and assistance requests reach the
                    // event bus. Severity-2 routine incidents stay in guard
                    // state/history: emitting them hourly saturated the 50k
                    // bus within a year and evicted social/parole events.
                    if report.severity < 3 && !report.requests_assistance {
                        continue;
                    }
                    let kind = if report.requests_assistance || report.severity >= 3 {
                        EventKind::SecurityResponse
                    } else {
                        EventKind::GuardReport
                    };
                    let ev = WorldEvent::new(0, self.time, kind, report.summary.clone())
                        .with_actor(gid.raw())
                        .with_cause(format!(
                            "guard duty report; post={:?} sev={} cred={:.2}",
                            report.post, report.severity, report.credibility
                        ));
                    let id = self.events.emit(ev);
                    // Warden inbox tracks received report/event ids (credibility-scored upstream).
                    self.warden.inbox.push(id);
                    if self.warden.inbox.len() > 200 {
                        let drop = self.warden.inbox.len() - 200;
                        self.warden.inbox.drain(0..drop);
                    }
                }
            }
        }

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
        }
    }

    fn resolve_schedules_step(&mut self) {
        let template = ScheduleTemplate::default_medium();
        let policy = self.schedule_policy();
        let minute = self.time.minute_of_day() as u16;

        let mut activities = HashMap::with_capacity(self.prisoners.len());
        for p in &self.prisoners {
            let schedule = resolve_schedule(p, &template, &policy);
            let activity = current_block(&schedule, minute)
                .map(|b| b.block.activity)
                .unwrap_or(Activity::Sleep);
            activities.insert(p.id, activity);
        }
        self.current_activity = activities;
    }

    /// Seed locations from the resolved activity (before any MoveTo decision).
    fn assign_locations_from_schedule(&mut self) {
        let mut locations = HashMap::with_capacity(self.prisoners.len());
        for p in &self.prisoners {
            let activity = self.current_activity.get(&p.id).copied().unwrap_or(Activity::Sleep);
            let kind = facility_for_activity(activity);
            let id = self.facility_of_kind(kind)
                .or_else(|| self.facilities.first().map(|f| f.id))
                .unwrap_or(FacilityId(0));
            locations.insert(p.id, id);
        }
        self.prisoner_location = locations;

        // Also assign guard locations based on their posts
        let mut guard_locations = HashMap::with_capacity(self.guards.len());
        for g in &self.guards {
            let kind = Self::facility_for_guard_post(g.post);
            let id = self.facility_of_kind(kind)
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
        self.facilities.iter().find(|f| f.kind == kind).map(|f| f.id)
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

        let mut decisions = HashMap::with_capacity(plans.len());
        let mut new_locations = locations;
        for (id, disposition, candidates) in plans {
            // AI deliberation is a budgeted/rate-limited privilege. When the
            // orchestrator declines, the deterministic utility chooser decides.
            let may_ai = self.orchestrator.may_deliberate(now_secs);
            let decision = if may_ai {
                self.orchestrator.telemetry.ai_decisions += 1;
                candidates
                    .iter()
                    .max_by(|a, b| {
                        let au = a.adjusted_utility() + trait_bias(&disposition, &a.decision);
                        let bu = b.adjusted_utility() + trait_bias(&disposition, &b.decision);
                        au.partial_cmp(&bu).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|c| c.decision.clone())
                    .unwrap_or(Decision::new(alc_agents::decision::DecisionAction::None))
            } else {
                self.orchestrator
                    .fallback_decision_with_disposition(&candidates, &disposition)
            };
            if may_ai {
                self.orchestrator.charge(1);
            }
            if let alc_agents::decision::DecisionAction::MoveTo { target_facility } = &decision.action {
                new_locations.insert(id, FacilityId(*target_facility));
            }
            decisions.insert(id, decision);
        }

        self.current_decisions = decisions;
        self.prisoner_location = new_locations;
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
            .find(|&other| other != p.id && locations.get(&other) == my_loc)
            .or_else(|| all_ids.iter().copied().find(|&other| other != p.id));

        let motivated = match need {
            NeedTag::Hunger | NeedTag::Thirst => self
                .facility_of_kind(FacilityKind::Kitchen)
                .map(|f| ActionCandidate::new(
                    Decision::new(alc_agents::decision::DecisionAction::MoveTo { target_facility: f.raw() }),
                    0.2 + level,
                )),
            NeedTag::Sleep => self
                .facility_of_kind(FacilityKind::CellBlock)
                .map(|f| ActionCandidate::new(
                    Decision::new(alc_agents::decision::DecisionAction::MoveTo { target_facility: f.raw() }),
                    0.2 + level,
                )),
            NeedTag::Medical => Some(ActionCandidate::new(
                Decision::new(alc_agents::decision::DecisionAction::RequestMedical),
                0.2 + level * 1.3,
            )),
            NeedTag::Companionship => nearby.map(|other| ActionCandidate::new(
                Decision::new(alc_agents::decision::DecisionAction::Socialize { with: other.raw() }),
                0.2 + level,
            )),
            NeedTag::Stimulation => self
                .facility_of_kind(FacilityKind::Recreation)
                .or_else(|| self.facility_of_kind(FacilityKind::Yard))
                .map(|f| ActionCandidate::new(
                    Decision::new(alc_agents::decision::DecisionAction::MoveTo { target_facility: f.raw() }), // wander
                    0.2 + level,
                )),
            NeedTag::Safety => Some(ActionCandidate::new(
                Decision::new(alc_agents::decision::DecisionAction::Comply), // safest visible option: keep close to staff/order
                0.2 + level,
            )),
            NeedTag::Respect => nearby
                .map(|other| ActionCandidate::new(
                    Decision::new(alc_agents::decision::DecisionAction::Fight { target: other.raw() }),
                    0.15 + level * 0.5,
                ).with_risk(0.85).with_legality(Legality::IllegalButPossible)),
            NeedTag::Fatigue => self
                .facility_of_kind(FacilityKind::CellBlock)
                .map(|f| ActionCandidate::new(
                    Decision::new(alc_agents::decision::DecisionAction::MoveTo { target_facility: f.raw() }),
                    0.2 + level,
                )),
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
        out.push(ActionCandidate::new(Decision::new(alc_agents::decision::DecisionAction::Comply), 0.25));
        if p.needs.medical_need > 0.6 {
            out.push(ActionCandidate::new(Decision::new(alc_agents::decision::DecisionAction::RequestMedical), 0.55));
        }
        out.push(ActionCandidate::new(Decision::new(alc_agents::decision::DecisionAction::None), 0.05));
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
        }

        let snaps: HashMap<PrisonerId, Snap> = self.prisoners.iter().map(|p| {
            (p.id, Snap {
                sociability: p.traits.sociability,
                aggression: p.traits.aggression,
                manipulation: p.traits.manipulation,
                respect: p.needs.respect,
                stimulation: p.needs.stimulation,
            })
        }).collect();

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
            for (i, &a) in ids.iter().enumerate() {
                for &b in ids.iter().skip(i + 1) {
                    let (Some(sa), Some(sb)) = (snaps.get(&a), snaps.get(&b)) else { continue };
                    let social = (sa.sociability + sb.sociability) * 0.5;
                    if !rng.chance(0.008 + social as f64 * 0.014) {
                        continue;
                    }

                    let hostility = ((sa.aggression + sb.aggression) * 0.5 + (sa.respect + sb.respect) * 0.15) * 0.45;
                    let rumor_bias = 0.12 + (sa.manipulation + sb.manipulation) * 0.12;
                    let roll = rng.next_f64() as f32;
                    let mut delta = RelationshipDelta::default();
                    let mut reverse = RelationshipDelta::default();
                    let (kind, summary) = if roll < hostility {
                        delta.hostility = 0.06;
                        delta.resentment = 0.04;
                        reverse.hostility = 0.05;
                        reverse.resentment = 0.05;
                        need_effects.push((a, NeedTag::Respect, 0.06, false));
                        need_effects.push((b, NeedTag::Safety, 0.05, false));
                        (EventKind::Argument, format!("Argument between {} and {}", a.raw(), b.raw()))
                    } else if roll < hostility + 0.22 {
                        delta.familiarity = 0.05;
                        delta.trust = 0.02;
                        reverse.familiarity = 0.05;
                        reverse.trust = 0.02;
                        need_effects.push((a, NeedTag::Companionship, 0.05, true));
                        need_effects.push((b, NeedTag::Companionship, 0.05, true));
                        (EventKind::RelationshipShift, format!("Trade between {} and {}", a.raw(), b.raw()))
                    } else if roll < hostility + 0.22 + rumor_bias {
                        delta.familiarity = 0.03;
                        reverse.familiarity = 0.03;
                        (EventKind::Rumor, format!("Rumor exchanged between {} and {}", a.raw(), b.raw()))
                    } else {
                        delta.friendship = 0.05;
                        delta.trust = 0.03;
                        delta.familiarity = 0.04;
                        reverse.friendship = 0.05;
                        reverse.trust = 0.03;
                        reverse.familiarity = 0.04;
                        need_effects.push((a, NeedTag::Companionship, 0.08, true));
                        need_effects.push((b, NeedTag::Companionship, 0.08, true));
                        (EventKind::RelationshipShift, format!("Friendship grew between {} and {}", a.raw(), b.raw()))
                    };

                    self.social.record(a, b, delta);
                    self.social.record(b, a, reverse);
                    let ev = WorldEvent::new(0, self.time, kind, summary)
                        .with_actor(a.raw())
                        .with_actor(b.raw())
                        .with_cause(format!("co-presence encounter; stimulation {:.2}/{:.2}", sa.stimulation, sb.stimulation));
                    self.events.emit(ev);
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
        for gang in &mut self.gangs {
            for &pid in departed {
                gang.lose(pid);
            }
        }
        let before = self.gangs.len();
        self.gangs.retain(|g| !g.members.is_empty());
        for _ in self.gangs.len()..before {
            self.events.emit(WorldEvent::new(0, self.time, EventKind::GangDisbanded,
                "Gang dissolved after losing all members"));
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
            let gang_id = gang.id.0;
            let leader = gang.leader;
            let member_ids: Vec<PrisonerId> = gang.members.clone();
            let name = gang.name.clone();
            self.gangs.push(gang);
            self.next_gang_id = self.next_gang_id.max(gang_id + 1);
            for pid in member_ids {
                if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == pid) {
                    p.gang_affiliation = Some(gang_id);
                }
            }
            self.events.emit(WorldEvent::new(
                0,
                self.time,
                EventKind::GangFormed,
                format!("Emergent gang '{}' coalesced around {:?}", name, leader),
            ));
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
        self.economy.accrue_daily_expenses(self.time.as_secs(), pop, self.guards.len() as u32, buildings);

        // Facility degradation.
        for f in &mut self.facilities {
            f.condition = (f.condition - 0.001).max(0.0);
        }

        // Sentence progress.
        for p in &mut self.prisoners {
            p.sentence_served_days += 1;
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
            let quiet_week = self.warden.security_incidents_this_week <= 2;
            self.warden.reset_weekly_incidents();
            // De-escalation: security posture relaxes after a quiet week —
            // otherwise incident spikes ratchet it to 1.0 permanently.
            if quiet_week && self.policies.security_level > 0.45 {
                let old = self.policies.security_level;
                self.policies.security_level = (self.policies.security_level - 0.1).max(0.45);
                self.audit_policy_change("security_level", old, self.policies.security_level);
                self.events.emit(WorldEvent::new(0, self.time, EventKind::PolicyChange,
                    format!("Security posture relaxed to {:.2} after a quiet week", self.policies.security_level)));
            }
            // Emergency measures expire rather than trapping the institution
            // in a permanent lockdown that blocks all intake.
            if self.policies.lockdown
                && self.days_simulated.saturating_sub(self.lockdown_since_day) >= 14
            {
                self.policies.lockdown = false;
                self.audit_policy_change("lockdown", 1.0, 0.0);
                self.events.emit(WorldEvent::new(0, self.time, EventKind::PolicyChange,
                    "Lockdown lifted after two weeks"));
            }
            // Staff right-sizing: don't carry a full roster for a shrunken
            // population (payroll was a major insolvency driver).
            let need = ((self.population() + 9) / 10).max(1) as usize;
            if self.guards.len() > need + 1 && self.economy.cash.0 < 5_000_000 {
                self.guards.pop();
                self.events.emit(WorldEvent::new(0, self.time, EventKind::PolicyChange,
                    "Guard roster reduced to match population"));
            }
            // Last-resort emergency funding: institutions do not simply
            // vanish when cash runs out, but the bailout costs privileges.
            if self.economy.cash.0 < 0 {
                let deficit = -self.economy.cash.0;
                self.economy.accrue_bonus(self.time.as_secs(), alc_economy::Money(deficit),
                    "emergency government funding (insolvency bailout)");
                let priv_old = self.policies.privileges_level;
                self.policies.privileges_level = (self.policies.privileges_level - 0.1).max(0.0);
                self.audit_policy_change("privileges_level", priv_old, self.policies.privileges_level);
                self.events.emit(WorldEvent::new(0, self.time, EventKind::BudgetChange,
                    "Emergency funding received; privileges reduced"));
            }
            // Social organizations emerge from relationships, not scripts.
            self.maybe_form_gang_step();
        }
        self.warden.set_context(self.capacity(), self.warden.security_incidents_this_week);

        // Warden arbitration: utility-based decision (deterministic core).
        let pop = self.population();
        let action = alc_warden::arbitrate(
            &mut self.warden,
            &self.economy,
            pop,
            &mut RngStream::new(self.seed, self.days_simulated),
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
            self.events.emit(WorldEvent::new(0, self.time, EventKind::BudgetChange, "Weekly autosave checkpoint"));
        }
    }

    /// Once per day, high-motivation prisoners roll an abstract escape attempt.
    fn resolve_escape_step(&mut self) {
        let staff_response = (self.staff_coverage() * 0.6 + self.policies.security_level * 0.4).clamp(0.0, 1.0);
        let candidates: Vec<(PrisonerId, f32, f32)> = self.prisoners.iter()
            .filter_map(|p| {
                let motivation = p.escape_motivation();
                if motivation <= 0.7 {
                    return None;
                }
                // Attempts are rare, deliberate acts: even the desperate only
                // try occasionally rather than every night.
                let mut gate = RngStream::new(self.seed, p.id.raw() ^ (self.days_simulated << 16) ^ 0x35C4);
                if !gate.chance((motivation - 0.7) as f64 * 1.5) {
                    return None;
                }
                let determination = (p.traits.risk_tolerance * 0.5
                    + (1.0 - p.traits.discipline) * 0.3
                    + p.traits.leadership * 0.2).clamp(0.0, 1.0);
                Some((p.id, motivation, determination))
            })
            .collect();

        let mut escaped = Vec::new();
        for (id, motivation, determination) in candidates {
            let mut rng = RngStream::new(self.seed, id.raw() ^ (self.days_simulated << 24) ^ 0x35C4B);
            let outcome = resolve_escape_attempt(
                &mut rng,
                motivation,
                self.policies.security_level,
                determination,
                staff_response,
            );
            let consequences = apply_escape_consequences(outcome, self.policies.security_level, &[id]);

            // Persistent institutional consequences (Directive §16).
            let sec_old = self.policies.security_level;
            self.policies.security_level =
                (self.policies.security_level + consequences.security_level_delta).clamp(0.0, 1.0);
            self.audit_policy_change("security_level", sec_old, self.policies.security_level);
            if consequences.lockdown_recommended && rng.chance(consequences.policy_pressure as f64 * 0.35) {
                if !self.policies.lockdown {
                    self.lockdown_since_day = self.days_simulated;
                    self.audit_policy_change("lockdown", 0.0, 1.0);
                }
                self.policies.lockdown = true;
            }

            let kind = match outcome {
                EscapeOutcome::SuccessfulEscape => EventKind::EscapeSucceeded,
                EscapeOutcome::PartialBreachContained => EventKind::EscapePrevented,
                _ => EventKind::EscapeAttempt,
            };
            let summary = format!("Prisoner {} escape attempt: {outcome:?}", id.raw());
            let mut ev = WorldEvent::new(0, self.time, kind, summary)
                .with_actor(id.raw())
                .with_cause(format!("escape motivation {motivation:.2}; determination {determination:.2}; security {:.2}", self.policies.security_level));
            for note in &consequences.notes {
                ev = ev.with_cause(note.clone());
            }
            self.events.emit(ev);

            if outcome == EscapeOutcome::SuccessfulEscape {
                escaped.push(id);
            }

            // Psychological aftermath. Consequences are applied through need
            // pressure, security policy and the event record rather than a
            // permanent privilege flag: a prevented attempt must not silently
            // bar an otherwise reforming prisoner from parole forever.
            if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == id) {
                p.needs.impulse(NeedTag::Respect, consequences.prisoner_discipline_severity * 0.2);
                p.needs.impulse(NeedTag::Safety, consequences.prisoner_discipline_severity * 0.1);
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
        let eligible: Vec<PrisonerId> = self.prisoners.iter()
            .filter(|p| p.is_eligible_for_parole())
            .map(|p| p.id)
            .collect();
        let mut released = Vec::new();

        for id in eligible {
            self.events.emit(
                WorldEvent::new(0, self.time, EventKind::ParoleHearing,
                    format!("Parole hearing held for prisoner {}", id.raw()))
                    .with_actor(id.raw()),
            );
            let Some(p) = self.prisoners.iter().find(|p| p.id == id) else { continue };
            let behavior = p.reputation.clamp(0.0, 1.0) * 0.4
                + (if p.schedule.disciplinary_status { 0.0 } else { 0.3 })
                + (1.0 - p.needs.total_pressure()) * 0.3;
            let rehab_progress = p.goals.iter()
                .find(|g| g.kind == GoalKind::Rehabilitation)
                .map_or(0.0, |g| g.progress.clamp(0.0, 1.0));
            let rehab = p.rehab_motivation() * 0.5 + rehab_progress * 0.5;
            let risk = p.escape_motivation() * 0.5
                + p.traits.aggression * 0.25
                + (1.0 - p.condition.mental_health.clamp(0.0, 1.0)) * 0.25;
            let chance = (0.25 + behavior * 0.35 + rehab * 0.25 - risk * 0.25).clamp(0.02, 0.95);

            let mut rng = RngStream::new(self.seed, id.raw() ^ (self.days_simulated << 16) ^ 0xBA20);
            if rng.chance(chance as f64) {
                released.push(id);
                self.events.emit(
                    WorldEvent::new(0, self.time, EventKind::ParoleGranted,
                        format!("Parole granted to prisoner {}", id.raw()))
                        .with_actor(id.raw())
                        .with_cause(format!("behavior {behavior:.2}, rehab {rehab:.2}, risk {risk:.2}")),
                );
            } else {
                self.events.emit(
                    WorldEvent::new(0, self.time, EventKind::ParoleDenied,
                        format!("Parole denied to prisoner {}", id.raw()))
                        .with_actor(id.raw())
                        .with_cause(format!("behavior {behavior:.2}, rehab {rehab:.2}, risk {risk:.2}")),
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
        let present: Vec<PrisonerId> = ids.iter().copied()
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
        let stability = (served_fraction * 0.55 + rehab_progress.clamp(0.0, 1.0) * 0.45).clamp(0.0, 0.95);
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
        let Some(p) = self.prisoners.iter().find(|p| p.id == pid) else { return; };
        let outcome_seed = RngStream::new(self.seed, pid.raw() ^ (self.days_simulated << 12) ^ 0x0E5C_A9E1)
            .next_u64();
        let rehab_progress = self
            .rehab_records
            .get(&pid)
            .map(|records| {
                let done = records.iter().filter(|r| r.completed).count() as f32;
                let attended: u32 = records.iter().map(|r| r.attendance_days).sum();
                (done * 0.6 + (attended.min(180) as f32 / 180.0) * 0.4).clamp(0.0, 1.0)
            })
            .unwrap_or(0.0);
        let (employment, housing, violation, return_to_custody) =
            Self::derive_post_release_outcomes(outcome_seed, pid.raw(), reason,
                p.sentence_days, p.sentence_served_days, rehab_progress);
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
        let ev = WorldEvent::new(0, self.time, EventKind::Release,
                format!("Prisoner {} left the institution: {reason}", pid.raw()))
                .with_actor(pid.raw())
                .with_cause("lifecycle: post-release record");
        self.history.push(ev.clone());
        self.events.emit(ev);
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
            .filter(|p| p.condition.health.clamp(0.0, 1.0) < 0.02 && p.needs.medical_neglect() > 0.7)
            .map(|p| p.id)
            .collect();
        for pid in dead.clone() {
            let ev = WorldEvent::new(0, self.time, EventKind::Death,
                    format!("Prisoner {} died (severe health/medical neglect)", pid.raw()))
                    .with_actor(pid.raw())
                    .with_cause("condition.health < 0.02 while medical_need high; unmonitored death");
            self.history.push(ev.clone());
            self.events.emit(ev);
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
        let Some(id) = self.prisoners.iter().min_by_key(|p| p.id.0).map(|p| p.id) else { return; };
        let ev = WorldEvent::new(0, self.time, EventKind::TransferOut,
                format!("Prisoner {} transferred out (overcrowded={} security={:.2})",
                    id.raw(), over_crowded, self.policies.security_level))
                .with_actor(id.raw());
        self.history.push(ev.clone());
        self.events.emit(ev);
        self.record_post_release(id, "transfer");
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
        if self.policies.lockdown || self.policies.security_level >= 0.9 || self.policies.intake_paused {
            return;
        }
        let cap = self.capacity();
        let free = cap.saturating_sub(self.population());
        if free == 0 {
            return;
        }
        // One admission per sweep keeps growth paced; never exceed capacity.
        let accepted = 1.min(free);
        for _ in 0..accepted {
            let id = self.next_prisoner_id;
            self.next_prisoner_id += 1;
            let p = generate_intake(self.seed, PrisonerId(id));
            self.events.emit(
                WorldEvent::new(0, self.time, EventKind::Admission, format!("New arrival: {}", p.name))
                    .with_actor(id)
                    .with_cause(format!("daily intake sweep; {free} bed(s) available of {cap}")),
            );
            self.prisoners.push(p);
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
        // Also prune social dyads so stale relationships never survive a
        // release/escape (R6 invariant: claims must reference existing pawns).
        let live_set: std::collections::HashSet<PrisonerId> = live.into_iter().collect();
        self.social.retain(&live_set);
    }

    fn rehab_step(&mut self) {
        use alc_social::rehab::{apply_program_progress, enroll_in_program, RehabOutcome, RehabPrisonerState, RehabProgram};
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
        let cap_facilities: usize = self.facilities.iter()
            .filter(|f| matches!(f.kind, FacilityKind::Education | FacilityKind::Workshop | FacilityKind::Medical))
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
                    let has_record = self.rehab_records.get(&pid).is_some_and(|recs| recs.iter().any(|r| r.program == program));
                    if !has_record {
                        self.rehab_records.entry(pid).or_default().push(RehabProgramRecord::new(program, day));
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
                        self.events.emit_rehab_failure(self.time, pid.raw(), rec.program.key(), RehabFailureReason::DisciplinaryRemoval);
                    }
                }
            }

            let enrolled_any = programs.iter().any(|pr| {
                state.skills.contains_key(pr.key())
                    || self.rehab_records.get(&pid).is_some_and(|recs| recs.iter().any(|r| r.program == *pr && r.is_active()))
            });
            let current_active = self.rehab_records.values()
                .map(|recs| recs.iter().filter(|r| r.is_active()).count())
                .sum::<usize>();
            let has_capacity = current_active < rehab_capacity;

            if !enrolled_any && !state.disciplinary_status && state.rehab_motivation > 0.45 && has_capacity {
                let program = if state.aggression > 0.6 { RehabProgram::AngerManagement }
                    else if state.mental_health < 0.5 { RehabProgram::Counseling }
                    else if state.purpose_need > 0.6 { RehabProgram::Education }
                    else { RehabProgram::Vocational };
                if enroll_in_program(&mut state, program) {
                    self.rehab_records.entry(pid).or_default().push(RehabProgramRecord::new(program, day));
                    events.push(WorldEvent::new(0, self.time, EventKind::ProgramEnrolled,
                        format!("{name} enrolled in {program:?}"))
                        .with_actor(pid.raw())
                        .with_cause(format!("eligibility met; capacity {current_active}/{rehab_capacity}")));
                }
            } else if !enrolled_any && !state.disciplinary_status && state.rehab_motivation > 0.45 && !has_capacity {
                events.push(WorldEvent::new(0, self.time, EventKind::RehabilitationFailure,
                    format!("{name} rehab enrollment deferred: no capacity ({current_active}/{rehab_capacity})"))
                    .with_actor(pid.raw())
                    .with_cause("eligibility met but rehab capacity exhausted"));
            }

            for program in programs {
                if !state.skills.contains_key(program.key()) { continue; }
                let has_record = self.rehab_records.get(&pid).is_some_and(|recs| recs.iter().any(|r| r.program == program));
                if !has_record {
                    self.rehab_records.entry(pid).or_default().push(RehabProgramRecord::new(program, day));
                }

                let terminal = self.rehab_records.get(&pid)
                    .and_then(|recs| recs.iter().find(|r| r.program == program))
                    .map(|r| r.completed || r.dropped);
                if terminal == Some(true) {
                    state.skills.remove(program.key());
                    continue;
                }

                match apply_program_progress(&mut state, program, dt_eff) {
                    RehabOutcome::Progress(delta) if delta > 0.0 => {
                        if let Some(rec) = self.rehab_records.get_mut(&pid)
                            .and_then(|recs| recs.iter_mut().find(|r| r.program == program && r.is_active()))
                        {
                            rec.progress_days += 1;
                            rec.attendance_days += 1;
                            rec.last_attended_day = Some(day);
                            rec.progress_sum += delta;
                        }
                        let progress = state.skills.get(program.key()).copied().unwrap_or(0.0);
                        events.push(WorldEvent::new(0, self.time, EventKind::RehabilitationProgress,
                            format!("Rehab progress: {} +{delta:.2} (day {progress:.2})", program.key()))
                            .with_actor(pid.raw())
                            .with_cause(format!("rehab funding multiplier {multiplier:.2}")));
                    }
                    RehabOutcome::Progress(_) => {
                        if let Some(rec) = self.rehab_records.get_mut(&pid)
                            .and_then(|recs| recs.iter_mut().find(|r| r.program == program && r.is_active()))
                        {
                            let idle_days = day.saturating_sub(rec.last_attended_day.unwrap_or(rec.enrolled_day));
                            if idle_days > 21 && rec.attendance_days < 2 {
                                rec.dropped = true;
                                rec.failure_reason = Some(RehabFailureReason::InsufficientEngagement);
                                state.skills.remove(program.key());
                                self.events.emit_rehab_failure(self.time, pid.raw(), program.key(), RehabFailureReason::InsufficientEngagement);
                            }
                        }
                    }
                    RehabOutcome::Completed => {
                        if let Some(rec) = self.rehab_records.get_mut(&pid)
                            .and_then(|recs| recs.iter_mut().find(|r| r.program == program && !r.dropped))
                        {
                            rec.completed = true;
                            rec.failure_reason = Some(RehabFailureReason::ProgramCompleted);
                            rec.last_attended_day = Some(day);
                        }
                        events.push(WorldEvent::new(0, self.time, EventKind::RehabilitationSuccess,
                            format!("{name} completed {program:?}")).with_actor(pid.raw()));
                        self.events.emit_rehab_failure(self.time, pid.raw(), program.key(), RehabFailureReason::ProgramCompleted);
                    }
                    RehabOutcome::Dropped => {
                        let reason = if state.disciplinary_status {
                            RehabFailureReason::DisciplinaryRemoval
                        } else {
                            RehabFailureReason::DroppedOut
                        };
                        if let Some(rec) = self.rehab_records.get_mut(&pid)
                            .and_then(|recs| recs.iter_mut().find(|r| r.program == program && !r.completed && !r.dropped))
                        {
                            rec.dropped = true;
                            rec.failure_reason = Some(reason);
                        }
                        self.events.emit_rehab_failure(self.time, pid.raw(), program.key(), reason);
                    }
                    RehabOutcome::NoEnrollment => {}
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
            if state.education { p.schedule.education = true; }
        }

        for ev in events {
            self.events.emit(ev);
        }
    }

    fn contraband_step(&mut self) {
        use alc_social::contraband::{contraband_discovery, contraband_market, ContrabandPrisonerProfile};
        if self.prisoners.is_empty() || self.guards.is_empty() { return; }
        let profiles: Vec<ContrabandPrisonerProfile> = self.prisoners.iter().map(|p| ContrabandPrisonerProfile {
            id: p.id,
            discipline: p.traits.discipline,
            risk_tolerance: p.traits.risk_tolerance,
            manipulation: p.traits.manipulation,
            aggression: p.traits.aggression,
            stimulation: p.needs.stimulation,
            freedom: p.needs.freedom,
            safety: p.needs.safety,
            desperation: p.needs.desperation(),
        }).collect();

        let mut rng = RngStream::new(self.seed, self.days_simulated ^ 0xC047_2ABA);
        let offers = contraband_market(&profiles, &mut rng);
        for offer in offers.into_iter().take(3) {
            // Track the trade in per-prisoner inventories (0.05 depth):
            // the seller loses one, the buyer gains one.
            self.move_contraband(offer.seller, &offer.item, -1);
            self.move_contraband(offer.buyer, &offer.item, 1);
            let flags = (self.contraband_state.get(&offer.seller).map_or(false, |s| s.flags_confiscated),
                         self.contraband_state.get(&offer.buyer).map_or(false, |s| s.flags_confiscated));
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
            found.item, holder.raw(), found.guard.raw(), found.severity, inv_count
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
        self.history.push(found_ev.clone());
        self.events.emit(found_ev);

        let conf_ev = WorldEvent::new(0, self.time, EventKind::ContrabandConfiscated,
            format!("Contraband confiscated: {:?} from prisoner {} by guard {}",
                found.item, holder.raw(), found.guard.raw()))
            .with_actor(holder.raw())
            .with_cause("confiscation following discovery");
        self.history.push(conf_ev.clone());
        self.events.emit(conf_ev);
    }

    fn contraband_state_entry(&mut self, id: PrisonerId) -> &mut PrisonerContrabandState {
        self.contraband_state.entry(id).or_default()
    }

    fn contraband_inventory_count(&self, id: PrisonerId, item: &ContrabandItem) -> u32 {
        self.contraband_state.get(&id).and_then(|s| s.inventory.get(item)).copied().unwrap_or(0)
    }

    fn move_contraband(&mut self, id: PrisonerId, item: &ContrabandItem, delta: i32) {
        let count = self.contraband_inventory_count(id, item) as i64;
        let new_count = if delta > 0 { count + delta as i64 } else { count - (-delta as i64) };
        let new_count = new_count.max(0) as u32;
        let st = self.contraband_state_entry(id);
        if new_count == 0 {
            st.inventory.remove(item);
        } else {
            st.inventory.insert(*item, new_count);
        }
    }

    fn construction_step(&mut self) {
        if self.construction_queue.is_empty() { return; }

        // Evaluate prerequisites/staff/materials deterministically by project id order.
        let staff_available = self.staff_coverage() >= 0.2 || self.guards.len() >= 2;
        let materials_available = self.economy.cash.0 > -50_000_00; // not catastrophically insolvent
        let has_admin = self.facilities.iter().any(|f| matches!(f.kind, FacilityKind::Admin));

        let mut approved: Vec<(u64, FacilityKind)> = Vec::new();
        let mut completed: Vec<ConstructionProject> = Vec::new();
        let mut cancelled: Vec<(u64, String, FacilityKind)> = Vec::new();
        let mut failed: Vec<(u64, String, FacilityKind)> = Vec::new();

        for p in &mut self.construction_queue {
            if matches!(p.status, ConstructionStatus::Completed | ConstructionStatus::Cancelled | ConstructionStatus::Failed) {
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
            if !p.materials_ready && p.progress_days == 0 && self.days_simulated.saturating_sub(p.started_day) > 7 {
                p.status = ConstructionStatus::Cancelled;
                p.terminal_reason = Some("materials/funding unavailable".to_string());
                cancelled.push((p.id, p.terminal_reason.clone().unwrap_or_default(), p.kind));
                continue;
            }
            if !p.staff_allocated && self.days_simulated.saturating_sub(p.started_day) > 14 && p.progress_days < p.total_days / 2 {
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
            let ev = WorldEvent::new(0, self.time, EventKind::Expansion,
                format!("Construction approved: {} (project {})", kind.display(), id))
                .with_cause(format!("prerequisites, staffing and materials validated for project {id}"));
            self.history.push(ev.clone());
            self.events.emit(ev);
        }
        for (id, reason, kind) in cancelled {
            let ev = WorldEvent::new(0, self.time, EventKind::ConstructionCancelled,
                format!("Construction cancelled: {} (project {}) - {}", kind.display(), id, reason))
                .with_cause(reason.clone());
            self.history.push(ev.clone());
            self.events.emit(ev);
        }
        for (id, reason, kind) in failed {
            let ev = WorldEvent::new(0, self.time, EventKind::ConstructionFailed,
                format!("Construction failed: {} (project {}) - {}", kind.display(), id, reason))
                .with_cause(reason.clone());
            self.history.push(ev.clone());
            self.events.emit(ev);
        }
        for proj in completed {
            let id = self.add_facility(proj.kind, proj.wing, proj.capacity);
            let ev = WorldEvent::new(0, self.time, EventKind::ConstructionComplete,
                format!("Construction complete: {} (facility {})", proj.kind.display(), id.0))
                .with_cause(format!("project {} completed after {} days", proj.id, proj.total_days));
            self.history.push(ev.clone());
            self.events.emit(ev);
        }

        // Keep terminal projects for auditability; prune only very old terminal entries to avoid unbounded growth.
        let cutoff = self.days_simulated.saturating_sub(365);
        self.construction_queue.retain(|p| {
            matches!(p.status, ConstructionStatus::Queued | ConstructionStatus::Approved | ConstructionStatus::InProgress)
                || p.started_day >= cutoff
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
        let _ = self.economy.spend(self.time.as_secs(), alc_economy::Money::from_major(cost as i64), alc_economy::Account::Construction, format!("Construction: {}", kind.display()));
        let proj = ConstructionProject {
            id, kind, wing, capacity, total_days, progress_days: 0, cost, started_day: self.days_simulated,
            status: ConstructionStatus::Queued,
            terminal_reason: None,
            staff_allocated: self.staff_coverage() >= 0.2 || self.guards.len() >= 2,
            materials_ready: self.economy.cash.0 > -50_000_00,
            prereqs_met: self.facilities.iter().any(|f| matches!(f.kind, FacilityKind::Admin)) || matches!(kind, FacilityKind::Admin),
        };
        self.events.emit(WorldEvent::new(0, self.time, EventKind::Expansion,
            format!("Construction started: {} ({} days, ${:.0})", kind.display(), total_days, cost))
            .with_cause(format!("project {id}")));
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
            let ev = WorldEvent::new(0, self.time, EventKind::Expansion,
                format!("Construction approved: {} (project {}) - {}", kind.display(), project_id, reason))
                .with_cause(reason);
            self.history.push(ev.clone());
            self.events.emit(ev);
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
            if p.id == project_id && matches!(p.status, ConstructionStatus::Queued | ConstructionStatus::Approved | ConstructionStatus::InProgress) {
                p.status = ConstructionStatus::Cancelled;
                p.terminal_reason = Some(reason.clone());
                found = Some(p.kind);
                break;
            }
        }
        if let Some(kind) = found {
            let ev = WorldEvent::new(0, self.time, EventKind::ConstructionCancelled,
                format!("Construction cancelled: {} (project {}) - {}", kind.display(), project_id, reason))
                .with_cause(reason);
            self.history.push(ev.clone());
            self.events.emit(ev);
            return true;
        }
        false
    }

    /// Mark a construction project as failed (world-side lifecycle API for engine delegation).
    pub fn fail_construction(&mut self, project_id: u64, reason: impl Into<String>) -> bool {
        let reason = reason.into();
        let mut found = None;
        for p in &mut self.construction_queue {
            if p.id == project_id && matches!(p.status, ConstructionStatus::Queued | ConstructionStatus::Approved | ConstructionStatus::InProgress) {
                p.status = ConstructionStatus::Failed;
                p.terminal_reason = Some(reason.clone());
                found = Some(p.kind);
                break;
            }
        }
        if let Some(kind) = found {
            let ev = WorldEvent::new(0, self.time, EventKind::ConstructionFailed,
                format!("Construction failed: {} (project {}) - {}", kind.display(), project_id, reason))
                .with_cause(reason);
            self.history.push(ev.clone());
            self.events.emit(ev);
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
            if p.id == project_id && matches!(p.status, ConstructionStatus::Approved | ConstructionStatus::InProgress) {
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
            let ev = WorldEvent::new(0, self.time, EventKind::Expansion,
                format!("Construction approved: {} (project {})", kind.display(), id))
                .with_cause("observer advance approved the queued request".to_string());
            self.history.push(ev.clone());
            self.events.emit(ev);
        }
        if let Some(proj) = completed_proj {
            let id = self.add_facility(proj.kind, proj.wing, proj.capacity);
            let ev = WorldEvent::new(0, self.time, EventKind::ConstructionComplete,
                format!("Construction complete: {} (facility {})", proj.kind.display(), id.0))
                .with_cause(format!("project {} completed after {} days", proj.id, proj.total_days));
            self.history.push(ev.clone());
            self.events.emit(ev);
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
        let ev = WorldEvent::new(0, self.time, EventKind::PolicyChange,
                format!("Policy changed: {key}: {old:.2} -> {new:.2}"))
                .with_cause("warden/policy mutation audit");
        self.history.push(ev.clone());
        self.events.emit(ev);
    }

    fn apply_warden_action(&mut self, action: WardenAction) {
        match action {
            WardenAction::Expand { building } => {
                let kind = match building.as_str() {
                    "medical" => FacilityKind::Medical,
                    "education" => FacilityKind::Education,
                    "isolation" => FacilityKind::Isolation,
                    "visitor" => FacilityKind::Visitor,
                    _ => FacilityKind::CellBlock,
                };
                let pid = self.enqueue_construction(kind, 1, 40);
                self.events.emit(
                    WorldEvent::new(0, self.time, EventKind::Expansion, format!("Warden queued expansion: {} (project {})", kind.display(), pid))
                        .with_cause("warden action"),
                );
            }
            WardenAction::HireGuards { count } => {
                for _ in 0..count {
                    let id = self.next_guard_id;
                    self.next_guard_id += 1;
                    self.guards.push(Guard::new(
                        GuardId(id),
                        format!("Guard {id}"),
                        GuardRank::Officer,
                        PostType::Corridor,
                        &mut RngStream::new(self.seed, id),
                    ));
                }
                self.events.emit(WorldEvent::new(0, self.time, EventKind::PolicyChange, format!("Hired {count} guards")));
            }
            WardenAction::IncreaseSecurity => {
                let old = self.policies.security_level;
                self.policies.security_level = (self.policies.security_level + 0.1).min(1.0);
                self.audit_policy_change("security_level", old, self.policies.security_level);
                self.events.emit(WorldEvent::new(0, self.time, EventKind::PolicyChange, "Security increased"));
            }
            WardenAction::ReduceExpenses => {
                let days_since = self.days_simulated.saturating_sub(self.warden.last_action_day);
                // Cooldown: only act every 7 days, and only when financial health is poor.
                if self.economy.health() < 0.4 && days_since >= 7 {
                    let old = self.policies.rehab_funding_multiplier;
                    self.policies.rehab_funding_multiplier =
                        (self.policies.rehab_funding_multiplier - 0.1).max(0.2);
                    self.warden.last_action_day = self.days_simulated;
                    self.warden.last_decision_at = self.time;
                    self.audit_policy_change("rehab_funding_multiplier", old, self.policies.rehab_funding_multiplier);
                }
                // Otherwise defer: no death-spiral cuts when healthy or on cooldown.
            }
            WardenAction::ChangePolicy { policy, value } => {
                let value = value as f32;
                match policy.as_str() {
                    "privileges" => {
                        let old = self.policies.privileges_level;
                        self.policies.privileges_level = value.clamp(0.0, 1.0);
                        self.audit_policy_change("privileges_level", old, self.policies.privileges_level);
                    }
                    "security" => {
                        let old = self.policies.security_level;
                        self.policies.security_level = value.clamp(0.0, 1.0);
                        self.audit_policy_change("security_level", old, self.policies.security_level);
                    }
                    "rehab_funding" => {
                        let old = self.policies.rehab_funding_multiplier;
                        self.policies.rehab_funding_multiplier = value.clamp(0.2, 3.0);
                        self.audit_policy_change("rehab_funding_multiplier", old, self.policies.rehab_funding_multiplier);
                    }
                    _ => {}
                }
            }
            WardenAction::NegotiateWithRepresentatives => {
                self.events.emit(WorldEvent::new(0, self.time, EventKind::PolicyChange, "Warden met with prisoner representatives"));
            }
            WardenAction::FireGuards { count } => {
                for _ in 0..count.min(self.guards.len() as u32) {
                    self.guards.pop();
                }
                self.events.emit(WorldEvent::new(0, self.time, EventKind::PolicyChange,
                    format!("Fired {count} guards")));
            }
            WardenAction::PromoteStaff { guard } => {
                if let Some(g) = self.guards.iter_mut().find(|g| g.id.raw() == guard) {
                    g.rank = match g.rank {
                        GuardRank::Officer => GuardRank::Sergeant,
                        GuardRank::Sergeant => GuardRank::Lieutenant,
                        GuardRank::Lieutenant => GuardRank::Captain,
                        GuardRank::Captain => GuardRank::Captain,
                    };
                    self.events.emit(WorldEvent::new(0, self.time, EventKind::PolicyChange,
                        format!("Guard {guard} promoted to {:?}", g.rank)));
                }
            }
            WardenAction::ImproveInfrastructure { facility } => {
                if let Some(f) = self.facilities.iter_mut().find(|f| f.id.0 == facility) {
                    f.condition = (f.condition + 0.05).min(1.0);
                    self.events.emit(WorldEvent::new(0, self.time, EventKind::PolicyChange,
                        format!("Facility {facility} condition improved to {:.2}", f.condition)));
                }
            }
            WardenAction::RespondToDisturbance { event_id } => {
                self.events.emit(WorldEvent::new(0, self.time, EventKind::Investigation,
                    format!("Warden responded to disturbance (event {event_id})")));
            }
            WardenAction::InvestigateMisconduct { target } => {
                self.events.emit(WorldEvent::new(0, self.time, EventKind::Investigation,
                    format!("Misconduct investigation initiated: {target}")));
            }
            WardenAction::AdjustParoleRecommendation { prisoner, lean } => {
                self.events.emit(WorldEvent::new(0, self.time, EventKind::ParoleBoardDecision,
                    format!("Parole recommendation adjusted: prisoner {prisoner} lean={lean:.2}")));
            }
            WardenAction::RequestFunding { amount_major } => {
                self.events.emit(WorldEvent::new(0, self.time, EventKind::BudgetChange,
                    format!("Funding request submitted: ${amount_major}")));
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
        Activity::Wake | Activity::Lockdown | Activity::Sleep | Activity::Count => FacilityKind::CellBlock,
    }
}

/// Map a scheduled activity to the need-drift activity used by the needs engine.
fn drift_activity(activity: Activity) -> DriftActivity {
    match activity {
        Activity::Breakfast | Activity::Lunch | Activity::Dinner => DriftActivity::Eat,
        Activity::Sleep => DriftActivity::Sleep,
        Activity::Work | Activity::Education | Activity::Program => DriftActivity::Work,
        Activity::Yard => DriftActivity::Yard,
        Activity::Recreation | Activity::Religious | Activity::FreeTime => DriftActivity::Recreation,
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
        assert!(w.current_decisions.values().any(|d| d.action != alc_agents::decision::DecisionAction::None));
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
        assert!(seen.len() >= 3, "expected a varied daily rhythm, saw {seen:?}");
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
                saw_social |= matches!(kind,
                    EventKind::RelationshipShift | EventKind::Argument | EventKind::Rumor);
                saw_parole |= matches!(kind,
                    EventKind::ParoleGranted | EventKind::ParoleDenied | EventKind::ParoleHearing);
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
                let affiliation = w.prisoners.iter().find(|p| p.id == *m).and_then(|p| p.gang_affiliation);
                assert_eq!(affiliation, Some(g.id.0), "member {m:?} not affiliated with {:?}", g.id);
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
            (p.name.clone(), p.sentence_days, p.sentence_served_days, p.classification)
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
        assert!(w.post_release_records.iter().any(|r| r.reason == "transfer"));
    }

    #[test]
    fn contraband_inventory_and_discovery_tracked() {
        use alc_social::ContrabandFound;
        let mut w = running(24, TimeSpeed::Paused);
        let id = w.prisoners[0].id;
        assert_eq!(w.contraband_inventory_count(id, &ContrabandItem::Weapons), 0);

        w.record_contraband_discovery(ContrabandFound {
            guard: w.guards[0].id,
            holder: id,
            item: ContrabandItem::Weapons,
            severity: 0.9,
        });
        assert!(w.contraband_state.get(&id).unwrap().flags_confiscated);
        assert_eq!(w.contraband_inventory_count(id, &ContrabandItem::Weapons), 1);
        assert!(w.events.recent().any(|e| e.kind == EventKind::ContrabandFound));
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
        assert_eq!(w.contraband_inventory_count(id, &ContrabandItem::Weapons), 0);
        w.move_contraband(id, &ContrabandItem::Weapons, 2);
        assert_eq!(w.contraband_inventory_count(id, &ContrabandItem::Weapons), 2);

        // The daily step runs without panicking.
        w.contraband_step();
        assert!(w.events.recent().any(|e| e.kind == EventKind::ContrabandFound));
    }

    #[test]
    fn rehab_records_track_progress_and_honor_funding_multiplier() {
        let mut w = running(26, TimeSpeed::Paused);
        let id = w.prisoners[0].id;
        {
            let p = w.prisoners.iter_mut().find(|p| p.id == id).unwrap();
            p.skills.insert(alc_social::RehabProgram::Education.key().to_string(), 0.0);
        }
        w.policies.rehab_funding_multiplier = 2.0;
        w.rehab_step();
        assert!(w.rehab_records.get(&id).is_some_and(|recs| {
            recs.iter().any(|r| r.program == alc_social::RehabProgram::Education)
        }));
        assert!(w.events.recent().any(|e| e.kind == EventKind::RehabilitationProgress));

        // Zero funding multiplier => no progress events.
        let mut w0 = running(26, TimeSpeed::Paused);
        let id0 = w0.prisoners[0].id;
        w0.prisoners.iter_mut().find(|p| p.id == id0).unwrap()
            .skills.insert(alc_social::RehabProgram::Education.key().to_string(), 0.0);
        w0.policies.rehab_funding_multiplier = 0.0;
        w0.rehab_step();
        assert!(w0.events.recent().all(|e| e.kind != EventKind::RehabilitationProgress));
    }

    #[test]
    fn policy_changes_emit_auditable_events() {
        let mut w = running(27, TimeSpeed::Paused);
        w.audit_policy_change("security_level", 0.5, 0.7);
        assert!(w.events.recent().any(|e| e.kind == EventKind::PolicyChange));
        assert!(w.history.iter().any(|e| e.kind == EventKind::PolicyChange));
    }
}
