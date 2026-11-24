//! Needs engine per Directive §4 (and design doc §3).
//!
//! Every need is a *deficit pressure* in `[0.0, 1.0]`: `0.0` means fully
//! satisfied, `1.0` means urgent deprivation. All needs fluctuate continuously
//! with time, activity, environment and personality (§4.2), relief only applies
//! when the corresponding action actually succeeded (§4.3), and the derived
//! helpers at the bottom of this file turn raw deficits into the concrete
//! behaviors the Directive requires:
//!
//! | Behavior | Helper |
//! |---|---|
//! | Hungry prisoners become irritable | [`Needs::irritability`] (feeds `Prisoner::aggression_modifier`) |
//! | Exhaustion lowers alertness / attendance | [`Needs::alertness`] |
//! | Isolation → depression / desperation | [`Needs::desperation`], [`Needs::depression_risk`] |
//! | Lack of safety → gang seeking | [`Needs::gang_seeking_pressure`] |
//! | Lack of stimulation → rule breaking | [`Needs::rule_breaking_urge`] |
//! | Lack of hope → reduced rehab motivation | [`Needs::rehab_motivation`] |
//! | Medical neglect → long-term consequences | [`Needs::medical_neglect`] |
//!
//! This module is intentionally free of AI/LLM calls: it is deterministic,
//! cheap enough to run every sim-minute, and auditable (Directive §19).

use crate::Traits;
use serde::{Deserialize, Serialize};

/// A need at or above this pressure is considered urgent (design doc §3.4).
pub const URGENT_THRESHOLD: f32 = 0.75;
/// Needs below this contribute no decision noise (design doc §3.7).
pub const DECISION_THRESHOLD: f32 = 0.05;

// -- Medical / neglect escalation thresholds (0.05 advisory hooks) ----------
//
// The world owns the actual Death / TransferOut / medical-hold decisions
// (Directive §16). These thresholds only *represent* severity so those
// pathways can be triggered by observable state instead of ad-hoc magic
// numbers — nothing here mutates state or forces world behavior.

/// `medical_need` at/above this is a mild-but-notable untreated deficit.
pub const ELEVATED_MEDICAL_THRESHOLD: f32 = 0.45;
/// `medical_need` at/above this is a severe untreated deficit (medical hold).
pub const SEVERE_MEDICAL_THRESHOLD: f32 = 0.75;
/// `medical_need` at/above this is an emergency (immediate care required).
pub const EMERGENCY_MEDICAL_THRESHOLD: f32 = 0.90;
/// [`Needs::medical_neglect`] at/above this counts as severe neglect.
pub const SEVERE_NEGLECT_THRESHOLD: f32 = 0.70;
/// [`Needs::medical_neglect`] at/above this is critical neglect — health is
/// degrading because care is not reaching the prisoner.
pub const CRITICAL_NEGLECT_THRESHOLD: f32 = 0.85;

// Condition-side thresholds live here too so the whole escalation ladder is
// documented in one place; `Condition` helpers in the crate root read them.
/// `Condition::health` at/below this is life-threatening (transfer/hospital).
pub const LIFE_THREAT_HEALTH_THRESHOLD: f32 = 0.20;
/// `Condition::health` at/below this is a fatality candidate under sustained
/// injury/illness/neglect.
pub const FATAL_HEALTH_THRESHOLD: f32 = 0.08;

/// Tags for the full §4 need catalog. Used to target relief and impulses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NeedTag {
    // Physical
    Hunger,
    Thirst,
    Sleep,
    Fatigue,
    Hygiene,
    Health,
    Medical,
    Exercise,
    Temperature,
    Bathroom,
    // Psychological
    Safety,
    Companionship,
    Respect,
    Freedom,
    Autonomy,
    Purpose,
    Stimulation,
    Hope,
    Stability,
    Privacy,
    Belonging,
    // Social
    Friendship,
    Romance,
    FamilyContact,
    GroupAcceptance,
    Status,
    Loyalty,
    Protection,
    Recognition,
}

