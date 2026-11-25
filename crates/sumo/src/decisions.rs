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
use alc_agents::{
    Decision, DecisionAction, DecisionValidator, DefaultValidator, ValidationContext,
    ValidationResult,
};
use alc_core::{PrisonerId, RngStream, SimTime};
use alc_economy::{Account as EconomyAccount, Money};
use alc_events::{EventKind, WorldEvent};
use alc_prisoner::Prisoner;
use alc_social::{
    apply_escape_consequences, apply_outcome, resolve_conflict, resolve_escape_attempt,
    ConflictOutcome, EscapeOutcome, RelationshipDelta,
};
use alc_world::World;
use serde::{Deserialize, Serialize};

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
            DecisionType::MoveTo => Decision::new(DecisionAction::MoveTo { target_facility: target.map_or(0, |pid| pid.raw()) }),
            DecisionType::Socialize => Decision::new(DecisionAction::Socialize { with: target.map_or(0, |pid| pid.raw()) }),
            DecisionType::Fight => Decision::new(DecisionAction::Fight { target: target.map_or(0, |pid| pid.raw()) }),
            DecisionType::Trade => Decision::new(DecisionAction::Trade { with: target.map_or(0, |pid| pid.raw()), item: "".into() }),
            DecisionType::Comply => Decision::new(DecisionAction::Comply),
            DecisionType::Refuse => Decision::new(DecisionAction::Refuse),
            DecisionType::EscapeAttempt => Decision::new(DecisionAction::EscapeAttempt),
            DecisionType::RequestMedical => Decision::new(DecisionAction::RequestMedical),
            DecisionType::JoinGang => Decision::new(DecisionAction::JoinGang { gang: target.map_or(0, |pid| pid.raw()) }),
            DecisionType::LeaveGang => Decision::new(DecisionAction::LeaveGang),
            DecisionType::LeadGang => Decision::new(DecisionAction::LeadGang { gang: target.map_or(0, |pid| pid.raw()) }),
            DecisionType::SpreadRumor => Decision::new(DecisionAction::SpreadRumor { about: target.map_or(0, |pid| pid.raw()), content: "".into() }),
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
    EvaluatorScore { drive: name.to_string(), score: clamp01(score), weight: clamp01(weight) }
}

/// Clamp to `0..=1`, mapping non-finite values to `0`.
fn clamp01(value: f32) -> f32 {
    if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 }
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
    let affinity = if weight_sum > 0.0 { clamp01(utility / weight_sum) } else { 0.0 };
    let max = scores.iter().map(|s| s.score).fold(f32::MIN, f32::max);
    let min = scores.iter().map(|s| s.score).fold(f32::MAX, f32::min);
    let spread = if max > 0.0 { ((max - min) / max).clamp(0.0, 1.0) } else { 0.0 };
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
        let importance =
            clamp01(severity_for(event.kind) * clamp01(event.confidence) * recency);
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
    let ctx = validation_context(world, id);

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
    proposals
}

