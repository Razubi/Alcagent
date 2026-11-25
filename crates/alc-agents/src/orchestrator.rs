//! Three-tier AI orchestration (Directive §19; design §9).
//!
//! The orchestrator routes decisions through three tiers:
//!
//! * **Tier 0 — deterministic simulation**: needs, schedules, movement.
//! * **Tier 1 — utility choice**: routine ranking/selection, and the
//!   deterministic fallback used whenever AI slots are unavailable.
//! * **Tier 2 — AI deliberation**: periodic, budgeted, rate-limited, and
//!   always re-validated before application.
//!
//! AI failure degrades to deterministic behaviour; it never halts the world.

use alc_core::RngStream;
use alc_core::config::AiConfig;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use tracing::debug;

use crate::context::{DecisionRequest, hash_validation_context};
use crate::decision::{
    ActionCandidate, Decision, DecisionAction, Legality, RawDecision, Rejection, Role,
    ValidationContext, ValidationResult, run_validation_pipeline, validate_format,
};
use crate::disposition::{Disposition, trait_bias};
use crate::provider::DecisionProviderKind;

// ---------------------------------------------------------------------------
// Rate limiting
// ---------------------------------------------------------------------------

/// Token-bucket rate limiter (per provider).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimiter {
    capacity: u32,
    tokens: u32,
    refill_per_sec: f32,
    last_refill_secs: u64,
}

impl RateLimiter {
    pub fn new(capacity: u32, refill_per_sec: f32) -> Self {
        Self {
            capacity,
            tokens: capacity,
            refill_per_sec,
            last_refill_secs: 0,
        }
    }

    /// Refill the bucket to `capacity` and attempt to withdraw `amount`.
    pub fn try_acquire(&mut self, amount: u32, now_secs: u64) -> bool {
        self.refill(now_secs);
        if self.tokens >= amount {
            self.tokens -= amount;
            true
        } else {
            false
        }
    }

    /// Top the bucket up based on elapsed wall-clock seconds.
    pub fn refill(&mut self, now_secs: u64) {
        let elapsed = now_secs.saturating_sub(self.last_refill_secs) as f32;
        self.tokens =
            (self.tokens as f32 + elapsed * self.refill_per_sec).min(self.capacity as f32) as u32;
        self.last_refill_secs = now_secs;
    }

    pub fn tokens(&self) -> u32 {
        self.tokens
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }
}

// ---------------------------------------------------------------------------
// Token budgets
// ---------------------------------------------------------------------------

/// Per-agent token accounting for the current simulation day and hour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTokenBudget {
    pub agent: u64,
    pub daily_limit: u32,
    pub consumed_today: u32,
    pub hourly_limit: u32,
    pub requests_this_hour: u32,
    pub ai_enabled: bool,
    /// Set after repeated failures; the agent becomes deterministic-only.
    pub deterministic_only: bool,
}

impl AgentTokenBudget {
    pub fn new(agent: u64, daily_limit: u32) -> Self {
        Self {
            agent,
            daily_limit,
            consumed_today: 0,
            hourly_limit: 12,
            requests_this_hour: 0,
            ai_enabled: true,
            deterministic_only: false,
        }
    }

    pub fn can_spend(&self, tokens: u32) -> bool {
        self.ai_enabled
            && !self.deterministic_only
            && self.consumed_today.saturating_add(tokens) <= self.daily_limit
            && self.requests_this_hour < self.hourly_limit
    }

    /// Charge only when the per-agent daily/hourly limits allow it.
    pub fn charge(&mut self, tokens: u32) -> bool {
        if !self.can_spend(tokens) {
            return false;
        }
        self.consumed_today = self.consumed_today.saturating_add(tokens);
        self.requests_this_hour = self.requests_this_hour.saturating_add(1);
        true
    }

    pub fn remaining(&self) -> u32 {
        self.daily_limit.saturating_sub(self.consumed_today)
    }

    pub fn reset_day(&mut self) {
        self.consumed_today = 0;
        self.requests_this_hour = 0;
    }

    pub fn reset_hour(&mut self) {
        self.requests_this_hour = 0;
    }

    /// Force deterministic behaviour after repeated AI failures.
    pub fn mark_deterministic_only(&mut self) {
        self.deterministic_only = true;
    }
}

/// Global plus per-agent token budget (design §9.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    pub global_per_day: u64,
    pub consumed_today: u64,
    pub default_per_agent: u32,
    pub reserve_fraction: f32,
    pub per_agent: HashMap<u64, AgentTokenBudget>,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self::new(1_000_000, 4_000)
    }
}

impl TokenBudget {
    pub fn new(global_per_day: u64, default_per_agent: u32) -> Self {
        Self {
            global_per_day,
            consumed_today: 0,
            default_per_agent,
            reserve_fraction: 0.15,
            per_agent: HashMap::new(),
        }
    }

    pub fn register(&mut self, agent: u64, per_agent_limit: u32) {
        self.per_agent
            .entry(agent)
            .or_insert_with(|| AgentTokenBudget::new(agent, per_agent_limit));
    }

    pub fn agent_budget(&self, agent: u64) -> Option<&AgentTokenBudget> {
        self.per_agent.get(&agent)
    }

