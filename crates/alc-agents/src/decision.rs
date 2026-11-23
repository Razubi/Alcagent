//! Structured decisions and the eight-stage validation pipeline.
//!
//! Agents propose [`Decision`] values; the simulation engine validates them
//! through the pipeline before application. Agents never mutate world state
//! directly (Directive §19; design §8). The pipeline is exposed both as one
//! shot [`run_validation_pipeline`] and as individual stage functions so the
//! engine (or tests) can run, inspect, or repair individual stages.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Canonical action catalog (design §6.5, Appendix A).
///
/// The model may only choose actions from this catalog; anything else is a
/// format/semantic rejection.
pub const ACTION_CATALOG: &[&str] = &[
    "move_to",
    "socialize",
    "fight",
    "trade",
    "comply",
    "refuse",
    "escape_attempt",
    "request_medical",
    "join_gang",
    "leave_gang",
    "lead_gang",
    "spread_rumor",
    "none",
];

/// Maximum length of a free-text field carried by a decision.
pub const MAX_FREE_TEXT: usize = 500;

/// Structured decision emitted by an agent. Validated by the engine before it
/// is converted into commands. This is the complete action set from the
/// design's action catalog subset owned by this crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    MoveTo {
        target_facility: u64,
    },
    Socialize {
        with: u64,
    },
    Fight {
        target: u64,
    },
    Trade {
        with: u64,
        item: String,
    },
    Comply,
    Refuse,
    EscapeAttempt,
    RequestMedical,
    JoinGang {
        gang: u64,
    },
    LeaveGang,
    LeadGang {
        gang: u64,
    },
    SpreadRumor {
        about: u64,
        content: String,
    },
    None,
}

impl Decision {
    /// Canonical action name for this decision.
    pub fn action_name(&self) -> &'static str {
        match self {
            Decision::MoveTo { .. } => "move_to",
            Decision::Socialize { .. } => "socialize",
            Decision::Fight { .. } => "fight",
            Decision::Trade { .. } => "trade",
            Decision::Comply => "comply",
            Decision::Refuse => "refuse",
            Decision::EscapeAttempt => "escape_attempt",
            Decision::RequestMedical => "request_medical",
            Decision::JoinGang { .. } => "join_gang",
            Decision::LeaveGang => "leave_gang",
            Decision::LeadGang { .. } => "lead_gang",
            Decision::SpreadRumor { .. } => "spread_rumor",
            Decision::None => "none",
        }
    }

    /// Identifier targeted by this decision, when it has one. This includes
    /// facilities and gangs, which are not agents; use [`Decision::entity_target`]
    /// when an agent id is required.
    pub fn target_id(&self) -> Option<u64> {
        match self {
            Decision::MoveTo { target_facility } => Some(*target_facility),
            Decision::Socialize { with } => Some(*with),
            Decision::Fight { target } => Some(*target),
            Decision::Trade { with, .. } => Some(*with),
            Decision::JoinGang { gang } => Some(*gang),
            Decision::LeadGang { gang } => Some(*gang),
            Decision::SpreadRumor { about, .. } => Some(*about),
            Decision::Comply
            | Decision::Refuse
            | Decision::EscapeAttempt
            | Decision::RequestMedical
            | Decision::LeaveGang
            | Decision::None => None,
        }
    }

    /// Agent-like target of this decision (excludes facilities and gangs).
    pub fn entity_target(&self) -> Option<u64> {
        match self {
            Decision::Socialize { with }
            | Decision::Fight { target: with }
            | Decision::Trade { with, .. }
            | Decision::SpreadRumor { about: with, .. } => Some(*with),
            _ => None,
        }
    }

    /// Free-text fields that must be treated as data, never instructions.
    pub fn free_text(&self) -> Vec<&str> {
        match self {
            Decision::Trade { item, .. } => vec![item.as_str()],
            Decision::SpreadRumor { content, .. } => vec![content.as_str()],
            _ => Vec::new(),
        }
    }

    /// Rough stamina cost used by resource validation.
    pub fn stamina_cost(&self) -> f32 {
        match self {
            Decision::MoveTo { .. } => 0.05,
            Decision::Socialize { .. } => 0.02,
            Decision::Fight { .. } => 0.15,
            Decision::EscapeAttempt => 0.25,
            Decision::JoinGang { .. } | Decision::LeadGang { .. } => 0.05,
            _ => 0.0,
        }
    }

    /// Whether this action is high-stakes and therefore inappropriate while an
    /// AI deliberation is pending.
    pub fn is_high_stakes(&self) -> bool {
        matches!(
            self,
            Decision::Fight { .. }
                | Decision::EscapeAttempt
                | Decision::JoinGang { .. }
                | Decision::LeaveGang
                | Decision::LeadGang { .. }
                | Decision::SpreadRumor { .. }
        )
    }

    /// Parse a canonical action name into a parameter-free decision where
    /// possible. Used by tooling and tests; the pipeline itself uses
    /// [`build_decision`] so parameterised actions are handled too.
    pub fn parse(action: &str) -> Option<Decision> {
        match normalize_action(action).as_str() {
            "comply" => Some(Decision::Comply),
            "refuse" => Some(Decision::Refuse),
            "escape_attempt" => Some(Decision::EscapeAttempt),
            "request_medical" => Some(Decision::RequestMedical),
            "leave_gang" => Some(Decision::LeaveGang),
            "none" => Some(Decision::None),
            _ => None,
        }
    }
}

