//! The engine decision pipeline (Directive §19).
//!
//! Agents propose [`alc_agents::Decision`]s through the engine. The pipeline:
//!
//! 1. Validate: [`alc_agents::DefaultValidator`] plus engine-side preconditions.
//! 2. Apply: world mutations via `alc_world::World` accessors and `alc_social`
//!    resolution helpers (`resolve_conflict`/`apply_outcome`,
//!    `resolve_escape_attempt`/`apply_escape_consequences`); events emitted on
//!    the world's event bus.
//!
//! Rejected decisions are reported in [`DecisionOutcome`] and left unapplied.
//!
//! ## Audit and incident mirroring
//!
//! Only *applied* proposals are appended to the engine's decision audit log
//! ([`Sim::record_decision`]); rejected proposals leave no audit trace. The
//! conflict events produced by an applied decision are additionally mirrored
//! into the engine's incident log and structured sidecar so the UI and the
//! causality invariant see them. Mirrored incidents are built with
//! [`crate::incidents::incident_from_world_event`], which derives causal links
//! from the event's free-text causes and links to a matching prior incident
//! when one exists. `cause`/`consequence` stay `None` unless a real incident
//! link is available; self-referential placeholders are never generated.
//!
//! ## Brain inspector
//!
//! [`evaluate_candidates`] provides a pure, read-only view of what a prisoner
//! could do next: evaluator drive scores ([`EvaluatorScore`]), a deterministic
//! confidence, and cited memories ([`MemoryCitation`]). It exists so the UI can
//! explain *why* a decision looks attractive without invoking the AI layer or
//! mutating the world.

use crate::Sim;
use alc_agents::{Decision, DecisionAction, ValidationResult};
use alc_core::PrisonerId;
use alc_events::EventKind;
use alc_prisoner::Prisoner;
use alc_world::World;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Maximum number of memory citations attached to one [`DecisionProposal`].
///
/// The brain inspector is a UI surface, so citations are deliberately bounded:
/// five is enough to explain a decision without turning the payload into a
/// second memory store.
pub const MAX_MEMORY_CITATIONS: usize = 5;

/// One evaluator drive score attached to a [`DecisionProposal`].
///
/// `score` is the strength of the drive in `0..=1`; `weight` is how much that
/// drive matters for the candidate decision. The ranking value of a proposal
/// is `sum(score * weight)` (see [`evaluate_candidates`]). Needs are modelled
/// as *deficit pressures* in `alc-prisoner` — a high `safety` score means the
/// prisoner feels unsafe, not safe — so every drive score is directly
/// comparable: higher means "this motivation is more strongly activated".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorScore {
    /// Drive name (for example `"safety"`, `"respect"`, `"companionship"`).
    pub drive: String,
    /// Drive strength in `0..=1` (higher = more activated).
    pub score: f32,
    /// Importance of this drive for the candidate in `0..=1`.
    pub weight: f32,
}

/// A memory record cited as an influence on a [`DecisionProposal`].
///
/// # Approximation
///
/// The current `World` persists no per-prisoner `alc_agents::Memory` store
/// (only a one-line `Prisoner::memory_summary`). Until that wiring lands,
/// citations are built deterministically from the two read-only sources that
/// do exist: the prisoner's intake summary and the bounded world event bus
/// filtered to events where the prisoner is an actor. The event bus is the
/// closest available proxy for episodic memory; `importance` combines event
/// severity, confidence and a fixed seven-day recency half-life.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryCitation {
    /// Stable id: event id for event-derived memories, `0` for the intake note.
    pub id: u64,
    /// Memory kind (`"episodic"`, `"social"`, `"semantic"`).
    pub kind: String,
    /// Retrieval importance in `0..=1` (higher = more influential).
    pub importance: f32,
    /// Human-readable summary shown by the brain inspector.
    pub summary: String,
}