    pub fn can_deliberate(&mut self, agent: u64) -> bool {
        let global_ok = self.consumed_today < self.global_per_day;
        let entry = self
            .per_agent
            .entry(agent)
            .or_insert_with(|| AgentTokenBudget::new(agent, self.default_per_agent));
        global_ok && entry.can_spend(1)
    }

    /// Whether an urgent decision may still be served from the reserve.
    pub fn can_deliberate_urgent(&self) -> bool {
        self.consumed_today <= self.global_per_day
    }

    /// Charge one agent and the global budget through a single path. Both the
    /// global cap and the per-agent daily/hourly limits must allow the charge.
    pub fn charge(&mut self, agent: u64, tokens: u32) -> bool {
        if self.consumed_today.saturating_add(tokens as u64) > self.global_per_day {
            return false;
        }
        let limit = self
            .per_agent
            .get(&agent)
            .map_or(self.default_per_agent, |b| b.daily_limit);
        let entry = self
            .per_agent
            .entry(agent)
            .or_insert_with(|| AgentTokenBudget::new(agent, limit));
        if !entry.can_spend(tokens) {
            return false;
        }
        entry.charge(tokens);
        self.consumed_today = self.consumed_today.saturating_add(tokens as u64);
        true
    }

    pub fn reserve_tokens(&self) -> u64 {
        (self.global_per_day as f32 * self.reserve_fraction) as u64
    }

    pub fn reset_day(&mut self) {
        self.consumed_today = 0;
        for budget in self.per_agent.values_mut() {
            budget.reset_day();
        }
    }

    pub fn mark_deterministic_only(&mut self, agent: u64) {
        let limit = self
            .per_agent
            .get(&agent)
            .map_or(self.default_per_agent, |b| b.daily_limit);
        self.per_agent
            .entry(agent)
            .or_insert_with(|| AgentTokenBudget::new(agent, limit))
            .mark_deterministic_only();
    }
}

// ---------------------------------------------------------------------------
// Scheduling
// ---------------------------------------------------------------------------

/// Scheduling configuration (design §9.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    pub prisoner_interval_secs: u64,
    pub guard_interval_secs: u64,
    pub warden_interval_secs: u64,
    /// Random jitter fraction used to desynchronize agents.
    pub jitter: f32,
    pub max_requests_per_tick: u32,
    pub max_concurrent_requests: u32,
    pub allow_while_paused: bool,
    /// Maximum age of an in-flight AI decision before it is stale.
    pub decision_timeout_secs: u64,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            prisoner_interval_secs: 3_600,
            guard_interval_secs: 1_800,
            warden_interval_secs: 14_400,
            jitter: 0.25,
            max_requests_per_tick: 8,
            max_concurrent_requests: 16,
            allow_while_paused: false,
            decision_timeout_secs: 120,
        }
    }
}

impl ScheduleConfig {
    pub fn interval_for(&self, role: Role) -> u64 {
        match role {
            Role::Prisoner => self.prisoner_interval_secs,
            Role::Guard => self.guard_interval_secs,
            Role::Warden => self.warden_interval_secs,
            Role::Unknown => self.prisoner_interval_secs,
        }
    }
}

/// Per-agent AI deliberation schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSchedule {
    pub agent: u64,
    pub role: Role,
    pub interval_secs: u64,
    pub jitter: f32,
    /// Deterministic per-agent phase in `[-jitter, jitter]`.
    pub phase: f32,
    pub last_deliberation_secs: u64,
    pub next_deliberation_secs: u64,
    pub ai_enabled: bool,
    pub deterministic_only: bool,
}

impl AgentSchedule {
    pub fn new(
        agent: u64,
        role: Role,
        interval_secs: u64,
        jitter: f32,
        phase: f32,
        now_secs: u64,
    ) -> Self {
        let delta = (interval_secs as f32 * (1.0 + phase)) as u64;
        Self {
            agent,
            role,
            interval_secs,
            jitter,
            phase,
            last_deliberation_secs: now_secs,
            next_deliberation_secs: now_secs + delta.max(1),
            ai_enabled: true,
            deterministic_only: false,
        }
    }

    pub fn is_due(&self, now_secs: u64) -> bool {
        self.ai_enabled && !self.deterministic_only && now_secs >= self.next_deliberation_secs
    }

    pub fn mark_deliberated(&mut self, now_secs: u64) {
        self.last_deliberation_secs = now_secs;
        let delta = (self.interval_secs as f32 * (1.0 + self.phase)) as u64;
        self.next_deliberation_secs = now_secs + delta.max(1);
    }

    /// Event triggers move the next deliberation forward (design §9.2).
    pub fn trigger_urgent(&mut self, now_secs: u64) {
        self.next_deliberation_secs = now_secs;
    }

    pub fn mark_deterministic_only(&mut self) {
        self.deterministic_only = true;
    }
}

/// Three-tier scheduler shared by prisoners, guards and the warden.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionScheduler {
    pub config: ScheduleConfig,
    pub agents: HashMap<u64, AgentSchedule>,
    #[serde(default)]
    pub requests_this_tick: u32,
    #[serde(default)]
    pub in_flight: u32,
}

impl Default for DecisionScheduler {
    fn default() -> Self {
        Self::new(ScheduleConfig::default())
    }
}

impl DecisionScheduler {
    pub fn new(config: ScheduleConfig) -> Self {
        Self {
            config,
            agents: HashMap::new(),
            requests_this_tick: 0,
            in_flight: 0,
        }
    }