/// Deterministic one-line rationale naming the strongest drive for a candidate.
fn candidate_rationale(
    prisoner: &Prisoner,
    decision_type: DecisionType,
    target: Option<PrisonerId>,
    scores: &[EvaluatorScore],
) -> String {
    let target_note = target.map(|t| format!(" target {}", t.raw())).unwrap_or_default();
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
        lines.push(format!("Validation: {}", describe_validation(&self.estimated_outcome)));
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

/// Validate + apply one decision on the engine.
///
/// Audit policy: only decisions that pass validation and are applied are
/// recorded via [`Sim::record_decision`]. Rejected proposals are reported in
/// the returned [`DecisionOutcome`] but deliberately left out of the audit log
/// so it represents decisions the engine actually carried out.
pub fn deliver(sim: &mut Sim, id: PrisonerId, decision: Decision) -> DecisionOutcome {
    tracing::debug!(agent = id.raw(), ?decision, "deliver_decision");
    let (ok, result) = validate(&sim.world, id, &decision);
    let outcome = DecisionOutcome {
        agent: id,
        decision: decision.clone(),
        result: result.clone(),
        applied: false,
    };
    if !ok {
        let reason = match &result {
            ValidationResult::Rejected { reason, .. } => reason.clone(),
            _ => "validation failed".to_string(),
        };
        sim.record_decision_audit(crate::DecisionAuditRecord {
            time: sim.world.time,
            actor: format!("prisoner:{}", id.raw()),
            source: "agent".to_string(),
            action: decision.action_name().to_string(),
            summary: format!("{:?}", decision),
            validation: "rejected".to_string(),
            reason,
            rationale: decision.rationale.clone(),
            repair_reason: decision.repair_reason.clone(),
            outcome: decision.outcome,
        });
        return outcome;
    }
    let events = apply(&mut sim.world, id, &result);
    let validation = match &result {
        ValidationResult::Repaired(_) => "repaired".to_string(),
        ValidationResult::Accepted(_) => "accepted".to_string(),
        ValidationResult::Rejected { reason } => format!("rejected: {reason}"),
    };
    sim.record_decision(decision.clone());
    sim.record_decision_audit(crate::DecisionAuditRecord {
        time: sim.world.time,
        actor: format!("prisoner:{}", id.raw()),
        source: "agent".to_string(),
        action: decision.action_name().to_string(),
        summary: format!("{:?}", decision),
        validation,
        reason: "applied by engine".to_string(),
        rationale: decision.rationale.clone(),
        repair_reason: decision.repair_reason.clone(),
        outcome: decision.outcome,
    });
    sim.emit_events(events.clone());
    mirror_conflict_incidents(sim, &events);
    DecisionOutcome { agent: id, decision, result, applied: true }
}

/// Mirror conflict events produced by a delivered decision into the engine's
/// incident log and sidecar (R1/R6). Only conflict-kind events are mirrored;
/// routine social/rumor events remain on the world event bus only.
fn mirror_conflict_incidents(sim: &mut Sim, events: &[WorldEvent]) {
    use crate::incidents::incident_from_world_event;

    // Derive the next id from the highest existing sidecar id (at least the
    // length, so an empty sidecar starts at 1). Using max+1 rather than
    // len+1 avoids collisions after loading a save whose incident ids were
    // assigned by the lead's tick pump and then compacted/re-indexed.
    let mut next_id = sim
        .incident_sidecar()
        .iter()
        .map(|inc| inc.id)
        .max()
        .unwrap_or(0)
        .max(sim.incident_sidecar().len() as u64)
        + 1;
    let known_prisoners: Vec<PrisonerId> =
        sim.world.prisoners.iter().map(|p| p.id).collect();
    for event in events {
        if !is_conflict_kind(event.kind) {
            continue;
        }
        let id = next_id;
        next_id += 1;

        sim.record_incident(event.clone());

        let location = current_facility(&sim.world, id_of_first_actor(event, &sim.world))
            .map(|fid| format!("facility {fid}"))
            .unwrap_or_else(|| "unknown facility".to_string());

        // Snapshot the current sidecar so this incident can link to a real
        // prior cause instead of a self-referential placeholder.
        let prior: Vec<_> = sim.incident_sidecar().to_vec();
        let incident = incident_from_world_event(
            event,
            id,
            &prior,
            &known_prisoners,
            severity_for(event.kind),
            location,
        );
        sim.record_sidecar_incident(incident);
    }
}

/// First actor of an event that corresponds to a known prisoner.
fn id_of_first_actor(event: &WorldEvent, world: &World) -> PrisonerId {
    event
        .actors
        .iter()
        .copied()
        .find(|raw| world.prisoners.iter().any(|p| p.id.raw() == *raw))
        .map(PrisonerId)
        .unwrap_or_else(|| world.prisoners.first().map(|p| p.id).unwrap_or(PrisonerId(0)))
}

/// Whether an event kind represents a conflict incident worth mirroring.
fn is_conflict_kind(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Fight
            | EventKind::Argument
            | EventKind::Threat
            | EventKind::Theft
            | EventKind::GangClash
            | EventKind::Protests
            | EventKind::MedicalEmergency
    )
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

/// Validate a decision: default validator, then engine-side rules.
fn validate(world: &World, id: PrisonerId, d: &Decision) -> (bool, ValidationResult) {
    if !world.prisoners.iter().any(|p| p.id == id) {
        return (
            false,
            ValidationResult::Rejected { reason: format!("unknown prisoner id {}", id.raw()) },
        );
    }
    let ctx = validation_context(world, id);
    let v = DefaultValidator.validate(d, &ctx);
    if matches!(v, ValidationResult::Rejected { .. }) {
        return (false, v);
    }
    if matches!(v, ValidationResult::Repaired(_)) {
        return (true, v);
    }
    engine_rules(world, id, d)
}

/// Build the single, canonical validation context for a prisoner.
///
/// This is shared by the engine-side decision pipeline. `lib.rs` currently
/// duplicates this construction in `propose_decision`/`validate_only`; those
/// should be switched to this helper by the lead so all three paths agree on
/// facility, nearby guards, visible entities and contraband state.
pub(crate) fn validation_context(world: &World, id: PrisonerId) -> ValidationContext {
    // Real location from the movement map (not the schedule wing).
    let facility = world.prisoner_location.get(&id).copied();
    let guards_nearby = facility
        .map(|f| {
            world
                .guard_location
                .values()
                .filter(|loc| **loc == f)
                .count() as u32
        })
        .unwrap_or(0);
    // Visibility is facility-local: self, same-facility prisoners and guards.
    let mut visible_entities: Vec<u64> = Vec::new();
    if let Some(f) = facility {
        for (pid, loc) in &world.prisoner_location {
            if *loc == f {
                visible_entities.push(pid.raw());
            }
        }
        for (gid, loc) in &world.guard_location {
            if *loc == f {
                visible_entities.push(gid.raw());
            }
        }
    }
    if !visible_entities.contains(&id.raw()) {
        visible_entities.push(id.raw());
    }

    ValidationContext {
        current_facility: facility.map(|f| f.0).unwrap_or(0),
        lockdown: world.policies.lockdown,
        stamina: world
            .prisoners
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.condition.hope)
            .unwrap_or(1.0),
        tokens_remaining_today: world.orchestrator.token_budget_per_agent,
        guards_nearby,
        agent_id: id.raw(),
        role: alc_agents::Role::Prisoner,
        in_gang: world.prisoners.iter().any(|p| p.id == id && p.gang_affiliation.is_some()),
        gang_id: world
            .prisoners
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.gang_affiliation),
        is_gang_leader: world
            .gangs
            .iter()
            .any(|g| g.leader.map(|l| l.raw()) == Some(id.raw())),
        visible_entities,
        known_facilities: world.facilities.iter().map(|f| f.id.0).collect(),
        // TODO: model real contraband per prisoner. No contraband field exists
        // yet, so skills are used as the closest available proxy.
        has_contraband: world
            .prisoners
            .iter()
            .find(|p| p.id == id)
            .map(|p| !p.skills.is_empty())
            .unwrap_or(false),
        last_decision: world.current_decisions.get(&id).cloned(),
        now_secs: world.time.as_secs(),
    }
}