impl NeedTag {
    /// Coarse category, mirroring design doc §3.2.
    pub fn category(self) -> NeedCategory {
        match self {
            NeedTag::Hunger | NeedTag::Thirst | NeedTag::Sleep | NeedTag::Fatigue
            | NeedTag::Hygiene | NeedTag::Health | NeedTag::Medical | NeedTag::Exercise
            | NeedTag::Temperature | NeedTag::Bathroom => NeedCategory::Physical,
            NeedTag::Safety | NeedTag::Companionship | NeedTag::Respect | NeedTag::Freedom
            | NeedTag::Autonomy | NeedTag::Purpose | NeedTag::Stimulation | NeedTag::Hope
            | NeedTag::Stability | NeedTag::Privacy | NeedTag::Belonging => NeedCategory::Psychological,
            NeedTag::Friendship | NeedTag::Romance | NeedTag::FamilyContact
            | NeedTag::GroupAcceptance | NeedTag::Status | NeedTag::Loyalty
            | NeedTag::Protection | NeedTag::Recognition => NeedCategory::Social,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NeedCategory {
    Physical,
    Psychological,
    Social,
}

/// Escalation ladder for medical / neglect state (0.05 "Death/transfer/medical
/// outcomes"). Ordered: `Stable < Elevated < Severe < Emergency < Critical`, so
/// `severity >= MedicalSeverity::Severe` is a natural triage gate.
///
/// Advisory only — the world decides whether a level triggers a medical hold,
/// a transfer, or (rarely, with cause) a death review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MedicalSeverity {
    /// No actionable medical pressure.
    Stable,
    /// Worth watching; no intervention required yet.
    Elevated,
    /// Severe medical need or neglect — medical-hold territory.
    Severe,
    /// Needs immediate care: hospitalization / transfer candidate.
    Emergency,
    /// Health is failing despite (or because of) neglect — death-review
    /// candidate for the world to evaluate with full context.
    Critical,
}

/// The complete need state of one prisoner.
///
/// Field names are kept stable because this is persisted state (Directive §23).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Needs {
    // Physical
    pub hunger: f32,
    pub thirst: f32,
    pub sleep: f32,
    pub fatigue: f32,
    pub hygiene: f32,
    pub medical_need: f32,
    pub exercise: f32,
    pub temperature_comfort: f32,
    pub bathroom: f32,
    // Psychological
    pub safety: f32,
    pub companionship: f32,
    pub respect: f32,
    pub freedom: f32,
    pub autonomy: f32,
    pub purpose: f32,
    pub stimulation: f32,
    pub stability: f32,
    pub privacy: f32,
    pub belonging: f32,
    // Social
    pub friendship: f32,
    pub family_contact: f32,
    pub group_acceptance: f32,
    pub status: f32,
    pub protection: f32,
    pub recognition: f32,
}

impl Default for Needs {
    fn default() -> Self {
        Self::new()
    }
}

impl Needs {
    /// Intake / start-of-life baseline (transit, arrest and separation already
    /// leave marks — see design doc §5.1 step 9).
    pub fn new() -> Self {
        Self {
            hunger: 0.2,
            thirst: 0.2,
            sleep: 0.1,
            fatigue: 0.0,
            hygiene: 0.1,
            medical_need: 0.0,
            exercise: 0.3,
            temperature_comfort: 0.1,
            bathroom: 0.1,
            safety: 0.2,
            companionship: 0.3,
            respect: 0.3,
            freedom: 0.4,
            autonomy: 0.4,
            purpose: 0.3,
            stimulation: 0.3,
            stability: 0.3,
            privacy: 0.4,
            belonging: 0.3,
            friendship: 0.3,
            family_contact: 0.5,
            group_acceptance: 0.3,
            status: 0.3,
            protection: 0.2,
            recognition: 0.4,
        }
    }

    /// Per-minute drift with a neutral context.
    ///
    /// Kept for compatibility with the world tick (`alc-world` calls
    /// `p.needs.drift(dt_mins)`); richer callers should use [`Self::drift_with`].
    pub fn drift(&mut self, dt_mins: u64) {
        self.drift_with(dt_mins, &NeedDriftContext::default());
    }