/// A proposed decision ready for engine validation, before world-state application.
///
/// The first six fields predate the brain-inspector extension and keep their
/// original meaning. The score, confidence and memory fields are populated by
/// [`evaluate_candidates`] (or by the engine when it builds a proposal
/// directly) and never affect validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionProposal {
    /// The prisoner who proposed the decision.
    pub prisoner_id: PrisonerId,
    /// The decision type and target.
    pub decision_type: DecisionType,
    /// Optional target id. Its interpretation depends on the decision type:
    /// a prisoner id for social decisions, a facility id for [`DecisionType::MoveTo`],
    /// or a gang id for [`DecisionType::JoinGang`]/[`DecisionType::LeadGang`].
    pub target: Option<PrisonerId>,
    /// Rationale for the decision (used by AI orchestration).
    pub rationale: String,
    /// Expected validation outcome if applied immediately.
    pub estimated_outcome: ValidationResult,
    /// Trace hash before the decision was applied, for determinism checks.
    ///
    /// [`evaluate_candidates`] takes only `&World`, so it records `0` here;
    /// the engine fills the real hash when it builds a proposal from a `Sim`.
    pub trace_hash_before: u64,
    /// Evaluator drives that produced this candidate (highest total first after
    /// sorting). Empty only for hand-built proposals that did not run through
    /// [`evaluate_candidates`].
    pub scores: Vec<EvaluatorScore>,
    /// Deterministic confidence in `0..=1` (see [`proposal_confidence`]).
    pub confidence: f32,
    /// Memory records cited as influences on this candidate.
    pub memory_records: Vec<MemoryCitation>,
}

/// The type of a prisoner decision, mapped from the canonical Decision catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DecisionType {
    /// Move to a different location or facility.
    MoveTo,
    /// Socialize with another prisoner.
    Socialize,
    /// Initiate a physical fight.
    Fight,
    /// Trade items with another prisoner.
    Trade,
    /// Comply with an order or request.
    Comply,
    /// Refuse an order or request.
    Refuse,
    /// Attempt to escape from prison.
    EscapeAttempt,
    /// Request medical attention.
    RequestMedical,
    /// Join a gang.
    JoinGang,
    /// Leave a gang.
    LeaveGang,
    /// Lead a gang.
    LeadGang,
    /// Spread a rumor.
    SpreadRumor,
    /// No specific decision.
    None,
}

impl DecisionType {
    /// Convert from an [`alc_agents::Decision`] variant.
    pub fn from_decision(decision: &Decision) -> Self {
        match &decision.action {
            DecisionAction::MoveTo { .. } => DecisionType::MoveTo,
            DecisionAction::Socialize { .. } => DecisionType::Socialize,
            DecisionAction::Fight { .. } => DecisionType::Fight,
            DecisionAction::Trade { .. } => DecisionType::Trade,
            DecisionAction::Comply => DecisionType::Comply,
            DecisionAction::Refuse => DecisionType::Refuse,
            DecisionAction::EscapeAttempt => DecisionType::EscapeAttempt,
            DecisionAction::RequestMedical => DecisionType::RequestMedical,
            DecisionAction::JoinGang { .. } => DecisionType::JoinGang,
            DecisionAction::LeaveGang => DecisionType::LeaveGang,
            DecisionAction::LeadGang { .. } => DecisionType::LeadGang,
            DecisionAction::SpreadRumor { .. } => DecisionType::SpreadRumor,
            DecisionAction::None => DecisionType::None,
        }
    }

    /// Convert to an [`alc_agents::Decision`] variant with minimal parameters.
    ///
    /// `target` is interpreted according to the variant: a prisoner id for
    /// social decisions, a facility id for [`DecisionType::MoveTo`], and a gang
    /// id for [`DecisionType::JoinGang`]/[`DecisionType::LeadGang`]. Passing
    /// `None` falls back to id `0`, which validation will normally reject for
    /// targeted decisions.
    pub fn to_decision(self, target: Option<PrisonerId>) -> Decision {
        match self {
            DecisionType::MoveTo => Decision::new(DecisionAction::MoveTo {
                target_facility: target.map_or(0, |pid| pid.raw()),
            }),
            DecisionType::Socialize => Decision::new(DecisionAction::Socialize {
                with: target.map_or(0, |pid| pid.raw()),
            }),
            DecisionType::Fight => Decision::new(DecisionAction::Fight {
                target: target.map_or(0, |pid| pid.raw()),
            }),
            DecisionType::Trade => Decision::new(DecisionAction::Trade {
                with: target.map_or(0, |pid| pid.raw()),
                item: "".into(),
            }),
            DecisionType::Comply => Decision::new(DecisionAction::Comply),
            DecisionType::Refuse => Decision::new(DecisionAction::Refuse),
            DecisionType::EscapeAttempt => Decision::new(DecisionAction::EscapeAttempt),
            DecisionType::RequestMedical => Decision::new(DecisionAction::RequestMedical),
            DecisionType::JoinGang => Decision::new(DecisionAction::JoinGang {
                gang: target.map_or(0, |pid| pid.raw()),
            }),
            DecisionType::LeaveGang => Decision::new(DecisionAction::LeaveGang),
            DecisionType::LeadGang => Decision::new(DecisionAction::LeadGang {
                gang: target.map_or(0, |pid| pid.raw()),
            }),
            DecisionType::SpreadRumor => Decision::new(DecisionAction::SpreadRumor {
                about: target.map_or(0, |pid| pid.raw()),
                content: "".into(),
            }),
            DecisionType::None => Decision::new(DecisionAction::None),
        }
    }
}