/// Resolve the prisoner's current facility id from the world movement map.
/// Returns `None` when the prisoner has no recorded location.
fn current_facility(world: &World, id: PrisonerId) -> Option<u64> {
    world.prisoner_location.get(&id).map(|f| f.0)
}

/// Engine-side validation rules (Directive §19 world-state / authority layers).
/// Validate a decision through BOTH the agent validator and the engine rules.
/// All public validation surfaces (delivery, proposals, dry-runs and candidate
/// evaluation) use this so they agree on the outcome for the same world state.
pub(crate) fn validate_with_engine_rules(world: &World, id: PrisonerId, decision: &Decision) -> ValidationResult {
    let ctx = validation_context(world, id);
    let base = alc_agents::validate_decision(decision, &ctx);
    match base {
        ValidationResult::Rejected { .. } => base,
        ValidationResult::Repaired(_) => base,
        ValidationResult::Accepted(_) => engine_rules(world, id, decision).1,
    }
}

pub(crate) fn engine_rules(world: &World, id: PrisonerId, d: &Decision) -> (bool, ValidationResult) {
    let reject = |reason: String| (false, ValidationResult::Rejected { reason });
    let ok = || (true, ValidationResult::Accepted(d.clone()));
    match &d.action {
        DecisionAction::MoveTo { target_facility } => {
            if world.policies.lockdown {
                return reject("lockdown active: movement restricted".into());
            }
            if !world.facilities.iter().any(|f| f.id.0 == *target_facility) {
                return reject(format!("unknown facility {target_facility}"));
            }
            ok()
        }
        DecisionAction::Socialize { with } | DecisionAction::Trade { with, .. } => {
            if !world.prisoners.iter().any(|p| p.id.raw() == *with) {
                return reject("unknown target prisoner".into());
            }
            ok()
        }
        DecisionAction::Fight { target } => {
            if !world.prisoners.iter().any(|p| p.id.raw() == *target) {
                return reject("unknown target prisoner".into());
            }
            ok()
        }
        DecisionAction::SpreadRumor { about, content } => {
            if !world.prisoners.iter().any(|p| p.id.raw() == *about) {
                return reject("unknown target prisoner".into());
            }
            if content.trim().is_empty() {
                return reject("rumor content is empty".into());
            }
            ok()
        }
        DecisionAction::JoinGang { gang } => {
            if !world.gangs.iter().any(|g| g.id.raw() == *gang) {
                return reject(format!("unknown gang {gang}"));
            }
            ok()
        }
        DecisionAction::LeadGang { gang } => match world.gangs.iter().find(|g| g.id.raw() == *gang) {
            None => reject(format!("unknown gang {gang}")),
            Some(g) if !g.members.iter().any(|&m| m == id) => {
                reject("prisoner is not a member of the gang".into())
            }
            Some(_) => ok(),
        },
        DecisionAction::EscapeAttempt if world.policies.lockdown => {
            reject("lockdown active: escape attempts blocked".into())
        }
        DecisionAction::EscapeAttempt => ok(),
        DecisionAction::RequestMedical | DecisionAction::Comply | DecisionAction::Refuse | DecisionAction::LeaveGang | DecisionAction::None => ok(),
        // All 13 current variants are handled above. The arm is kept as a
        // defensive guard: if a future variant is added to Decision without
        // an engine rule, it must be rejected instead of silently accepted as
        // a no-op. It is unreachable today, hence the explicit allow.
        #[allow(unreachable_patterns)]
        other => reject(format!("unsupported decision {:?}", other)),
    }
}

