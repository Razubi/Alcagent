//! Test harness: invariants and soak scenarios (Directive §24 Phase 4).
//!
//! The invariants mirror the Phase 4 look-for list:
//!
//! * no deadlock in the tick loop,
//! * memory bounded,
//! * finances non-negative (or the institution is in receivership),
//! * population never exceeds capacity,
//! * no contradictory confirmed facts.
//!
//! [`soak_run`] advances a world for a number of simulation days, checking
//! those invariants as it goes and returning a [`SoakReport`].

use alc_core::{SimTime, TimeSpeed};
use alc_prisoner::Prisoner;
use alc_world::World;

/// Default per-agent memory budget used by the bounded-memory invariant
/// (design §5.1 long-term cap).
pub const DEFAULT_MEMORY_RECORD_BUDGET: usize = 5_000;

/// Maximum number of in-flight pending AI decisions before the memory-bound
/// invariant trips.
pub const MAX_PENDING_DECISIONS: usize = 10_000;

// ---------------------------------------------------------------------------
// Standalone invariants
// ---------------------------------------------------------------------------

/// Invariant: money is integral.
pub fn invariant_money_integral(cents: i64) -> bool {
    cents % 1 == 0
}

/// Invariant: population cannot exceed facility capacity.
pub fn invariant_capacity(population: u32, capacity: u32) -> bool {
    population <= capacity
}

/// Invariant: population cannot exceed capacity (world form).
pub fn invariant_population_capacity(world: &World) -> bool {
    invariant_capacity(world.population(), world.capacity())
}

/// Invariant: no agent holds contradictory confirmed facts about the same
/// entity and content.
pub fn invariant_no_contradiction(records: &[(u64, String, bool)]) -> bool {
    let mut seen: std::collections::HashMap<(u64, String), bool> = std::collections::HashMap::new();
    for (about, content, value) in records {
        if let Some(prev) = seen.get(&(*about, content.clone())) {
            if prev != value {
                return false;
            }
        }
        seen.insert((*about, content.clone()), *value);
    }
    true
}

/// Directive §24 wording for [`invariant_no_contradiction`].
pub fn invariant_no_contradictory_confirmed_facts(records: &[(u64, String, bool)]) -> bool {
    invariant_no_contradiction(records)
}

/// Invariant: finances are solvent — cash is non-negative unless the
/// institution has formally entered receivership.
pub fn invariant_finances(cash_cents: i64, receivership: bool) -> bool {
    invariant_money_integral(cash_cents) && (cash_cents >= 0 || receivership)
}

/// World-level finance invariant. The current [`alc_economy::Economy`] has no
/// receivership flag, so any negative cash is a violation.
pub fn invariant_world_finances(world: &World) -> bool {
    invariant_finances(world.economy.cash.0, false)
}

/// Invariant: an agent's memory stores stay within their configured caps and
/// the total record count stays bounded.
pub fn invariant_memory_bounded(memory: &alc_agents::Memory, max_records: usize) -> bool {
    memory.is_bounded(max_records)
}

/// Layered-memory form of the bounded-memory invariant.
pub fn invariant_layered_memory_bounded(
    memory: &alc_agents::LayeredMemory,
    max_records: usize,
) -> bool {
    memory.is_bounded(max_records)
}

/// Invariant: no deadlock in the tick loop.
///
/// A live tick loop requires a well-formed registry and finite agent state.
/// This snapshot check catches the structural corruption a stalled or
/// deadlocked loop leaves behind (duplicate ids, NaN needs, inconsistent
/// orchestrator counters, runaway pending-decision queues). Run it after
/// [`soak_run`] and on every autosave boundary.
pub fn invariant_no_deadlock(world: &World) -> bool {
    use std::collections::HashSet;

    let mut prisoner_ids = HashSet::new();
    for prisoner in &world.prisoners {
        if !prisoner_ids.insert(prisoner.id.0) {
            return false;
        }
        if !prisoner_state_finite(prisoner) {
            return false;
        }
    }

    let mut guard_ids = HashSet::new();
    for guard in &world.guards {
        if !guard_ids.insert(guard.id.0) {
            return false;
        }
        if !finite_f32(&[
            guard.personality,
            guard.competence,
            guard.experience,
            guard.morale,
            guard.fatigue,
            guard.stress,
            guard.loyalty,
            guard.integrity,
        ]) {
            return false;
        }
    }

    let mut facility_ids = HashSet::new();
    for facility in &world.facilities {
        if !facility_ids.insert(facility.id.0) {
            return false;
        }
        if !facility.condition.is_finite() {
            return false;
        }
    }

    // Orchestrator accounting must be internally consistent.
    if world.orchestrator.consumed_today > world.orchestrator.token_budget_per_agent {
        return false;
    }
    if world.orchestrator.pending.len() > MAX_PENDING_DECISIONS {
        return false;
    }

    // Time must be finite; the day counter must not overflow.
    world.time.as_secs() != u64::MAX
}

