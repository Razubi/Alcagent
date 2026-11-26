//! Pure data extraction for the egui panels — no rendering, no simulation
//! mutations. Everything here operates over read-only engine snapshots
//! (`sumo::Observation`, `sumo::AgentObservation`, `sumo::Stats`,
//! [`alc_world::World`], [`alc_events::WorldEvent`]), so the panels and the
//! unit tests share the same code path.
//!
//! Helpers intentionally reuse the ratatui-era extraction where the
//! [`alc_ui`](crate) crate already provides it:
//! [`alc_ui::views::name_map`], [`alc_ui::views::class_short`] and
//! [`alc_ui::views::truncate`].

use crate::theme;
use alc_core::PrisonerId;
use alc_events::{EventKind, WorldEvent};
use alc_guard::{AuthorityLevel, Guard, GuardRank, PostType};
use alc_prisoner::{
    Activity, Classification, Needs, ResolvedBlock, SchedulePolicy, ScheduleTemplate,
    current_block, resolve_schedule,
};
use alc_warden::WardenAction;
use alc_world::{FacilityKind, World};
use std::collections::{HashMap, VecDeque};
use sumo::{
    AgentObservation, BudgetSnapshot, ConstructionSummary, DecisionAuditRecord, EconomySummary,
    GangSummary, GuardSummary, IncidentRecord, MapFrame, PawnPosition, PolicySummary,
    PostReleaseSummary, PrisonerSummary, RehabOverview, RehabProgramInfo, ScheduleTimeline,
    SocialEdge, SocialRoleSummary, Stats, WardenSummary,
};

// ---------------------------------------------------------------------------
// Incident reports — typed buckets over the live event stream
// ---------------------------------------------------------------------------

/// Typed incident buckets for the Incident Reports panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IncidentKind {
    Fight,
    Argument,
    Threat,
    Escape,
    Contraband,
    Misconduct,
    GangClash,
    Security,
    Other,
}

impl IncidentKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            IncidentKind::Fight => "Fights",
            IncidentKind::Argument => "Arguments",
            IncidentKind::Threat => "Threats",
            IncidentKind::Escape => "Escapes",
            IncidentKind::Contraband => "Contraband items",
            IncidentKind::Misconduct => "Misconduct",
            IncidentKind::GangClash => "Gang clashes",
            IncidentKind::Security => "Security incidents",
            IncidentKind::Other => "Other",
        }
    }

    /// Map a raw event kind onto the incident-report taxonomy.
    #[must_use]
    pub fn from_event_kind(kind: EventKind) -> Self {
        match kind {
            EventKind::Fight => IncidentKind::Fight,
            EventKind::Argument => IncidentKind::Argument,
            EventKind::Threat => IncidentKind::Threat,
            EventKind::GangClash => IncidentKind::GangClash,
            EventKind::EscapeAttempt | EventKind::EscapePrevented | EventKind::EscapeSucceeded => {
                IncidentKind::Escape
            }
            EventKind::ContrabandFound => IncidentKind::Contraband,
            EventKind::StaffMisconduct | EventKind::PropertyDamage | EventKind::Theft => {
                IncidentKind::Misconduct
            }
            EventKind::SecurityResponse
            | EventKind::GuardReport
            | EventKind::Investigation
            | EventKind::Protests => IncidentKind::Security,
            _ => IncidentKind::Other,
        }
    }

    /// Display colour for the bucket (shared by panels).
    #[must_use]
    pub fn colour(self) -> egui::Color32 {
        theme::incident_colour(self)
    }
}

/// One aggregated incident row for the Incident Reports panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncidentRow {
    pub kind: IncidentKind,
    pub count: u32,
}

/// Aggregate typed incidents from an event stream, most frequent first.
#[must_use]
pub fn incident_report_rows(events: &[WorldEvent]) -> Vec<IncidentRow> {
    let mut counts: HashMap<IncidentKind, u32> = HashMap::new();
    for e in events {
        *counts
            .entry(IncidentKind::from_event_kind(e.kind))
            .or_insert(0) += 1;
    }
    let mut rows: Vec<IncidentRow> = counts
        .into_iter()
        .map(|(kind, count)| IncidentRow { kind, count })
        .collect();
    rows.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.kind.label().cmp(b.kind.label()))
    });
    rows
}

// ---------------------------------------------------------------------------
// Live event feed — incremental, capped, newest-last
// ---------------------------------------------------------------------------

/// Maximum number of rows kept in the live feed ring buffer.
pub const LIVE_FEED_CAP: usize = 400;

/// Append newly observed events to the live feed (newest last). The panel
/// tracks `last_seen` as the window pointer; this function is pure: it
/// appends everything given and evicts the oldest entries past `cap`.
pub fn live_feed_append(feed: &mut VecDeque<WorldEvent>, events: &[WorldEvent], cap: usize) {
    for e in events {
        feed.push_back(e.clone());
    }
    while feed.len() > cap {
        feed.pop_front();
    }
}

// ---------------------------------------------------------------------------
// Policy management — GUI policy knobs mirrored by the Policy Management panel
// ---------------------------------------------------------------------------

/// GUI-level policy knobs mirrored by the Policy Management panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyChangeKind {
    SecurityUp,
    SecurityDown,
    LockdownOn,
    LockdownOff,
    PrivilegesUp,
    PrivilegesDown,
    RehabFundingUp,
    RehabFundingDown,
}