    /// Register an agent and stagger its first deliberation.
    pub fn register(
        &mut self,
        agent: u64,
        role: Role,
        interval_secs: u64,
        now_secs: u64,
        rng: &mut RngStream,
    ) {
        let interval = if interval_secs == 0 {
            self.config.interval_for(role)
        } else {
            interval_secs
        };
        let phase = if self.config.jitter > 0.0 {
            rng.range(
                f64::from(-self.config.jitter),
                f64::from(self.config.jitter),
            ) as f32
        } else {
            0.0
        };
        self.agents.insert(
            agent,
            AgentSchedule::new(agent, role, interval, self.config.jitter, phase, now_secs),
        );
    }

    pub fn unregister(&mut self, agent: u64) {
        self.agents.remove(&agent);
    }

    pub fn is_due(&self, agent: u64, now_secs: u64) -> bool {
        self.agents
            .get(&agent)
            .map_or(false, |s| s.is_due(now_secs))
    }

    pub fn next_deliberation(&self, agent: u64) -> Option<u64> {
        self.agents.get(&agent).map(|s| s.next_deliberation_secs)
    }

    /// Agents due now, earliest-deadline first (design §9.2 prioritization).
    pub fn due_agents(&self, now_secs: u64) -> Vec<u64> {
        let mut due: Vec<(u64, u64)> = self
            .agents
            .values()
            .filter(|s| s.is_due(now_secs))
            .map(|s| (s.next_deliberation_secs, s.agent))
            .collect();
        due.sort();
        due.into_iter().map(|(_, agent)| agent).collect()
    }

    pub fn trigger_urgent(&mut self, agent: u64, now_secs: u64) {
        if let Some(schedule) = self.agents.get_mut(&agent) {
            schedule.trigger_urgent(now_secs);
        }
    }

    pub fn mark_deliberated(&mut self, agent: u64, now_secs: u64) {
        if let Some(schedule) = self.agents.get_mut(&agent) {
            schedule.mark_deliberated(now_secs);
        }
    }

    pub fn mark_deterministic_only(&mut self, agent: u64) {
        if let Some(schedule) = self.agents.get_mut(&agent) {
            schedule.mark_deterministic_only();
        }
    }

    /// Reset per-tick counters; call at the start of every simulation tick.
    pub fn begin_tick(&mut self) {
        self.requests_this_tick = 0;
    }

    /// Try to reserve a request slot for this tick.
    pub fn try_slot(&mut self) -> bool {
        if self.requests_this_tick >= self.config.max_requests_per_tick {
            return false;
        }
        if self.in_flight >= self.config.max_concurrent_requests {
            return false;
        }
        self.requests_this_tick += 1;
        true
    }

    pub fn mark_in_flight(&mut self) {
        self.in_flight = self.in_flight.saturating_add(1);
    }

    pub fn mark_completed(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
    }

    /// Agents currently registered and eligible for AI deliberation.
    pub fn waiting(&self) -> usize {
        self.agents
            .values()
            .filter(|s| s.ai_enabled && !s.deterministic_only)
            .count()
    }
}

// ---------------------------------------------------------------------------
// Tier selection
// ---------------------------------------------------------------------------

/// Decision tier (design §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionTier {
    Deterministic,
    Utility,
    Ai,
}

/// Which engine produced a decision (design §9.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineKind {
    Ai,
    Utility,
    Deterministic,
    Fallback,
}

/// Inputs to tier selection (design §6.3).
#[derive(Debug, Clone, Copy, Default)]
pub struct TierPolicy {
    pub urgent_event: bool,
    pub ai_budget_available: bool,
    pub ai_enabled: bool,
    pub ai_due: bool,
    pub routine: bool,
    pub utility_variance: f32,
    pub routine_variance_threshold: f32,
}

/// Select the decision tier for a context.
pub fn select_tier(policy: &TierPolicy) -> DecisionTier {
    if policy.urgent_event && policy.ai_budget_available && policy.ai_enabled {
        return DecisionTier::Ai;
    }
    if policy.routine && policy.utility_variance < policy.routine_variance_threshold {
        return DecisionTier::Utility;
    }
    if policy.ai_due && policy.ai_budget_available && policy.ai_enabled {
        DecisionTier::Ai
    } else {
        DecisionTier::Utility
    }
}

// ---------------------------------------------------------------------------
// Deterministic fallback utility chooser
// ---------------------------------------------------------------------------

/// Ranks action candidates by risk- and legality-adjusted utility. Used when
/// AI slots, budgets, or providers are unavailable (design §9.7, §9.5).
#[derive(Debug, Clone, Copy)]
pub struct UtilityChooser {
    pub risk_aversion: f32,
    pub legality_weight: f32,
}

impl Default for UtilityChooser {
    fn default() -> Self {
        Self {
            risk_aversion: 0.5,
            legality_weight: 0.2,
        }
    }
}