/// Deterministic evaluator scores for one candidate decision type.
///
/// The mapping follows the Directive's needs→behavior principles: safety drives
/// compliance and escape, respect drives fights, companionship drives
/// socializing, autonomy drives refusal, purpose drives movement/gang/work,
/// and so on. Needs are deficit pressures in `alc-prisoner`, so a high
/// `safety` score means "feels unsafe". Every returned score and weight is
/// clamped to `0..=1`; the vector is never empty (an inert candidate such as
/// [`DecisionType::None`] still carries low-weight stability drives).
pub fn score_for(prisoner: &Prisoner, dt: DecisionType) -> Vec<EvaluatorScore> {
    let n = &prisoner.needs;
    let t = &prisoner.traits;
    let c = &prisoner.condition;
    match dt {
        DecisionType::MoveTo => vec![
            drive("hunger", n.hunger, 0.30),
            drive("thirst", n.thirst, 0.25),
            drive("purpose", n.purpose, 0.20),
            drive("stimulation", n.stimulation, 0.15),
            drive("privacy", n.privacy, 0.10),
        ],
        DecisionType::Socialize => vec![
            drive("companionship", n.companionship, 0.35),
            drive("belonging", n.belonging, 0.25),
            drive("friendship", n.friendship, 0.20),
            drive("sociability", t.sociability, 0.12),
            drive("stimulation", n.stimulation, 0.08),
        ],
        DecisionType::Fight => vec![
            drive("respect", n.respect, 0.30),
            drive("aggression", t.aggression, 0.25),
            drive("irritability", n.irritability(), 0.20),
            drive("impulsiveness", t.impulsiveness, 0.15),
            drive("stimulation", n.stimulation, 0.10),
        ],
        DecisionType::Trade => vec![
            drive("purpose", n.purpose, 0.20),
            drive("belonging", n.belonging, 0.20),
            drive("friendship", n.friendship, 0.20),
            drive("status", n.status, 0.20),
            drive("manipulation", t.manipulation, 0.20),
        ],
        DecisionType::Comply => vec![
            drive("safety", n.safety, 0.35),
            drive("protection", n.protection, 0.20),
            drive("discipline", t.discipline, 0.25),
            drive("stability", n.stability, 0.20),
        ],
        DecisionType::Refuse => vec![
            drive("autonomy", n.autonomy, 0.35),
            drive("freedom", n.freedom, 0.25),
            drive("impulsiveness", t.impulsiveness, 0.20),
            drive("respect", n.respect, 0.20),
        ],
        DecisionType::EscapeAttempt => vec![
            drive("freedom", n.freedom, 0.30),
            drive("safety", n.safety, 0.20),
            drive("risk_tolerance", t.risk_tolerance, 0.20),
            drive("despair", n.despair_pressure(c.hope), 0.20),
            drive("hope_deficit", n.hope_deficit(c.hope), 0.10),
        ],
        DecisionType::RequestMedical => vec![
            drive("medical", n.medical_need, 0.40),
            drive("health_deficit", 1.0 - c.health, 0.20),
            drive("illness", c.illness, 0.20),
            drive("fatigue", n.fatigue, 0.10),
            drive("stability", n.stability, 0.10),
        ],
        DecisionType::JoinGang => vec![
            drive("protection", n.protection, 0.30),
            drive("safety", n.safety, 0.25),
            drive("belonging", n.belonging, 0.20),
            drive("gang_seeking", n.gang_seeking_pressure(), 0.15),
            drive("status", n.status, 0.10),
        ],
        DecisionType::LeaveGang => vec![
            drive("autonomy", n.autonomy, 0.35),
            drive("freedom", n.freedom, 0.25),
            drive("disloyalty", 1.0 - t.loyalty, 0.20),
            drive("respect", n.respect, 0.20),
        ],
        DecisionType::LeadGang => vec![
            drive("status", n.status, 0.30),
            drive("ambition", t.ambition, 0.25),
            drive("recognition", n.recognition, 0.20),
            drive("leadership", t.leadership, 0.15),
            drive("respect", n.respect, 0.10),
        ],
        DecisionType::SpreadRumor => vec![
            drive("manipulation", t.manipulation, 0.35),
            drive("stimulation", n.stimulation, 0.25),
            drive("status", n.status, 0.20),
            drive("recognition", n.recognition, 0.20),
        ],
        DecisionType::None => vec![
            drive("stability", n.stability, 0.10),
            drive("discipline", t.discipline, 0.10),
        ],
    }
}