impl PolicyChangeKind {
    pub const ALL: [PolicyChangeKind; 8] = [
        PolicyChangeKind::SecurityUp,
        PolicyChangeKind::SecurityDown,
        PolicyChangeKind::LockdownOn,
        PolicyChangeKind::LockdownOff,
        PolicyChangeKind::PrivilegesUp,
        PolicyChangeKind::PrivilegesDown,
        PolicyChangeKind::RehabFundingUp,
        PolicyChangeKind::RehabFundingDown,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            PolicyChangeKind::SecurityUp => "Security ↑",
            PolicyChangeKind::SecurityDown => "Security ↓",
            PolicyChangeKind::LockdownOn => "Lockdown: ON",
            PolicyChangeKind::LockdownOff => "Lockdown: OFF",
            PolicyChangeKind::PrivilegesUp => "Privileges ↑",
            PolicyChangeKind::PrivilegesDown => "Privileges ↓",
            PolicyChangeKind::RehabFundingUp => "Rehab funding ↑",
            PolicyChangeKind::RehabFundingDown => "Rehab funding ↓",
        }
    }

    /// Panel state for the button row: what the control currently reads as.
    #[must_use]
    pub fn current_value(self, policies: &alc_world::Policies) -> f64 {
        match self {
            PolicyChangeKind::SecurityUp | PolicyChangeKind::SecurityDown => {
                policies.security_level as f64
            }
            PolicyChangeKind::LockdownOn | PolicyChangeKind::LockdownOff => {
                if policies.lockdown {
                    1.0
                } else {
                    0.0
                }
            }
            PolicyChangeKind::PrivilegesUp | PolicyChangeKind::PrivilegesDown => {
                policies.privileges_level as f64
            }
            PolicyChangeKind::RehabFundingUp | PolicyChangeKind::RehabFundingDown => {
                policies.rehab_funding_multiplier as f64
            }
        }
    }
}

/// Build the `WardenAction` a caller can submit for a policy change. The GUI
/// itself routes policy changes through validated `Command::Set*` variants in
/// `alc_gui::app` (which call `sumo::Sim` setters); this helper is kept for
/// tests and non-GUI callers of `sumo::Sim::apply_warden_action`.
#[must_use]
pub fn policy_change_action(kind: PolicyChangeKind, current: &alc_world::Policies) -> WardenAction {
    use PolicyChangeKind::*;
    match kind {
        SecurityUp => WardenAction::IncreaseSecurity,
        SecurityDown => WardenAction::ChangePolicy {
            policy: "security".into(),
            value: (current.security_level - 0.1).clamp(0.05, 1.0) as f64,
        },
        LockdownOn => WardenAction::ChangePolicy {
            policy: "lockdown".into(),
            value: 1.0,
        },
        LockdownOff => WardenAction::ChangePolicy {
            policy: "lockdown".into(),
            value: 0.0,
        },
        PrivilegesUp => WardenAction::ChangePolicy {
            policy: "privileges".into(),
            value: (current.privileges_level + 0.1).clamp(0.0, 1.0) as f64,
        },
        PrivilegesDown => WardenAction::ChangePolicy {
            policy: "privileges".into(),
            value: (current.privileges_level - 0.1).clamp(0.0, 1.0) as f64,
        },
        RehabFundingUp => WardenAction::ChangePolicy {
            policy: "rehab_funding".into(),
            value: (current.rehab_funding_multiplier + 0.1).clamp(0.2, 3.0) as f64,
        },
        RehabFundingDown => WardenAction::ChangePolicy {
            policy: "rehab_funding".into(),
            value: (current.rehab_funding_multiplier - 0.1).clamp(0.2, 3.0) as f64,
        },
    }
}

// ---------------------------------------------------------------------------
// Guard Assignment roster — guard details for the Staff screen
// ---------------------------------------------------------------------------

/// Read-only snapshot of one guard for the Staff roster grids.
#[derive(Debug, Clone, PartialEq)]
pub struct GuardRosterRow {
    pub id_raw: u64,
    pub name: String,
    pub rank: GuardRank,
    pub post: PostType,
    pub shift: u8,
    pub fatigue: f32,
    pub stress: f32,
    pub morale: f32,
    pub authority: AuthorityLevel,
    pub credibility: f32,
    pub history: Vec<String>,
}

/// Build a roster row from a world guard.
#[must_use]
pub fn guard_roster_row(guard: &Guard) -> GuardRosterRow {
    GuardRosterRow {
        id_raw: guard.id.raw(),
        name: guard.name.clone(),
        rank: guard.rank,
        post: guard.post,
        shift: guard.shift,
        fatigue: guard.fatigue,
        stress: guard.stress,
        morale: guard.morale,
        authority: guard.authority,
        credibility: guard.credibility(),
        history: guard.history.clone(),
    }
}

/// Guard-posts summary (post kind → posting count) for the Prison Map screen.
#[must_use]
pub fn guard_posts_summary(world: &World) -> Vec<(String, usize)> {
    let mut posts: Vec<(PostType, usize)> = Vec::new();
    for g in &world.guards {
        if let Some(entry) = posts.iter_mut().find(|(p, _)| *p == g.post) {
            entry.1 += 1;
        } else {
            posts.push((g.post, 1));
        }
    }
    posts.sort_by_key(|(p, _)| format!("{p:?}"));
    posts
        .into_iter()
        .map(|(p, n)| (format!("{p:?}"), n))
        .collect()
}

/// Fraction of required facility staff actually staffed today
/// (mirrors `alc_world::World::staff_coverage`, private to the engine).
#[must_use]
pub fn staff_coverage(world: &World) -> f32 {
    let required: u32 = world.facilities.iter().map(|f| f.staff_required).sum();
    if required == 0 {
        1.0
    } else {
        (world.guards.len() as f32 / required as f32).min(1.0)
    }
}

// ---------------------------------------------------------------------------
// Daily Schedule — unresolved template view (§5, design doc §4.1)
// ---------------------------------------------------------------------------

/// One row of the Daily Schedule panel: a prisoner plus their live activity
/// and the policy-resolved daily schedule.
#[derive(Debug, Clone)]
pub struct ScheduleRow {
    pub name: String,
    pub classification: Classification,
    /// `world.current_activity` — the activity resolved for the latest tick.
    pub current: Option<Activity>,
    /// The full unresolved template resolved against live policy.
    pub resolved: Vec<ResolvedBlock>,
}

/// Build Daily Schedule rows for every prisoner in the world.
#[must_use]
pub fn daily_schedule_rows(world: &World) -> Vec<ScheduleRow> {
    world
        .prisoners
        .iter()
        .map(|p| ScheduleRow {
            name: p.name.clone(),
            classification: p.classification,
            current: world.current_activity.get(&p.id).copied(),
            resolved: resolve_schedule(
                p,
                &ScheduleTemplate::default_medium(),
                &schedule_policy(world),
            ),
        })
        .collect()
}