impl UtilityChooser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn score(&self, candidate: &ActionCandidate) -> f32 {
        let legality = match candidate.legality {
            Legality::Legal => self.legality_weight,
            Legality::IllegalButPossible => 0.0,
            Legality::Forbidden => f32::NEG_INFINITY,
        };
        candidate.expected_utility - self.risk_aversion * candidate.risk + legality
    }

    pub fn choose(&self, candidates: &[ActionCandidate]) -> Option<Decision> {
        candidates
            .iter()
            .filter(|c| c.legality != Legality::Forbidden)
            .max_by(|a, b| self.score(a).total_cmp(&self.score(b)))
            .map(|c| c.decision.clone())
    }

    /// Score a candidate including its personality-driven bias (Directive §3).
    /// With a [`Disposition::default`] this is identical to [`Self::score`].
    pub fn score_with_disposition(
        &self,
        candidate: &ActionCandidate,
        disposition: &Disposition,
    ) -> f32 {
        self.score(candidate) + trait_bias(disposition, &candidate.decision)
    }

    /// Rank candidates using both need-derived utility and personality.
    pub fn choose_with_disposition(
        &self,
        candidates: &[ActionCandidate],
        disposition: &Disposition,
    ) -> Option<Decision> {
        candidates
            .iter()
            .filter(|c| c.legality != Legality::Forbidden)
            .max_by(|a, b| {
                self.score_with_disposition(a, disposition)
                    .total_cmp(&self.score_with_disposition(b, disposition))
            })
            .map(|c| c.decision.clone())
    }
}

/// Deterministic fallback decision: highest adjusted utility, or
/// [`Decision::None`] when no candidate is viable.
pub fn fallback_utility_decision(candidates: &[ActionCandidate]) -> Decision {
    if candidates.is_empty() {
        return Decision::new(DecisionAction::None);
    }
    UtilityChooser::default()
        .choose(candidates)
        .unwrap_or_else(|| Decision::new(DecisionAction::None))
}

/// Personality-aware fallback: same deterministic ranking, with trait biases
/// applied so discipline/aggression/sociability measurably change behavior.
pub fn fallback_utility_decision_with_disposition(
    candidates: &[ActionCandidate],
    disposition: &Disposition,
) -> Decision {
    UtilityChooser::default()
        .choose_with_disposition(candidates, disposition)
        .unwrap_or_else(|| Decision::new(DecisionAction::None))
}

// ---------------------------------------------------------------------------
// Pending decisions and stale revalidation
// ---------------------------------------------------------------------------

/// An AI decision that has been requested but not yet applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDecision {
    pub agent: u64,
    pub requested_at_secs: u64,
    pub decision: Decision,
    pub context_hash: u64,
}

impl PendingDecision {
    pub fn new(agent: u64, requested_at_secs: u64, decision: Decision, context_hash: u64) -> Self {
        Self {
            agent,
            requested_at_secs,
            decision,
            context_hash,
        }
    }

    pub fn age_secs(&self, now_secs: u64) -> u64 {
        now_secs.saturating_sub(self.requested_at_secs)
    }

    pub fn is_stale(&self, now_secs: u64, timeout_secs: u64) -> bool {
        self.age_secs(now_secs) > timeout_secs
    }
}

/// Re-validate a pending decision against the current world state. Stale
/// decisions are discarded; fresh ones are revalidated through the
/// eight-stage pipeline (design §9.3).
pub fn revalidate_pending(
    pending: &PendingDecision,
    ctx: &ValidationContext,
    now_secs: u64,
    timeout_secs: u64,
) -> ValidationResult {
    if pending.is_stale(now_secs, timeout_secs) {
        return ValidationResult::Rejected {
            reason: "stale decision".to_string(),
        };
    }
    if pending.context_hash != 0 && pending.context_hash != hash_validation_context(ctx) {
        return ValidationResult::Rejected {
            reason: "stale context".to_string(),
        };
    }
    let raw = RawDecision::from_decision(&pending.decision);
    run_validation_pipeline(&raw, ctx)
}

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Structured orchestration telemetry (design §9.8).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrchestratorTelemetry {
    pub ai_decisions: u64,
    pub utility_decisions: u64,
    pub deterministic_decisions: u64,
    pub fallback_decisions: u64,
    pub rejected_decisions: u64,
    pub stale_decisions: u64,
    pub tokens_consumed_today: u64,
}

/// Runtime-only counters added by the AI orchestration policy. They are kept
/// out of the persisted telemetry struct so the v5 bincode layout is unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiRuntimeTelemetry {
    pub provider_calls: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub retries: u64,
    pub rate_limited: u64,
    pub provider_failures: u64,
    pub fallback_after_failure: u64,
    pub batches: u64,
    pub decisions_batched: u64,
    pub validated_decisions: u64,
    pub accepted_decisions: u64,
    pub repaired_decisions: u64,
}

/// Observer-facing snapshot of the configured orchestration policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiRuntimeSnapshot {
    pub provider: String,
    pub model: String,
    pub max_tokens_per_day: u64,
    pub max_tokens_per_agent_per_day: u32,
    pub tokens_consumed_today: u64,
    pub rate_limit_per_sec: u32,
    pub rate_burst: u32,
    pub rate_tokens: u32,
    pub max_retries: u32,
    pub cache_enabled: bool,
    pub cache_entries: usize,
    pub max_cache_entries: usize,
    pub cache_ttl_secs: u64,
    pub max_batch_size: usize,
    pub decision_timeout_secs: u64,
    pub ai_decisions: u64,
    pub utility_decisions: u64,
    pub fallback_decisions: u64,
    pub rejected_decisions: u64,
    pub stale_decisions: u64,
    pub provider_calls: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub retries: u64,
    pub rate_limited: u64,
    pub provider_failures: u64,
    pub batches: u64,
    pub decisions_batched: u64,
    pub validated_decisions: u64,
    pub accepted_decisions: u64,
    pub repaired_decisions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CacheKey {
    provider: String,
    model: String,
    agent: u64,
    context_hash: u64,
}

