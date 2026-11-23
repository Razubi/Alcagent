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
use alc_core::{PrisonerId, SimTime};
use alc_events::{EventKind, WorldEvent};
use alc_guard::{AuthorityLevel, Guard, GuardRank, PostType};
use alc_prisoner::{
    Activity, Classification, Needs, ResolvedBlock, ScheduleTemplate, SchedulePolicy,
    current_block, resolve_schedule,
};
use alc_warden::WardenAction;
use alc_world::{FacilityKind, World};
use std::collections::{HashMap, VecDeque};

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
        *counts.entry(IncidentKind::from_event_kind(e.kind)).or_insert(0) += 1;
    }
    let mut rows: Vec<IncidentRow> =
        counts.into_iter().map(|(kind, count)| IncidentRow { kind, count }).collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.kind.label().cmp(b.kind.label())));
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
    #[must_use]
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
                if policies.lockdown { 1.0 } else { 0.0 }
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

/// Build the `WardenAction` the GUI emits for a policy change. The engine
/// validates and applies it through `sumo::Sim::apply_warden_action` (§2/§9
/// authority path); for the `lockdown` knob the engine applier has no
/// "lockdown" arm in its `ChangePolicy` match, so the GUI sets the flag on
/// the world after applying the action (see `alc_gui::app`).
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
    posts.into_iter().map(|(p, n)| (format!("{p:?}"), n)).collect()
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
            resolved: resolve_schedule(p, &ScheduleTemplate::default_medium(), &schedule_policy(world)),
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
    schedule: &[ResolvedBlock],
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
    lb.iter()
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
    history.iter().rev().take(take).map(String::as_str).collect()
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
    alc_ui::views::truncate(s, max)
}

#[must_use]
pub fn short_class(c: Classification) -> &'static str {
    alc_ui::views::class_short(c)
}

#[must_use]
pub fn name_map(world: &World) -> HashMap<PrisonerId, String> {
    alc_ui::views::name_map(world)
}

// ---------------------------------------------------------------------------
// Tests — pure smoke tests over synthetic inputs
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::TimeSpeed;
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
            WorldEvent::new(9, SimTime::EPOCH, EventKind::PolicyChange, "p"),
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
        assert!(rows.last().unwrap().kind == IncidentKind::Other);
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

        // Cap: append 2 then 1 with cap 3 — the oldest is evicted.
        let mut feed2 = VecDeque::new();
        live_feed_append(&mut feed2, &events, 3);
        let third = WorldEvent::new(3, SimTime::EPOCH, EventKind::Threat, "t");
        live_feed_append(&mut feed2, std::slice::from_ref(&third), 3);
        assert_eq!(feed2.len(), 3);
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
        let minute = world.time.minute_of_day();
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
        assert!(rows.iter().all(|r| r.credibility >= 0.0 && r.credibility <= 1.0));
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
        assert!(rows.len() >= 13, "expected >= 13 stat rows, got {}", rows.len());
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
        let pop_row = rows.iter().find(|(l, _)| *l == "Current population").unwrap();
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
        assert_eq!(activity_to_facility_kind(Activity::Breakfast), FacilityKind::Kitchen);
        assert_eq!(activity_to_facility_kind(Activity::Work), FacilityKind::Workshop);
        assert_eq!(activity_to_facility_kind(Activity::Yard), FacilityKind::Yard);
        assert_eq!(activity_to_facility_kind(Activity::Sleep), FacilityKind::CellBlock);
        assert_eq!(activity_to_facility_kind(Activity::Medical), FacilityKind::Medical);
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
}