/// The policy view consumed by `resolve_schedule` — built from world state
/// (mirrors `alc_world::World::schedule_policy`, private to the engine).
#[must_use]
pub fn schedule_policy(world: &World) -> SchedulePolicy {
    SchedulePolicy {
        lockdown: world.policies.lockdown,
        privileges_level: world.policies.privileges_level.max(0.05),
        security_level: world.policies.security_level,
        staff_coverage: staff_coverage(world),
        weather_ok: true,
        rehab_funding_multiplier: world.policies.rehab_funding_multiplier,
    }
}

/// The block active at `minute_of_day` in a resolved schedule, if any
/// (wrapper around the engine's public `current_block`).
#[must_use]
pub fn current_schedule_block(
    schedule: &alc_prisoner::ResolvedSchedule,
    minute_of_day: u16,
) -> Option<&ResolvedBlock> {
    current_block(schedule, minute_of_day)
}

/// Activity → facility kind mapping (mirrors `alc_world`'s private
/// `facility_for_activity`, used by the Prison Map screen).
#[must_use]
pub fn activity_to_facility_kind(activity: Activity) -> FacilityKind {
    use Activity as A;
    match activity {
        A::Breakfast | A::Lunch | A::Dinner => FacilityKind::Kitchen,
        A::Work | A::Program => FacilityKind::Workshop,
        A::Education => FacilityKind::Education,
        A::Yard => FacilityKind::Yard,
        A::Medical | A::SickCall => FacilityKind::Medical,
        A::Visits => FacilityKind::Visitor,
        A::Recreation | A::Showers | A::Religious | A::FreeTime => FacilityKind::Recreation,
        A::Wake | A::Lockdown | A::Sleep | A::Count => FacilityKind::CellBlock,
    }
}

// ---------------------------------------------------------------------------
// Stats dashboard — `sumo::Stats` fields flattened into label/value rows
// ---------------------------------------------------------------------------

/// Flatten `sumo::Stats` into label/value pairs for the Stats Dashboard panel
/// (`since` included as the last row).
#[must_use]
pub fn stats_dashboard_rows(stats: &sumo::Stats) -> Vec<(&'static str, String)> {
    vec![
        ("Admissions", stats.admissions.to_string()),
        ("Releases", stats.releases.to_string()),
        ("Conflicts", stats.conflicts.to_string()),
        ("Medical incidents", stats.medical.to_string()),
        ("Security incidents", stats.security_incidents.to_string()),
        ("Parole hearings", stats.parole_hearings.to_string()),
        ("Avg population", format!("{:.2}", stats.avg_population)),
        ("Current population", stats.current_population.to_string()),
        ("Capacity", stats.capacity.to_string()),
        ("Gangs formed", stats.gangs_formed.to_string()),
        ("Gang recruits", stats.gang_recruits.to_string()),
        ("Max population", stats.max_population.to_string()),
        ("Population (alias)", stats.population.to_string()),
        ("Since", stats.since.to_string()),
    ]
}

// ---------------------------------------------------------------------------
// Leaderboard — `sumo::Sim::leader_board` flattened into rank rows
// ---------------------------------------------------------------------------

/// Number and cap `sumo::Sim::leader_board(n)` output: `(rank, name, reputation)`.
#[must_use]
pub fn leaderboard_table<'a>(lb: &'a [(String, f32)], n: usize) -> Vec<(u32, &'a str, f32)> {
    let mut sorted: Vec<&'a (String, f32)> = lb.iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted
        .into_iter()
        .take(n)
        .enumerate()
        .map(|(i, (name, reputation))| (i as u32 + 1, name.as_str(), *reputation))
        .collect()
}

// ---------------------------------------------------------------------------
// Shared row/detail shaping for screens
// ---------------------------------------------------------------------------

/// Format a 0..1 value as a percentage with one decimal.
#[must_use]
pub fn pct(v: f32) -> String {
    format!("{:.1}%", (v * 100.0).clamp(0.0, 100.0))
}

/// Selected prisonner-history-relevant strings reversed for compact display:
/// the most recent `take` history entries, newest first.
#[must_use]
pub fn history_newest_first(history: &[String], take: usize) -> Vec<&str> {
    history
        .iter()
        .rev()
        .take(take)
        .map(String::as_str)
        .collect()
}

/// Needs as labelled (label, value) pairs for the prisoner detail panel.
#[must_use]
pub fn need_rows(needs: &Needs) -> Vec<(&'static str, f32)> {
    vec![
        ("Hunger", needs.hunger),
        ("Thirst", needs.thirst),
        ("Sleep", needs.sleep),
        ("Fatigue", needs.fatigue),
        ("Hygiene", needs.hygiene),
        ("Medical", needs.medical_need),
        ("Exercise", needs.exercise),
        ("Temperature", needs.temperature_comfort),
        ("Bathroom", needs.bathroom),
        ("Safety", needs.safety),
        ("Companionship", needs.companionship),
        ("Respect", needs.respect),
        ("Freedom", needs.freedom),
        ("Autonomy", needs.autonomy),
        ("Purpose", needs.purpose),
        ("Stimulation", needs.stimulation),
        ("Stability", needs.stability),
        ("Privacy", needs.privacy),
        ("Belonging", needs.belonging),
        ("Friendship", needs.friendship),
        ("Family contact", needs.family_contact),
        ("Group acceptance", needs.group_acceptance),
        ("Status", needs.status),
        ("Protection", needs.protection),
        ("Recognition", needs.recognition),
    ]
}

// ---------------------------------------------------------------------------
// Shared read-only accessors (re-export alc_ui view helpers)
// ---------------------------------------------------------------------------

#[must_use]
pub fn short_truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[must_use]
pub fn short_class(c: Classification) -> &'static str {
    match c {
        Classification::Minimum => "MIN",
        Classification::Low => "LOW",
        Classification::Medium => "MED",
        Classification::High => "HIGH",
        Classification::Maximum => "MAX",
    }
}

