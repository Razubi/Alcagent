# Social Systems Design

**Agent:** Social Systems Agent  
**Scope:** Social relationships, emergent social structure, gangs/organizations, conflict/violence/discipline, information/epistemics, and escape-attempt modeling.  
**Primary directive references:** `agents.md` §3 (information model), §6 (social relationships and prison society), §7 (gangs and prison organizations), §8 (conflict, violence, and disciplinary events), §16 (escape attempts and prison crises).  
**Related systems:** Prisoner AI, Needs/Psychology, Warden AI, Guard AI, Staff, Economy, Facilities/Schedules, Events, Memory/History, UI, Persistence.

`architecture-report.md (removed)` was not present at design time; this document defines social-layer contracts and integration points so other agents can align interfaces.

---

## 1. Design goals and responsibilities

### 1.1 Goals
- Model prisoners (and relevant staff/warden interactions) as socially situated agents whose behavior emerges from relationships, perceptions, group pressures, and history.
- Keep relationships **directional**, **contextual**, and **memory-backed** (who helped/betrayed/humiliated/protected whom matters).
- Detect social roles and structures **emergently** from interaction graphs and behavior, without hard-coded caste labels.
- Support gangs as **living organizations** with internal politics and individual-member divergence.
- Resolve interpersonal conflict and escape attempts at an **appropriate abstraction level** with explainable, state-dependent outcomes and persistent consequences.
- Maintain an epistemic layer (facts/knowledge/rumors/suspicion/beliefs) that allows incomplete information, misinformation, and asymmetry.

### 1.2 Non-goals / boundaries
- No scripted-only gang creation or conflict outcomes; scripts may seed conditions but outcomes arise from state.
- No direct mutation of global simulation state by AI text/plans; social system exposes **validated actions/events** and state transitions.
- No requirement for per-tick LLM inference for every social micro-decision; use hybrid deterministic/utility logic with periodic deeper reasoning hooks.
- No graphic depiction logic; UI consumes structured summaries/timelines/networks.

### 1.3 Core invariants
- Relationship dimensions are bounded and time-evolving; no dimension is permanently static.
- Every consequential social outcome emits an explainable event with causes, participants, and state deltas.
- Information is agent-local by default; shared "ground truth" exists only in world/event logs, not in agent perception.
- Gang membership and leadership are derived states subject to change; individuals retain independent goals.

---

## 2. Shared primitives and identifiers

```rust
use std::collections::{BTreeMap, HashMap, HashSet};

pub type AgentId = u64;        // prisoners, guards, staff, warden (typed via registry)
pub type GroupId = u64;        // gangs/organizations/cells/cliques as needed
pub type EventId = u64;
pub type LocationId = u64;
pub type Tick = u64;
pub type SimTime = i64;        // canonical simulation timestamp

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentKind { Prisoner, Guard, Staff, Warden }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelationshipScope { PrisonerPrisoner, PrisonerStaff, StaffStaff, CrossGroup }

/// Normalized scalar in [-1.0, 1.0] unless noted; 0.0 = neutral/unknown pressure.
#[derive(Clone, Copy, Debug, Default)]
pub struct Bounded(pub f32);

impl Bounded {
    pub fn clamp(v: f32) -> Self { Self(v.clamp(-1.0, 1.0)) }
    pub fn pos01(v: f32) -> f32 { v.clamp(0.0, 1.0) }
}

#[derive(Clone, Debug)]
pub struct SocialContext {
    pub now: SimTime,
    pub tick: Tick,
    pub location: Option<LocationId>,
    pub witnesses: Vec<AgentId>,
    pub staff_present: Vec<AgentId>,
    pub active_policy_flags: PolicyFlags, // e.g. lockdown, strict enforcement
    pub environmental_stress: f32,        // crowding, noise, heat, scarcity signals
}

#[derive(Clone, Debug, Default)]
pub struct PolicyFlags {
    pub lockdown: bool,
    pub solitary_enforcement: bool,
    pub surveillance_level: f32,
    pub use_of_force_posture: f32,
    pub privilege_restrictions: f32,
}
```

Integration expectation: core simulation owns ID registries, agent profiles (personality/needs/health/skills), location graph, schedules, and authoritative event bus. Social system consumes snapshots/queries and emits `SocialEvent`s and `StateDelta`s.

---

## 3. Information model (epistemics) — §3

Agents act on perceived reality, not omniscient truth. Each agent maintains epistemic records keyed by claim/topic/entity.

