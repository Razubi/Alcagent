//! Spec 008 engine integration tests: gang DTOs, invariants, digest and saves.

use std::path::PathBuf;

use alc_core::GangId;
use alc_social::{GangHistoryKind, RelationshipDelta};
use sumo::Sim;
use sumo::state_digest::compute_world_digest;

mod unique {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub fn temp_dir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("sumo-gang-008-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
}

fn setup(seed: u64) -> Sim {
    let mut sim = Sim::new(seed);
    let ids: Vec<_> = sim.world().prisoners.iter().take(4).map(|p| p.id).collect();
    let world = sim.world_mut();
    assert!(world.create_gang(GangId(1), "Iron Choir", ids[0]));
    assert!(world.join_gang(ids[1], GangId(1)));
    assert!(world.create_gang(GangId(2), "Ash Gang", ids[2]));
    assert!(world.join_gang(ids[3], GangId(2)));
    {
        let (left, right) = world.gangs.split_at_mut(1);
        left[0].form_rivalry(&mut right[0]);
    }
    let mut hostile = RelationshipDelta::default();
    hostile.hostility = 0.9;
    hostile.rivalry = 0.9;
    world.social.record(ids[1], ids[3], hostile);
    sim
}

#[test]
fn gang_summaries_expose_pressure_history_and_real_relation_scores() {
    let sim = setup(0x008_001);
    let summaries = sim.gangs();
    assert_eq!(summaries.len(), 2);

    for summary in &summaries {
        assert!(summary.pressure.total() > 0.0, "rivalry creates pressure");
        assert!(
            summary
                .history
                .iter()
                .any(|entry| entry.kind == GangHistoryKind::Formed),
            "formation must be present in durable history"
        );
        assert_eq!(summary.relations.len(), 1);
        let relation = &summary.relations[0];
        assert!((relation.affinity - 1.0).abs() > f32::EPSILON);
        assert!(relation.hostility > 0.0);
    }

    let rivals = summaries
        .iter()
        .find(|g| g.id == 1)
        .expect("first gang")
        .rivalries
        .clone();
    assert_eq!(rivals.len(), 1);
    assert!(rivals[0].1 > 0.0 && rivals[0].1 <= 1.0);
    assert!((rivals[0].1 - 1.0).abs() > f32::EPSILON);
}

#[test]
fn gang_invariants_detect_drift_and_reconciliation_repairs_it() {
    let mut sim = setup(0x008_002);
    assert!(sim.check_invariants().is_empty());

    let pid = sim.world().prisoners[1].id;
    sim.world_mut().prisoners[1].gang_affiliation = Some(999);
    assert!(
        sim.check_invariants()
            .iter()
            .any(|err| err.contains("gang_membership_and_history"))
    );

    sim.world_mut().reconcile_gang_membership();
    assert!(sim.check_invariants().is_empty());
    assert_eq!(sim.world().prisoner_gang(pid), Some(GangId(1)));
}

#[test]
fn gang_history_changes_the_world_digest_deterministically() {
    let baseline = Sim::new(0x008_003);
    let mut changed = Sim::new(0x008_003);
    assert_eq!(
        compute_world_digest(baseline.world()),
        compute_world_digest(changed.world())
    );

    let pid = changed.world().prisoners[0].id;
    assert!(
        changed
            .world_mut()
            .create_gang(GangId(5), "Digest Crew", pid)
    );
    assert_ne!(
        compute_world_digest(baseline.world()),
        compute_world_digest(changed.world()),
        "durable gang history must be part of logical state"
    );

    let replay = setup(0x008_004);
    let replay_again = setup(0x008_004);
    assert_eq!(
        compute_world_digest(replay.world()),
        compute_world_digest(replay_again.world())
    );
}

#[test]
fn save_round_trip_preserves_gang_history_and_invariants() {
    let dir: PathBuf = unique::temp_dir("roundtrip");
    let mut sim = setup(0x008_005);
    let pid = sim.world().prisoners[0].id;
    sim.world_mut().gang_politics_step();
    let expected = sim.world().gang_history().to_vec();
    let digest = compute_world_digest(sim.world());

    sim.save(&dir).expect("save must succeed");
    let loaded = Sim::load(&dir).expect("v5 save must load");
    assert_eq!(loaded.world().gang_history(), expected.as_slice());
    assert_eq!(compute_world_digest(loaded.world()), digest);
    assert!(loaded.check_invariants().is_empty());

    let _ = pid;
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn weekly_step_keeps_invariants_and_exposes_history_in_summaries() {
    let mut sim = setup(0x008_006);
    for day in 0..21 {
        sim.world_mut().days_simulated = day;
        sim.world_mut().gang_politics_step();
        assert!(
            sim.check_invariants().is_empty(),
            "day {day}: {errs:?}",
            errs = sim.check_invariants()
        );
    }
    assert!(sim.gangs().iter().all(|g| !g.history.is_empty()));
}