/// Apply a validated decision to the world; return events to emit.
fn apply(world: &mut World, id: PrisonerId, result: &ValidationResult) -> Vec<WorldEvent> {
    let mut events = Vec::new();
    let decision = match result {
        ValidationResult::Accepted(d) | ValidationResult::Repaired(d) => d.clone(),
        ValidationResult::Rejected { .. } => return events,
    };
    match decision.action {
        DecisionAction::MoveTo { target_facility } => {
            if let Some(f) = world.facilities.iter().find(|f| f.id.0 == target_facility) {
                if let Some(p) = world.prisoners.iter_mut().find(|p| p.id == id) {
                    p.schedule.wing = f.wing;
                }
                // Keep the movement map authoritative: MapFrame, co-presence
                // encounters and validation visibility all read this map.
                world.prisoner_location.insert(id, f.id);
            }
        }
        DecisionAction::Socialize { with } => {
            world.social.record(
                id,
                PrisonerId(with),
                RelationshipDelta { familiarity: 0.08, friendship: 0.04, trust: 0.02, ..Default::default() },
            );
            events.push(ev(world.time, EventKind::RelationshipShift, format!("{} socialized with {with}", id.raw()), vec![id.raw()]));
        }
        DecisionAction::Trade { with, item } => {
            let other = PrisonerId(with);
            let both_exist = world.prisoners.iter().any(|p| p.id == id)
                && world.prisoners.iter().any(|p| p.id == other);
            if both_exist {
                // Prisoners have no personal wallets in the current World model
                // (economy.cash is the institution's ledger). Model the trade
                // as a small, balanced pair of ledger entries in the prison
                // economy plus a familiarity gain. TODO: wire a real prisoner
                // cash ledger when one exists so value transfers between
                // inmates rather than only being observable in the ledger.
                let amount = Money::from_major(1);
                let at = world.time.as_secs();
                world.economy.record(at, EconomyAccount::Supplies, amount, format!("trade credit: {} -> {} ({item})", id.raw(), with));
                world.economy.record(at, EconomyAccount::Supplies, Money(-amount.0), format!("trade debit: {} -> {} ({item})", id.raw(), with));
            }
            // A trade is a stronger social signal than a casual conversation.
            let delta = RelationshipDelta {
                familiarity: 0.12,
                friendship: 0.03,
                trust: 0.01,
                ..Default::default()
            };
            world.social.record(id, other, delta);
            events.push(ev(
                world.time,
                EventKind::RelationshipShift,
                format!("{} traded with {with} ({item})", id.raw()),
                vec![id.raw(), with],
            ));
        }
        DecisionAction::Fight { target } => events.extend(fight_events(world, id, PrisonerId(target))),
        DecisionAction::EscapeAttempt => events.extend(escape_events(world, id)),
        DecisionAction::SpreadRumor { about, content } => {
            events.push(ev(world.time, EventKind::Rumor, format!("{} spreads rumor about {about}: {content}", id.raw()), vec![id.raw()]));
        }
        DecisionAction::JoinGang { gang } => {
            if let Some(p) = world.prisoners.iter_mut().find(|p| p.id == id) {
                p.gang_affiliation = Some(gang);
            }
            events.push(ev(world.time, EventKind::GangRecruit, format!("{} joins gang {gang}", id.raw()), vec![id.raw()]));
        }
        DecisionAction::LeadGang { gang } => {
            if let Some(g) = world.gangs.iter_mut().find(|g| g.id.raw() == gang) {
                g.change_leader(id);
            }
            events.push(ev(world.time, EventKind::LeadershipDispute, format!("{} takes leadership of gang {gang}", id.raw()), vec![id.raw()]));
        }
        DecisionAction::LeaveGang => {
            if let Some(p) = world.prisoners.iter_mut().find(|p| p.id == id) {
                p.gang_affiliation = None;
            }
        }
        DecisionAction::RequestMedical => {
            if let Some(p) = world.prisoners.iter_mut().find(|p| p.id == id) {
                p.needs.medical_need = (p.needs.medical_need + 0.35).min(1.0);
            }
        }
        DecisionAction::Comply | DecisionAction::Refuse | DecisionAction::None => {}
    }
    events
}