#[must_use]
pub fn name_map(world: &World) -> HashMap<PrisonerId, String> {
    world
        .prisoners
        .iter()
        .map(|p| (p.id, p.name.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Point-in-time snapshot consumed by every observation view
// ---------------------------------------------------------------------------

/// Everything an observation view needs, captured once per engine revision.
///
/// The App caches this struct and only rebuilds it when a tick or a validated
/// control changes the world, so panning/zooming the map or typing in a search
/// box never re-clones the prison population.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub speed: alc_core::TimeSpeed,
    pub observation: sumo::Observation,
    pub stats: Stats,
    pub prisoners: Vec<PrisonerSummary>,
    pub gangs: Vec<GangSummary>,
    pub guards: Vec<GuardSummary>,
    pub warden: WardenSummary,
    /// Runtime-only AI orchestration policy and telemetry.
    pub ai_runtime: sumo::AiRuntimeSnapshot,
    pub economy: EconomySummary,
    pub social_edges: Vec<SocialEdge>,
    pub social_roles: Vec<SocialRoleSummary>,
    pub facilities: Vec<FacilityView>,
    pub construction: Vec<ConstructionSummary>,
    pub events: Vec<sumo::WorldEvent>,
    pub incidents: Vec<IncidentRecord>,
    pub decision_audit: Vec<DecisionAuditRecord>,
    pub policy: PolicySummary,
    pub budget: BudgetSnapshot,
    pub rehab: RehabOverview,
    pub rehab_catalog: Vec<RehabProgramInfo>,
    pub rehab_rows: Vec<sumo::RehabEnrollmentSummary>,
    pub contraband: Vec<sumo::ContrabandCaseSummary>,
    pub post_release: Vec<PostReleaseSummary>,
    pub map: MapFrame,
    pub violations: Vec<String>,
    pub parole_due: Vec<PrisonerId>,
    /// Details for the currently selected prisoner, when one is selected.
    pub selected_agent: Option<AgentObservation>,
    pub selected_schedule: Option<ScheduleTimeline>,
    /// Typed parole status for the selected prisoner. Kept typed so control
    /// gating cannot silently break when engine variants are renamed.
    pub selected_parole_status: Option<sumo::ParoleStatus>,
    /// Conditions and decision record for the selected prisoner (spec 006).
    pub selected_parole_conditions: Option<sumo::ParoleConditions>,
    pub selected_parole_reason: Option<String>,
    pub selected_parole_released: bool,
}

/// Capture a read-only snapshot. `selected_prisoner` enriches the snapshot
/// with that agent's individual observation, schedule and parole status.
#[must_use]
pub fn capture(sim: &sumo::Sim, selected_prisoner: Option<PrisonerId>) -> Snapshot {
    let world = sim.world();
    let map = sim.map_frame();

    let mut facilities: Vec<FacilityView> = world
        .facilities
        .iter()
        .map(|f| FacilityView {
            id: f.id.0,
            kind: f.kind,
            wing: f.wing,
            capacity: f.capacity,
            condition: f.condition,
            staff_required: f.staff_required,
            x: map.facility_positions.get(&f.id.0).map_or(100.0, |p| p.0),
            y: map.facility_positions.get(&f.id.0).map_or(100.0, |p| p.1),
            locked: map.locked_nodes.contains(&f.id.0),
            prisoners: Vec::new(),
            guards: Vec::new(),
        })
        .collect();
    facilities.sort_by_key(|f| f.id);
    for pawn in map.prisoners.iter().chain(map.guards.iter()) {
        if let Some(facility) = facilities.iter_mut().find(|f| f.id == pawn.node_id) {
            if pawn.is_prisoner {
                facility.prisoners.push(pawn.id);
            } else {
                facility.guards.push(pawn.id);
            }
        }
    }

    let events: Vec<sumo::WorldEvent> = sim.events().last_reversed(300);
    let selected_agent = selected_prisoner.and_then(|id| sim.agent_observation(id));
    let selected_schedule = selected_prisoner.and_then(|id| sim.schedule_timeline(id));
    let selected_parole_status = selected_prisoner.and_then(|id| sim.parole_status(id));
    let selected_parole_conditions = selected_prisoner
        .and_then(|id| sim.parole_conditions(id))
        .cloned();
    let selected_parole_reason = selected_prisoner
        .and_then(|id| sim.parole_decision_reason(id))
        .map(str::to_string);
    let selected_parole_released = selected_prisoner
        .and_then(|id| sim.parole_released_at(id))
        .is_some();

    Snapshot {
        speed: sim.speed(),
        observation: sim.observation(),
        stats: sim.stats_since(sumo::SimTime::EPOCH),
        prisoners: sim.prisoners(),
        gangs: sim.gangs(),
        guards: sim.guards(),
        warden: sim.warden(),
        ai_runtime: sim.ai_runtime_snapshot(),
        economy: sim.economy(),
        social_edges: sim.social_edges(),
        social_roles: sim.social_roles(),
        facilities,
        construction: sim.construction_queue(),
        events,
        incidents: sim.incident_records(),
        decision_audit: sim.decision_audit_log().to_vec(),
        policy: sim.policy_summary(),
        budget: sim.budget_snapshot(),
        rehab: sim.rehab_overview(),
        rehab_catalog: sim.rehab_program_catalog(),
        rehab_rows: sim.rehab_summaries(),
        contraband: sim.contraband_summaries(),
        post_release: sim.post_release_records(),
        map,
        violations: sim.check_invariants(),
        parole_due: sim.parole_due(),
        selected_agent,
        selected_schedule,
        selected_parole_status,
        selected_parole_conditions,
        selected_parole_reason,
        selected_parole_released,
    }
}

/// One facility node on the map, enriched with occupancy and roster ids.
#[derive(Debug, Clone)]
pub struct FacilityView {
    pub id: u64,
    pub kind: FacilityKind,
    pub wing: u32,
    pub capacity: u32,
    pub condition: f32,
    pub staff_required: u32,
    /// World-space X from the engine `MapFrame`.
    pub x: f32,
    /// World-space Y from the engine `MapFrame`.
    pub y: f32,
    pub locked: bool,
    pub prisoners: Vec<u64>,
    pub guards: Vec<u64>,
}

impl FacilityView {
    /// Occupancy as a 0..1 fraction of capacity.
    #[must_use]
    pub fn utilisation(&self) -> f32 {
        if self.capacity == 0 {
            0.0
        } else {
            (self.prisoners.len() as f32 / self.capacity as f32).min(1.0)
        }
    }

    /// Human-readable facility label for map nodes and rosters.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} #{} (W{})", self.kind.display(), self.id, self.wing)
    }
}

// ---------------------------------------------------------------------------
// Map projection & pan/zoom math (pure, so it can be unit tested)
// ---------------------------------------------------------------------------

/// Build a shared camera from the legacy `(zoom, pan)` pair.
fn camera(zoom: f32, pan: egui::Vec2) -> sumo::MapCamera {
    sumo::MapCamera {
        zoom,
        pan: (pan.x, pan.y),
    }
}

/// Project a world-space point into canvas-local screen coordinates.
/// Delegates to the shared engine camera so GUI and TUI projections agree.
#[must_use]
pub fn world_to_screen(world: (f32, f32), zoom: f32, pan: egui::Vec2) -> egui::Pos2 {
    let (x, y) = camera(zoom, pan).project(world);
    egui::pos2(x, y)
}

/// Inverse of [`world_to_screen`].
#[must_use]
pub fn screen_to_world(screen: egui::Pos2, zoom: f32, pan: egui::Vec2) -> (f32, f32) {
    camera(zoom, pan).unproject((screen.x, screen.y))
}

/// Zoom by `factor` while keeping the world point under `anchor` fixed.
/// Returns the new `(zoom, pan)` pair.
#[must_use]
pub fn zoom_at(
    anchor: egui::Pos2,
    zoom: f32,
    pan: egui::Vec2,
    factor: f32,
    min: f32,
    max: f32,
) -> (f32, egui::Vec2) {
    let camera = camera(zoom, pan).zoom_at((anchor.x, anchor.y), factor, min, max);
    (camera.zoom, egui::vec2(camera.pan.0, camera.pan.1))
}

/// World-space position of a pawn, interpolating along its current nav edge.
#[must_use]
pub fn pawn_world_pos(pawn: &PawnPosition, positions: &HashMap<u64, (f32, f32)>) -> (f32, f32) {
    if let (Some(progress), Some(target)) = (pawn.edge_progress, pawn.target_node) {
        let start = positions
            .get(&pawn.node_id)
            .copied()
            .unwrap_or((100.0, 100.0));
        let end = positions.get(&target).copied().unwrap_or(start);
        let t = progress.clamp(0.0, 1.0);
        (
            start.0 + (end.0 - start.0) * t,
            start.1 + (end.1 - start.1) * t,
        )
    } else {
        positions
            .get(&pawn.node_id)
            .copied()
            .unwrap_or((100.0, 100.0))
    }
}

/// Best-effort extraction of a facility/node id from an incident location.
///
/// Incident locations are free-form (e.g. `"facility 3"`), so this returns
/// `None` rather than guessing when no id can be parsed.
#[must_use]
pub fn incident_node(location: &str) -> Option<u64> {
    let lower = location.to_ascii_lowercase();
    for marker in ["facility", "node", "wing", "#"] {
        if let Some(idx) = lower.find(marker) {
            let tail = &location[idx + marker.len()..];
            let digits: String = tail
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(id) = digits.parse::<u64>() {
                return Some(id);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Audit detail — pure row shaping shared by panels and tests
// ---------------------------------------------------------------------------

/// Stable identity for a decision audit row. Free-form fields are
/// length-prefixed so field boundaries cannot be forged and trimming older
/// rows cannot make a key point at a different decision.
#[must_use]
pub fn decision_key(record: &DecisionAuditRecord) -> String {
    use std::fmt::Write;

    fn push(key: &mut String, value: &str) {
        let _ = write!(key, "{}:{}", value.len(), value);
    }

    // `SimTime::to_string` is minute-resolution; `secs` distinguishes audit
    // rows written within the same displayed minute.
    let time = record.time.secs.to_string();
    let outcome = format!("{:?}", record.outcome);
    let mut key = String::with_capacity(128);
    push(&mut key, &time);
    push(&mut key, &outcome);
    for field in [
        &record.actor,
        &record.source,
        &record.action,
        &record.summary,
        &record.validation,
        &record.reason,
        &record.rationale,
        &record.repair_reason,
    ] {
        push(&mut key, field);
    }
    key
}

/// Validation filter match. The engine emits statuses such as
/// `rejected: bad target`, so exact equality alone misses real audit rows.
#[must_use]
pub fn validation_matches(validation: &str, filter: &str) -> bool {
    let value = validation.to_ascii_lowercase();
    let wanted = filter.to_ascii_lowercase();
    value == wanted || value.starts_with(&format!("{wanted}:"))
}

/// Labelled rows for the incident detail panel: cause, actors, validation of
/// the staff response, and outcome.
#[must_use]
pub fn incident_detail_rows(
    record: &IncidentRecord,
    names: &HashMap<u64, String>,
) -> Vec<(String, String)> {
    let name = |id: u64| names.get(&id).cloned().unwrap_or_else(|| format!("#{id}"));
    let actors: Vec<String> = record
        .involved_prisoners
        .iter()
        .copied()
        .map(name)
        .collect();
    let witnesses: Vec<String> = record.witnesses.iter().copied().map(name).collect();
    let cause = record
        .causal_chain
        .first()
        .map(|link| {
            format!(
                "{} #{}: {}",
                if link.is_incident {
                    "incident"
                } else {
                    "event"
                },
                link.source_id,
                link.description
            )
        })
        .or_else(|| record.cause.map(|id| format!("incident #{id}")))
        .unwrap_or_else(|| "none recorded".to_string());
    let ok = record.response_actions.iter().filter(|a| a.success).count();
    let failed = record.response_actions.len() - ok;
    let validation = if record.response_actions.is_empty() {
        "no staff response recorded".to_string()
    } else {
        format!(
            "{} response(s): {} ok / {} failed",
            record.response_actions.len(),
            ok,
            failed
        )
    };
    vec![
        (
            "ID / kind".to_string(),
            format!("#{} · {}", record.id, record.kind),
        ),
        ("Time".to_string(), record.at.to_string()),
        (
            "Severity".to_string(),
            format!("{:.0}%", record.severity * 100.0),
        ),
        ("Location".to_string(), record.location.clone()),
        ("Cause".to_string(), cause),
        (
            "Actors".to_string(),
            if actors.is_empty() {
                "-".into()
            } else {
                actors.join(", ")
            },
        ),
        (
            "Witnesses".to_string(),
            if witnesses.is_empty() {
                "-".into()
            } else {
                witnesses.join(", ")
            },
        ),
        ("Validation".to_string(), validation),
        (
            "Outcome".to_string(),
            format!(
                "{} · {}",
                if record.resolved { "resolved" } else { "open" },
                record.disposition
            ),
        ),
    ]
}

/// Labelled rows for the decision detail panel: cause, actors, validation and
/// outcome, plus the supporting rationale and summary.
#[must_use]
pub fn decision_detail_rows(record: &DecisionAuditRecord) -> Vec<(String, String)> {
    let cause = if record.reason.is_empty() {
        if record.rationale.is_empty() {
            "none recorded".to_string()
        } else {
            record.rationale.clone()
        }
    } else {
        record.reason.clone()
    };
    let mut rows = vec![
        ("Time".to_string(), record.time.to_string()),
        (
            "Actors".to_string(),
            format!("{} · {}", record.actor, record.source),
        ),
        ("Cause".to_string(), cause),
        ("Action".to_string(), record.action.clone()),
        ("Validation".to_string(), record.validation.clone()),
        ("Outcome".to_string(), format!("{:?}", record.outcome)),
        ("Summary".to_string(), record.summary.clone()),
    ];
    if !record.rationale.is_empty() {
        rows.push(("Rationale".to_string(), record.rationale.clone()));
    }
    if !record.repair_reason.is_empty() {
        rows.push(("Repair reason".to_string(), record.repair_reason.clone()));
    }
    rows
}

// ---------------------------------------------------------------------------
// Tests — pure smoke tests over synthetic inputs
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::{SimTime, TimeSpeed};
    use alc_world::Policies;

    fn sim(seed: u64) -> sumo::Sim {
        let mut s = sumo::Sim::new(seed);
        s.set_speed(TimeSpeed::Extreme);
        s
    }

    #[test]
    fn incident_aggregate_groups_events_by_typed_bucket() {
        let events = vec![
            WorldEvent::new(1, SimTime::EPOCH, EventKind::Fight, "f"),
            WorldEvent::new(2, SimTime::EPOCH, EventKind::Fight, "f2"),
            WorldEvent::new(3, SimTime::EPOCH, EventKind::Argument, "a"),
            WorldEvent::new(4, SimTime::EPOCH, EventKind::Threat, "t"),
            WorldEvent::new(5, SimTime::EPOCH, EventKind::GangClash, "g"),
            WorldEvent::new(6, SimTime::EPOCH, EventKind::EscapeSucceeded, "e"),
            WorldEvent::new(7, SimTime::EPOCH, EventKind::ContrabandFound, "c"),
            WorldEvent::new(8, SimTime::EPOCH, EventKind::StaffMisconduct, "m"),
            WorldEvent::new(9, SimTime::EPOCH, EventKind::Investigation, "i"),
            WorldEvent::new(10, SimTime::EPOCH, EventKind::Rumor, "r"),
            WorldEvent::new(11, SimTime::EPOCH, EventKind::FamilyCorrespondence, "fc"),
        ];
        let rows = incident_report_rows(&events);
        let count_of = |kind: IncidentKind| rows.iter().find(|r| r.kind == kind).map(|r| r.count);

        assert_eq!(count_of(IncidentKind::Fight), Some(2));
        assert_eq!(count_of(IncidentKind::Argument), Some(1));
        assert_eq!(count_of(IncidentKind::Threat), Some(1));
        assert_eq!(count_of(IncidentKind::GangClash), Some(1));
        assert_eq!(count_of(IncidentKind::Escape), Some(1));
        assert_eq!(count_of(IncidentKind::Contraband), Some(1));
        assert_eq!(count_of(IncidentKind::Misconduct), Some(1));
        assert_eq!(count_of(IncidentKind::Security), Some(1));
        // Most frequent first: Fights (2) leads.
        assert_eq!(rows[0].kind, IncidentKind::Fight);
        assert_eq!(rows[0].count, 2);
        assert_eq!(count_of(IncidentKind::Other), Some(2));
    }

    #[test]
    fn live_feed_append_is_capped_newest_last_and_ignores_empty() {
        let mut feed = VecDeque::new();
        let events = vec![
            WorldEvent::new(1, SimTime::EPOCH, EventKind::Admission, "a"),
            WorldEvent::new(2, SimTime::EPOCH, EventKind::Argument, "b"),
        ];
        live_feed_append(&mut feed, &events, LIVE_FEED_CAP);
        assert_eq!(feed.len(), 2);
        assert_eq!(feed.back().map(|e| e.id), Some(2));

        live_feed_append(&mut feed, &[], LIVE_FEED_CAP);
        assert_eq!(feed.len(), 2);

        // Cap: append 2 then 1 with cap 2 — the oldest is evicted.
        let mut feed2 = VecDeque::new();
        live_feed_append(&mut feed2, &events, 2);
        let third = WorldEvent::new(3, SimTime::EPOCH, EventKind::Threat, "t");
        live_feed_append(&mut feed2, std::slice::from_ref(&third), 2);
        assert_eq!(feed2.len(), 2);
        assert_eq!(feed2.front().map(|e| e.id), Some(2));
        assert_eq!(feed2.back().map(|e| e.id), Some(3));
    }

    #[test]
    fn policy_change_actions_target_the_engine_knobs() {
        let cur = Policies::default();
        match policy_change_action(PolicyChangeKind::SecurityUp, &cur) {
            WardenAction::IncreaseSecurity => {}
            other => panic!("expected IncreaseSecurity, got {other:?}"),
        }
        match policy_change_action(PolicyChangeKind::LockdownOn, &cur) {
            WardenAction::ChangePolicy { policy, value } => {
                assert_eq!(policy, "lockdown");
                assert_eq!(value, 1.0);
            }
            other => panic!("expected lockdown ChangePolicy, got {other:?}"),
        }
        match policy_change_action(PolicyChangeKind::LockdownOff, &cur) {
            WardenAction::ChangePolicy { policy, value } => {
                assert_eq!(policy, "lockdown");
                assert_eq!(value, 0.0);
            }
            other => panic!("expected lockdown ChangePolicy, got {other:?}"),
        }
        match policy_change_action(PolicyChangeKind::PrivilegesDown, &cur) {
            WardenAction::ChangePolicy { policy, value } => {
                assert_eq!(policy, "privileges");
                assert!((value as f32 - 0.4).abs() < 0.06, "value={value}");
            }
            other => panic!("expected privileges ChangePolicy, got {other:?}"),
        }
        match policy_change_action(PolicyChangeKind::RehabFundingUp, &cur) {
            WardenAction::ChangePolicy { policy, value } => {
                assert_eq!(policy, "rehab_funding");
                assert!((value as f32 - 1.1).abs() < 0.06, "value={value}");
            }
            other => panic!("expected rehab_funding ChangePolicy, got {other:?}"),
        }
        match policy_change_action(PolicyChangeKind::SecurityUp, &cur) {
            WardenAction::IncreaseSecurity => {}
            other => panic!("SecurityUp must raise security, got {other:?}"),
        }
        for kind in PolicyChangeKind::ALL {
            let _ = policy_change_action(kind, &cur);
        }
    }

    #[test]
    fn daily_schedule_rows_cover_every_prisoner_and_show_live_activity() {
        let s = sim(0xC0FFEE);
        let world = s.world();
        let rows = daily_schedule_rows(world);
        assert_eq!(rows.len(), world.prisoners.len());
        assert!(!rows.is_empty());
        let minute = world.time.minute_of_day() as u16;
        for row in &rows {
            assert!(!row.name.is_empty());
            assert!(!row.resolved.is_empty(), "empty schedule for {}", row.name);
            let current = current_schedule_block(&row.resolved, minute).map(|b| b.block.activity);
            assert_eq!(current, row.current);
        }
    }

    #[test]
    fn guard_roster_rows_cover_every_guard_and_posts_summarize_all() {
        let s = sim(0xB0BA);
        let world = s.world();
        let rows: Vec<GuardRosterRow> = world.guards.iter().map(guard_roster_row).collect();
        assert_eq!(rows.len(), world.guards.len());
        assert!(!rows.is_empty());
        assert!(
            rows.iter()
                .all(|r| r.credibility >= 0.0 && r.credibility <= 1.0)
        );
        assert!(rows.iter().all(|r| r.fatigue >= 0.0 && r.fatigue <= 1.0));
        assert!(rows.iter().all(|r| r.stress >= 0.0 && r.stress <= 1.0));

        let summary = guard_posts_summary(world);
        let total: usize = summary.iter().map(|(_, n)| *n).sum();
        assert_eq!(total, world.guards.len());
        assert!(staff_coverage(world) >= 0.0);
    }

    #[test]
    fn stats_dashboard_exposes_the_sumo_stats_fields() {
        let s = sim(0xDADA);
        let stats = s.stats_since(SimTime::EPOCH);
        let rows = stats_dashboard_rows(&stats);
        assert!(
            rows.len() >= 13,
            "expected >= 13 stat rows, got {}",
            rows.len()
        );
        let labels: Vec<&str> = rows.iter().map(|(l, _)| *l).collect();
        for want in [
            "Admissions",
            "Releases",
            "Conflicts",
            "Medical incidents",
            "Security incidents",
            "Parole hearings",
            "Avg population",
            "Current population",
            "Capacity",
            "Gangs formed",
            "Gang recruits",
            "Max population",
            "Population (alias)",
            "Since",
        ] {
            assert!(labels.contains(&want), "missing stat row {want}");
        }
        let pop_row = rows
            .iter()
            .find(|(l, _)| *l == "Current population")
            .unwrap();
        assert_eq!(pop_row.1, stats.current_population.to_string());
        assert_eq!(pop_row.1.parse::<u32>().unwrap(), s.world().population());
    }

    #[test]
    fn leaderboard_table_ranks_desc_and_caps() {
        let lb = vec![
            ("A".to_string(), 0.1),
            ("B".to_string(), 0.9),
            ("C".to_string(), 0.5),
            ("D".to_string(), 0.7),
        ];
        let rows = leaderboard_table(&lb, 10);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], (1, "B", 0.9));
        assert_eq!(rows[1], (2, "D", 0.7));
        assert_eq!(rows[2], (3, "C", 0.5));
        assert_eq!(rows[3], (4, "A", 0.1));
        let capped = leaderboard_table(&lb, 2);
        assert_eq!(capped.len(), 2);
    }

    #[test]
    fn activity_to_facility_mapping_covers_major_activities() {
        assert_eq!(
            activity_to_facility_kind(Activity::Breakfast),
            FacilityKind::Kitchen
        );
        assert_eq!(
            activity_to_facility_kind(Activity::Work),
            FacilityKind::Workshop
        );
        assert_eq!(
            activity_to_facility_kind(Activity::Yard),
            FacilityKind::Yard
        );
        assert_eq!(
            activity_to_facility_kind(Activity::Sleep),
            FacilityKind::CellBlock
        );
        assert_eq!(
            activity_to_facility_kind(Activity::Medical),
            FacilityKind::Medical
        );
    }

    #[test]
    fn utilisation_helpers_shape_gui_rows() {
        let rows = need_rows(&Needs::default());
        assert!(rows.len() >= 10);
        assert!(rows.iter().all(|(l, v)| !l.is_empty() && v.is_finite()));
        assert_eq!(short_class(Classification::Maximum), "MAX");
        assert_eq!(short_truncate("abcdef", 3), "ab…");
        assert_eq!(pct(0.5), "50.0%");
    }

    #[test]
    fn map_projection_round_trips() {
        let zoom = 1.35;
        let pan = egui::vec2(42.0, -17.0);
        let world = (123.5, 88.25);
        let screen = world_to_screen(world, zoom, pan);
        let back = screen_to_world(screen, zoom, pan);
        assert!((back.0 - world.0).abs() < 1e-4);
        assert!((back.1 - world.1).abs() < 1e-4);
    }

    #[test]
    fn zoom_at_keeps_the_anchor_world_point_fixed() {
        let anchor = egui::pos2(320.0, 240.0);
        let zoom = 1.0;
        let pan = egui::vec2(0.0, 0.0);
        let before = screen_to_world(anchor, zoom, pan);
        let (new_zoom, new_pan) = zoom_at(anchor, zoom, pan, 1.5, 0.25, 4.0);
        let after = screen_to_world(anchor, new_zoom, new_pan);
        assert!((new_zoom - 1.5).abs() < 1e-6);
        assert!((after.0 - before.0).abs() < 1e-4);
        assert!((after.1 - before.1).abs() < 1e-4);
        // Clamping must still keep the anchor fixed.
        let (clamped, _) = zoom_at(anchor, 0.3, pan, 0.1, 0.25, 4.0);
        assert!((clamped - 0.25).abs() < 1e-6);
    }

    #[test]
    fn pawn_position_interpolates_along_a_nav_edge() {
        let mut positions = HashMap::new();
        positions.insert(1, (100.0, 100.0));
        positions.insert(2, (300.0, 100.0));
        let pawn = PawnPosition {
            id: 9,
            name: "P".into(),
            is_prisoner: true,
            node_id: 1,
            edge_progress: Some(0.25),
            target_node: Some(2),
            activity: "walking".into(),
            x: 100.0,
            y: 100.0,
            gang: None,
            classification: None,
            mood: None,
            intention: None,
            threat: None,
            post: None,
        };
        assert_eq!(pawn_world_pos(&pawn, &positions), (150.0, 100.0));
        let mut stationary = pawn.clone();
        stationary.edge_progress = None;
        stationary.target_node = None;
        assert_eq!(pawn_world_pos(&stationary, &positions), (100.0, 100.0));
    }

    #[test]
    fn incident_node_parses_known_location_formats() {
        assert_eq!(incident_node("facility 3"), Some(3));
        assert_eq!(incident_node("Node 12"), Some(12));
        assert_eq!(incident_node("yard #7"), Some(7));
        assert_eq!(incident_node("somewhere near the kitchen"), None);
    }

    #[test]
    fn capture_builds_a_complete_read_only_snapshot() {
        let s = sim(0x51DE);
        let id = s.world().prisoners[0].id;
        let snap = capture(&s, Some(id));
        assert_eq!(snap.prisoners.len(), s.world().prisoners.len());
        assert_eq!(snap.guards.len(), s.world().guards.len());
        assert_eq!(snap.facilities.len(), s.world().facilities.len());
        assert_eq!(snap.social_roles, s.social_roles());
        assert_eq!(snap.map.facility_positions.len(), snap.facilities.len());
        assert!(snap.facilities.iter().all(|f| f.utilisation() <= 1.0));
        assert!(snap.selected_agent.is_some());
        assert!(snap.selected_schedule.is_some());
        assert!(
            snap.violations.is_empty(),
            "engine invariants: {:?}",
            snap.violations
        );
    }

    #[test]
    fn decision_key_is_stable_and_unique() {
        let mut first = DecisionAuditRecord::default();
        first.actor = "guard:1".to_string();
        first.action = "fight".to_string();
        let mut second = first.clone();
        second.time = SimTime { secs: 42 };
        assert_eq!(decision_key(&first), decision_key(&first));
        assert_ne!(decision_key(&first), decision_key(&second));
        assert!(validation_matches("rejected: bad target", "rejected"));
        assert!(validation_matches("Accepted", "accepted"));
        assert!(!validation_matches("repaired", "rejected"));
    }

    #[test]
    fn incident_detail_rows_cover_cause_actors_validation_and_outcome() {
        let mut names = HashMap::new();
        names.insert(7, "Rioter".to_string());
        let record = IncidentRecord {
            id: 12,
            at: SimTime { secs: 60 },
            kind: "Fight".to_string(),
            severity: 0.9,
            location: "Yard".to_string(),
            involved_prisoners: vec![7],
            cause: Some(11),
            consequence: None,
            causal_chain: vec![sumo::incidents::CausalLink::incident(11, "prior argument")],
            witnesses: vec![7],
            response_actions: vec![sumo::incidents::ResponseAction {
                action: sumo::incidents::ResponseActionKind::GuardIntervention,
                responder: sumo::incidents::Responder::Guard(3),
                at: SimTime { secs: 61 },
                description: "separated".to_string(),
                success: true,
            }],
            disposition: "Disciplinary".to_string(),
            resolved: true,
        };
        let rows = incident_detail_rows(&record, &names);
        let lookup = |label: &str| {
            rows.iter()
                .find(|(key, _)| key == label)
                .map(|(_, value)| value.as_str())
                .unwrap_or_else(|| panic!("missing {label} row"))
        };
        assert!(lookup("Cause").contains("prior argument"));
        assert!(lookup("Actors").contains("Rioter"));
        assert!(lookup("Validation").contains("1 ok / 0 failed"));
        assert!(lookup("Outcome").contains("resolved"));
        assert!(lookup("Outcome").contains("Disciplinary"));
    }

    #[test]
    fn decision_detail_rows_cover_cause_actors_validation_and_outcome() {
        let mut record = DecisionAuditRecord::default();
        record.time = SimTime { secs: 90 };
        record.actor = "prisoner:7".to_string();
        record.source = "agent".to_string();
        record.action = "fight".to_string();
        record.summary = "attacked peer".to_string();
        record.validation = "rejected: guard nearby".to_string();
        record.reason = "guard nearby".to_string();
        record.rationale = "status".to_string();
        let record = record;
        let rows = decision_detail_rows(&record);
        let lookup = |label: &str| {
            rows.iter()
                .find(|(key, _)| key == label)
                .map(|(_, value)| value.as_str())
                .unwrap_or_else(|| panic!("missing {label} row"))
        };
        assert_eq!(lookup("Cause"), "guard nearby");
        assert!(lookup("Actors").contains("prisoner:7"));
        assert_eq!(lookup("Validation"), "rejected: guard nearby");
        assert_eq!(lookup("Outcome"), "Proposed");
    }

    #[test]
    fn capture_keeps_parole_status_typed() {
        let mut sim = sim(0xC0FFEE_15);
        let id = sim
            .prisoners()
            .first()
            .map(|p| p.id)
            .expect("fresh world has a prisoner");
        sim.parole_mut()
            .set(id, sumo::ParoleStatus::Scheduled, SimTime::EPOCH);

        let snap = capture(&sim, Some(id));
        assert_eq!(
            snap.selected_parole_status,
            Some(sumo::ParoleStatus::Scheduled)
        );
        assert_eq!(capture(&sim, None).selected_parole_status, None);
    }
}
