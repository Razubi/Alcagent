//! Consistency of the four validation surfaces (bug B11).
//!
//! The engine exposes four ways to reach a validation outcome:
//!
//! * [`Sim::propose_decision`] — a pure dry run that attaches an
//!   `estimated_outcome` to a [`DecisionProposal`];
//! * [`Sim::validate_only`] — a pure re-validation of a proposal;
//! * [`sumo::decisions::evaluate_candidates`] — a pure, read-only enumeration
//!   of every candidate with its own `estimated_outcome`;
//! * [`Sim::deliver_decision`] — the real, state-mutating pipeline.
//!
//! For the same decision at the same world state all four must agree on the
//! validation *discriminant* (accepted / repaired / rejected). These tests
//! exercise that contract using only sumo's public API and real ids taken from
//! a fresh `Sim::new(seed)`.
//!
//! NOTE ON LOCKDOWN: the two lockdown tests at the bottom intentionally assert
//! the documented invariant ("lockdown suppresses movement/escape in all
//! paths"). If they fail, that is a genuine B11 inconsistency, not a test bug:
//! `propose_decision` and `deliver_decision` suppress `MoveTo` under lockdown,
//! while `validate_only` and `evaluate_candidates` currently do not. The
//! failures are reported verbatim rather than weakened.

use alc_agents::{Decision, DecisionAction, ValidationResult};
use alc_core::PrisonerId;
use sumo::decisions::{evaluate_candidates, DecisionType};
use sumo::Sim;

const SEED: u64 = 4242;

/// Discriminant-only comparison: `ValidationResult` deliberately has no
/// `PartialEq`, and the B11 contract is about accepted/repaired/rejected, not
/// the wrapped `Decision` or rejection reason.
fn tag(result: &ValidationResult) -> &'static str {
    match result {
        ValidationResult::Accepted(_) => "accepted",
        ValidationResult::Repaired(_) => "repaired",
        ValidationResult::Rejected { .. } => "rejected",
    }
}

/// Run the three surfaces that can be invoked on one fresh world for the same
/// decision, with no tick in between:
/// `propose_decision.estimated_outcome`, `validate_only`, and
/// `deliver_decision.result`.
fn assert_three_way_agreement(
    mut sim: Sim,
    pid: PrisonerId,
    dt: DecisionType,
    target: Option<PrisonerId>,
) {
    let proposal = sim
        .propose_decision(pid, dt, target)
        .unwrap_or_else(|| panic!("no proposal produced for {dt:?} by prisoner {pid:?}"));

    let dry = sim.validate_only(pid, &proposal);
    let decision = dt.to_decision(proposal.target);
    let delivered = sim.deliver_decision(pid, decision);

    let proposed_tag = tag(&proposal.estimated_outcome);
    let validate_tag = tag(&dry);
    let delivered_tag = tag(&delivered.result);

    assert_eq!(
        proposed_tag, validate_tag,
        "{dt:?}: propose_decision estimated {:?} but validate_only said {:?} (reason: {:?})",
        proposed_tag, validate_tag, dry.rejected_reason()
    );
    assert_eq!(
        proposed_tag, delivered_tag,
        "{dt:?}: propose_decision estimated {:?} but deliver_decision said {:?} (reason: {:?})",
        proposed_tag, delivered_tag, delivered.result.rejected_reason()
    );
}

/// A co-located peer of `pid`, taken from the real bootstrap population.
fn colocated_peer(sim: &Sim, pid: PrisonerId) -> PrisonerId {
    let facility = sim
        .world()
        .prisoner_location
        .get(&pid)
        .copied()
        .expect("prisoner has a recorded location");
    sim.world()
        .prisoners
        .iter()
        .find(|p| {
            p.id != pid && sim.world().prisoner_location.get(&p.id) == Some(&facility)
        })
        .expect("at least one co-located prisoner exists at bootstrap")
        .id
}

// ---------------------------------------------------------------------------
// 1. Three-way agreement for several real Decision variants.
// ---------------------------------------------------------------------------

#[test]
fn agreement_socialize_with_real_prisoner() {
    let sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    let peer = colocated_peer(&sim, pid);
    assert_three_way_agreement(sim, pid, DecisionType::Socialize, Some(peer));
}

#[test]
fn agreement_fight_with_real_target() {
    let sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    let peer = colocated_peer(&sim, pid);
    assert_three_way_agreement(sim, pid, DecisionType::Fight, Some(peer));
}

#[test]
fn agreement_fight_repaired_to_socialize_by_guards() {
    let mut sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    let peer = colocated_peer(&sim, pid);
    // Two co-located guards trigger the deterministic de-escalation repair in
    // the validator stage 8, so all three surfaces must return `Repaired`.
    let facility = sim.world().prisoner_location[&pid];
    let guard_ids: Vec<_> = sim.world().guards.iter().take(2).map(|g| g.id).collect();
    for gid in guard_ids {
        sim.world_mut().guard_location.insert(gid, facility);
    }
    assert_three_way_agreement(sim, pid, DecisionType::Fight, Some(peer));
}

