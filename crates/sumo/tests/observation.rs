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
    sim.world_mut().current_decisions.insert(
        pid,
        alc_agents::Decision::new(alc_agents::DecisionAction::Comply),
    );
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
    assert!(
        stats.admissions >= 1,
        "expected intake events during 20 days"
    );
}

#[test]
fn stats_since_reports_current_population() {
    let mut sim = Sim::new(21);
    sim.set_speed(TimeSpeed::FastForward);
    sim.run_until(sim.world().time.add_days(5));
    let stats = sim.stats_since(sim.world().time);
    assert!(stats.population <= stats.capacity);
}

#[test]
fn social_roles_are_deterministic_for_equal_seeds() {
    let a = Sim::new(123).social_roles();
    let b = Sim::new(123).social_roles();
    assert_eq!(a, b);
}

#[test]
fn social_roles_are_sorted_bounded_and_stable() {
    let mut sim = Sim::new(77);
    let ids: Vec<sumo::PrisonerId> = sim.world().prisoners.iter().take(6).map(|p| p.id).collect();
    assert!(ids.len() >= 3, "bootstrap should contain several prisoners");

    // Plant a strong affiliative clique so the view has a deterministic,
    // non-empty result without depending on tick timing.
    for (i, &a) in ids.iter().enumerate() {
        for &b in ids.iter().skip(i + 1) {
            let rel = sim.world_mut().social.get_mut(a, b);
            rel.trust = 0.9;
            rel.friendship = 0.9;
            rel.loyalty = 0.8;
            rel.familiarity = 0.8;
            rel.hostility = 0.0;
            rel.resentment = 0.0;
            rel.rivalry = 0.0;
        }
    }

    let first = sim.social_roles();
    let second = sim.social_roles();
    assert_eq!(first, second, "view must be stable across repeated calls");
    assert!(!first.is_empty());
    assert!(first.iter().any(|row| row.role == "Leader"));
    assert!(first.iter().any(|row| row.role == "Friend"));

    for row in &first {
        assert!(
            (0.0..=1.0).contains(&row.strength),
            "strength out of bounds: {}",
            row.strength
        );
    }
    for pair in first.windows(2) {
        let left = (pair[0].prisoner.raw(), pair[0].role.as_str());
        let right = (pair[1].prisoner.raw(), pair[1].role.as_str());
        assert!(left <= right, "rows out of order: {left:?} > {right:?}");
    }
}

#[test]
fn social_roles_reflect_hostile_interactions() {
    let mut sim = Sim::new(99);
    let ids: Vec<sumo::PrisonerId> = sim.world().prisoners.iter().take(2).map(|p| p.id).collect();
    assert_eq!(ids.len(), 2);

    for &(a, b) in &[(ids[0], ids[1]), (ids[1], ids[0])] {
        let rel = sim.world_mut().social.get_mut(a, b);
        rel.hostility = 0.9;
        rel.friendship = 0.0;
        rel.trust = 0.0;
        rel.familiarity = 0.5;
    }

    let rows = sim.social_roles();
    assert!(
        rows.iter()
            .any(|row| row.role == "Enemy" && row.prisoner == ids[0]),
        "hostile dyad should surface an Enemy classification: {rows:?}"
    );
}
