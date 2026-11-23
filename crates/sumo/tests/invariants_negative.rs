//! Negative tests for the S6 invariants in `sumo::invariants`.
//!
//! These tests deliberately inject invalid state through the public API and
//! assert that `Sim::check_invariants()` reports a specific violation. They
//! guard against regressions where an invariant silently becomes vacuous.

use alc_core::{GuardId, PrisonerId, SimTime};
use alc_events::EventKind;
use alc_social::RelationshipDelta;
use sumo::incidents::Incident;
use sumo::{Policy, Sim};

fn fight_incident(id: u64) -> Incident {
    Incident {
        id,
        kind: EventKind::Fight,
        cause: None,
        consequence: None,
        timestamp: SimTime::default(),
        location: "Yard".to_string(),
        involved_prisoners: Vec::new(),
        severity: 0.5,
        resolved: false,
    }
}

#[test]
fn set_policy_all_levels_keeps_invariants_clean() {
    for policy in [Policy::Normal, Policy::Enhanced, Policy::Lockdown] {
        let mut sim = Sim::new(42);
        sim.set_policy(policy);
        let errs = sim.check_invariants();
        assert!(
            errs.is_empty(),
            "set_policy({:?}) left invariant violations: {:?}",
            policy,
            errs
        );
    }
}

#[test]
fn set_lockdown_true_then_false_keeps_invariants_clean() {
    let mut sim = Sim::new(7);
    sim.set_lockdown(true);
    let errs = sim.check_invariants();
    assert!(errs.is_empty(), "set_lockdown(true) left violations: {:?}", errs);
    sim.set_lockdown(false);
    let errs = sim.check_invariants();
    assert!(errs.is_empty(), "set_lockdown(false) left violations: {:?}", errs);
}

#[test]
fn orphan_fight_incident_is_detected() {
    let mut sim = Sim::new(1);
    // A conflict incident with no cause and no consequence must be flagged.
    sim.record_sidecar_incident(fight_incident(100));
    let errs = sim.check_invariants();
    assert!(
        errs.iter().any(|e| e.contains("every_incident_has_cause_and_consequence")),
        "orphan fight should fail the incident invariant, got: {:?}",
        errs
    );

    // A dangling cause id must also be flagged specifically.
    let mut sim2 = Sim::new(1);
    let mut inc = fight_incident(101);
    inc.cause = Some(9999);
    inc.consequence = Some(9999);
    sim2.record_sidecar_incident(inc);
    let errs = sim2.check_invariants();
    assert!(
        errs.iter().any(|e| e.contains("every_incident_has_cause_and_consequence")),
        "fight with dangling cause/consequence should fail the incident invariant, got: {:?}",
        errs
    );
}

#[test]
fn duplicate_incident_id_is_detected() {
    let mut sim = Sim::new(2);
    sim.record_sidecar_incident(fight_incident(55));
    sim.record_sidecar_incident(fight_incident(55));
    let errs = sim.check_invariants();
    assert!(
        errs.iter().any(|e| e.contains("every_incident_has_cause_and_consequence")),
        "duplicate incident id should fail the incident invariant, got: {:?}",
        errs
    );
}

#[test]
fn stale_guard_location_is_detected() {
    let mut sim = Sim::new(3);
    // Insert a guard_location entry for a guard id that does not exist.
    sim.world
        .guard_location
        .insert(GuardId(u64::MAX), alc_core::FacilityId(1));
    let errs = sim.check_invariants();
    assert!(
        errs.iter().any(|e| e.contains("pawn_in_exactly_one_place")),
        "stale guard_location entry should fail the pawn invariant, got: {:?}",
        errs
    );
}

#[test]
fn stale_social_dyad_is_detected() {
    let mut sim = Sim::new(4);
    // Record a relationship involving a prisoner id that does not exist.
    sim.world
        .social
        .record(PrisonerId(u64::MAX - 1), PrisonerId(u64::MAX), RelationshipDelta::default());
    let errs = sim.check_invariants();
    assert!(
        errs.iter().any(|e| e.contains("suspicion_claim_records_reference_existing_pawns")),
        "stale social dyad should fail the social invariant, got: {:?}",
        errs
    );
}

#[test]
fn orphan_world_event_actor_is_detected() {
    let mut sim = Sim::new(5);
    // A dangling cause reference is genuine causality corruption (unlike a
    // released prisoner actor, which is valid historical data).
    let mut bad = fight_incident(1);
    bad.cause = Some(u64::MAX);
    sim.record_sidecar_incident(bad);
    let errs = sim.check_invariants();
    assert!(
        errs.iter().any(|e| e.contains("every_incident_has_cause_and_consequence")),
        "sidecar incident with dangling cause should fail the incident invariant, got: {:?}",
        errs
    );
}

#[test]
#[ignore = "pushes 10,001 dummy incidents; run explicitly if needed"]
fn logs_over_limit_are_detected() {
    let mut sim = Sim::new(6);
    for i in 0..=sumo::invariants::MAX_LOG_ENTRIES {
        sim.record_sidecar_incident(fight_incident(i as u64 + 1));
    }
    let errs = sim.check_invariants();
    assert!(
        errs.iter().any(|e| e.contains("logs_and_incident_store_bounded")),
        "over-limit sidecar should fail the bounded-log invariant, got: {:?}",
        errs
    );
}
