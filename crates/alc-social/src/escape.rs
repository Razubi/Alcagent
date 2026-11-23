//! Escape attempts and prison crises per Directive §16.

use alc_core::{PrisonerId, RngStream};
use serde::{Deserialize, Serialize};

/// Escape attempt outcomes (Directive §16), preserved for compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscapeOutcome {
    AttemptPrevented,
    DiscoveredPreAttempt,
    DiscoveredDuring,
    DisruptedNoEscape,
    PartialBreachContained,
    SuccessfulEscape,
}

impl Default for EscapeOutcome {
    fn default() -> Self {
        EscapeOutcome::AttemptPrevented
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscapeVector {
    Perimeter,
    Transport,
    WorkDetail,
    VisitorArea,
    StaffCompromise,
    Other,
}

/// Inputs for escape motivation computation from needs/traits/sentence/institutional conditions.
#[derive(Debug, Clone, Default)]
pub struct EscapeMotivationInput {
    // Needs / psychological pressures (0..1; higher means more pressure toward escape)
    pub fear_for_safety: f32,
    pub desperation_hopelessness: f32,
    pub perceived_injustice: f32,
    pub isolation: f32,
    pub belonging_inside: f32, // protective factor
    pub hope: f32,             // protective factor
    // Traits
    pub risk_tolerance: f32,
    pub impulsiveness: f32,
    pub discipline: f32, // protective factor
    pub intelligence: f32,
    // Sentence / legal situation
    pub sentence_remaining_ratio: f32, // 0..1 of sentence left
    pub recent_setback: f32,           // parole denial, discipline, etc.
    pub outside_relationship_pull: f32,
    // Institutional conditions
    pub institutional_harshness: f32,
    pub security_weakness_belief: f32, // epistemic/perceived opportunity
    pub staff_corruption_belief: f32,
    pub gang_coercion: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EscapeMotivation {
    pub total: f32, // 0..1
    pub fear_component: f32,
    pub desperation_component: f32,
    pub sentence_component: f32,
    pub injustice_component: f32,
    pub opportunity_component: f32,
}

/// Compute escape motivation from needs, traits, sentence, and institutional conditions.
pub fn compute_escape_motivation(input: &EscapeMotivationInput) -> EscapeMotivation {
    let fear_component =
        (input.fear_for_safety * 0.7 + input.gang_coercion * 0.3).clamp(0.0, 1.0);
    let desperation_component = (input.desperation_hopelessness * 0.45
        + input.isolation * 0.2
        + (1.0 - input.hope).clamp(0.0, 1.0) * 0.2
        + (1.0 - input.belonging_inside).clamp(0.0, 1.0) * 0.15)
        .clamp(0.0, 1.0);
    let sentence_component = (input.sentence_remaining_ratio * 0.6
        + input.recent_setback * 0.25
        + input.outside_relationship_pull * 0.15)
        .clamp(0.0, 1.0);
    let injustice_component = (input.perceived_injustice * 0.6
        + input.institutional_harshness * 0.4)
        .clamp(0.0, 1.0);
    let opportunity_component = (input.security_weakness_belief * 0.7
        + input.staff_corruption_belief * 0.3)
        .clamp(0.0, 1.0);

    let drivers = fear_component * 0.22
        + desperation_component * 0.22
        + sentence_component * 0.18
        + injustice_component * 0.18
        + opportunity_component * 0.2;
    let trait_amp = 0.7
        + input.risk_tolerance * 0.25
        + input.impulsiveness * 0.15
        + input.intelligence * 0.05
        - input.discipline * 0.2;
    let total = (drivers * trait_amp).clamp(0.0, 1.0);

    EscapeMotivation {
        total,
        fear_component,
        desperation_component,
        sentence_component,
        injustice_component,
        opportunity_component,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EscapeConsequences {
    pub outcome: EscapeOutcome,
    pub security_level_delta: f32,
    pub surveillance_delta: f32,
    pub lockdown_recommended: bool,
    pub investigation_opened: bool,
    pub investigation_scope: f32, // 0..1
    pub staff_alert_delta: f32,
    pub policy_pressure: f32,
    pub prisoner_discipline_severity: f32,
    pub notes: Vec<String>,
}

/// Resolve an abstract escape attempt from motivation + security conditions.
/// Preserves legacy signature/behavior for existing callers.
pub fn resolve_escape_attempt(
    rng: &mut RngStream,
    motivation: f32,
    security_level: f32,
    prisoner_determination: f32,
    staff_response: f32,
) -> EscapeOutcome {
    let chance = (motivation * 0.4
        + prisoner_determination * 0.3
        + (1.0 - security_level) * 0.2
        + (1.0 - staff_response) * 0.1)
        .clamp(0.05, 0.85) as f64;
    if rng.chance(chance) {
        if security_level > 0.7 {
            EscapeOutcome::PartialBreachContained
        } else {
            EscapeOutcome::SuccessfulEscape
        }
    } else if rng.chance(0.5) {
        EscapeOutcome::DiscoveredDuring
    } else {
        EscapeOutcome::AttemptPrevented
    }
}

/// Apply persistent consequences of an escape attempt (security changes, investigations, etc.).
pub fn apply_escape_consequences(
    outcome: EscapeOutcome,
    security_level: f32,
    participants: &[PrisonerId],
) -> EscapeConsequences {
    let mut c = EscapeConsequences {
        outcome,
        ..Default::default()
    };
    let participant_factor = (participants.len() as f32 * 0.05).min(0.3);

    match outcome {
        EscapeOutcome::AttemptPrevented => {
            c.security_level_delta = 0.03 + participant_factor * 0.2;
            c.surveillance_delta = 0.05;
            c.investigation_opened = false;
            c.investigation_scope = 0.1;
            c.staff_alert_delta = 0.05;
            c.policy_pressure = 0.05;
            c.prisoner_discipline_severity = 0.4;
            c.notes.push("attempt prevented; minor procedural review".into());
        }
        EscapeOutcome::DiscoveredPreAttempt => {
            c.security_level_delta = 0.06 + participant_factor * 0.3;
            c.surveillance_delta = 0.1;
            c.investigation_opened = true;
            c.investigation_scope = 0.5 + participant_factor;
            c.staff_alert_delta = 0.1;
            c.policy_pressure = 0.15;
            c.prisoner_discipline_severity = 0.5;
            c.notes.push("plot discovered pre-attempt; investigation initiated".into());
        }
        EscapeOutcome::DiscoveredDuring => {
            c.security_level_delta = 0.08 + participant_factor * 0.3;
            c.surveillance_delta = 0.12;
            c.lockdown_recommended = true;
            c.investigation_opened = true;
            c.investigation_scope = 0.65 + participant_factor;
            c.staff_alert_delta = 0.15;
            c.policy_pressure = 0.25;
            c.prisoner_discipline_severity = 0.7;
            c.notes.push("attempt interrupted during execution; lockdown/investigation likely".into());
        }
        EscapeOutcome::DisruptedNoEscape => {
            c.security_level_delta = 0.1 + participant_factor * 0.3;
            c.surveillance_delta = 0.15;
            c.lockdown_recommended = true;
            c.investigation_opened = true;
            c.investigation_scope = 0.75;
            c.staff_alert_delta = 0.18;
            c.policy_pressure = 0.35;
            c.prisoner_discipline_severity = 0.75;
            c.notes.push("major disruption without escape; institutional response required".into());
        }
        EscapeOutcome::PartialBreachContained => {
            c.security_level_delta = 0.15 + participant_factor * 0.4;
            c.surveillance_delta = 0.2;
            c.lockdown_recommended = true;
            c.investigation_opened = true;
            c.investigation_scope = 0.9;
            c.staff_alert_delta = 0.25;
            c.policy_pressure = 0.55;
            c.prisoner_discipline_severity = 0.9;
            c.notes.push("partial breach contained; serious security and policy review".into());
        }
        EscapeOutcome::SuccessfulEscape => {
            c.security_level_delta = 0.25 + participant_factor * 0.5;
            c.surveillance_delta = 0.3;
            c.lockdown_recommended = true;
            c.investigation_opened = true;
            c.investigation_scope = 1.0;
            c.staff_alert_delta = 0.35;
            c.policy_pressure = 0.9;
            c.prisoner_discipline_severity = 1.0;
            c.notes.push("successful escape; persistent institutional and external consequences".into());
        }
    }

    // Diminishing returns if already very secure.
    c.security_level_delta *= (1.1 - security_level.clamp(0.0, 1.0) * 0.5).max(0.4);
    c.surveillance_delta = c.surveillance_delta.clamp(0.0, 1.0);
    c.policy_pressure = c.policy_pressure.clamp(0.0, 1.0);
    c
}