    /// Full §4.3 drift: continuous growth modulated by personality, current
    /// activity and environment, minus relief that actually succeeded.
    pub fn drift_with(&mut self, dt_mins: u64, ctx: &NeedDriftContext<'_>) {
        let hours = (dt_mins as f32 / 60.0).max(0.0);
        if hours <= 0.0 || !hours.is_finite() {
            return;
        }

        let sociability = ctx.traits.map_or(0.5, |t| t.sociability);
        let ambition = ctx.traits.map_or(0.5, |t| t.ambition);
        let stability_trait = ctx.traits.map_or(0.5, |t| t.emotional_stability);
        let intelligence = ctx.traits.map_or(0.5, |t| t.intelligence);
        let discipline = ctx.traits.map_or(0.5, |t| t.discipline);

        // A single, explicit update rule for every need (design doc §3.4).
        let step = |tag: NeedTag, value: &mut f32, rate: f32, trait_mult: f32, relief_rate: f32| {
            let activity = activity_term(ctx.activity, tag);
            let environment = environment_term(ctx, tag);
            // Both Sleep and Fatigue are driven by the `Sleep` tag (a night's
            // rest relieves both), so a single lookup covers them correctly.
            let relief = if ctx.relief.contains(&tag) { relief_rate } else { 0.0 };
            let delta = hours * (rate * trait_mult.max(0.0) + activity + environment - relief);
            *value = (*value + delta).clamp(0.0, 1.0);
        };

        // -- Physical -------------------------------------------------------
        step(NeedTag::Hunger, &mut self.hunger, 0.030, 1.0, 0.45);
        step(
            NeedTag::Thirst,
            &mut self.thirst,
            0.045,
            1.0 + ctx.heat * 0.6,
            0.40,
        );
        step(NeedTag::Sleep, &mut self.sleep, 0.025, 1.0, 0.55);
        step(NeedTag::Sleep, &mut self.fatigue, 0.020, 1.0, 0.55);
        step(NeedTag::Hygiene, &mut self.hygiene, 0.015, 1.0, 0.50);
        step(NeedTag::Medical, &mut self.medical_need, 0.008, 1.0, 0.40);
        step(NeedTag::Exercise, &mut self.exercise, 0.018, 1.0, 0.45);
        // Use each need's own tag so `environment_term`, `activity_term` and
        // `relief` target the correct field (they previously inherited the
        // Hygiene tag, so heat/relief for Temperature and Bathroom were lost).
        step(
            NeedTag::Temperature,
            &mut self.temperature_comfort,
            0.010,
            1.0 + ctx.heat * 0.5,
            0.35,
        );
        step(NeedTag::Bathroom, &mut self.bathroom, 0.040, 1.0, 0.60);

        // -- Psychological --------------------------------------------------
        step(
            NeedTag::Safety,
            &mut self.safety,
            0.008,
            1.0 + (1.0 - stability_trait),
            0.30,
        );
        step(
            NeedTag::Companionship,
            &mut self.companionship,
            0.012,
            1.0 + sociability,
            0.35,
        );
        step(
            NeedTag::Respect,
            &mut self.respect,
            0.006,
            1.0 + (1.0 - stability_trait) * 0.5,
            0.30,
        );
        step(
            NeedTag::Companionship,
            &mut self.freedom,
            0.010,
            1.0 + ambition * 0.5,
            0.10,
        );
        step(
            NeedTag::Companionship,
            &mut self.autonomy,
            0.008,
            1.0 + ambition * 0.5,
            0.15,
        );
        step(
            NeedTag::Companionship,
            &mut self.purpose,
            0.012,
            (1.0 - discipline * 0.3).max(0.2),
            0.30,
        );
        step(
            NeedTag::Stimulation,
            &mut self.stimulation,
            0.020,
            (1.0 + intelligence * 0.5 - discipline * 0.5).max(0.2),
            0.40,
        );
        step(
            NeedTag::Companionship,
            &mut self.stability,
            0.006,
            1.0,
            0.20,
        );
        step(
            NeedTag::Companionship,
            &mut self.privacy,
            0.010,
            1.0 + (1.0 - sociability) * 0.5,
            0.25,
        );
        step(
            NeedTag::Companionship,
            &mut self.belonging,
            0.010,
            1.0 + sociability * 0.5,
            0.30,
        );

        // -- Social ---------------------------------------------------------
        step(
            NeedTag::Companionship,
            &mut self.friendship,
            0.010,
            1.0 + sociability * 0.5,
            0.30,
        );
        step(
            NeedTag::Companionship,
            &mut self.family_contact,
            0.012,
            1.0,
            0.35,
        );
        step(
            NeedTag::Safety,
            &mut self.group_acceptance,
            0.008,
            1.0 + sociability * 0.3,
            0.30,
        );
        step(
            NeedTag::Companionship,
            &mut self.status,
            0.008,
            1.0 + ambition,
            0.25,
        );
        step(
            NeedTag::Safety,
            &mut self.protection,
            0.008,
            1.0 + (1.0 - stability_trait),
            0.30,
        );
        step(
            NeedTag::Companionship,
            &mut self.recognition,
            0.010,
            1.0 + ambition,
            0.25,
        );

        if !self.all_finite() {
            *self = Self::new();
        }
    }

    /// Relief when an activity actually satisfies a need (§4.3).
    ///
    /// Health/Hope/Romance/Loyalty live on other aggregates or have no direct
    /// pressure field here; their relief is projected onto the closest stored
    /// need so every tag has well-defined behavior.
    pub fn relieve(&mut self, need: NeedTag, amount: f32) {
        let amount = amount.max(0.0);
        match need {
            NeedTag::Hunger => self.hunger = (self.hunger - amount).max(0.0),
            NeedTag::Thirst => self.thirst = (self.thirst - amount).max(0.0),
            NeedTag::Sleep => {
                self.sleep = (self.sleep - amount).max(0.0);
                self.fatigue = (self.fatigue - amount).max(0.0);
            }
            NeedTag::Fatigue => self.fatigue = (self.fatigue - amount).max(0.0),
            NeedTag::Hygiene => {
                self.hygiene = (self.hygiene - amount).max(0.0);
                self.bathroom = (self.bathroom - amount * 0.8).max(0.0);
            }
            NeedTag::Health | NeedTag::Medical => {
                self.medical_need = (self.medical_need - amount).max(0.0)
            }
            NeedTag::Exercise => self.exercise = (self.exercise - amount).max(0.0),
            NeedTag::Temperature => {
                self.temperature_comfort = (self.temperature_comfort - amount).max(0.0)
            }
            NeedTag::Bathroom => self.bathroom = (self.bathroom - amount).max(0.0),
            NeedTag::Companionship => {
                self.companionship = (self.companionship - amount).max(0.0);
                self.friendship = (self.friendship - amount).max(0.0);
                self.belonging = (self.belonging - amount * 0.7).max(0.0);
            }
            NeedTag::Respect => self.respect = (self.respect - amount).max(0.0),
            NeedTag::Freedom => self.freedom = (self.freedom - amount).max(0.0),
            NeedTag::Autonomy => self.autonomy = (self.autonomy - amount).max(0.0),
            NeedTag::Purpose => self.purpose = (self.purpose - amount).max(0.0),
            NeedTag::Stimulation => self.stimulation = (self.stimulation - amount).max(0.0),
            NeedTag::Hope => self.purpose = (self.purpose - amount).max(0.0),
            NeedTag::Stability => self.stability = (self.stability - amount).max(0.0),
            NeedTag::Privacy => self.privacy = (self.privacy - amount).max(0.0),
            NeedTag::Belonging => self.belonging = (self.belonging - amount).max(0.0),
            NeedTag::Friendship | NeedTag::Romance => {
                self.friendship = (self.friendship - amount).max(0.0)
            }
            NeedTag::FamilyContact => self.family_contact = (self.family_contact - amount).max(0.0),
            NeedTag::GroupAcceptance => {
                self.group_acceptance = (self.group_acceptance - amount).max(0.0)
            }
            NeedTag::Status => self.status = (self.status - amount).max(0.0),
            NeedTag::Loyalty => self.belonging = (self.belonging - amount).max(0.0),
            NeedTag::Protection => self.protection = (self.protection - amount).max(0.0),
            NeedTag::Recognition => self.recognition = (self.recognition - amount).max(0.0),
            NeedTag::Safety => {
                self.safety = (self.safety - amount).max(0.0);
                self.protection = (self.protection - amount * 0.8).max(0.0);
            }
        }
    }

