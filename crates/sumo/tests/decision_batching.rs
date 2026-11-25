use alc_agents::decision::{Decision, DecisionAction, ValidationResult};
use alc_core::config::AiConfig;
use alc_core::{GuardId, RngStream};
use alc_guard::{Guard, GuardRank, PostType};
use sumo::{DecisionType, PrisonerId, Sim};

fn sim_with(max_batch_size: usize) -> Sim {
    Sim::new_with_config(
        0xBEEF,
        &AiConfig {
            max_batch_size,
            ..AiConfig::default()
        },
    )
}

#[test]
fn batch_applies_accepted_and_rejects_invalid_proposals() {
    let mut sim = sim_with(16);
    let first = sim.world().prisoners[0].id;
    let second = sim.world().prisoners[1].id;

    let outcome = sim.deliver_decision_batch(vec![
        (first, Decision::new(DecisionAction::Comply)),
        (
            second,
            Decision::new(DecisionAction::Socialize { with: 999_999 }),
        ),
    ]);

    assert_eq!(outcome.accepted, 1);
    assert_eq!(outcome.rejected, 1);
    assert_eq!(outcome.applied, 1);
    assert_eq!(outcome.outcomes.len(), 2);
    assert!(outcome.outcomes[0].applied);
    assert!(!outcome.outcomes[1].applied);
    assert!(sim.world().current_decisions.contains_key(&first));
    assert!(!sim.world().current_decisions.contains_key(&second));
}

#[test]
fn batch_is_capped_and_outcomes_are_deterministic() {
    let mut sim = sim_with(2);
    let ids: Vec<PrisonerId> = sim.world().prisoners.iter().take(3).map(|p| p.id).collect();
    let requests = ids
        .iter()
        .map(|id| (*id, Decision::new(DecisionAction::Comply)))
        .collect();

    let first = sim.deliver_decision_batch(requests);
    let mut sim2 = sim_with(2);
    let requests2 = ids
        .iter()
        .map(|id| (*id, Decision::new(DecisionAction::Comply)))
        .collect();
    let second = sim2.deliver_decision_batch(requests2);

    assert_eq!(first.outcomes.len(), 2);
    assert_eq!(second.outcomes.len(), 2);
    assert_eq!(
        first
            .outcomes
            .iter()
            .map(|o| (o.agent.raw(), o.applied))
            .collect::<Vec<_>>(),
        second
            .outcomes
            .iter()
            .map(|o| (o.agent.raw(), o.applied))
            .collect::<Vec<_>>()
    );
}

#[test]
fn batch_validation_agrees_with_dry_run_gate() {
    let mut sim = sim_with(16);
    let id = sim.world().prisoners[0].id;
    let proposal = sim
        .propose_decision(id, DecisionType::MoveTo, Some(PrisonerId(999_999)))
        .expect("prisoner should be available for proposals");
    let dry = sim.validate_only(id, &proposal);
    assert!(matches!(dry, ValidationResult::Rejected { .. }));

    let decision = Decision::new(DecisionAction::MoveTo {
        target_facility: 999_999,
    });
    let outcome = sim.deliver_decision_batch(vec![(id, decision)]);
    assert_eq!(outcome.rejected, 1);
    assert_eq!(outcome.applied, 0);
}

#[test]
fn batch_revalidates_repaired_decision_through_engine_rules() {
    let mut sim = sim_with(16);
    let actor = sim.world().prisoners[0].id;
    let facility = sim.world().prisoner_location[&actor];
    for guard in [sim.world().guards[0].id, sim.world().guards[1].id] {
        sim.world.guard_location.insert(guard, facility);
    }
    // Observable guard outside the prisoner id range: the repair is valid to
    // agent-stage rules but must still fail the engine's target check.
    let observer = GuardId(999_999);
    sim.world.guards.push(Guard::new(
        observer,
        "Observer",
        GuardRank::Officer,
        PostType::Corridor,
        &mut RngStream::new(0xBEEF, 999_999),
    ));
    sim.world.guard_location.insert(observer, facility);

    let outcome = sim.deliver_decision_batch(vec![(
        actor,
        Decision::new(DecisionAction::Fight {
            target: observer.raw(),
        }),
    )]);

    assert_eq!(outcome.rejected, 1);
    assert_eq!(outcome.applied, 0);
    match &outcome.outcomes[0].result {
        ValidationResult::Rejected { reason } => assert_eq!(reason, "unknown target prisoner"),
        other => panic!("expected rejection after repair, got {other:?}"),
    }
}
