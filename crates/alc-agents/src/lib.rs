//! AI orchestration layer (Directive §19).
//!
//! Hybrid architecture: deterministic simulation for routine matters,
//! utility-based decisions for frequent choices, periodic AI deliberation
//! for complex reasoning. Agents emit structured [`Decision`]s that the
//! simulation engine validates before application — agents never mutate
//! world state directly.
//!
//! Modules:
//!
//! * [`decision`] — the [`Decision`] action set and the eight-stage
//!   validation pipeline.
//! * [`disposition`] — personality → utility bias so traits (§3) measurably
//!   shape routine decisions instead of being decorative.
//! * [`memory`] — the four-layer memory system, importance scoring, decay,
//!   consolidation, and ranked recall.
//! * [`orchestrator`] — the three-tier scheduler, token budgets, rate
//!   limiting, stale-decision revalidation, and the deterministic fallback
//!   utility chooser.

pub mod context;
pub mod decision;
pub mod disposition;
pub mod memory;
pub mod orchestrator;
pub mod provider;

pub use context::{
    ContextMemory, DecisionRequest, EpistemicContext, ProviderContext, context_hash,
    hash_validation_context,
};
pub use decision::{
    ACTION_CATALOG, ActionCandidate, Decision, DecisionAction, DecisionValidation,
    DecisionValidator, DefaultValidator, Legality, RawDecision, RawTarget, Rejection, Role,
    ValidationContext, ValidationResult, ValidationStage, clamp_and_repair, normalize_action,
    parse_raw_decision, run_validation_json, run_validation_pipeline,
    run_validation_pipeline_detailed, validate_authority, validate_decision,
    validate_decision_detailed, validate_format, validate_legality, validate_resources,
    validate_safety, validate_semantic, validate_world_state,
};
pub use disposition::{Disposition, trait_bias};
pub use memory::{
    EpistemicKind, EpistemicRecord, ImportanceInput, ImportanceWeights, LayeredMemory, Memory,
    MemoryConfig, MemoryKind, MemoryLayer, MemoryQuery, MemoryRecord, MemoryStore, RankedEpistemic,
    RankedMemory, score_importance,
};
pub use orchestrator::{
    AgentSchedule, AgentTokenBudget, AiRuntimeSnapshot, AiRuntimeState, AiRuntimeTelemetry,
    DecisionScheduler, DecisionTier, EngineKind, Orchestrator, OrchestratorTelemetry,
    PendingDecision, RateLimiter, ScheduleConfig, TierPolicy, TokenBudget, UtilityChooser,
    fallback_utility_decision, fallback_utility_decision_with_disposition, revalidate_pending,
    select_tier,
};
pub use provider::{DecisionProviderKind, ProviderFailure, ProviderOutcome, raw_decision};