/// One drive score with clamped, finite inputs.
fn drive(name: &str, score: f32, weight: f32) -> EvaluatorScore {
    EvaluatorScore {
        drive: name.to_string(),
        score: clamp01(score),
        weight: clamp01(weight),
    }
}

/// Clamp to `0..=1`, mapping non-finite values to `0`.
fn clamp01(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Ranking value of a score vector: `sum(score * weight)`.
fn weighted_utility(scores: &[EvaluatorScore]) -> f32 {
    scores.iter().map(|s| s.score * s.weight).sum()
}

/// Deterministic confidence in `0..=1` for one proposal.
///
/// Combines three signals:
///
/// * **score spread / affinity** — the weighted mean drive strength (`affinity`)
///   and how tightly the drives agree (`spread`). A candidate whose drives all
///   point the same way is more confidently preferred.
/// * **validation result** — accepted `1.0`, repaired `0.65`, rejected `0.0`.
/// * **need pressure** — the prisoner's overall current need pressure, so the
///   inspector knows whether this is a calm or urgent moment.
///
/// The function is pure and returns a finite value even for empty or
/// non-finite inputs (empty proposals fall back to a low-confidence base).
pub fn proposal_confidence(
    scores: &[EvaluatorScore],
    validation: &ValidationResult,
    need_pressure: f32,
) -> f32 {
    let validation_factor = match validation {
        ValidationResult::Accepted(_) => 1.0,
        ValidationResult::Repaired(_) => 0.65,
        ValidationResult::Rejected { .. } => 0.0,
    };
    let pressure = clamp01(need_pressure);
    if scores.is_empty() {
        return (0.10 + 0.25 * validation_factor + 0.10 * pressure).clamp(0.0, 1.0);
    }

    let weight_sum: f32 = scores.iter().map(|s| s.weight).sum();
    let utility = weighted_utility(scores);
    let affinity = if weight_sum > 0.0 {
        clamp01(utility / weight_sum)
    } else {
        0.0
    };
    let max = scores.iter().map(|s| s.score).fold(f32::MIN, f32::max);
    let min = scores.iter().map(|s| s.score).fold(f32::MAX, f32::min);
    let spread = if max > 0.0 {
        ((max - min) / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let agreement = 1.0 - spread;

    (0.15 + 0.30 * affinity + 0.20 * agreement + 0.20 * validation_factor + 0.15 * pressure)
        .clamp(0.0, 1.0)
}

/// Retrieve bounded, deterministic memory citations for a prisoner.
///
/// # Approximation
///
/// The `World` does not yet persist a per-agent [`alc_agents::Memory`], so this
/// function uses the closest read-only sources available today:
///
/// 1. `prisoner.memory_summary` (the intake note) as a `semantic` memory with
///    id `0`.
/// 2. Events from the bounded world bus where the prisoner is an actor, ranked
///    by `severity × confidence × recency`, where recency uses a fixed
///    seven-day half-life and severity reuses the same coarse conflict table as
///    incident mirroring. Conflict/social events are labelled `episodic` /
///    `social`; everything else is `semantic`.
///
/// Results are sorted by importance (descending) then id (descending, so newer
/// events win ties) and truncated to [`MAX_MEMORY_CITATIONS`]. The function is
/// pure: it never calls `Memory::touch`, mutate the bus, or consume budget.
/// When real per-agent memory retrieval is wired, only this function needs to
/// change.
pub fn memory_citations(world: &World, prisoner: &Prisoner) -> Vec<MemoryCitation> {
    let now_secs = world.time.as_secs();
    let mut citations = Vec::new();

    if !prisoner.memory_summary.trim().is_empty() {
        citations.push(MemoryCitation {
            id: 0,
            kind: "semantic".to_string(),
            importance: 0.60,
            summary: prisoner.memory_summary.clone(),
        });
    }

    for event in world.events.recent() {
        if !event.actors.contains(&prisoner.id.raw()) {
            continue;
        }
        let age_secs = now_secs.saturating_sub(event.at.as_secs());
        let recency = 0.5f32.powf(age_secs as f32 / (7.0 * 86_400.0));
        let importance = clamp01(severity_for(event.kind) * clamp01(event.confidence) * recency);
        citations.push(MemoryCitation {
            id: event.id,
            kind: memory_kind_for(event.kind).to_string(),
            importance,
            summary: event.summary.clone(),
        });
    }

    citations.sort_by(|a, b| b.importance.total_cmp(&a.importance).then(b.id.cmp(&a.id)));
    citations.truncate(MAX_MEMORY_CITATIONS);
    citations
}

/// Classify an event as an episodic, social or semantic memory.
fn memory_kind_for(kind: EventKind) -> &'static str {
    match kind {
        EventKind::RelationshipShift
        | EventKind::GangFormed
        | EventKind::GangDisbanded
        | EventKind::GangRecruit
        | EventKind::GangRivalry
        | EventKind::LeadershipDispute
        | EventKind::Dialogue
        | EventKind::Interaction
        | EventKind::Visitor
        | EventKind::FamilyCorrespondence => "social",
        EventKind::Argument
        | EventKind::Threat
        | EventKind::Theft
        | EventKind::Fight
        | EventKind::GangClash
        | EventKind::Protests
        | EventKind::PropertyDamage
        | EventKind::ContrabandFound
        | EventKind::MedicalEmergency
        | EventKind::Illness
        | EventKind::Injury
        | EventKind::EscapeAttempt
        | EventKind::EscapePrevented
        | EventKind::EscapeSucceeded => "episodic",
        _ => "semantic",
    }
}

/// Evaluate one candidate per [`DecisionType`] for a prisoner.
///
/// This is the deterministic "brain inspector" entry point: it builds up to 13
/// proposals, scores each drive set with [`score_for`], validates the real
/// decision with [`alc_agents::validate_decision`] against a single
/// [`validation_context`] call, attaches [`memory_citations`], and returns the
/// candidates sorted by descending `sum(score * weight)` (ties broken by
/// [`DecisionType`] order, then target id).
///
/// # Targets
///
/// Real targets are used where the world provides them: a co-located prisoner
/// for social/fight/trade/rumor candidates, a real gang for join/lead, and a
/// real facility for movement. [`DecisionProposal::target`] is an opaque
/// `PrisonerId` slot, so facility and gang ids are wrapped in it; the
/// decision type determines how it is interpreted. `to_decision` maps that id
/// to the correct field for every variant, including gang decisions.
///
/// Candidates whose required target does not exist are skipped (for example
/// no gangs → no join/lead candidate; nobody co-located → no social
/// candidate). [`DecisionType::LeaveGang`] is only considered when the prisoner
/// belongs to a gang.
///
/// # Purity
///
/// This function only reads `world`. It does not mutate state, emit events,
/// spend tokens, or change the decision log. Because it has no `Sim` handle it
/// records `trace_hash_before = 0`; the engine fills the real hash when it
/// adopts one of these proposals.
///
/// Memory retrieval is per-prisoner rather than per-candidate, so every candidate
/// in one call shares the same bounded citation list.
pub fn evaluate_candidates(world: &World, id: PrisonerId) -> Vec<DecisionProposal> {
    let Some(prisoner) = world.prisoners.iter().find(|p| p.id == id) else {
        return Vec::new();
    };
    let current = world.prisoner_location.get(&id).copied();
    let mut candidates: Vec<(DecisionType, Option<PrisonerId>)> = Vec::new();

    // MoveTo: a real facility, preferring one other than the current location.
    if let Some(facility) = world
        .facilities
        .iter()
        .map(|f| f.id.raw())
        .find(|fid| Some(*fid) != current.map(|f| f.raw()))
        .or_else(|| world.facilities.first().map(|f| f.id.raw()))
    {
        candidates.push((DecisionType::MoveTo, Some(PrisonerId(facility))));
    }

    // Co-located prisoners, ascending by id for determinism.
    let mut neighbours: Vec<PrisonerId> = match current {
        Some(facility) => world
            .prisoners
            .iter()
            .filter(|p| p.id != id && world.prisoner_location.get(&p.id) == Some(&facility))
            .map(|p| p.id)
            .collect(),
        None => Vec::new(),
    };
    neighbours.sort_by_key(|p| p.raw());
    if let Some(&peer) = neighbours.first() {
        candidates.push((DecisionType::Socialize, Some(peer)));
        candidates.push((DecisionType::Fight, Some(peer)));
        candidates.push((DecisionType::Trade, Some(peer)));
        candidates.push((DecisionType::SpreadRumor, Some(peer)));
    }

    // Real gang ids: join the first gang the prisoner is not in (or any gang),
    // and lead a gang they already belong to.
    if let Some(gang) = world
        .gangs
        .iter()
        .map(|g| g.id)
        .find(|g| Some(g.raw()) != prisoner.gang_affiliation)
        .or_else(|| world.gangs.first().map(|g| g.id))
    {
        candidates.push((DecisionType::JoinGang, Some(PrisonerId(gang.raw()))));
    }
    if let Some(gang) = world.gangs.iter().find(|g| g.members.contains(&id)) {
        candidates.push((DecisionType::LeadGang, Some(PrisonerId(gang.id.raw()))));
    }
    if prisoner.gang_affiliation.is_some() {
        candidates.push((DecisionType::LeaveGang, None));
    }

    // Untargeted candidates are always available.
    candidates.push((DecisionType::Comply, None));
    candidates.push((DecisionType::Refuse, None));
    candidates.push((DecisionType::EscapeAttempt, None));
    candidates.push((DecisionType::RequestMedical, None));
    candidates.push((DecisionType::None, None));

    let need_pressure = prisoner.needs.total_pressure();
    let memories = memory_citations(world, prisoner);

    let mut proposals: Vec<DecisionProposal> = candidates
        .into_iter()
        .map(|(decision_type, target)| {
            let decision = decision_type.to_decision(target);
            let estimated_outcome = validate_with_engine_rules(world, id, &decision);
            let scores = score_for(prisoner, decision_type);
            let confidence = proposal_confidence(&scores, &estimated_outcome, need_pressure);
            let rationale = candidate_rationale(prisoner, decision_type, target, &scores);
            DecisionProposal {
                prisoner_id: id,
                decision_type,
                target,
                rationale,
                estimated_outcome,
                trace_hash_before: 0,
                scores,
                confidence,
                memory_records: memories.clone(),
            }
        })
        .collect();

    proposals.sort_by(|a, b| {
        weighted_utility(&b.scores)
            .total_cmp(&weighted_utility(&a.scores))
            .then_with(|| a.decision_type.cmp(&b.decision_type))
            .then_with(|| {
                a.target
                    .map_or(u64::MAX, |t| t.raw())
                    .cmp(&b.target.map_or(u64::MAX, |t| t.raw()))
            })
    });
    // Inspector DTOs stay bounded even if new decision types are added.
    proposals.truncate(16);
    proposals
}

/// Deterministic one-line rationale naming the strongest drive for a candidate.
fn candidate_rationale(
    prisoner: &Prisoner,
    decision_type: DecisionType,
    target: Option<PrisonerId>,
    scores: &[EvaluatorScore],
) -> String {
    let target_note = target
        .map(|t| format!(" target {}", t.raw()))
        .unwrap_or_default();
    let top = scores.iter().max_by(|a, b| {
        (a.score * a.weight)
            .total_cmp(&(b.score * b.weight))
            .then_with(|| a.drive.cmp(&b.drive))
    });
    match top {
        Some(top) => format!(
            "Prisoner {} weighs {:?}{}: {} drive {:.2} (weight {:.2})",
            prisoner.id.raw(),
            decision_type,
            target_note,
            top.drive,
            top.score,
            top.weight
        ),
        None => format!(
            "Prisoner {} considers {:?}{}: no strongly activated drive",
            prisoner.id.raw(),
            decision_type,
            target_note
        ),
    }
}

impl DecisionProposal {
    /// Multi-line human-readable explanation for the brain-inspector UI.
    ///
    /// Lists the decision, validation state, confidence, the three highest
    /// weighted drives and the three most important memory citations. Content
    /// is derived only from data already stored on the proposal, so it is
    /// deterministic and cannot mutate world state.
    pub fn explanation(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("Decision: {:?}", self.decision_type));
        if let Some(target) = self.target {
            lines.push(format!("Target id: {}", target.raw()));
        }
        lines.push(format!("Confidence: {:.0}%", self.confidence * 100.0));
        lines.push(format!(
            "Validation: {}",
            describe_validation(&self.estimated_outcome)
        ));
        lines.push(format!("Rationale: {}", self.rationale));

        let mut scores: Vec<&EvaluatorScore> = self.scores.iter().collect();
        scores.sort_by(|a, b| {
            (b.score * b.weight)
                .total_cmp(&(a.score * a.weight))
                .then_with(|| a.drive.cmp(&b.drive))
        });
        lines.push("Top drives:".to_string());
        if scores.is_empty() {
            lines.push("  (no evaluator scores)".to_string());
        } else {
            for score in scores.iter().take(3) {
                lines.push(format!(
                    "  - {}: score {:.2}, weight {:.2}",
                    score.drive, score.score, score.weight
                ));
            }
        }

        let mut memories: Vec<&MemoryCitation> = self.memory_records.iter().collect();
        memories.sort_by(|a, b| b.importance.total_cmp(&a.importance).then(b.id.cmp(&a.id)));
        lines.push("Memory influences:".to_string());
        if memories.is_empty() {
            lines.push("  (no cited memories)".to_string());
        } else {
            for memory in memories.iter().take(3) {
                lines.push(format!(
                    "  - [{} #{}] importance {:.2}: {}",
                    memory.kind, memory.id, memory.importance, memory.summary
                ));
            }
        }

        lines.join("\n")
    }
}

/// Short description of a validation result for explanations.
fn describe_validation(result: &ValidationResult) -> &'static str {
    match result {
        ValidationResult::Accepted(_) => "accepted",
        ValidationResult::Repaired(_) => "repaired",
        ValidationResult::Rejected { .. } => "rejected",
    }
}

