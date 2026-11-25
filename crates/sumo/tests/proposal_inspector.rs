//! Brain-inspector data tests: evaluator scores, confidence and memory
//! citations attached to decision proposals (Directive §19/§20).
//!
//! These tests exercise the pure [`evaluate_candidates`] entry point, which
//! only reads `&World`. No test mutates the world through the engine pipeline.

use alc_core::GangId;
use alc_prisoner::Needs;
use sumo::Sim;
use sumo::decisions::{DecisionType, evaluate_candidates};

const SEED: u64 = 7;

/// A fresh proposal carries at least three drives, a bounded confidence and
/// (because intake writes `memory_summary`) at least one memory citation.
///
/// If a future world genuinely has no memory sources, `memory_records` may be
/// empty by design; the test documents that intake currently guarantees one.
#[test]
fn proposal_contains_scores_confidence_and_memories() {
    let sim = Sim::new(SEED);
    let id = sim.world().prisoners[0].id;

    let proposals = evaluate_candidates(sim.world(), id);
    assert!(
        !proposals.is_empty(),
        "evaluate_candidates returned no candidates for {id:?}"
    );

    let top = &proposals[0];
    assert!(
        top.scores.len() >= 3,
        "top candidate {:?} has only {} evaluator drives: {:?}",
        top.decision_type,
        top.scores.len(),
        top.scores
    );
    assert!(top.confidence.is_finite(), "confidence must be finite");
    assert!(
        (0.0..=1.0).contains(&top.confidence),
        "confidence {} out of range",
        top.confidence
    );
    assert!(
        !top.memory_records.is_empty(),
        "fresh intake writes memory_summary, so at least one citation is expected"
    );
    for memory in &top.memory_records {
        assert!(memory.importance.is_finite());
        assert!(
            (0.0..=1.0).contains(&memory.importance),
            "memory importance {} out of range",
            memory.importance
        );
        assert!(
            !memory.summary.trim().is_empty(),
            "citation summary must not be empty"
        );
    }
}

/// The explanation names the strongest weighted drive and the memory section.
#[test]
fn proposal_explanation_mentions_top_drive() {
    let sim = Sim::new(SEED);
    let id = sim.world().prisoners[0].id;
    let top = &evaluate_candidates(sim.world(), id)[0];

    let explanation = top.explanation();
    assert!(
        explanation.contains("Top drives:"),
        "missing drive section:\n{explanation}"
    );
    assert!(
        explanation.contains("Memory influences:"),
        "missing memory section:\n{explanation}"
    );
    assert!(
        explanation.contains("Confidence:"),
        "missing confidence line:\n{explanation}"
    );

    let best = top
        .scores
        .iter()
        .max_by(|a, b| (a.score * a.weight).total_cmp(&(b.score * b.weight)))
        .expect("scores are non-empty");
    assert!(
        explanation.contains(&best.drive),
        "explanation does not mention top drive {:?}:\n{explanation}",
        best.drive
    );
}

/// Evaluation is pure and deterministic: repeated calls match field-by-field,
/// and no observable engine state (trace, events, audit log) changes.
#[test]
fn evaluate_candidates_is_pure_and_deterministic() {
    let sim = Sim::new(SEED);
    let id = sim.world().prisoners[0].id;

    let trace_before = sim.trace_hash();
    let events_before = sim.events().len();
    let decisions_before = sim.decision_log().len();

    let first = evaluate_candidates(sim.world(), id);
    let second = evaluate_candidates(sim.world(), id);

    assert_eq!(sim.trace_hash(), trace_before, "trace_hash changed");
    assert_eq!(sim.events().len(), events_before, "event bus changed");
    assert_eq!(
        sim.decision_log().len(),
        decisions_before,
        "decision log changed"
    );

    assert_eq!(
        first.len(),
        second.len(),
        "candidate count is not deterministic"
    );
    for (a, b) in first.iter().zip(second.iter()) {
        assert_eq!(a.prisoner_id, b.prisoner_id);
        assert_eq!(a.decision_type, b.decision_type);
        assert_eq!(a.target, b.target);
        assert_eq!(a.rationale, b.rationale);
        assert_eq!(
            a.scores, b.scores,
            "scores differ for {:?}",
            a.decision_type
        );
        assert_eq!(
            a.confidence, b.confidence,
            "confidence differs for {:?}",
            a.decision_type
        );
        assert_eq!(
            a.memory_records, b.memory_records,
            "memory citations differ for {:?}",
            a.decision_type
        );
        assert_eq!(a.trace_hash_before, b.trace_hash_before);
        assert_eq!(a.explanation(), b.explanation());
        // `ValidationResult` has no `PartialEq`; compare its contents instead.
        assert_eq!(
            a.estimated_outcome.accepted_decision(),
            b.estimated_outcome.accepted_decision()
        );
        assert_eq!(
            a.estimated_outcome.rejected_reason(),
            b.estimated_outcome.rejected_reason()
        );
    }
}