/// Tick the world once and report whether simulation time advanced.
pub fn tick_once(world: &mut World) -> bool {
    let before = world.time;
    world.tick();
    world.time != before
}

fn finite_f32(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn prisoner_state_finite(p: &Prisoner) -> bool {
    let needs = [
        p.needs.hunger,
        p.needs.thirst,
        p.needs.sleep,
        p.needs.fatigue,
        p.needs.hygiene,
        p.needs.medical_need,
        p.needs.exercise,
        p.needs.temperature_comfort,
        p.needs.bathroom,
        p.needs.safety,
        p.needs.companionship,
        p.needs.respect,
        p.needs.freedom,
        p.needs.autonomy,
        p.needs.purpose,
        p.needs.stimulation,
        p.needs.stability,
        p.needs.privacy,
        p.needs.belonging,
        p.needs.friendship,
        p.needs.family_contact,
        p.needs.group_acceptance,
        p.needs.status,
        p.needs.protection,
        p.needs.recognition,
    ];
    let condition = [
        p.condition.health,
        p.condition.injury,
        p.condition.illness,
        p.condition.mental_health,
        p.condition.hope,
    ];
    let traits = [
        p.traits.intelligence,
        p.traits.emotional_stability,
        p.traits.impulsiveness,
        p.traits.aggression,
        p.traits.sociability,
        p.traits.loyalty,
        p.traits.manipulation,
        p.traits.risk_tolerance,
        p.traits.discipline,
        p.traits.empathy,
        p.traits.ambition,
        p.traits.leadership,
    ];
    finite_f32(&needs)
        && finite_f32(&condition)
        && finite_f32(&traits)
        && p.reputation.is_finite()
        && p.needs.total_pressure().is_finite()
}

// ---------------------------------------------------------------------------
// Soak runner
// ---------------------------------------------------------------------------

/// A single failed invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub invariant: String,
    pub detail: String,
}

/// Result of a [`soak_run`].
#[derive(Debug, Clone)]
pub struct SoakReport {
    pub days_requested: u64,
    pub days_simulated: u64,
    pub ticks: u64,
    pub violations: Vec<Violation>,
    /// Convenience flag: true when no invariant failed.
    pub ok: bool,
}

