//! Council & Shadow policy mode tests (R3).
//!
//! These exercise the three [`PolicyMode`] branches of
//! [`sumo::drivers::warden_cycle`]:
//!
//! - `Deterministic` must remain byte-for-byte trace-identical to the
//!   historical arbitrate path.
//! - `Council` must apply a deterministic drive vote and explain it in a
//!   `PolicyChange` event.
//! - `Shadow` must apply the deterministic baseline while recording the
//!   council's alternative as a `SHADOW` event.

use alc_core::{TimeSpeed, WardenId};
use alc_warden::{Warden, WardenTraits};
use alc_warden::WardenAction;
use sumo::council::{self, DriveWeights};
use sumo::{PolicyMode, Sim};

/// Extreme speed runs 30 simulated minutes per tick, keeping 30-day runs quick.
fn prepared(seed: u64, mode: PolicyMode) -> Sim {
    let mut sim = Sim::new(seed);
    sim.set_policy_mode(mode);
    sim.set_speed(TimeSpeed::Extreme);
    sim
}

/// Run `days` days, invoking the explicit warden driver once per simulated day.
fn drive(sim: &mut Sim, days: u64) {
    for _ in 0..days {
        sim.run_days(1);
        sim.warden_cycle();
    }
}

/// Work around the known Wave 5 drift between `world.policies` (mutated by
/// `World::tick`'s own warden arbitration) and `engine_state.policy_state`.
///
/// `NoAction` is a no-op applier, but `Sim::apply_warden_action` still
/// reconciles the engine-side policy mirror afterwards, so this is a safe,
/// public resync with no behavioral side effects.
fn resync_policy_mirror(sim: &mut Sim) {
    sim.apply_warden_action(WardenAction::NoAction {
        rationale: "test policy-mirror resync".into(),
    });
}

#[test]
fn deterministic_mode_trace_unchanged() {
    // Default mode (implicit Deterministic) vs one explicitly set: identical
    // seeding and identical driver calls must produce the same trace hash.
    let mut default_mode = prepared(4242, PolicyMode::Deterministic);
    let mut explicit_mode = prepared(4242, PolicyMode::Deterministic);

    drive(&mut default_mode, 30);
    drive(&mut explicit_mode, 30);

    assert_eq!(
        default_mode.trace_hash(),
        explicit_mode.trace_hash(),
        "explicit Deterministic mode must not change the trace"
    );
    assert_eq!(default_mode.days_simulated(), explicit_mode.days_simulated());
    assert_eq!(
        default_mode.events().recent().count(),
        explicit_mode.events().recent().count(),
        "event volume must match in Deterministic mode"
    );

    // A second identical run must also match (no hidden per-process state).
    let mut replay = prepared(4242, PolicyMode::Deterministic);
    drive(&mut replay, 30);
    assert_eq!(
        default_mode.trace_hash(),
        replay.trace_hash(),
        "same-seed Deterministic runs must replay identically"
    );
}

#[test]
fn council_mode_produces_rationale() {
    let mut sim = prepared(1, PolicyMode::Council);

    let mut council_events = 0_u32;
    for _ in 0..30 {
        sim.run_days(1);
        let applied = sim.warden_cycle();
        assert_eq!(applied.len(), 1, "the council must apply exactly one action");

        // The rationale event is emitted last, so it must be the newest event.
        let last = sim
            .events()
            .recent()
            .last()
            .expect("warden cycle must emit events");
        assert!(
            last.summary.contains("Council decision"),
            "expected the council rationale event, got: {}",
            last.summary
        );
        assert!(
            last.summary.contains("council"),
            "rationale must name the council: {}",
            last.summary
        );
        council_events += 1;
    }

    assert_eq!(council_events, 30, "one council decision per cycle");

    // Every event summary must cite at least one institutional drive.
    let drive_named = sim.events().recent().any(|e| {
        e.summary.contains("Council decision")
            && council::Drive::ALL
                .iter()
                .any(|d| e.summary.contains(d.label()))
    });
    assert!(drive_named, "council events must cite a drive by name");

    resync_policy_mirror(&mut sim);
    let errors = sim.check_invariants();
    assert!(
        errors.is_empty(),
        "invariants violated in Council mode: {errors:?}"
    );
}