/// Normalize an action alias (`Move-To`, `move to`, `move_to`) to the catalog
/// spelling. Unknown names are lowercased/underscored so callers can report
/// them usefully.
pub fn normalize_action(action: &str) -> String {
    action.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

/// A raw, model-produced decision before validation.
///
/// Field names and bounds follow design Appendix A. `parameters` values are
/// strings so the schema stays simple and bincode-safe.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawDecision {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub target: Option<RawTarget>,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub expected_outcome: Option<String>,
    #[serde(default)]
    pub urgency: f32,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub reasoning: String,
}

/// Target reference of a raw decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTarget {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub id: u64,
}

impl RawDecision {
    /// Reconstruct a raw decision from an already-structured one. Used by the
    /// legacy [`DecisionValidator`] trait so it can reuse the pipeline.
    pub fn from_decision(decision: &Decision) -> Self {
        let action = decision.action_name().to_string();
        let target = decision.target_id().map(|id| RawTarget { kind: target_kind(decision).to_string(), id });
        let mut parameters = BTreeMap::new();
        if let Decision::Trade { item, .. } = decision {
            parameters.insert("item".to_string(), item.clone());
        }
        if let Decision::SpreadRumor { content, .. } = decision {
            parameters.insert("content".to_string(), content.clone());
        }
        Self {
            action,
            target,
            parameters,
            intent: String::new(),
            expected_outcome: None,
            urgency: 0.5,
            confidence: 0.5,
            reasoning: String::new(),
        }
    }
}

fn target_kind(decision: &Decision) -> &'static str {
    match decision {
        Decision::MoveTo { .. } => "location",
        Decision::JoinGang { .. } | Decision::LeadGang { .. } => "gang",
        Decision::SpreadRumor { .. } => "agent",
        _ => "agent",
    }
}

/// The eight validation stages (design §8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationStage {
    Format,
    Semantic,
    Authority,
    WorldState,
    Resource,
    Legality,
    Safety,
    ClampRepair,
}

impl ValidationStage {
    pub fn as_str(self) -> &'static str {
        match self {
            ValidationStage::Format => "format",
            ValidationStage::Semantic => "semantic",
            ValidationStage::Authority => "authority",
            ValidationStage::WorldState => "world_state",
            ValidationStage::Resource => "resource",
            ValidationStage::Legality => "legality",
            ValidationStage::Safety => "safety",
            ValidationStage::ClampRepair => "clamp_repair",
        }
    }
}

