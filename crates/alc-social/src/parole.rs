//! Parole board review and decisions (Directive §15).

use alc_core::SimTime;
use serde::{Deserialize, Serialize};

/// Conditions that may be attached to a parole grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParoleCondition {
    RegularSupervision,
    Curfew,
    EmploymentRequirement,
    TreatmentProgram,
    NoContactOrder,
    MandatoryReporting,
}

/// Outcome of a parole board review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParoleDecision {
    Grant { conditions: Vec<ParoleCondition> },
    Deny { reason: String },
    Defer { reason: String, review_days: u32 },
}

/// Minimal prisoner information for a parole review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParoleSubject {
    pub sentence_days: u32,
    pub sentence_served_days: u32,
    pub disciplinary_status: bool,
    pub aggression: f32,
}

impl ParoleSubject {
    pub fn served_fraction(&self) -> f32 {
        // A zero-length sentence has no meaningful served fraction. Returning
        // 0.0 (instead of `served / sentence.max(1)`, which is 0/1 == 0.0 only
        // while served is zero) keeps time-served cases off the parole track.
        if self.sentence_days == 0 {
            return 0.0;
        }
        self.sentence_served_days as f32 / self.sentence_days as f32
    }

    pub fn is_eligible(&self) -> bool {
        // Explicit sentence-length guard: zero-length sentences are released as
        // sentence-complete / time-served, never paroled.
        self.sentence_days > 0 && self.served_fraction() >= 0.6 && !self.disciplinary_status
    }
}

/// Outcome of post-release reintegration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostReleaseOutcome {
    /// Successfully reintegrated into society.
    SuccessfulReintegration,
    /// Re-offended and returned to custody.
    Recidivism,
    /// Absconded / failed to comply with conditions.
    Absconded,
    /// Transferred to another facility.
    Transferred,
    /// Died shortly after release.
    DeathAfterRelease,
}

/// Post-release reintegration record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostReleaseRecord {
    /// Prisoner who was released.
    pub prisoner_id: u64,
    /// Release type (parole, escape, transfer, etc.).
    pub release_type: String,
    /// Date of release.
    pub at: SimTime,
    /// Recidivism risk score (0..1, higher = riskier).
    pub recidivism_risk: f32,
    /// Actual outcome observed after release.
    pub outcome: PostReleaseOutcome,
    /// Whether the prisoner had a reintegration plan.
    pub had_reintegration_plan: bool,
    /// Months until next review or outcome.
    pub review_months: u32,
}

/// Compute a deterministic recidivism risk score for a parolee.
///
/// Uses prisoner attributes and sentence characteristics to produce a
/// reproducible 0..1 score. This is a stub/simple model; real implementations
/// would use risk assessment matrices and psychological evaluation.
pub fn recidivism_score(
    subject: &ParoleSubject,
    behavior: f32,
    rehab: f32,
    months_in_custody: u32,
) -> f32 {
    let behavior = behavior.clamp(0.0, 1.0);
    let rehab = rehab.clamp(0.0, 1.0);
    let served = subject.served_fraction();

    // Base: longer custody + more time served can indicate entrenched patterns
    let base_risk = 0.2 + (1.0 - served) * 0.2 + (1.0 - behavior) * 0.25;

    // Rehabilitation mitigates risk
    let rehab_mitigation = rehab * 0.3;

    // Behavior mitigates risk
    let behavior_mitigation = behavior * 0.35;

    // Aggression from the prisoner record adds risk
    let aggression_enhancement = subject.aggression * 0.15;

    // Time in custody reduces risk of immediate reoffending
    let custody_stabilization = (months_in_custody as f32 / 12.0).min(1.0) * 0.1;

    let risk = base_risk - rehab_mitigation - behavior_mitigation + aggression_enhancement
        - custody_stabilization;
    risk.clamp(0.0, 1.0)
}

/// Parole board decision logic.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParoleBoard;

