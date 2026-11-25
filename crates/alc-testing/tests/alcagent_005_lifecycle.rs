//! Alcagent 0.05 lifecycle smoke tests at the world/harness boundary.
//!
//! These complement the `sumo` integration smoke suite (`alcagent_005_smoke`)
//! with world-level lifecycle coverage driven through the public
//! [`alc_testing`] harness invariants:
//!
//! * construction queue progression / completion retires projects cleanly,
//! * post-release, rehab and contraband lifecycle state keeps the harness
//!   invariants (deadlock / capacity / finance / bounded logs) clean.

use alc_core::TimeSpeed;
use alc_testing::{check_world_invariants, invariant_no_deadlock};
use alc_world::{FacilityKind, PostReleaseRecord, PostReleaseSentenceInfo, World};

/// A queued construction project advances one day-boundary per tick and must
/// retire from the queue (with the invariants staying clean) once built.
#[test]
fn construction_queue_progression_completes_and_retires() {
    let mut world = World::new(0x0500_1001);
    world.speed = TimeSpeed::Extreme;
    let project_id = world.enqueue_construction(FacilityKind::Medical, 2, 30);
    assert_eq!(world.construction_queue.len(), 1);

    let mut violations = Vec::new();
    check_world_invariants(&world, &mut violations);
    assert!(
        violations.is_empty(),
        "fresh construction queue must be sane: {violations:?}"
    );

    // Medical builds take 10 day boundaries; run well past completion.
    let target_days = world.days_simulated + 12;
    while world.days_simulated < target_days {
        alc_testing::tick_once(&mut world);
        if world.time.minute_of_day() == 0 && world.time.hour_of_day() == 0 {
            check_world_invariants(&world, &mut violations);
        }
    }

    let completed = world
        .construction_queue
        .iter()
        .find(|p| p.id == project_id)
        .expect("completed construction project should remain in audit queue");
    assert_eq!(
        completed.status,
        alc_world::ConstructionStatus::Completed,
        "completed construction project must reach Completed status"
    );
    assert!(
        violations.is_empty(),
        "construction lifecycle must not trip harness invariants: {violations:?}"
    );
}

/// Post-release history plus rehab / contraband lifecycle state for live
/// prisoners must keep the harness invariants clean.
#[test]
fn lifecycle_state_blocks_keep_invariants_clean() {
    let mut world = World::new(0x0500_1002);
    let pid = world.prisoners[0].id;

    // Historical post-release record (Directive §15/§16 lifecycle entry).
    world.post_release_records.push(PostReleaseRecord {
        id: 1,
        prisoner_id: pid.raw(),
        name: world.prisoners[0].name.clone(),
        reason: "parole".to_string(),
        day: world.days_simulated,
        sentence_info: PostReleaseSentenceInfo {
            sentence_days: world.prisoners[0].sentence_days,
            sentence_served_days: world.prisoners[0].sentence_served_days,
            classification: world.prisoners[0].classification.clone(),
            escape_motivation_at_exit: world.prisoners[0].escape_motivation(),
        },
        outcome_seed: 0x0E5C_A9E1,
        employment: true,
        housing: true,
        violation: false,
        return_to_custody: false,
    });

    // Live rehab enrollment slot + contraband state for a current prisoner.
    world.rehab_records.insert(pid, Vec::new());
    world.contraband_state.entry(pid).or_default();

    let mut violations = Vec::new();
    check_world_invariants(&world, &mut violations);
    assert!(
        invariant_no_deadlock(&world),
        "lifecycle state must not trip the deadlock/finite-state invariant"
    );
    assert!(
        violations.is_empty(),
        "0.05 lifecycle state blocks must keep harness invariants clean: {violations:?}"
    );
}

/// Construction cancel/failure APIs should produce terminal queue records and
/// keep harness invariants clean.
#[test]
fn construction_cancel_and_failure_keep_invariants_clean() {
    use alc_events::EventKind;
    use alc_world::ConstructionStatus;

    let mut world = World::new(0x0500_1003);
    world.speed = TimeSpeed::Extreme;

    // Cancel path.
    let project_id = world.enqueue_construction(FacilityKind::Medical, 1, 20);
    assert!(world.cancel_construction(project_id, "test cancel"));
    let cancelled = world
        .construction_queue
        .iter()
        .find(|p| p.id == project_id)
        .expect("cancelled project retained for audit");
    assert_eq!(cancelled.status, ConstructionStatus::Cancelled);
    assert_eq!(cancelled.terminal_reason.as_deref(), Some("test cancel"));
    assert!(
        world
            .history
            .iter()
            .any(|e| e.kind == EventKind::ConstructionCancelled),
        "cancel should emit ConstructionCancelled"
    );

    // Failure path: missing admin prerequisite for non-admin build.
    let mut world = World::new(0x0500_1004);
    world.speed = TimeSpeed::Extreme;
    world.facilities.retain(|f| f.kind != FacilityKind::Admin);
    let failed_id = world.enqueue_construction(FacilityKind::Education, 1, 20);
    let target_days = world.days_simulated + 2;
    let mut violations = Vec::new();
    while world.days_simulated < target_days {
        alc_testing::tick_once(&mut world);
    }
    let failed = world
        .construction_queue
        .iter()
        .find(|p| p.id == failed_id)
        .expect("failed project retained for audit");
    assert_eq!(failed.status, ConstructionStatus::Failed);
    assert!(
        failed
            .terminal_reason
            .as_deref()
            .unwrap_or_default()
            .contains("admin")
    );
    assert!(
        world
            .history
            .iter()
            .any(|e| e.kind == EventKind::ConstructionFailed),
        "missing prerequisite should emit ConstructionFailed"
    );
    check_world_invariants(&world, &mut violations);
    assert!(
        violations.is_empty(),
        "construction failure path tripped invariants: {violations:?}"
    );
}
