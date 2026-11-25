# AI Agent Systems Design

**Project:** Autonomous Living Prison Simulation
**Document:** AI agent systems — decision loops, memory, information model, orchestration, guard autonomy
**Author:** AI Agent Systems Agent
**Status:** Draft for review
**References:** `agents.md` §3, §10, §18, §19, §22, §23, §25

---

## Table of Contents

1. [Purpose and Scope](#1-purpose-and-scope)
2. [Design Principles](#2-design-principles)
3. [Architecture Overview](#3-architecture-overview)
4. [Information Model](#4-information-model)
5. [Memory Systems](#5-memory-systems)
6. [Decision-Making Loop](#6-decision-making-loop)
7. [Prompt Construction](#7-prompt-construction)
8. [Decision Output and Validation](#8-decision-output-and-validation)
9. [AI Orchestration](#9-ai-orchestration)
10. [Guard Autonomy](#10-guard-autonomy)
11. [Warden Coordination](#11-warden-coordination)
12. [Module Specifications and Interfaces](#12-module-specifications-and-interfaces)
13. [Configuration](#13-configuration)
14. [Persistence](#14-persistence)
15. [Observability and Logging](#15-observability-and-logging)
16. [Testing and Verification](#16-testing-and-verification)
17. [Risks and Mitigations](#17-risks-and-mitigations)
18. [Open Questions](#18-open-questions)
19. [Appendix A — Decision JSON Schema](#appendix-a--decision-json-schema)
20. [Appendix B — Prompt Template Skeleton](#appendix-b--prompt-template-skeleton)
21. [Appendix C — Glossary](#appendix-c--glossary)

---

## 1. Purpose and Scope

This document specifies the autonomous AI agent systems for the prison simulation:

- **Prisoner decision loop** — observe → evaluate needs → review memories → assess people → consider actions → select → execute → observe consequences → update memories and beliefs.
- **Information model** — direct knowledge, rumors, suspicion, beliefs, confirmed facts.
- **Memory systems** — short-term, long-term, and institutional memory; selectivity; importance scoring.
- **AI orchestration** — hybrid deterministic/AI decision model, periodic updates, prompts, rate limiting, token budgeting, caching, error handling, logging.
- **Guard autonomy** — guard decision loop, reporting to the warden, assistance requests, bounded authority.

The design is language-agnostic at the conceptual level but uses Rust interface sketches consistent with `agents.md` §22. It does not specify UI, physics, map representation, or economy internals; it defines the interfaces those systems must expose to the AI layer.

### Non-goals

- Defining the exact simulation tick rate or world geometry (owned by the world simulation).
- Defining LLM provider SDK implementations (only the gateway abstraction is specified).
- Defining the UI's rendering of agent state (the UI consumes the logging/telemetry and snapshot APIs defined here).

---

## 2. Design Principles

1. **Agents propose, the engine disposes.** Agents never mutate arbitrary simulation state. They emit structured decisions; the simulation engine validates and applies them.
2. **Hybrid by frequency and cost.** Routine needs, movement, schedules, and resource consumption are deterministic. Frequent choices use deterministic utility scoring. Expensive AI deliberation is reserved for complex social, strategic, and crisis decisions, run periodically.
3. **Information is imperfect.** Agents only know what they observed, heard, inferred, or were told. Beliefs have confidence, provenance, and decay. Lies, rumors, and misinterpretation are first-class.
4. **Memory is selective and identity-forming.** Insignificant events are forgotten; important events become part of personality, reputation, and future decisions.
5. **Bounded autonomy.** Guards act independently within policy and authority; they cannot change policy, sentence prisoners, or access arbitrary state. The warden receives reports rather than omniscient awareness.
6. **Degradation, not collapse.** Provider errors, timeouts, malformed output, or exhausted budgets fall back to deterministic behavior. The simulation never halts because an AI call failed.
7. **Observable and auditable.** Every AI request, token, cost, validation result, and fallback is logged and attributable to an agent and simulation time.
8. **Configurable and testable.** Model selection, budgets, schedules, and thresholds are configuration. Deterministic mock providers support reproducible tests.

---

## 3. Architecture Overview

### 3.1 Layered View

```
+====================================================================+
|                     SIMULATION ENGINE (authoritative)               |
|  World state, tick loop, command application, event bus, outcomes    |
+----------------------------------+---------------------------------+
                                   |  WorldView / Command / Outcome
+----------------------------------v---------------------------------+
|                     AI ORCHESTRATION LAYER                         |
|  AgentRegistry · DecisionScheduler · PromptBuilder · ContextBuilder|
|  LlmGateway · RateLimiter · TokenBudget · ResponseCache             |
|  ActionValidator · DecisionRepair · Telemetry                       |
+----------------------------------+---------------------------------+
                                   |  LlmRequest / LlmResponse
+----------------------------------v---------------------------------+
|                  MODEL PROVIDERS (configurable)                     |
|  OpenAI-compatible · Anthropic · local · deterministic mock         |
+====================================================================+

Agent domains built on the shared framework:
  PrisonerAgent · GuardAgent · WardenAgent
```

### 3.2 Module Map

| Module | Responsibility |
|---|---|
| `ai::info` | Beliefs, knowledge, rumors, suspicion, facts, person models, rumor propagation |
| `ai::memory` | Layered memory stores, importance scoring, retrieval, consolidation, forgetting, institutional memory |
| `ai::decision` | Decision loop stages, needs/goals evaluation, action candidates, decision types |
| `ai::prompt` | Context construction, prompt templates, token budgeting, prompt registry |
| `ai::validation` | Decision schema/semantic/authority/world-state validation, repair, rejection |
| `ai::orchestrator` | Registry, scheduling, async decision lifecycle, budget, rate limiting, cache, retries, telemetry |
| `ai::llm` | Provider-agnostic LLM client abstraction, model catalog, request/response types |
| `ai::agents::prisoner` | Prisoner-specific profile, goals, needs mapping, action catalog |
| `ai::agents::guard` | Guard profile, duty loop, incident reports, assistance requests |
| `ai::agents::warden` | Warden profile, institutional objectives, reports inbox, orders |

### 3.3 Ownership and Dependencies

- `ai::llm` depends on nothing in the simulation.
- `ai::info`, `ai::memory` depend on simulation time and entity IDs only.
- `ai::decision` depends on `ai::info`, `ai::memory`, and a read-only `WorldView`.
- `ai::prompt` depends on `ai::decision` context types.
- `ai::validation` depends on the simulation engine's command and authority interfaces.
- `ai::orchestrator` depends on all of the above and drives the loop.
- Agent modules depend on the orchestrator, not on each other directly.

---

## 4. Information Model

### 4.1 Goals

Implement `agents.md` §3: "Prisoners should not know information they have not personally observed, heard, inferred, or been told." The information model separates:

- **Direct knowledge** — first-hand observation.
- **Rumors** — second-hand claims, unverified, subject to distortion.
- **Suspicion** — inference from ambiguous or weak evidence.
- **Beliefs** — subjective interpretations that may be false and are shaped by personality.
- **Confirmed facts** — corroborated by independent sources or authoritative records.

The model must support contradiction, confidence decay, confirmation bias, deception, and incomplete information.

### 4.2 Core Types

```rust
pub type BeliefId = u64;
pub type AgentId = u64;
pub type EntityId = u64;
pub type RecordId = u64;

/// The epistemic category of a piece of information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InformationKind {
    /// First-hand observation by this agent.
    DirectKnowledge,
    /// Second-hand claim from another agent, unverified.
    Rumor,
    /// Inference from weak, ambiguous, or incomplete evidence.
    Suspicion,
    /// Subjective interpretation; may be wrong; shaped by personality.
    Belief,
    /// Corroborated by independent sources or an authoritative record.
    ConfirmedFact,
}

/// Where the information came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Source {
    Observation {
        observer: AgentId,
        at: SimTime,
        location: LocationId,
        /// 0..1, affected by distance, lighting, attention, fatigue.
        clarity: f32,
    },
    Hearsay {
        speaker: AgentId,
        at: SimTime,
        /// How reliable the listener believes the speaker to be.
        speaker_reliability: f32,
        /// How many hops from the original claim.
        hops: u8,
    },
    Inference {
        premises: Vec<BeliefId>,
        rule: InferenceRule,
    },
    Institutional {
        record: RecordId,
        authority: AuthorityLevel,
    },
    Testimony {
        witness: AgentId,
        /// 0..1 credibility of the witness.
        credibility: f32,
    },
}

/// A subject-predicate claim about an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Belief {
    pub id: BeliefId,
    pub subject: EntityRef,
    pub predicate: Predicate,
    /// Categorical or continuous value.
    pub value: BeliefValue,
    /// 0..1 subjective confidence.
    pub confidence: f32,
    pub kind: InformationKind,
    pub source: Source,
    pub created: SimTime,
    pub updated: SimTime,
    pub corroborations: u32,
    pub contradictions: Vec<BeliefId>,
    /// 0..1, how much this belief matters to the agent.
    pub salience: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Predicate {
    // Social / dispositional
    IsHostile,
    IsFriendly,
    IsInformant,
    IsUnreliable,
    IsDangerous,
    IsVulnerable,
    IsLeader,
    OwesMoney,
    OwesFavor,
    IsInGang(GangId),
    // Material / situational
    HasContraband,
    HasWeapon,
    PlansEscape,
    IsPlanningSomething,
    IsInvolvedInIncident(EventId),
    IsAtLocation(LocationId),
    // Institutional
    PolicyIs(PolicyId),
    WardenIs(Disposition),
    StaffShortage,
    // World
    LocationIs(LocationId, LocationProperty),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BeliefValue {
    Bool(bool),
    Scale(f32),              // e.g. hostility 0..1
    Categorical(String),
    Entity(EntityRef),
    Count(u32),
}
```

### 4.3 Belief Lifecycle

**Ingestion.** Perceptions are converted to candidate beliefs by `BeliefExtractor`. Each candidate is checked against existing beliefs for the same `(subject, predicate)`:

- **Consistent evidence:** confidence increases:
  `c' = c + (1 - c) * reliability * clarity * corroboration_factor`
  where `reliability` depends on source type and the observer's trust in the speaker, and `corroboration_factor` increases with independent corroborations.
- **Contradictory evidence:** a contradiction record is added. Resolution is by source reliability, recency, number of corroborations, and the agent's `confirmation_bias` trait. Confirmation bias increases the weight of evidence consistent with existing beliefs and can suppress contradiction resolution.
- **New claim:** stored with initial confidence derived from source reliability and clarity.

**Promotion.** A belief can be promoted to `ConfirmedFact` when:
- it has ≥ 2 independent corroborating sources (different `Source::Observation` observers or an authoritative `Institutional` record), or
- an authoritative institutional record directly confirms it.

Promotion is not irreversible; authoritative records can be revealed as wrong by later high-reliability evidence (e.g., an investigation exonerating a prisoner).

**Demotion and decay.** Confidence decays over time since last corroboration:
`c' = c * (1 - decay_rate * dt) + baseline * dt`, clamped to `[0, 1]`.
Rumors decay faster than direct knowledge; beliefs tied to strong goals decay slower. Confirmed facts decay only when contradicted by reliable evidence.

**Forgetting.** Beliefs below `forget_threshold` for longer than `forget_after` are removed from active store and may be archived in long-term memory as weak impressions.

### 4.4 Rumor Propagation

Rumors spread through conversations and are deliberately used for deception.

```rust
pub struct RumorPropagation {
    /// Base distortion applied per hop.
    pub distortion: f32,
    /// Probability the listener believes the claim.
    pub acceptance_bias: f32,
    /// Probability a lie is introduced.
    pub deception_probability: f32,
    /// Probability the speaker exaggerates or omits detail.
    pub embellishment_probability: f32,
}
```

When agent A tells agent B a belief:

1. Determine whether A lies (`manipulation`, `deception_probability`, relationship, motive).
2. If lying, invert or alter the predicate/value; tag the resulting belief as `Rumor` with `speaker_reliability` reduced for a known liar.
3. Otherwise, apply distortion: shift continuous values by noise scaled by `distortion * hops`, drop details, or change salience.
4. The listener accepts or rejects based on `acceptance_bias`, trust in A, and prior beliefs. Rejected rumors may still be stored as low-confidence suspicions.
5. `hops` increments; confidence is multiplied by a per-hop attenuation factor.

Rumors are a primary driver of gang conflict, informant paranoia, and prison politics.

### 4.5 Person Models

Agents maintain a `PersonModel` per known individual, aggregating beliefs about that person:

```rust
pub struct PersonModel {
    pub entity: AgentId,
    /// Estimated traits (never exact; inferred from behavior).
    pub traits: TraitEstimate,
    pub relationship: RelationshipVector,
    /// Aggregated trust in this person.
    pub trust: f32,
    /// Perceived threat.
    pub threat: f32,
    /// Perceived power/influence.
    pub power: f32,
    /// Perceived reliability as an informant.
    pub reliability: f32,
    pub last_seen: SimTime,
    pub last_seen_location: Option<LocationId>,
    pub known_gang: Option<GangId>,
    pub notes: Vec<BeliefId>,
    /// 0..1, how much the agent likes/dislikes them.
    pub affect: f32,
}
```

Person models are the primary input to "assess nearby people" and to social decisions. They are deliberately imperfect and can be manipulated by others.

### 4.6 Query API

```rust
pub trait BeliefStore {
    fn ingest(&mut self, candidate: CandidateBelief) -> IngestionResult;
    fn get(&self, subject: &EntityRef, predicate: &Predicate) -> Option<&Belief>;
    fn query(&self, filter: &BeliefFilter) -> Vec<&Belief>;
    fn contradiction_groups(&self) -> Vec<Vec<BeliefId>>;
    fn decay(&mut self, now: SimTime);
    fn promote_confirmed(&mut self, belief: BeliefId);
    fn person_model(&self, entity: AgentId) -> Option<&PersonModel>;
    fn update_person_model(&mut self, model: PersonModel);
}
```

`BeliefFilter` supports predicates, minimum confidence, kind, recency, subject type, and salience, enabling the context builder to retrieve "what does this prisoner believe about the people in the room?"

---

## 5. Memory Systems

### 5.1 Layered Design

| Layer | Capacity | Lifetime | Content | Write policy |
|---|---|---|---|---|
| **Sensory buffer** | ~10–30 items | Seconds | Raw perceptual items, sounds, glimpses | Automatic, overwritten continuously |
| **Short-term memory** | 7 ± 2 chunks (configurable) | Minutes to hours | Recent interactions, immediate context, current conversation | Automatic for salient items; overwritten by decay/eviction |
| **Long-term memory** | Bounded (configurable, e.g. 500–5000 records) | Days to years | Important relationships, events, lessons, goals, grudges | Selective via importance scoring and consolidation |
| **Institutional memory** | Effectively unbounded, pruned by relevance | Permanent world history | Major events, policy changes, public records, legends | Written by the world/history system; read access varies by role |

All layers are per-agent except institutional memory, which is shared and access-controlled.

### 5.2 Memory Record

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryLayer { Sensory, ShortTerm, LongTerm, Institutional }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryKind {
    Episodic,    // a specific event
    Semantic,    // a general fact about the world/people
    Procedural,  // a learned lesson ("don't ...")
    Social,      // relationship facts, debts, favors, grudges
    Emotional,   // emotionally charged impressions
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: MemoryId,
    pub layer: MemoryLayer,
    pub kind: MemoryKind,
    /// Compact natural-language summary used in prompts.
    pub summary: String,
    /// Optional embedding for semantic retrieval.
    pub embedding: Option<Vec<f32>>,
    pub tags: Vec<MemoryTag>,
    /// 0..1 importance score.
    pub importance: f32,
    /// -1..1 emotional valence.
    pub emotional_valence: f32,
    /// 0..1 emotional intensity (arousal).
    pub emotional_intensity: f32,
    pub created: SimTime,
    pub last_accessed: SimTime,
    pub access_count: u32,
    pub rehearsal_count: u32,
    pub decay_rate: f32,
    pub source_event: Option<EventId>,
    pub participants: Vec<AgentId>,
    pub location: Option<LocationId>,
    pub facts: Vec<Fact>,
    /// True if this memory has been consolidated into long-term.
    pub consolidated: bool,
}
```

### 5.3 Importance Scoring

Importance determines whether an event is promoted to long-term memory and how slowly it decays.

```
importance =
      w_goal   * goal_relevance       // 0.30
    + w_emotion* emotional_intensity  // 0.20
    + w_social * social_significance  // 0.15
    + w_conseq* consequence_magnitude // 0.15
    + w_novel * novelty               // 0.10
    + w_self  * self_relevance        // 0.05
    + w_rep   * repetition_bonus      // 0.05
```

All inputs are normalized to `[0, 1]` and clamped. Weights are configurable and can be modulated by personality (e.g., high-empathy agents weight social significance more; paranoid agents weight threat-related novelty more).

**Component definitions:**

- `goal_relevance`: max relevance of the event to the agent's active goals (survival, revenge, escape, status, wealth, loyalty, etc.).
- `emotional_intensity`: peak emotional arousal during the event; fights, betrayals, humiliations, and reunions score high.
- `social_significance`: involves friends, enemies, leaders, or relationship change; public witnessed events score higher.
- `consequence_magnitude`: impact on health, freedom, resources, reputation, sentence, or gang standing.
- `novelty`: first occurrence of this kind of event; repeated similar events reduce novelty but increase `repetition_bonus`.
- `self_relevance`: whether the agent was the actor, victim, or a direct witness.
- `repetition_bonus`: grows with the number of similar memories, up to a cap, reflecting pattern recognition.

**Thresholds and caps:**

- `long_term_threshold`: events scoring above this are written to long-term memory immediately or at consolidation.
- `short_term_threshold`: events below this are held only briefly and forgotten.
- Traumatic or identity-defining events can bypass the threshold and are stored permanently.
- Long-term store has a maximum size; eviction removes the lowest `importance * rehearsal` records first, optionally archiving them as semantic summaries.

### 5.4 Selectivity Rules

Memory must be selective (`agents.md` §18: "Not every insignificant event should be permanently recorded"). The rules:

**Always store (long-term):**
- Direct physical harm to or by the agent.
- Betrayal, loyalty tests, favors given/received, humiliations.
- Major goal progress or failure (parole denial, escape discovery, gang promotion).
- Deaths, serious injuries, riots, lockdowns witnessed first-hand.
- Policy changes that directly affect the agent.
- First meeting with a significant person (leader, enemy, ally).

**Store if importance ≥ `long_term_threshold`:**
- Notable arguments, threats, alliances, trades, losses, victories.
- New information about a friend/enemy's nature or plans.
- Lessons learned (procedural memory).

**Store only in short-term (then forget):**
- Routine movement, meals, showers, sleep.
- Mundane small talk with no relational consequence.
- Passing glimpses of strangers.

**Never store:**
- Raw sensor data after it has been interpreted.
- Duplicate events already represented by an existing memory (merge instead).

**Deduplication/merging:**
- If a new event matches an existing memory by participants, predicate, location, and time window, merge: increase `rehearsal_count`, update `importance` and `emotional_intensity`, and refresh `last_accessed`.
- Repeated similar events may generalize into a semantic memory ("X often bullies people in the yard").

### 5.5 Retrieval

Retrieval is query-driven and budget-bounded.

```rust
pub struct MemoryQuery {
    pub context_tags: Vec<MemoryTag>,
    pub subject: Option<EntityRef>,
    pub predicate: Option<Predicate>,
    pub location: Option<LocationId>,
    pub since: Option<SimTime>,
    pub min_importance: f32,
    pub top_k: usize,
    pub max_tokens: u32,
}

pub trait MemoryStore {
    fn store(&mut self, record: MemoryRecord) -> MemoryId;
    fn retrieve(&self, query: &MemoryQuery) -> Vec<RankedMemory>;
    fn consolidate(&mut self, now: SimTime);
    fn decay(&mut self, now: SimTime);
    fn forget(&mut self, predicate: impl Fn(&MemoryRecord) -> bool);
}
```

Ranking score:

```
retrieval_score =
      0.35 * semantic_similarity
    + 0.25 * recency
    + 0.20 * importance
    + 0.10 * emotional_intensity
    + 0.10 * goal_relevance
```

Additional behaviors:
- **Spacing/rehearsal:** repeatedly retrieved memories are strengthened (`rehearsal_count`, slower decay).
- **Intrusive memories:** high-emotional-intensity memories have a small probability of surfacing unprompted, especially under stress or fatigue.
- **Contextual priming:** being in a location or with a person associated with a memory raises its retrieval score.
- **Budget enforcement:** retrieval stops when `max_tokens` is reached; lower-ranked memories are dropped, not truncated mid-record.

### 5.6 Consolidation and Forgetting

Consolidation runs periodically (e.g., at sleep/lockdown or every `consolidation_interval`):

1. Promote short-term records above `long_term_threshold` into long-term memory.
2. Merge duplicate/related records.
3. Generalize repeated patterns into semantic/procedural memories.
4. Apply decay to all records:
   `importance_effective = importance * 0.5 ^ (age / half_life) * (1 + rehearsal_bonus)`
   Traumatic memories use a much longer half-life.
5. Evict or archive records below `forget_threshold`.
6. Update person models from new memories.

Forgetting is desirable: it prevents unbounded growth, creates incomplete knowledge, and makes rumors and grudges fade at different rates depending on importance and rehearsal.

### 5.7 Institutional Memory

Institutional memory is the prison's persistent history (`agents.md` §18). It is not an agent memory; it is a shared, access-controlled store.

```rust
pub struct InstitutionalMemory {
    pub events: Vec<WorldEventRecord>,
    pub policies: Vec<PolicyRecord>,
    pub finances: Vec<FinancialRecord>,
    pub constructions: Vec<ConstructionRecord>,
    pub releases_paroles: Vec<ReleaseRecord>,
    pub escapes: Vec<EscapeRecord>,
    pub investigations: Vec<InvestigationRecord>,
}

pub trait InstitutionalMemoryStore {
    fn query(&self, query: &InstitutionalQuery, access: AccessLevel) -> Vec<InstitutionalRecord>;
    fn public_bulletin(&self, since: SimTime) -> Vec<InstitutionalRecord>;
}
```

Access levels:
- **Public:** bulletins, policy changes, official counts.
- **Prisoner:** public information plus rumors and hearsay; prisoners cannot query arbitrary records.
- **Guard:** incident reports, assignment records, prisoner disciplinary summaries for their post.
- **Warden:** broad institutional records, finances, investigations; still not omniscient about private prisoner thoughts or unobserved events.

Agents can form beliefs from institutional records; those beliefs are tagged with `Source::Institutional` and generally higher reliability, but can still be misinterpreted.

### 5.8 Memory Budget and Persistence

- Each agent has a `MemoryConfig` with layer capacities and thresholds.
- Memories are persisted with the agent (see §14).
- Memory size is monitored; if an agent exceeds its long-term cap, consolidation runs early and low-value records are archived.
- For performance, embeddings are optional and can be computed lazily; tag-based retrieval is the baseline.

---

## 6. Decision-Making Loop

### 6.1 Loop Stages

The canonical loop (`agents.md` §3) is implemented as nine stages:

```
  +----------------+
  | 1. OBSERVE     |  perception filtered by senses, attention, fatigue
  +----------------+
           |
  +----------------+
  | 2. EVALUATE    |  needs pressure, emotional state
  |    NEEDS       |
  +----------------+
           |
  +----------------+
  | 3. REVIEW      |  retrieve relevant memories
  |    MEMORIES    |
  +----------------+
           |
  +----------------+
  | 4. ASSESS      |  person models, threats, opportunities
  |    PEOPLE      |
  +----------------+
           |
  +----------------+
  | 5. CONSIDER    |  action candidates from catalog + goals
  |    ACTIONS     |
  +----------------+
           |
  +----------------+
  | 6. SELECT      |  deterministic utility OR AI deliberation
  +----------------+
           |
  +----------------+
  | 7. EXECUTE     |  validated command enqueued and applied by engine
  +----------------+
           |
  +----------------+
  | 8. OBSERVE     |  outcome event, consequences
  |    CONSEQUENCES|
  +----------------+
           |
  +----------------+
  | 9. UPDATE      |  memory, beliefs, relationships, emotions
  +----------------+
           |
           +----> back to 1
```

Stages 1–5 and 8–9 are deterministic and always run. Stage 6 is hybrid: deterministic for routine decisions, AI for complex ones.

### 6.2 Data Structures

```rust
pub struct Perception {
    pub agent: AgentId,
    pub at: SimTime,
    pub location: LocationId,
    pub visible_entities: Vec<ObservedEntity>,
    pub visible_objects: Vec<ObservedObject>,
    pub sounds: Vec<SoundCue>,
    pub events: Vec<WorldEvent>,
    pub schedule: ScheduleContext,
    pub attention_focus: Option<EntityRef>,
    /// 0..1 perceptual reliability, reduced by fatigue/stress.
    pub clarity: f32,
}

pub struct NeedState {
    pub physical: HashMap<NeedId, f32>,
    pub psychological: HashMap<NeedId, f32>,
    pub social: HashMap<NeedId, f32>,
    pub dominant_need: NeedId,
    /// 0..1 overall urgency.
    pub urgency: f32,
}

pub struct EmotionalState {
    pub valence: f32,       // -1..1
    pub arousal: f32,       // 0..1
    pub dominant_emotion: Emotion,
    pub stress: f32,
}

pub struct GoalState {
    pub goal: Goal,
    pub priority: f32,
    pub progress: f32,
    pub deadline: Option<SimTime>,
    pub blocked: bool,
}

pub struct DecisionContext {
    pub agent: AgentSnapshot,
    pub perception: Perception,
    pub needs: NeedState,
    pub emotions: EmotionalState,
    pub memories: Vec<RankedMemory>,
    pub people: Vec<PersonModel>,
    pub action_candidates: Vec<ActionCandidate>,
    pub goals: Vec<GoalState>,
    pub institutional: InstitutionalContext,
    pub constraints: ActionConstraints,
    pub prompt_budget: PromptBudget,
}

pub struct ActionCandidate {
    pub action: ActionId,
    pub target: Option<EntityRef>,
    pub parameters: Parameters,
    pub preconditions: Vec<Condition>,
    /// Deterministic utility estimate, used for ranking and fallback.
    pub expected_utility: f32,
    pub risk: f32,
    pub legality: Legality,
    pub authority_required: AuthorityLevel,
    pub cost: ResourceCost,
    pub duration: SimDuration,
}
```

### 6.3 Deterministic vs AI Selection

The decision engine selects between two paths:

| Decision tier | Trigger | Engine | Frequency |
|---|---|---|---|
| **Tier 0 — Deterministic simulation** | Needs decay, movement, schedule progression, resource consumption | Simulation engine | Every tick |
| **Tier 1 — Utility choice** | Routine choice among similar candidates (what to eat, where to walk, whom to greet) | Deterministic utility scorer | Every few ticks or on state change |
| **Tier 2 — AI deliberation** | Complex social, strategic, crisis, or novel decisions | AI model via orchestrator | Periodic + event triggers |

Selection of tier for stage 6:

```
if urgent_event && ai_budget_available && agent.ai_enabled:
    tier = AI_DELIBERATION
elif decision_is_routine(candidates) && utility_variance < threshold:
    tier = UTILITY
else:
    tier = AI_DELIBERATION (if due) else UTILITY
```

The AI never sees a blank slate: it receives the already-built `DecisionContext`, including deterministic candidate list, needs, memories, and people. It chooses among candidates and may set parameters within validated ranges. It cannot invent actions outside the catalog or mutate state.

### 6.4 Goals and Needs

- **Needs** are computed by the deterministic needs system and passed into the context. They are not re-derived by the AI, though the AI can weight them.
- **Goals** are persistent, personality-derived objectives (survival, dominance, freedom, loyalty, wealth, revenge, rehabilitation). Goals are proposed by AI at creation or during major reflections and validated/selected by the deterministic goal system. Goals have priorities and progress.
- The AI's job is to choose actions that serve needs and goals under current beliefs and constraints.

### 6.5 Action Catalog

Actions are defined by the simulation, not by the model:

```rust
pub struct ActionDefinition {
    pub id: ActionId,
    pub name: &'static str,
    pub category: ActionCategory,
    pub parameter_schema: ParameterSchema,
    pub preconditions: Vec<Condition>,
    pub authority_required: AuthorityLevel,
    pub legality: Legality,
    pub cost: ResourceCost,
    pub cooldown: SimDuration,
    pub risk_model: RiskModel,
}
```

Examples: `move_to`, `talk`, `trade`, `threaten`, `attack`, `flee`, `report`, `request_assistance`, `search`, `escort`, `enforce_lockdown`, `assign_post`, `change_policy`, `request_parole`, `participate_program`, `recruit`, `extort`, `steal`, `hide_contraband`, `plan_escape`, `meditate`, `seek_medical`.

Candidates are generated deterministically from the catalog and current world state. The AI may choose a candidate and fill parameters; it may also propose a **compound intent** (a plan) that decomposes into validated sub-actions over time.

### 6.6 Outcome Application

The engine applies a validated command and produces an `Outcome`:

```rust
pub struct Outcome {
    pub decision_id: DecisionId,
    pub agent: AgentId,
    pub action: ActionId,
    pub success: bool,
    pub success_probability: f32,
    pub consequences: Vec<Consequence>,
    pub events: Vec<WorldEvent>,
    pub observed_by: Vec<AgentId>,
    pub at: SimTime,
}
```

Consequences include relationship changes, injuries, reputation shifts, disciplinary actions, resource changes, and emotional impacts. Observers generate their own perceptions and update their own memories and beliefs.

---

## 7. Prompt Construction

### 7.1 Prompt Anatomy

Prompts are assembled from a system template and a context builder. The system prompt is stable and cache-friendly; the context varies per decision.

```
+------------------------------------------------------------------+
| SYSTEM PROMPT (stable, cached)                                  |
|  - role and identity summary                                     |
|  - personality and behavioral constraints                        |
|  - information-model rules (what can be known)                   |
|  - action catalog (compressed)                                   |
|  - output schema and validation rules                            |
|  - in-world behavioral guardrails                                |
+------------------------------------------------------------------+
| CONTEXT (dynamic, budgeted)                                      |
|  1. Identity and current goals                                   |
|  2. Needs and emotional state                                    |
|  3. Location and schedule                                        |
|  4. Perception summary                                           |
|  5. Relevant people                                             |
|  6. Relevant memories                                           |
|  7. Institutional context                                        |
|  8. Available actions and constraints                             |
+------------------------------------------------------------------+
| TASK                                                              |
|  - instruction to select exactly one decision                    |
|  - JSON output schema                                            |
|  - self-check instructions                                        |
+------------------------------------------------------------------+
```

### 7.2 Context Builder

```rust
pub struct ContextBuilder {
    pub sections: Vec<SectionBuilder>,
    pub budget: PromptBudget,
    pub prompt_version: PromptVersion,
}

impl ContextBuilder {
    pub fn build(&self, ctx: &DecisionContext) -> BuiltPrompt {
        // 1. Render each section into text.
        // 2. Token-count each section.
        // 3. Apply budget: drop/trim lowest-priority sections first.
        // 4. Summarize or abbreviate overflow sections.
        // 5. Assemble final prompt and record section token counts.
    }
}
```

Section priorities (default, highest first):

1. Output schema and task instruction (never dropped).
2. Identity and goals.
3. Perception and location.
4. Needs and emotions.
5. Available actions.
6. Relevant people.
7. Relevant memories.
8. Institutional context.

### 7.3 Token Budgeting

```rust
pub struct PromptBudget {
    pub total_tokens: u32,
    pub sections: Vec<SectionBudget>,
}

pub struct SectionBudget {
    pub section: SectionKind,
    pub priority: u8,
    pub max_tokens: u32,
    pub trim_strategy: TrimStrategy,
}

pub enum TrimStrategy {
    Drop,
    Summarize,
    Abbreviate,
    TailDrop,       // drop oldest items
    HeadDrop,       // drop least relevant items
}
```

- `total_tokens` is configured per agent role and model context window.
- The builder guarantees the prompt fits within `total_tokens` including the expected output reserve.
- Trimming is logged so orchestration can detect context starvation.
- A minimum memory allowance ensures the agent retains some history even under pressure.

### 7.4 Prompt Registry and Versioning

- Prompts are versioned (`prompt_version`) and stored in a registry.
- Model selection can depend on prompt version and agent role.
- Prompt changes are logged; decision logs record the prompt version for audit and replay.
- A/B testing of prompt variants is supported via configuration.

### 7.5 Example Context Rendering

```
IDENTITY: Marcus, 34, sentenced 8 years for armed robbery, 3 remaining.
PERSONALITY: disciplined, loyal, ambitious, low impulsiveness, high distrust of authority.
GOALS: [avoid trouble (0.7)], [earn parole (0.6)], [protect gang cousin (0.8)].
NEEDS: hunger 0.6, sleep 0.4, safety 0.3, respect 0.5, stimulation 0.7.
EMOTION: tense, irritated.
LOCATION: Cell Block C, common room. Schedule: recreation until 17:00.
PEOPLE: Darnell (cellmate, trust 0.8, friend), Rico (rival, threat 0.6),
        Guard Hale (authority, resentment 0.5).
RECENT MEMORIES: Yesterday Rico threatened Darnell in the yard.
                Guard Hale searched my cell and found nothing.
                Parole board meets next week.
ACTIONS: talk, trade, move_to, report, request_assistance, ...
```

The model receives this plus the JSON output schema.

---

## 8. Decision Output and Validation

### 8.1 Structured Decision Schema

Agents produce structured decisions, never free-form state changes. The canonical schema is JSON (see Appendix A):

```json
{
  "decision_id": "dec_01J...",
  "agent_id": 42,
  "action": "talk",
  "target": { "kind": "agent", "id": 17 },
  "parameters": { "topic": "warn_about_rico", "tone": "quiet" },
  "intent": "Warn Darnell that Rico may act tonight.",
  "expected_outcome": "Darnell stays alert and avoids Rico.",
  "urgency": 0.6,
  "confidence": 0.7,
  "reasoning": "Rico has threatened Darnell before; recreation ends soon.",
  "memory_updates": [
    { "kind": "episodic", "summary": "I warned Darnell about Rico." }
  ],
  "follow_up": null
}
```

Key constraints:
- `action` must be in the agent's action catalog.
- `target` must be a valid entity reference and, if required, in range.
- `parameters` must conform to the action's `ParameterSchema`.
- `memory_updates` are suggestions; the memory system applies them selectively, subject to importance scoring.
- No field may address simulation internals (no entity IDs outside perception, no direct state paths).

### 8.2 Validation Pipeline

Validation is performed by the simulation engine's `ActionValidator`, not by the agent.

```
RawDecision
    |
    v
[1] Format validation        JSON parse + schema check
    |
    v
[2] Semantic validation      action exists, parameters valid, target exists
    |
    v
[3] Authority validation     agent has authority for action/role
    |
    v
[4] World-state validation   preconditions, location, schedule, reachability
    |
    v
[5] Resource validation      cost affordable, cooldown elapsed
    |
    v
[6] Legality/policy check    allowed by current policy and classification
    |
    v
[7] Safety/anti-exploit      no self-harm loops, bounded values, no injection
    |
    v
[8] Clamp/repaire            clamp out-of-range values, normalize enums
    |
    v
ValidatedDecision  OR  Rejection(reason)
```

```rust
pub enum ValidationResult {
    Accepted(ValidatedDecision),
    Repaired(ValidatedDecision, Vec<Repair>),
    Rejected(Rejection),
}

pub struct Rejection {
    pub stage: ValidationStage,
    pub reason: RejectionReason,
    pub message: String,
    /// Whether a repair retry is allowed.
    pub retryable: bool,
}
```

**Stage details:**

1. **Format:** parse JSON, enforce schema, reject unknown fields, enforce types, ranges, and required fields.
2. **Semantic:** action ID known; target kind matches; parameter enum values valid; no NaN/Infinity; string lengths bounded.
3. **Authority:** the agent's role and rank permit the action; prisoners cannot issue orders; guards cannot change policy; only the warden can approve parole recommendations, etc.
4. **World-state:** target is visible or known; path exists; action allowed by current schedule and lockdown; prerequisites (e.g., has contraband to trade) hold.
5. **Resource:** agent can afford the cost; cooldown has elapsed; action does not exceed per-tick action budget.
6. **Legality/policy:** action is permitted by current institutional policy and the agent's disciplinary status. (Some actions are illegal but possible — e.g., assault — and are allowed as decisions with consequences, not rejected. Legality affects outcome probability and disciplinary response, not whether the AI may choose it.)
7. **Safety/anti-exploit:** reject self-targeting loops, actions with unbounded parameters, prompt-injection-like strings in free-text fields, repeated identical decisions, and attempts to reference non-observable entities.
8. **Clamp/repair:** coerce values into legal ranges, normalize aliases, fill defaults.

### 8.3 No Arbitrary State Mutation

This is a hard invariant:

- Agents have **no** handle to mutable world state.
- Agents receive a read-only `WorldView` and return a `Decision`.
- Only the simulation engine applies effects through `Command`s.
- AI-generated free text is treated as data, never as code or instructions to the engine.
- `memory_updates` are applied through the memory system's selectivity rules, not blindly.
- Validation failures never partially apply; the decision is atomic.

### 8.4 Repair and Fallback

On rejection:

1. If `retryable`, send the model a **repair prompt** containing the original context plus the rejection reason and the invalid output; maximum 2 repair attempts.
2. If still invalid, fall back to the deterministic utility decision for the same context.
3. Log the rejection, the repair attempts, and the fallback.
4. If the AI repeatedly fails, temporarily mark the agent as `deterministic_only` and back off AI calls.

Repair prompts are budgeted separately and are smaller than full prompts. They do not repeat the entire context; they include only the relevant section and the error.

### 8.5 Command Application and Outcomes

A `ValidatedDecision` is converted to one or more `Command`s:

```rust
pub enum Command {
    Move { agent: AgentId, to: LocationId, path: Path },
    Interact { actor: AgentId, target: EntityRef, interaction: Interaction },
    Trade { actor: AgentId, target: AgentId, offer: Bundle, request: Bundle },
    Attack { actor: AgentId, target: AgentId, method: AttackMethod },
    Report { reporter: AgentId, report: IncidentReport },
    RequestAssistance { requester: AgentId, request: AssistanceRequest },
    Enforce { guard: AgentId, target: AgentId, measure: EnforcementMeasure },
    Manage { warden: AgentId, directive: AdministrativeDirective },
    // ...
}
```

The engine executes commands in deterministic order (priority by urgency, then agent ID), resolves interactions, emits events, and produces `Outcome`s. Outcomes are then fed back into the loop for all observing agents.

---

## 9. AI Orchestration

### 9.1 Hybrid Decision Model

The orchestration layer routes decisions through three tiers (see §6.3). The key rule: **AI is used where it adds value, not everywhere.**

| Concern | Engine |
|---|---|
| Needs decay, schedule progression, movement, resource consumption | Deterministic |
| Routine candidate ranking and selection | Deterministic utility |
| Social interpretation, negotiation, conflict strategy, crisis response, planning, dialogue content | AI |
| Guard duty decisions | Deterministic routine + AI for incidents |
| Warden strategic decisions | AI periodic + deterministic execution |

### 9.2 Scheduling and Periodic Updates

AI updates are periodic, not continuous (`agents.md` §19). The scheduler staggers agents to avoid bursts.

```rust
pub struct ScheduleConfig {
    /// Base interval between AI deliberations per role.
    pub prisoner_interval: SimDuration,
    pub guard_interval: SimDuration,
    pub warden_interval: SimDuration,
    /// Random jitter fraction to desynchronize agents.
    pub jitter: f32,
    /// Maximum AI requests per simulation tick.
    pub max_requests_per_tick: u32,
    /// Maximum concurrent in-flight requests.
    pub max_concurrent_requests: u32,
    /// Whether AI deliberation is allowed while the sim is paused.
    pub allow_while_paused: bool,
}
```

Next deliberation time:

```
next_ai_time = last_ai_time + interval * (1 + rand(-jitter, +jitter))
```

Event triggers can move `next_ai_time` earlier, subject to budget:

- Witnessed violence or accident.
- Direct order from the warden or a guard.
- Urgent need crossing a threshold (severe hunger, injury, fear).
- A direct conversation or confrontation.
- Discovery of contraband or evidence.
- Lockdown, riot, or escape alarm.

Triggered AI decisions have higher queue priority but still consume the agent's token budget.

### 9.3 Async Decision Lifecycle

AI calls are asynchronous relative to the simulation tick:

```
tick N:     scheduler marks agent due -> enqueue LlmRequest
tick N+k:   response arrives -> validate -> apply Command -> Outcome
meanwhile:  agent continues routine behavior under its current intent
```

- Agents have a `pending_decision` state. While pending, they may continue low-stakes routine actions (idle, walk, eat) but not high-stakes ones.
- Responses carry the `SimTime` of the request. The validator checks that the decision is still applicable (target still present, schedule unchanged). Stale decisions are re-validated or discarded.
- A maximum wait (`decision_timeout`) applies; on timeout, the deterministic fallback is used and the request is abandoned.
- The simulation can keep running while AI calls are in flight; it does not block the world.

### 9.4 Rate Limiting

```rust
pub struct RateLimitConfig {
    /// Requests per minute per provider.
    pub requests_per_minute: u32,
    /// Tokens per minute per provider.
    pub tokens_per_minute: u64,
    /// Burst capacity.
    pub burst: u32,
    /// Per-agent requests per simulation hour.
    pub per_agent_per_sim_hour: u32,
}
```

Implementation: token-bucket rate limiter per provider, plus a global request queue.

- Requests are prioritized: urgent triggers > scheduled deliberation > background reflection.
- If the provider limit is reached, requests wait in the queue; if the queue exceeds a threshold, lower-priority requests are dropped to deterministic fallback.
- Provider-specific limits are configured; multiple providers can share load.

### 9.5 Token Budgeting

```rust
pub struct BudgetConfig {
    /// Global token budget per simulation day.
    pub global_per_sim_day: u64,
    /// Per-agent token budget per simulation day.
    pub per_agent_per_sim_day: u64,
    /// Maximum input tokens per decision.
    pub max_input_tokens: u32,
    /// Maximum output tokens per decision.
    pub max_output_tokens: u32,
    /// Reserve fraction for high-priority/urgent decisions.
    pub reserve_fraction: f32,
    /// Cost per million input/output tokens (per model).
    pub cost_rates: HashMap<ModelId, CostRate>,
}
```

Accounting:
- Every request and response records input/output tokens and cost.
- Budgets are tracked per agent, per role, and globally.
- When an agent's budget is exhausted, it falls back to deterministic utility for the rest of the period.
- When the global budget is exhausted, only urgent triggers are served from the reserve; everything else is deterministic.
- Budgets reset per simulation day (configurable) and are persisted across saves.

### 9.6 Caching

Two caching layers:

**Prompt-prefix cache.** The stable system prompt is sent first so providers with prompt caching can reuse it. Cache keys include `prompt_version` and model.

**Response cache.** Decisions can be cached when:
- the model is deterministic (temperature = 0) or the provider supports seed control;
- the prompt hash matches;
- the agent's memory and relevant world state have not changed (cache invalidation on significant events);
- the cache entry is within its TTL.

```rust
pub struct CacheConfig {
    pub enabled: bool,
    pub ttl: SimDuration,
    pub max_entries: usize,
    pub invalidate_on_significant_event: bool,
    /// Cache only if temperature == 0.
    pub require_deterministic: bool,
}
```

Caching reduces cost and latency but must not make agents repetitive. A cache hit still passes through validation and is logged as a cache hit.

### 9.7 Error Handling and Retries

```rust
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub backoff_multiplier: f32,
    pub jitter: f32,
    pub timeout: Duration,
    /// Fallback model if the primary fails.
    pub fallback_models: Vec<ModelId>,
    /// Circuit breaker threshold.
    pub circuit_breaker_failures: u32,
    pub circuit_breaker_cooldown: Duration,
}
```

Failure handling matrix:

| Failure | Handling |
|---|---|
| Timeout | Retry with trimmed context; then fallback model; then deterministic |
| Malformed JSON | Repair prompt (max 2); then deterministic |
| Schema violation | Repair prompt; then deterministic |
| Validation rejection | Repair with reason; then deterministic |
| Provider 429 (rate limit) | Wait/queue with backoff; then fallback provider |
| Provider 5xx | Retry with backoff; circuit breaker; fallback provider |
| Provider auth/config error | Log, disable provider, use fallback; continue sim |
| Budget exhausted | Deterministic utility; no request |
| Agent stuck in pending state | Timeout; fallback; clear pending |

The simulation never panics or halts due to AI failure. All failures are logged and surfaced in the UI as degraded-agent indicators.

### 9.8 Logging and Telemetry

Every AI interaction produces structured records:

```rust
pub struct DecisionRecord {
    pub decision_id: DecisionId,
    pub agent: AgentId,
    pub role: Role,
    pub sim_time: SimTime,
    pub tier: DecisionTier,
    pub engine: EngineKind,       // Ai, Utility, Deterministic, Fallback
    pub action: ActionId,
    pub target: Option<EntityRef>,
    pub outcome_success: bool,
    pub prompt_version: PromptVersion,
    pub context_hash: u64,
    pub model: Option<ModelId>,
    pub validation: ValidationSummary,
    pub fallback_reason: Option<String>,
    pub latency: Duration,
}

pub struct LlmRequestRecord {
    pub request_id: RequestId,
    pub decision_id: DecisionId,
    pub provider: ProviderId,
    pub model: ModelId,
    pub prompt_tokens: u32,
    pub output_tokens: u32,
    pub cost_usd: f64,
    pub latency: Duration,
    pub cache_hit: bool,
    pub attempts: u32,
    pub error: Option<String>,
}
```

Records feed:
- The event timeline / UI.
- Debugging and post-hoc analysis.
- Budget dashboards.
- Replay and determinism audits.

### 9.9 Model Selection

```rust
pub struct ModelCatalog {
    pub models: Vec<ModelDefinition>,
}

pub struct ModelDefinition {
    pub id: ModelId,
    pub provider: ProviderId,
    pub name: String,
    pub context_window: u32,
    pub cost_per_million_input: f64,
    pub cost_per_million_output: f64,
    pub speed_tier: SpeedTier,
    pub capabilities: ModelCapabilities,
}
```

Selection policy:
- **Cheap/fast model** for routine deliberation and short dialogues.
- **Strong model** for strategic planning, crisis decisions, warden strategy, complex social reasoning.
- **Local/mock model** for offline tests and deterministic replay.
- **Fallback chain** per role, configurable.
- **Provider failover** when a provider is unavailable.

Model selection is deterministic given configuration and agent role; it is logged per decision.

### 9.10 Determinism and Replay

AI output is nondeterministic, so reproducibility is handled explicitly:

- Each simulation run has a `RunId` and a seed.
- Every AI request/response is logged with prompt hash, model, and response.
- A **replay mode** can feed logged responses instead of calling providers.
- A **mock provider** with scripted or deterministic responses supports tests.
- Tests use `temperature = 0` and pinned prompt versions where possible.

The simulation's deterministic systems remain reproducible; AI decisions are recorded as an auditable overlay.

### 9.11 Concurrency

- The orchestrator is async; provider calls run concurrently up to `max_concurrent_requests`.
- Simulation state mutation remains single-threaded and ordered by the engine.
- AI requests are cancel-safe; results are applied only if the agent is still in a compatible state.
- Long simulations use background processing so the UI can observe while AI calls proceed.

---

## 10. Guard Autonomy

### 10.1 Guard Profile and Authority

Guards are agents with a duty profile layered on the shared framework:

```rust
pub struct GuardProfile {
    pub rank: Rank,
    pub post: Option<PostId>,
    pub shift: ShiftId,
    pub competence: f32,
    pub experience: f32,
    pub morale: f32,
    pub fatigue: f32,
    pub stress: f32,
    pub loyalty: f32,
    pub integrity: f32,
    pub authority: AuthorityLevel,
    pub use_of_force_policy: UseOfForcePolicy,
    pub assigned_wing: Option<WingId>,
    pub current_task: Option<DutyTask>,
}

pub enum AuthorityLevel {
    Observe,
    Direct,
    Detain,
    Restrain,
    UseNonLethalForce,
    UseLethalForce,   // only under strict policy and escalation
}
```

Guards have **meaningful autonomy within their assigned authority** (`agents.md` §10): they can choose how to patrol, whether to warn or enforce, when to request backup, and how to de-escalate. They cannot change policy, sentence prisoners, or exceed their use-of-force authority.

### 10.2 Guard Decision Loop

Guards run the same nine-stage loop as prisoners, with duty-specific mappings:

1. **Observe** — post area, prisoners, cell conditions, sounds, other guards; perception includes institutional alerts (lockdown, alarm).
2. **Evaluate needs** — fatigue, stress, morale, shift remaining; guard needs are job-related (rest, safety, duty completion).
3. **Review memories** — incident memory, prisoner models, previous shifts, standing orders.
4. **Assess people** — threat assessment, known troublemakers, vulnerable prisoners, group dynamics.
5. **Consider actions** — patrol, observe, check-in, search, escort, enforce, report, request assistance, rest.
6. **Select** — deterministic routine patrol; AI for incidents, conflicts, and complex judgments.
7. **Execute** — validated command.
8. **Observe consequences** — outcome, prisoner reactions, bystander reactions.
9. **Update** — incident memory, prisoner models, relationships, stress.

Duty state machine:

```
PATROL -> OBSERVE -> INTERVENE -> REPORT -> REQUEST_ASSISTANCE -> REST
```

AI deliberation is triggered by:
- A disturbance or rule violation.
- A direct order from the warden.
- A request from another guard.
- An anomaly (unusual prisoner movement, sounds, missing prisoner).
- Periodic reflection at shift milestones.

Routine patrols and checks are deterministic; guards do not spend AI calls on an empty corridor.

### 10.3 Reporting to the Warden

The warden receives reports rather than seeing every event (`agents.md` §10).

```rust
pub struct IncidentReport {
    pub id: ReportId,
    pub reporter: AgentId,
    pub at: SimTime,
    pub location: LocationId,
    pub kind: IncidentKind,
    pub severity: Severity,
    pub involved: Vec<AgentId>,
    pub description: String,
    pub evidence: Vec<Evidence>,
    /// 0..1 reporter confidence; affected by perception, fatigue, bias.
    pub confidence: f32,
    pub recommended_action: Option<ActionId>,
    pub status: ReportStatus,
    pub acknowledged_by: Option<AgentId>,
    pub resolution: Option<Resolution>,
}
```

Report properties:
- Reports are generated by the guard's memory and perception, so they can be inaccurate, biased, or incomplete.
- Severity is the reporter's assessment; the warden may reassess.
- Reports enter the warden's **reports inbox** (see §11).
- Guards can also file low-severity "observations" that do not demand immediate action.
- Repeated reports about the same prisoner accumulate into a pattern the warden can investigate.

### 10.4 Assistance Requests and Dispatch

```rust
pub struct AssistanceRequest {
    pub id: RequestId,
    pub requester: AgentId,
    pub kind: AssistanceKind,
    pub location: LocationId,
    pub severity: Severity,
    pub needed_units: u32,
    pub at: SimTime,
    pub status: RequestStatus,
    pub assigned: Vec<AgentId>,
    pub acknowledged: Vec<AgentId>,
    pub resolved_at: Option<SimTime>,
}

pub enum AssistanceKind {
    Backup,
    Medical,
    RiotControl,
    EscapeResponse,
    Fire,
    Hostage,
    SearchSupport,
    EscortSupport,
}
```

Protocol:

1. Guard creates an `AssistanceRequest` with location, kind, severity, and needed units.
2. The request is routed to the **dispatcher** — either the warden AI or a deterministic dispatch system, depending on configuration.
3. The dispatcher selects nearby available guards based on proximity, current duty, fatigue, and authority.
4. Selected guards receive an **order** (not a request). Orders are high-priority and interrupt routine behavior.
5. Guards acknowledge. A guard may decline only for a valid reason (already engaged, injury, conflict of interest); unjustified refusal affects morale/discipline.
6. If no acknowledgment within a timeout, the request escalates in severity and widens the selection radius.
7. The requester tracks response and can update severity or cancel.
8. On resolution, all participants update memory and the warden receives a summary.

Deterministic dispatch rules:

```
candidate_score =
      0.40 * proximity
    + 0.25 * availability
    + 0.15 * competence
    + 0.10 * (1 - fatigue)
    + 0.10 * authority_match
```

Guards on duty in the same wing get priority; specialized units (riot, medical) are selected when available.

### 10.5 Guard Memory

Guards use the same layered memory system with duty-specific content:
- **Short-term:** current post, recent prisoner behavior, active incidents.
- **Long-term:** prisoner models, recurring troublemakers, reliable informants, past incidents, lessons about de-escalation.
- **Institutional:** policies, use-of-force rules, investigation outcomes, personnel records (access-controlled).

Guards form opinions and relationships, which can lead to bias, favoritism, corruption, or excessive force. These are modeled through traits, memory, and relationship systems, and are subject to investigation and discipline.

### 10.6 Stress, Fatigue, and Misconduct

- Fatigue reduces perception clarity and decision quality, increasing the chance of mistakes.
- Stress accumulates from incidents and can trigger either withdrawal or aggression.
- Low morale and low loyalty increase the likelihood of policy violations, corruption, or excessive force.
- Misconduct is detected through reports, investigations, and witnessed events, with consequences for the guard.
- Guards can request reassignment, rest, or support; the warden can grant or deny.

---

## 11. Warden Coordination

### 11.1 Reports Inbox

The warden AI periodically reviews the reports inbox:

- New reports are prioritized by severity, confidence, location, and institutional impact.
- The warden can request more information, order an investigation, assign guards, change postings, or ignore a report.
- Reports can be corroborated or contradicted by other reports; the warden's belief store is updated accordingly.
- The warden does not automatically see unobserved events; it knows what guards report, what it observes directly, and what institutional records state.

### 11.2 Orders and Assignments

The warden issues **directives**, which are validated commands:

```rust
pub enum AdministrativeDirective {
    AssignPost { guard: AgentId, post: PostId, shift: ShiftId },
    ChangePolicy { policy: PolicyId, value: PolicyValue },
    AuthorizeSearch { location: LocationId, scope: SearchScope },
    OrderLockdown { scope: LockdownScope, duration: SimDuration },
    InitiateInvestigation { subject: EntityRef, focus: InvestigationFocus },
    AdjustParoleRecommendation { prisoner: AgentId, recommendation: Recommendation },
    ApproveProgram { program: ProgramId, participant: AgentId },
    RequestFunding { amount: Money, purpose: String },
    ExpandFacility { facility: FacilityId },
    DisciplineStaff { guard: AgentId, measure: StaffMeasure },
    DisciplinePrisoner { prisoner: AgentId, measure: PrisonerMeasure },
}
```

Guards receive orders through the command system and execute them within their authority. The warden cannot directly control guard movement or force decisions; it influences through assignments, policy, and orders.

### 11.3 Warden AI vs Guard AI

- **Warden AI** is strategic and periodic, reviewing aggregated reports and institutional state. It does not micromanage every guard.
- **Guard AI** is tactical and local, handling immediate situations.
- **Deterministic dispatch** handles routine assistance routing unless the warden AI is configured to manage it.
- This separation keeps the warden from being omniscient and keeps guard responses fast.

---

## 12. Module Specifications and Interfaces

### 12.1 Module Responsibilities

| Module | Public types | Key operations |
|---|---|---|
| `ai::info` | `Belief`, `Source`, `InformationKind`, `PersonModel`, `BeliefStore` | ingest, merge, decay, propagate, query |
| `ai::memory` | `MemoryRecord`, `MemoryLayer`, `MemoryKind`, `MemoryStore`, `MemoryQuery` | store, retrieve, consolidate, decay, forget |
| `ai::decision` | `Perception`, `NeedState`, `DecisionContext`, `ActionCandidate`, `Decision` | build context, generate candidates, select |
| `ai::prompt` | `ContextBuilder`, `PromptBudget`, `BuiltPrompt`, `PromptRegistry` | build, trim, version |
| `ai::validation` | `ActionValidator`, `ValidationResult`, `Rejection`, `Repair` | validate, repair, reject |
| `ai::llm` | `LlmClient`, `LlmRequest`, `LlmResponse`, `ModelCatalog` | complete, stream |
| `ai::orchestrator` | `Orchestrator`, `AgentController`, `DecisionScheduler`, `RateLimiter`, `TokenBudget`, `ResponseCache`, `Telemetry` | register, schedule, execute, account, log |
| `ai::agents::prisoner` | `PrisonerProfile`, `PrisonerAgent` | prisoner loop |
| `ai::agents::guard` | `GuardProfile`, `GuardAgent`, `IncidentReport`, `AssistanceRequest` | guard loop, report, request |
| `ai::agents::warden` | `WardenProfile`, `WardenAgent`, `AdministrativeDirective` | strategy, directives |

### 12.2 Core Traits

```rust
/// Read-only view of the world for agent reasoning.
pub trait WorldView {
    fn time(&self) -> SimTime;
    fn location(&self, agent: AgentId) -> Option<LocationId>;
    fn entity(&self, id: EntityRef) -> Option<EntitySnapshot>;
    fn visible_from(&self, location: LocationId) -> Vec<EntityRef>;
    fn schedule_at(&self, time: SimTime) -> ScheduleContext;
    fn policy(&self, id: PolicyId) -> Option<PolicyValue>;
    fn institutional_record(&self, id: RecordId) -> Option<InstitutionalRecord>;
}

/// A decision engine: deterministic utility or AI-backed.
pub trait DecisionEngine {
    fn decide(&self, ctx: &DecisionContext) -> DecisionOutcome;
}

/// Validation is always owned by the simulation engine.
pub trait ActionValidator {
    fn validate(&self, decision: &Decision, world: &dyn WorldView) -> ValidationResult;
}

/// Agent lifecycle, driven by the orchestrator.
pub trait Agent {
    fn id(&self) -> AgentId;
    fn role(&self) -> Role;
    fn observe(&mut self, world: &dyn WorldView) -> Perception;
    fn build_context(&self, perception: Perception) -> DecisionContext;
    fn apply_outcome(&mut self, outcome: &Outcome);
    fn next_deliberation(&self, now: SimTime) -> SimTime;
}

/// Provider-agnostic LLM access.
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmError>;
    fn model(&self) -> ModelId;
    fn capabilities(&self) -> ModelCapabilities;
}

/// Orchestrator entry point called by the simulation tick.
pub trait Orchestrator {
    fn register(&mut self, agent: Box<dyn Agent>);
    fn on_tick(&mut self, now: SimTime, world: &dyn WorldView);
    fn apply_outcomes(&mut self, outcomes: Vec<Outcome>);
    fn telemetry(&self) -> &Telemetry;
}
```

### 12.3 Decision Sequence

```
Simulation tick
      |
      v
Orchestrator::on_tick
      |
      +--> for each agent due:
      |        agent.observe(world)
      |        agent.build_context(perception)
      |        DecisionScheduler chooses tier
      |        [AI tier] --> PromptBuilder.build --> LlmGateway.complete
      |                            |                       |
      |                            |                       v
      |                            |               RawDecision
      |                            |                       |
      |                            |                       v
      |                            |               ActionValidator.validate
      |                            |                       |
      |                            |           Accepted / Repaired / Rejected
      |                            |                       |
      |                            |                       v
      |                            +------------ ValidatedDecision
      |
      +--> engine converts ValidatedDecision -> Command
      |
      v
Engine applies commands in deterministic order -> Outcome(s)
      |
      v
Orchestrator.apply_outcomes -> agents update memory/beliefs
```

### 12.4 Guard Assistance Sequence

```
Guard observes disturbance
      |
      v
Guard AI/deterministic chooses RequestAssistance
      |
      v
ActionValidator validates request
      |
      v
Dispatcher (warden AI or deterministic) selects guards
      |
      v
Orders issued to selected guards (high priority)
      |
      v
Guards acknowledge / decline
      |
      v
Response converges; Outcome emitted
      |
      v
Reports and memories updated; warden notified
```

---

## 13. Configuration

All AI behavior is configurable without code changes.

```rust
pub struct AiConfig {
    pub enabled: bool,
    pub schedule: ScheduleConfig,
    pub budget: BudgetConfig,
    pub rate_limit: RateLimitConfig,
    pub cache: CacheConfig,
    pub retry: RetryConfig,
    pub memory: MemoryConfig,
    pub info: InfoConfig,
    pub prompts: PromptConfig,
    pub models: ModelCatalog,
    pub providers: Vec<ProviderConfig>,
    pub agents: HashMap<Role, AgentAiConfig>,
}

pub struct AgentAiConfig {
    pub ai_enabled: bool,
    pub allowed_actions: Vec<ActionId>,
    pub max_decisions_per_sim_day: u32,
    pub deliberation_interval: SimDuration,
    pub model_preference: Vec<ModelId>,
    pub prompt_version: PromptVersion,
    pub deterministic_fallback: bool,
}

pub struct InfoConfig {
    pub base_decay_rate: f32,
    pub rumor_decay_rate: f32,
    pub confirmation_bias_weight: f32,
    pub corroboration_required_for_fact: u32,
    pub forget_threshold: f32,
    pub forget_after: SimDuration,
}

pub struct MemoryConfig {
    pub short_term_slots: usize,
    pub long_term_max_records: usize,
    pub long_term_threshold: f32,
    pub short_term_threshold: f32,
    pub importance_weights: ImportanceWeights,
    pub decay_half_life: SimDuration,
    pub consolidation_interval: SimDuration,
    pub retrieval_top_k: usize,
    pub retrieval_max_tokens: u32,
}
```

Configuration is hot-reloadable where safe (budgets, rate limits, prompt versions); structural changes require a restart or new simulation.

---

## 14. Persistence

AI state must survive saves (`agents.md` §23).

**Persisted per agent:**
- Identity, personality, traits.
- Belief store (with provenance, confidence, timestamps).
- Layered memory records (including importance, rehearsal, decay state).
- Person models and relationship vectors.
- Goals and goal progress.
- Pending decision state (or mark as abandoned on load).
- AI config overrides, budget counters, model preferences.
- Decision history pointer / summary.

**Persisted globally:**
- Institutional memory.
- Token/cost accounting for the current period.
- Prompt registry and versions.
- Telemetry logs (or a configurable retention window).
- Run ID and AI replay log.

**Save/load invariants:**
- No AI request is in flight across a save; pending decisions are resolved or abandoned deterministically.
- Memory and belief timestamps use simulation time, so loading restores decay state accurately.
- Cache entries are not persisted; they are rebuilt lazily.
- Budget counters persist so agents cannot reset their allowance by saving.

---

## 15. Observability and Logging

The UI and debug tooling consume:

- **Agent snapshot:** identity, needs, emotions, goals, current intent, current action, location, pending decision, last decision, budget usage, degraded flag.
- **Decision feed:** all decisions with engine, validation result, fallback reason.
- **Belief inspector:** beliefs by subject/predicate, confidence, kind, source, contradictions.
- **Memory inspector:** records by layer, importance, retrieval count, last accessed.
- **Orchestration dashboard:** requests in flight, queue depth, provider latency, token usage, cost, cache hit rate, fallback rate.
- **Guard views:** post, duty, stress, fatigue, reports filed, assistance requests.
- **Warden views:** reports inbox, directives issued, institutional indicators.

Logs are structured (JSON) and written to an append-only store with simulation-time indexing.

---

## 16. Testing and Verification

### 16.1 Unit Tests

- **Information model:** ingestion, merge, contradiction, decay, promotion, rumor distortion, lying, person model aggregation.
- **Memory:** importance scoring, threshold selectivity, retrieval ranking, consolidation, forgetting, deduplication, capacity eviction.
- **Validation:** schema, semantic, authority, world-state, resource, legality, safety; repair; rejection reasons.
- **Prompt:** budget trimming, section priorities, cache-key stability, prompt versioning.
- **Orchestration:** scheduling, staggering, budget exhaustion fallback, rate limiting, retry, circuit breaker, timeout, stale decision handling.
- **Guard:** report generation, assistance dispatch scoring, escalation, authority limits.

### 16.2 Integration Tests

- Full decision loop with a **mock LLM** returning scripted decisions.
- Malformed-output and rejection paths.
- Multi-agent scenarios: rumor propagation leading to a fight; guard reports leading to an investigation; assistance request resolving a riot.
- Save/load round-trip preserving beliefs, memories, and budgets.

### 16.3 Simulation Tests

- Long-duration runs (days/months of sim time) to detect memory growth, budget drift, repetitive behavior, and deadlocks.
- Determinism tests with mock providers.
- Stress tests with hundreds of agents and degraded AI (all deterministic) to verify the simulation remains stable.

### 16.4 Invariants

- No agent code path can mutate world state directly.
- Every applied command has a corresponding validated decision and log record.
- Every AI failure has a logged fallback; the simulation continues.
- Budgets are never exceeded; over-budget agents are deterministic.
- AI decisions never reference unobserved entities or inaccessible records.

---

## 17. Risks and Mitigations

| Risk | Mitigation |
|---|---|
| AI cost explodes with population | Token budgets per agent/day, deterministic tiers, caching, staggering, model tiering |
| Repetitive or degenerate behavior | Importance-weighted memory, novelty, cache TTL, temperature control, prompt variation |
| Hallucinated/invalid actions | Strict schema, action catalog, multi-stage validation, repair, deterministic fallback |
| Prompt injection via prisoner dialogue | Treat all free text as data; sanitize parameters; never interpret text as engine instructions |
| Provider outage | Circuit breaker, fallback providers, deterministic fallback, queue with priorities |
| Memory unbounded growth | Capacity caps, consolidation, decay, eviction, archival |
| Guard/warden omniscience | Reports-based warden, access-controlled institutional memory, imperfect guard perception |
| Simulation nondeterminism | Deterministic engine, recorded AI overlay, replay mode, mock providers |
| Agents stuck waiting on AI | Timeouts, pending-state limits, stale-decision revalidation |
| Save/load divergence | Simulation-time timestamps, persisted budgets, no in-flight requests at save |

---

## 18. Open Questions

1. **Tick granularity:** How many simulation ticks per AI deliberation, and how does that scale with population? Needs benchmarking.
2. **Embeddings:** Are embeddings available locally, or should retrieval be tag/keyword-based initially?
3. **Provider support:** Which providers must be supported at launch, and what are their rate limits?
4. **Guard dispatch ownership:** Should the warden AI always handle dispatch, or should a deterministic dispatcher be the default?
5. **Compound plans:** How far should the AI be allowed to plan multi-step actions versus one validated action at a time?
6. **Dialogue granularity:** Should conversation content be AI-generated line-by-line or summarized at the interaction level?
7. **Warden omniscience boundary:** Exactly which institutional records should the warden AI see automatically?
8. **Replay policy:** Should replay be a debug feature only, or a first-class deterministic mode?
9. **Budget accounting period:** Per simulation day, per real hour, or both?
10. **Guard misconduct modeling:** How should corruption and excessive force be detected and investigated?

---

## Appendix A — Decision JSON Schema

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "AgentDecision",
  "type": "object",
  "additionalProperties": false,
  "required": ["decision_id", "agent_id", "action", "intent", "urgency", "confidence", "reasoning"],
  "properties": {
    "decision_id": { "type": "string", "minLength": 1 },
    "agent_id": { "type": "integer", "minimum": 0 },
    "action": { "type": "string", "minLength": 1 },
    "target": {
      "type": "object",
      "additionalProperties": false,
      "required": ["kind", "id"],
      "properties": {
        "kind": { "enum": ["agent", "location", "object", "gang", "none"] },
        "id": { "type": "integer", "minimum": 0 }
      }
    },
    "parameters": {
      "type": "object",
      "additionalProperties": true,
      "propertyNames": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$" }
    },
    "intent": { "type": "string", "minLength": 1, "maxLength": 500 },
    "expected_outcome": { "type": "string", "maxLength": 500 },
    "urgency": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
    "confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
    "reasoning": { "type": "string", "maxLength": 2000 },
    "memory_updates": {
      "type": "array",
      "maxItems": 5,
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "summary"],
        "properties": {
          "kind": { "enum": ["episodic", "semantic", "procedural", "social", "emotional"] },
          "summary": { "type": "string", "minLength": 1, "maxLength": 300 }
        }
      }
    },
    "follow_up": {
      "type": ["object", "null"],
      "additionalProperties": false,
      "required": ["action", "delay"],
      "properties": {
        "action": { "type": "string" },
        "delay": { "type": "integer", "minimum": 0 },
        "condition": { "type": "string", "maxLength": 300 }
      }
    }
  }
}
```

Validation notes:
- `parameters` values are further constrained by the selected action's `ParameterSchema`.
- `target` must be observable or known to the agent.
- `memory_updates` are suggestions and are filtered by importance scoring.
- `follow_up` is a plan hint; it is decomposed into validated sub-actions and never executed directly.

---

## Appendix B — Prompt Template Skeleton

```text
# ROLE
You are {name}, a {role} in {prison_name}.
{personality_summary}
{identity_summary}

# RULES
- You only know what you have observed, heard, inferred, or been told.
- Your beliefs may be wrong. Confidence is not certainty.
- You cannot directly change the world. You choose one action.
- You must output a single JSON object matching the schema.
- Do not invent actions, targets, or facts.
- Free-text fields are descriptions, not commands.

# ACTION CATALOG
{compressed_action_catalog}

# CURRENT STATE
## Needs
{needs_summary}
## Emotions
{emotion_summary}
## Location and Schedule
{location_and_schedule}
## Perception
{perception_summary}

# PEOPLE
{people_summary}

# MEMORIES
{relevant_memories}

# INSTITUTIONAL CONTEXT
{institutional_context}

# AVAILABLE ACTIONS
{candidate_actions}

# TASK
Choose exactly one action that best serves your goals and needs given your
beliefs and the current situation. Output JSON only.

# OUTPUT SCHEMA
{output_schema}
```

Budget trimming drops or abbreviates sections from the bottom upward, never the rules, task, or output schema.

---

## Appendix C — Glossary

- **Agent snapshot** — the immutable-ish summary of an agent's identity, traits, needs, emotions, goals, and current state used to build context.
- **Belief** — a subject-predicate claim with confidence, kind, provenance, and decay.
- **Candidate action** — a validated-by-construction action option generated by the deterministic catalog.
- **Confirmed fact** — a belief corroborated by independent sources or an authoritative record.
- **Decision context** — the assembled input to a decision: perception, needs, memories, people, candidates, constraints, and budget.
- **Deliberation** — an AI decision cycle; distinct from routine deterministic updates.
- **Deterministic fallback** — the utility-based decision used when AI is unavailable or exhausted.
- **Directive** — a warden-issued validated command (administrative action).
- **Epistemic kind** — direct knowledge, rumor, suspicion, belief, or confirmed fact.
- **Importance score** — weighted measure determining memory promotion and decay.
- **Information model** — the system representing what agents know and how confidently.
- **Institutional memory** — the prison's shared, persistent history and records.
- **Orchestrator** — the layer scheduling, budgeting, validating, and logging agent decisions.
- **Person model** — an agent's aggregated beliefs about another individual.
- **Prompt budget** — token allocation across context sections.
- **Rumor propagation** — the process by which beliefs spread with distortion and deception.
- **Tier** — deterministic simulation, utility choice, or AI deliberation.
- **Token budget** — configured AI token allowance per agent/role/global period.
- **Validated decision** — a decision that passed all validation stages and may be applied by the engine.