/// Why a decision was rejected, at which stage, and whether a repair prompt
/// is allowed (design §8.2, §8.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rejection {
    pub stage: ValidationStage,
    pub reason: String,
    pub retryable: bool,
}

impl Rejection {
    pub fn new(stage: ValidationStage, reason: impl Into<String>) -> Self {
        let retryable = matches!(
            stage,
            ValidationStage::Format | ValidationStage::Semantic | ValidationStage::Safety
        );
        Self { stage, reason: reason.into(), retryable }
    }

    /// Human-readable `stage: reason` message.
    pub fn message(&self) -> String {
        format!("{}: {}", self.stage.as_str(), self.reason)
    }
}

/// Validation pipeline result. Kept in the original shape for compatibility;
/// richer detail lives in [`Rejection`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationResult {
    Accepted(Decision),
    Repaired(Decision),
    Rejected {
        reason: String,
    },
}

impl ValidationResult {
    pub fn rejected_reason(&self) -> Option<&str> {
        match self {
            ValidationResult::Rejected { reason } => Some(reason.as_str()),
            _ => None,
        }
    }

    pub fn accepted_decision(&self) -> Option<&Decision> {
        match self {
            ValidationResult::Accepted(d) | ValidationResult::Repaired(d) => Some(d),
            ValidationResult::Rejected { .. } => None,
        }
    }
}

/// Role of the deciding agent, used by authority validation and scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Prisoner,
    Guard,
    Warden,
    Unknown,
}

impl Default for Role {
    fn default() -> Self {
        Role::Unknown
    }
}

/// Read-only context supplied by the engine to every validation stage.
///
/// The original five fields are preserved for compatibility; newer fields are
/// optional and default to "unknown", in which case the corresponding checks
/// are skipped rather than rejecting valid decisions.
#[derive(Debug, Clone, Default)]
pub struct ValidationContext {
    pub current_facility: u64,
    pub lockdown: bool,
    pub stamina: f32,
    pub tokens_remaining_today: u32,
    pub guards_nearby: u32,

    /// Deciding agent. `0` means unknown; self-target checks are skipped then.
    pub agent_id: u64,
    pub role: Role,
    pub in_gang: bool,
    pub gang_id: Option<u64>,
    pub is_gang_leader: bool,
    /// Entities currently visible/known. Empty means unknown (check skipped).
    pub visible_entities: Vec<u64>,
    /// Facilities known to exist. Empty means unknown (check skipped).
    pub known_facilities: Vec<u64>,
    pub has_contraband: bool,
    /// Last applied decision, used by the anti-repeat safety check.
    pub last_decision: Option<Decision>,
    /// Current simulation time in seconds (used for audit/logging hooks).
    pub now_secs: u64,
}

/// Validator trait retained for engine integration. Implementations should
/// delegate to [`run_validation_pipeline`].
pub trait DecisionValidator {
    fn validate(&self, d: &Decision, ctx: &ValidationContext) -> ValidationResult;
}

/// Default validator: full eight-stage pipeline.
pub struct DefaultValidator;

impl DecisionValidator for DefaultValidator {
    fn validate(&self, d: &Decision, ctx: &ValidationContext) -> ValidationResult {
        validate_decision(d, ctx)
    }
}

/// Convenience wrapper: validate an already-structured decision through the
/// full eight-stage pipeline.
pub fn validate_decision(d: &Decision, ctx: &ValidationContext) -> ValidationResult {
    let raw = RawDecision::from_decision(d);
    run_validation_pipeline(&raw, ctx)
}

/// Legal status of an action. Illegal-but-possible actions are accepted as
/// decisions; consequences and discipline are applied by the engine, not by
/// validation (design §8.2 stage 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Legality {
    Legal,
    IllegalButPossible,
    Forbidden,
}

/// A deterministic action candidate with its pre-computed utility estimate.
/// The AI picks among candidates; the fallback chooser ranks them when AI is
/// unavailable (design §6.2, §9.5).
#[derive(Debug, Clone)]
pub struct ActionCandidate {
    pub decision: Decision,
    pub expected_utility: f32,
    pub risk: f32,
    pub legality: Legality,
}

