//! Decision pipeline tests (§19 — agents propose, the engine disposes).

use alc_agents::{Decision, DecisionAction, ValidationResult};
use alc_core::GangId;
use sumo::Sim;

#[test]
fn deliver_move_to_valid_facility() {
    let mut sim = Sim::new(10);
    let id = sim.world().prisoners[0].id;
    let f = sim.world().facilities[0].id;
    let out = sim.deliver_decision(
        id,
        Decision::new(DecisionAction::MoveTo {
            target_facility: f.0,
        }),
    );
    assert!(matches!(out.result, ValidationResult::Accepted(_)));
    assert!(out.applied);
}

#[test]
fn deliver_move_to_unknown_facility_rejected() {
    let mut sim = Sim::new(11);
    let id = sim.world().prisoners[0].id;
    let out = sim.deliver_decision(
        id,
        Decision::new(DecisionAction::MoveTo {
            target_facility: 999,
        }),
    );
    assert!(matches!(out.result, ValidationResult::Rejected { .. }));
    assert!(!out.applied);
}

#[test]
fn deliver_move_to_rejected_during_lockdown() {
    let mut sim = Sim::new(12);
    let id = sim.world().prisoners[0].id;
    sim.world_mut().policies.lockdown = true;
    let f = sim.world().facilities[0].id;
    let out = sim.deliver_decision(
        id,
        Decision::new(DecisionAction::MoveTo {
            target_facility: f.0,
        }),
    );
    assert!(matches!(out.result, ValidationResult::Rejected { .. }));
}

#[test]
fn escape_attempt_rejected_during_lockdown() {
    let mut sim = Sim::new(13);
    let id = sim.world().prisoners[0].id;
    sim.world_mut().policies.lockdown = true;
    let out = sim.deliver_decision(id, Decision::new(DecisionAction::EscapeAttempt));
    assert!(matches!(out.result, ValidationResult::Rejected { .. }));
    assert!(!out.applied);
}

#[test]
fn fight_repaired_to_socialize_when_guards_nearby() {
    let mut sim = Sim::new(14);
    let id = sim.world().prisoners[0].id;
    let other = sim.world().prisoners[1].id;
    // Place two guards in the prisoner's facility: guards_nearby counts only
    // co-located guards (information model §3), not the global guard total.
    let facility = sim.world().prisoner_location[&id];
    let guard_ids: Vec<_> = sim.world().guards.iter().take(2).map(|g| g.id).collect();
    for gid in guard_ids {
        sim.world_mut().guard_location.insert(gid, facility);
    }
    let out = sim.deliver_decision(
        id,
        Decision::new(DecisionAction::Fight {
            target: other.raw(),
        }),
    );
    assert!(matches!(
        out.result,
        ValidationResult::Repaired(Decision {
            action: DecisionAction::Socialize { .. },
            ..
        })
    ));
}

#[test]
fn socialize_accepted_and_records_relationship() {
    let mut sim = Sim::new(15);
    let id = sim.world().prisoners[0].id;
    let other = sim.world().prisoners[1].id;
    let out = sim.deliver_decision(
        id,
        Decision::new(DecisionAction::Socialize { with: other.raw() }),
    );
    assert!(matches!(out.result, ValidationResult::Accepted(_)));
    let rel = sim.world().social.get(id, other);
    assert!(rel.familiarity > 0.0);
}

#[test]
fn spread_rumor_emits_event() {
    let mut sim = Sim::new(16);
    let id = sim.world().prisoners[0].id;
    let out = sim.deliver_decision(
        id,
        Decision::new(DecisionAction::SpreadRumor {
            about: id.raw(),
            content: "rumor".into(),
        }),
    );
    assert!(matches!(out.result, ValidationResult::Accepted(_)));
    assert!(sim.world().events.len() > 0);
}

#[test]
fn join_gang_applied_and_recorded() {
    let mut sim = Sim::new(17);
    let founder = sim.world().prisoners[0].id;
    sim.world_mut()
        .gangs
        .push(alc_social::Gang::new(GangId(1), "Testers", founder));
    let out = sim.deliver_decision(founder, Decision::new(DecisionAction::JoinGang { gang: 1 }));
    assert!(matches!(out.result, ValidationResult::Accepted(_)));
    assert_eq!(sim.world().prisoners[0].gang_affiliation, Some(1));
}

#[test]
fn lead_gang_rejected_when_not_member() {
    let mut sim = Sim::new(18);
    // World boots with no gangs → unknown gang → rejected.
    let id = sim.world().prisoners[0].id;
    let out = sim.deliver_decision(id, Decision::new(DecisionAction::LeadGang { gang: 42 }));
    assert!(matches!(out.result, ValidationResult::Rejected { .. }));
}

#[test]
fn request_medical_applied() {
    let mut sim = Sim::new(19);
    let id = sim.world().prisoners[0].id;
    sim.world_mut().prisoners[0].needs.medical_need = 0.5;
    let before = sim.world().prisoners[0].needs.medical_need;
    let out = sim.deliver_decision(id, Decision::new(DecisionAction::RequestMedical));
    assert!(matches!(out.result, ValidationResult::Accepted(_)));
    assert!(
        sim.world().prisoners[0].needs.medical_need < before,
        "requesting care must relieve medical need, not create a runaway deficit (before={before}, after={})",
        sim.world().prisoners[0].needs.medical_need
    );
}

#[test]
fn deliver_unknown_prisoner_rejected() {
    let mut sim = Sim::new(20);
    let out = sim.deliver_decision(
        alc_core::PrisonerId(999),
        Decision::new(DecisionAction::Comply),
    );
    assert!(matches!(out.result, ValidationResult::Rejected { .. }));
    assert!(!out.applied);
}