fn validation_label(result: &ValidationResult) -> String {
    match result {
        ValidationResult::Rejected { reason } => format!("rejected: {reason}"),
        _ => describe_validation(result).to_string(),
    }
}

/// Outcome of one [`Sim::deliver_decision`] call.
#[derive(Debug, Clone)]
pub struct DecisionOutcome {
    /// The agent the decision was delivered for.
    pub agent: PrisonerId,
    /// The decision as validated (after normalization).
    pub decision: Decision,
    /// Validation result.
    pub result: ValidationResult,
    /// Whether the decision was applied to the world.
    pub applied: bool,
}

/// Aggregate result of one bounded decision batch.
#[derive(Debug, Clone, Default)]
pub struct DecisionBatchOutcome {
    /// Per-proposal outcomes in deterministic prisoner-id order.
    pub outcomes: Vec<DecisionOutcome>,
    /// Number of accepted or repaired proposals.
    pub accepted: usize,
    /// Number of rejected proposals.
    pub rejected: usize,
    /// Number of proposals actually applied to the world.
    pub applied: usize,
}

/// Validate + apply one decision on the engine.
///
/// Audit policy: applied decisions enter [`Sim::decision_log`] and every
/// outcome (including rejections) enters [`Sim::decision_audit_log`] with its
/// validation status, so observers can trace why a proposal was accepted,
/// repaired or rejected. Only applied decisions mutate world state.
pub fn deliver(sim: &mut Sim, id: PrisonerId, decision: Decision) -> DecisionOutcome {
    tracing::debug!(agent = id.raw(), ?decision, "deliver_decision");
    let (ok, result) = validate(&sim.world, id, &decision);
    let validated = result
        .accepted_decision()
        .cloned()
        .unwrap_or_else(|| decision.clone());
    let label = validation_label(&result);
    if !ok {
        let reason = result
            .rejected_reason()
            .unwrap_or("validation failed")
            .to_string();
        sim.record_decision_audit(crate::DecisionAuditRecord {
            time: sim.world.time,
            actor: format!("prisoner:{}", id.raw()),
            source: "agent".to_string(),
            action: decision.action_name().to_string(),
            summary: format!("{:?}", decision),
            validation: label,
            reason,
            rationale: decision.rationale.clone(),
            repair_reason: decision.repair_reason.clone(),
            outcome: decision.outcome,
        });
        return DecisionOutcome {
            agent: id,
            decision,
            result,
            applied: false,
        };
    }

    let mut applied_decision = validated;
    applied_decision.outcome = match &result {
        ValidationResult::Accepted(_) => alc_agents::decision::DecisionOutcomeState::Accepted,
        ValidationResult::Repaired(_) => alc_agents::decision::DecisionOutcomeState::Repaired,
        ValidationResult::Rejected { .. } => unreachable!("rejected result handled above"),
    };
    let apply_result = match &result {
        ValidationResult::Accepted(_) => ValidationResult::Accepted(applied_decision.clone()),
        ValidationResult::Repaired(_) => ValidationResult::Repaired(applied_decision.clone()),
        ValidationResult::Rejected { .. } => unreachable!("rejected result handled above"),
    };
    let mut events = sim.world.apply_validated_decision(id, &apply_result);
    if events.iter().any(|e| e.kind == EventKind::EscapeSucceeded) {
        sim.engine_state.post_release.insert(id, false);
    }
    sim.record_decision(applied_decision.clone());
    sim.record_decision_audit(crate::DecisionAuditRecord {
        time: sim.world.time,
        actor: format!("prisoner:{}", id.raw()),
        source: "agent".to_string(),
        action: applied_decision.action_name().to_string(),
        summary: format!("{:?}", applied_decision),
        validation: label,
        reason: "applied by engine".to_string(),
        rationale: applied_decision.rationale.clone(),
        repair_reason: applied_decision.repair_reason.clone(),
        outcome: applied_decision.outcome,
    });
    // Emits with bus-assigned ids and mirrors conflict kinds exactly once.
    sim.emit_events(&mut events);
    DecisionOutcome {
        agent: id,
        decision: applied_decision,
        result,
        applied: true,
    }
}

