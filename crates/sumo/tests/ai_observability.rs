use alc_agents::decision::{Decision, DecisionAction};
use alc_core::config::AiConfig;
use sumo::{DecisionAuditRecord, Sim, WardenAction, decisions::evaluate_candidates};

fn custom_config() -> AiConfig {
    AiConfig {
        provider: "local-deterministic".to_string(),
        model: "local-deterministic".to_string(),
        max_tokens_per_day: 9_000,
        max_tokens_per_agent_per_day: 300,
        rate_limit_per_sec: 3,
        rate_burst: 12,
        max_retries: 4,
        cache_ttl_secs: 60,
        max_batch_size: 5,
        decision_timeout_secs: 90,
        ..AiConfig::default()
    }
}

#[test]
fn runtime_snapshot_reflects_startup_config() {
    let sim = Sim::new_with_config(7, &custom_config());
    let snapshot = sim.ai_runtime_snapshot();
    assert_eq!(snapshot.provider, "local-deterministic");
    assert_eq!(snapshot.model, "local-deterministic");
    assert_eq!(snapshot.max_tokens_per_day, 9_000);
    assert_eq!(snapshot.max_tokens_per_agent_per_day, 300);
    assert_eq!(snapshot.rate_limit_per_sec, 3);
    assert_eq!(snapshot.rate_burst, 12);
    assert_eq!(snapshot.max_retries, 4);
    assert_eq!(snapshot.cache_ttl_secs, 60);
    assert_eq!(snapshot.max_batch_size, 5);
    assert_eq!(snapshot.decision_timeout_secs, 90);
}

#[test]
fn unsupported_provider_is_observable_not_fatal() {
    let mut config = custom_config();
    config.provider = "remote-unknown".to_string();
    let sim = Sim::new_with_config(7, &config);
    assert_eq!(sim.ai_runtime_snapshot().provider, "unsupported");
}

#[test]
fn batch_delivery_updates_runtime_telemetry() {
    let mut sim = Sim::new_with_config(7, &custom_config());
    let first = sim.world().prisoners[0].id;
    let second = sim.world().prisoners[1].id;
    let before = sim.ai_runtime_snapshot();

    let outcome = sim.deliver_decision_batch(vec![
        (first, Decision::new(DecisionAction::Comply)),
        (
            second,
            Decision::new(DecisionAction::Socialize { with: 999_999 }),
        ),
    ]);
    assert_eq!(outcome.accepted, 1);
    assert_eq!(outcome.rejected, 1);

    let after = sim.ai_runtime_snapshot();
    assert_eq!(after.batches, before.batches + 1);
    assert_eq!(after.decisions_batched, before.decisions_batched + 2);
    assert_eq!(after.accepted_decisions, before.accepted_decisions + 1);
    assert_eq!(after.rejected_decisions, before.rejected_decisions + 1);
}

#[test]
fn observer_dto_histories_and_proposals_are_bounded() {
    let mut sim = Sim::new_with_config(7, &custom_config());
    for _ in 0..24 {
        sim.apply_warden_action(WardenAction::RequestFunding { amount_major: 10 });
    }

    assert!(sim.warden().recent_decisions.len() <= 16);
    assert!(sim.guards().iter().all(|g| g.history.len() <= 16));

    let id = sim.world().prisoners[0].id;
    assert!(evaluate_candidates(sim.world(), id).len() <= 16);
}

#[test]
fn world_tick_fallback_audits_are_not_labelled_accepted_ai() {
    let mut sim = Sim::new_with_config(7, &custom_config());
    sim.set_speed(alc_core::TimeSpeed::RealTime);
    for _ in 0..8 {
        sim.tick();
    }
    let fallback = sim
        .decision_audit_log()
        .iter()
        .filter(|record| record.source == "world_tick_fallback")
        .count();
    assert!(
        fallback > 0,
        "expected at least one deterministic fallback audit"
    );
    assert!(
        sim.decision_audit_log()
            .iter()
            .all(|record| record.source != "world_tick_fallback"
                || record.validation == "deterministic_fallback")
    );
    let _typed: &[DecisionAuditRecord] = sim.decision_audit_log();
}
