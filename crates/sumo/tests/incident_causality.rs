//! Regression tests for bug B2: incident causality must be real, never
//! self-referential. Mirrored incidents derive causal links from
//! `WorldEvent.causes` and a matching prior incident; `cause`/`consequence`
//! stay `None` when there is no honest link to record.

use alc_core::{PrisonerId, SimTime};
use alc_events::{EventKind, WorldEvent};
use sumo::incidents::{incident_from_world_event, CausalLink, Incident};
use sumo::invariants::invariant_every_incident_has_cause_and_consequence;
use sumo::Sim;

fn known() -> Vec<PrisonerId> {
    vec![PrisonerId(1), PrisonerId(2), PrisonerId(7)]
}

fn event(kind: EventKind, causes: Vec<&str>) -> WorldEvent {
    let mut e = WorldEvent::new(
        500,
        SimTime { secs: 1000 },
        kind,
        "prisoner altercation in the yard",
    );
    e = e.with_actor(1).with_actor(7).with_actor(999); // 999 is a guard, not a prisoner
    for cause in causes {
        e = e.with_cause(cause);
    }
    e
}

#[test]
fn event_cause_string_becomes_a_real_world_event_link() {
    let e = event(EventKind::Fight, vec!["mutual disrespect over unpaid debt"]);
    let inc = incident_from_world_event(&e, 10, &[], &known(), 0.9, "facility 3".into());

    // Free-text cause -> real CausalLink to the originating world event.
    assert!(inc.causal_chain.iter().any(|l| {
        !l.is_incident
            && l.source_id == e.id
            && l.description == "mutual disrespect over unpaid debt"
    }));

    // No prior incident -> no incident cause id; consequence never guessed.
    assert_eq!(inc.cause, None);
    assert_eq!(inc.consequence, None);

    // Provenance link to the event itself is always present.
    assert!(inc.causal_chain.iter().any(|l| {
        !l.is_incident
            && l.source_id == e.id
            && l.description.contains("originating event")
    }));

    // Witnesses/involved are filtered to known prisoners only.
    assert_eq!(inc.involved_prisoners, vec![PrisonerId(1), PrisonerId(7)]);
    assert_eq!(inc.witnesses, vec![PrisonerId(1), PrisonerId(7)]);
}

#[test]
fn prior_incident_becomes_a_real_cause_link() {
    let prior = Incident {
        id: 4,
        kind: EventKind::Argument,
        causal_chain: vec![CausalLink::world_event(499, "earlier insult")],
        timestamp: SimTime { secs: 900 },
        involved_prisoners: vec![PrisonerId(1)],
        severity: 0.3,
        ..Incident::default()
    };
    let e = event(EventKind::Fight, vec!["escalation"]);
    let inc = incident_from_world_event(&e, 11, &[prior.clone()], &known(), 0.9, "yard".into());

    assert_eq!(inc.cause, Some(prior.id));
    assert_eq!(inc.consequence, None);
    assert!(inc.causal_chain.iter().any(|l| {
        l.is_incident && l.source_id == prior.id && l.description.contains("preceding")
    }));
}

#[test]
fn no_incident_is_self_referential() {
    let e = event(EventKind::Argument, vec![]);
    let inc = incident_from_world_event(&e, 42, &[], &known(), 0.25, "cell".into());

    assert_ne!(inc.cause, Some(inc.id));
    assert_ne!(inc.consequence, Some(inc.id));
    assert!(!(inc.cause == Some(inc.id) && inc.consequence == Some(inc.id)));

    // A generated id must never be reused as a self-link in the chain either.
    assert!(inc
        .causal_chain
        .iter()
        .all(|l| !(l.is_incident && l.source_id == inc.id)));
}

#[test]
fn invariant_accepts_legitimate_no_cause_incidents() {
    let mut sim = Sim::new(1);
    let e = event(EventKind::Theft, vec![]);
    let inc = incident_from_world_event(&e, 1, &[], &known(), 0.5, "yard".into());
    assert_eq!(inc.cause, None);
    sim.record_sidecar_incident(inc);
    assert!(invariant_every_incident_has_cause_and_consequence(&sim));
}

#[test]
fn invariant_rejects_dangling_cause_and_self_reference() {
    let mut sim = Sim::new(2);
    let mut bad = incident_from_world_event(&event(EventKind::Fight, vec![]), 1, &[], &known(), 0.9, "yard".into());
    bad.cause = Some(9999); // dangling
    sim.record_sidecar_incident(bad);
    assert!(!invariant_every_incident_has_cause_and_consequence(&sim));

    let mut sim2 = Sim::new(3);
    let mut self_ref = incident_from_world_event(&event(EventKind::Fight, vec![]), 7, &[], &known(), 0.9, "yard".into());
    self_ref.cause = Some(7);
    self_ref.consequence = Some(7);
    sim2.record_sidecar_incident(self_ref);
    assert!(!invariant_every_incident_has_cause_and_consequence(&sim2));
}

#[test]
fn invariant_rejects_dangling_incident_chain_link() {
    let mut sim = Sim::new(4);
    let mut bad = incident_from_world_event(
        &event(EventKind::Fight, vec![]),
        1,
        &[],
        &known(),
        0.9,
        "yard".into(),
    );
    bad.causal_chain.push(CausalLink::incident(9_999, "missing prior"));
    sim.record_sidecar_incident(bad);
    assert!(
        !invariant_every_incident_has_cause_and_consequence(&sim),
        "a structured chain link must resolve to a live incident"
    );
}
