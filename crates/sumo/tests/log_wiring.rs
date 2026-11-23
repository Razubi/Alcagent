//! Integration tests for the decision pipeline's audit/incident wiring.
//!
//! These exercise the public `Sim` API only — no private internals — and
//! verify that applied decisions are auditable, that conflict events reach
//! the incident stores with resolvable causality, and that dry-run proposals
//! never mutate world or engine state (R1/R2 acceptance).

use alc_agents::Decision;
use alc_core::{FacilityId, PrisonerId};
use alc_events::EventKind;
use sumo::{DecisionType, Sim};

/// Seed used by all tests; the bootstrap is deterministic in the seed.
const SEED: u64 = 42;

/// Fight decision records an applied decision and mirrors a Fight incident.
#[test]
fn fight_decision_records_decision_and_incident() {
    let mut sim = Sim::new(SEED);
    let p1 = sim.world().prisoners[0].id;
    let p2 = sim.world().prisoners[1].id;

    let decision_log_before = sim.decision_log().len();
    let incident_log_before = sim.incident_log().len();
    let sidecar_before = sim.incident_sidecar().len();

    let outcome = sim.deliver_decision(p1, Decision::Fight { target: p2.raw() });
    assert!(outcome.applied, "fight between two known prisoners should apply");

    // R2: the applied decision is in the audit log.
    assert_eq!(sim.decision_log().len(), decision_log_before + 1);
    assert_eq!(sim.decision_log().last().unwrap(), &Decision::Fight { target: p2.raw() });

    // R1: a conflict event was mirrored into both incident stores.
    assert!(sim.incident_log().len() > incident_log_before);
    assert!(sim.incident_sidecar().len() > sidecar_before);

    let fight = sim
        .incident_sidecar()
        .iter()
        .find(|inc| inc.kind == EventKind::Fight || inc.kind == EventKind::Argument || inc.kind == EventKind::Threat)
        .expect("a fight decision should produce at least one conflict incident");
    assert!(fight.cause.is_some(), "conflict incident must have a cause");
    assert!(fight.consequence.is_some(), "conflict incident must have a consequence");
    assert!(
        sim.incident_sidecar().iter().any(|i| i.id == fight.cause.unwrap()),
        "cause id must resolve to an existing incident"
    );
    assert!(
        sim.incident_sidecar().iter().any(|i| i.id == fight.consequence.unwrap()),
        "consequence id must resolve to an existing incident"
    );

    // Conflict incidents must list their prisoner actors.
    assert!(
        fight.involved_prisoners.contains(&p1) || fight.involved_prisoners.contains(&p2),
        "conflict incident should name at least one involved prisoner"
    );

    // Engine invariants must still hold after delivery.
    assert!(
        sim.check_invariants().is_empty(),
        "invariants violated after fight delivery: {:?}",
        sim.check_invariants()
    );
}

/// MoveTo updates the authoritative movement map used by MapFrame.
#[test]
fn move_to_updates_prisoner_location() {
    let mut sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;

    // Pick a facility that is not the prisoner's current location.
    let current = sim.world().prisoner_location.get(&pid).copied();
    let target = sim
        .world()
        .facilities
        .iter()
        .map(|f| f.id)
        .find(|id| Some(*id) != current)
        .expect("bootstrap must provide more than one facility");

    let outcome = sim.deliver_decision(pid, Decision::MoveTo { target_facility: target.0 });
    assert!(outcome.applied, "MoveTo to a valid facility should apply");

    assert_eq!(
        sim.world().prisoner_location.get(&pid).copied(),
        Some(target),
        "prisoner_location must reflect the move"
    );

    // The schedule wing must stay in sync with the target facility.
    let wing = sim
        .world()
        .facilities
        .iter()
        .find(|f| f.id == target)
        .unwrap()
        .wing;
    assert_eq!(
        sim.world().prisoners.iter().find(|p| p.id == pid).unwrap().schedule.wing,
        wing
    );
}