#[derive(Debug, Clone)]
struct CachedDecision {
    decision: Decision,
    created_at_secs: u64,
}

/// Runtime policy state. `#[serde(skip)]` on the owning field preserves the
/// persisted v5 layout; configuration is re-applied on startup/load.
#[derive(Debug, Clone)]
pub struct AiRuntimeState {
    pub config: AiConfig,
    pub provider: DecisionProviderKind,
    pub telemetry: AiRuntimeTelemetry,
    cache: BTreeMap<CacheKey, CachedDecision>,
}

impl Default for AiRuntimeState {
    fn default() -> Self {
        Self::new(&AiConfig::default())
    }
}

impl AiRuntimeState {
    pub fn new(config: &AiConfig) -> Self {
        Self {
            config: config.clone(),
            provider: DecisionProviderKind::from_config(config),
            telemetry: AiRuntimeTelemetry::default(),
            cache: BTreeMap::new(),
        }
    }

    pub fn configure(&mut self, config: &AiConfig) {
        self.provider = DecisionProviderKind::from_config(config);
        self.config = config.clone();
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }
}

/// Hybrid three-tier orchestrator.
///
/// The original public fields and methods (`token_budget_per_agent`,
/// `consumed_today`, `rate`, `may_deliberate`, `charge`, `reset_day`) are
/// preserved; per-agent budgets, scheduling, pending decisions and telemetry
/// are additive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Orchestrator {
    pub token_budget_per_agent: u32,
    pub consumed_today: u32,
    pub rate: RateLimiter,
    #[serde(default)]
    pub budget: TokenBudget,
    #[serde(default)]
    pub scheduler: DecisionScheduler,
    #[serde(default)]
    pub pending: Vec<PendingDecision>,
    #[serde(default)]
    pub telemetry: OrchestratorTelemetry,
    /// Runtime-only provider/cache/config state (not part of save format v5).
    #[serde(skip)]
    pub runtime: AiRuntimeState,
}

impl Orchestrator {
    pub fn new(tokens_per_agent: u32) -> Self {
        let config = AiConfig {
            max_tokens_per_agent_per_day: tokens_per_agent,
            ..AiConfig::default()
        };
        Self {
            token_budget_per_agent: tokens_per_agent,
            consumed_today: 0,
            rate: RateLimiter::new(config.rate_burst, config.rate_limit_per_sec as f32),
            budget: TokenBudget::new(config.max_tokens_per_day, tokens_per_agent),
            scheduler: DecisionScheduler::new(ScheduleConfig::default()),
            pending: Vec::new(),
            telemetry: OrchestratorTelemetry::default(),
            runtime: AiRuntimeState::new(&config),
        }
    }

    /// Build an orchestrator whose provider, budgets and limits come from
    /// `AiConfig` rather than the legacy single-token constructor.
    pub fn from_config(config: &AiConfig) -> Self {
        let mut orchestrator = Self::new(config.max_tokens_per_agent_per_day);
        orchestrator.configure(config);
        orchestrator
    }

    /// Re-apply runtime configuration to an existing (possibly loaded)
    /// orchestrator without changing its persisted state.
    pub fn configure(&mut self, config: &AiConfig) {
        self.token_budget_per_agent = config.max_tokens_per_agent_per_day;
        self.budget.global_per_day = config.max_tokens_per_day;
        self.budget.default_per_agent = config.max_tokens_per_agent_per_day;
        self.rate = RateLimiter::new(config.rate_burst, config.rate_limit_per_sec as f32);
        self.scheduler.config.prisoner_interval_secs =
            config.decision_interval_mins.saturating_mul(60);
        self.scheduler.config.decision_timeout_secs = config.decision_timeout_secs;
        self.runtime.configure(config);
    }

    /// Returns true when this agent may take an AI deliberation slot this tick.
    /// Preserved original semantics.
    pub fn may_deliberate(&mut self, now_secs: u64) -> bool {
        self.consumed_today < self.token_budget_per_agent && self.rate.try_acquire(1, now_secs)
    }

    pub fn charge(&mut self, tokens: u32) {
        self.consumed_today = self.consumed_today.saturating_add(tokens);
        self.telemetry.tokens_consumed_today = self
            .telemetry
            .tokens_consumed_today
            .saturating_add(tokens as u64);
    }

    pub fn reset_day(&mut self) {
        self.consumed_today = 0;
        self.telemetry.tokens_consumed_today = 0;
        self.budget.reset_day();
    }

    /// Register an agent with the scheduler. `interval_secs == 0` uses the
    /// role default from [`ScheduleConfig`].
    pub fn register_agent(
        &mut self,
        agent: u64,
        role: Role,
        interval_secs: u64,
        now_secs: u64,
        rng: &mut RngStream,
    ) {
        self.scheduler
            .register(agent, role, interval_secs, now_secs, rng);
        let limit = self
            .budget
            .agent_budget(agent)
            .map_or(self.token_budget_per_agent, |b| b.daily_limit);
        self.budget.register(agent, limit);
    }

    /// Agents currently due for AI deliberation.
    pub fn due_agents(&self, now_secs: u64) -> Vec<u64> {
        self.scheduler.due_agents(now_secs)
    }