impl ActionCandidate {
    pub fn new(decision: Decision, expected_utility: f32) -> Self {
        Self { decision, expected_utility, risk: 0.0, legality: Legality::Legal }
    }

    pub fn with_risk(mut self, risk: f32) -> Self {
        self.risk = risk.clamp(0.0, 1.0);
        self
    }

    pub fn with_legality(mut self, legality: Legality) -> Self {
        self.legality = legality;
        self
    }

    /// Utility adjusted for risk and legality; used by the fallback chooser.
    pub fn adjusted_utility(&self) -> f32 {
        let legality = match self.legality {
            Legality::Legal => 0.0,
            Legality::IllegalButPossible => -0.05,
            Legality::Forbidden => f32::NEG_INFINITY,
        };
        self.expected_utility - 0.5 * self.risk + legality
    }
}

// ---------------------------------------------------------------------------
// Stage 1 — format validation
// ---------------------------------------------------------------------------

/// Parse a JSON decision payload. This is the format stage's parsing half;
/// [`validate_format`] performs schema and catalog checks.
pub fn parse_raw_decision(json: &str) -> Result<RawDecision, Rejection> {
    serde_json::from_str::<RawDecision>(json)
        .map_err(|e| Rejection::new(ValidationStage::Format, format!("json parse: {e}")))
}

/// Stage 1: JSON/schema validation. Enforces catalog membership, bounded
/// strings, valid parameter keys, and finite numeric fields.
pub fn validate_format(raw: &RawDecision) -> Result<Decision, Rejection> {
    let action = normalize_action(&raw.action);
    if action.is_empty() {
        return Err(Rejection::new(ValidationStage::Format, "missing action"));
    }
    if !ACTION_CATALOG.contains(&action.as_str()) {
        return Err(Rejection::new(
            ValidationStage::Format,
            format!("unknown action '{}'", raw.action),
        ));
    }
    if raw.urgency.is_nan() || raw.urgency.is_infinite() {
        return Err(Rejection::new(ValidationStage::Format, "urgency must be finite"));
    }
    if raw.confidence.is_nan() || raw.confidence.is_infinite() {
        return Err(Rejection::new(ValidationStage::Format, "confidence must be finite"));
    }
    if raw.intent.chars().count() > MAX_FREE_TEXT {
        return Err(Rejection::new(ValidationStage::Format, "intent exceeds 500 chars"));
    }
    if raw.expected_outcome.as_deref().map_or(false, |s| s.chars().count() > MAX_FREE_TEXT) {
        return Err(Rejection::new(ValidationStage::Format, "expected_outcome exceeds 500 chars"));
    }
    if raw.reasoning.chars().count() > 2_000 {
        return Err(Rejection::new(ValidationStage::Format, "reasoning exceeds 2000 chars"));
    }
    if let Some(target) = &raw.target {
        if !matches!(target.kind.as_str(), "agent" | "location" | "object" | "gang" | "none") {
            return Err(Rejection::new(
                ValidationStage::Format,
                format!("invalid target kind '{}'", target.kind),
            ));
        }
    }
    for key in raw.parameters.keys() {
        if !valid_param_key(key) {
            return Err(Rejection::new(
                ValidationStage::Format,
                format!("invalid parameter key '{key}'"),
            ));
        }
    }
    build_decision(&action, raw)
}

