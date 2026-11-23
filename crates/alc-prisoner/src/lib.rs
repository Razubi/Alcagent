//! Prisoner entities, needs, schedules, intake per Directive §3, §4, §5, §13.
//!
//! The crate is split into three focused subsystems, all deterministic and
//! serde-serializable so a world save round-trips exactly (Directive §23):
//!
//! - [`needs`] — need drift, relief and the needs→behavior effects (§4).
//! - [`schedule`] — configurable daily templates, resolution and attendance (§5).
//! - [`intake`] — correlated intake generation and arrival social hooks (§13).
//!
//! This crate never calls an LLM and never mutates another subsystem's state;
//! it publishes plain data that the simulation engine validates and applies.

pub mod intake;
pub mod needs;
pub mod schedule;

pub use intake::*;
pub use needs::*;
pub use schedule::*;

use alc_core::{PrisonerId, RngStream};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Security classification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
pub enum Classification {
    #[default]
    Minimum,
    Low,
    Medium,
    High,
    Maximum,
}

/// All personality + cognitive traits (Directive §3). Each 0..1 except intelligence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Traits {
    pub intelligence: f32,
    pub emotional_stability: f32,
    pub impulsiveness: f32,
    pub aggression: f32,
    pub sociability: f32,
    pub loyalty: f32,
    pub manipulation: f32,
    pub risk_tolerance: f32,
    pub discipline: f32,
    pub empathy: f32,
    pub ambition: f32,
    pub leadership: f32,
}

impl Default for Traits {
    fn default() -> Self {
        Self {
            intelligence: 0.5,
            emotional_stability: 0.5,
            impulsiveness: 0.5,
            aggression: 0.5,
            sociability: 0.5,
            loyalty: 0.5,
            manipulation: 0.5,
            risk_tolerance: 0.5,
            discipline: 0.5,
            empathy: 0.5,
            ambition: 0.5,
            leadership: 0.5,
        }
    }
}

impl Traits {
    pub fn randomized(rng: &mut RngStream) -> Self {
        let r = |rng: &mut RngStream| rng.range(0.1, 0.95) as f32;
        Self {
            intelligence: r(rng),
            emotional_stability: r(rng),
            impulsiveness: r(rng),
            aggression: r(rng),
            sociability: r(rng),
            loyalty: r(rng),
            manipulation: r(rng),
            risk_tolerance: r(rng),
            discipline: r(rng),
            empathy: r(rng),
            ambition: r(rng),
            leadership: r(rng),
        }
    }
}

/// Physical + mental condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub health: f32,
    pub injury: f32,
    pub illness: f32,
    pub mental_health: f32,
    /// 0..1 of actual hope (not a deficit). Consumed by needs→behavior effects.
    pub hope: f32,
}

impl Default for Condition {
    fn default() -> Self {
        Self {
            health: 1.0,
            injury: 0.0,
            illness: 0.0,
            mental_health: 0.8,
            hope: 0.5,
        }
    }
}

/// Personal goal (Directive §3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub kind: GoalKind,
    pub progress: f32,
    pub urgency: f32,
    pub description: String,
}

