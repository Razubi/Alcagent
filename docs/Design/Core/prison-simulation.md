# Prison Simulation — Core Design

**Owner role:** Prison Simulation Agent
**Scope:** Master Directive §2 (core systems), §3 (prisoners), §4 (needs), §5 (daily life & schedules), §13 (intake & population growth). Also touches §21 (time, as a consumer), §23/§18 (persistence & memory interfaces), §19 (hybrid AI), §22 (technical requirements).
**Status:** Design proposal — ready for integration review by the Architecture Agent and the other subagents.
**Rust:** All sketches target stable Rust; `serde` everywhere so state is serializable by construction.

---

## 0. Scope, ownership, and interfaces

This document defines the **simulation core for prisoners**: their data model, their needs, their daily rhythm, and how the population grows and changes. It deliberately stops at the boundary of other subsystems, but defines the contracts those subsystems must satisfy.

| Concern | This doc owns | Referenced / contracted to |
|---|---|---|
| Prisoner struct & all §3 attributes | ✅ | — |
| Needs catalog, fluctuation, need→behavior effects | ✅ | Conflict, Social, Rehab (effect consumers) |
| Schedules, attendance, movement, co-presence | ✅ | Facility/Warden (policy), Guard (staffing), UI |
| Intake generation, admissions/transfers/releases/deaths | ✅ | Warden/Economy (capacity, budget), Social (arrival impact), Parole (releases) |
| Simulation clock & tick scheduling | consumes | Architecture Agent / core `World` |
| Relationship *dynamics*, gangs, reputation math | holds IDs + local scores | Social Systems Agent |
| Memory *storage/retrieval*, LLM prompts, decisions | holds handles + structured needs | AI Agent Systems Agent |
| Facility layout, capacity, finances, policies | consumes | Warden / Economy / Architecture |
| Persistence format & autosave | ✅ for prisoner-related state; world-level coordinator | Architecture Agent (top-level save) |

**Key principle (Directive §2, §22):** no subsystem reaches into another's internals. Cross-system effects are expressed as **typed events and read-only views**. The prisoner core publishes `PrisonerEvent`s and exposes a `PrisonerView`; it consumes `WorldView`, `FacilityView`, `PolicyView`, and `StaffingView`.

I assume the Architecture Agent will provide a central `World` with a fixed-step accumulator. If not, §1.3 specifies the minimum clock contract this core needs.

---

## 1. Simulation clock & tick model (consumer contract)

### 1.1 Time types

```rust
/// Authoritative simulation time. Owned by core/World; consumed everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SimTime {
    /// Days since world epoch (day 0 = intake of first cohort).
    pub day: u32,
    /// Minutes since midnight, [0, 1440).
    pub minute_of_day: u16,
}

impl SimTime {
    pub fn total_minutes(self) -> u64 { self.day as u64 * 1440 + self.minute_of_day as u64 }
    pub fn is_between(self, start: u16, end_exclusive: u16) -> bool { /* handles wrap */ }
    pub fn weekday(self) -> Weekday { /* day % 7 */ }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Weekday { Mon, Tue, Wed, Thu, Fri, Sat, Sun }

/// A fixed simulation step. Needs use this as `dt`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TickDelta {
    pub sim_minutes: f32,
}
```

### 1.2 Tick layers (hybrid cadence, Directive §19)

Prisoner state is updated at three cadences so we never run expensive inference per minute:

```rust
pub enum PrisonerTick {
    /// Every fixed step (e.g. 1 sim-minute). Deterministic: needs, fatigue,
    /// movement progress, sentence countdown. O(N), cheap, no AI.
    Continuous(TickDelta),
    /// Every 5–15 sim-minutes. Local utility-based action selection,
    /// schedule compliance roll, co-presence encounter checks. No LLM.
    Decision(SimTime),
    /// Every sim-hour or on a triggered "significant event". Optional LLM
    /// subagent for planning / social interpretation / narrative (§19).
    Reflective(SimTime),
}
```

Deterministic ticks are the simulation's source of truth. AI subagents only ever **propose** structured actions that the engine validates (Directive §19). This core never calls an LLM directly.

### 1.3 Required clock contract

- Fixed-step accumulator; game speed only changes how many steps run per wall-clock frame.
- Monotonic `SimTime`; no backwards jumps except on load.
- A single deterministic RNG stream per world, seeded and serializable (§6), plus **derived per-prisoner RNG streams** (seeded from world seed + prisoner id) so adding/removing a prisoner never reshuffles other agents' random draws.

```rust
pub type Seed = u64;
pub fn prisoner_rng(seed: Seed, id: PrisonerId) -> StdRng { /* splitmix64(seed ^ id.0) */ }
```

---

## 2. Prisoner entity model (Directive §3)

### 2.1 Top-level aggregate

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct PrisonerId(pub u64);

#[derive(Debug, Serialize, Deserialize)]
pub struct Prisoner {
    pub id: PrisonerId,
    pub identity: Identity,
    pub criminal_background: CriminalBackground,
    pub sentence: Sentence,
    pub classification: SecurityClassification,
    pub personality: Personality,
    pub reputation: Reputation,
    pub personal_history: PersonalHistory,
    pub skills: Skills,
    pub physical: PhysicalCondition,
    pub mental: MentalCondition,

    // Dynamic / per-tick state
    pub needs: Needs,                    // §3
    pub body: BodyState,                 // current location, posture, injury
    pub schedule_state: ScheduleState,   // §4: current/next activity, compliance
    pub information: InformationModel,   // §2.15
    pub memory: MemoryHandles,           // §2.14
    pub relationships: RelationshipRefs, // §2.12
    pub gang: GangAffiliation,
    pub goals: Vec<Goal>,
    pub routine: DailyRoutine,
    pub decision: DecisionState,

    pub status: PrisonerStatus,
    pub intake: IntakeRecord,
}
```

`status` drives the lifecycle handled in §5:

```rust
pub enum PrisonerStatus {
    Active,
    PendingTransfer { destination: FacilityId, scheduled: SimTime },
    InDiscipline { until: SimTime, reason: DisciplineReason },
    MedicalHold { until: SimTime },
    Released { at: SimTime },
    Transferred { at: SimTime },
    Deceased { at: SimTime, cause: DeathCause },
    Escaped { at: SimTime },
}
```

> Only `Active`/`PendingTransfer`/`InDiscipline`/`MedicalHold` agents are stepped each tick. Terminal states stay in world history (§5.4, §15, §18) but are removed from the active population index.

### 2.2 Identity (§3: name, age, identity)

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct Identity {
    pub first_name: String,
    pub last_name: String,
    pub aliases: Vec<String>,
    pub age: u16,
    pub date_of_birth: SimDate,
    pub gender: Gender,
    pub ethnicity: String,
    pub nationality: String,
    pub height_cm: u16,
    pub weight_kg: u16,
    pub distinguishing_marks: Vec<String>,
    pub prisoner_number: String,
    pub risk_flags: Vec<RiskFlag>, // e.g. SuicideWatch, EscapeRisk, GangAffiliated
}

pub enum RiskFlag { SuicideWatch, EscapeRisk, SelfHarm, GangAffiliated, Vulnerability }
```

Identity is generated at intake (§5.1) and mostly immutable; `aliases` and `risk_flags` can grow with events.

### 2.3 Personality & trait profile (§3)

