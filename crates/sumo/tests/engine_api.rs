//! Engine time-control API tests (§21).

use alc_core::{SimTime, TimeSpeed};
use sumo::Sim;

#[test]
fn new_sim_is_paused_at_epoch() {
    let sim = Sim::new(7);
    assert_eq!(sim.speed(), TimeSpeed::Paused);
    assert_eq!(sim.world().time, SimTime::EPOCH);
    assert!(sim.world().population() > 0);
}

#[test]
fn tick_advances_time_when_not_paused() {
    let mut sim = Sim::new(1);
    sim.set_speed(TimeSpeed::RealTime);
    let before = sim.world().time;
    sim.tick();
    assert!(sim.world().time > before);
}

#[test]
fn paused_means_no_progress() {
    let mut sim = Sim::new(3);
    let before = sim.world().time;
    sim.tick();
    assert_eq!(sim.world().time, before);
}

#[test]
fn run_until_target() {
    let mut sim = Sim::new(2);
    sim.set_speed(TimeSpeed::FastForward);
    let target = Sim::new(2).world().time.add_days(5);
    let ticks = sim.run_until(target);
    assert!(ticks > 0);
    assert!(sim.world().time >= target);
    assert!(sim.days_simulated() > 0);
}