fn valid_param_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn build_decision(action: &str, raw: &RawDecision) -> Result<Decision, Rejection> {
    let target = raw.target.as_ref().map(|t| t.id);
    let missing = || Rejection::new(ValidationStage::Format, format!("{action} requires a target"));
    let decision = match action {
        "move_to" => Decision::MoveTo { target_facility: target.ok_or_else(missing)? },
        "socialize" => Decision::Socialize { with: target.ok_or_else(missing)? },
        "fight" => Decision::Fight { target: target.ok_or_else(missing)? },
        "trade" => Decision::Trade {
            with: target.ok_or_else(missing)?,
            item: raw.parameters.get("item").cloned().unwrap_or_default(),
        },
        "spread_rumor" => Decision::SpreadRumor {
            about: target.ok_or_else(missing)?,
            content: raw.parameters.get("content").cloned().unwrap_or_default(),
        },
        "join_gang" => Decision::JoinGang { gang: target.ok_or_else(missing)? },
        "lead_gang" => Decision::LeadGang { gang: target.ok_or_else(missing)? },
        "comply" => Decision::Comply,
        "refuse" => Decision::Refuse,
        "escape_attempt" => Decision::EscapeAttempt,
        "request_medical" => Decision::RequestMedical,
        "leave_gang" => Decision::LeaveGang,
        "none" => Decision::None,
        other => {
            return Err(Rejection::new(
                ValidationStage::Format,
                format!("unknown action '{other}'"),
            ))
        }
    };
    Ok(decision)
}

// ---------------------------------------------------------------------------
// Stage 2 — semantic validation
// ---------------------------------------------------------------------------

/// Stage 2: action-specific semantics — parameter sanity, known targets,
/// reachable facilities.
pub fn validate_semantic(d: &Decision, ctx: &ValidationContext) -> Result<Decision, Rejection> {
    match d {
        Decision::Trade { item, .. } if item.trim().is_empty() => {
            return Err(Rejection::new(ValidationStage::Semantic, "trade item must be non-empty"));
        }
        Decision::SpreadRumor { content, .. } if content.trim().is_empty() => {
            return Err(Rejection::new(ValidationStage::Semantic, "rumor content must be non-empty"));
        }
        _ => {}
    }

    // Only enforce visibility when the engine actually supplied a known set.
    if !ctx.visible_entities.is_empty() {
        if let Some(target) = d.entity_target() {
            if target != ctx.agent_id && !ctx.visible_entities.contains(&target) {
                return Err(Rejection::new(
                    ValidationStage::Semantic,
                    format!("target {target} is not observable"),
                ));
            }
        }
    }
    if !ctx.known_facilities.is_empty() {
        if let Decision::MoveTo { target_facility } = d {
            if !ctx.known_facilities.contains(target_facility) {
                return Err(Rejection::new(
                    ValidationStage::Semantic,
                    format!("facility {target_facility} is not known"),
                ));
            }
        }
    }
    Ok(d.clone())
}

// ---------------------------------------------------------------------------
// Stage 3 — authority validation
// ---------------------------------------------------------------------------

/// Stage 3: role/rank authority. Staff cannot perform inmate gang actions;
/// gang actions require actual affiliation and, for leadership, the leader
/// role.
pub fn validate_authority(d: &Decision, ctx: &ValidationContext) -> Result<Decision, Rejection> {
    let staff = matches!(ctx.role, Role::Guard | Role::Warden);
    match d {
        Decision::JoinGang { .. } | Decision::LeaveGang | Decision::LeadGang { .. } => {
            if staff {
                return Err(Rejection::new(
                    ValidationStage::Authority,
                    "staff cannot perform gang actions",
                ));
            }
        }
        Decision::EscapeAttempt | Decision::RequestMedical => {
            if staff {
                return Err(Rejection::new(
                    ValidationStage::Authority,
                    "staff cannot use inmate escape/medical actions",
                ));
            }
        }
        _ => {}
    }

    match d {
        Decision::JoinGang { .. } => {
            if ctx.in_gang {
                return Err(Rejection::new(
                    ValidationStage::Authority,
                    "already affiliated with a gang",
                ));
            }
        }
        Decision::LeaveGang => {
            if !ctx.in_gang {
                return Err(Rejection::new(
                    ValidationStage::Authority,
                    "not affiliated with a gang",
                ));
            }
        }
        Decision::LeadGang { gang } => {
            if !ctx.in_gang {
                return Err(Rejection::new(
                    ValidationStage::Authority,
                    "not affiliated with a gang",
                ));
            }
            if !ctx.is_gang_leader {
                return Err(Rejection::new(
                    ValidationStage::Authority,
                    "not the gang leader",
                ));
            }
            if ctx.gang_id != Some(*gang) {
                return Err(Rejection::new(
                    ValidationStage::Authority,
                    "gang mismatch",
                ));
            }
        }
        _ => {}
    }
    Ok(d.clone())
}

