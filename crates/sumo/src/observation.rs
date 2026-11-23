//! Read-only observation snapshots for observers and the UI (Directive §20).
//!
//! `Observation` is a plain-data snapshot of the world at one instant. It is
//! cheap to clone and never borrows the world, so UI code can render without
//! holding a lock.

use alc_core::SimTime;
use alc_world::World;
use serde::{Deserialize, Serialize};

/// A point-in-time, read-only snapshot of the prison world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// Simulation time at time of snapshot.
    pub at: SimTime,
    /// Number of days simulated at time of snapshot.
    pub days_simulated: u64,
    /// Current total prisoner population.
    pub population: u32,
    /// Total facility capacity.
    pub capacity: u32,
    /// Number of guards currently on duty.
    pub guards: u32,
    /// Number of facilities in the prison.
    pub facilities: u32,
    /// Cash value in major currency units (thousands).
    pub cash_major: f64,
    /// Overall economic health index (0.0 = collapsed, 1.0 = robust).
    pub economy_health: f64,
    /// Current security level setting.
    pub security_level: f32,
    /// Whether the prison is currently under lockdown.
    pub lockdown: bool,
    /// Name of the current warden.
    pub warden_name: String,
    /// The warden's governing philosophy.
    pub warden_philosophy: String,
    /// Number of active gangs in the prison.
    pub gangs: u32,
    /// Names of the N most "notable" prisoners (highest reputation), for
    /// quick-scan dashboards.
    pub notable_prisoners: Vec<String>,
    /// Last few event summaries (most recent last).
    pub recent_events: Vec<String>,
    /// Total members across all gangs.
    pub gang_members: u32,
    /// Leaderboard (name, reputation), sorted desc, top ~5.
    pub leader_board: Vec<(String, f32)>,
}

impl Observation {
    /// Capture a snapshot of `world`.
    pub fn capture(world: &World) -> Self {
        let mut prisoners = world.prisoners.clone();
        prisoners.sort_by(|a, b| b.reputation.partial_cmp(&a.reputation).unwrap_or(std::cmp::Ordering::Equal));
        let notable_prisoners = prisoners
            .iter()
            .take(5)
            .map(|p| p.name.clone())
            .collect();

        let recent_events: Vec<String> = world
            .events
            .recent()
            .skip(world.events.len().saturating_sub(8))
            .map(|e| e.summary.clone())
            .collect();

        Observation {
            at: world.time,
            days_simulated: world.days_simulated,
            population: world.population(),
            capacity: world.capacity(),
            guards: world.guards.len() as u32,
            facilities: world.facilities.len() as u32,
            cash_major: world.economy.cash.as_major(),
            economy_health: world.economy.health(),
            security_level: world.policies.security_level,
            lockdown: world.policies.lockdown,
            warden_name: world.warden.name.clone(),
            warden_philosophy: world.warden.philosophy.clone(),
            gangs: world.gangs.len() as u32,
            notable_prisoners,
            recent_events,
            gang_members: world.gangs.iter().map(|g| g.members.len()).sum::<usize>() as u32,
            leader_board: leader_board(world, 5),
        }
    }
}

fn leader_board(world: &World, n: usize) -> Vec<(String, f32)> {
    let mut v: Vec<(String, f32)> = world.prisoners.iter().map(|p| (p.name.clone(), p.reputation)).collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    v.truncate(n);
    v
}

/// Per-agent read-only observation for the individual-prisoner view (§20).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentObservation {
    /// The prisoner's unique identifier.
    pub id: alc_core::PrisonerId,
    /// The prisoner's name.
    pub name: String,
    /// The prisoner's age.
    pub age: u32,
    /// The prisoner's security classification.
    pub classification: alc_prisoner::Classification,
    /// The prisoner's current need states.
    pub needs: alc_prisoner::Needs,
    /// The prisoner's physical/mental condition.
    pub condition: alc_prisoner::Condition,
    /// The prisoner's current goals.
    pub goals: Vec<alc_prisoner::Goal>,
    /// The prisoner's gang affiliation, if any.
    pub gang_affiliation: Option<u64>,
    /// Modifier affecting the prisoner's aggression level.
    pub aggression_modifier: f32,
    /// The prisoner's drive for social interaction.
    pub social_drive: f32,
    /// Compact memory summary for the brain-inspector view (§19/§20).
    pub memory_summary: String,
    /// The prisoner's current intention from the world decision map, if any.
    pub current_intention: Option<String>,
}

/// Compact metrics derived from the event log over a window (§17, §20).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    /// The time point from which these stats are computed.
    pub since: SimTime,
    /// Number of prisoner admissions since `since`.
    pub admissions: u32,
    /// Number of prisoner releases since `since`.
    pub releases: u32,
    /// Number of conflicts (arguments, fights, thefts, etc.) since `since`.
    pub conflicts: u32,
    /// Number of medical incidents since `since`.
    pub medical: u32,
    /// Number of security incidents since `since`.
    pub security_incidents: u32,
    /// Number of parole hearings since `since`.
    pub parole_hearings: u32,
    /// Average population over the observation window.
    pub avg_population: f64,
    /// Current population at the time of the stats.
    pub current_population: u32,
    /// Facility capacity.
    pub capacity: u32,
    /// Number of gangs formed since `since`.
    pub gangs_formed: u32,
    /// Number of prisoner-gang recruitments since `since`.
    pub gang_recruits: u32,
    /// High-water mark population over the observation window.
    pub max_population: u32,
    /// Alias of `current_population` (tests use this name).
    pub population: u32,
    /// Number of prisoner deaths since simulation start.
    pub deaths: u32,
    /// Number of prisoner transfers/release movements since simulation start.
    pub transfers: u32,
    /// Number of policy changes emitted since simulation start.
    pub policy_changes: u32,
}

impl Stats {
    /// Build `Stats` from an iterator of events since a given time.
    ///
    /// Counts admissions, releases, conflicts, medical incidents, security
    /// incidents, and parole hearings from the event log. The resulting stats
    /// are a point-in-time summary for the UI and engine reporting.
    pub fn from_events(
        since: SimTime,
        events: impl Iterator<Item = alc_events::WorldEvent>,
        current_population: u32,
        avg_population: f64,
        capacity: u32,
        max_population: u32,
    ) -> Self {
        let mut s = Stats {
            since,
            admissions: 0,
            releases: 0,
            conflicts: 0,
            medical: 0,
            security_incidents: 0,
            parole_hearings: 0,
            deaths: 0,
            transfers: 0,
            policy_changes: 0,
            avg_population,
            current_population,
            population: current_population,
            capacity,
            gangs_formed: 0,
            gang_recruits: 0,
            max_population,
        };
        use alc_events::EventKind::*;
        for e in events {
            match e.kind {
                Admission => s.admissions += 1,
                Release | ParoleGranted => s.releases += 1,
                Argument | Threat | Theft | Fight | GangClash | Protests | PropertyDamage => s.conflicts += 1,
                MedicalEmergency | Illness | Injury => s.medical += 1,
                EscapeAttempt | EscapePrevented | EscapeSucceeded | ContrabandFound | StaffMisconduct | Investigation => {
                    s.security_incidents += 1
                }
                RelationshipShift => {}
                ParoleHearing | ParoleDenied | ParoleBoardDecision => s.parole_hearings += 1,
                GangFormed => s.gangs_formed += 1,
                GangRecruit => s.gang_recruits += 1,
                Death => s.deaths += 1,
                TransferOut => s.transfers += 1,
                PolicyChange => s.policy_changes += 1,
                _ => {}
            }
        }
        s
    }
}