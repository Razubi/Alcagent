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
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash,
)]
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

impl Condition {
    /// Combined physical trauma load in 0..1 (injury dominates, illness
    /// persists underneath). Used by the medical triage hooks below.
    pub fn trauma_load(&self) -> f32 {
        (self.injury.clamp(0.0, 1.0) * 0.6 + self.illness.clamp(0.0, 1.0) * 0.4).clamp(0.0, 1.0)
    }

    /// Severity implied by *health* alone, using the shared escalation ladder
    /// in [`needs`] (`Stable < Elevated < Severe < Emergency < Critical`).
    ///
    /// Health bands:
    /// - `Critical`  — `health <= FATAL_HEALTH_THRESHOLD`
    /// - `Emergency` — life-threatening health, or failing health under heavy trauma
    /// - `Severe`    — seriously compromised health or sustained trauma
    /// - `Elevated`  — visibly unwell
    pub fn severity(&self) -> MedicalSeverity {
        let health = self.health.clamp(0.0, 1.0);
        let trauma = self.trauma_load();
        if health <= FATAL_HEALTH_THRESHOLD {
            MedicalSeverity::Critical
        } else if health <= LIFE_THREAT_HEALTH_THRESHOLD || (health <= 0.35 && trauma >= 0.75) {
            MedicalSeverity::Emergency
        } else if health <= 0.45 || trauma >= 0.70 {
            MedicalSeverity::Severe
        } else if health <= 0.65 || trauma >= 0.40 {
            MedicalSeverity::Elevated
        } else {
            MedicalSeverity::Stable
        }
    }

    /// Health is failing enough to warrant hospitalization / transfer review.
    pub fn is_critical(&self) -> bool {
        self.severity() >= MedicalSeverity::Emergency
    }

    /// Near-zero health: a fatality candidate under sustained injury, illness
    /// or neglect. Advisory — the world evaluates context before acting.
    pub fn is_fatal(&self) -> bool {
        self.severity() == MedicalSeverity::Critical
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

// ---------------------------------------------------------------------------
// 0.05 lifecycle state — all persisted fields on `Prisoner` carry
// `#[serde(default)]` so pre-0.05 saves keep loading unchanged (Directive §23).
// ---------------------------------------------------------------------------

/// Rehabilitation enrollment bookkeeping (Directive §14, gap report Slice 3).
///
/// Program *progress* still lives in `Prisoner::skills` under the
/// `alc_social::rehab::RehabProgram::key()` keys — `alc-social` depends on
/// this crate, so the progress engine stays there. This record adds the
/// enrollment/outcome lifecycle the world and UI need to read directly.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RehabRecord {
    /// Program key currently enrolled in (`"rehab_education"` etc.).
    pub program: Option<String>,
    /// World day the current enrollment started.
    pub enrolled_day: Option<u32>,
    /// Attendance sessions recorded while enrolled.
    pub sessions_attended: u32,
    /// Lifetime program completions (feeds parole + post-release records).
    pub completions: u32,
    /// Lifetime dropouts (a parole/reputation signal, not a dead end).
    pub dropouts: u32,
}

impl RehabRecord {
    pub fn is_enrolled(&self) -> bool {
        self.program.is_some()
    }

    /// Record the start of an enrollment (world calls this after
    /// `alc_social::rehab::accept`/`enroll_in_program` succeeds).
    pub fn enroll(&mut self, program: impl Into<String>, day: u32) {
        self.program = Some(program.into());
        self.enrolled_day = Some(day);
    }

    pub fn record_session(&mut self) {
        self.sessions_attended = self.sessions_attended.saturating_add(1);
    }

    /// Program finished successfully; returns its key and clears enrollment.
    pub fn record_completion(&mut self) -> Option<String> {
        let key = self.program.take()?;
        self.enrolled_day = None;
        self.completions = self.completions.saturating_add(1);
        Some(key)
    }