// ---------------------------------------------------------------------------
// Stage 4 — world-state validation
// ---------------------------------------------------------------------------

/// Stage 4: preconditions, schedule and lockdown. Kept conservative so the
/// engine remains the source of truth for reachability and paths.
pub fn validate_world_state(d: &Decision, ctx: &ValidationContext) -> Result<Decision, Rejection> {
    if ctx.lockdown && matches!(d, Decision::EscapeAttempt) {
        return Err(Rejection::new(ValidationStage::WorldState, "lockdown active"));
    }
    Ok(d.clone())
}

// ---------------------------------------------------------------------------
// Stage 5 — resource validation
// ---------------------------------------------------------------------------

/// Stage 5: stamina/action-budget affordability. Token budgets are accounted
/// by the orchestrator, not by validation.
pub fn validate_resources(d: &Decision, ctx: &ValidationContext) -> Result<Decision, Rejection> {
    let cost = d.stamina_cost();
    if ctx.stamina < cost {
        return Err(Rejection::new(
            ValidationStage::Resource,
            format!("insufficient stamina: {:.2} < {:.2}", ctx.stamina, cost),
        ));
    }
    Ok(d.clone())
}

// ---------------------------------------------------------------------------
// Stage 6 — legality / policy validation
// ---------------------------------------------------------------------------

/// Stage 6: legality. Illegal-but-possible actions (assault, escape planning,
/// rumor spreading) are allowed as decisions; their consequences and the
/// disciplinary response are applied by the engine (design §8.2).
pub fn validate_legality(d: &Decision, _ctx: &ValidationContext) -> Result<Decision, Rejection> {
    // Illegal-but-possible actions are deliberately allowed through; their
    // consequences, investigations and discipline are engine outcomes.
    Ok(d.clone())
}

// ---------------------------------------------------------------------------
// Stage 7 — safety / anti-exploit validation
// ---------------------------------------------------------------------------

/// Stage 7: self-harm loops, repeated identical decisions, unbounded
/// parameters, and prompt-injection-like free text.
pub fn validate_safety(d: &Decision, ctx: &ValidationContext) -> Result<Decision, Rejection> {
    if ctx.agent_id != 0 {
        if let Some(target) = d.entity_target() {
            if target == ctx.agent_id && matches!(d, Decision::Fight { .. }) {
                return Err(Rejection::new(ValidationStage::Safety, "self-targeting attack"));
            }
        }
    }
    if ctx.last_decision.as_ref() == Some(d) {
        return Err(Rejection::new(
            ValidationStage::Safety,
            "repeated identical decision",
        ));
    }
    for text in d.free_text() {
        if text.chars().count() > MAX_FREE_TEXT {
            return Err(Rejection::new(
                ValidationStage::Safety,
                "free-text field exceeds bound",
            ));
        }
        if contains_injection(text) {
            return Err(Rejection::new(
                ValidationStage::Safety,
                "prompt-injection-like content",
            ));
        }
    }
    Ok(d.clone())
}

fn contains_injection(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "ignore previous",
        "ignore all previous",
        "system prompt",
        "you must now",
        "developer mode",
        "jailbreak",
        "reveal your instructions",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

// ---------------------------------------------------------------------------
// Stage 8 — clamp / repair
// ---------------------------------------------------------------------------

/// Stage 8: coerce out-of-range values into legal ranges, trim free text,
/// normalize aliases, and apply deterministic de-escalation repairs. This
/// stage always succeeds.
pub fn clamp_and_repair(d: &Decision, ctx: &ValidationContext) -> Decision {
    let mut out = d.clone();
    match &mut out {
        Decision::Trade { item, .. } => *item = clamp_text(item, 200),
        Decision::SpreadRumor { content, .. } => *content = clamp_text(content, MAX_FREE_TEXT),
        _ => {}
    }
    // Deterministic de-escalation: a heavily guarded fight becomes a
    // conversation. Preserved from the original DefaultValidator behavior.
    if matches!(out, Decision::Fight { .. }) && ctx.guards_nearby > 1 {
        out = Decision::Socialize { with: 0 };
    }
    out
}

fn clamp_text(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        trimmed.chars().take(max).collect()
    }
}

