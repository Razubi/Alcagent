use alc_agents::context::hash_validation_context;
use alc_agents::decision::{Decision, DecisionAction, ValidationResult};
use alc_agents::orchestrator::PendingDecision;
use alc_core::config::AiConfig;
use alc_core::{GuardId, PrisonerId, RngStream};
use alc_guard::{Guard, GuardRank, PostType};
use alc_world::World;

fn world_with(max_batch_size: usize) -> World {
    let config = AiConfig {
        max_batch_size,
        ..AiConfig::default()
    };
    World::new_with_config(0xA1C_0, &config)
}

#[test]
fn batch_applies_only_accepted_decisions() {
    let mut world = world_with(16);
    let first = world.prisoners[0].id;
    let second = world.prisoners[1].id;

    let outcome = world.submit_ai_batch(vec![
        (first, Decision::new(DecisionAction::Comply)),
        (
            second,
            Decision::new(DecisionAction::Socialize { with: 999_999 }),
        ),
    ]);

    assert_eq!(outcome.accepted, 1);
    assert_eq!(outcome.rejected, 1);
    assert_eq!(outcome.entries.len(), 2);
    assert!(world.current_decisions.contains_key(&first));
    assert!(!world.current_decisions.contains_key(&second));
}

#[test]
fn repaired_decision_is_revalidated_against_engine_rules() {
    let mut world = world_with(16);
    let actor = world.prisoners[0].id;
    let facility = world.prisoner_location[&actor];
    let guards = [world.guards[0].id, world.guards[1].id];
    for guard in guards {
        world.guard_location.insert(guard, facility);
    }
    // A guard id outside the prisoner id range is observable but not a valid
    // social target, which lets this test exercise engine-rule revalidation of
    // the repaired action rather than agent-stage target rejection.
    let observer = GuardId(999_999);
    world.guards.push(Guard::new(
        observer,
        "Observer",
        GuardRank::Officer,
        PostType::Corridor,
        &mut RngStream::new(0xA1C_0, 999_999),
    ));
    world.guard_location.insert(observer, facility);

    // Stage 8 de-escalates a heavily guarded fight to `Socialize` with the
    // original target. The engine rule must still reject a non-prisoner
    // counterparty after repair.
    let outcome = world.submit_ai_batch(vec![(
        actor,
        Decision::new(DecisionAction::Fight {
            target: observer.raw(),
        }),
    )]);

    assert_eq!(outcome.rejected, 1);
    assert_eq!(outcome.accepted, 0);
    assert!(!outcome.entries[0].applied);
    match &outcome.entries[0].result {
        ValidationResult::Rejected { reason } => {
            assert_eq!(reason, "unknown target prisoner");
        }
        other => panic!("expected repaired decision to be rejected, got {other:?}"),
    }
    assert!(!world.current_decisions.contains_key(&actor));
}

#[test]
fn stale_context_hash_rejects_batch_entry() {
    let mut world = world_with(16);
    let id = world.prisoners[0].id;
    let stale = hash_validation_context(&world.validation_context(id).expect("missing context"));
    world.policies.lockdown = true;

    let outcome = world.submit_ai_batch_with_contexts(vec![(
        id,
        Decision::new(DecisionAction::Comply),
        stale,
    )]);

    assert_eq!(outcome.rejected, 1);
    assert!(!world.current_decisions.contains_key(&id));
}

#[test]
fn batch_size_is_capped_by_configuration() {
    let mut world = world_with(2);
    let ids: Vec<PrisonerId> = world.prisoners.iter().take(3).map(|p| p.id).collect();
    let requests = ids
        .iter()
        .map(|id| (*id, Decision::new(DecisionAction::Comply)))
        .collect();

    let outcome = world.submit_ai_batch(requests);
    assert_eq!(outcome.entries.len(), 2);
    assert_eq!(outcome.accepted, 2);
    assert_eq!(world.current_decisions.len(), 2);
}

#[test]
fn departed_agent_state_is_pruned() {
    let mut world = world_with(16);
    let id = world.prisoners[0].id;
    world.orchestrator.submit_pending(PendingDecision::new(
        id.raw(),
        0,
        Decision::new(DecisionAction::Comply),
        0,
    ));

    assert!(world.validation_context(id).is_some());
    world.prisoners.retain(|p| p.id != id);
    world.prune_departed_agent_state(&[id]);

    assert!(world.validation_context(id).is_none());
    assert!(world.orchestrator.pending.is_empty());
    assert!(world.orchestrator.budget.agent_budget(id.raw()).is_none());
}