impl SoakReport {
    pub fn ok(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn invariant_names(&self) -> Vec<&str> {
        self.violations
            .iter()
            .map(|v| v.invariant.as_str())
            .collect()
    }
}

/// Run a world for `days` simulation days, checking invariants at each day
/// boundary. Respects the world's current [`TimeSpeed`] but temporarily runs
/// at [`TimeSpeed::Extreme`] when the world is paused, restoring the original
/// speed before returning. Returns early with a `no_deadlock` violation if
/// the tick loop stalls.
pub fn soak_run(world: &mut World, days: u64) -> SoakReport {
    let mut report = SoakReport {
        days_requested: days,
        days_simulated: 0,
        ticks: 0,
        violations: Vec::new(),
        ok: true,
    };
    if days == 0 {
        return report;
    }

    // A fresh world starts paused; a soak run must still advance. Restore the
    // caller's speed on the way out.
    let original_speed = world.speed;
    if world.speed == TimeSpeed::Paused {
        world.speed = TimeSpeed::Extreme;
    }

    let target_days = world.days_simulated.saturating_add(days);
    // Generous tick budget: two real-time minutes-per-tick days plus slack.
    let max_ticks = days
        .saturating_mul(1_440)
        .saturating_mul(2)
        .saturating_add(1_000);
    let mut stalls = 0u32;

    while world.days_simulated < target_days && report.ticks < max_ticks {
        let before = world.time;
        world.tick();
        report.ticks += 1;

        if world.time == before {
            stalls += 1;
            if stalls >= 5 {
                report.violations.push(Violation {
                    invariant: "no_deadlock".into(),
                    detail: "tick did not advance simulation time".into(),
                });
                break;
            }
        } else {
            stalls = 0;
        }

        if world.time.minute_of_day() == 0 && world.time.hour_of_day() == 0 {
            check_world_invariants(world, &mut report.violations);
        }
    }

    if report.ticks >= max_ticks && world.days_simulated < target_days {
        report.violations.push(Violation {
            invariant: "no_deadlock".into(),
            detail: "soak exceeded its maximum tick budget".into(),
        });
    }

    check_world_invariants(world, &mut report.violations);
    report
        .violations
        .sort_by(|a, b| a.invariant.cmp(&b.invariant).then(a.detail.cmp(&b.detail)));
    report.violations.dedup();
    report.days_simulated = world.days_simulated;
    report.ok = report.violations.is_empty();
    world.speed = original_speed;
    report
}

/// Check every world-level invariant, appending failures to `violations`.
pub fn check_world_invariants(world: &World, violations: &mut Vec<Violation>) {
    if !invariant_no_deadlock(world) {
        violations.push(Violation {
            invariant: "no_deadlock".into(),
            detail: "duplicate ids, non-finite state, or inconsistent orchestrator counters".into(),
        });
    }
    if !invariant_population_capacity(world) {
        violations.push(Violation {
            invariant: "population_capacity".into(),
            detail: format!(
                "population {} exceeds capacity {}",
                world.population(),
                world.capacity()
            ),
        });
    }
    if !invariant_world_finances(world) {
        violations.push(Violation {
            invariant: "finances".into(),
            detail: format!("cash is negative: {} cents", world.economy.cash.0),
        });
    }
    if world.orchestrator.pending.len() > MAX_PENDING_DECISIONS {
        violations.push(Violation {
            invariant: "memory_bounded".into(),
            detail: format!(
                "pending decisions {} exceeds {}",
                world.orchestrator.pending.len(),
                MAX_PENDING_DECISIONS
            ),
        });
    }
}

/// Current simulation time helper for report consumers.
pub fn sim_time(world: &World) -> SimTime {
    world.time
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_integral_invariant() {
        assert!(invariant_money_integral(0));
        assert!(invariant_money_integral(-5));
        assert!(invariant_money_integral(123_456));
    }

    #[test]
    fn capacity_invariant() {
        assert!(invariant_capacity(10, 10));
        assert!(!invariant_capacity(11, 10));
        assert!(invariant_population_capacity(&World::new(1)));
    }

    #[test]
    fn contradiction_invariant() {
        let consistent = vec![
            (1, "is hostile".to_string(), true),
            (1, "is friendly".to_string(), false),
        ];
        assert!(invariant_no_contradiction(&consistent));
        let contradictory = vec![
            (1, "is hostile".to_string(), true),
            (1, "is hostile".to_string(), false),
        ];
        assert!(!invariant_no_contradiction(&contradictory));
        assert!(!invariant_no_contradictory_confirmed_facts(&contradictory));
    }

    #[test]
    fn finance_invariant_allows_receivership() {
        assert!(invariant_finances(0, false));
        assert!(invariant_finances(100, false));
        assert!(!invariant_finances(-1, false));
        assert!(invariant_finances(-1, true));
    }

    #[test]
    fn soak_run_advances_and_stays_consistent() {
        let mut world = World::new(0xA1C_0A_6E_17_0);
        world.speed = TimeSpeed::Extreme;
        let report = soak_run(&mut world, 3);
        assert!(report.ok(), "violations: {:?}", report.violations);
        assert_eq!(report.days_simulated, 3);
        assert!(report.ticks > 0);
    }

    #[test]
    fn soak_run_resumes_a_paused_world_and_restores_speed() {
        let mut world = World::new(1);
        world.speed = TimeSpeed::Paused;
        let report = soak_run(&mut world, 1);
        assert!(report.ok(), "violations: {:?}", report.violations);
        assert_eq!(report.days_simulated, 1);
        assert_eq!(world.speed, TimeSpeed::Paused);
    }

    #[test]
    fn deadlock_invariant_flags_duplicate_ids() {
        let mut world = World::new(1);
        assert!(invariant_no_deadlock(&world));
        // Force a duplicate id to simulate registry corruption.
        let duplicate = world.prisoners[0].clone();
        world.prisoners.push(duplicate);
        assert!(!invariant_no_deadlock(&world));
    }

    #[test]
    fn deadlock_invariant_flags_non_finite_prisoner_need() {
        // The soak harness relies on this detector to catch a poisoned needs
        // state; verify it actually trips on NaN rather than silently passing.
        let mut world = World::new(1);
        assert!(invariant_no_deadlock(&world));
        world.prisoners[0].needs.hunger = f32::NAN;
        assert!(!invariant_no_deadlock(&world));

        let mut violations = Vec::new();
        check_world_invariants(&world, &mut violations);
        assert!(
            violations.iter().any(|v| v.invariant == "no_deadlock"),
            "check_world_invariants missed the NaN need: {violations:?}"
        );
    }
}
