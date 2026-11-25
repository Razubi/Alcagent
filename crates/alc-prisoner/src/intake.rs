//! Intake & population growth per Directive §13 (design doc §5).
//!
//! [`generate_intake`] builds a *correlated* profile: the criminal background
//! drives sentence length, classification, traits, skills, health, goals and
//! initial needs, so arrivals feel like people rather than dice rolls.
//! [`arrival_social_hooks`] turns a cohort of newcomers plus the current
//! population into concrete seeds for the Social/Gang/Economy systems — it
//! suggests, never forces, relationships and group membership (Directive §2).

use crate::{Classification, Condition, Goal, GoalKind, Needs, Prisoner};
use alc_core::{PrisonerId, RngStream};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Broad background template. Each variant encodes the §2.3 correlations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackgroundKind {
    Violent,
    Property,
    Drug,
    Fraud,
    Organized,
    PublicOrder,
}

impl BackgroundKind {
    pub fn tag(self) -> &'static str {
        match self {
            BackgroundKind::Violent => "violent",
            BackgroundKind::Property => "property",
            BackgroundKind::Drug => "drug",
            BackgroundKind::Fraud => "fraud",
            BackgroundKind::Organized => "organized",
            BackgroundKind::PublicOrder => "public-order",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "violent" => Some(Self::Violent),
            "property" => Some(Self::Property),
            "drug" => Some(Self::Drug),
            "fraud" => Some(Self::Fraud),
            "organized" => Some(Self::Organized),
            "public-order" => Some(Self::PublicOrder),
            _ => None,
        }
    }

    /// Recover a background from persisted text or infer it from observable
    /// attributes. Handlers for legacy `Prisoner::generate` profiles take the
    /// inference path, so arrival hooks work on any population.
    pub fn infer(p: &Prisoner) -> Self {
        if let Some(tag) = p.background.split('|').next().and_then(Self::from_tag) {
            return tag;
        }
        if p.traits.aggression > 0.62 {
            Self::Violent
        } else if p.traits.intelligence > 0.65 && p.traits.manipulation > 0.55 {
            Self::Fraud
        } else if p.needs.medical_need > 0.4 {
            Self::Drug
        } else if p.sentence_days <= 120 {
            Self::Property
        } else {
            Self::PublicOrder
        }
    }

    fn sentence_days(self, rng: &mut RngStream) -> u32 {
        let (lo, hi) = match self {
            BackgroundKind::Violent => (365, 2555),
            BackgroundKind::Organized => (730, 3650),
            BackgroundKind::Drug => (180, 1460),
            BackgroundKind::Fraud => (90, 1095),
            BackgroundKind::Property => (30, 365),
            BackgroundKind::PublicOrder => (30, 180),
        };
        rng.range(lo as f64, hi as f64) as u32
    }

    fn classification(self, days: u32) -> Classification {
        if self == BackgroundKind::Organized || days > 1825 {
            Classification::Maximum
        } else if days > 1095 || self == BackgroundKind::Violent {
            Classification::High
        } else if days > 365 {
            Classification::Medium
        } else if days > 90 {
            Classification::Low
        } else {
            Classification::Minimum
        }
    }
}