    /// Discrete impulses from events (e.g. a fight ⇒ Health/Safety/Respect
    /// jumps, design doc §3.4). Bounded and clamped.
    pub fn impulse(&mut self, need: NeedTag, amount: f32) {
        // A non-finite event magnitude must never poison persisted needs.
        if !amount.is_finite() {
            return;
        }
        // Keep Sleep impulses consistent with `relieve`/drift: losing sleep
        // raises both the sleep deficit and bodily fatigue.
        if need == NeedTag::Sleep {
            self.sleep = (self.sleep + amount).clamp(0.0, 1.0);
            self.fatigue = (self.fatigue + amount).clamp(0.0, 1.0);
            return;
        }
        let value = match need {
            NeedTag::Hunger => &mut self.hunger,
            NeedTag::Thirst => &mut self.thirst,
            NeedTag::Sleep => &mut self.sleep,
            NeedTag::Fatigue => &mut self.fatigue,
            NeedTag::Hygiene => &mut self.hygiene,
            NeedTag::Health | NeedTag::Medical => &mut self.medical_need,
            NeedTag::Exercise => &mut self.exercise,
            NeedTag::Temperature => &mut self.temperature_comfort,
            NeedTag::Bathroom => &mut self.bathroom,
            NeedTag::Safety => &mut self.safety,
            NeedTag::Companionship => &mut self.companionship,
            NeedTag::Respect => &mut self.respect,
            NeedTag::Freedom => &mut self.freedom,
            NeedTag::Autonomy => &mut self.autonomy,
            NeedTag::Purpose | NeedTag::Hope => &mut self.purpose,
            NeedTag::Stimulation => &mut self.stimulation,
            NeedTag::Stability => &mut self.stability,
            NeedTag::Privacy => &mut self.privacy,
            NeedTag::Belonging | NeedTag::Loyalty => &mut self.belonging,
            NeedTag::Friendship | NeedTag::Romance => &mut self.friendship,
            NeedTag::FamilyContact => &mut self.family_contact,
            NeedTag::GroupAcceptance => &mut self.group_acceptance,
            NeedTag::Status => &mut self.status,
            NeedTag::Protection => &mut self.protection,
            NeedTag::Recognition => &mut self.recognition,
        };
        *value = (*value + amount).clamp(0.0, 1.0);
    }

    /// Total weighted pressure — drives decision urgency (§3.7).
    /// Normalized to 0..1 as a weighted *average* across needs (the raw
    /// weighted sum saturates the 1.0 cap with only moderate needs, which
    /// flattened guard tension and parole behavior scores).
    pub fn total_pressure(&self) -> f32 {
        let weights = [
            (self.hunger, 1.2),
            (self.thirst, 1.3),
            (self.sleep, 1.0),
            (self.fatigue, 0.9),
            (self.safety, 1.1),
            (self.companionship, 0.6),
            (self.stimulation, 0.5),
            (self.medical_need, 1.4),
            (self.bathroom, 0.8),
            (self.respect, 0.7),
        ];
        let total: f32 = weights.iter().map(|(_, w)| *w).sum();
        (weights.iter().map(|(v, w)| v * w).sum::<f32>() / total).clamp(0.0, 1.0)
    }

    /// Dominant need for utility-based action selection.
    pub fn dominant(&self) -> (NeedTag, f32) {
        let candidates = [
            (NeedTag::Hunger, self.hunger * 1.2),
            (NeedTag::Thirst, self.thirst * 1.3),
            (NeedTag::Sleep, self.sleep.max(self.fatigue)),
            (NeedTag::Safety, self.safety * 1.1),
            (NeedTag::Companionship, self.companionship * 0.6),
            (NeedTag::Stimulation, self.stimulation * 0.5),
            (NeedTag::Medical, self.medical_need * 1.4),
            (NeedTag::Respect, self.respect * 0.7),
        ];
        candidates
            .iter()
            .copied()
            .fold((NeedTag::Hunger, f32::MIN), |acc, c| {
                if c.1 > acc.1 { c } else { acc }
            })
    }

