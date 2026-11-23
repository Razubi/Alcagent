//! Determinism test (§22): same seed → identical traces.

use alc_core::TimeSpeed;
use sumo::Sim;

#[derive(PartialEq, Debug)]
struct Trace {
    time_secs: u64,
    population: u32,
    events: Vec<(u64, String, String)>,
}

fn trace(seed: u64) -> Trace {
    let mut sim = Sim::new(seed);
    sim.set_speed(TimeSpeed::FastForward);
    sim.run_until(sim.world().time.add_days(10));
    Trace {
        time_secs: sim.world().time.as_secs(),
        population: sim.world().population(),
        events: sim
            .world()
            .events
            .recent()
            .map(|e| (e.id, format!("{:?}", e.kind), e.summary.clone()))
            .collect(),
    }
}

#[test]
fn same_seed_produces_identical_traces() {
    assert_eq!(trace(123), trace(123));
}