// ---------------------------------------------------------------------------
// Pipeline entry points
// ---------------------------------------------------------------------------

/// Run all eight stages in order. Returns [`ValidationResult::Repaired`] when
/// stage 8 changed the decision relative to the parsed input.
pub fn run_validation_pipeline(raw: &RawDecision, ctx: &ValidationContext) -> ValidationResult {
    let initial = match validate_format(raw) {
        Ok(d) => d,
        Err(e) => return ValidationResult::Rejected { reason: e.message() },
    };

    let stages: [fn(&Decision, &ValidationContext) -> Result<Decision, Rejection>; 6] = [
        validate_semantic,
        validate_authority,
        validate_world_state,
        validate_resources,
        validate_legality,
        validate_safety,
    ];

    let mut current = initial.clone();
    for stage in stages {
        match stage(&current, ctx) {
            Ok(next) => current = next,
            Err(e) => return ValidationResult::Rejected { reason: e.message() },
        }
    }

    let repaired = clamp_and_repair(&current, ctx);
    if repaired == initial {
        ValidationResult::Accepted(repaired)
    } else {
        ValidationResult::Repaired(repaired)
    }
}

/// Parse JSON then run the pipeline.
pub fn run_validation_json(json: &str, ctx: &ValidationContext) -> ValidationResult {
    match parse_raw_decision(json) {
        Ok(raw) => run_validation_pipeline(&raw, ctx),
        Err(e) => ValidationResult::Rejected { reason: e.message() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ValidationContext {
        ValidationContext { agent_id: 7, role: Role::Prisoner, stamina: 1.0, ..Default::default() }
    }

    #[test]
    fn format_rejects_unknown_action() {
        let raw = RawDecision { action: "teleport".into(), ..Default::default() };
        assert!(validate_format(&raw).is_err());
    }

    #[test]
    fn lockdown_rejects_escape() {
        let raw = RawDecision { action: "escape_attempt".into(), ..Default::default() };
        let mut c = ctx();
        c.lockdown = true;
        match run_validation_pipeline(&raw, &c) {
            ValidationResult::Rejected { .. } => {}
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn guarded_fight_is_repaired_to_socialize() {
        let raw = RawDecision { action: "fight".into(), target: Some(RawTarget { kind: "agent".into(), id: 9 }), ..Default::default() };
        let mut c = ctx();
        c.guards_nearby = 3;
        match run_validation_pipeline(&raw, &c) {
            ValidationResult::Repaired(Decision::Socialize { .. }) => {}
            other => panic!("expected repair, got {other:?}"),
        }
    }

    #[test]
    fn repeated_decision_is_rejected() {
        let d = Decision::Comply;
        let mut c = ctx();
        c.last_decision = Some(Decision::Comply);
        assert!(validate_safety(&d, &c).is_err());
    }

    #[test]
    fn json_round_trip_through_pipeline() {
        let json = r#"{"action":"trade","target":{"kind":"agent","id":3},"parameters":{"item":"soap"},"intent":"swap"}"#;
        match run_validation_json(json, &ctx()) {
            ValidationResult::Accepted(Decision::Trade { with: 3, .. }) => {}
            other => panic!("expected accepted trade, got {other:?}"),
        }
    }

    #[test]
    fn forbidden_self_attack_is_rejected() {
        let d = Decision::Fight { target: 7 };
        assert!(validate_safety(&d, &ctx()).is_err());
    }
}