All trait values are `f32` in `[0.0, 1.0]`. We keep both the directive's behavioral traits and a Big-Five backbone (used by the AI agent systems agent for coherent prompting and by us for need sensitivity).

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Personality {
    pub traits: TraitProfile,
    pub big_five: BigFive,
    pub motivations: Vec<Motivation>,
    pub fears: Vec<Fear>,
    pub beliefs: Vec<Belief>,
    pub leadership: f32,    // derived: drives gang leadership attempts
    pub temperament: Temperament,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitProfile {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BigFive {
    pub openness: f32,
    pub conscientiousness: f32,
    pub extraversion: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
}

pub enum Motivation { Money, Power, Protection, Redemption, LoyaltyToGang, Family, Revenge, Status, Curiosity, Survival }
pub enum Fear { Violence, Isolation, Rape, Death, Betrayal, LossOfFamily, Authority, FinancialRuin }
pub enum Belief { SystemIsFair, SystemIsCorrupt, MightMakesRight, RedemptionPossible, GangAboveAll, God, Luck }
pub enum Temperament { Calm, Volatile, Brooding, Charismatic, Paranoid }
```

**Trait generation is correlated, not independent** (so prisoners feel like people, not dice rolls). Examples used by the generator (§5.1):

- violent offense ⇒ higher `aggression`, lower `empathy`;
- long sentence ⇒ lower `hope` (need), higher `impulsiveness` variance;
- high `intelligence` + low `agreeableness` + high `manipulation` ⇒ "operator" archetype;
- high `sociability` + high `loyalty` ⇒ gang-join candidate;
- high `leadership` + high `ambition` ⇒ leadership-challenge candidate;
- high `emotional_stability` + high `discipline` ⇒ routine-follower, low misconduct.

### 2.4 Criminal background (§3)

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct CriminalBackground {
    pub offenses: Vec<Offense>,          // current conviction(s)
    pub prior_convictions: u8,
    pub first_offense_age: u16,
    pub arrest_history: Vec<ArrestRecord>,
    pub criminal_role: CriminalRole,     // e.g. Organizer, Enforcer, Driver, Opportunist
    pub modus_operandi: Vec<String>,
    pub known_associates: Vec<PrisonerId>, // pre-existing inside connections
    pub victim_impact_notes: String,      // feeds parole victim restrictions (§15)
    pub substance_history: SubstanceHistory,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Offense {
    pub category: OffenseCategory,
    pub description: String,
    pub severity: f32,        // 0..1, influences sentence + classification
    pub violent: bool,
    pub against_person: bool,
    pub date: SimDate,
}

pub enum OffenseCategory {
    Violent, Property, Drug, Fraud, WhiteCollar, Sexual, Weapons,
    OrganizedCrime, PublicOrder, Cyber, Other,
}
pub enum CriminalRole { Leader, Enforcer, Specialist, Driver, Broker, Opportunist, Unknown }
```

### 2.5 Sentence & classification (§3)

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct Sentence {
    pub length_days: u32,
    pub served_days: u32,
    pub sentence_type: SentenceType,
    pub parole_eligible_after_days: Option<u32>,
    pub consecutive_with: Vec<PrisonerId>,
    pub fine_cents: u64,
}
impl Sentence {
    pub fn remaining_days(&self) -> u32 { self.length_days.saturating_sub(self.served_days) }
    pub fn progress(&self) -> f32 { /* served / length */ }
}

pub enum SentenceType { Determinate, Indeterminate, Life, LifeWithoutParole, ShortTerm, Pretrial }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SecurityClassification { Minimum, Low, Medium, High, Maximum }

/// Overlays that can coexist with a classification level.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ClassificationFlags {
    pub protective_custody: bool,
    pub administrative_segregation: bool,
    pub disciplinary_segregation: bool,
    pub medical: bool,
    pub suicide_watch: bool,
}
```

`SecurityClassification` gates schedule blocks (§4.2), work assignments, and privilege sets, and is itself mutable by the warden/discipline systems via typed commands.

### 2.6 Physical & mental condition (§3)

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct PhysicalCondition {
    pub base_health: f32,        // 0..1 constitutional health
    pub current_health: f32,     // 0..1, modified by illness/injury/medical care
    pub injuries: Vec<Injury>,
    pub illnesses: Vec<Illness>,
    pub disabilities: Vec<Disability>,
    pub addictions: Vec<Addiction>,
    pub pain_level: f32,         // 0..1, feeds irritability & sleep
    pub mobility: Mobility,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MentalCondition {
    pub disorders: Vec<MentalDisorder>,
    pub trauma_history: Vec<Trauma>,
    pub current_mood: Mood,
    pub mood_volatility: f32,    // from emotional_stability
    pub insight: f32,            // self-awareness, affects rehab uptake
}
pub enum Mood { Calm, Anxious, Depressed, Angry, Euphoric, Numb, Agitated }
pub enum Mobility { Ambulatory, Limited, Wheelchair, Bedridden }
```

Medical neglect (unmet `Medical`/`Health` needs) degrades `current_health`, raises `pain_level`, and can create chronic `Illness` entries — the long-term consequence required by Directive §8/§11.

### 2.7 Skills (§3)

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct Skills {
    /// skill -> proficiency 0..100
    pub proficiencies: HashMap<Skill, u8>,
    pub certifications: Vec<Certification>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Skill {
    Cooking, Cleaning, Electrical, Plumbing, Carpentry, Metalwork, Mechanics,
    Agriculture, ComputerLiteracy, Accounting, Negotiation, Leadership,
    Combat, Stealth, FirstAid, Teaching, Art, Music, Language(String),
}
```

Skills determine work-assignment productivity, education outcomes, trade value in the prison economy, and gang usefulness.

### 2.8 Reputation (§3)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reputation {
    pub overall: f32,                       // -1..1
    pub among_prisoners: f32,
    pub among_staff: f32,
    pub components: HashMap<ReputationFacet, f32>,
}
pub enum ReputationFacet {
    Tough, Fair, Snitch, Leader, Troublemaker, Rehabilitation, Reliable, Predator, Protector,
}
```

Reputation is a **local cached projection** of how others view this prisoner; the authoritative per-observer opinion lives in the relationship system. Social Systems Agent owns the update math; we store the aggregate for quick utility scoring and UI display.

### 2.9 Personal history (§3)

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct PersonalHistory {
    pub childhood: String,
    pub family: FamilyBackground,
    pub education_years: u8,
    pub employment_history: Vec<Employment>,
    pub relationships_outside: Vec<OutsideRelation>, // spouses, children, friends
    pub formative_events: Vec<LifeEvent>,
    pub gang_history_pre_incarceration: Option<String>,
}
pub struct FamilyBackground {
    pub stable_upbringing: bool,
    pub abuse_history: bool,
    pub incarcerated_relatives: Vec<String>,
    pub dependents: u8,
}
pub enum LifeEvent { Loss, Betrayal, Achievement, Trauma, Move, Marriage, Birth, Incident }
```

### 2.10 Personal goals (§3)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: GoalId,
    pub kind: GoalKind,
    pub priority: f32,       // 0..1
    pub progress: f32,       // 0..1
    pub target: Option<EntityRef>,
    pub deadline: Option<SimTime>,
    pub status: GoalStatus,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum GoalId(u64);
pub enum GoalKind {
    Survive, GainProtection, JoinGang, LeadGang, GetRevenge(PrisonerId),
    ReconcileFamily, EarnParole, LearnSkill(Skill), SaveMoney, Escape,
    AvoidConflict, GainStatus, ProtectSomeone(PrisonerId), StaySober,
}
pub enum GoalStatus { Active, Pending, Completed, Abandoned, Blocked }
```

Goals are both generated at intake and adopted/emerged from needs and events (e.g., `Safety` deficit → `JoinGang`; `Hope` deficit → `Escape`).

### 2.11 Daily routine (§3)

A **preference template**, not a strict script — it biases decisions and is compared against the actual `Schedule` in §4.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyRoutine {
    pub wake_preference: u16,      // minute-of-day
    pub sleep_preference: u16,
    pub prefers_early_meals: bool,
    pub workout_habits: f32,       // 0..1
    pub social_time_preference: f32,
    pub work_ethic: f32,           // from discipline/ambition
    pub religious_observance: Option<String>,
    pub favored_activities: Vec<Activity>,
    pub avoided_activities: Vec<Activity>,
}
```

Discrepancies between `routine` and the imposed `Schedule` create friction (lateness, refusal, dissatisfaction) that feeds mood and needs.

### 2.12 Relationships & gang affiliation (§3)

We store **references and a thin local cache**; the Social Systems Agent owns the `Relationship` aggregate (trust, friendship, fear, respect, loyalty, hostility, attraction, resentment, dependency, rivalry, debt, gratitude — Directive §6).

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipRefs {
    pub prisoner_opinions: HashMap<PrisonerId, f32>, // cached -1..1 summary
    pub staff_opinions: HashMap<StaffId, f32>,
    pub warden_opinion: f32,
    pub notable: Vec<NotableRelationship>,           // mentor, rival, enemy, protector
}
pub struct NotableRelationship {
    pub other: EntityRef,
    pub tags: Vec<RelationshipTag>, // Friend, Rival, Enemy, Protector, Informant, Broker, Debt
    pub strength: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GangAffiliation {
    pub gang: Option<GangId>,
    pub role: Option<GangRole>,
    pub loyalty_to_gang: f32,
    pub recruitment_pressure: f32,  // 0..1, rises with unmet Safety/Belonging
}
pub enum GangRole { Leader, Lieutenant, Member, Associate, Prospect, Former }
```

Gangs themselves are owned by the Social Systems Agent (Directive §7); this core only emits membership intent and tracks loyalty.

### 2.13 Information model (Directive §3)

Prisoners must not know things they have not observed/heard/inferred/told, and may misinterpret or lie. We model knowledge as typed claims with provenance and confidence.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InformationModel {
    /// Things directly perceived by this prisoner.
    pub direct: Vec<Observation>,
    /// Second-hand claims, tagged with source and propagation count.
    pub rumors: Vec<Rumor>,
    pub suspicions: Vec<Suspicion>,
    pub beliefs: Vec<BeliefRecord>,
    pub confirmed_facts: Vec<Fact>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub at: SimTime,
    pub kind: ObservationKind,
    pub subject: EntityRef,
    pub location: LocationId,
    pub perceived_trustworthiness: f32, // snapshot of observer state at the time
}
pub enum ObservationKind {
    SawArgue, SawFight, SawTheft, SawContraband, SawMedicalIssue,
    WasThreatened, WasHelped, WasHumiliated, HeardPlan, SawGuardBribery,
}
pub struct Rumor {
    pub claim: String,
    pub source: EntityRef,
    pub hops: u8,
    pub confidence: f32,
    pub received_at: SimTime,
}
pub struct Suspicion { pub about: EntityRef, pub claim: String, pub confidence: f32 }
pub struct Fact { pub claim: String, pub confidence: f32, pub source: EntityRef }
```

Rules: an `Observation` is corrupted into a `BeliefRecord` with probability rising with distance, poor lighting, stress, and low `intelligence`; `Rumor::hops` increments on retell and degrades `confidence`; high `manipulation` agents can intentionally inject false rumors. This model is the single source of truth for what a prisoner "knows" and is passed to the AI agent systems agent when building prompts/context (Directive §19).

### 2.14 Memory handles (Directive §18)

Layered memory is implemented by the AI Agent Systems Agent; the prisoner stores handles and a small immediate buffer.

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryHandles {
    pub short_term: VecDeque<MemoryEntry>,   // recent interactions, bounded (e.g. 64)
    pub long_term_root: MemoryId,            // external store, serialized separately
    pub last_consolidated: SimTime,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub at: SimTime,
    pub kind: MemoryKind,
    pub salience: f32,         // 0..1: determines promotion to long-term
    pub description: String,
    pub participants: Vec<EntityRef>,
}
pub enum MemoryKind {
    Interaction, Conflict, Betrayal, HelpReceived, Protection, Humiliation,
    Theft, RuleViolation, ProgramCompletion, Loss, Intake, Transfer, Narrative,
}
```

Selection rule: only entries with `salience >= threshold` (threshold depends on `emotional_stability` and repetition) are promoted to long-term memory; insignificant events decay out of `short_term`. Important events become part of identity and future behavior, as required by §18.

### 2.15 Attribute → behavior mapping

Attributes must *drive* behavior (Directive §3). This table is the contract consumed by the decision layer (§3.7, §4.4).

| Attribute | Behavioral effect | System consumer |
|---|---|---|
| `impulsiveness` ↑ | Shorter deliberation; reacts to insult immediately; higher chance of escalation | Conflict, decision cadence |
| `ambition` ↑ | Seeks gang leadership, status, program advancement | Goals, Social |
| `sociability` ↑ | Faster companionship need growth; seeks alliances; initiates interactions | Needs, encounters |
| `manipulation` ↑ | Exploits relationships, plants rumors, deceptive compliance | Information, Social |
| `loyalty` ↑ | Resists recruitment by rival gangs; honors debts | Gang, relations |
| `discipline` ↑ | Follows routine, lower refusal/skipping, better schedule compliance | Schedule §4.4 |
| `empathy` ↑ | Stronger mood impact from others' distress; more protective behavior | Mood, Social |
| `risk_tolerance` ↑ | More likely to break rules, escape, join contraband activity | Conflict, Escape |
| `emotional_stability` ↓ | Larger mood swings from unmet needs; louder reactions | Mood, needs sensitivity |
| `intelligence` ↑ | Better planning, more accurate inferences, longer time horizons | AI decisions, information |
| `aggression` ↑ | Higher conflict initiation and violence severity given provocation | Conflict |
| `fear` (motivation) | Cooperates with guards/stronger inmates; seeks protection | Social, decisions |
| `leadership` ↑ | Organizes groups, challenges established authority | Gang, events |

---

## 3. Needs engine (Directive §4)

### 3.1 Representation

Every need is stored as a **deficit pressure** in `[0.0, 1.0]`:

- `0.0` = fully satisfied / no motivation from this need.
- `1.0` = maximum deprivation / urgent.

This single convention unifies all three categories. "High hunger" = high `Hunger.level`. "Feeling unsafe" = high `Safety.level` (safety deficit). "Lonely" = high `Companionship.level`. "Angry at disrespect" = high `Respect.level`. `urgency = level`.

```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct NeedState {
    pub level: f32,            // 0..1 deficit pressure
    pub trend: f32,            // last derivative, for UI + mood
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Needs(pub HashMap<Need, NeedState>);
```

### 3.2 Need catalog (all directive needs)

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Need {
    // Physical (§4 Physical needs)
    Hunger, Thirst, Sleep, Fatigue, Hygiene, Health, Medical, Exercise,
    Temperature, Bathroom,
    // Psychological (§4 Psychological needs)
    Safety, Companionship, Respect, Freedom, Autonomy, Purpose, Stimulation,
    Hope, Stability, Privacy, Belonging,
    // Social (§4 Social needs)
    Friendship, Romance, FamilyContact, GroupAcceptance, Status, Loyalty,
    Protection, Recognition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NeedCategory { Physical, Psychological, Social }

impl Need {
    pub fn category(self) -> NeedCategory { /* ... */ }
}
```

| # | Need | Category | Rises when... | Primary relief action(s) |
|---|---|---|---|---|
| 1 | Hunger | Physical | time since last meal | Eat (meal, canteen, steal) |
| 2 | Thirst | Physical | time, heat, exertion | Drink (fountain, buy, commit theft) |
| 3 | Sleep | Physical | time awake | Sleep in cell (if safe/quiet) |
| 4 | Fatigue | Physical | work, yard, exertion, poor sleep | Rest, sit, skip activity |
| 5 | Hygiene | Physical | time since shower, dirtiness | Shower, clean cell |
| 6 | Health | Physical | injury/illness, poor conditions | Care, medication, rest |
| 7 | Medical | Physical | untreated condition | Medical appointment, sick call |
| 8 | Exercise | Physical | inactivity | Yard, workout, work detail |
| 9 | Temperature | Physical | cold/hot environment, inadequate clothing | Seek warmth/shade, clothing, HVAC |
| 10 | Bathroom | Physical | time since toilet | Use toilet (privacy gate) |
| 11 | Safety | Psychological | threats, violence nearby, segregation | Gang protection, guards, avoid areas |
| 12 | Companionship | Psychological | isolation, low sociability support | Socialize, join group |
| 13 | Respect | Psychological | insults, humiliation, status loss | Assert self, retaliate, gain status |
| 14 | Freedom | Psychological | confinement, lockdown, sentence length | (rarely satisfiable) routine, escape, visitation |
| 15 | Autonomy | Psychological | micromanagement, forced work | Choose activities, refuse, negotiate |
| 16 | Purpose | Psychological | no work/program/goal | Work, education, rehabilitation |
| 17 | Stimulation | Psychological | boredom, empty schedule | Activities, gambling, rule-breaking,Conflict |
| 18 | Hope | Psychological | long sentence, setbacks, neglect | Parole progress, rehab, family contact |
| 19 | Stability | Psychological | transfers, policy churn, regime changes | Routine, stable cell/assignment |
| 20 | Privacy | Psychological | overcrowding, searches, shared cell | Cell time, lock, complaints |
| 21 | Belonging | Psychological | exclusion, no group | Join gang/clique, family contact |
| 22 | Friendship | Social | no close ally | Build friendship, reciprocal help |
| 23 | Romance | Social | intimacy deficit (where appropriate) | Relationship with consenting inmate/visitor |
| 24 | FamilyContact | Social | time since visit/call/letter | Visits, calls, letters |
| 25 | GroupAcceptance | Social | rejection by peers/gang | Conform, prove self, gift |
| 26 | Status | Social | low reputation/rank | Gain respect, leadership, conflicts |
| 27 | Loyalty | Social | betrayal or unreciprocated loyalty | Honor debts, punish betrayal |
| 28 | Protection | Social | perceived threat | Gang membership, ally, guard protection |
| 29 | Recognition | Social | achievements unnoticed | Participate visibly, report, boast |

### 3.3 Need configuration

```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct NeedConfig {
    /// Baseline growth per sim-hour when nothing is done (0..1/h).
    pub base_rate: f32,
    /// How strongly this prisoner's personality/traits amplify growth.
    pub trait_modulator: TraitInfluence,
    /// Multiplier applied by the current activity (1.0 = neutral).
    pub activity_multiplier: ActivityMod,
    /// Multiplier applied by the environment (crowding, weather, regime).
    pub environment_multiplier: f32,
    /// Per-sim-hour natural decay when satisfied (e.g. fatigue after rest).
    pub relief_rate: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TraitInfluence {
    /// e.g. Companionship += sociability; Status += ambition;
    /// Safety/Protection += (1 - emotional_stability) and fear.
    pub trait: Option<DirectiveTrait>,
    pub direction: f32, // +1 amplifies growth, -1 dampens
    pub strength: f32,
}
pub enum DirectiveTrait { Intelligence, EmotionalStability, Impulsiveness, Aggression, Sociability, Loyalty, Manipulation, RiskTolerance, Discipline, Empathy, Ambition, Fear }

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ActivityMod {
    None,
    Work(f32), Recreation(f32), Yard(f32), Eat(f32), Sleep(f32),
    Medical(f32), Conflict(f32), Visitor(f32), Lockdown(f32),
}
```

Default rates (tunable via config; expressed per sim-hour) — representative values:

| Need | base_rate/h | Notes |
|---|---|---|
| Hunger | 0.030 | ~3 meals/day keeps it low |
| Thirst | 0.045 | faster in heat/exertion |
| Sleep | 0.025 | ~8h sleep clears it |
| Fatigue | 0.020 | `Work`/`Yard` ×2–3; `Sleep` relieves |
| Hygiene | 0.015 | `Showers` resets |
| Health | 0.010 | jumps on injury/illness events |
| Medical | 0.008 | jumps on new condition |
| Exercise | 0.018 | relieved by Yard/workout |
| Temperature | 0.010 | environment-driven |
| Bathroom | 0.040 | slow enough to be a decision |
| Safety | 0.008 | event-driven (threats) |
| Companionship | 0.012 | ×(1+sociability) |
| Respect | 0.006 | event-driven (insults) |
| Freedom | 0.010 | ×sentence progress; `Lockdown` ↑↑ |
| Autonomy | 0.008 | `Lockdown`/forced work ↑ |
| Purpose | 0.012 | relieved by work/program/goal |
| Stimulation | 0.020 | `Lockdown`/idle ↑↑ |
| Hope | 0.010 | ×(1 − progress); relieved by rehab/parole |
| Stability | 0.006 | jumps on transfer/policy change |
| Privacy | 0.010 | ×crowding; `Search` jumps |
| Belonging | 0.010 | ×(1 − group ties) |
| Friendship | 0.010 | ×(1 − close allies) |
| Romance | 0.006 | gated by age/context |
| FamilyContact | 0.012 | slow; jumps after long gap |
| GroupAcceptance | 0.008 | relevant if in gang/clique |
| Status | 0.008 | ×ambition |
| Loyalty | 0.006 | jumps on betrayal |
| Protection | 0.008 | ×(1 − Safety) until protected |
| Recognition | 0.010 | ×ambition |

### 3.4 Fluctuation logic

Every `Continuous` tick (§1.2):

```
level(t + dt) = clamp(
    level(t)
      + dt * ( base_rate
             + Σ activity_term(activity, need)
             + Σ environment_term(world, need)
             - relief_rate * relief_active(need) )
      + Σ discrete_impulses(need, events_since_last_tick),
    0.0, 1.0 )
```

- **Growth terms** are continuous and deterministic, parameterized by config.
- **Discrete impulses** are bounded jumps from events (a fight ⇒ `Health += 0.3`, `Safety += 0.25`, `Respect += 0.2` for the victim).
- **Relief** only applies when the corresponding action actually succeeds (e.g., a meal that was refused does not reduce `Hunger`).
- All needs are clamped; `trend = (new_level - old_level) / dt` for UI and mood.
- A post-update hook recomputes derived mood: e.g. if `Σ(physiological deficits) > threshold` ⇒ `Mood::Agitated`; if `Companionship + Belonging + Friendship high` ⇒ `Depressed` risk; if `Respect` spikes ⇒ `Angry`.

Rust sketch:

```rust
impl Needs {
    pub fn tick(
        &mut self,
        dt: TickDelta,
        ctx: &NeedContext<'_>,   // activity, world env, personality, events
        config: &NeedConfigs,
    ) -> NeedsTickReport {
        let hours = dt.sim_minutes / 60.0;
        let mut report = NeedsTickReport::default();
        for (need, state) in self.0.iter_mut() {
            let cfg = &config.for_need(*need);
            let trait_mult = cfg.trait_modulator.apply(&ctx.personality);
            let growth = cfg.base_rate * trait_mult
                       + cfg.activity_multiplier.value(ctx.activity, *need)
                       + cfg.environment_multiplier.value(ctx.env, *need);
            let relief = if ctx.relief_active(*need) { cfg.relief_rate } else { 0.0 };
            let impulses: f32 = ctx.events.iter()
                .map(|e| e.need_impulse(*need)).sum();
            let prev = state.level;
            state.level = (prev + hours * (growth - relief) + impulses).clamp(0.0, 1.0);
            state.trend = (state.level - prev) / hours.max(1e-6);
            if state.level >= URGENT_THRESHOLD { report.urgent.push(*need); }
        }
        report
    }
}
pub const URGENT_THRESHOLD: f32 = 0.75;
```

`ActivityMod::value` returns an additive hourly term (e.g. `Work(0.03)` adds 0.03/h to `Fatigue`, `Eat(-0.30)` gives a strong one-off relief handled as an impulse). `Environment` terms derive from `WorldView`: crowd density ⇒ `Privacy`, weather ⇒ `Temperature`, staffing ⇒ `Safety`, regime ⇒ `Freedom`/`Autonomy`/`Stability`.

### 3.5 Worked daily example (medium security, ordinary day)

| Time | Event | Need effects |
|---|---|---|
| 06:00 wake | `Sleep` relief ends | `Fatigue` begins slow rise |
| 06:30 breakfast | Eat | `Hunger` ↓ 0.5, `Thirst` ↓ 0.15 |
| 07:30 work detail | `Work` | `Fatigue` ↑↑, `Purpose` ↓, `Stimulation` ↓ |
| 10:00 yard | `Yard` | `Exercise` ↓↓, `Companionship` ↓, `Stimulation` ↓, `Safety` ↓ if threatened |
| 12:00 lunch | Eat | `Hunger` ↓↓ |
| 13:00 education | `Education` | `Purpose` ↓, `Stimulation` ↓, `Hope` ↓ (prog.) |
| 15:00 visit | `Visitor` | `FamilyContact` ↓↓, `Hope` ↓, `Belonging` ↓ |
| 17:00 dinner | Eat | `Hunger` ↓↓ |
| 19:00 recreation | `Recreation` | `Stimulation` ↓, `Friendship` ↓ |
| 21:00 lockdown | `Lockdown` | `Freedom` ↑, `Autonomy` ↑, `Privacy` ↑ (crowding) |
| 22:30 sleep | `Sleep` | `Sleep` ↓↓, `Fatigue` ↓↓ |

A disturbance at 14:00 (guard shortage → yard cancelled) leaves `Exercise`/`Stimulation` high, raising `Stimulation`-driven rule-breaking probability in the evening — an emergent consequence of a staffing decision, exactly the cross-system behavior §1/§8 demand.

### 3.6 Personality modulation

Trait influence is applied per need at generation and at tick time (examples):

- `Companionship`, `Friendship`, `Belonging` grow faster with high `sociability`.
- `Status`, `Recognition` grow faster with high `ambition`.
- `Safety`, `Protection` grow faster with low `emotional_stability` and with `Fear`.
- `Respect` spikes are more volatile with high `impulsiveness` / low `emotional_stability`.
- `Freedom`, `Autonomy` grow faster with high `ambition` and long remaining sentence.
- `Stimulation` grows faster with high `intelligence` (boredom) and low `discipline`.
- `Hope` decays faster with long remaining sentence and low `insight`; lifted by `Purpose` progress.
- `Privacy` grows faster with high `introversion` (low `sociability`).

### 3.7 Needs → decisions (Directive §4)

Needs drive decisions through a **utility score** computed in the `Decision` tick. This is deterministic, cheap, and auditable; LLM subagents may refine weights but cannot bypass validation.

```
score(action) =
      Σ_i  urgency_i * effectiveness(action, need_i)        // need relief
    + Σ_g  goal_progress(action, goal_g) * priority_g        // goals
    + personality_preference(action)                         // traits
    + impulse_bonus(action)                                  // impulsiveness
    - risk(action) * (1 - risk_tolerance) - expected_cost(action)
    + information_bias(action)                               // rumors/beliefs
```

- Only needs above a small threshold contribute, avoiding noise.
- `effectiveness` is negative for relief (reduces a deficit) and is normalized; `action` with the highest score is chosen, with stochastic tie-breaking via the prisoner's RNG.
- The **top 2–3 unmet needs** form the "motivation context" for the AI agent systems agent's prompt/context builder (Directive §19).

Required behavioral examples (Directive §4) are guaranteed by construction:

| Example | Mechanism |
|---|---|
| Hungry prisoners become irritable | `Hunger` high ⇒ mood `Agitated`, `aggression` effective ↑, patience ↓ in Conflict |
| Exhausted prisoners skip activities / less alert | `Fatigue`/`Sleep` high ⇒ `attendance_prob` ↓, alertness ↓ |
| Social isolation → depression/desperation | `Companionship`+`Friendship` high ⇒ depressed mood, desperate companionship actions, higher gang acceptance |
| Lack of safety → gangs | `Safety`/`Protection` high ⇒ `JoinGang` goal adopted, recruitment susceptibility ↑ |
| Lack of stimulation → rule-breaking | `Stimulation` high + low `discipline` ⇒ rule-violation action weight ↑ |
| Lack of hope → reduced rehab motivation | `Hope` high ⇒ rehab participation probability ↓, `Escape` goal weight ↑ |
| Respect conflicts → arguments/fights | `Respect` high + `impulsiveness`/`aggression` ⇒ conflict initiation ↑ |
| Medical neglect → illness/long-term consequences | unmet `Medical`/`Health` ⇒ `current_health` ↓, chronic `Illness` added |

Cross-system effect events published by the needs engine:

```rust
pub enum NeedsEffect {
    MoodChanged(PrisonerId, Mood),
    MotivationSpike(PrisonerId, Need),        // consumed by Goals/decisions
    RehabMotivation(PrisonerId, f32),         // consumed by Rehabilitation
    ConflictPressure(PrisonerId, f32),        // consumed by Conflict
    GangSeeking(PrisonerId, f32),             // consumed by Social/Gangs
    EscapePressure(PrisonerId, f32),          // consumed by Escape system
    MedicalNeglect(PrisonerId, f32),          // consumed by Health/Economy
}
```

### 3.8 Needs API

```rust
pub struct NeedsEngine { configs: NeedConfigs }
impl NeedsEngine {
    pub fn new(config: NeedConfigs) -> Self;
    pub fn initial_for(&self, p: &Prisoner, ctx: &WorldView) -> Needs; // intake baseline
    pub fn tick(&self, p: &mut Prisoner, dt: TickDelta, ctx: &NeedContext) -> NeedsTickReport;
    pub fn urgent_needs(&self, p: &Prisoner) -> Vec<(Need, f32)>;
}
```

---

## 4. Daily life & schedules (Directive §5)

### 4.1 Schedule data model

The clock is continuously advancing; a schedule is a **template resolved against context** into concrete activity windows each day.

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Activity {
    Wake, Breakfast, Work, Recreation, Yard, Education, Religious,
    Medical, Visits, Showers, Lunch, Dinner, Lockdown, Sleep,
    Count, FreeTime, Transport, SickCall, PhoneCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleBlock {
    pub id: BlockId,
    pub label: String,
    pub activity: Activity,
    /// minute-of-day window; end may be < start to wrap midnight.
    pub start_minute: u16,
    pub end_minute: u16,
    pub location: LocationId,
    /// Who is eligible: classification + wing + privilege + discipline gates.
    pub eligibility: Eligibility,
    /// Minimum staff needed to run the block; else it is cancelled/limited.
    pub staff_requirement: StaffRequirement,
    pub attendance: AttendancePolicy,
    /// If true, agents must physically walk to `location` (encounters happen).
    pub requires_travel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eligibility {
    pub classifications: Vec<SecurityClassification>,
    pub wings: Vec<WingId>,
    pub min_privilege_level: u8,
    pub allow_disciplinary: bool,
    pub require_program_enrollment: Option<ProgramId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaffRequirement {
    pub role: StaffRole,
    pub min_count: u8,
    pub on_shortage: ShortagePolicy, // Cancel, Shorten, MergeWithOther, SuperviseRemotely
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttendancePolicy {
    pub mandatory: bool,
    pub refusal_allowed: bool,
    pub late_tolerance_minutes: u8,
    pub on_refusal: DisciplineReason,
    pub on_unexcused_absence: DisciplineReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleTemplate {
    pub id: ScheduleId,
    pub name: String,
    pub applies_to: Eligibility,
    pub blocks: Vec<ScheduleBlock>,
    pub priority: u8, // higher overrides lower when both match
}

pub type ResolvedSchedule = Vec<ResolvedBlock>; // concrete windows for a given day/world
pub struct ResolvedBlock {
    pub block: ScheduleBlock,
    pub effective_start: SimTime,
    pub effective_end: SimTime,
    pub cancelled: bool,
    pub reason: Option<CancelReason>, // StaffShortage, Lockdown, PolicyChange, Weather
}
```

### 4.2 Variation rules (Directive §5: classification, wing, privilege, discipline, staffing, policy)

`ScheduleResolver` builds today's `ResolvedSchedule` per prisoner by:

1. Selecting all `ScheduleTemplate`s whose `applies_to` matches the prisoner (classification, wing, privilege level, disciplinary status). Highest `priority` wins for overlapping windows; otherwise blocks union.
2. Applying **world state modifiers** from `WorldView`/`FacilityView`/`PolicyView`/`StaffingView`:
   - **Staff shortage** ⇒ blocks with unmet `staff_requirement` are cancelled/shortened (`ShortagePolicy`).
   - **Lockdown** (policy or emergency) ⇒ replaces most blocks with `Lockdown`/`Sleep` windows.
   - **Weather** ⇒ yard cancelled/moved.
   - **Capacity/maintenance** ⇒ showers, medical, visits limited.
   - **Warden policy changes** (Directive §9) ⇒ privilege levels, meal times, work hours shift.
3. Writing the resolved blocks to the prisoner's `schedule_state`.

This makes schedules genuinely configurable data (TOML/RON config files), not hardcoded logic, and ensures warden decisions and staffing affect daily life.

### 4.3 Resolver & prisoner-facing state

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct ScheduleState {
    pub today: ResolvedSchedule,
    pub current_activity: Option<Activity>,
    pub next_activity: Option<Activity>,
    pub activity_started_at: Option<SimTime>,
    pub compliance: ComplianceMode,
    pub lateness_minutes: u16,
    pub skipped_blocks: Vec<BlockId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ComplianceMode { Comply, Late, Refuse, Skip, SeekMedical, Participate, BreakRule, InTransit }

pub struct ScheduleResolver<'a> {
    pub templates: &'a [ScheduleTemplate],
    pub world: &'a WorldView,
    pub staffing: &'a StaffingView,
    pub policy: &'a PolicyView,
}
impl ScheduleResolver<'_> {
    pub fn resolve_for(&self, p: &Prisoner, today: SimTime) -> ResolvedSchedule;
    pub fn current_block(&self, p: &Prisoner, now: SimTime) -> Option<&ResolvedBlock>;
}
```

When the clock crosses a block boundary, the engine emits a `BlockTransition` event and the decision layer rolls attendance (§4.4).

### 4.4 Refusal, lateness, rule-breaking (Directive §5)

At each block transition, `attendance_probability` is computed; then a single RNG draw decides compliance.

```
attendance_prob = base(attendance.mandatory)
                + discipline * w1
                - (unmet_needs_pressure) * w2
                - routine_mismatch * w3
                + privilege_incentive * w4
                + guard_presence * w5
                - group_norm_influence * w6   (peers skipping → more skipping)
```

Outcomes map to `ComplianceMode`:

| Outcome | Trigger | Consequence |
|---|---|---|
| `Comply` | normal | Attendance logged; needs relieved |
| `Late` | roll within `late_tolerance` | Minor log; repeated lateness → discipline |
| `Refuse` | `refusal_allowed` + low discipline/high need pressure/peer norm | `on_refusal` discipline reason; relationship impact with guards |
| `Skip` (work) | low `Purpose` satisfaction + low `discipline` | Discipline; `Stimulation` relief if skipped for fun |
| `Socialize` / trade / argue | during `FreeTime`/`Recreation`/transit | Emergent interaction events (§4.5) |
| `SeekMedical` | high `Medical`/`Health` need | Redirect to `SickCall`; medical system notified |
| `Participate` | program enrollment + motivation | Rehab/education progress |
| `BreakRule` | `Stimulation` high + `risk_tolerance` high + low guardianship | New `PrisonerEvent::RuleViolation` |

Consequences feed the Crime & Disciplinary system (Directive §8) and the relationship system (guard opinions drop, peer status changes). Directives explicitly require that prisoners sometimes follow, refuse, are late, skip work, socialize, trade, argue, seek medical attention, participate, and break rules — all covered.

### 4.5 Movement & emergent interactions (no teleporting)

Directive §5: schedules must create opportunities for emergent interactions rather than teleporting agents.

- A `TravelIntent { from, to, path: Vec<LocationId>, progress: f32 }` is created when an agent must reach a block's `location`. Agents traverse corridors over sim-time; `progress` advances during `Continuous` ticks.
- A **spatial index** (`HashMap<LocationId, HashSet<PrisonerId>>`, rebuilt each decision tick) tracks co-presence.
- When two or more agents co-locate (in a cell, yard, corridor, mess hall, workshop), an `EncounterOpportunity` is generated with context (activity, crowding, staff presence, mutual relationship). The AI/social layers decide whether it becomes a conversation, trade, argument, or fight.
- Dwell time varies with personality and routine, so encounters are staggered and unpredictable rather than synchronized.
- Guards also occupy locations (Guard Agent), enabling observation, enforcement, and bribery encounters.

```rust
pub struct TravelIntent {
    pub from: LocationId,
    pub to: LocationId,
    pub path: Vec<LocationId>,
    pub segment_index: usize,
    pub segment_progress: f32,
    pub started_at: SimTime,
}
pub struct EncounterOpportunity {
    pub participants: Vec<PrisonerId>,
    pub location: LocationId,
    pub activity: Activity,
    pub staff_present: Vec<StaffId>,
    pub crowd_density: f32,
    pub at: SimTime,
}
```

### 4.6 Example schedules (config files, RON/TOML)

**Default medium-security weekday (excerpt):**

| Start | End | Activity | Location | Eligibility | Staff |
|---|---|---|---|---|---|
| 06:00 | 06:30 | Wake | Cellblock | All | 1 guard |
| 06:30 | 07:15 | Breakfast | Mess Hall | All | 2 guards |
| 07:30 | 11:30 | Work | Workshops | Medium/Low, no discipline | 3 guards |
| 11:45 | 12:30 | Lunch | Mess Hall | All | 2 guards |
| 12:45 | 13:45 | Education | Education Bldg | Enrolled | 1 teacher |
| 14:00 | 15:30 | Yard | Yard | All, weather-ok | 4 guards |
| 15:45 | 16:30 | Religious | Chapel | Observant | 1 guard |
| 16:45 | 17:30 | Showers | Shower Block | All | 2 guards |
| 17:30 | 18:15 | Dinner | Mess Hall | All | 2 guards |
| 18:30 | 20:00 | Recreation | Common Area | Privilege ≥ 1 | 2 guards |
| 20:00 | 20:30 | Visits | Visitor Area | Approved visitors | 2 guards |
| 21:00 | 06:00 | Lockdown/Sleep | Cellblock | All | 2 guards night |

**Variations resolved by `ScheduleResolver`:**

- **Maximum security / disciplinary:** Work & Yard replaced by 1h controlled Recreation; more Count/Lockdown; no Visits without approval.
- **Minimum security / privileged:** later wake, self-directed work, longer yard, evening recreation extended.
- **Medical hold:** SickCall/Medical blocks inserted; work skipped.
- **Staff shortage day:** Yard cancelled (needs `4` guards, only `2` ⇒ `Cancel`), Education shortened — producing the §3.5 ripple effect.

### 4.7 Schedule API

```rust
pub struct ScheduleSystem<'a> { /* templates, resolver, spatial index */ }
impl ScheduleSystem<'_> {
    pub fn on_day_start(&mut self, world: &WorldView, today: SimTime);
    pub fn on_block_transition(&mut self, p: &mut Prisoner, block: &ResolvedBlock, ctx: &DecisionContext);
    pub fn tick_travel(&mut self, p: &mut Prisoner, dt: TickDelta);
    pub fn encounter_opportunities(&self) -> Vec<EncounterOpportunity>;
}
pub enum ScheduleEvent {
    BlockTransition(PrisonerId, BlockId),
    Attendance(PrisonerId, BlockId, ComplianceMode),
    Encounter(EncounterOpportunity),
    LocationArrived(PrisonerId, LocationId),
}
```

---

## 5. Intake & population dynamics (Directive §13)

### 5.1 Incoming prisoner generation pipeline

Generation reuses the full §2 model so every arrival is a distinct individual with unique circumstances. Inputs: world seed, current world state, facility capacity, warden policy, and a configurable `IntakeConfig` of distributions.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntakeConfig {
    pub age_distribution: Distribution,
    pub offense_weights: HashMap<OffenseCategory, f32>,
    pub sentence_length_weights: HashMap<SentenceType, f32>,
    pub classification_weights: HashMap<SecurityClassification, f32>,
    pub personality_mean: TraitProfile,
    pub personality_variance: f32,
    pub skill_weights: HashMap<Skill, f32>,
    pub health_baseline: f32,
    pub background_templates: Vec<BackgroundTemplate>,
    pub goal_weights: HashMap<GoalKind, f32>,
}

pub struct IntakeGenerator<'a> {
    config: &'a IntakeConfig,
    rng: StdRng,          // world-level stream
    names: &'a NamePool,
}

impl IntakeGenerator<'_> {
    /// Produces a complete, correlated profile.
    pub fn generate(&mut self, ctx: &IntakeContext) -> Prisoner {
        let identity = self.gen_identity(ctx);
        let criminal = self.gen_criminal_background(&identity, ctx);
        let sentence = self.gen_sentence(&criminal, ctx);
        let classification = self.gen_classification(&criminal, &sentence, ctx);
        let personality = self.gen_personality(&criminal, &sentence, ctx); // correlated
        let physical = self.gen_health(&criminal, &personality, ctx);
        let mental = self.gen_mental(&personality, &criminal, ctx);
        let skills = self.gen_skills(&criminal, &identity, ctx);
        let goals = self.gen_goals(&sentence, &personality, &classification, ctx);
        let routine = self.gen_routine(&personality, ctx);
        let connections = self.gen_potential_connections(ctx); // existing inmates known/gang ties
        // ... assemble Prisoner, seed per-prisoner RNG, compute initial Needs
        // via NeedsEngine::initial_for(ctx.world)
    }
}
```

Pipeline steps (each a method):

1. **Identity:** name, age, DOB, demographics, prisoner number, risk flags.
2. **Criminal history:** offense category/severity, priors, role, associates, substance history.
3. **Sentence & classification:** type, length, parole eligibility; classification derived from severity + priors + risk, with flags (protective custody, suicide watch).
4. **Personality:** correlated trait sampling (§2.3); Big Five derived; motivations, fears, beliefs.
5. **Health & mental:** baseline with addiction/trauma/disability links to background.
6. **Skills & background:** employment/education feed proficiencies.
7. **Relationships & goals:** pre-existing inside connections, family outside, initial goals (Survive/GetProtection/JoinGang/EarnParole...).
8. **Potential social connections:** assign probable gang alignment, rivals (enemies of existing affiliates), allies (shared background) — realized by Social Systems Agent on arrival (§5.3).
9. **Needs baseline:** seeds `Needs` from long transit, recent arrest, family separation — e.g., arriving after a long journey ⇒ high `Fatigue`/`Hunger`/`Hygiene`/`FamilyContact`.

`IntakeContext` carries `capacity`, `current_population`, `active_gangs`, `policy`, `world`, and the `IntakeSource`.

### 5.2 Population manager & transitions

```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum IntakeSource { NewAdmission, TransferIn, CourtOrder, ProgramTransfer }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntakeRecord {
    pub admitted_at: SimTime,
    pub source: IntakeSource,
    pub origin: Option<FacilityId>,
    pub intake_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PopulationEvent {
    Admission(PrisonerId, IntakeSource),
    TransferOut { id: PrisonerId, destination: FacilityId },
    Release { id: PrisonerId, reason: ReleaseReason },
    Death { id: PrisonerId, cause: DeathCause },
    StatusChange(PrisonerId, PrisonerStatus),
}
pub enum ReleaseReason { SentenceServed, Parole, CourtOrder, Deportation, Compassionate }
```

`PopulationManager` responsibilities:

- **Admissions:** pull from a pending-intake queue when `capacity > population` and warden policy allows; generate via `IntakeGenerator`; publish `Admission` and trigger social-impact hook (§5.3), economy hook (funding/expenses), and world-history entry.
- **Transfers in/out:** scheduled (`PrisonerStatus::PendingTransfer`) or event-driven (warden decision, gang violence, medical, overcrowding). Transfer out removes from active index, records destination.
- **Releases:** sentence served (`Sentence::remaining_days() == 0`) or via Parole subsystem (Directive §15). Move to `Released` status, keep in history, emit `Release` for finances/reputation; optionally start high-level post-release tracking (§15).
- **Deaths:** processed at an appropriate abstraction level (see §5.4).
- **Institutional changes:** expansion (Directive §12) raises capacity, enabling more intake; closures/capacity cuts trigger transfers/releases.

```rust
pub struct PopulationManager<'a> {
    pub pending: VecDeque<IntakeRequest>,
    pub active: HashMap<PrisonerId, Prisoner>,
    pub capacity: u32,
    pub policy: &'a PolicyView,
}
impl PopulationManager<'_> {
    pub fn daily_tick(&mut self, now: SimTime, world: &WorldView) -> Vec<PopulationEvent>;
    pub fn request_intake(&mut self, req: IntakeRequest);
    pub fn transfer_out(&mut self, id: PrisonerId, dest: FacilityId, at: SimTime);
    pub fn release(&mut self, id: PrisonerId, reason: ReleaseReason, at: SimTime);
    pub fn record_death(&mut self, id: PrisonerId, cause: DeathCause, at: SimTime);
}
```

Population caps and stability: configurable max population, per-classification ratios, and a minimum staffing ratio. The manager emits warnings if ratios are breached so the warden AI can react (Directive §9/§11).

### 5.3 Social impact of arrivals (Directive §13)

On each `Admission`, emit an `ArrivalImpact` that the Social/Gang systems consume:

```rust
pub struct ArrivalImpact {
    pub newcomer: PrisonerId,
    pub shared_background_with: Vec<PrisonerId>, // friendship seeds
    pub known_rivals: Vec<PrisonerId>,           // pre-existing enemies inside
    pub likely_gang: Option<GangId>,
    pub resource_demand_delta: f32,              // Economy: food/medical/bunk
    pub stability_risk: f32,                     // 0..1
}
```

Arrival effects required by §13 and how they emerge:

- **New rivalries** — if the newcomer's background/gang opposes an existing member, seed hostility through the relationship system.
- **Friendships** — shared ethnicity, region, offense type, or pre-existing acquaintance seed trust.
- **Challenge leadership** — high `leadership`/`ambition` newcomer raises "leadership challenge" pressure on current gang leaders.
- **Join/form groups** — `likely_gang` triggers recruitment; if no compatible group and `Belonging` high, a new clique/gang may form (Social Systems Agent).
- **Resource demand** — increases food/medical/bunk consumption (Economy hook), potentially straining capacity and altering institutional stability.
- **Institutional stability** — `stability_risk` feeds world-level unrest metrics and warden decisions.

This is pure emergence from attributes + world state; no scripted "arrival event" forces a specific outcome.

### 5.4 Deaths — appropriate abstraction

Directive §13 explicitly requires deaths "handled at an appropriate simulation abstraction," and §26 implies sensitivity. Design rule: **no graphic detail, high-level records only.**

```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum DeathCause { NaturalCauses, Illness, Accident, Violence, Other }
```

- A death is produced by simulation conditions (chronic illness from neglected `Medical`/`Health`, severe injury from Conflict, advanced age, or an abstract accident), never as a set piece.
- Effect: status ⇒ `Deceased`, remove from active index, preserve full profile + `world history` entry, notify relationships (grief, debt resolution), gangs (member lost, leadership vacuum), economy (expense change), and reputation.
- Only `cause`, `at`, and a one-line narrative are stored — no graphic description.

### 5.5 Population → other systems (events)

`PopulationManager` publishes typed events consumed by other subsystems:

- `Admission` → Social (impact §5.3), Economy (revenue/expense), UI, History.
- `Release`/`TransferOut` → Parole/World history, Economy, Social network pruning.
- `Death` → Social (grief), Gang, Economy, History.
- `StatusChange` → Schedule resolver (eligibility changes), UI.

---

## 6. Persistence & serialization plan (Directives §23, §18, §22)

### 6.1 Strategy

- Derive `Serialize`/`Deserialize` on **every** state type in this doc (`Prisoner`, `Needs`, `MemoryHandles`, `ScheduleState`, `IntakeRecord`, etc.).
- Binary encoding: **`bincode`** for the authoritative save (compact, fast, scheme-friendly), with an optional **`serde_json`** human-readable debug dump for inspection. (If the Architecture Agent prefers `postcard`/MessagePack, only the codec changes; types stay identical.)
- **Versioned schema** with an explicit header and migration path.

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct SaveHeader {
    pub magic: [u8; 4],          // e.g. *PAC"
    pub schema_version: u32,
    pub created_at: SimTime,
    pub world_seed: Seed,
    pub rng_state: SerializableRng,  // restore deterministic stream
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorldSave {
    pub header: SaveHeader,
    pub sim_time: SimTime,
    pub prisoners: HashMap<PrisonerId, Prisoner>,
    pub population: PopulationSnapshot, // capacity, pending queue, active index
    pub history: WorldHistorySnapshot,
    pub schedule_templates: Vec<ScheduleTemplate>,
    // facility, gangs, staff, finances, policies, AI goals are added by the
    // respective subsystem owners; top-level coordinator owned by Architecture Agent.
}
```

### 6.2 Determinism & RNG

- Persist the world RNG state and each prisoner's derived RNG state (seed + call count) so a loaded world continues deterministically (Directive §23 "preserve the world accurately").
- Use `rand::rngs::StdRng` with serde support (`rand` feature `serde1`) or store a raw counter-based RNG state.

### 6.3 Derived vs authoritative state

- `#[serde(skip)]` on purely derived caches that can be rebuilt on load: spatial index, `ResolvedBlock` windows (recomputed from templates + date), opinion aggregates that are projections of relationships, temporary context vectors.
- Mark them `#[serde(default)]` so old saves reconstruct them safely. **Authoritative** state (needs levels, memories, sentence, status, schedules template set, goals, information model) is always persisted.

### 6.4 Autosave & integrity (Directive §23)

- Configurable autosave interval (e.g., every in-game day or N sim-hours) and on major events (transfer, death, expansion).
- Atomic writes: serialize to temp file, `fsync`, then rename over the save — never leave a half-written save.
- Keep rotating backups (`world.autosave.1`, `.2`) and a manual-save slot.
- On load: verify magic + version; run migrations (`V1 -> V2`) sequentially; rebuild derived indices; validate invariants (no active prisoner with terminal status, sentence progress ≤ length, needs in range). Reject or repair corrupt saves with a logged warning.

```rust
pub struct SaveManager { pub dir: PathBuf, pub autosave_interval_minutes: u32, pub backups: u8 }
impl SaveManager {
    pub fn save_world(&self, world: &WorldSave, path: &Path) -> Result<(), SaveError>; // atomic
    pub fn load_world(&self, path: &Path) -> Result<WorldSave, SaveError>;            // migrate+validate
    pub fn maybe_autosave(&mut self, now: SimTime, world: &WorldSave) -> Result<bool, SaveError>;
}
```

### 6.5 What this core contributes to the save

| State | Persisted? | Notes |
|---|---|---|
| All §2 prisoner attributes | ✅ | incl. information model + goals + routine |
| `Needs` levels + trends | ✅ | authoritative; trends can be recomputed |
| Memory short-term + long-term root | ✅ | long-term store serialized by AI agent systems |
| `ScheduleState` (current activity, compliance) | ✅ | resolved blocks rebuilt from templates |
| `IntakeRecord`, pending queue, capacity | ✅ | — |
| Status & terminal records | ✅ | deceased/released preserved in history |
| Spatial index / co-presence | ❌ | derived, rebuilt on load |
| Opinion caches | ❌/✅ | rebuild from relationships (Social-owned) |

---

## 7. Testing strategy (Directive §4 Phase 4)

- **Needs tests:** monotone rise when unmet; bounded [0,1]; relief actions reduce correct need; personalities modulate rates; medical neglect degrades health; all §4 example behaviors fire from constructed states.
- **Schedule tests:** resolver honors classification/wing/privilege/discipline gates; staff shortage cancels correct block; lockdown overrides; compliance probabilities respond to discipline/needs/peers; no teleporting (travel progress advances).
- **Intake tests:** generated profiles are complete and in-range; trait correlations hold statistically; capacity gates admissions; releases/transfers/deaths update active index and history; arrival impacts reference valid prisoners.
- **Determinism tests:** same seed + same command sequence ⇒ byte-identical `WorldSave`; adding a prisoner doesn't change other prisoners' RNG draws.
- **Persistence tests:** save→load→save equality; version migration from `V1`; atomic write interruption leaves previous save intact; invariant validation rejects corrupt state.
- **Long-run simulations:** thousands of in-game days; assert no population collapse/explosion, no NaN needs, bounded memory (short-term memory capped), and that at least some emergent events (gang formation, conflicts, parole) occur.

---

## 8. Implementation roadmap & dependencies

Foundational order — do not build higher layers before their infrastructure exists (Directive Phase 2/3).

```text
A. Core kernel (Architecture Agent)
   SimTime, tick scheduler, WorldView, typed events, RNG + Seed
        │
        ▼
B. Prisoner data model (§2) ───────────────► serde derives, ids
        │                                        │
        ▼                                        ▼
C. Needs engine (§3) ◄────────────── B      Persistence core (§6: header, codec, atomic save)
        │                                        │
        ▼                                        │
D. Schedules + movement/co-presence (§4) ◄────────┘ (needs template config, world views)
        │
        ▼
E. Intake & population dynamics (§5) ──► depends on B, C, D, capacity/facility (Warden/Architecture)
        │
        ▼
F. Cross-system hooks: Social/Gang (§5.3), Conflict, Rehab, Parole, Economy, UI events
        │
        ▼
G. Autosave integration (§6.4) + full-save assembly (other agents' state plugged in)
        │
        ▼
H. Tests (§7) + long-run validation + tuning
```

| Phase | Deliverable | Depends on | Owner |
|---|---|---|---|
| 0 | Clock, ids, RNG, `WorldView`, event bus | — | Architecture |
| 1 | `Prisoner` + all §2 types, serde | 0 | Prison Sim |
| 2 | `NeedsEngine` + configs + need→behavior effects | 1 | Prison Sim |
| 3 | `ScheduleResolver`, compliance, travel, encounter gen | 0,1,2 (+staff/policy views) | Prison Sim |
| 4 | `IntakeGenerator`, `PopulationManager`, social-impact events | 1,2,3 (+capacity) | Prison Sim |
| 5 | Integration contracts with Social/Conflict/Rehab/Parole/Economy/UI | 4 | All agents |
| 6 | Save schema, migrations, autosave, atomic writes | 1–5 (+all state owners) | Architecture + Prison Sim |
| 7 | Tests, long-run sims, tuning | all | Testing + Prison Sim |

**Inputs needed from other agents (for integration):**
- Architecture: `WorldView`, `FacilityView`, `LocationId`, `StaffingView`, `PolicyView`, event bus signatures, final save coordinator.
- Social Systems: `Relationship` aggregate API, `GangId`/`GangRole`, `ArrivalImpact` consumer.
- AI Agent Systems: `MemoryId`/long-term store API, context-builder contract, structured-action schema.
- Warden/Economy: capacity, policy, staffing, and funding hooks that alter schedules/intake.
- Guard: staff presence per location (feeds schedule staffing + encounters + safety).
- UI: `PrisonerView` and schedule/needs event streams for visualization.

---

## 9. Open decisions (to resolve with Architecture Agent)

1. **Codec:** `bincode` vs `postcard` vs MessagePack — types are codec-agnostic; recommend `bincode` + JSON debug.
2. **Deterministic RNG representation:** counter-based (simplest to serialize) vs `StdRng` serde — counter-based recommended.
3. **Authoritative needs owner:** this doc assumes `NeedsEngine` in the simulation core; AI agent systems agent only reads it and proposes weight changes.
4. **Tick rate:** recommend 1 sim-minute `Continuous`, 10-min `Decision`, hourly `reflective`; confirm against performance budget for hundreds of agents.
5. **Clock ownership:** confirm §1.3 contract matches the core `World` design; if the Architecture Agent defines `SimTime` differently, this core adapts to its type.
6. **Terminal-state retention horizon:** how long released/deceased prisoners remain in active memory vs archived history (recommend: history forever, active profile in save until next archive tick).

---

*End of design — Prison Simulation Agent. Only this document was created; no existing source files were modified.*
