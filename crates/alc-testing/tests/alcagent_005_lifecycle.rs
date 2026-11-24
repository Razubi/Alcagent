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

    assert!(
        !world.construction_queue.iter().any(|p| p.id == project_id),
        "completed construction project must leave the queue"
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