/// Event helper: fresh event id (assigned by the bus) + time + actors.
fn ev(at: SimTime, kind: EventKind, summary: String, actors: Vec<u64>) -> WorldEvent {
    let mut e = WorldEvent::new(0, at, kind, summary);
    for a in actors {
        e = e.with_actor(a);
    }
    e
}

/// Resolve a fight into world effects and an event (Directive §8).
fn fight_events(world: &mut World, a: PrisonerId, b: PrisonerId) -> Vec<WorldEvent> {
    let Some(pa) = world.prisoners.iter().find(|p| p.id == a).cloned() else { return Vec::new() };
    if !world.prisoners.iter().any(|p| p.id == b) {
        return Vec::new();
    }
    let mut rng = RngStream::new(world.seed, world.time.secs);
    let tension = 0.5 + (pa.traits.aggression + pa.traits.aggression) * 0.15;
    let outcome =
        resolve_conflict(&mut rng, tension, 0.15, pa.aggression_modifier(), pa.traits.discipline, 0.1);
    let _effects = apply_outcome(&mut world.social, &mut world.gangs, &[a, b], outcome, 0.6, None);
    let kind = match outcome {
        ConflictOutcome::Compliance => None,
        ConflictOutcome::Retreat | ConflictOutcome::Argument => Some(EventKind::Argument),
        ConflictOutcome::Threat => Some(EventKind::Threat),
        ConflictOutcome::Intimidation | ConflictOutcome::Bullying => Some(EventKind::Threat),
        ConflictOutcome::Theft => Some(EventKind::Theft),
        ConflictOutcome::Fight => Some(EventKind::Fight),
        ConflictOutcome::GangClash | ConflictOutcome::Disturbance => Some(EventKind::GangClash),
    };
    match kind {
        Some(k) => vec![ev(world.time, k, format!("conflict resolved: {outcome:?}"), vec![a.raw(), b.raw()])],
        None => Vec::new(),
    }
}

/// Resolve an escape attempt into world consequences and an event (Directive §16).
fn escape_events(world: &mut World, p: PrisonerId) -> Vec<WorldEvent> {
    let Some(prisoner) = world.prisoners.iter().find(|x| x.id == p).cloned() else { return Vec::new() };
    let mut rng = RngStream::new(world.seed, world.time.secs);
    let outcome =
        resolve_escape_attempt(&mut rng, prisoner.escape_motivation(), world.policies.security_level, 0.5, 0.2);
    let _c = apply_escape_consequences(outcome, world.policies.security_level, &[p]);
    let kind = match outcome {
        EscapeOutcome::SuccessfulEscape => EventKind::EscapeSucceeded,
        EscapeOutcome::AttemptPrevented => EventKind::EscapePrevented,
        _ => EventKind::EscapeAttempt,
    };
    vec![ev(world.time, kind, format!("escape attempt outcome: {outcome:?}"), vec![p.raw()])]
}