    /// Needs at or above [`URGENT_THRESHOLD`], sorted worst-first. These form
    /// the motivation context handed to the AI layer (design doc §3.7).
    pub fn urgent(&self) -> Vec<(NeedTag, f32)> {
        let mut out = vec![
            (NeedTag::Medical, self.medical_need),
            (NeedTag::Thirst, self.thirst),
            (NeedTag::Hunger, self.hunger),
            (NeedTag::Safety, self.safety),
            (NeedTag::Sleep, self.sleep.max(self.fatigue)),
            (NeedTag::Respect, self.respect),
            (NeedTag::Stimulation, self.stimulation),
            (NeedTag::Companionship, self.companionship),
        ];
        out.retain(|(_, v)| *v >= URGENT_THRESHOLD);
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out
    }

    // ------------------------------------------------------------------
    // Needs → concrete behaviors (Directive §4 worked examples)
    // ------------------------------------------------------------------

    /// Hunger (and, secondarily, fatigue/thirst) makes a prisoner irritable.
    /// Consumed by `Prisoner::aggression_modifier` and the Conflict system.
    pub fn irritability(&self) -> f32 {
        (self.hunger * 0.65 + self.fatigue * 0.2 + self.thirst * 0.1
            + (1.0 - self.alertness()) * 0.1)
            .clamp(0.0, 1.0)
    }

    /// Exhaustion lowers alertness (and therefore attendance and vigilance).
    pub fn alertness(&self) -> f32 {
        (1.0 - (self.sleep * 0.55 + self.fatigue * 0.45)).clamp(0.0, 1.0)
    }

    /// Social isolation → depression / desperation. Combines the companionship
    /// family of deficits; the AI/Rehab systems add `hope` via
    /// [`Self::despair_pressure`].
    pub fn desperation(&self) -> f32 {
        (self.companionship * 0.35
            + self.friendship * 0.2
            + self.belonging * 0.15
            + self.family_contact * 0.1
            + self.freedom * 0.1
            + self.stimulation * 0.1)
            .clamp(0.0, 1.0)
    }

    /// Alias used by the mood/mental-health layer.
    pub fn depression_risk(&self) -> f32 {
        self.desperation()
    }

    /// Full despair pressure: isolation plus lost hope (§4 "lack of hope").
    pub fn despair_pressure(&self, hope: f32) -> f32 {
        (self.desperation() * 0.6 + (1.0 - hope.clamp(0.0, 1.0)) * 0.4).clamp(0.0, 1.0)
    }

    /// Lack of safety/protection creates pressure to seek a gang or patron
    /// (§4 "lack of safety → gangs"). Consumed by Goals/Gangs/intake hooks.
    pub fn gang_seeking_pressure(&self) -> f32 {
        (self.safety * 0.45 + self.protection * 0.35 + self.belonging * 0.2).clamp(0.0, 1.0)
    }

    /// Boredom plus low discipline produces an urge to break rules
    /// (§4 "lack of stimulation → rule-breaking"). Guard presence is applied by
    /// the caller (see `schedule::attendance_roll_with`).
    pub fn rule_breaking_urge(&self, discipline: f32) -> f32 {
        (self.stimulation * (1.0 - discipline.clamp(0.0, 1.0) * 0.7) * self.alertness())
            .clamp(0.0, 1.0)
    }

    /// Hope is held on `Condition`, not here, so it is passed in. Low hope
    /// (especially with despair) suppresses rehabilitation participation.
    pub fn rehab_motivation(&self, hope: f32) -> f32 {
        let hope = hope.clamp(0.0, 1.0);
        (hope * (1.0 - self.desperation() * 0.35) * (1.0 - self.freedom * 0.1)).clamp(0.0, 1.0)
    }