impl ParoleBoard {
    /// Review a prisoner for parole.
    ///
    /// - `behavior`: observed institutional behavior quality (0..1, higher is better)
    /// - `rehab`: rehabilitation/program progress and engagement (0..1)
    /// - `risk`: assessed risk to public/institution on release (0..1, higher is riskier)
    pub fn review(
        prisoner: &ParoleSubject,
        behavior: f32,
        rehab: f32,
        risk: f32,
    ) -> ParoleDecision {
        let behavior = behavior.clamp(0.0, 1.0);
        let rehab = rehab.clamp(0.0, 1.0);
        let risk = risk.clamp(0.0, 1.0);

        // A zero-length sentence has no parole phase. Reject explicitly rather
        // than deferring for a review that could never make it eligible.
        if prisoner.sentence_days == 0 {
            return ParoleDecision::Deny {
                reason: "zero-length sentence is not parole-eligible".into(),
            };
        }

        let served_frac = prisoner.served_fraction();
        let base_eligible = prisoner.is_eligible();

        // Ineligibility / insufficient progress -> defer or deny.
        if !base_eligible || served_frac < 0.5 {
            let review_days = if served_frac < 0.3 { 180 } else { 90 };
            return ParoleDecision::Defer {
                reason: "insufficient sentence progress or ineligible status".into(),
                review_days,
            };
        }

        if prisoner.disciplinary_status {
            return ParoleDecision::Deny {
                reason: "active disciplinary status".into(),
            };
        }

        let support = behavior * 0.4 + rehab * 0.35 + (1.0 - risk) * 0.25;
        let aggravators =
            (risk * 0.5 + (1.0 - behavior) * 0.3 + (1.0 - rehab) * 0.2).clamp(0.0, 1.0);

        if support >= 0.68 && risk <= 0.45 {
            let mut conditions = vec![
                ParoleCondition::RegularSupervision,
                ParoleCondition::MandatoryReporting,
            ];
            if rehab < 0.7 {
                conditions.push(ParoleCondition::TreatmentProgram);
            }
            if risk > 0.25 || behavior < 0.8 {
                conditions.push(ParoleCondition::Curfew);
            }
            if served_frac < 0.85 {
                conditions.push(ParoleCondition::EmploymentRequirement);
            }
            if prisoner.aggression > 0.6 || risk > 0.35 {
                conditions.push(ParoleCondition::NoContactOrder);
            }
            ParoleDecision::Grant { conditions }
        } else if support >= 0.5 && aggravators < 0.65 {
            let review_days = if risk > 0.55 || rehab < 0.45 { 120 } else { 60 };
            let reason = if rehab < 0.45 {
                "additional rehabilitation progress required".into()
            } else if risk > 0.55 {
                "risk remains elevated; reassessment required".into()
            } else {
                "further observation period recommended".into()
            };
            ParoleDecision::Defer {
                reason,
                review_days,
            }
        } else {
            let reason = if risk > 0.7 {
                "risk assessment too high for release".into()
            } else if behavior < 0.4 {
                "insufficient institutional behavior record".into()
            } else {
                "parole denied based on overall review".into()
            };
            ParoleDecision::Deny { reason }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_length_sentence_is_not_parole_eligible() {
        let p = ParoleSubject {
            sentence_days: 0,
            sentence_served_days: 0,
            disciplinary_status: false,
            aggression: 0.1,
        };
        assert_eq!(p.served_fraction(), 0.0);
        assert!(!p.is_eligible());
        // Even with a (nonsensical) positive served count, no parole track.
        let bogus = ParoleSubject {
            sentence_served_days: 500,
            ..p.clone()
        };
        assert_eq!(bogus.served_fraction(), 0.0);
        assert!(!bogus.is_eligible());

        // The board denies rather than defers — waiting cannot create eligibility.
        assert!(matches!(
            ParoleBoard::review(&p, 1.0, 1.0, 0.0),
            ParoleDecision::Deny { .. }
        ));
    }

    #[test]
    fn parole_board_can_grant_deny_and_defer() {
        let p = ParoleSubject {
            sentence_days: 1000,
            sentence_served_days: 700,
            disciplinary_status: false,
            aggression: 0.3,
        };

        let grant = ParoleBoard::review(&p, 0.9, 0.85, 0.2);
        assert!(matches!(grant, ParoleDecision::Grant { .. }));

        let deny = ParoleBoard::review(&p, 0.2, 0.2, 0.9);
        assert!(matches!(deny, ParoleDecision::Deny { .. }));

        let mut early = p.clone();
        early.sentence_served_days = 300;
        let defer = ParoleBoard::review(&early, 0.9, 0.9, 0.1);
        assert!(matches!(defer, ParoleDecision::Defer { .. }));
    }
}
