//! Observation + stats tests (§20, §17).

use alc_core::TimeSpeed;
use sumo::Sim;

#[test]
fn observation_captures_world_state() {
    let sim = Sim::new(5);
    let obs = sim.observation();
    assert_eq!(obs.population, sim.world().population());
    assert_eq!(obs.capacity, sim.world().capacity());
    assert_eq!(obs.guards, sim.world().guards.len() as u32);
    assert!(obs.population <= obs.capacity);
}

#[test]
fn agent_observation_includes_brain_inspector_data() {
    let mut sim = Sim::new(31);
    let pid = sim.world().prisoners[0].id;
    // Plant a current decision so the intention surface has real data.
    sim.world_mut()
        .current_decisions
        .insert(pid, alc_agents::Decision::new(alc_agents::DecisionAction::Comply));
    let obs = sim.agent_observation(pid).expect("prisoner exists");
    assert_eq!(obs.id, pid);
    assert_eq!(obs.memory_summary, sim.world().prisoners[0].memory_summary);
    assert!(obs.current_intention.is_some());
    assert!(obs.current_intention.unwrap().contains("Comply"));
}

#[test]
fn stats_count_events_over_window() {
    let mut sim = Sim::new(6);
    sim.set_speed(TimeSpeed::FastForward);
    let start = sim.world().time;
    sim.run_until(sim.world().time.add_days(20));
    let stats = sim.stats_since(start);
    assert_eq!(stats.since, start);
    assert!(stats.population <= stats.capacity);
    assert!(stats.admissions >= 1, "expected intake events during 20 days");
}

#[test]
fn stats_since_reports_current_population() {
    let mut sim = Sim::new(21);
    sim.set_speed(TimeSpeed::FastForward);
    sim.run_until(sim.world().time.add_days(5));
    let stats = sim.stats_since(sim.world().time);
    assert!(stats.population <= stats.capacity);
}
