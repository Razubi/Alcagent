use alc_agents::context::{DecisionRequest, EpistemicContext, hash_validation_context};
use alc_agents::decision::{
    ActionCandidate, Decision, DecisionAction, Role, ValidationContext, ValidationResult,
};
use alc_agents::memory::EpistemicKind;
use alc_agents::orchestrator::{Orchestrator, PendingDecision, revalidate_pending};
use alc_agents::provider::DecisionProviderKind;
use alc_core::config::AiConfig;

fn request(agent: u64, now_secs: u64) -> DecisionRequest {
    DecisionRequest::new(
        agent,
        Role::Prisoner,
        now_secs,
        vec![
            ActionCandidate::new(Decision::new(DecisionAction::Comply), 0.9),
            ActionCandidate::new(Decision::new(DecisionAction::None), 0.1),
        ],
        ValidationContext {
            agent_id: agent,
            role: Role::Prisoner,
            stamina: 1.0,
            tokens_remaining_today: 4_000,
            now_secs,
            ..ValidationContext::default()
        },
        EpistemicContext::default(),
    )
}

fn baseline_config() -> AiConfig {
    AiConfig {
        cache_enabled: false,
        rate_burst: 10_000,
        rate_limit_per_sec: 10_000,
        ..AiConfig::default()
    }
}

#[test]
fn config_drives_provider_model_and_runtime_snapshot() {
    let mut config = AiConfig::default();
    config.provider = "local-deterministic".to_string();
    config.model = "local-deterministic".to_string();
    config.max_tokens_per_day = 12_345;
    config.max_tokens_per_agent_per_day = 321;
    config.rate_limit_per_sec = 7;
    config.rate_burst = 11;
    config.max_retries = 3;
    config.cache_ttl_secs = 90;
    config.max_batch_size = 4;
    config.decision_timeout_secs = 45;

    let orchestrator = Orchestrator::from_config(&config);
    let snapshot = orchestrator.runtime_snapshot();
    assert_eq!(snapshot.provider, "local-deterministic");
    assert_eq!(snapshot.model, config.model);
    assert_eq!(snapshot.max_tokens_per_day, 12_345);
    assert_eq!(snapshot.max_tokens_per_agent_per_day, 321);
    assert_eq!(snapshot.rate_limit_per_sec, 7);
    assert_eq!(snapshot.rate_burst, 11);
    assert_eq!(snapshot.max_retries, 3);
    assert_eq!(snapshot.cache_ttl_secs, 90);
    assert_eq!(snapshot.max_batch_size, 4);
    assert_eq!(snapshot.decision_timeout_secs, 45);
}

#[test]
fn unsupported_provider_degrades_to_utility_fallback() {
    let mut config = baseline_config();
    config.provider = "remote-unknown".to_string();
    config.model = "unavailable-model".to_string();

    let mut orchestrator = Orchestrator::from_config(&config);
    assert_eq!(
        orchestrator.runtime.provider,
        DecisionProviderKind::Unsupported {
            provider: "remote-unknown".to_string(),
            model: "unavailable-model".to_string(),
        }
    );
    assert!(orchestrator.propose(request(1, 0), 0).is_none());
    orchestrator.record_fallback();
    let snapshot = orchestrator.runtime_snapshot();
    assert_eq!(snapshot.provider, "unsupported");
    assert_eq!(snapshot.provider_failures, 1);
    assert_eq!(snapshot.fallback_decisions, 1);
}

#[test]
fn per_agent_budget_is_enforced_inside_propose_charge_path() {
    let mut config = baseline_config();
    config.max_tokens_per_day = 10_000;
    config.max_tokens_per_agent_per_day = 40;

    let mut orchestrator = Orchestrator::from_config(&config);
    let first = orchestrator.propose(request(7, 0), 0);
    assert!(first.is_some(), "first proposal should fit the budget");
    let second = orchestrator.propose(request(7, 1), 1);
    assert!(
        second.is_none(),
        "second proposal must not exceed agent budget"
    );

    let budget = orchestrator
        .budget
        .agent_budget(7)
        .expect("agent budget should be registered");
    assert_eq!(budget.remaining(), 8);
    assert_eq!(orchestrator.telemetry().tokens_consumed_today, 32);
    assert_eq!(orchestrator.runtime_snapshot().tokens_consumed_today, 32);
}