    /// Complement of [`Self::rehab_motivation`]'s hope input.
    pub fn hope_deficit(&self, hope: f32) -> f32 {
        (1.0 - hope.clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }

    /// Unmet medical/health needs feed long-term health degradation (§8/§11).
    pub fn medical_neglect(&self) -> f32 {
        (self.medical_need * 0.7 + self.hygiene * 0.2 + self.fatigue * 0.1).clamp(0.0, 1.0)
    }

    // -- Medical/neglect triage hooks (0.05, advisory only) -----------------

    /// Severity implied by needs alone. `Condition` severity is layered on top
    /// by `Prisoner::medical_crisis`, so this never reaches `Critical` — that
    /// level is reserved for failing *health*, not unmet need.
    pub fn medical_severity(&self) -> MedicalSeverity {
        let neglect = self.medical_neglect();
        if self.medical_need >= EMERGENCY_MEDICAL_THRESHOLD || neglect >= CRITICAL_NEGLECT_THRESHOLD {
            MedicalSeverity::Emergency
        } else if self.medical_need >= SEVERE_MEDICAL_THRESHOLD || neglect >= SEVERE_NEGLECT_THRESHOLD {
            MedicalSeverity::Severe
        } else if self.medical_need >= ELEVATED_MEDICAL_THRESHOLD {
            MedicalSeverity::Elevated
        } else {
            MedicalSeverity::Stable
        }
    }

    /// Severe neglect: care is not reaching the prisoner (medical hold and
    /// long-term health degradation both key off this).
    pub fn is_severe_neglect(&self) -> bool {
        self.medical_neglect() >= SEVERE_NEGLECT_THRESHOLD
    }

    /// Emergency-level unmet medical need (immediate care required).
    pub fn is_medical_emergency(&self) -> bool {
        self.medical_need >= EMERGENCY_MEDICAL_THRESHOLD
            || self.medical_neglect() >= CRITICAL_NEGLECT_THRESHOLD
    }

    fn all_finite(&self) -> bool {
        [
            self.hunger, self.thirst, self.sleep, self.fatigue, self.hygiene,
            self.medical_need, self.exercise, self.temperature_comfort, self.bathroom,
            self.safety, self.companionship, self.respect, self.freedom, self.autonomy,
            self.purpose, self.stimulation, self.stability, self.privacy, self.belonging,
            self.friendship, self.family_contact, self.group_acceptance, self.status,
            self.protection, self.recognition,
        ]
        .iter()
        .all(|v| v.is_finite())
    }
}

/// The activity currently driving need growth/relief (design doc §3.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriftActivity {
    #[default]
    None,
    Work,
    Yard,
    Recreation,
    Eat,
    Sleep,
    Medical,
    Conflict,
    Visitor,
    Lockdown,
}

/// Context for [`Needs::drift_with`]: observation of the world the prisoner
/// actually experiences. Never omniscient — callers pass their local view.
#[derive(Debug, Clone)]
pub struct NeedDriftContext<'a> {
    pub traits: Option<&'a Traits>,
    pub activity: DriftActivity,
    /// Needs relieved by actions that succeeded *this* tick.
    pub relief: &'a [NeedTag],
    /// 0..1 crowding — raises Privacy/FamilyContact pressure.
    pub crowd_density: f32,
    /// 0..1 heat/exertion — raises Thirst/Temperature pressure.
    pub heat: f32,
    /// 0..1 guard coverage — low coverage raises Safety/Protection pressure.
    pub staff_coverage: f32,
    pub lockdown: bool,
}

impl Default for NeedDriftContext<'_> {
    fn default() -> Self {
        Self {
            traits: None,
            activity: DriftActivity::None,
            relief: &[],
            crowd_density: 0.0,
            heat: 0.0,
            staff_coverage: 1.0,
            lockdown: false,
        }
    }
}

impl<'a> NeedDriftContext<'a> {
    pub fn with_traits(traits: &'a Traits) -> Self {
        Self {
            traits: Some(traits),
            ..Self::default()
        }
    }
}

/// Additive hourly term from the current activity (design doc §3.4). Negative
/// values are legitimate relief (e.g. eating reduces Hunger).
fn activity_term(activity: DriftActivity, need: NeedTag) -> f32 {
    use DriftActivity as A;
    use NeedTag as N;
    match (activity, need) {
        (A::Work, N::Fatigue) => 0.030,
        (A::Work, N::Hygiene) => 0.012,
        (A::Work, N::Exercise) => -0.010,
        (A::Work, N::Purpose) => -0.012,
        (A::Yard, N::Exercise) => -0.030,
        (A::Yard, N::Companionship) => -0.015,
        (A::Yard, N::Stimulation) => -0.010,
        (A::Recreation, N::Stimulation) => -0.020,
        (A::Recreation, N::Friendship) => -0.010,
        (A::Eat, N::Hunger) => -0.350,
        (A::Eat, N::Thirst) => -0.120,
        (A::Sleep, N::Sleep) => -0.400,
        (A::Sleep, N::Fatigue) => -0.300,
        (A::Medical, N::Medical) => -0.300,
        (A::Conflict, N::Safety) => 0.250,
        (A::Conflict, N::Respect) => 0.200,
        (A::Visitor, N::FamilyContact) => -0.300,
        (A::Lockdown, N::Freedom) => 0.040,
        (A::Lockdown, N::Autonomy) => 0.030,
        (A::Lockdown, N::Stimulation) => 0.035,
        (A::Lockdown, N::Privacy) => 0.020,
        _ => 0.0,
    }
}

/// Additive hourly term from the locally observed environment.
fn environment_term(ctx: &NeedDriftContext<'_>, need: NeedTag) -> f32 {
    use NeedTag as N;
    let crowding = ctx.crowd_density.clamp(0.0, 1.0);
    let heat = ctx.heat.clamp(0.0, 1.0);
    let low_staff = 1.0 - ctx.staff_coverage.clamp(0.0, 1.0);
    let lockdown = if ctx.lockdown { 1.0 } else { 0.0 };
    match need {
        N::Thirst => heat * 0.030,
        N::Temperature => heat * 0.025,
        N::Privacy => crowding * 0.030 + lockdown * 0.020,
        N::FamilyContact => crowding * 0.005,
        N::Safety => low_staff * 0.030,
        N::Protection => low_staff * 0.025,
        N::Freedom => lockdown * 0.030,
        N::Autonomy => lockdown * 0.025,
        N::Stimulation => lockdown * 0.030,
        _ => 0.0,
    }
}

/// Needs that can be published as cross-system effects (design doc §3.7).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum NeedsEffect {
    Irritability(f32),
    LowAlertness(f32),
    DepressionRisk(f32),
    GangSeeking(f32),
    RuleBreakingUrge(f32),
    LowRehabMotivation(f32),
    MedicalNeglect(f32),
}

