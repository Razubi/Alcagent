//! Rehabilitation and program participation effects (Directive §14).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Rehabilitation programs available to prisoners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RehabProgram {
    Education,
    Vocational,
    Counseling,
    AddictionRecovery,
    AngerManagement,
}

impl RehabProgram {
    pub fn key(self) -> &'static str {
        match self {
            RehabProgram::Education => "rehab_education",
            RehabProgram::Vocational => "rehab_vocational",
            RehabProgram::Counseling => "rehab_counseling",
            RehabProgram::AddictionRecovery => "rehab_addiction_recovery",
            RehabProgram::AngerManagement => "rehab_anger_management",
        }
    }

    pub fn difficulty(self) -> f32 {
        match self {
            RehabProgram::Education => 0.55,
            RehabProgram::Vocational => 0.5,
            RehabProgram::Counseling => 0.45,
            RehabProgram::AddictionRecovery => 0.7,
            RehabProgram::AngerManagement => 0.6,
        }
    }
}

/// Outcome of applying rehabilitation progress over a time step.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RehabOutcome {
    /// Progress increased by the given delta (0..1 scale).
    Progress(f32),
    /// Program completed this step.
    Completed,
    /// Prisoner dropped or was removed from the program.
    Dropped,
    /// Prisoner is not enrolled in this program.
    NoEnrollment,
}

/// Mutable prisoner state needed for rehabilitation simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RehabPrisonerState {
    pub disciplinary_status: bool,
    pub education: bool,
    pub skills: HashMap<String, f32>,
    pub hope: f32,
    pub mental_health: f32,
    pub health: f32,
    pub aggression: f32,
    pub emotional_stability: f32,
    pub impulsiveness: f32,
    pub discipline: f32,
    pub intelligence: f32,
    pub reputation: f32,
    pub fatigue: f32,
    /// 0..1 need deficits (higher = worse pressure).
    pub purpose_need: f32,
    pub stability_need: f32,
    pub respect_need: f32,
    /// Derived motivation/despair inputs.
    pub rehab_motivation: f32,
    pub desperation: f32,
}

impl Default for RehabPrisonerState {
    fn default() -> Self {
        Self {
            disciplinary_status: false,
            education: false,
            skills: HashMap::new(),
            hope: 0.5,
            mental_health: 0.7,
            health: 0.8,
            aggression: 0.5,
            emotional_stability: 0.5,
            impulsiveness: 0.5,
            discipline: 0.5,
            intelligence: 0.5,
            reputation: 0.2,
            fatigue: 0.2,
            purpose_need: 0.4,
            stability_need: 0.4,
            respect_need: 0.4,
            rehab_motivation: 0.5,
            desperation: 0.3,
        }
    }
}

/// Attempt to enroll a prisoner in a rehabilitation program.
///
/// Enrollment is willingness/eligibility-based and recorded in skills to avoid
/// requiring schema changes elsewhere.
pub fn enroll_in_program(prisoner: &mut RehabPrisonerState, program: RehabProgram) -> bool {
    if prisoner.disciplinary_status {
        return false;
    }
    let key = program.key().to_string();
    if prisoner.skills.contains_key(&key) {
        return true;
    }

    let willingness = (prisoner.rehab_motivation * 0.5
        + prisoner.discipline * 0.2
        + prisoner.hope * 0.15
        + (1.0 - prisoner.desperation) * 0.15)
        .clamp(0.0, 1.0);
    let threshold = 0.35 + program.difficulty() * 0.2;
    if willingness < threshold {
        return false;
    }

    prisoner.skills.insert(key, 0.0);
    if matches!(program, RehabProgram::Education | RehabProgram::Vocational) {
        prisoner.education = true;
    }
    // Enrollment can provide a small immediate purpose/hope lift.
    prisoner.purpose_need = (prisoner.purpose_need - 0.05).max(0.0);
    prisoner.hope = (prisoner.hope + 0.02).clamp(0.0, 1.0);
    true
}