#[test]
fn retry_and_rate_limit_telemetry_use_sim_time_only() {
    let mut config = baseline_config();
    config.max_retries = 2;
    let mut orchestrator = Orchestrator::from_config(&config);
    orchestrator.runtime.provider = DecisionProviderKind::AlwaysFail { retryable: true };
    assert!(orchestrator.propose(request(2, 0), 0).is_none());

    let snapshot = orchestrator.runtime_snapshot();
    assert_eq!(snapshot.provider_calls, 3);
    assert_eq!(snapshot.retries, 2);
    assert_eq!(snapshot.provider_failures, 3);
    assert_eq!(snapshot.rate_limited, 0);

    let mut limited = Orchestrator::from_config(&AiConfig {
        rate_burst: 1,
        rate_limit_per_sec: 0,
        ..baseline_config()
    });
    assert!(limited.propose(request(3, 0), 0).is_some());
    assert!(limited.propose(request(4, 0), 0).is_none());
    assert_eq!(limited.runtime_snapshot().rate_limited, 1);
}

#[test]
fn cache_is_bounded_and_reports_hits() {
    let config = AiConfig {
        cache_enabled: true,
        max_cache_entries: 2,
        rate_burst: 10_000,
        rate_limit_per_sec: 10_000,
        ..AiConfig::default()
    };
    let mut orchestrator = Orchestrator::from_config(&config);
    let req = request(9, 0);
    assert!(orchestrator.propose(req.clone(), 0).is_some());
    assert!(orchestrator.propose(req, 0).is_some());
    let snapshot = orchestrator.runtime_snapshot();
    assert_eq!(snapshot.cache_hits, 1);
    assert!(snapshot.cache_entries <= snapshot.max_cache_entries);
}

#[test]
fn stale_validation_context_is_rejected() {
    let ctx = ValidationContext {
        agent_id: 5,
        role: Role::Prisoner,
        stamina: 1.0,
        tokens_remaining_today: 100,
        now_secs: 10,
        ..ValidationContext::default()
    };
    let hash = hash_validation_context(&ctx);
    let pending = PendingDecision::new(5, 10, Decision::new(DecisionAction::Comply), hash);
    assert!(matches!(
        revalidate_pending(&pending, &ctx, 20, 5),
        ValidationResult::Rejected { reason } if reason == "stale decision"
    ));

    let mut changed = ctx.clone();
    changed.lockdown = true;
    assert!(matches!(
        revalidate_pending(&pending, &changed, 20, 120),
        ValidationResult::Rejected { reason } if reason == "stale context"
    ));
}

#[test]
fn context_keeps_epistemic_buckets_distinct() {
    let now_secs = 42;
    let records = vec![
        (
            alc_agents::memory::EpistemicRecord::new(
                EpistemicKind::Rumor,
                2,
                "heard a rumor".to_string(),
            )
            .with_time(now_secs),
            0.9,
        ),
        (
            alc_agents::memory::EpistemicRecord::new(
                EpistemicKind::DirectKnowledge,
                3,
                "saw a fight".to_string(),
            )
            .with_time(now_secs),
            0.8,
        ),
        (
            alc_agents::memory::EpistemicRecord::new(
                EpistemicKind::Suspicion,
                4,
                "suspect theft".to_string(),
            )
            .with_time(now_secs),
            0.7,
        ),
        (
            alc_agents::memory::EpistemicRecord::new(
                EpistemicKind::Belief,
                5,
                "believes escape is possible".to_string(),
            )
            .with_time(now_secs),
            0.6,
        ),
        (
            alc_agents::memory::EpistemicRecord::new(
                EpistemicKind::ConfirmedFact,
                6,
                "policy changed".to_string(),
            )
            .with_time(now_secs),
            0.5,
        ),
    ];
    let context = EpistemicContext::from_records(&records, 8);
    assert_eq!(context.rumor.len(), 1);
    assert_eq!(context.direct_knowledge.len(), 1);
    assert_eq!(context.suspicion.len(), 1);
    assert_eq!(context.belief.len(), 1);
    assert_eq!(context.confirmed_fact.len(), 1);
}
