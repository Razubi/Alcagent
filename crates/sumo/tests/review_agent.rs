//! Integration and review tests added by the Testing/Review Agent.
//!
//! Owns: crates/sumo/tests/*.rs (this file). Does not modify other crates.
//!
//! Required coverage:
//! - full 365-day run with no invariant violations
//! - save/load round-trip preserves population
//! - decision pipeline accepts valid and rejects invalid
//!
//! Additional checks cover long-run stability signals requested in the review:
//! no panics/infinite loops (bounded runtime), population never exceeds
//! capacity, cash not deeply negative, and engine invariants clean.

use alc_agents::{Decision, DecisionAction, ValidationResult};
use alc_core::{PrisonerId, SimTime, TimeSpeed};
use std::time::Instant;
use sumo::Sim;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sumo-review-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn full_365_day_run_no_invariant_violations() {
    let mut sim = Sim::new(0xA1C_0A_6E_17_0);
    sim.set_speed(TimeSpeed::Extreme);

    let start = Instant::now();
    sim.run_until(sim.world().time.add_days(365));
    let elapsed = start.elapsed();

    assert!(
        sim.world().time >= SimTime::EPOCH.add_days(365),
        "simulation did not reach 365 days (time={})",
        sim.world().time
    );
    // Wall-clock budgets vary widely on shared CI hosts. A 365-day run that
    // completes at all proves there is no stall/infinite loop; keep a generous
    // upper bound so a genuinely hung simulation still fails.
    assert!(
        elapsed.as_secs() < 300,
        "365-day run took too long ({elapsed:?}), possible stall/infinite-loop behavior"
    );

    let pop = sim.world().population();
    let cap = sim.world().capacity();
    assert!(pop <= cap, "population {pop} exceeded capacity {cap}");

    let cash = sim.world().economy.cash.0;
    assert!(
        cash > -100_000_00,
        "cash went deeply negative: {cash} cents"
    );

    let errs = sim.check_invariants();
    assert!(errs.is_empty(), "invariants violated after 365 days: {errs:?}");
}

#[test]
fn save_load_round_trip_preserves_population() {
    let dir = temp_dir("save-load-pop");
    let mut sim = Sim::new(42);
    sim.set_speed(TimeSpeed::FastForward);
    sim.run_until(sim.world().time.add_days(10));

    let pop_before = sim.world().population();
    let time_before = sim.world().time;
    let days_before = sim.world().days_simulated;

    sim.save(&dir).expect("save");
    let loaded = Sim::load(&dir).expect("load");

    assert_eq!(loaded.world().population(), pop_before, "population not preserved");
    assert_eq!(loaded.world().time, time_before, "time not preserved");
    assert_eq!(loaded.world().days_simulated, days_before, "days_simulated not preserved");
    assert!(loaded.world().population() <= loaded.world().capacity());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn decision_pipeline_accepts_valid_and_rejects_invalid() {
    let mut sim = Sim::new(99);
    let id = sim.world().prisoners[0].id;
    let valid_target = sim.world().facilities[0].id.0;

    // Valid: move to an existing facility.
    let accepted = sim.deliver_decision(id, Decision::new(DecisionAction::MoveTo { target_facility: valid_target }));
    assert!(
        matches!(accepted.result, ValidationResult::Accepted(_) | ValidationResult::Repaired(_)),
        "expected valid decision to be accepted/repaired, got {:?}",
        accepted.result
    );
    assert!(accepted.applied, "valid decision should be applied");

    // Invalid: unknown target facility.
    let rejected = sim.deliver_decision(id, Decision::new(DecisionAction::MoveTo { target_facility: 999_999 }));
    assert!(
        matches!(rejected.result, ValidationResult::Rejected { .. }),
        "expected invalid decision to be rejected, got {:?}",
        rejected.result
    );
    assert!(!rejected.applied, "invalid decision must not be applied");

    // Invalid: unknown prisoner id.
    let unknown = sim.deliver_decision(PrisonerId(u64::MAX), Decision::new(DecisionAction::Comply));
    assert!(
        matches!(unknown.result, ValidationResult::Rejected { .. }),
        "expected unknown-prisoner decision to be rejected, got {:?}",
        unknown.result
    );
    assert!(!unknown.applied);
}

#[test]
fn run_until_makes_progress_when_not_paused() {
    let mut sim = Sim::new(7);
    sim.set_speed(TimeSpeed::RealTime);
    let target = sim.world().time.add_mins(10);
    let ticks = sim.run_until(target);
    assert!(ticks > 0, "run_until should advance when not paused");
    assert!(sim.world().time >= target, "run_until did not reach target time");
}

#[test]
fn invariants_hold_over_repeated_windows() {
    let mut sim = Sim::new(12345);
    sim.set_speed(TimeSpeed::FastForward);
    for _ in 0..12 {
        sim.run_until(sim.world().time.add_days(30));
        assert!(sim.world().population() <= sim.world().capacity());
        let errs = sim.check_invariants();
        assert!(errs.is_empty(), "invariants violated: {errs:?}");
    }
}

// --- Documented bugs in crates not owned by this agent (ignored) ---
// "If you find a bug in a crate you don't own, write a failing test in
// alc-testing or sumo/tests that documents it, and note it in your summary."

#[test]
fn bug_doc_world_reduce_expenses_spirals_rehab_funding() {
    let mut sim = Sim::new(0xA1C_0A_6E_17_0);
    sim.set_speed(TimeSpeed::Extreme);
    let before = sim.world().policies.rehab_funding_multiplier;
    sim.run_until(sim.world().time.add_days(60));
    let after = sim.world().policies.rehab_funding_multiplier;
    // Healthy behavior: cuts are cooldowned/finance-gated and never a daily
    // death spiral while finances are healthy.
    assert!(
        after >= before - 0.2,
        "rehab_funding_multiplier dropped too aggressively: {before} -> {after} (likely daily ReduceExpenses without cooldown/finance check)"
    );
}

#[test]
fn bug_doc_world_intake_ignores_policy_controls() {
    let mut sim = Sim::new(2026);
    sim.set_speed(TimeSpeed::Extreme);
    // Intake respects restrictive policy: lockdown or max security halts
    // admissions entirely.
    sim.world_mut().policies.security_level = 1.0;
    sim.world_mut().policies.lockdown = true;
    let pop_before = sim.world().population();
    sim.run_until(sim.world().time.add_days(30));
    let pop_after = sim.world().population();
    assert!(
        pop_after <= pop_before,
        "population grew under restrictive/lockdown conditions without policy-aware intake gating: {pop_before} -> {pop_after}"
    );
}