fn pick_background(rng: &mut RngStream) -> BackgroundKind {
    // Weights sum to 1.0; ordered so common offenses dominate.
    let r = rng.next_f64();
    if r < 0.20 {
        BackgroundKind::Violent
    } else if r < 0.38 {
        BackgroundKind::Property
    } else if r < 0.56 {
        BackgroundKind::Drug
    } else if r < 0.70 {
        BackgroundKind::Fraud
    } else if r < 0.85 {
        BackgroundKind::Organized
    } else {
        BackgroundKind::PublicOrder
    }
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Generate one complete, correlated intake profile from a world seed and the
/// prisoner's id. Deterministic: the same `(seed, id)` always yields the same
/// prisoner, and adding other prisoners never reshuffles this one.
pub fn generate_intake(seed: u64, id: PrisonerId) -> Prisoner {
    let mut rng = RngStream::new(seed, id.raw()).fork(0x14_1A_7E);
    generate_intake_with(&mut rng, id)
}

/// Generate a cohort of consecutive ids with independent derived streams.
///
/// IDs are handed out with `checked_add` so a cohort starting near `u64::MAX`
/// stops cleanly instead of wrapping (panic in debug, duplicate IDs in release).
/// Every generated prisoner therefore has a distinct id.
pub fn generate_intake_cohort(seed: u64, first_id: u64, count: u32) -> Vec<Prisoner> {
    let mut cohort = Vec::with_capacity(count as usize);
    let mut next = Some(first_id);
    for _ in 0..count {
        let Some(id) = next else {
            break;
        };
        next = id.checked_add(1);
        cohort.push(generate_intake(seed, PrisonerId(id)));
    }
    cohort
}

/// Internal generator that takes an explicit stream (used by the batch helper
/// and by tests that want to control the draw sequence).
pub fn generate_intake_with(rng: &mut RngStream, id: PrisonerId) -> Prisoner {
    let mut p = Prisoner::generate(id, rng);
    let kind = pick_background(rng);

    // 1. Sentence + classification follow the offense (§2.3, §2.5).
    let days = kind.sentence_days(rng);
    let class = kind.classification(days);
    p.sentence_days = days;
    p.sentence_served_days = 0;
    p.classification = class;
    p.background = format!("{}|{}", kind.tag(), p.background);

    // 2. Correlated traits: violent → aggressive/less empathetic; fraud →
    //    smart/disciplined; organized → loyal/leaderlike; drug → impulsive.
    {
        let t = &mut p.traits;
        match kind {
            BackgroundKind::Violent => {
                t.aggression = clamp01(t.aggression + 0.25);
                t.empathy = clamp01(t.empathy - 0.25);
                t.risk_tolerance = clamp01(t.risk_tolerance + 0.10);
            }
            BackgroundKind::Organized => {
                t.loyalty = clamp01(t.loyalty + 0.25);
                t.leadership = clamp01(t.leadership + 0.15);
                t.manipulation = clamp01(t.manipulation + 0.10);
            }
            BackgroundKind::Drug => {
                t.impulsiveness = clamp01(t.impulsiveness + 0.20);
                t.emotional_stability = clamp01(t.emotional_stability - 0.15);
                t.discipline = clamp01(t.discipline - 0.15);
            }
            BackgroundKind::Fraud => {
                t.intelligence = clamp01(t.intelligence + 0.20);
                t.manipulation = clamp01(t.manipulation + 0.15);
                t.discipline = clamp01(t.discipline + 0.10);
                t.aggression = clamp01(t.aggression - 0.20);
            }
            BackgroundKind::Property => {
                t.risk_tolerance = clamp01(t.risk_tolerance + 0.10);
                t.ambition = clamp01(t.ambition - 0.05);
            }
            BackgroundKind::PublicOrder => {
                t.impulsiveness = clamp01(t.impulsiveness + 0.10);
            }
        }
    }

    // 3. Health & hope: long sentences and poor backgrounds lower hope;
    //    drug/organized histories carry health/addiction costs (§2.6).
    let hope = clamp01(rng.range(0.25, 0.85) as f32 - (days as f32 / 7300.0));
    p.condition = Condition {
        health: clamp01(rng.range(0.55, 1.0) as f32),
        injury: 0.0,
        illness: if kind == BackgroundKind::Drug {
            clamp01(rng.range(0.1, 0.5) as f32)
        } else {
            0.0
        },
        mental_health: clamp01(
            rng.range(0.45, 0.95) as f32 - (1.0 - p.traits.emotional_stability) * 0.2,
        ),
        hope,
    };

    // 4. Skills follow background and employment history (§2.7).
    let mut skills: HashMap<String, f32> = HashMap::new();
    let boost = |map: &mut HashMap<String, f32>, key: &str, base: f32, rng: &mut RngStream| {
        let v = clamp01(base + rng.range(0.0, 0.4) as f32);
        map.insert(key.to_string(), v);
    };
    boost(&mut skills, "cooking", 0.3, rng);
    boost(&mut skills, "cleaning", 0.3, rng);
    match kind {
        BackgroundKind::Violent | BackgroundKind::Organized => {
            boost(&mut skills, "combat", 0.6, rng);
            boost(&mut skills, "negotiation", 0.4, rng);
        }
        BackgroundKind::Fraud => {
            boost(&mut skills, "accounting", 0.7, rng);
            boost(&mut skills, "computer", 0.6, rng);
        }
        BackgroundKind::Drug => {
            boost(&mut skills, "first-aid", 0.4, rng);
            boost(&mut skills, "stealth", 0.5, rng);
        }
        BackgroundKind::Property => {
            boost(&mut skills, "stealth", 0.6, rng);
            boost(&mut skills, "mechanics", 0.4, rng);
        }
        BackgroundKind::PublicOrder => {
            boost(&mut skills, "labor", 0.5, rng);
        }
    }
    p.skills = skills;

    // 5. Goals follow sentence, hope, classification and aggression (§2.10).
    let mut goals = vec![Goal {
        kind: GoalKind::Survive,
        progress: 0.1,
        urgency: 0.6 + p.needs.safety * 0.2,
        description: "Survive intake and find a safe routine".into(),
    }];
    if hope < 0.4 || p.needs.freedom > 0.5 {
        goals.push(Goal {
            kind: GoalKind::Escape,
            progress: 0.0,
            urgency: 0.5 + (1.0 - hope) * 0.4,
            description: "Find a way out".into(),
        });
    }
    if p.needs.safety > 0.2 || p.traits.sociability > 0.6 {
        goals.push(Goal {
            kind: GoalKind::Protection,
            progress: 0.0,
            urgency: 0.4 + p.needs.safety * 0.3,
            description: "Secure protection inside".into(),
        });
    }
    if p.traits.ambition > 0.6 && class >= Classification::Medium {
        goals.push(Goal {
            kind: GoalKind::Status,
            progress: 0.0,
            urgency: 0.5,
            description: "Build influence and status".into(),
        });
    }
    if hope > 0.5 {
        goals.push(Goal {
            kind: GoalKind::Rehabilitation,
            progress: 0.0,
            urgency: 0.3 + hope * 0.3,
            description: "Make something of the sentence".into(),
        });
    }
    p.goals = goals;

    // 6. Needs baseline: transit, arrest and family separation leave marks
    //    (design doc §5.1 step 9).
    p.needs = Needs::new();
    p.needs.fatigue = clamp01(p.needs.fatigue + rng.range(0.2, 0.5) as f32);
    p.needs.hunger = clamp01(p.needs.hunger + rng.range(0.1, 0.3) as f32);
    p.needs.hygiene = clamp01(p.needs.hygiene + rng.range(0.1, 0.3) as f32);
    p.needs.family_contact = clamp01(p.needs.family_contact + rng.range(0.1, 0.3) as f32);
    if hours_in_transit_long(rng) {
        p.needs.sleep = clamp01(p.needs.sleep + 0.2);
    }
    if class >= Classification::High {
        p.needs.safety = clamp01(p.needs.safety + 0.25);
    }
    p.needs.medical_need = p.condition.illness;

    // 7. Assignment/privileges follow classification and discipline.
    p.schedule.wing = match class {
        Classification::Minimum => 1,
        Classification::Low => 2,
        Classification::Medium => 3,
        Classification::High => 4,
        Classification::Maximum => 5,
    };
    p.schedule.privileges = clamp01(0.8 - (class as u32) as f32 * 0.12);
    p.schedule.education = rng.chance((0.3 + hope * 0.2) as f64);
    p.schedule.disciplinary_status = false;

    p.memory_summary = format!("Intake: {} offender, {}d sentence.", kind.tag(), days);
    p
}

fn hours_in_transit_long(rng: &mut RngStream) -> bool {
    rng.chance(0.35)
}

/// Suggested *additive* relationship change, serializable so saves round-trip.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RelationshipDeltaSeed {
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

impl RelationshipDeltaSeed {
    pub fn friendship_seed(intensity: f32) -> Self {
        let i = intensity.clamp(0.0, 1.0);
        Self {
            trust: 0.15 * i,
            friendship: 0.25 * i,
            familiarity: 0.25 * i,
            ..Self::default()
        }
    }

    pub fn rivalry_seed(intensity: f32) -> Self {
        let i = intensity.clamp(0.0, 1.0);
        Self {
            hostility: 0.35 * i,
            rivalry: 0.40 * i,
            fear: 0.10 * i,
            resentment: 0.15 * i,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationshipSeed {
    pub from: PrisonerId,
    pub to: PrisonerId,
    pub delta: RelationshipDeltaSeed,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RivalrySeed {
    pub newcomer: PrisonerId,
    pub other: PrisonerId,
    pub intensity: f32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GangJoinCandidate {
    pub newcomer: PrisonerId,
    /// `None` means "no compatible group — form a new clique".
    pub gang: Option<u64>,
    pub score: f32,
    pub reason: String,
}

/// All arrival suggestions for one newcomer, consumed by Social/Gang/Economy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArrivalSocialHooks {
    pub newcomer: PrisonerId,
    pub friendship_seeds: Vec<RelationshipSeed>,
    pub rivalry_seeds: Vec<RivalrySeed>,
    pub gang_join_candidates: Vec<GangJoinCandidate>,
    /// 0..1 feeds world instability metrics and warden decisions.
    pub stability_risk: f32,
    /// Additional food/medical/bunk load (Economy hook, §5.3).
    pub resource_demand_delta: f32,
}

impl Default for ArrivalSocialHooks {
    fn default() -> Self {
        Self {
            newcomer: PrisonerId(0),
            friendship_seeds: Vec::new(),
            rivalry_seeds: Vec::new(),
            gang_join_candidates: Vec::new(),
            stability_risk: 0.0,
            resource_demand_delta: 0.0,
        }
    }
}

/// Compute arrival suggestions for a cohort of newcomers against the current
/// population. Pure and deterministic: the Social system decides whether to
/// apply any of these (Directive §2 — agents propose, engine validates).
pub fn arrival_social_hooks(
    profiles: &[Prisoner],
    existing: &[Prisoner],
) -> Vec<ArrivalSocialHooks> {
    profiles.iter().map(|p| hooks_for(p, existing)).collect()
}

fn hooks_for(newcomer: &Prisoner, existing: &[Prisoner]) -> ArrivalSocialHooks {
    let mut out = ArrivalSocialHooks {
        newcomer: newcomer.id,
        ..Default::default()
    };
    let kind = BackgroundKind::infer(newcomer);
    let safety_pressure = newcomer.needs.gang_seeking_pressure();

    for other in existing {
        if other.id == newcomer.id {
            continue;
        }

        // Friendships: shared background / offense type seeds trust (§5.3).
        if BackgroundKind::infer(other) == kind {
            out.friendship_seeds.push(RelationshipSeed {
                from: newcomer.id,
                to: other.id,
                delta: RelationshipDeltaSeed::friendship_seed(
                    0.4 + newcomer.traits.sociability * 0.4,
                ),
                reason: format!("shared {kind:?} background"),
            });
        }

        // Rivalries: opposed gang ties or a leadership challenge (§5.3).
        if let Some(other_gang) = other.gang_affiliation {
            let opposed = newcomer.gang_affiliation.is_some()
                && newcomer.gang_affiliation != Some(other_gang);
            let aggressive = newcomer.traits.aggression > 0.55 && other.traits.aggression > 0.5;
            let challenge = newcomer.traits.leadership > 0.7 && other.traits.leadership > 0.6;
            if opposed || aggressive || challenge {
                let intensity = (if opposed { 0.4 } else { 0.0 })
                    + (if aggressive {
                        newcomer.traits.aggression * 0.4
                    } else {
                        0.0
                    })
                    + (if challenge {
                        newcomer.traits.leadership * 0.4
                    } else {
                        0.0
                    });
                out.rivalry_seeds.push(RivalrySeed {
                    newcomer: newcomer.id,
                    other: other.id,
                    intensity: intensity.min(1.0),
                    reason: if challenge {
                        "leadership challenge".into()
                    } else if opposed {
                        "opposed gang affiliation".into()
                    } else {
                        "mutual aggression".into()
                    },
                });
            }
        }
    }

    // Gang-join candidates inferred from gangs already present inside (§5.3).
    let mut gangs: Vec<u64> = existing.iter().filter_map(|p| p.gang_affiliation).collect();
    gangs.sort_unstable();
    gangs.dedup();
    if newcomer.gang_affiliation.is_none() {
        for gang in gangs {
            let score = (newcomer.traits.sociability * 0.30
                + newcomer.traits.loyalty * 0.25
                + newcomer.needs.belonging * 0.20
                + safety_pressure * 0.25)
                .clamp(0.0, 1.0);
            if score > 0.30 {
                out.gang_join_candidates.push(GangJoinCandidate {
                    newcomer: newcomer.id,
                    gang: Some(gang),
                    score,
                    reason: "sociability/loyalty + unmet safety/belonging".into(),
                });
            }
        }
        if safety_pressure > 0.55 {
            out.gang_join_candidates.push(GangJoinCandidate {
                newcomer: newcomer.id,
                gang: None,
                score: safety_pressure,
                reason: "no compatible group — new clique may form".into(),
            });
        }
    }

    out.stability_risk = (newcomer.traits.aggression * 0.30
        + newcomer.traits.leadership * 0.20
        + newcomer.needs.total_pressure() * 0.30
        + safety_pressure * 0.20)
        .clamp(0.0, 1.0);
    out.resource_demand_delta =
        1.0 + newcomer.needs.medical_need * 0.5 + newcomer.condition.illness * 0.5;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::RngStream;

    #[test]
    fn intake_is_deterministic_and_complete() {
        let a = generate_intake(42, PrisonerId(1));
        let b = generate_intake(42, PrisonerId(1));
        assert_eq!(a.background, b.background);
        assert_eq!(a.sentence_days, b.sentence_days);
        assert_eq!(a.traits.aggression, b.traits.aggression);
        assert!(!a.goals.is_empty());
        assert!(!a.skills.is_empty());
        assert!(a.condition.hope >= 0.0 && a.condition.hope <= 1.0);
    }

    #[test]
    fn intake_cohort_ids_are_unique_and_never_wrap() {
        let cohort = generate_intake_cohort(7, 10, 4);
        let ids: Vec<u64> = cohort.iter().map(|p| p.id.raw()).collect();
        assert_eq!(ids, vec![10, 11, 12, 13]);

        // Starting at the top of the id space must truncate, not wrap or panic.
        let tail = generate_intake_cohort(7, u64::MAX - 1, 5);
        let tail_ids: Vec<u64> = tail.iter().map(|p| p.id.raw()).collect();
        assert_eq!(tail_ids, vec![u64::MAX - 1, u64::MAX]);

        let mut all: Vec<u64> = ids;
        all.extend(tail_ids);
        let mut sorted = all.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len(), "cohort ids must be unique");
    }

    #[test]
    fn violent_background_correlates_with_aggression_and_combat() {
        let mut found = false;
        for id in 1..200 {
            let p = generate_intake(99, PrisonerId(id));
            if BackgroundKind::infer(&p) == BackgroundKind::Violent {
                assert!(p.traits.aggression > 0.25);
                assert!(p.traits.empathy < 0.85);
                assert!(p.skills.contains_key("combat"));
                found = true;
                break;
            }
        }
        assert!(found, "no violent intake generated");
    }

    #[test]
    fn arrival_hooks_seed_friendships_rivalries_and_gang_candidates() {
        let mut rng = RngStream::new(5, 0);
        let mut existing = Vec::new();
        for i in 1..8 {
            let mut p = Prisoner::generate(PrisonerId(i), &mut rng);
            p.background = "violent|existing".into();
            p.gang_affiliation = Some(1);
            p.traits.leadership = 0.8;
            existing.push(p);
        }
        let mut newcomer = generate_intake(5, PrisonerId(99));
        newcomer.background = "violent|new".into();
        newcomer.traits.aggression = 0.8;
        newcomer.traits.leadership = 0.9;
        newcomer.needs.safety = 0.9;
        let hooks = arrival_social_hooks(&[newcomer], &existing);
        assert_eq!(hooks.len(), 1);
        assert!(!hooks[0].friendship_seeds.is_empty());
        assert!(!hooks[0].rivalry_seeds.is_empty());
        assert!(!hooks[0].gang_join_candidates.is_empty());
    }
}
