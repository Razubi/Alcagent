//! Spec 009 — guard autonomy: area-scoped perception, imperfect reports,
//! structured prisoner interaction, and duty reassignment consequences.

use alc_core::{SimTime, TimeSpeed};
use alc_guard::{GuardReport, PostType};
use sumo::Sim;

fn corridor_guard(sim: &Sim) -> alc_core::GuardId {
    sim.world()
        .guards
        .iter()
        .find(|g| g.post == PostType::Corridor)
        .expect("bootstrap should include a corridor guard")
        .id
}

#[test]
fn observations_are_limited_to_the_guards_assigned_area() {
    let mut sim = Sim::new(901);
    let gid = corridor_guard(&sim);
    let before = sim.world().guard_observations(gid);
    assert!(
        !before.is_empty(),
        "corridor guard should see cell-block inmates"
    );

    let facility = sim.world().guard_location[&gid];
    assert!(before.iter().all(|o| {
        o.facility_id == facility.0
            && sim
                .world()
                .prisoner_location
                .get(&sumo::PrisonerId(o.prisoner_id))
                == Some(&facility)
    }));

    // Re-posting the guard to the yard must remove cell-block perception.
    assert!(sim.assign_guard_post(gid, PostType::Yard));
    let after = sim.world().guard_observations(gid);
    assert!(
        after.iter().all(|o| o.facility_id != facility.0),
        "reassigned guard must not observe the old facility"
    );
}

#[test]
fn implausible_reports_are_filtered_before_the_warden_inbox() {
    let mut sim = Sim::new(902);
    let gid = corridor_guard(&sim);
    let low_credibility = GuardReport {
        at: sim.world().time,
        post: PostType::Yard,
        summary: "unverified minor disturbance".into(),
        severity: 2,
        credibility: 0.1,
        witnessed_directly: false,
        requests_assistance: false,
    };
    assert_eq!(sim.deliver_guard_report(gid, low_credibility), 0);
    assert!(sim.world().warden.inbox.is_empty());

    let credible = GuardReport {
        at: sim.world().time,
        post: PostType::Yard,
        summary: "credible disturbance report".into(),
        severity: 2,
        credibility: 0.8,
        witnessed_directly: true,
        requests_assistance: false,
    };
    let id = sim.deliver_guard_report(gid, credible);
    assert!(id > 0, "credible report should reach the world bus");
    assert!(sim.world().warden.inbox.contains(&id));

    // The observer view resolves the warden's report DTOs from inbox ids only.
    let warden = sim.warden();
    assert_eq!(warden.inbox.len(), 1);
    assert_eq!(warden.inbox[0].summary, "credible disturbance report");
}

#[test]
fn duty_reassignment_changes_local_incident_pressure() {
    let mut sim = Sim::new(903);
    let gid = corridor_guard(&sim);
    let cell_block = sim.world().guard_location[&gid];

    let staffed = sim.world().guard_deterrence(cell_block);
    assert!(
        staffed > 0.0,
        "corridor guard should deter cell-block incidents"
    );

    assert!(sim.assign_guard_post(gid, PostType::Yard));
    let unstaffed = sim.world().guard_deterrence(cell_block);
    assert!(
        unstaffed < staffed,
        "moving the guard away must lower cell-block deterrence"
    );

    assert!(sim.assign_guard_post(gid, PostType::Wing));
    let restored = sim.world().guard_deterrence(cell_block);
    assert!(
        restored > unstaffed,
        "posting the guard to the wing must restore cell-block deterrence"
    );
}

#[test]
fn guard_interactions_change_prisoner_need_state() {
    let mut sim = Sim::new(904);
    {
        let world = sim.world_mut();
        world.time = SimTime::from_secs(3600);
        world.last_guard_duty_hour = 0;
        for guard in &mut world.guards {
            guard.competence = 1.0;
            guard.integrity = 1.0;
            guard.fatigue = 0.0;
            guard.stress = 0.0;
        }
        for p in &mut world.prisoners {
            p.needs.hunger = 1.0;
            p.needs.thirst = 1.0;
            p.needs.medical_need = 1.0;
            p.needs.respect = 0.9;
            p.needs.safety = 0.6;
        }
    }
    let before: Vec<(sumo::PrisonerId, f32)> = sim
        .world()
        .prisoners
        .iter()
        .map(|p| (p.id, p.needs.safety))
        .collect();
    let mut changed = false;
    // Repeat across duty hours so a single perceptual mistake cannot make the
    // test flaky; every sweep uses a deterministic seed derived from the hour.
    for hour in 1..=12 {
        sim.world_mut().time = SimTime::from_secs(hour * 3600);
        sim.world_mut().guard_duty_sweep();
        changed = sim.world().prisoners.iter().any(|p| {
            before
                .iter()
                .find(|(id, _)| *id == p.id)
                .is_some_and(|(_, safety)| p.needs.safety > *safety)
        });
        if changed {
            break;
        }
    }
    assert!(changed, "guard intervention should alter prisoner needs");
}

#[test]
fn guard_reliability_markers_survive_save_load() {
    let dir = std::env::temp_dir().join(format!("alcagent-009-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut sim = Sim::new(905);
    sim.set_speed(TimeSpeed::Extreme);
    sim.run_days(3);
    let source_index = sim
        .guards()
        .iter()
        .position(|g| g.tracked_opinions > 0)
        .expect("duty ticks should form opinions");
    let source = sim.guards()[source_index].clone();
    sim.save(&dir).expect("save guard autonomy markers");
    let loaded = Sim::load(&dir).expect("load guard autonomy markers");
    let restored = &loaded.guards()[source_index];
    assert_eq!(restored.tracked_opinions, source.tracked_opinions);
    assert_eq!(restored.mistakes, source.mistakes);
    assert_eq!(restored.violations, source.violations);
    std::fs::remove_dir_all(&dir).ok();
}