    /// Program abandoned/removed; returns its key and clears enrollment.
    pub fn record_dropout(&mut self) -> Option<String> {
        let key = self.program.take()?;
        self.enrolled_day = None;
        self.dropouts = self.dropouts.saturating_add(1);
        Some(key)
    }
}

/// Contraband carried by one prisoner (gap report Slice 3, "contraband
/// lifecycle"). Item names match `alc_social::contraband`'s item vocabulary;
/// this crate never invents items itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContrabandState {
    /// Item names currently in inventory.
    pub items: Vec<String>,
    /// World day of the last search (guard discovery), if searched.
    pub last_search_day: Option<u32>,
    /// World day of the last confiscation.
    pub last_confiscation_day: Option<u32>,
    /// Times caught carrying — a disciplinary/reputation signal.
    pub offenses: u32,
}

impl ContrabandState {
    pub fn is_carrying(&self) -> bool {
        !self.items.is_empty()
    }

    pub fn carry(&mut self, item: impl Into<String>) {
        let item = item.into();
        if !self.items.contains(&item) {
            self.items.push(item);
        }
    }

    pub fn has(&self, item: &str) -> bool {
        self.items.iter().any(|i| i == item)
    }

    /// Guard search: records the day and returns everything confiscated.
    pub fn confiscate(&mut self, day: u32) -> Vec<String> {
        self.last_search_day = Some(day);
        if !self.items.is_empty() {
            self.last_confiscation_day = Some(day);
            self.offenses = self.offenses.saturating_add(1);
        }
        std::mem::take(&mut self.items)
    }
}

/// How a prisoner left the institution. `Death` and `TransferOut` also carry a
/// record — the gap report groups them with post-release for history purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReleasePathway {
    Parole,
    SentenceComplete,
    Escape,
    TransferOut,
    Death,
    Pardon,
}

/// High-level post-release outcome (Directive §15: track at an appropriate
/// abstraction; the world updates this from outside data, not this crate).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostReleaseOutcome {
    #[default]
    Unknown,
    Reintegrated,
    Employed,
    HousingStable,
    Violation,
    ReturnedToCustody,
    Deceased,
}

/// Snapshot taken when a prisoner departs. The world stores these in its own
/// history (the prisoner entity itself leaves `World::prisoners`), so this
/// type is plain data with no behavior beyond construction helpers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostReleaseRecord {
    pub prisoner_id: PrisonerId,
    pub name: String,
    pub age: u32,
    /// World day of departure.
    pub day: u32,
    pub pathway: ReleasePathway,
    pub sentence_days: u32,
    pub served_days: u32,
    /// Rehab completions at departure (reintegration signal).
    pub programs_completed: u32,
    /// Contraband still on the person at departure, if any.
    pub carried_contraband: Vec<String>,
    /// Reputation-derived behavior score at departure (0..1, higher = better).
    pub behavior_score: f32,
    /// Escape motivation at departure (0..1, higher = riskier).
    pub risk_score: f32,
    /// Free-text context (cause of death, transfer destination, conditions…).
    pub notes: String,
    /// Filled in later from post-release tracking.
    pub outcome: PostReleaseOutcome,
    pub outcome_day: Option<u32>,
}

/// One disciplinary action recorded against a prisoner (Directive §8). Strings
/// keep this crate free of the crime system's types (§2 typed contracts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisciplineRecord {
    /// World day of the infraction.
    pub day: u32,
    /// What happened: `"refusal"`, `"fight"`, `"contraband"`, `"escape-attempt"`…
    pub kind: String,
    /// What was done: `"warning"`, `"loss-of-privileges"`, `"solitary"`…
    pub action: String,
    /// 0..1 severity (feeds reputation and parole scoring).
    pub severity: f32,
}

/// One medical event/outcome recorded against a prisoner (gap report
/// "Death/transfer/medical outcomes"). `severity` comes from the shared
/// [`MedicalSeverity`] ladder so world rules can compare consistently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MedicalOutcomeRecord {
    /// World day of the event.
    pub day: u32,
    pub severity: MedicalSeverity,
    /// What happened: `"neglect"`, `"fight-injury"`, `"illness"`, `"overdose"`…
    pub cause: String,
    /// Whether care was actually delivered.
    pub treated: bool,
    /// Where it was handled: `"cell"`, `"infirmary"`, `"hospital"`, `"transfer"`…
    pub disposition: String,
}