impl Default for Goal {
    fn default() -> Self {
        Self {
            kind: GoalKind::Survive,
            progress: 0.0,
            urgency: 0.5,
            description: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalKind {
    #[default]
    Survive,
    Escape,
    GangLeadership,
    Protection,
    Rehabilitation,
    Parole,
    Revenge,
    Friendship,
    Status,
    Purpose,
}

/// A prisoner aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prisoner {
    pub id: PrisonerId,
    pub name: String,
    pub age: u32,
    pub background: String,
    pub sentence_days: u32,
    pub sentence_served_days: u32,
    pub classification: Classification,
    pub traits: Traits,
    pub condition: Condition,
    pub reputation: f32,
    pub needs: Needs,
    pub gang_affiliation: Option<u64>,
    pub goals: Vec<Goal>,
    pub skills: HashMap<String, f32>,
    pub schedule: ScheduleAssignment,
    pub memory_summary: String,
}

impl Default for Prisoner {
    fn default() -> Self {
        Self {
            id: PrisonerId(0),
            name: "Unnamed".into(),
            age: 18,
            background: String::new(),
            sentence_days: 0,
            sentence_served_days: 0,
            classification: Classification::default(),
            traits: Traits::default(),
            condition: Condition::default(),
            reputation: 0.0,
            needs: Needs::default(),
            gang_affiliation: None,
            goals: Vec::new(),
            skills: HashMap::new(),
            schedule: ScheduleAssignment::default(),
            memory_summary: String::new(),
        }
    }
}

/// Daily schedule assignment (Directive §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleAssignment {
    pub wing: u32,
    pub work_assignment: Option<String>,
    pub education: bool,
    pub privileges: f32, // 0..1
    pub disciplinary_status: bool,
}

impl Default for ScheduleAssignment {
    fn default() -> Self {
        Self {
            wing: 1,
            work_assignment: None,
            education: false,
            privileges: 0.5,
            disciplinary_status: false,
        }
    }
}

impl Prisoner {
    pub fn generate(id: PrisonerId, rng: &mut RngStream) -> Self {
        let first = ["Marcus", "Dmitri", "Andre", "Tyrell", "Viktor", "Hector", "Samuel", "Kwame", "Liam", "Jose"];
        let last = ["Volkov", "Mercer", "Okafor", "Reyes", "Kowalski", "Dubois", "Haddad", "Fitzgerald", "Osei", "Marsh"];
        let name = format!("{} {}", first[(rng.next_u64() % 10) as usize], last[(rng.next_u64() % 10) as usize]);
        let sentence_days = [30, 90, 180, 365, 730, 1460][(rng.next_u64() % 6) as usize];
        let classification = match sentence_days {
            30..=90 => Classification::Minimum,
            91..=365 => Classification::Low,
            366..=730 => Classification::Medium,
            731..=1095 => Classification::High,
            _ => Classification::Maximum,
        };
        let mut goals = vec![Goal {
            kind: GoalKind::Survive,
            progress: 0.5,
            urgency: 0.6,
            description: "Stay alive and out of solitary".into(),
        }];
        if rng.chance(0.3) {
            goals.push(Goal { kind: GoalKind::Escape, progress: 0.0, urgency: 0.8, description: "Find a way out".into() });
        }
        if rng.chance(0.4) {
            goals.push(Goal { kind: GoalKind::GangLeadership, progress: 0.1, urgency: 0.5, description: "Rise to the top".into() });
        }
        Self {
            id,
            name,
            age: rng.range(18.0, 55.0) as u32,
            background: format!("Sentence {sentence_days}d, classification {classification:?}"),
            sentence_days,
            sentence_served_days: 0,
            classification,
            traits: Traits::randomized(rng),
            condition: Condition {
                health: 1.0,
                injury: 0.0,
                illness: 0.0,
                mental_health: rng.range(0.5, 0.95) as f32,
                hope: rng.range(0.3, 0.8) as f32,
            },
            reputation: rng.range(0.0, 0.3) as f32,
            needs: Needs::new(),
            gang_affiliation: None,
            goals,
            skills: HashMap::new(),
            schedule: ScheduleAssignment {
                wing: 1,
                work_assignment: Some("laundry".into()),
                education: rng.chance(0.3),
                privileges: 0.5,
                disciplinary_status: false,
            },
            memory_summary: String::new(),
        }
    }

    pub fn sentence_remaining_days(&self) -> u32 {
        self.sentence_days.saturating_sub(self.sentence_served_days)
    }

    pub fn is_eligible_for_parole(&self) -> bool {
        let frac = self.sentence_served_days as f32 / self.sentence_days.max(1) as f32;
        frac >= 0.6 && !self.schedule.disciplinary_status
    }