impl Needs {
    /// Auditable bundle of all derived needs→behavior effects. Deterministic
    /// and consumed by Social/Conflict/Rehab/Health systems.
    pub fn effects(&self, traits: &Traits, hope: f32) -> Vec<NeedsEffect> {
        vec![
            NeedsEffect::Irritability(self.irritability()),
            NeedsEffect::LowAlertness(1.0 - self.alertness()),
            NeedsEffect::DepressionRisk(self.depression_risk()),
            NeedsEffect::GangSeeking(self.gang_seeking_pressure()),
            NeedsEffect::RuleBreakingUrge(self.rule_breaking_urge(traits.discipline)),
            NeedsEffect::LowRehabMotivation(1.0 - self.rehab_motivation(hope)),
            NeedsEffect::MedicalNeglect(self.medical_neglect()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neutral() -> NeedDriftContext<'static> {
        NeedDriftContext::default()
    }

    #[test]
    fn hunger_rises_when_unmet_and_irritability_follows() {
        let mut n = Needs::new();
        let before = n.hunger;
        n.drift_with(120, &neutral());
        assert!(n.hunger > before);
        assert!(n.irritability() > 0.0);
    }

    #[test]
    fn relief_reduces_only_the_targeted_need() {
        let mut n = Needs::new();
        let hunger = n.hunger;
        let thirst = n.thirst;
        n.relieve(NeedTag::Hunger, 0.15);
        assert!(n.hunger < hunger);
        assert_eq!(n.thirst, thirst);
    }

    #[test]
    fn needs_are_bounded() {
        let mut n = Needs::new();
        n.drift_with(60 * 24 * 30, &neutral());
        assert!(n.hunger <= 1.0 && n.hunger >= 0.0);
        assert!(n.total_pressure().is_finite());
    }

    #[test]
    fn exhaustion_lowers_alertness_and_safety_drives_gang_seeking() {
        let mut n = Needs::new();
        n.fatigue = 0.9;
        n.sleep = 0.8;
        assert!(n.alertness() < 0.2);
        n.safety = 0.9;
        n.protection = 0.8;
        assert!(n.gang_seeking_pressure() > 0.7);
    }

    #[test]
    fn low_hope_suppresses_rehab_and_boredom_drives_rule_breaking() {
        let n = Needs::new();
        assert!(n.rehab_motivation(0.1) < n.rehab_motivation(0.9));
        let mut bored = Needs::new();
        bored.stimulation = 0.95;
        assert!(bored.rule_breaking_urge(0.1) > bored.rule_breaking_urge(0.9));
    }

    // -- Gameplay-depth checks (Directive §4: needs must never be static) ---

    /// Coarse hourly view of `ScheduleTemplate::default_medium()`; mirrors the
    /// activity mapping used by the world tick without depending on alc-world.
    fn hourly_activity(minute: u16) -> DriftActivity {
        use DriftActivity as A;
        match minute {
            0..360 => A::Sleep,
            360..390 => A::None,  // wake
            390..435 => A::Eat,   // breakfast
            435..450 => A::None,
            450..690 => A::Work,
            690..705 => A::None,
            705..750 => A::Eat,   // lunch
            750..765 => A::None,
            765..825 => A::Work,  // education
            825..840 => A::None,
            840..930 => A::Yard,
            930..945 => A::None,
            945..990 => A::Recreation,
            990..1050 => A::None, // showers
            1050..1095 => A::Eat, // dinner
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

    fn assert_all_needs_valid(n: &Needs, step: u32) {
        let values = [
            ("hunger", n.hunger), ("thirst", n.thirst), ("sleep", n.sleep),
            ("fatigue", n.fatigue), ("hygiene", n.hygiene),
            ("medical_need", n.medical_need), ("exercise", n.exercise),
            ("temperature_comfort", n.temperature_comfort), ("bathroom", n.bathroom),
            ("safety", n.safety), ("companionship", n.companionship),
            ("respect", n.respect), ("freedom", n.freedom), ("autonomy", n.autonomy),
            ("purpose", n.purpose), ("stimulation", n.stimulation),
            ("stability", n.stability), ("privacy", n.privacy), ("belonging", n.belonging),
            ("friendship", n.friendship), ("family_contact", n.family_contact),
            ("group_acceptance", n.group_acceptance), ("status", n.status),
            ("protection", n.protection), ("recognition", n.recognition),
        ];
        for (name, v) in values {
            assert!(v.is_finite() && (0.0..=1.0).contains(&v), "hour {step}: {name}={v}");
        }
    }

    #[test]
    fn needs_drift_over_30h_hunger_sleep_and_meals_fluctuate() {
        let mut n = Needs::new();
        let initial_hunger = n.hunger;
        let mut hunger_after_night = 0.0;
        let mut hunger_after_breakfast = 0.0;
        let mut sleep_after_night = 0.0;
        let mut sleep_before_second_night = 0.0;
        let mut sleep_after_second_night = 0.0;

        for hour in 0..30u32 {
            let minute = ((hour * 60) % 1440) as u16;
            let activity = hourly_activity(minute);
            let relief = relief_for(activity);
            let ctx = NeedDriftContext {
                activity,
                relief: &relief,
                crowd_density: 0.4,
                heat: 0.2,
                staff_coverage: 0.7,
                lockdown: activity == DriftActivity::Lockdown,
                ..NeedDriftContext::default()
            };
            n.drift_with(60, &ctx);
            assert_all_needs_valid(&n, hour);
            match hour {
                5 => {
                    hunger_after_night = n.hunger;
                    sleep_after_night = n.sleep;
                }
                7 => hunger_after_breakfast = n.hunger,
                23 => sleep_before_second_night = n.sleep,
                29 => sleep_after_second_night = n.sleep,
                _ => {}
            }
        }

        // Six hours asleep with no food builds a real hunger deficit...
        assert!(hunger_after_night > initial_hunger + 0.10,
            "hunger did not rise overnight: {initial_hunger} -> {hunger_after_night}");
        // ...which breakfast then relieves measurably.
        assert!(hunger_after_breakfast < hunger_after_night - 0.15,
            "breakfast did not relieve hunger: {hunger_after_night} -> {hunger_after_breakfast}");
        // Sleep need is paid down overnight, accumulates through the waking day...
        assert!(sleep_after_night < 0.05, "sleep need not relieved overnight: {sleep_after_night}");
        assert!(sleep_before_second_night > sleep_after_night + 0.30,
            "sleep need did not build while awake: {sleep_before_second_night}");
        // ...and drops again during the next sleep block.
        assert!(sleep_after_second_night < sleep_before_second_night - 0.30,
            "second sleep block did not relieve sleep need: {sleep_after_second_night}");
    }

    #[test]
    fn temperature_and_bathroom_drift_use_their_own_tags() {
        let relief = [NeedTag::Temperature, NeedTag::Bathroom];
        let hot_with_relief = NeedDriftContext { relief: &relief, heat: 1.0, ..NeedDriftContext::default() };
        let hot = NeedDriftContext { heat: 1.0, ..NeedDriftContext::default() };
        let cool = NeedDriftContext::default();

        let mut a = Needs::new();
        let mut b = Needs::new();
        let mut c = Needs::new();
        a.drift_with(60, &hot_with_relief);
        b.drift_with(60, &hot);
        c.drift_with(60, &cool);

        // Both targeted needs are relieved only when their own tag is present.
        assert!(a.temperature_comfort < b.temperature_comfort);
        assert!(a.bathroom < b.bathroom);
        // Heat genuinely raises temperature pressure (environment term now lands).
        assert!(b.temperature_comfort > c.temperature_comfort);
    }

    #[test]
    fn extreme_and_nonfinite_inputs_never_corrupt_needs() {
        let mut n = Needs::new();
        n.drift_with(u64::MAX, &NeedDriftContext::default());
        assert_all_needs_valid(&n, 0);
        n.impulse(NeedTag::Respect, f32::NAN);
        n.impulse(NeedTag::Sleep, f32::INFINITY);
        n.relieve(NeedTag::Hunger, f32::NAN);
        assert_all_needs_valid(&n, 1);
        n.impulse(NeedTag::Sleep, 0.5);
        assert!(n.sleep > 0.0 && n.fatigue > 0.0, "sleep impulse must raise both deficits");
    }

    // -- 0.05 medical/neglect triage hooks (Death/TransferOut pathways) -----

    #[test]
    fn medical_severity_climbs_the_threshold_ladder() {
        let mut n = Needs::new();
        assert_eq!(n.medical_severity(), MedicalSeverity::Stable);
        assert!(!n.is_severe_neglect());

        n.medical_need = ELEVATED_MEDICAL_THRESHOLD;
        assert_eq!(n.medical_severity(), MedicalSeverity::Elevated);

        n.medical_need = SEVERE_MEDICAL_THRESHOLD;
        assert_eq!(n.medical_severity(), MedicalSeverity::Severe);

        n.medical_need = EMERGENCY_MEDICAL_THRESHOLD;
        assert_eq!(n.medical_severity(), MedicalSeverity::Emergency);
        assert!(n.is_medical_emergency());

        // Neglect (untreated medical need + poor hygiene/fatigue) escalates on
        // its own curve: hygiene/fatigue contribute up to 0.30 of
        // `medical_neglect`, so a moderate medical_need plus squalor reaches
        // Severe before the raw `medical_need` threshold does.
        let mut neglected = Needs::new();
        neglected.medical_need = 0.60;
        neglected.hygiene = 1.0;
        neglected.fatigue = 1.0;
        assert!(neglected.is_severe_neglect(), "neglect ladder must key off hygiene/fatigue too");
        assert_eq!(neglected.medical_severity(), MedicalSeverity::Severe);

        // Critical neglect needs sustained, near-emergency medical need.
        neglected.medical_need = 0.80;
        assert!(neglected.medical_neglect() >= CRITICAL_NEGLECT_THRESHOLD);
        assert_eq!(neglected.medical_severity(), MedicalSeverity::Emergency);
    }

    #[test]
    fn severity_levels_are_ordered_for_triage_comparisons() {
        assert!(MedicalSeverity::Stable < MedicalSeverity::Elevated);
        assert!(MedicalSeverity::Severe < MedicalSeverity::Emergency);
        assert!(MedicalSeverity::Emergency < MedicalSeverity::Critical);
        assert!(MedicalSeverity::Severe >= MedicalSeverity::Severe);
    }
}