#[test]
fn agreement_move_to_real_facility() {
    let sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    let facility = sim.world().facilities[0].id;
    assert_three_way_agreement(sim, pid, DecisionType::MoveTo, Some(PrisonerId(facility.0)));
}

#[test]
fn agreement_request_medical() {
    let sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    assert_three_way_agreement(sim, pid, DecisionType::RequestMedical, None);
}

#[test]
fn agreement_comply() {
    let sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    assert_three_way_agreement(sim, pid, DecisionType::Comply, None);
}

/// Escape is blocked by the default validator's world-state stage when
/// lockdown is off only if `ctx.lockdown` is set — assert the normal
/// (non-lockdown) agreement here too.
#[test]
fn agreement_escape_attempt_without_lockdown() {
    let sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    assert_three_way_agreement(sim, pid, DecisionType::EscapeAttempt, None);
}

// ---------------------------------------------------------------------------
// 2. Rejected proposals do not mutate world state and validate_only agrees.
// ---------------------------------------------------------------------------

#[test]
fn rejected_empty_rumor_does_not_mutate_and_validate_only_agrees() {
    let mut sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;

    // `DecisionType::to_decision` always builds `SpreadRumor` with empty
    // content, which stage 2 rejects.
    let proposal = sim
        .propose_decision(pid, DecisionType::SpreadRumor, Some(pid))
        .expect("rumor proposal is produced for an available prisoner");
    assert_eq!(
        tag(&proposal.estimated_outcome),
        "rejected",
        "empty rumor content must be estimated as rejected"
    );

    let dry = sim.validate_only(pid, &proposal);
    assert_eq!(tag(&dry), "rejected", "validate_only must agree the rumor is rejected");

    let mut expected = proposal.clone();
    expected.decision_type = DecisionType::SpreadRumor;
    expected.target = Some(pid);
    let decision = Decision::new(DecisionAction::SpreadRumor {
        about: pid.raw(),
        content: String::new(),
    });

    let events_before = sim.events().len();
    let decisions_before = sim.decision_log().len();
    let trace_before = sim.trace_hash();

    let delivered = sim.deliver_decision(pid, decision);
    assert_eq!(tag(&delivered.result), "rejected");
    assert!(!delivered.applied);

    assert_eq!(sim.events().len(), events_before, "rejected proposal emitted events");
    assert_eq!(
        sim.decision_log().len(),
        decisions_before,
        "rejected proposal was appended to decision_log"
    );
    assert_eq!(sim.trace_hash(), trace_before, "rejected proposal changed trace_hash");
}

#[test]
fn unknown_prisoner_is_rejected_without_mutation() {
    let mut sim = Sim::new(SEED);
    let ghost = PrisonerId(u64::MAX);

    // The proposal path refuses the unavailable agent outright.
    assert!(
        sim.propose_decision(ghost, DecisionType::Comply, None).is_none(),
        "unknown prisoner must not produce a proposal"
    );

    // A hand-built proposal for the unknown id is still rejected by
    // validate_only (it checks id existence before building a context).
    let hand_built = sumo::decisions::DecisionProposal {
        prisoner_id: ghost,
        decision_type: DecisionType::Comply,
        target: None,
        rationale: String::new(),
        estimated_outcome: ValidationResult::Accepted(Decision::new(DecisionAction::Comply)),
        trace_hash_before: 0,
        scores: Vec::new(),
        confidence: 0.0,
        memory_records: Vec::new(),
    };
    let dry = sim.validate_only(ghost, &hand_built);
    assert_eq!(tag(&dry), "rejected", "validate_only must reject an unknown prisoner id");

    let events_before = sim.events().len();
    let decisions_before = sim.decision_log().len();

    let delivered = sim.deliver_decision(ghost, Decision::new(DecisionAction::Comply));
    assert_eq!(tag(&delivered.result), "rejected");
    assert!(!delivered.applied);

    assert_eq!(sim.events().len(), events_before);
    assert_eq!(sim.decision_log().len(), decisions_before);
}

// ---------------------------------------------------------------------------
// 3. evaluate_candidates proposals validate consistently with their estimate.
// ---------------------------------------------------------------------------

