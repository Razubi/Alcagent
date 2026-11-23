//! Performance smoke + soak test (§22, §24 Phase 4): long-duration run.

use alc_core::TimeSpeed;
use sumo::Sim;

#[test]
fn soak_150_days_fast_forward() {
    let mut sim = Sim::new(77);
    sim.set_speed(TimeSpeed::FastForward);
    let target = sim.world().time.add_days(150);
    sim.run_until(target);
    assert!(sim.world().time >= target, "simulation did not reach target time");
    assert!(sim.world().population() <= sim.world().capacity());
    let errs = sim.check_invariants();
    assert!(errs.is_empty(), "invariants violated: {errs:?}");
}
