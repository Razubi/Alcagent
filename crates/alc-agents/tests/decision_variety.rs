//! A simulated day of need-driven decisions must not collapse into one action.
//!
//! This mirrors `alc-world`'s hourly loop at the boundary between owned crates:
//! needs drift with the schedule, a dominant need yields an action candidate
//! set, and the deterministic utility chooser (with personality bias) decides.
//! Every produced decision is pushed through the eight-stage validation
//! pipeline, so the test covers the whole `alc-agents` decision path.

use alc_agents::{
    ActionCandidate, Decision, DecisionAction, Disposition, Legality, Role, ValidationContext, ValidationResult,
    fallback_utility_decision_with_disposition, validate_decision,
};
use alc_core::PrisonerId;
use alc_prisoner::{DriftActivity, NeedDriftContext, NeedTag, Needs, generate_intake};
use std::collections::HashSet;

/// Coarse hourly view of the medium-security weekday, with a 09:00 sick call.
fn activity_at(minute: u16) -> DriftActivity {
    use DriftActivity as A;
    match minute {
        0..360 => A::Sleep,
        360..390 => A::None,
        390..435 => A::Eat,
        435..450 => A::None,
        450..540 => A::Work,
        540..600 => A::Medical,
        600..690 => A::Work,
        690..705 => A::None,
        705..750 => A::Eat,
        750..765 => A::None,
        765..825 => A::Work,
        825..840 => A::None,
        840..930 => A::Yard,
        930..945 => A::None,
        945..990 => A::Recreation,
        990..1050 => A::None,
        1050..1095 => A::Eat,
        1095..1110 => A::None,
        1110..1200 => A::Recreation,
        1200..1230 => A::Visitor,
        1230..1440 => A::Lockdown,
        _ => A::Sleep,
    }
}

fn relief_for(activity: DriftActivity) -> Vec<NeedTag> {
    use DriftActivity as A;
    use NeedTag as N;
    match activity {
        A::Eat => vec![N::Hunger, N::Thirst],
        A::Sleep => vec![N::Sleep],
        A::Work => vec![N::Purpose],
        A::Yard => vec![N::Exercise],
        A::Recreation => vec![N::Stimulation, N::Companionship],
        A::Visitor => vec![N::FamilyContact],
        A::Medical => vec![N::Medical],
        _ => Vec::new(),
    }
}

/// Same shape as the world's need→candidate translation: the dominant need
/// supplies a motivated action, plus routine baselines for a real ranking.
fn candidates_for(needs: &Needs, nearby: u64) -> Vec<ActionCandidate> {
    let (need, level) = needs.dominant();
    let level = level.clamp(0.0, 1.0);
    let motivated = match need {
        NeedTag::Hunger | NeedTag::Thirst => Some(ActionCandidate::new(
            Decision::new(DecisionAction::MoveTo { target_facility: 11 }),
            0.2 + level,
        )),
        NeedTag::Sleep | NeedTag::Fatigue => Some(ActionCandidate::new(
            Decision::new(DecisionAction::MoveTo { target_facility: 12 }),
            0.2 + level,
        )),
        NeedTag::Stimulation => Some(ActionCandidate::new(
            Decision::new(DecisionAction::MoveTo { target_facility: 13 }),
            0.2 + level,
        )),
        NeedTag::Medical => Some(ActionCandidate::new(Decision::new(DecisionAction::RequestMedical), 0.2 + level * 1.3)),
        NeedTag::Companionship => Some(ActionCandidate::new(
            Decision::new(DecisionAction::Socialize { with: nearby }),
            0.2 + level,
        )),
        NeedTag::Safety => Some(ActionCandidate::new(Decision::new(DecisionAction::Comply), 0.2 + level)),
        NeedTag::Respect => Some(
            ActionCandidate::new(Decision::new(DecisionAction::Fight { target: nearby }), 0.15 + level * 0.5)
                .with_risk(0.85)
                .with_legality(Legality::IllegalButPossible),
        ),
        _ => None,
    };

    let mut out = Vec::with_capacity(4);
    if let Some(c) = motivated {
        out.push(c);
    }
    out.push(ActionCandidate::new(Decision::new(DecisionAction::Comply), 0.25));
    if needs.medical_need > 0.6 {
        out.push(ActionCandidate::new(Decision::new(DecisionAction::RequestMedical), 0.55));
    }
    out.push(ActionCandidate::new(Decision::new(DecisionAction::None), 0.05));
    out
}

#[test]
fn needs_driven_decisions_vary_across_a_simulated_day() {
    let mut p = generate_intake(909, PrisonerId(1));
    // A sick, isolated newcomer: four distinct pressures across one day.
    p.condition.illness = 0.85;
    p.needs.medical_need = 0.85;
    p.needs.safety = 0.0;
    p.needs.protection = 0.0;
    p.needs.companionship = 0.85;
    p.traits.sociability = 0.95;
    p.needs.hunger = 0.25;
    p.needs.thirst = 0.25;
    p.needs.sleep = 0.10;
    p.needs.fatigue = 0.0;

    let disposition = Disposition::from(&p);
    let validation_ctx = ValidationContext {
        agent_id: p.id.raw(),
        role: Role::Prisoner,
        stamina: 1.0,
        ..Default::default()
    };

    let mut seen: HashSet<&'static str> = HashSet::new();
    let mut timeline = Vec::new();
    for hour in 0..30u32 {
        // Two explainable simulation events: an afternoon disturbance raises
        // perceived danger, then staff de-escalation relieves it. Both go
        // through the needs engine, so the resulting action shift is emergent
        // from state rather than scripted at the decision layer.
        if hour == 16 {
            p.needs.impulse(NeedTag::Safety, 0.7);
        }
        if hour == 22 {
            p.needs.relieve(NeedTag::Safety, 0.7);
        }
        let minute = ((hour * 60) % 1440) as u16;
        let activity = activity_at(minute);
        let relief = relief_for(activity);
        let drift = NeedDriftContext {
            activity,
            relief: &relief,
            crowd_density: 0.5,
            heat: 0.1,
            staff_coverage: 0.7,
            lockdown: activity == DriftActivity::Lockdown,
            ..NeedDriftContext::default()
        };
        p.needs.drift_with(60, &drift);

        let candidates = candidates_for(&p.needs, 42);
        let decision = fallback_utility_decision_with_disposition(&candidates, &disposition);
        let name = decision.action_name();
        match validate_decision(&decision, &validation_ctx) {
            ValidationResult::Accepted(_) | ValidationResult::Repaired(_) => {}
            ValidationResult::Rejected { reason } => {
                panic!("hour {hour}: decision {name} rejected by pipeline: {reason}")
            }
        }
        seen.insert(name);
        timeline.push((hour, name));
    }

    assert!(
        seen.len() >= 4,
        "only {} distinct decisions across 30 sim-hours: {timeline:?}",
        seen.len()
    );
    for expected in ["request_medical", "socialize", "move_to", "comply"] {
        assert!(seen.contains(expected), "never chose {expected}: {timeline:?}");
    }
}