### 3.1 Data model

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClaimKind {
    Fact,            // "X happened at T/L" as understood by agent
    TraitBelief,     // "A is dangerous/helpful/corrupt"
    RelationshipBelief, // "A and B are allied"
    IntentBelief,    // "A plans to attack/escape/snitch"
    GroupBelief,     // "Gang G controls yard"
    PolicyBelief,    // "guards search cells tonight"
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EpistemicStatus {
    DirectKnowledge, // personally observed/performed
    ConfirmedFact,   // corroborated by trusted source or repeated evidence
    Belief,          // inferred/generalized
    Suspicion,       // weak/uncertain hypothesis
    Rumor,           // socially transmitted, unverified
}

#[derive(Clone, Debug)]
pub struct EpistemicRecord {
    pub id: u64,
    pub kind: ClaimKind,
    pub subject: Option<AgentId>,
    pub object: Option<AgentId>,
    pub group: Option<GroupId>,
    pub payload: ClaimPayload,          // structured claim data
    pub status: EpistemicStatus,
    pub confidence: f32,                // 0..1
    pub salience: f32,                  // 0..1, affects recall/decision weight
    pub source: InformationSource,
    pub acquired_at: SimTime,
    pub last_reinforced_at: SimTime,
    pub expires_at: Option<SimTime>,    // rumors/suspicions decay faster
    pub contradiction_count: u8,
}

#[derive(Clone, Debug)]
pub enum ClaimPayload {
    EventWitness { event_id: EventId },
    AttributeEstimate { key: String, value: f32 },
    NarrativeTag { tag: String },       // "protected_me", "stole_food", etc.
    LocationIntel { location: LocationId, note: String },
    PlanHint { description: String },
}

#[derive(Clone, Debug)]
pub enum InformationSource {
    DirectObservation,
    PersonalExperience,
    ToldBy { from: AgentId, context: String },
    Overheard { near: Vec<AgentId> },
    RumorChain { origin_hint: Option<AgentId>, hops: u8 },
    OfficialReport, // for staff/warden channels
    Inference,
}

#[derive(Clone, Debug, Default)]
pub struct EpistemicState {
    pub records: HashMap<u64, EpistemicRecord>,
    pub by_subject: HashMap<AgentId, Vec<u64>>,
    pub by_group: HashMap<GroupId, Vec<u64>>,
}
```

### 3.2 Acquisition, distortion, and decay
- **Direct observation:** creates/updates `DirectKnowledge` with high confidence; limited by perception radius, line-of-sight, attention, noise, and agent state (fatigue/fear/intoxication proxies if modeled).
- **Communication:** `ToldBy`/`Overheard` produce `Rumor`, `Belief`, or promoted knowledge depending on source trust, prior accuracy, relationship, and incentives.
- **Inference:** periodic reasoning may derive `Suspicion`/`Belief` (e.g. repeated proximity + secrecy => alliance suspicion).
- **Misinterpretation:** observation resolution can map true event -> ambiguous claim when stress is high, visibility poor, or bias strong (personality prejudice, prior hostility). Represent as lower confidence + alternate narrative tags.
- **Lying/deception:** speaker may emit claim inconsistent with own `DirectKnowledge` when utility favors manipulation, fear, loyalty, or self-protection. Credibility adjusted by listener's trust/respect/fear model of speaker.
- **Rumor spread:** when agents socialize, eligible high-salience rumors transmit with mutation probability `p_mutate` increasing per hop and with low speaker credibility; hop count increments; confidence decays.
- **Decay/reinforcement:** confidence and salience decay over time (faster for rumors/suspicions). Reinforcement from new evidence can promote status (`Rumor -> Belief -> ConfirmedFact` for agent) or create contradictions.

```rust
pub fn decay_and_promote(agent: &mut EpistemicState, now: SimTime) {
    for rec in agent.records.values_mut() {
        let age = (now - rec.last_reinforced_at).max(0) as f32;
        let base = match rec.status {
            EpistemicStatus::Rumor => 0.015,
            EpistemicStatus::Suspicion => 0.008,
            EpistemicStatus::Belief => 0.003,
            EpistemicStatus::ConfirmedFact | EpistemicStatus::DirectKnowledge => 0.001,
        };
        rec.confidence = (rec.confidence - base * age.sqrt()).clamp(0.0, 1.0);
        rec.salience *= 0.999_f32.powf(age.max(1.0));
        // promotion example: repeated independent corroboration can mark ConfirmedFact
    }
    agent.records.retain(|_, r| r.confidence > 0.05 || r.status == EpistemicStatus::DirectKnowledge);
}
```

### 3.3 Interaction with other systems
- **Prisoner/Guard AI:** decision context includes top-k relevant epistemic records (by salience, recency, subject overlap), not raw world truth.
- **Warden/Staff:** receive `OfficialReport` records; delays, omissions, and bias create institutional blind spots.
- **Conflict/Escape:** suspicion of threat, snitching, or opportunity is a primary trigger; investigations consume epistemic traces.
- **UI:** can display "known to X" vs "world fact" in inspector/debug modes.

---

## 4. Social relationship system — §6

Relationships are **directed edges** (`from -> to`) because fear/respect/trust are not symmetric.

### 4.1 Data model

```rust
#[derive(Clone, Debug, Default)]
pub struct RelationshipEdge {
    pub from: AgentId,
    pub to: AgentId,
    pub trust: f32,        // -1 distrust .. +1 trust
    pub friendship: f32,   // 0..1 affiliative warmth/companionship
    pub fear: f32,         // 0..1 perceived threat/intimidation
    pub respect: f32,      // -1 contempt .. +1 respect
    pub loyalty: f32,      // 0..1 willingness to support/defend
    pub hostility: f32,    // 0..1 antagonism/aggression tendency
    pub attraction: f32,   // 0..1 interpersonal/romantic pull where appropriate
    pub resentment: f32,   // 0..1 lingering grievance
    pub dependency: f32,   // 0..1 reliance for protection/resources/access
    pub rivalry: f32,      // 0..1 status/competition tension
    pub debt: f32,         // -1 owed to other .. +1 other owes me (accounting-like)
    pub gratitude: f32,    // 0..1 positive obligation/appreciation
    pub familiarity: f32,  // 0..1 interaction history depth
    pub last_interaction: SimTime,
    pub volatility: f32,   // 0..1 how quickly edge changes (personality/context-driven)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SocialMemoryTag {
    Helped,
    Betrayed,
    Humiliated,
    Protected,
    StoleFrom,
    SharedResources,
    LiedTo,
    SupportedInCrisis,
    Threatened,
    Attacked,
    Defended,
    SnitchedOn,       // suspected or confirmed; confidence tracked in epistemics
    Recruited,
    Exploited,
}

#[derive(Clone, Debug)]
pub struct SocialMemoryEvent {
    pub at: SimTime,
    pub tag: SocialMemoryTag,
    pub actor: AgentId,
    pub target: AgentId,
    pub witnesses: Vec<AgentId>,
    pub location: Option<LocationId>,
    pub severity: f32,         // 0..1
    pub context: EventId,      // link to world event for explainability
    pub confidence: f32,       // from epistemic resolution for observer memories
}

#[derive(Clone, Debug, Default)]
pub struct SocialMemoryLog {
    pub events: Vec<SocialMemoryEvent>, // ring/bounded with salience-based retention
}

#[derive(Clone, Debug, Default)]
pub struct SocialState {
    pub edges: HashMap<(AgentId, AgentId), RelationshipEdge>,
    pub memories: HashMap<AgentId, SocialMemoryLog>,
    pub dyad_summaries: HashMap<(AgentId, AgentId), DyadSummary>, // optional cache
}
```

### 4.2 Evolution rules
Relationship updates are event-driven deltas plus slow drift/decay. Each applied delta must be attributable to a `SocialMemoryEvent` or periodic state pressure (e.g. prolonged isolation).

**Baseline update function (illustrative):**
```rust
pub struct RelationshipDelta {
    pub trust: f32, pub friendship: f32, pub fear: f32, pub respect: f32,
    pub loyalty: f32, pub hostility: f32, pub attraction: f32, pub resentment: f32,
    pub dependency: f32, pub rivalry: f32, pub debt: f32, pub gratitude: f32,
}

pub fn apply_social_event(
    state: &mut SocialState,
    evt: &SocialMemoryEvent,
    profiles: &ProfileStore, // personality, needs, roles, gang context (read-only)
) {
    let d = base_delta(evt.tag, evt.severity);
    let m = modifiers(evt, profiles); // personality, current needs, power imbalance, gang alignment
    let edge_at = state.edges.entry((evt.actor, evt.target)).or_default();
    blend(edge_at, d.actor_to_target, m.at);
    let edge_ta = state.edges.entry((evt.target, evt.actor)).or_default();
    blend(edge_ta, d.target_to_actor, m.ta);
    remember(state, evt);
}

fn base_delta(tag: SocialMemoryTag, s: f32) -> DualDelta { /* examples below */ DualDelta::default() }
```

**Representative base deltas (directional):**
| Tag | Actor→Target (typical) | Target→Actor (typical) |
|---|---|---|
| Helped | gratitude+, trust+ small, dependency- small | trust+, friendship+, gratitude+ |
| Protected | loyalty+ (if aligned), dependency context | trust+, loyalty+, gratitude+, fear- (safety), dependency+ possible |
| Betrayed | hostility+ (defensive rationalization possible), trust- | trust--, hostility++, resentment++, loyalty-- |
| Humiliated | contempt/respect shift context | resentment++, hostility+, respect--, trust-, rivalry/fear context |
| StoleFrom | fear/hostility context if discovered | trust--, hostility+, resentment+ |
| SharedResources | debt+/gratitude context | trust+, friendship+, gratitude+, debt- (owed) |
| LiedTo | trust impact if later suspected | trust--, resentment+, suspicion generated |
| SupportedInCrisis | loyalty+ | trust++, loyalty++, gratitude++, friendship+ |

Modifiers include: impulsiveness/aggression (amplify hostility swings), empathy/discipline (dampen, promote forgiveness), trauma/fear state (amplify fear/dependency), gang membership alignment (in-group bias), power asymmetry (staff over prisoner), public vs private context (humiliation/respect stronger with witnesses), and repetition (habitual betrayal hardens hostility).

### 4.3 Memory retention and recall
- Append to actor/target (and optionally key witnesses) memory logs with confidence-weighted salience.
- **Retention score:** `salience * severity * emotional_intensity * recency_decay * identity_relevance`.
- Keep bounded per-agent episodic buffers (e.g. recent N + top-K salient long-term). Periodically consolidate repeated patterns into summary beliefs/traits in epistemic state ("A repeatedly protected me").
- Recall queries for AI decisions: fetch recent + tag-relevant + person-relevant memories to justify actions (ally with helper, avoid betrayer, retaliate against humiliator).

### 4.4 Derived dyad labels (for queries/UI, not forced categories)
```rust
#[derive(Clone, Debug, Default)]
pub struct DyadSummary {
    pub bond_score: f32,      // friendship+trust+loyalty - hostility - resentment
    pub threat_score: f32,    // fear+hostility+rivalry pressure
    pub influence_of_to_on_from: f32, // respect/fear/dependency/leadership signals
}
```
These summaries feed detection (§5) and decision utilities but do not replace underlying dimensions.

---

## 5. Emergent social structure detection — §6

Detection runs periodically (e.g. every N ticks or on significant social-change threshold) over the directed relationship graph plus recent behavior/events. No agent is assigned a fixed role; roles are **inferred scores/flags** with confidence.

### 5.1 Social graph representation
```rust
#[derive(Clone, Debug)]
pub struct SocialGraphSnapshot {
    pub nodes: Vec<AgentId>,
    pub edges: Vec<EdgeView>, // filtered/pruned active dyads
}

#[derive(Clone, Debug)]
pub struct EdgeView {
    pub from: AgentId,
    pub to: AgentId,
    pub w_trust: f32,
    pub w_affinity: f32,   // friendship + loyalty + gratitude
    pub w_hostility: f32,  // hostility + resentment + rivalry
    pub w_fear: f32,
    pub w_respect: f32,
    pub w_dependency: f32,
    pub recent_interactions: u16,
}
```

### 5.2 Detection algorithms (hybrid)
1. **Affinity/hostility clustering (unsigned/signed communities):**
   - Build affinity projection graph `A(i,j)=max(0, bond(i,j)+bond(j,i))`.
   - Detect communities/cliques via modularity/community detection (Louvain/Leiden-like or lightweight label propagation for scale) on affinity graph.
   - Detect antagonistic clusters/rival blocks via negative-edge analysis on hostility projection.
2. **Centrality and brokerage:**
   - Degree/strength centrality on affinity graph -> local popularity/integration.
   - Betweenness-like brokerage score (approximate on sampled shortest paths for large populations) -> **brokers** connecting otherwise weakly connected groups.
   - Eigenvector/influence proxy from respect+fear+dependency in-weights -> **influencers**.
3. **Leadership candidacy:**
   - `leader_score = influence_in * coalition_size * coordination_events * conflict_success_proxy * legitimacy(respect-trust) - volatility`.
   - Compare within detected community and within formal gang (§6) for **gang leaders** / informal leaders.
4. **Isolation and vulnerability:**
   - `isolation = low affinity in-degree/out-degree + low interaction frequency + recent rejection/bullying tags`.
   - `vulnerability = isolation + low physical/social protection + high victimization history + high fear + debt pressure + medical/needs fragility signals` (needs/health from other systems).
5. **Role classification outputs (emergent):**
   - **Friends:** mutual high bond, reciprocal positive memories, repeated cooperation.
   - **Rivals:** high rivalry/competition + comparable status/influence + repeated contest interactions.
   - **Enemies:** mutual or asymmetric high hostility/resentment/threat, prior attacks/betrayals.
   - **Allies:** cooperative coalition behavior (shared defense/trade/support), may be situational and group-scoped.
   - **Influencers:** high incoming influence weights and opinion/action propagation impact.
   - **Leaders:** high leader_score with follower compliance/support signals.
   - **Isolated prisoners:** persistent low integration across windows.
   - **Vulnerable prisoners:** high vulnerability composite and exploitation exposure.
   - **Informants (inference, not certainty):** behavioral/epistemic anomaly score — unusual private contact with staff, reduced in-group trust, information-leak correlations, protective treatment signals — emitted as `Suspicion`/risk flag with confidence, never ground truth except confirmed investigation events.
   - **Brokers:** high brokerage across communities/gangs/markets.
   - **Gang leaders:** top leadership within a `GroupId` that qualifies as gang (§6), possibly contested.

```rust
#[derive(Clone, Debug)]
pub struct DetectedSocialRole {
    pub agent: AgentId,
    pub role: SocialRoleKind,
    pub score: f32,
    pub confidence: f32,
    pub evidence: Vec<String>, // human/debug explainability hooks
    pub window_start: SimTime,
    pub window_end: SimTime,
}

#[derive(Clone, Debug)]
pub enum SocialRoleKind {
    FriendClusterMember(GroupId), Rival, Enemy, Ally, Influencer, Leader,
    Isolated, Vulnerable, InformantSuspect, Broker, GangLeader(GroupId),
}
```

### 5.3 Outputs and consumers
- Publish `SocialStructureSnapshot` to event/history and caches used by AI context builders.
- Prisoner AI uses roles for goal selection (seek allies/protection, avoid enemies, challenge leader).
- Guard/Warden use vulnerability/tension/informant-suspect signals for supervision and investigations (as uncertain intelligence, not proof).
- UI renders network overlays, role badges with confidence, and trend lines.

---

## 6. Gang system — §7

Gangs are emergent `Group` entities formed from sustained social/economic/protection patterns; they can appear, merge, split, and dissolve.

### 6.1 Data model

```rust
#[derive(Clone, Debug)]
pub struct Gang {
    pub id: GroupId,
    pub name: Option<String>,          // may be adopted/emergent label
    pub formed_at: SimTime,
    pub members: HashMap<AgentId, MemberRecord>,
    pub hierarchy: HierarchyState,
    pub culture: GangCulture,
    pub resources: GangResources,
    pub rivalries: HashMap<GroupId, RivalryState>,
    pub alliances: HashMap<GroupId, AllianceState>,
    pub recruitment: RecruitmentState,
    pub internal_disputes: Vec<InternalDispute>,
    pub reputation: GangReputation,
    pub goals: Vec<GroupGoal>,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub struct MemberRecord {
    pub joined_at: SimTime,
    pub rank: Rank,
    pub commitment: f32,      // 0..1 loyalty to gang
    pub influence: f32,       // within-gang influence
    pub grievances: f32,
    pub expected_protection: f32,
    pub contributions: f32,   // labor, intimidation, trade, coordination
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rank { Leader, Lieutenant, Enforcer, Member, Associate, Prospect }

#[derive(Clone, Debug, Default)]
pub struct HierarchyState {
    pub leader: Option<AgentId>,
    pub council: Vec<AgentId>,
    pub succession_pressure: HashMap<AgentId, f32>,
    pub legitimacy: f32, // perceived fairness/strength/tradition blend
}

#[derive(Clone, Debug, Default)]
pub struct GangCulture {
    pub violence_norm: f32,
    pub secrecy_norm: f32,
    pub solidarity_norm: f32,
    pub exploitation_norm: f32,
    pub ideological_tags: Vec<String>, // shared background/cause tags (emergent)
}

#[derive(Clone, Debug, Default)]
pub struct GangResources {
    pub contraband_access: f32, // abstracted index
    pub commissary_credit_pool: f32,
    pub protection_capacity: f32,
    pub information_network: f32,
    pub territory_control: HashMap<LocationId, f32>,
}

#[derive(Clone, Debug)]
pub struct RivalryState { pub intensity: f32, pub causes: Vec<String>, pub since: SimTime }
#[derive(Clone, Debug)]
pub struct AllianceState { pub strength: f32, pub terms: Vec<String>, pub since: SimTime }
#[derive(Clone, Debug, Default)]
pub struct RecruitmentState { pub targets: Vec<AgentId>, pub pressure_tactics: f32, pub openness: f32 }
```

### 6.2 Emergent formation and evolution
A gang emerges when clustering + coordination signals exceed thresholds over a time window:
- **Formation drivers (from §7):** mutual protection needs (high fear/vulnerability), shared background tags, repeated friendships/cooperation, power ambition, common enemies, shared economic interest (trade/protection rackets abstracted), or recruitment chains from proto-groups.
- **Formation algorithm (periodic):**
  1. Identify dense affinity clusters with sustained interaction and coordination events.
  2. Compute `group_cohesion`, `external_threat`, `resource_motive`, `leadership_presence`.
  3. If criteria met and no adequate existing gang absorbs cluster, instantiate `Gang` with provisional hierarchy (highest leader_score, fallback council).
  4. Emit `GangFormed` event with causal factors for history/UI.

**Evolution processes:**
- **Recruitment:** target isolated/vulnerable or resource-valuable prisoners; success depends on protection offered, fear of rivals, relationship to recruiters, and individual goals. May be voluntary or coercive pressure (modeled as social pressure, not explicit instruction).
- **Attrition/exit:** sentence changes, transfers, protection failure, value conflict, fear of enforcement, better alternatives; commitment decays.
- **Leadership change:** challenge when legitimacy low, leader weakened/removed, or rival claimant's support exceeds threshold; may be orderly succession or disruptive dispute.
- **Fracture/merge:** sustained internal dispute + subcoalition brokerage break => split; strong alliance + overlapping territory/goals + leadership agreement => merge.
- **Alliances/rivalries:** updated by shared enemies, trade compatibility, territory overlap, betrayals, and violent incidents.

### 6.3 Individual motivations vs gang leadership
A gang is not a single mind. Action selection for member `m` in gang context:

```rust
pub struct ActionUtilityInput<'a> {
    pub self_goals: &'a [Goal],        // safety, status, profit, loyalty, survival, autonomy...
    pub self_needs: &'a NeedState,
    pub self_personality: &'a Personality,
    pub gang_order: Option<&'a GroupDirective>,
    pub gang_pressure: f32,            // coercion, expected punishment, dependency
    pub relationship_to_leader: f32,
    pub personal_risk: f32,
    pub moral_discomfort: f32,         // proxy from empathy/discipline/beliefs
}

pub fn resolve_member_choice(input: ActionUtilityInput) -> MemberDecision {
    let comply = w_loyalty(input)*input.relationship_to_leader
               + w_fear(input)*input.gang_pressure
               + w_benefit(input)*expected_gain(input)
               - w_autonomy(input)*goal_conflict(input)
               - w_risk(input)*input.personal_risk;
    let refuse = w_self(input)*goal_conflict(input) + w_moral(input)*input.moral_discomfort
               - w_retaliation_fear(input)*input.gang_pressure;
    let covert = opportunity_for_deception(input); // fake compliance, leak, stall
    argmax(comply, refuse, covert, negotiate)
}
```

Outcomes: compliance, quiet refusal, negotiation, covert sabotage/defection, or challenge. Repeated divergence drives `grievances`, dispute creation, demotion/expulsion attempts, or leadership crises. The system must allow loyal members, reluctant members, opportunists, and defectors within the same gang.

### 6.4 Internal disputes
```rust
#[derive(Clone, Debug)]
pub struct InternalDispute {
    pub id: u64,
    pub parties: Vec<AgentId>,
    pub kind: DisputeKind, // leadership, resource_share, discipline, accusation, strategy
    pub intensity: f32,
    pub started_at: SimTime,
    pub public: bool,
}
```
Disputes modify commitment, create rival subfactions, and can trigger conflict events (§7) or fracture.

---

## 7. Conflict, violence, and discipline — §8

Conflicts are resolved as **abstract stateful encounters** with explainable inputs and outcomes, not predetermined scripts.

### 7.1 Conflict sources and incident taxonomy
Sources: interpersonal tension, rivalry, disrespect/humiliation, scarcity/competition, emotional dysregulation, debts/theft, gang pressure, rumor/suspicion (including snitch accusations), policy friction, and environmental stress.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IncidentKind {
    Argument, Threat, Intimidation, Theft, Bullying, Fight,
    GangDispute, GroupDisturbance, RefusalToComply, PropertyDamage,
    Protest, MedicalEmergency, // medical emergency can be cause or consequence
}
```

### 7.2 Escalation model (abstract)
Each potential/active incident tracks an escalation stage:
`LatentTension -> VerbalExchange -> ThreatDisplay -> PhysicalConfrontation -> GroupInvolvement/Crisis -> Contained/Resolved`

Transitions are probabilistic-utility steps evaluated at interaction opportunities:

```rust
pub struct EscalationInput {
    pub instigator: AgentProfileView,
    pub target: AgentProfileView,
    pub relationship: RelationshipEdge,        // directional views
    pub emotional_state: EmotionalState,       // anger, fear, shame, stress
    pub physical_state: PhysicalState,         // health, fatigue, injury, strength proxy
    pub audience: Vec<AgentId>,
    pub staff_presence: StaffPresence,
    pub location_risk: f32,                    // visibility, chokepoints, weapons/tools access proxy
    pub policy: PolicyFlags,
    pub recent_triggers: Vec<SocialMemoryTag>,
    pub gang_context: Option<GangContext>,     // backup, orders, rival presence
}

pub enum EscalationChoice { Deescalate, Persist, Escalate, SeekHelp, Disengage, Comply }
```

**Selection factors (must consider per §8):** personality (impulsiveness, aggression, discipline, empathy), emotional state, physical condition, relationships (fear/respect/hostility/dependency), environment, staff presence/intervention likelihood, prison policies, and previous events/memories. High fear of consequences or strong staff deterrence can suppress escalation; humiliation before witnesses or gang backup can amplify it.

### 7.3 Outcome resolution
When escalation reaches confrontation, resolve via contested abstract roll influenced by capabilities and context, producing one of: `Avoided, VerbalWin, IntimidationSuccess, TheftSucceeded, FightMinor, FightSerious, GroupRiotFragment, StaffIntervention, MutualDeterrence, MedicalComplication`.

```rust
pub struct ConfrontationResult {
    pub kind: IncidentKind,
    pub participants: Vec<ParticipantOutcome>,
    pub injuries: Vec<InjuryRecord>,          // abstract severity tiers
    pub property: Vec<PropertyEffect>,
    pub witnesses: Vec<AgentId>,
    pub staff_actions: Vec<StaffActionRecord>,
    pub discovered_by: Vec<AgentId>,         // feeds reports/epistemics
    pub explanation: Vec<CauseFactor>,        // for logs/UI/debug
}
```

Outcome probability is bounded and situational; no action is universally beneficial or consequence-free. Even "winners" may incur injuries, discipline, retaliation risk, or reputational costs.

### 7.4 Consequences pipeline
Every resolved incident emits follow-on effects (immediate and delayed):
- **Medical:** injury/illness treatment needs, recovery time, capacity load (coordinates with health/medical systems).
- **Disciplinary:** warnings, privilege loss, segregation/lockdown placement requests, sentence/parole-relevant records, hearings (validated/applied by institutional systems).
- **Social/psychological:** relationship deltas (fear, respect, resentment), trauma/stress changes, status shifts.
- **Gang:** retaliation obligations, cohesion or fracture, territory tension, leadership credibility changes.
- **Institutional:** incident reports (possibly delayed/incomplete), investigations, staff stress/morale effects, policy pressure signals to warden.
- **Epistemic:** witnesses form direct knowledge/rumors; uninvolved agents may receive distorted rumors.

```rust
pub fn apply_incident_consequences(world: &mut WorldFacade, result: &ConfrontationResult) {
    record_history(result);
    update_relationships_from_incident(result);
    enqueue_medical_needs(result);
    propose_discipline_cases(result);     // warden/staff systems adjudicate within authority
    update_gang_states(result);
    distribute_information(result);       // epistemic updates per observer
    emit_reputation_and_policy_signals(result);
}
```

Disciplinary authority and due-process mechanics are owned by warden/guard/institution systems; social system provides evidence summaries, participant/witness context, and risk signals.

---

## 8. Escape attempts — §16

Escape attempts are **emergent, abstract simulation events** with multi-stage motivation/planning/execution modeling and persistent consequences.

### 8.1 Motivation triggers
Compute an `escape_motivation` score per prisoner over time (sampled periodically or on trigger events):

```rust
pub struct EscapeMotivation {
    pub total: f32, // 0..1
    pub factors: EscapeFactors,
}

pub struct EscapeFactors {
    pub fear_for_safety: f32,
    pub desperation_hopelessness: f32, // low hope, severe needs, isolation
    pub sentence_pressure: f32,        // long remaining, recent denial/setback
    pub relationship_pull: f32,        // outside ties, protective separation, coercion
    pub perceived_injustice: f32,
    pub institutional_conditions: f32, // abuse/neglect perception, chaos, harshness
    pub opportunity_belief: f32,       // perceived security weakness (epistemic)
    pub risk_tolerance: f32,           // personality
}
```

Triggers: acute threat, gang coercion, disciplinary crisis, parole failure, family emergency belief (true or rumor), degrading conditions, or observed security lapse. High discipline, hope, strong bonds inside, or high perceived risk suppress motivation.

### 8.2 Attempt lifecycle (abstraction)
`Idle -> Ideation -> InformationGathering/Preparation -> WindowSelection -> AttemptExecution -> Outcome`

- **Preparation:** abstract progress accumulated through feasible actions (routine observation, acquiring tools/access via social/economic interactions, recruiting accomplices) constrained by surveillance, staff, schedules, and opportunity. Represent as hidden/partially observable state plus epistemic traces.
- **Detection risk:** increases with preparation visibility, informant suspicion signals, staff competence/surveillance, and inconsistent behavior; may produce preemption (`AttemptDiscovered` before execution).
- **Execution window:** chosen when `motivation * perceived_success * opportunity_window > caution + deterrence`.

```rust
pub struct EscapeAttemptPlan {
    pub actors: Vec<AgentId>,      // solo or group
    pub target_vector: EscapeVector, // perimeter breach, during transport, work detail, etc. abstract
    pub readiness: f32,
    pub secrecy: f32,
    pub support: f32,              // accomplices/resources
    pub planned_at: SimTime,
}

#[derive(Clone, Copy, Debug)]
pub enum EscapeVector { Perimeter, Transport, WorkDetail, VisitorArea, StaffCompromise, Other }
```

### 8.3 Outcome modeling
Resolve execution as contextual contested event (not deterministic):

```rust
pub enum EscapeOutcome {
    Prevented,              // stopped during attempt
    DiscoveredPreAttempt,   // plot uncovered
    DiscoveredDuring,       // attempt interrupted and attributed
    DisruptedNoEscape,      // chaos/disruption but no exit
    PartialBreachContained, // reached restricted boundary then contained
    SuccessfulEscape,       // rare, state-dependent
}

pub struct EscapeResolutionInput {
    pub readiness: f32,
    pub secrecy: f32,
    pub security_conditions: f32, // staffing, locks, surveillance, maintenance state
    pub staff_response: f32,      // alertness, competence, coordination, fatigue
    pub policy_posture: f32,
    pub facility_condition: f32,
    pub environmental_factors: f32,
    pub chance: f32,              // bounded randomness after state weighting
}
```

`P(success)` is generally low and requires compounded favorable conditions (low security effectiveness, high readiness/secrecy, delayed/poor response, favorable window). Randomness may perturb but not override dominant state causes; explanation factors are logged.

### 8.4 Consequences
- **Immediate:** injuries, lockdowns, medical load, staff actions, participant capture status.
- **Disciplinary/legal abstraction:** charges/segregation/privilege impacts, parole implications (institution-managed).
- **Social:** heroization or resentment among prisoners, fear shifts, gang credibility changes, informant suspicion/retaliation risks.
- **Institutional (warden-facing signals):** investigations, security reviews, staffing/policy revisions, political/reputation pressure, maintenance fixes.
- **Persistent world effects:** successful escape creates fugitive/case record in world history; later recapture/closure possible via events system.

All escape-related information propagates through epistemics (some agents know facts, many know rumors).

---

## 9. Interactions with other systems

| System | Consumes from Social | Provides to Social |
|---|---|---|
| Prisoner AI / Decision | relationship summaries, role flags, epistemic context, gang pressures, conflict/escape options | intents, chosen interactions, dialogue/communication acts, compliance/refusal |
| Needs & Psychology | — (indirect via decisions) | hunger/fatigue/safety/belonging/stress/hope, health/mental condition modifiers |
| Schedules & Locations | meeting opportunities, congregation hotspots | who can interact, where, supervision level, routine disruptions |
| Guard AI & Staff | threat/vulnerability/informant-suspect signals (uncertain), incident context | presence, interventions, searches, reports, relationship actions (help/abuse), enforcement |
| Warden AI | tension indices, gang stability, conflict/escape risk, social climate metrics | policies, staffing levels, program access, segregation decisions, negotiations |
| Economy | trade/protection interactions as abstract events | scarcity, commissary/resources, wages/program incentives affecting debts/dependency |
| Facilities/Map | territory/control candidates, incident locations | capacity, visibility, security hardware state, restricted areas |
| Event System | emitted `SocialEvent`s/alerts | external shocks (arrivals, shortages, lockdowns) as inputs |
| Memory & History | key social memories and institutional records | retrieval windows, consolidation policies, historical summaries |
| UI/Visualization | graphs, profiles, gang/org views, incident timelines, explanations | inspection queries/filters |
| Persistence | serializable social/gang/epistemic state | load/save, migrations/versioning |

**Contract style:** other systems call query APIs (snapshots/views) and submit validated interaction intents; social system returns outcome events/deltas. Avoid circular synchronous mutation; prefer event-queue application within tick phases.

---

## 10. Update cadence, performance, and explainability

- **Continuous/lightweight:** apply event-driven relationship deltas and epistemic inserts on interactions/incidents.
- **Frequent (per short interval):** decay, local tension updates, small rumor propagation steps in active areas.
- **Periodic (medium interval):** recompute local role scores, gang health/cohesion, recruitment pressure, escape motivation.
- **Periodic (long interval / on threshold):** global community detection, broker/influence recomputation, gang formation/split/merge evaluation.
- **Scalability:** cap edges per agent by salience/pruning, use spatial/social bucketing to limit candidate interaction pairs, approximate centralities at large N, and summarize stale weak ties.
- **Explainability:** every major detection/outcome stores top contributing factors (inputs, weights, sampled chance) for logs, debugging, and UI "why" panels.
- **Validation:** reject impossible actions (unknown actors/locations, dead/removed agents), clamp all scalars, and ensure relationship updates remain bounded.

---

## 11. Persistence and history

Persist as part of world save (§23): relationship edges (or compacted significant edges + reconstruction hints), social memory logs (tiered), epistemic states (pruned), gangs/orgs and disputes, detection snapshots (optional/cacheable), and incident/escape records linked to global event IDs. On load, rebuild indices (`by_subject`, caches) and resume decay timers from `SimTime`.

History integration (§18): promote major betrayals, leadership changes, riots, gang wars, investigations, and escape outcomes into institutional history with causal chains.

---

## 12. Implementation notes for downstream agents

- Treat "informants" as **suspected roles** in social detection unless corroborated by adjudicated events; never leak omniscient truth into prisoner epistemics.
- Keep attraction/romance modeling configurable and policy-appropriate to setting/tone; relationship dimension exists but behavior enablement can be gated by simulation rules.
- Model contraband, weapons, and explicit criminal methods only at the same abstraction level as the rest of the simulation; social system tracks influence/access/risk signals, not procedural detail.
- Prefer additive, inspectable state changes over opaque black-box scores so testing and balancing can target specific subsystems.
