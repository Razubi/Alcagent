//! Personality → decision bias (Directive §3, §19).
//!
//! Needs answer *what* an agent wants; traits answer *how* it pursues it.
//! Before this module the utility chooser ranked only need-derived candidate
//! utilities, so `Traits` were decorative for routine decisions. A
//! [`Disposition`] is the read-only personality slice handed to the chooser;
//! [`trait_bias`] returns a bounded, deterministic additive adjustment per
//! action. All fields default to `0.5`, which is neutral (zero bias), so
//! existing callers of `fallback_utility_decision` are unchanged.

use alc_prisoner::{Prisoner, Traits};
use serde::{Deserialize, Serialize};

use crate::decision::{Decision, DecisionAction};

/// Read-only personality snapshot used to bias utility-based decisions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Disposition {
    pub discipline: f32,
    pub impulsiveness: f32,
    pub aggression: f32,
    pub emotional_stability: f32,
    pub sociability: f32,
    pub risk_tolerance: f32,
    pub manipulation: f32,
    pub hope: f32,
    pub ambition: f32,
    pub leadership: f32,
}

/// Neutral disposition: every trait at the midpoint, therefore no bias.
impl Default for Disposition {
    fn default() -> Self {
        Self {
            discipline: 0.5,
            impulsiveness: 0.5,
            aggression: 0.5,
            emotional_stability: 0.5,
            sociability: 0.5,
            risk_tolerance: 0.5,
            manipulation: 0.5,
            hope: 0.5,
            ambition: 0.5,
            leadership: 0.5,
        }
    }
}

impl From<&Traits> for Disposition {
    fn from(t: &Traits) -> Self {
        Self {
            discipline: t.discipline,
            impulsiveness: t.impulsiveness,
            aggression: t.aggression,
            emotional_stability: t.emotional_stability,
            sociability: t.sociability,
            risk_tolerance: t.risk_tolerance,
            manipulation: t.manipulation,
            hope: 0.5,
            ambition: t.ambition,
            leadership: t.leadership,
        }
    }
}

impl From<&Prisoner> for Disposition {
    fn from(p: &Prisoner) -> Self {
        let mut d = Self::from(&p.traits);
        d.hope = p.condition.hope;
        d
    }
}

/// Bounded additive utility adjustment for one candidate decision.
///
/// The adjustment is centred on 0.5 for every trait, so a neutral personality
/// never changes ranking. Values are kept within `±0.5` to avoid overwhelming
/// need-derived utilities (which the engine supplies in `0..1.5`).
pub fn trait_bias(disposition: &Disposition, decision: &Decision) -> f32 {
    // Non-finite trait values are treated as unknown (neutral), so a bad save
    // or model output can never inject NaN into decision utilities.
    let c = |v: f32| {
        if v.is_finite() {
            v.clamp(0.0, 1.0) - 0.5
        } else {
            0.0
        }
    };
    let discipline = c(disposition.discipline);
    let impulsive = c(disposition.impulsiveness);
    let aggression = c(disposition.aggression);
    let stability = c(disposition.emotional_stability);
    let social = c(disposition.sociability);
    let risk = c(disposition.risk_tolerance);
    let manipulation = c(disposition.manipulation);
    let hopeless = -c(disposition.hope);
    let ambition = c(disposition.ambition);
    let leadership = c(disposition.leadership);

    let bias = match &decision.action {
        // Discipline keeps routines; impulsiveness erodes them.
        DecisionAction::Comply => 0.20 * discipline + 0.05 * stability,
        DecisionAction::Refuse => -0.30 * discipline + 0.10 * impulsive,
        // Aggression/impulse push toward violence; stability restrains it.
        DecisionAction::Fight { .. } => 0.25 * aggression + 0.15 * impulsive - 0.15 * stability,
        DecisionAction::Socialize { .. } => 0.25 * social,
        DecisionAction::Trade { .. } => 0.15 * social + 0.10 * manipulation,
        DecisionAction::SpreadRumor { .. } => 0.25 * manipulation,
        // Desperation and risk tolerance make escape thinkable.
        DecisionAction::EscapeAttempt => 0.25 * risk + 0.15 * hopeless,
        DecisionAction::JoinGang { .. } | DecisionAction::LeaveGang => {
            0.20 * risk + 0.10 * social + 0.10 * hopeless
        }
        DecisionAction::LeadGang { .. } => 0.20 * leadership + 0.15 * ambition,
        // Movement/medical attention are need-driven, not personality-driven.
        DecisionAction::MoveTo { .. } | DecisionAction::RequestMedical | DecisionAction::None => {
            0.0
        }
    };
    bias.clamp(-0.5, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_disposition_has_no_effect() {
        let neutral = Disposition::default();
        for action in [
            Decision::new(DecisionAction::Comply),
            Decision::new(DecisionAction::Refuse),
            Decision::new(DecisionAction::Fight { target: 3 }),
            Decision::new(DecisionAction::Socialize { with: 3 }),
            Decision::new(DecisionAction::EscapeAttempt),
            Decision::new(DecisionAction::RequestMedical),
        ] {
            assert_eq!(trait_bias(&neutral, &action), 0.0, "{action:?}");
        }
    }

    #[test]
    fn disposition_is_derived_from_prisoner_state() {
        use alc_core::{PrisonerId, RngStream};
        use alc_prisoner::Prisoner;
        let mut rng = RngStream::new(12, 3);
        let mut p = Prisoner::generate(PrisonerId(3), &mut rng);
        p.traits.discipline = 0.9;
        p.condition.hope = 0.1;
        let d = Disposition::from(&p);
        assert_eq!(d.discipline, 0.9);
        assert_eq!(d.hope, 0.1);
        assert!(trait_bias(&d, &Decision::new(DecisionAction::Comply)) > 0.0);
        assert!(trait_bias(&d, &Decision::new(DecisionAction::Refuse)) < 0.0);
        // Desperation raises escape appeal *relative to* a hopeful prisoner;
        // the absolute score also depends on risk tolerance.
        let desperate = trait_bias(&d, &Decision::new(DecisionAction::EscapeAttempt));
        let mut hopeful = d;
        hopeful.hope = 0.9;
        let calm = trait_bias(&hopeful, &Decision::new(DecisionAction::EscapeAttempt));
        assert!(
            desperate > calm,
            "desperation should push escape: {desperate} vs {calm}"
        );
    }

    #[test]
    fn biases_stay_bounded_for_extreme_traits() {
        let extreme = Disposition {
            discipline: f32::NAN,
            impulsiveness: f32::INFINITY,
            ..Default::default()
        };
        // NaN clamps to 0.0 and +inf clamps to 1.0; no panic, no NaN leak.
        for action in [
            Decision::new(DecisionAction::Comply),
            Decision::new(DecisionAction::Refuse),
            Decision::new(DecisionAction::Fight { target: 1 }),
        ] {
            let b = trait_bias(&extreme, &action);
            assert!(
                b.is_finite() && (-0.5..=0.5).contains(&b),
                "{action:?}: {b}"
            );
        }
    }
}