    /// Attribute→behavior modifiers (Directive §3 examples).
    ///
    /// Hunger now feeds irritability through `Needs::irritability` (§4), so a
    /// hungry prisoner is measurably more aggressive than a fed one.
    pub fn aggression_modifier(&self) -> f32 {
        self.traits.aggression * 0.5
            + self.traits.impulsiveness * 0.3
            + self.needs.irritability() * 0.2
            + (1.0 - self.condition.mental_health) * 0.2
            - self.traits.emotional_stability * 0.3
    }

    pub fn social_drive(&self) -> f32 {
        self.traits.sociability * 0.5 + self.needs.companionship * 0.3 + self.needs.belonging * 0.2
    }

    pub fn escape_motivation(&self) -> f32 {
        self.needs.freedom * 0.4
            + self.needs.despair_pressure(self.condition.hope) * 0.2
            + (self.sentence_remaining_days() as f32 / 3650.0).min(1.0) * 0.2
            + self.traits.risk_tolerance * 0.2
    }

    // -- Derived behavior wrappers (Directive §4 worked examples) -----------

    /// Low alertness from fatigue/sleep loss — consumed by attendance and guards.
    pub fn alertness(&self) -> f32 {
        self.needs.alertness()
    }

    /// Pressure to seek a gang or patron from unmet safety/protection/belonging.
    pub fn gang_seeking_pressure(&self) -> f32 {
        self.needs.gang_seeking_pressure()
    }

    /// Boredom + low discipline → rule-breaking urge.
    pub fn rule_breaking_urge(&self) -> f32 {
        self.needs.rule_breaking_urge(self.traits.discipline)
    }

    /// Rehab participation probability: low hope/desperation suppresses it.
    pub fn rehab_motivation(&self) -> f32 {
        self.needs.rehab_motivation(self.condition.hope)
    }

    /// Auditable bundle of needs→behavior effects for the AI/decision layer.
    pub fn needs_effects(&self) -> Vec<NeedsEffect> {
        self.needs.effects(&self.traits, self.condition.hope)
    }
}

/// Signed per-dyad relationship delta applied by `alc-social`'s
/// `Relationship::adjust` (Directive A6/A8 semantics): each field moves the
/// corresponding relationship axis, with negatives weakening it. Owned by this
/// crate so `alc-social` (which depends on `alc-prisoner`) can re-export it
/// without creating a dependency cycle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct RelationshipDelta {
    pub trust: f32,
    pub friendship: f32,
    pub fear: f32,
    pub respect: f32,
    pub loyalty: f32,
    pub hostility: f32,
    pub attraction: f32,
    pub resentment: f32,
    pub dependency: f32,
    pub rivalry: f32,
    pub debt: f32,
    pub gratitude: f32,
    pub familiarity: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_api_survives_refactor() {
        let mut rng = RngStream::new(1, 1);
        let mut p = Prisoner::generate(PrisonerId(1), &mut rng);
        p.needs.drift(10);
        assert!(p.aggression_modifier().is_finite());
        assert!(p.escape_motivation().is_finite());
        assert!(p.sentence_remaining_days() <= p.sentence_days);
        assert!(!p.is_eligible_for_parole());
    }

    #[test]
    fn hunger_feeds_aggression_and_hope_feeds_rehab() {
        let mut rng = RngStream::new(2, 2);
        let mut p = Prisoner::generate(PrisonerId(2), &mut rng);
        let calm = p.aggression_modifier();
        p.needs.hunger = 1.0;
        assert!(p.aggression_modifier() > calm);

        p.condition.hope = 0.9;
        let hopeful = p.rehab_motivation();
        p.condition.hope = 0.1;
        assert!(p.rehab_motivation() < hopeful);
    }

    #[test]
    fn persistence_round_trip() {
        let mut rng = RngStream::new(3, 3);
        let p = Prisoner::generate(PrisonerId(3), &mut rng);
        let json = serde_json::to_string(&p).unwrap();
        let back: Prisoner = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, p.id);
        assert_eq!(back.name, p.name);
        assert_eq!(back.needs.hunger, p.needs.hunger);
    }
}