/// Contrasting dominant needs produce different top candidates: a prisoner who
/// feels unsafe seeks a gang, while a humiliated, aggressive prisoner picks a
/// fight. This proves needs actually reorder the evaluator output.
#[test]
fn candidate_ordering_differs_with_dominant_need() {
    let mut sim = Sim::new(SEED);
    let safety_seeker = sim.world().prisoners[0].id;
    let aggressor = sim.world().prisoners[1].id;
    let founder = sim.world().prisoners[2].id;
    let facility = sim.world().prisoner_location[&safety_seeker];

    // A gang must exist for JoinGang/LeadGang candidates to be generated.
    sim.world_mut()
        .gangs
        .push(alc_social::Gang::new(GangId(9001), "Testers", founder));
    // Co-locate both prisoners so social/fight candidates exist for both.
    sim.world_mut()
        .prisoner_location
        .insert(aggressor, facility);

    {
        let p = sim
            .world_mut()
            .prisoners
            .iter_mut()
            .find(|p| p.id == safety_seeker)
            .expect("safety seeker exists");
        p.needs = Needs::new();
        p.needs.safety = 1.0;
        p.needs.protection = 1.0;
        p.needs.belonging = 1.0;
        p.needs.status = 0.9;
        p.needs.respect = 0.0;
        p.needs.autonomy = 0.0;
        p.needs.freedom = 0.0;
        p.needs.stimulation = 0.0;
        p.needs.hunger = 0.0;
        p.needs.thirst = 0.0;
        p.needs.purpose = 0.0;
        p.traits.aggression = 0.0;
        p.traits.impulsiveness = 0.0;
        p.traits.manipulation = 0.0;
        p.traits.risk_tolerance = 0.0;
        p.condition.hope = 1.0;
    }
    {
        let p = sim
            .world_mut()
            .prisoners
            .iter_mut()
            .find(|p| p.id == aggressor)
            .expect("aggressor exists");
        p.needs = Needs::new();
        p.needs.respect = 1.0;
        p.needs.hunger = 0.9;
        p.needs.safety = 0.0;
        p.needs.protection = 0.0;
        p.needs.belonging = 0.0;
        p.needs.autonomy = 0.0;
        p.needs.freedom = 0.0;
        p.needs.stimulation = 0.0;
        p.needs.companionship = 0.0;
        p.needs.friendship = 0.0;
        p.needs.purpose = 0.0;
        p.needs.thirst = 0.0;
        p.needs.status = 0.0;
        p.needs.recognition = 0.0;
        p.needs.stability = 0.0;
        p.condition.hope = 0.9;
        p.traits.aggression = 0.9;
        p.traits.impulsiveness = 0.9;
        p.traits.manipulation = 0.0;
        p.traits.risk_tolerance = 0.0;
        p.traits.discipline = 0.0;
        p.traits.sociability = 0.0;
    }

    let seeker_proposals = evaluate_candidates(sim.world(), safety_seeker);
    let aggressor_proposals = evaluate_candidates(sim.world(), aggressor);
    assert!(!seeker_proposals.is_empty());
    assert!(!aggressor_proposals.is_empty());

    let seeker_top = seeker_proposals[0].decision_type;
    let aggressor_top = aggressor_proposals[0].decision_type;
    assert_eq!(
        seeker_top,
        DecisionType::JoinGang,
        "unsafe prisoner should seek a gang; ranking: {:?}",
        seeker_proposals
            .iter()
            .map(|p| (
                p.decision_type,
                p.scores.iter().map(|s| s.score * s.weight).sum::<f32>()
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        aggressor_top,
        DecisionType::Fight,
        "humiliated aggressive prisoner should pick a fight; ranking: {:?}",
        aggressor_proposals
            .iter()
            .map(|p| (
                p.decision_type,
                p.scores.iter().map(|s| s.score * s.weight).sum::<f32>()
            ))
            .collect::<Vec<_>>()
    );
    assert_ne!(
        seeker_top, aggressor_top,
        "contrasting needs must change the top candidate"
    );
}

/// End-to-end check that the engine-constructed proposal exposes the new
/// inspector fields.
///
/// Ignored until the lead applies the documented `lib.rs` edit: until then the
/// `DecisionProposal` struct literal in `Sim::propose_decision` is missing the
/// new fields, so this test target cannot compile. Enable once that lands.
#[test]
fn sim_propose_still_works() {
    let sim = Sim::new(SEED);
    let prisoner = sim.world().prisoners[0].id;

    let proposal = sim
        .propose_decision(prisoner, DecisionType::Comply, None)
        .expect("prisoner should be available for a proposal");

    assert_eq!(proposal.prisoner_id, prisoner);
    assert_eq!(proposal.decision_type, DecisionType::Comply);
    assert!(
        !proposal.scores.is_empty(),
        "engine proposal should carry evaluator drives"
    );
    assert!(proposal.confidence.is_finite());
    assert!((0.0..=1.0).contains(&proposal.confidence));
    assert!(!proposal.explanation().is_empty());
}
