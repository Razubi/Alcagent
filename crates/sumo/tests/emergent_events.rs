//! Spec 007 integration coverage for condition-driven world events.
//!
//! The generators live in `alc-events`, but their value comes from the authoritative
//! world: they must fire under explainable pressure, stay deterministic, remain
//! bounded per day, and reach the incident views through the normal event pump.

use std::collections::BTreeMap;

use alc_core::{PrisonerId, TimeSpeed};
use sumo::Sim;

const SEED: u64 = 0x0_0050_007;

/// Build a stressed but finite institution so generators have clear causes.
fn stressed_sim(days: u64) -> Sim {
    let mut sim = Sim::new(SEED);
    {
        let w = sim.world_mut();
        for p in &mut w.prisoners {
            p.needs.hunger = 1.0;
            p.needs.companionship = 1.0;
            p.needs.family_contact = 1.0;
            p.needs.medical_need = 1.0;
            p.needs.safety = 0.0;
        }
        for g in &mut w.guards {
            g.stress = 1.0;
            g.morale = 0.0;
        }
        for f in &mut w.facilities {
            f.condition = 0.0;
        }
        let ids: Vec<u64> = w.prisoners.iter().take(8).map(|p| p.id.raw()).collect();
        for pair in ids.windows(2) {
            let rel = w.social.get_mut(PrisonerId(pair[0]), PrisonerId(pair[1]));
            rel.hostility = 1.0;
            rel.resentment = 1.0;
            rel.rivalry = 1.0;
        }
    }
    sim.set_speed(TimeSpeed::Extreme);
    sim.run_until(sim.world().time.add_days(days));
    sim
}

fn generated_signature(sim: &Sim) -> Vec<(u64, String, Vec<String>)> {
    sim.events()
        .recent()
        .filter(|e| e.causes.iter().any(|c| c == "condition-driven"))
        .map(|e| (e.at.as_days(), e.summary.clone(), e.causes.clone()))
        .collect()
}

#[test]
fn condition_driven_events_are_deterministic_for_fixed_seed() {
    let first = generated_signature(&stressed_sim(5));
    let second = generated_signature(&stressed_sim(5));
    assert!(!first.is_empty(), "stressed world must generate explainable events");
    assert_eq!(first, second);
}

#[test]
fn condition_driven_events_are_bounded_per_day() {
    let sim = stressed_sim(21);
    let mut per_day = BTreeMap::new();
    for (day, _, _) in generated_signature(&sim) {
        *per_day.entry(day).or_insert(0usize) += 1;
    }
    assert!(!per_day.is_empty());
    assert!(
        per_day.values().all(|count| *count <= 3),
        "daily generator cap exceeded: {per_day:?}"
    );
}

#[test]
fn condition_driven_events_reach_incident_views() {
    let sim = stressed_sim(5);
    let mirrored = sim
        .incident_log()
        .iter()
        .filter(|e| e.causes.iter().any(|c| c == "condition-driven"))
        .count();
    assert!(mirrored > 0, "generated events must be visible in incident views");
}