/// Retention caps for the per-prisoner outcome histories (bounded memory even
/// across multi-year runs — Directive §22 performance).
pub const MAX_DISCIPLINE_RECORDS: usize = 16;
pub const MAX_MEDICAL_RECORDS: usize = 8;

/// Composite medical triage result: worst severity from condition *and* needs,
/// plus what is driving it. Advisory input for the world's medical / transfer /
/// death pathways — returning a crisis never mutates state by itself.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MedicalCrisis {
    pub severity: MedicalSeverity,
    pub cause: MedicalCrisisCause,
    /// 0..1 composite triage score (higher = worse); useful for ranking a
    /// whole population for the infirmary without bespoke comparisons.
    pub score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MedicalCrisisCause {
    /// Untreated need / poor hygiene (care never reached the prisoner).
    Neglect,
    Injury,
    Illness,
    /// Mental-health driven presentation.
    MentalHealth,
    /// Two or more contributors at comparable weight.
    Combined,
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
    /// 0.05 rehab enrollment lifecycle (progress itself lives in `skills`).
    #[serde(default)]
    pub rehab: RehabRecord,
    /// 0.05 contraband inventory flags.
    #[serde(default)]
    pub contraband: ContrabandState,
    /// Set when the prisoner departs; the world copies it into history before
    /// dropping the entity (parole/escape/transfer/death all snapshot here).
    #[serde(default)]
    pub post_release: Option<PostReleaseRecord>,
    /// Recent disciplinary actions (bounded, newest last).
    #[serde(default)]
    pub discipline_history: Vec<DisciplineRecord>,
    /// Recent medical outcomes (bounded, newest last).
    #[serde(default)]
    pub medical_history: Vec<MedicalOutcomeRecord>,
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
            rehab: RehabRecord::default(),
            contraband: ContrabandState::default(),
            post_release: None,
            discipline_history: Vec::new(),
            medical_history: Vec::new(),
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
        let first = [
            "Marcus", "Dmitri", "Andre", "Tyrell", "Viktor", "Hector", "Samuel", "Kwame", "Liam",
            "Jose",
        ];
        let last = [
            "Volkov",
            "Mercer",
            "Okafor",
            "Reyes",
            "Kowalski",
            "Dubois",
            "Haddad",
            "Fitzgerald",
            "Osei",
            "Marsh",
        ];
        let name = format!(
            "{} {}",
            first[(rng.next_u64() % 10) as usize],
            last[(rng.next_u64() % 10) as usize]
        );
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
            goals.push(Goal {
                kind: GoalKind::Escape,
                progress: 0.0,
                urgency: 0.8,
                description: "Find a way out".into(),
            });
        }
        if rng.chance(0.4) {
            goals.push(Goal {
                kind: GoalKind::GangLeadership,
                progress: 0.1,
                urgency: 0.5,
                description: "Rise to the top".into(),
            });
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
            rehab: RehabRecord::default(),
            contraband: ContrabandState::default(),
            post_release: None,
            discipline_history: Vec::new(),
            medical_history: Vec::new(),
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

    // -- 0.05 lifecycle hooks (all additive; existing behavior unchanged) ---

    /// Program keys this prisoner is enrolled in, read from the `skills` map
    /// (the `alc_social::rehab` key convention: `"rehab_…"`). The world writes
    /// progress there; this is the read side.
    pub fn enrolled_rehab_programs(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .skills
            .keys()
            .filter(|k| k.starts_with("rehab_"))
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    /// Whether any rehabilitation program is in progress.
    pub fn is_rehab_enrolled(&self) -> bool {
        self.rehab.is_enrolled() || self.skills.keys().any(|k| k.starts_with("rehab_"))
    }

    /// Enrollment hook: records the program in [`Prisoner::rehab`] *and* seeds
    /// the progress key `alc_social::rehab::apply_program_progress` reads.
    ///
    /// Tolerates either wiring order: call it before `alc-social` seeds the
    /// key (this creates it) or after (the existing key is left untouched).
    /// One *primary* program at a time — returns `false` if another program is
    /// already the primary enrollment. The willingness roll stays in
    /// `alc-social`; call this after it accepts.
    pub fn enroll_rehab(&mut self, program_key: &str, day: u32) -> bool {
        if self.rehab.is_enrolled() {
            // Already tracking a primary program (this one or another):
            // no *new* enrollment happened, so report false (callers gate
            // "ProgramEnrolled" events on this).
            return false;
        }
        self.rehab.enroll(program_key, day);
        // `or_insert` keeps an alc-social-seeded progress value intact.
        self.skills.entry(program_key.to_string()).or_insert(0.0);
        true
    }

    /// Progress (0..1) of a program this prisoner is enrolled in.
    pub fn rehab_progress(&self, program_key: &str) -> Option<f32> {
        self.skills.get(program_key).copied()
    }

    /// Record one attendance session in the current program.
    pub fn record_rehab_session(&mut self) {
        self.rehab.record_session();
    }

    /// Program completed: clears enrollment, counts the completion, and drops
    /// the progress key (mirrors `RehabOutcome::Completed`). Works whether or
    /// not the enrollment hook was used — the world just reports the outcome.
    pub fn complete_rehab(&mut self, program_key: &str) -> Option<String> {
        let tracked = self.rehab.program.as_deref() == Some(program_key);
        if !tracked && !self.skills.contains_key(program_key) {
            return None;
        }
        self.skills.remove(program_key);
        if tracked {
            self.rehab.record_completion();
        } else {
            self.rehab.completions = self.rehab.completions.saturating_add(1);
        }
        Some(program_key.to_string())
    }

    /// Program dropped: clears enrollment, counts the dropout, drops the key
    /// (mirrors `RehabOutcome::Dropped`).
    pub fn drop_rehab(&mut self, program_key: &str) -> Option<String> {
        let tracked = self.rehab.program.as_deref() == Some(program_key);
        if !tracked && !self.skills.contains_key(program_key) {
            return None;
        }
        self.skills.remove(program_key);
        if tracked {
            self.rehab.record_dropout();
        } else {
            self.rehab.dropouts = self.rehab.dropouts.saturating_add(1);
        }
        Some(program_key.to_string())
    }

    /// Append a disciplinary action, keeping only the most recent
    /// [`MAX_DISCIPLINE_RECORDS`] entries.
    pub fn record_discipline(
        &mut self,
        day: u32,
        kind: impl Into<String>,
        action: impl Into<String>,
        severity: f32,
    ) {
        self.discipline_history.push(DisciplineRecord {
            day,
            kind: kind.into(),
            action: action.into(),
            severity: severity.clamp(0.0, 1.0),
        });
        let overflow = self
            .discipline_history
            .len()
            .saturating_sub(MAX_DISCIPLINE_RECORDS);
        if overflow > 0 {
            self.discipline_history.drain(0..overflow);
        }
    }

    /// Append a medical outcome, keeping only the most recent
    /// [`MAX_MEDICAL_RECORDS`] entries.
    pub fn record_medical_outcome(
        &mut self,
        day: u32,
        severity: MedicalSeverity,
        cause: impl Into<String>,
        treated: bool,
        disposition: impl Into<String>,
    ) {
        self.medical_history.push(MedicalOutcomeRecord {
            day,
            severity,
            cause: cause.into(),
            treated,
            disposition: disposition.into(),
        });
        let overflow = self
            .medical_history
            .len()
            .saturating_sub(MAX_MEDICAL_RECORDS);
        if overflow > 0 {
            self.medical_history.drain(0..overflow);
        }
    }

    /// Composite medical triage from condition *and* needs. `None` below
    /// `Severe`. Advisory only: the world decides whether a crisis means a
    /// medical hold, a transfer, or a death review with fuller context.
    pub fn medical_crisis(&self) -> Option<MedicalCrisis> {
        let from_condition = self.condition.severity();
        let from_needs = self.needs.medical_severity();
        let severity = from_condition.max(from_needs);
        if severity < MedicalSeverity::Severe {
            return None;
        }

        // Dominant driver: neglect (needs) vs injury vs illness vs mental state.
        let neglect = self.needs.medical_neglect();
        let mental = 1.0 - self.condition.mental_health.clamp(0.0, 1.0);
        let contributors = [
            (MedicalCrisisCause::Neglect, neglect),
            (
                MedicalCrisisCause::Injury,
                self.condition.injury.clamp(0.0, 1.0),
            ),
            (
                MedicalCrisisCause::Illness,
                self.condition.illness.clamp(0.0, 1.0),
            ),
            (MedicalCrisisCause::MentalHealth, mental),
        ];
        let mut ranked = contributors;
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        let cause = if ranked[0].1 - ranked[1].1 < 0.10 {
            MedicalCrisisCause::Combined
        } else {
            ranked[0].0
        };

        let rank = severity as u8 as f32 / MedicalSeverity::Critical as u8 as f32;
        let score =
            (rank * 0.4 + (1.0 - self.condition.health.clamp(0.0, 1.0)) * 0.3 + neglect * 0.3)
                .clamp(0.0, 1.0);
        Some(MedicalCrisis {
            severity,
            cause,
            score,
        })
    }

    /// Emergency-level or worse: hospitalization / transfer-out candidate.
    pub fn medical_transfer_advised(&self) -> bool {
        matches!(self.medical_crisis(), Some(c) if c.severity >= MedicalSeverity::Emergency)
    }

    /// Failing health: the world may open a death review (never automatic —
    /// the review itself belongs to the world/incident layer).
    pub fn death_review_advised(&self) -> bool {
        matches!(self.medical_crisis(), Some(c) if c.severity == MedicalSeverity::Critical)
    }

    /// Snapshot the departure record without mutating the prisoner (world
    /// keeps `post_release` as history if the entity is retained).
    pub fn close_out(
        &self,
        day: u32,
        pathway: ReleasePathway,
        notes: impl Into<String>,
    ) -> PostReleaseRecord {
        PostReleaseRecord {
            prisoner_id: self.id,
            name: self.name.clone(),
            age: self.age,
            day,
            pathway,
            sentence_days: self.sentence_days,
            served_days: self.sentence_served_days,
            programs_completed: self.rehab.completions,
            carried_contraband: self.contraband.items.clone(),
            behavior_score: self.reputation.clamp(0.0, 1.0),
            risk_score: self.escape_motivation().clamp(0.0, 1.0),
            notes: notes.into(),
            outcome: PostReleaseOutcome::default(),
            outcome_day: None,
        }
    }

    /// Mark the prisoner as departed: fills [`Prisoner::post_release`] so the
    /// world can copy it into history before removing the entity.
    pub fn mark_departed(
        &mut self,
        day: u32,
        pathway: ReleasePathway,
        notes: impl Into<String>,
    ) -> PostReleaseRecord {
        let record = self.close_out(day, pathway, notes);
        self.post_release = Some(record.clone());
        record
    }

    /// Update the tracked post-release outcome (reintegration / violation /
    /// return-to-custody) from outside data (Directive §15).
    pub fn set_post_release_outcome(&mut self, outcome: PostReleaseOutcome, day: u32) {
        if let Some(record) = self.post_release.as_mut() {
            record.outcome = outcome;
            record.outcome_day = Some(day);
        }
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

    // -- 0.05 lifecycle state: backward compat, hooks, triage ---------------

    #[test]
    fn pre_05_saves_without_new_fields_still_load_with_defaults() {
        let mut rng = RngStream::new(4, 4);
        let p = Prisoner::generate(PrisonerId(4), &mut rng);
        let mut value = serde_json::to_value(&p).unwrap();
        let obj = value.as_object_mut().unwrap();
        for key in [
            "rehab",
            "contraband",
            "post_release",
            "discipline_history",
            "medical_history",
        ] {
            assert!(
                obj.remove(key).is_some(),
                "field {key} missing from serialization"
            );
        }
        let back: Prisoner = serde_json::from_value(value).unwrap();
        assert!(!back.rehab.is_enrolled());
        assert!(!back.contraband.is_carrying());
        assert!(back.post_release.is_none());
        assert!(back.discipline_history.is_empty());
        assert!(back.medical_history.is_empty());
        // Untouched behavior is unchanged.
        assert_eq!(back.id, p.id);
        assert_eq!(back.schedule.wing, p.schedule.wing);
        assert!(back.sentence_remaining_days() <= back.sentence_days);
    }

    #[test]
    fn rehab_enrollment_lifecycle_tracks_progress_and_outcomes() {
        let mut rng = RngStream::new(5, 5);
        let mut p = Prisoner::generate(PrisonerId(5), &mut rng);
        assert!(!p.is_rehab_enrolled());

        assert!(p.enroll_rehab("rehab_anger_management", 5));
        assert!(
            !p.enroll_rehab("rehab_anger_management", 6),
            "must not double-enroll"
        );
        assert!(p.is_rehab_enrolled());
        assert_eq!(p.rehab_progress("rehab_anger_management"), Some(0.0));
        assert_eq!(
            p.enrolled_rehab_programs(),
            vec!["rehab_anger_management".to_string()]
        );

        p.record_rehab_session();
        assert_eq!(p.rehab.sessions_attended, 1);

        assert!(p.complete_rehab("rehab_anger_management").is_some());
        assert!(!p.is_rehab_enrolled());
        assert_eq!(p.rehab.completions, 1);
        assert!(p.rehab_progress("rehab_anger_management").is_none());

        // Dropout path (world mirrors `RehabOutcome::Dropped`).
        assert!(p.enroll_rehab("rehab_vocational", 10));
        assert!(p.drop_rehab("rehab_vocational").is_some());
        assert_eq!(p.rehab.dropouts, 1);
        assert!(!p.is_rehab_enrolled());

        // Wiring order B: `alc-social` seeds the progress key first (it runs
        // its own willingness roll), then the world records the enrollment.
        let mut seeded = Prisoner::generate(PrisonerId(52), &mut rng);
        seeded.skills.insert("rehab_counseling".into(), 0.42);
        assert!(seeded.enroll_rehab("rehab_counseling", 12));
        assert!(seeded.is_rehab_enrolled());
        assert_eq!(
            seeded.rehab_progress("rehab_counseling"),
            Some(0.42),
            "an alc-social-seeded progress value must be preserved"
        );
        // Completion reported after alc-social already removed the key.
        seeded.skills.remove("rehab_counseling");
        assert!(seeded.complete_rehab("rehab_counseling").is_some());
        assert_eq!(seeded.rehab.completions, 1);
        assert!(!seeded.is_rehab_enrolled());
        // A completion for a program that was never tracked reports nothing.
        assert!(seeded.complete_rehab("rehab_unknown").is_none());
    }

    #[test]
    fn contraband_flags_support_carry_search_and_confiscation() {
        let mut rng = RngStream::new(6, 6);
        let mut p = Prisoner::generate(PrisonerId(6), &mut rng);
        assert!(!p.contraband.is_carrying());

        p.contraband.carry("shank");
        p.contraband.carry("phone");
        p.contraband.carry("shank"); // idempotent
        assert!(p.contraband.is_carrying());
        assert!(p.contraband.has("shank"));
        assert_eq!(p.contraband.items.len(), 2);

        let taken = p.contraband.confiscate(9);
        assert_eq!(taken.len(), 2);
        assert!(!p.contraband.is_carrying());
        assert_eq!(p.contraband.offenses, 1);
        assert_eq!(p.contraband.last_confiscation_day, Some(9));
        // Clean searches do not count as offenses.
        assert!(p.contraband.confiscate(10).is_empty());
        assert_eq!(p.contraband.offenses, 1);
    }

    #[test]
    fn discipline_and_medical_histories_stay_bounded() {
        let mut p = Prisoner::default();
        for day in 0..40 {
            p.record_discipline(day, "refusal", "warning", 0.3);
        }
        assert_eq!(p.discipline_history.len(), MAX_DISCIPLINE_RECORDS);
        assert_eq!(p.discipline_history.last().unwrap().day, 39);
        assert_eq!(
            p.discipline_history[0].day,
            40 - MAX_DISCIPLINE_RECORDS as u32
        );

        for day in 0..30 {
            p.record_medical_outcome(day, MedicalSeverity::Severe, "neglect", false, "cell");
        }
        assert_eq!(p.medical_history.len(), MAX_MEDICAL_RECORDS);
        assert_eq!(p.medical_history.last().unwrap().day, 29);
        assert!(!p.medical_history[0].treated);
    }

    #[test]
    fn medical_crisis_represents_severe_neglect_and_failing_health() {
        let mut p = Prisoner::default();
        assert!(
            p.medical_crisis().is_none(),
            "healthy prisoner has no crisis"
        );
        assert!(!p.medical_transfer_advised());
        assert!(!p.death_review_advised());

        // Severe unmet need → advisory transfer/hospitalization signal.
        p.needs.medical_need = 0.95;
        let crisis = p.medical_crisis().expect("emergency must be representable");
        assert!(crisis.severity >= MedicalSeverity::Severe);
        assert!(crisis.score > 0.0 && crisis.score <= 1.0);
        assert!(p.medical_transfer_advised());
        assert!(!p.death_review_advised());

        // Failing health → death-review candidate (still advisory).
        p.needs.medical_need = 0.0;
        p.condition.health = 0.05;
        let crisis = p
            .medical_crisis()
            .expect("critical health must be representable");
        assert_eq!(crisis.severity, MedicalSeverity::Critical);
        assert!(p.death_review_advised());
        assert!(p.condition.is_fatal());
        // Hooks never mutate state by themselves.
        assert_eq!(p.condition.health, 0.05);
        assert_eq!(p.medical_history.len(), 0);
    }

    #[test]
    fn condition_severity_uses_the_shared_ladder() {
        let mut c = Condition::default();
        assert_eq!(c.severity(), MedicalSeverity::Stable);
        c.health = 0.60;
        assert_eq!(c.severity(), MedicalSeverity::Elevated);
        c.health = 0.40;
        assert_eq!(c.severity(), MedicalSeverity::Severe);
        assert!(!c.is_fatal());
        c.health = 0.15;
        assert_eq!(c.severity(), MedicalSeverity::Emergency);
        assert!(c.is_critical());
        c.health = 0.05;
        assert_eq!(c.severity(), MedicalSeverity::Critical);
        // Heavy trauma escalates even at moderate health.
        let mut t = Condition {
            health: 0.50,
            injury: 0.9,
            illness: 0.8,
            ..Condition::default()
        };
        assert!(t.severity() >= MedicalSeverity::Severe);
        t.injury = 0.0;
        t.illness = 0.0;
        assert_eq!(t.severity(), MedicalSeverity::Elevated); // health 0.50 band
    }

    #[test]
    fn departure_snapshot_records_pathway_without_losing_history() {
        let mut rng = RngStream::new(7, 7);
        let mut p = Prisoner::generate(PrisonerId(7), &mut rng);
        p.enroll_rehab("rehab_education", 2);
        p.complete_rehab("rehab_education");
        p.contraband.carry("contraband-phone");

        let record = p.mark_departed(30, ReleasePathway::Parole, "released to halfway house");
        assert_eq!(record.pathway, ReleasePathway::Parole);
        assert_eq!(record.programs_completed, 1);
        assert_eq!(
            record.carried_contraband,
            vec!["contraband-phone".to_string()]
        );
        assert_eq!(record.prisoner_id, p.id);
        assert!(p.post_release.is_some());

        // close_out is a pure snapshot (used for TransferOut/Death previews).
        let preview = p.close_out(31, ReleasePathway::TransferOut, "moved to medical unit");
        assert_eq!(preview.pathway, ReleasePathway::TransferOut);
        assert_eq!(preview.day, 31);
        assert_eq!(
            p.post_release.as_ref().unwrap().day,
            30,
            "preview must not overwrite state"
        );

        // Post-release outcome hook (world fills it from outside data).
        p.set_post_release_outcome(PostReleaseOutcome::ReturnedToCustody, 45);
        let stored = p.post_release.as_ref().unwrap();
        assert_eq!(stored.outcome, PostReleaseOutcome::ReturnedToCustody);
        assert_eq!(stored.outcome_day, Some(45));
        assert_eq!(stored.pathway, ReleasePathway::Parole);
    }
}