/// Apply rehabilitation progress for a prisoner over `dt` minutes.
pub fn apply_program_progress(
    prisoner: &mut RehabPrisonerState,
    program: RehabProgram,
    dt: u64,
) -> RehabOutcome {
    let key = program.key().to_string();
    if !prisoner.skills.contains_key(&key) {
        return RehabOutcome::NoEnrollment;
    }

    if prisoner.disciplinary_status && prisoner.discipline < 0.4 {
        prisoner.skills.remove(&key);
        return RehabOutcome::Dropped;
    }

    let hours = dt as f32 / 60.0;
    if hours <= 0.0 {
        return RehabOutcome::Progress(0.0);
    }

    let engagement = (prisoner.rehab_motivation * 0.45
        + prisoner.discipline * 0.2
        + prisoner.intelligence * 0.15
        + prisoner.mental_health * 0.1
        + (1.0 - prisoner.fatigue) * 0.1)
        .clamp(0.0, 1.0);
    let dropout_risk =
        ((1.0 - engagement) * 0.2 + prisoner.desperation * 0.15).clamp(0.0, 1.0);
    if dropout_risk > 0.85 && engagement < 0.2 {
        prisoner.skills.remove(&key);
        return RehabOutcome::Dropped;
    }

    let rate = 0.01 + engagement * 0.03;
    let delta = (rate * hours * (1.0 - program.difficulty() * 0.4)).clamp(0.0, 0.2);

    let new_progress = {
        let progress = prisoner
            .skills
            .get_mut(&key)
            .expect("program enrollment checked above");
        *progress = (*progress + delta).min(1.0);
        *progress
    };

    // Program-specific effects.
    match program {
        RehabProgram::Education => {
            let e = prisoner.skills.entry("education".into()).or_insert(0.0);
            *e = (*e + delta * 0.5).min(1.0);
            prisoner.hope = (prisoner.hope + delta * 0.1).min(1.0);
            prisoner.purpose_need = (prisoner.purpose_need - delta * 0.2).max(0.0);
        }
        RehabProgram::Vocational => {
            let e = prisoner.skills.entry("vocational".into()).or_insert(0.0);
            *e = (*e + delta * 0.6).min(1.0);
            prisoner.hope = (prisoner.hope + delta * 0.08).min(1.0);
            prisoner.purpose_need = (prisoner.purpose_need - delta * 0.15).max(0.0);
        }
        RehabProgram::Counseling => {
            prisoner.mental_health = (prisoner.mental_health + delta * 0.15).min(1.0);
            prisoner.hope = (prisoner.hope + delta * 0.12).min(1.0);
            prisoner.stability_need = (prisoner.stability_need - delta * 0.2).max(0.0);
        }
        RehabProgram::AddictionRecovery => {
            prisoner.health = (prisoner.health + delta * 0.08).min(1.0);
            prisoner.hope = (prisoner.hope + delta * 0.1).min(1.0);
            prisoner.impulsiveness = (prisoner.impulsiveness - delta * 0.05).max(0.0);
        }
        RehabProgram::AngerManagement => {
            prisoner.aggression = (prisoner.aggression - delta * 0.08).max(0.0);
            prisoner.emotional_stability =
                (prisoner.emotional_stability + delta * 0.06).min(1.0);
            prisoner.respect_need = (prisoner.respect_need - delta * 0.1).max(0.0);
        }
    }

    if new_progress >= 1.0 {
        prisoner.reputation = (prisoner.reputation + 0.05).min(1.0);
        prisoner.skills.remove(&key);
        return RehabOutcome::Completed;
    }

    RehabOutcome::Progress(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enroll_and_progress_rehab_program() {
        let mut p = RehabPrisonerState {
            disciplinary_status: false,
            hope: 0.9,
            discipline: 0.8,
            aggression: 0.9,
            rehab_motivation: 0.85,
            desperation: 0.1,
            purpose_need: 0.8,
            ..Default::default()
        };

        let enrolled = enroll_in_program(&mut p, RehabProgram::AngerManagement);
        assert!(enrolled);

        let mut saw_progress = false;
        let mut completed = false;
        for _ in 0..400 {
            match apply_program_progress(&mut p, RehabProgram::AngerManagement, 60) {
                RehabOutcome::Progress(d) if d > 0.0 => saw_progress = true,
                RehabOutcome::Completed => {
                    completed = true;
                    break;
                }
                RehabOutcome::NoEnrollment => break,
                _ => {}
            }
        }
        assert!(saw_progress || completed);
        assert!(p.aggression < 0.9 || completed);
    }
}