    /// Start a tick and return the agents granted an AI slot, respecting
    /// per-tick request limits, concurrency, and per-agent token budgets.
    pub fn take_ai_slots(&mut self, now_secs: u64) -> Vec<u64> {
        self.scheduler.begin_tick();
        let due = self.scheduler.due_agents(now_secs);
        let max = self.scheduler.config.max_requests_per_tick as usize;
        let mut granted = Vec::new();
        for agent in due {
            if granted.len() >= max {
                break;
            }
            if !self.scheduler.try_slot() {
                break;
            }
            if !self.budget.can_deliberate(agent) {
                self.scheduler.mark_completed();
                self.telemetry.fallback_decisions =
                    self.telemetry.fallback_decisions.saturating_add(1);
                continue;
            }
            self.scheduler.mark_in_flight();
            self.scheduler.mark_deliberated(agent, now_secs);
            granted.push(agent);
        }
        debug!(granted = granted.len(), "AI slots granted");
        granted
    }

    /// Convenience entry point for the tick loop.
    pub fn on_tick(&mut self, now_secs: u64) -> Vec<u64> {
        self.take_ai_slots(now_secs)
    }

    /// Record an in-flight AI decision.
    pub fn submit_pending(&mut self, pending: PendingDecision) {
        self.pending.push(pending);
    }

    /// Ask the configured provider for a raw proposal.
    ///
    /// This path enforces rate limits, per-agent/global token budgets, retry
    /// policy and the bounded cache. The returned decision is still
    /// *unvalidated*: callers must pass it through the engine gate.
    pub fn propose(&mut self, request: DecisionRequest, now_secs: u64) -> Option<Decision> {
        let config = self.runtime.config.clone();
        let key = CacheKey {
            provider: self.runtime.provider.name().to_string(),
            model: config.model.clone(),
            agent: request.agent,
            context_hash: request.context_hash,
        };

        if config.cache_enabled {
            if let Some(decision) = self.cached_decision(&key, now_secs, config.cache_ttl_secs) {
                self.runtime.telemetry.cache_hits += 1;
                return Some(decision);
            }
            self.runtime.telemetry.cache_misses += 1;
        }

        if !self.rate.try_acquire(1, now_secs) {
            self.runtime.telemetry.rate_limited += 1;
            return None;
        }

        for attempt in 0..=config.max_retries {
            if attempt > 0 {
                self.runtime.telemetry.retries += 1;
            }
            let outcome = self.runtime.provider.propose(&request);
            self.runtime.telemetry.provider_calls += 1;
            if let Some(failure) = outcome.failure {
                self.runtime.telemetry.provider_failures += 1;
                if failure.retryable && attempt < config.max_retries {
                    continue;
                }
                return None;
            }
            let Some(raw) = outcome.raw else {
                return None;
            };
            let parsed = match validate_format(&raw) {
                Ok(decision) => decision,
                Err(rejection) => {
                    self.telemetry.rejected_decisions += 1;
                    if rejection.retryable && attempt < config.max_retries {
                        continue;
                    }
                    return None;
                }
            };
            let cost = outcome.tokens.max(1);
            if !self.budget.charge(request.agent, cost) {
                self.runtime.telemetry.fallback_after_failure += 1;
                return None;
            }
            self.consumed_today = self.consumed_today.saturating_add(cost);
            self.telemetry.tokens_consumed_today = self
                .telemetry
                .tokens_consumed_today
                .saturating_add(u64::from(cost));
            if config.cache_enabled {
                self.cache_decision(key, parsed.clone(), now_secs, config.max_cache_entries);
            }
            self.telemetry.ai_decisions += 1;
            return Some(parsed);
        }
        self.runtime.telemetry.fallback_after_failure += 1;
        None
    }

    fn cached_decision(&mut self, key: &CacheKey, now_secs: u64, ttl: u64) -> Option<Decision> {
        let expired = self.runtime.cache.get(key).map_or(false, |entry| {
            now_secs.saturating_sub(entry.created_at_secs) > ttl
        });
        if expired {
            self.runtime.cache.remove(key);
            return None;
        }
        self.runtime
            .cache
            .get(key)
            .map(|entry| entry.decision.clone())
    }

    fn cache_decision(
        &mut self,
        key: CacheKey,
        decision: Decision,
        now_secs: u64,
        max_entries: usize,
    ) {
        self.runtime.cache.insert(
            key,
            CachedDecision {
                decision,
                created_at_secs: now_secs,
            },
        );
        let cap = max_entries.max(1);
        while self.runtime.cache.len() > cap {
            let oldest = self.runtime.cache.keys().next().cloned();
            match oldest {
                Some(key) => {
                    self.runtime.cache.remove(&key);
                }
                None => break,
            }
        }
    }

    /// Record a deterministic/utility fallback after AI declined or failed.
    pub fn record_fallback(&mut self) {
        self.telemetry.fallback_decisions += 1;
    }

    /// Record one canonical gate outcome.
    pub fn record_validated(&mut self, repaired: bool) {
        self.runtime.telemetry.validated_decisions += 1;
        if repaired {
            self.runtime.telemetry.repaired_decisions += 1;
        } else {
            self.runtime.telemetry.accepted_decisions += 1;
        }
    }

    pub fn record_rejected(&mut self) {
        self.telemetry.rejected_decisions += 1;
    }