#[test]
fn evaluate_candidates_proposals_validate_consistently() {
    let sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    let proposals = evaluate_candidates(sim.world(), pid);
    assert!(!proposals.is_empty(), "evaluate_candidates produced no proposals");

    let events_before = sim.events().len();
    let decisions_before = sim.decision_log().len();
    let trace_before = sim.trace_hash();

    for proposal in &proposals {
        let dry = sim.validate_only(pid, proposal);
        assert_eq!(
            tag(&proposal.estimated_outcome),
            tag(&dry),
            "candidate {:?} (target {:?}): estimated {:?} but validate_only says {:?}",
            proposal.decision_type,
            proposal.target,
            tag(&proposal.estimated_outcome),
            tag(&dry)
        );
    }

    assert_eq!(sim.events().len(), events_before, "evaluate_candidates/validate_only emitted events");
    assert_eq!(sim.decision_log().len(), decisions_before, "pure paths mutated decision_log");
    assert_eq!(sim.trace_hash(), trace_before, "pure paths mutated trace_hash");
}

// ---------------------------------------------------------------------------
// 4. Lockdown suppresses movement / escape in ALL paths.
// ---------------------------------------------------------------------------

#[test]
fn lockdown_propose_suppresses_movement_and_escape() {
    let mut sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    let facility = sim.world().facilities[0].id;
    sim.world_mut().policies.lockdown = true;

    assert!(
        sim.propose_decision(pid, DecisionType::MoveTo, Some(PrisonerId(facility.0)))
            .is_none(),
        "propose_decision must suppress MoveTo proposals during lockdown"
    );
    assert!(
        sim.propose_decision(pid, DecisionType::EscapeAttempt, None).is_none(),
        "propose_decision must suppress EscapeAttempt proposals during lockdown"
    );
    // Comply remains available during lockdown.
    assert!(
        sim.propose_decision(pid, DecisionType::Comply, None).is_some(),
        "Comply must still be proposable during lockdown"
    );
}

#[test]
fn lockdown_deliver_rejects_movement_and_escape() {
    let mut sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    let facility = sim.world().facilities[0].id;
    sim.world_mut().policies.lockdown = true;

    let move_out = sim.deliver_decision(
        pid,
        Decision::new(DecisionAction::MoveTo { target_facility: facility.0 }),
    );
    assert_eq!(tag(&move_out.result), "rejected", "deliver must reject MoveTo during lockdown");
    assert!(!move_out.applied);

    let escape_out = sim.deliver_decision(pid, Decision::new(DecisionAction::EscapeAttempt));
    assert_eq!(tag(&escape_out.result), "rejected", "deliver must reject EscapeAttempt during lockdown");
    assert!(!escape_out.applied);
}

/// B11 exposure: `evaluate_candidates` validates through the default validator
/// only, so a `MoveTo` proposal can carry `Accepted` even while lockdown makes
/// `deliver_decision` reject the identical decision.
#[test]
fn lockdown_candidate_estimates_must_mark_movement_and_escape_rejected() {
    let mut sim = Sim::new(SEED);
    let pid = sim.world().prisoners[0].id;
    sim.world_mut().policies.lockdown = true;

    let proposals = evaluate_candidates(sim.world(), pid);
    let risky: Vec<_> = proposals
        .iter()
        .filter(|p| {
            matches!(p.decision_type, DecisionType::MoveTo | DecisionType::EscapeAttempt)
        })
        .collect();
    assert!(
        risky.iter().any(|p| p.decision_type == DecisionType::MoveTo),
        "expected a MoveTo candidate to enumerate"
    );
    assert!(
        risky.iter().any(|p| p.decision_type == DecisionType::EscapeAttempt),
        "expected an EscapeAttempt candidate to enumerate"
    );

    for proposal in risky {
        assert_eq!(
            tag(&proposal.estimated_outcome),
            "rejected",
            "candidate {:?} must be marked rejected during lockdown",
            proposal.decision_type
        );
    }
}

/// B11 exposure: `validate_only` built from a proposal captured before
/// lockdown is re-validated against the locked-down world state. It must
/// reject movement just as `deliver_decision` does.
#[test]
fn lockdown_validate_only_must_reject_pre_lockdown_movement_proposal() {
    // Capture a real proposal while the world is open.
    let open = Sim::new(SEED);
    let pid = open.world().prisoners[0].id;
    let facility = open.world().facilities[0].id;
    let proposal = open
        .propose_decision(pid, DecisionType::MoveTo, Some(PrisonerId(facility.0)))
        .expect("MoveTo proposal available before lockdown");
    assert_eq!(tag(&proposal.estimated_outcome), "accepted");

    // Now re-validate the exact same decision against a locked-down world.
    let mut locked = Sim::new(SEED);
    locked.world_mut().policies.lockdown = true;

    let dry = locked.validate_only(pid, &proposal);
    assert_eq!(
        tag(&dry),
        "rejected",
        "validate_only must reject MoveTo during lockdown to agree with deliver_decision"
    );

    let delivered = locked.deliver_decision(pid, proposal.decision_type.to_decision(proposal.target));
    assert_eq!(tag(&delivered.result), "rejected");
}
