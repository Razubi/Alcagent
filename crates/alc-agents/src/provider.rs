//! Deterministic decision providers (Directive §19).
//!
//! No provider in this build performs network inference. The local provider
//! deterministically ranks the bounded candidates already present in the
//! canonical context; unsupported providers/models return a retryable=false
//! failure so the orchestrator can degrade to the utility tier.

use alc_core::config::AiConfig;
use serde::{Deserialize, Serialize};

use crate::context::DecisionRequest;
use crate::decision::{RawDecision, RawTarget};

/// A configured provider implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionProviderKind {
    /// The bundled deterministic provider.
    LocalDeterministic,
    /// An unsupported provider/model id; calls fail gracefully.
    Unsupported { provider: String, model: String },
    /// Deterministic failure source used by policy tests.
    AlwaysFail { retryable: bool },
}

impl Default for DecisionProviderKind {
    fn default() -> Self {
        Self::LocalDeterministic
    }
}

impl DecisionProviderKind {
    /// Resolve a provider from config. Unknown ids degrade rather than panic
    /// or attempting network access.
    pub fn from_config(config: &AiConfig) -> Self {
        let provider = config.provider.trim().to_ascii_lowercase();
        let model = config.model.trim().to_ascii_lowercase();
        let provider_ok = matches!(
            provider.as_str(),
            "" | "local" | "local-deterministic" | "deterministic"
        );
        let model_ok = matches!(
            model.as_str(),
            "" | "default" | "deterministic" | "local-deterministic"
        );
        if provider == "always-fail" {
            return Self::AlwaysFail { retryable: false };
        }
        if provider_ok && model_ok {
            Self::LocalDeterministic
        } else {
            Self::Unsupported {
                provider: config.provider.clone(),
                model: config.model.clone(),
            }
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::LocalDeterministic => "local-deterministic",
            Self::Unsupported { .. } => "unsupported",
            Self::AlwaysFail { .. } => "always-fail",
        }
    }

    /// Produce a raw, unvalidated proposal. Validation always happens later in
    /// the canonical gate.
    pub fn propose(&self, request: &DecisionRequest) -> ProviderOutcome {
        match self {
            Self::LocalDeterministic => {
                let best = request
                    .candidates
                    .iter()
                    .max_by(|a, b| {
                        a.adjusted_utility()
                            .total_cmp(&b.adjusted_utility())
                            .then_with(|| a.decision.action_name().cmp(b.decision.action_name()))
                            .then_with(|| {
                                a.decision
                                    .target_id()
                                    .unwrap_or(u64::MAX)
                                    .cmp(&b.decision.target_id().unwrap_or(u64::MAX))
                            })
                    })
                    .map(|c| c.decision.clone());
                let tokens = 32_u32.saturating_add((request.epistemic.len() as u32) * 4);
                match best {
                    Some(decision) => {
                        let mut raw = RawDecision::from_decision(&decision);
                        raw.reasoning = format!(
                            "deterministic ranking; context {} memories",
                            request.epistemic.len()
                        );
                        raw.confidence = 0.75;
                        ProviderOutcome {
                            raw: Some(raw),
                            failure: None,
                            tokens,
                        }
                    }
                    None => {
                        let mut raw = RawDecision::default();
                        raw.action = "none".to_string();
                        ProviderOutcome {
                            raw: Some(raw),
                            failure: None,
                            tokens,
                        }
                    }
                }
            }
            Self::Unsupported { provider, model } => ProviderOutcome {
                raw: None,
                failure: Some(ProviderFailure {
                    reason: format!("unsupported provider '{}' model '{}'", provider, model),
                    retryable: false,
                }),
                tokens: 0,
            },
            Self::AlwaysFail { retryable } => ProviderOutcome {
                raw: None,
                failure: Some(ProviderFailure {
                    reason: "deterministic provider failure".to_string(),
                    retryable: *retryable,
                }),
                tokens: 1,
            },
        }
    }
}

/// Why a provider could not return a proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFailure {
    pub reason: String,
    pub retryable: bool,
}

/// Raw provider result. `raw` is never applied directly; the orchestrator
/// parses it and the engine validates it through the canonical gate.
#[derive(Debug, Clone)]
pub struct ProviderOutcome {
    pub raw: Option<RawDecision>,
    pub failure: Option<ProviderFailure>,
    pub tokens: u32,
}

/// Build a raw decision from an action name and optional agent target. Used by
/// the local provider and by tests that emulate model output.
pub fn raw_decision(action: &str, target: Option<u64>) -> RawDecision {
    RawDecision {
        action: action.to_string(),
        target: target.map(|id| RawTarget {
            kind: "agent".to_string(),
            id,
        }),
        ..RawDecision::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{DecisionRequest, EpistemicContext};
    use crate::decision::{ActionCandidate, Decision, DecisionAction, Role, ValidationContext};

    fn request() -> DecisionRequest {
        DecisionRequest::new(
            1,
            Role::Prisoner,
            0,
            vec![ActionCandidate::new(
                Decision::new(DecisionAction::Comply),
                0.9,
            )],
            ValidationContext::default(),
            EpistemicContext::default(),
        )
    }

    #[test]
    fn local_provider_ranks_candidates() {
        let outcome = DecisionProviderKind::LocalDeterministic.propose(&request());
        let raw = outcome.raw.expect("local provider returned no decision");
        assert_eq!(raw.action, "comply");
        assert!(outcome.failure.is_none());
    }

    #[test]
    fn unsupported_config_degrades_without_panicking() {
        let mut config = AiConfig::default();
        config.provider = "remote-unknown".to_string();
        let provider = DecisionProviderKind::from_config(&config);
        assert!(matches!(provider, DecisionProviderKind::Unsupported { .. }));
        let outcome = provider.propose(&request());
        assert!(outcome.raw.is_none());
        assert_eq!(outcome.failure.expect("missing failure").retryable, false);
    }
}