/// Trade has an observable social effect on the relationship edge.
#[test]
fn trade_has_observable_effect() {
    let mut sim = Sim::new(SEED);
    let p1 = sim.world().prisoners[0].id;
    let p2 = sim.world().prisoners[1].id;

    let familiarity_before = sim.world().social.get(p1, p2).familiarity;
    let ledger_before = sim.world().economy.ledger.len();

    let outcome = sim.deliver_decision(
        p1,
        Decision::Trade { with: p2.raw(), item: "cigarette".into() },
    );
    assert!(outcome.applied);

    // Relationship edge was strengthened.
    assert!(
        sim.world().social.get(p1, p2).familiarity > familiarity_before,
        "trade should increase familiarity"
    );

    // Ledger records the (balanced) economic transfer.
    assert!(
        sim.world().economy.ledger.len() > ledger_before,
        "trade should leave at least one observable ledger entry"
    );
    let new_entries = &sim.world().economy.ledger[ledger_before..];
    let sum: i64 = new_entries.iter().map(|e| e.amount.0).sum();
    assert_eq!(sum, 0, "trade ledger entries should balance to zero");

    // The event bus carries a relationship-shift event naming both actors.
    assert!(
        sim.events().recent().any(|e| {
            e.kind == EventKind::RelationshipShift
                && e.actors.contains(&p1.raw())
                && e.actors.contains(&p2.raw())
        }),
        "trade should emit a RelationshipShift event with both actors"
    );
}

/// Rejected decisions leave the audit log untouched.
#[test]
fn rejected_decision_not_recorded() {
    let mut sim = Sim::new(SEED);
    let p1 = sim.world().prisoners[0].id;
    let decision_log_before = sim.decision_log().len();
    let incidents_before = sim.incident_log().len();

    // Unknown target id (well outside the bootstrapped prisoner range).
    let outcome = sim.deliver_decision(p1, Decision::Fight { target: 999_999 });
    assert!(!outcome.applied, "fight against unknown target must be rejected");
    assert!(outcome.result.rejected_reason().is_some());

    assert_eq!(
        sim.decision_log().len(),
        decision_log_before,
        "rejected decisions must not enter the audit log"
    );
    assert_eq!(
        sim.incident_log().len(),
        incidents_before,
        "rejected decisions must not emit incidents"
    );
}

/// propose_decision is a pure dry run: no events, trace or audit changes.
#[test]
fn propose_decision_does_not_mutate() {
    let sim = Sim::new(SEED);
    let p1 = sim.world().prisoners[0].id;
    let p2 = sim.world().prisoners[1].id;

    let trace_before = sim.trace_hash();
    let events_before = sim.events().len();
    let decision_log_before = sim.decision_log().len();
    let incidents_before = sim.incident_log().len();
    let sidecar_before = sim.incident_sidecar().len();
    let economy_ledger_before = sim.world().economy.ledger.len();

    let proposal = sim.propose_decision(p1, DecisionType::Socialize, Some(p2));
    assert!(proposal.is_some(), "proposal should be produced for an available prisoner");

    // Everything observable stays identical.
    assert_eq!(sim.trace_hash(), trace_before, "trace_hash must not change");
    assert_eq!(sim.events().len(), events_before, "event bus must not change");
    assert_eq!(sim.decision_log().len(), decision_log_before, "audit log must not change");
    assert_eq!(sim.incident_log().len(), incidents_before, "incident log must not change");
    assert_eq!(sim.incident_sidecar().len(), sidecar_before, "sidecar must not change");
    assert_eq!(
        sim.world().economy.ledger.len(),
        economy_ledger_before,
        "economy ledger must not change"
    );
    assert_eq!(
        proposal.unwrap().trace_hash_before,
        trace_before,
        "proposal should capture the pre-decision trace"
    );
}

/// A second fight delivery gets a unique sidecar id (no duplicate ids).
#[test]
fn multiple_deliveries_assign_unique_incident_ids() {
    let mut sim = Sim::new(SEED);
    let p1 = sim.world().prisoners[0].id;
    let p2 = sim.world().prisoners[1].id;

    sim.deliver_decision(p1, Decision::Fight { target: p2.raw() });
    sim.deliver_decision(p2, Decision::Fight { target: p1.raw() });

    let mut ids: Vec<u64> = sim.incident_sidecar().iter().map(|i| i.id).collect();
    ids.sort_unstable();
    let before_len = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), before_len, "incident ids must be unique across deliveries");

    assert!(
        sim.check_invariants().is_empty(),
        "invariants violated after repeated deliveries: {:?}",
        sim.check_invariants()
    );
}

/// MoveTo into an unknown facility is rejected without touching locations.
#[test]
fn move_to_unknown_facility_rejected() {
    let mut sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    let before = sim.world().prisoner_location.get(&pid).copied();

    let outcome = sim.deliver_decision(pid, Decision::MoveTo { target_facility: 999_999 });
    assert!(!outcome.applied);

    assert_eq!(sim.world().prisoner_location.get(&pid).copied(), before);
    assert!(sim.decision_log().is_empty());
}

/// Suppress unused-import lint for FacilityId if future test variants drop it.
#[allow(dead_code)]
fn _facility_id_type_is_public(_: FacilityId, _: PrisonerId) {}