    pub fn record_batch(&mut self, count: usize) {
        self.runtime.telemetry.batches += 1;
        self.runtime.telemetry.decisions_batched = self
            .runtime
            .telemetry
            .decisions_batched
            .saturating_add(count as u64);
    }

    /// Drop all runtime state belonging to agents that left the institution.
    pub fn prune_departed(&mut self, departed: &[u64]) {
        for id in departed {
            self.scheduler.unregister(*id);
            self.budget.per_agent.remove(id);
            self.pending.retain(|p| p.agent != *id);
            self.runtime.cache.retain(|key, _| key.agent != *id);
        }
    }

    /// Snapshot the configured policy and all counters for observer DTOs.
    pub fn runtime_snapshot(&self) -> AiRuntimeSnapshot {
        let config = &self.runtime.config;
        AiRuntimeSnapshot {
            provider: self.runtime.provider.name().to_string(),
            model: config.model.clone(),
            max_tokens_per_day: config.max_tokens_per_day,
            max_tokens_per_agent_per_day: config.max_tokens_per_agent_per_day,
            tokens_consumed_today: self.telemetry.tokens_consumed_today,
            rate_limit_per_sec: config.rate_limit_per_sec,
            rate_burst: config.rate_burst,
            rate_tokens: self.rate.tokens(),
            max_retries: config.max_retries,
            cache_enabled: config.cache_enabled,
            cache_entries: self.runtime.cache_len(),
            max_cache_entries: config.max_cache_entries,
            cache_ttl_secs: config.cache_ttl_secs,
            max_batch_size: config.max_batch_size,
            decision_timeout_secs: config.decision_timeout_secs,
            ai_decisions: self.telemetry.ai_decisions,
            utility_decisions: self.telemetry.utility_decisions,
            fallback_decisions: self.telemetry.fallback_decisions,
            rejected_decisions: self.telemetry.rejected_decisions,
            stale_decisions: self.telemetry.stale_decisions,
            provider_calls: self.runtime.telemetry.provider_calls,
            cache_hits: self.runtime.telemetry.cache_hits,
            cache_misses: self.runtime.telemetry.cache_misses,
            retries: self.runtime.telemetry.retries,
            rate_limited: self.runtime.telemetry.rate_limited,
            provider_failures: self.runtime.telemetry.provider_failures,
            batches: self.runtime.telemetry.batches,
            decisions_batched: self.runtime.telemetry.decisions_batched,
            validated_decisions: self.runtime.telemetry.validated_decisions,
            accepted_decisions: self.runtime.telemetry.accepted_decisions,
            repaired_decisions: self.runtime.telemetry.repaired_decisions,
        }
    }

    /// Remove and return decisions that exceeded the decision timeout.
    pub fn reap_stale(&mut self, now_secs: u64) -> Vec<PendingDecision> {
        let timeout = self.scheduler.config.decision_timeout_secs;
        let mut stale = Vec::new();
        self.pending.retain(|p| {
            if p.is_stale(now_secs, timeout) {
                stale.push(p.clone());
                false
            } else {
                true
            }
        });
        self.telemetry.stale_decisions = self
            .telemetry
            .stale_decisions
            .saturating_add(stale.len() as u64);
        stale
    }

    /// Re-validate a pending decision against a fresh context.
    pub fn revalidate(
        &mut self,
        pending: &PendingDecision,
        ctx: &ValidationContext,
        now_secs: u64,
    ) -> ValidationResult {
        let timeout = self.scheduler.config.decision_timeout_secs;
        revalidate_pending(pending, ctx, now_secs, timeout)
    }

    /// Deterministic fallback when AI is unavailable.
    pub fn fallback_decision(&mut self, candidates: &[ActionCandidate]) -> Decision {
        let decision = fallback_utility_decision(candidates);
        self.telemetry.utility_decisions = self.telemetry.utility_decisions.saturating_add(1);
        decision
    }

    /// Deterministic fallback that also honours the agent's personality.
    pub fn fallback_decision_with_disposition(
        &mut self,
        candidates: &[ActionCandidate],
        disposition: &Disposition,
    ) -> Decision {
        let decision = fallback_utility_decision_with_disposition(candidates, disposition);
        self.telemetry.utility_decisions = self.telemetry.utility_decisions.saturating_add(1);
        decision
    }

    /// Record that an agent is now deterministic-only after repeated failures.
    pub fn mark_deterministic_only(&mut self, agent: u64) {
        self.scheduler.mark_deterministic_only(agent);
        self.budget.mark_deterministic_only(agent);
        debug!(agent, "agent marked deterministic-only");
    }

    pub fn telemetry(&self) -> &OrchestratorTelemetry {
        &self.telemetry
    }

    /// Current queue depth (agents waiting for a slot).
    pub fn queue_depth(&self) -> usize {
        self.scheduler.waiting()
    }
}