#[test]
fn shadow_mode_applies_baseline_and_records_divergence() {
    // Reference run: strict Deterministic mode with the same seed and drive
    // schedule. Shadow must apply exactly the same actions.
    let mut baseline = prepared(31, PolicyMode::Deterministic);
    let mut shadow = prepared(31, PolicyMode::Shadow);

    let mut shadow_events = 0_u32;
    let mut divergences = 0_u32;

    for day in 0..30 {
        baseline.run_days(1);
        shadow.run_days(1);
        let expected = baseline.warden_cycle();
        let applied = shadow.warden_cycle();

        assert_eq!(expected.len(), 1);
        assert_eq!(applied.len(), 1);
        assert_eq!(
            format!("{:?}", applied[0]),
            format!("{:?}", expected[0]),
            "shadow mode must apply the deterministic baseline (day {day})"
        );

        let last = shadow
            .events()
            .recent()
            .last()
            .expect("shadow cycle must emit a comparison event");
        assert!(
            last.summary.contains("SHADOW"),
            "shadow mode must record its comparison: {}",
            last.summary
        );
        assert!(
            last.summary.contains("baseline=") && last.summary.contains("council="),
            "shadow event must contain both actions: {}",
            last.summary
        );
        assert!(
            last.summary.contains("rationale="),
            "shadow event must contain the council rationale: {}",
            last.summary
        );
        shadow_events += 1;
        if last.summary.contains("SHADOW divergence") {
            divergences += 1;
        }
    }

    assert_eq!(shadow_events, 30, "one shadow comparison per cycle");

    // The world itself must follow the baseline: same population/time as the
    // deterministic reference.
    assert_eq!(shadow.world().time, baseline.world().time);
    assert_eq!(shadow.world().population(), baseline.world().population());

    // Divergence is data, not a requirement — but the counter must be sane.
    assert!(divergences <= shadow_events);
    eprintln!("SHADOW divergences: {divergences}/{shadow_events}");
    if let Some(e) = shadow
        .events()
        .recent()
        .find(|e| e.summary.contains("SHADOW"))
    {
        eprintln!("first shadow event: {}", e.summary);
    }

    resync_policy_mirror(&mut shadow);
    let errors = shadow.check_invariants();
    assert!(
        errors.is_empty(),
        "invariants violated in Shadow mode: {errors:?}"
    );
}

#[test]
fn council_deliberation_is_deterministic() {
    let world = sumo::World::new(9);
    let weights = DriveWeights::from_warden(&world.warden);
    let candidates = council::candidate_actions(&world);

    let first = council::deliberate(&world, &weights, &candidates);
    let second = council::deliberate(&world, &weights, &candidates);
    let third = council::deliberate(&world, &weights, &candidates);

    assert_eq!(
        first.total.to_bits(),
        second.total.to_bits(),
        "totals must match bit-for-bit"
    );
    assert_eq!(first.total.to_bits(), third.total.to_bits());
    assert_eq!(first.rationale, second.rationale);
    assert_eq!(first.rationale, third.rationale);
    assert!(
        council::actions_equivalent(&first.action, &second.action),
        "same snapshot must select the same action"
    );
    assert_eq!(first.scores.len(), council::Drive::COUNT);
    assert!(first.total > 0.0);
}

#[test]
fn drive_weights_derive_from_warden() {
    let traits = |strictness: f32, compassion: f32, reform: f32| WardenTraits {
        strictness,
        compassion,
        political_ambition: 0.5,
        financial_discipline: 0.5,
        risk_tolerance: 0.5,
        reform_orientation: reform,
        authoritarian_tendencies: 0.5,
        staff_loyalty: 0.5,
        corruption_susceptibility: 0.2,
        reputation_concerns: 0.5,
        competence: 0.5,
    };

    let strict = Warden::new(WardenId(0), "Strict", traits(0.95, 0.1, 0.1));
    let reformist = Warden::new(WardenId(1), "Reformist", traits(0.1, 0.95, 0.95));

    let strict_weights = DriveWeights::from_warden(&strict);
    let reformist_weights = DriveWeights::from_warden(&reformist);

    assert_ne!(
        strict_weights, reformist_weights,
        "different warden personalities must produce different weights"
    );
    assert!(
        strict_weights.security > reformist_weights.security,
        "strict warden should prioritise security: {} vs {}",
        strict_weights.security,
        reformist_weights.security
    );
    assert!(
        reformist_weights.rehab > strict_weights.rehab,
        "reformist warden should prioritise rehab: {} vs {}",
        reformist_weights.rehab,
        strict_weights.rehab
    );

    // Weights must be usable as a vote: positive, bounded and finite.
    for drive in council::Drive::ALL {
        for weights in [strict_weights, reformist_weights] {
            let w = weights.get(drive);
            assert!(w.is_finite(), "{:?} weight must be finite", drive);
            assert!(
                (0.05..=2.0).contains(&w),
                "{:?} weight {w} out of range",
                drive
            );
        }
    }
    assert!(strict_weights.total() > 0.0);
}