/// Deliver a bounded batch through one canonical validation slice per agent.
pub(crate) fn deliver_batch(
    sim: &mut Sim,
    requests: Vec<(PrisonerId, Decision)>,
) -> DecisionBatchOutcome {
    let cap = sim.world.ai_config().max_batch_size.max(1);
    let mut selected: BTreeMap<u64, (PrisonerId, Decision)> = BTreeMap::new();
    for (id, decision) in requests {
        if sim.world.prisoners.iter().any(|p| p.id == id) {
            selected.insert(id.raw(), (id, decision));
        }
    }
    while selected.len() > cap {
        selected.pop_last();
    }

    // Capture all contexts before applying anything in the batch.
    let mut prepared = Vec::with_capacity(selected.len());
    for (_, (id, decision)) in selected {
        if let Some(ctx) = sim.world.validation_context(id) {
            prepared.push((id, decision, ctx));
        }
    }
    sim.world.orchestrator.record_batch(prepared.len());

    let mut batch = DecisionBatchOutcome::default();
    let mut all_events = Vec::new();
    for (id, decision, ctx) in prepared {
        let validation = sim.world.validate_decision_in_context(id, &decision, &ctx);
        let accepted = validation.is_accepted();
        let label = validation_label(&validation.result);
        let reason = if accepted {
            "applied by engine batch".to_string()
        } else {
            validation
                .rejected_reason()
                .unwrap_or("validation failed")
                .to_string()
        };
        if accepted {
            let mut validated = validation
                .accepted_decision()
                .cloned()
                .expect("accepted validation carries a decision");
            validated.outcome = match &validation.result {
                ValidationResult::Accepted(_) => {
                    alc_agents::decision::DecisionOutcomeState::Accepted
                }
                ValidationResult::Repaired(_) => {
                    alc_agents::decision::DecisionOutcomeState::Repaired
                }
                ValidationResult::Rejected { .. } => unreachable!(),
            };
            let apply_result = match &validation.result {
                ValidationResult::Accepted(_) => ValidationResult::Accepted(validated.clone()),
                ValidationResult::Repaired(_) => ValidationResult::Repaired(validated.clone()),
                ValidationResult::Rejected { .. } => unreachable!(),
            };
            all_events.extend(sim.world.apply_validated_decision(id, &apply_result));
            if all_events
                .iter()
                .any(|e| e.kind == EventKind::EscapeSucceeded)
            {
                sim.engine_state.post_release.insert(id, false);
            }
            sim.record_decision(validated.clone());
            sim.world
                .orchestrator
                .record_validated(matches!(validation.result, ValidationResult::Repaired(_)));
            batch.accepted += 1;
            batch.applied += 1;
        } else {
            sim.world.orchestrator.record_rejected();
            batch.rejected += 1;
        }
        sim.record_decision_audit(crate::DecisionAuditRecord {
            time: sim.world.time,
            actor: format!("prisoner:{}", id.raw()),
            source: "agent".to_string(),
            action: decision.action_name().to_string(),
            summary: format!("{:?}", decision),
            validation: label,
            reason,
            rationale: decision.rationale.clone(),
            repair_reason: decision.repair_reason.clone(),
            outcome: decision.outcome,
        });
        batch.outcomes.push(DecisionOutcome {
            agent: id,
            decision,
            result: validation.result,
            applied: accepted,
        });
    }
    sim.emit_events(&mut all_events);
    batch
}

/// Coarse severity mapping for mirrored conflict incidents.
fn severity_for(kind: EventKind) -> f32 {
    match kind {
        EventKind::Argument | EventKind::Rumor => 0.25,
        EventKind::Threat | EventKind::Theft => 0.5,
        EventKind::Protests | EventKind::GangClash => 0.75,
        EventKind::Fight | EventKind::MedicalEmergency => 0.9,
        _ => 0.5,
    }
}

/// Validate a decision through the World's canonical gate.
fn validate(world: &World, id: PrisonerId, d: &Decision) -> (bool, ValidationResult) {
    let validation = world.validate_decision_for(id, d);
    (validation.is_accepted(), validation.result)
}

/// Dry-run validation shared by proposals, candidate evaluation and delivery.
///
/// This is a thin adapter over [`World::validate_decision_for`] so every
/// public surface validates through one canonical, bounded world slice.
pub(crate) fn validate_with_engine_rules(
    world: &World,
    id: PrisonerId,
    decision: &Decision,
) -> ValidationResult {
    world.validate_decision_for(id, decision).result
}