/// Helper used by callers that need a rejection without a full context.
pub fn rejection_message(rejection: &Rejection) -> String {
    rejection.message()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::RawTarget;

    fn candidate(decision: Decision, utility: f32) -> ActionCandidate {
        ActionCandidate::new(decision, utility)
    }

    #[test]
    fn rate_limiter_refills() {
        let mut limiter = RateLimiter::new(2, 1.0);
        assert!(limiter.try_acquire(1, 0));
        assert!(limiter.try_acquire(1, 0));
        assert!(!limiter.try_acquire(1, 0));
        assert!(limiter.try_acquire(1, 2));
    }

    #[test]
    fn budget_blocks_when_exhausted() {
        let mut budget = TokenBudget::new(10, 2);
        budget.register(1, 2);
        assert!(budget.can_deliberate(1));
        assert!(budget.charge(1, 2));
        assert!(!budget.can_deliberate(1));
    }

    #[test]
    fn scheduler_staggers_and_limits() {
        let mut rng = RngStream::new(42, 1);
        let mut scheduler = DecisionScheduler::new(ScheduleConfig {
            max_requests_per_tick: 1,
            jitter: 0.0,
            ..Default::default()
        });
        scheduler.register(1, Role::Prisoner, 60, 0, &mut rng);
        scheduler.register(2, Role::Prisoner, 60, 0, &mut rng);
        assert!(scheduler.due_agents(30).is_empty());
        assert_eq!(scheduler.due_agents(60).len(), 2);
        scheduler.begin_tick();
        let slots: Vec<u64> = (0..2).filter(|_| scheduler.try_slot()).collect();
        assert_eq!(slots.len(), 1);
    }

    #[test]
    fn tier_selection_prefers_ai_for_urgent_events() {
        let policy = TierPolicy {
            urgent_event: true,
            ai_budget_available: true,
            ai_enabled: true,
            ..Default::default()
        };
        assert_eq!(select_tier(&policy), DecisionTier::Ai);
        let routine = TierPolicy {
            routine: true,
            utility_variance: 0.01,
            ..Default::default()
        };
        assert_eq!(select_tier(&routine), DecisionTier::Utility);
    }

    #[test]
    fn fallback_chooser_ranks_by_utility() {
        let candidates = vec![
            candidate(Decision::new(DecisionAction::Comply), 0.2),
            candidate(Decision::new(DecisionAction::Socialize { with: 3 }), 0.8),
            candidate(Decision::new(DecisionAction::Fight { target: 3 }), 0.9)
                .with_legality(Legality::IllegalButPossible),
        ];
        assert_eq!(
            fallback_utility_decision(&candidates),
            Decision::new(DecisionAction::Socialize { with: 3 })
        );
    }

    #[test]
    fn discipline_trait_flips_routine_decisions() {
        use crate::disposition::Disposition;
        let disciplined = Disposition {
            discipline: 0.95,
            emotional_stability: 0.9,
            aggression: 0.3,
            impulsiveness: 0.2,
            ..Default::default()
        };
        let loose = Disposition {
            discipline: 0.05,
            emotional_stability: 0.2,
            aggression: 0.9,
            impulsiveness: 0.9,
            ..Default::default()
        };
        let mut hi_compliant = 0;
        let mut lo_defiant = 0;
        for hour in 0..24u32 {
            // Needs drift a little through the day; the trait effect is the
            // only difference between the two agents' candidate sets.
            let jitter = (hour % 5) as f32 * 0.005;
            let candidates = vec![
                candidate(Decision::new(DecisionAction::Comply), 0.35),
                candidate(Decision::new(DecisionAction::Refuse), 0.40 + jitter)
                    .with_legality(Legality::IllegalButPossible),
                candidate(Decision::new(DecisionAction::Fight { target: 9 }), 0.38)
                    .with_risk(0.1)
                    .with_legality(Legality::IllegalButPossible),
                candidate(Decision::new(DecisionAction::None), 0.05),
            ];
            let hi = fallback_utility_decision_with_disposition(&candidates, &disciplined);
            let lo = fallback_utility_decision_with_disposition(&candidates, &loose);
            assert!(
                matches!(hi.action, DecisionAction::Comply),
                "hour {hour}: disciplined picked {hi:?}"
            );
            assert!(
                matches!(lo.action, DecisionAction::Refuse),
                "hour {hour}: undisciplined picked {lo:?}"
            );
            hi_compliant += 1;
            lo_defiant += 1;
        }
        assert_eq!((hi_compliant, lo_defiant), (24, 24));
    }

    #[test]
    fn stale_pending_is_rejected() {
        let pending = PendingDecision::new(1, 100, Decision::new(DecisionAction::Comply), 0);
        assert!(pending.is_stale(200, 50));
        assert!(!pending.is_stale(120, 50));
        let ctx = ValidationContext::default();
        match revalidate_pending(&pending, &ctx, 200, 50) {
            ValidationResult::Rejected { .. } => {}
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn orchestrator_keeps_legacy_budget_api() {
        let mut orch = Orchestrator::new(100);
        assert!(orch.may_deliberate(0));
        orch.charge(10);
        assert_eq!(orch.consumed_today, 10);
        orch.reset_day();
        assert_eq!(orch.consumed_today, 0);
    }

    #[test]
    fn raw_decision_maps_to_catalog() {
        let raw = RawDecision {
            action: "spread_rumor".into(),
            target: Some(RawTarget {
                kind: "agent".into(),
                id: 4,
            }),
            parameters: [("content".to_string(), "guards are coming".to_string())].into(),
            ..Default::default()
        };
        let decision = crate::decision::validate_format(&raw).unwrap();
        assert_eq!(
            decision,
            Decision::new(DecisionAction::SpreadRumor {
                about: 4,
                content: "guards are coming".into()
            })
        );
    }
}
