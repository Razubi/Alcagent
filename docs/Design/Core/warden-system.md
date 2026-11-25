# Warden System Design — Institution Management Layer

**Project:** Project Alcagent — Autonomous Living Prison Simulation (Rust)
**Scope:** Directive §9 (AI Warden), §10 (AI Guard Subagents), §11 (Prison Administration & Finances), §12 (Prison Expansion), plus §13-adjacent administration (intake/population/admission decisions as they touch institutional management) and §15/§16 touchpoints where the warden acts (parole recommendations, disturbance/escape response).
**Status:** Design only. No source files are modified by this document.
**Companion docs (expected):** `architecture-report.md (removed)` (not yet present at time of writing — skipped). Integration contracts here are written to be compatible with whatever architecture report lands; if it conflicts, the architecture report wins on module placement, this document wins on behavior.

---

## 0. Document Map

| § | Content | Directive |
|---|---------|-----------|
| 1 | Design principles & layered architecture | §2, §19, §22 |
| 2 | Shared data foundations & conventions | §22 |
| 3 | Warden AI — data model | §9 |
| 4 | Warden AI — knowledge model (incomplete information) | §9 |
| 5 | Warden AI — decision engine specification | §9, §19 |
| 6 | Warden AI — memory, decision journal, corruption | §9, §18 |
| 7 | Guard subagents — data model | §10 |
| 8 | Guard subagents — posts, shifts, assignment | §10 |
| 9 | Guard subagents — duty loop, authority matrix, autonomous response | §10 |
| 10 | Guard → Warden reporting pipeline | §10 |
| 11 | Administration & finances — ledger model | §11 |
| 12 | Administration & finances — revenue/expense formulas | §11 |
| 13 | Administration & finances — budget process & financial crises | §11 |
| 14 | Expansion — building catalog, construction lifecycle, prerequisites | §12 |
| 15 | Expansion — scaling consequences & administrative capacity | §12 |
| 16 | Population & intake administration (§13-adjacent) | §13 |
| 17 | Integration points — Social Systems, Prison Simulation, AI Systems, Economy, UI, Persistence, Events | §6–§8, §13–§20 |
| 18 | Configuration, tuning constants, feature flags | §22 |
| 19 | Testing strategy & failure-mode analysis | §24 Phase 4 |
| 20 | Implementation phasing & file ownership | §24, §25 |
| 21 | Open questions / decisions deferred | — |

---

## 1. Design Principles & Layered Architecture

### 1.1 Non-negotiable principles

1. **The warden is not omniscient.** Every decision is made against a *WardenView* — a projection of world state assembled only from reports, confirmed facts, rumors, and decaying memory. The simulation engine never hands the warden raw agent state.
2. **Decisions are proposed, never applied.** AI and utility layers emit `InstitutionCommand`s. The deterministic engine validates each command against preconditions, budget, authority, and cooldowns. Invalid commands are rejected and fed back as negative feedback to the decision-maker — never silently applied.
3. **Guards are autonomous within authority.** Guards do not wait for warden orders for routine duties or emergencies. The warden sets policy, assignment, and posture; guards execute and report.
4. **Deterministic where cheap, AI where valuable.** Ledger accrual, construction progress, fatigue accrual, and report generation are deterministic. Utility AI handles frequent cheap decisions. LLM subagents are reserved for strategic reasoning, negotiation dialogue, and investigations — on a slow cadence, schema-validated, with deterministic fallback.
5. **Consequences are coupled outward.** Every institution-layer action that touches prisoners/staff must ride the same event bus other systems consume (Social, Simulation, Economy, History). No side-channels.
6. **Failure is a legitimate outcome.** The prison can go bankrupt, lose its charter, or descend into unrest. The systems must model degradation, not just success paths.

### 1.2 Layered architecture

```
┌─────────────────────────────────────────────────────────────┐
│ OBSERVER/UI LAYER        §17.6  dashboards, incident stream │
├─────────────────────────────────────────────────────────────┤
│ AI ORCHESTRATION LAYER   §17.3  prompts, context, validation│
│   LLM subagent (strategic) ──► validated InstitutionCommand │
│   Utility AI (frequent)    ──► scored InstitutionCommand    │
│   Reflex rules (emergencies)──► deterministic InstitutionCmd│
├─────────────────────────────────────────────────────────────┤
│ INSTITUTION MANAGEMENT LAYER  (this document)               │
│   WardenAgent · GuardAgent · PostRegistry · ReportPipeline  │
│   Ledger · BudgetPlan · ConstructionSite · PopulationAdmin   │
├─────────────────────────────────────────────────────────────┤
│ SIMULATION CORE (deterministic)                             │
│   time · needs · schedules · relationships · events · save  │
└─────────────────────────────────────────────────────────────┘
```

### 1.3 Module layout (proposed, crate-local)

```
src/
  institution/
    mod.rs                 // facade: InstitutionSystem::tick()
    warden/
      agent.rs             // WardenAgent, personality, objectives
      knowledge.rs         // KnowledgeStore, claims, confidence
      decision.rs          // DecisionEngine, utility scoring
      journal.rs           // DecisionJournal, institutional memory hooks
      actions.rs           // CandidateAction catalog & preconditions
    guard/
      agent.rs             // GuardAgent, ranks, traits
      post.rs              // PostType, PostInstance, shift rotation
      duties.rs            // duty loop, patrol, observation
      authority.rs         // AuthorityLevel matrix, escalation
      response.rs          // autonomous incident response FSM
      morale.rs            // fatigue/stress/burnout/misconduct risk
    report/
      mod.rs               // Report, ReportKind, credibility scoring
      pipeline.rs          // event → observation → report → inbox
    finance/
      ledger.rs            // Account, Ledger, accrual
      budget.rs            // BudgetPlan, allocations, forecasts
      revenue.rs           // funding formulas, grants, penalties
      expenses.rs          // expense formulas, degradation curves
      crisis.rs            // deficit, insolvency, emergency measures
    expansion/
      catalog.rs           // BuildingSpec table (§14)
      construction.rs      // ConstructionSite lifecycle
      capacity.rs          // population capacity & admin capacity
    population/
      intake_admin.rs      // admission decisions, transfers, releases
    events.rs              // InstitutionEvent enum (bus payloads)
    commands.rs            // InstitutionCommand enum + validation
```

File ownership: this layer owns `src/institution/**`. Simulation core owns time/needs; Social owns relationships; Economy agent (if split out) may co-own `finance/` — coordinate via §17 integration contracts before editing.

---

## 2. Shared Data Foundations & Conventions

- **IDs:** opaque newtypes (`WardenId`, `GuardId`, `BuildingId`, `PostId`, `ReportId`, `ProjectId`), UUIDv7 (time-ordered) for save-friendliness.
- **Scales:** all personality/condition scores are `f32` normalized `0.0..=100.0` unless noted; stored clamped.
- **Time:** all timestamps are simulation ticks (`SimTime`), monotonically increasing. Finance accrues per tick; strategic decisions run on cadence hooks.
- **Determinism:** every random draw flows through a seeded RNG (`Pcg64` or `Xoshiro256`) stored in world state so saves/loads and replays are bit-stable. AI calls are the *only* non-deterministic component and are always behind the validation boundary.
- **Serialization:** all persistent structures derive `Serialize`/`Deserialize` (serde), with `#[serde(default)]` on new fields for save compatibility and explicit schema versioning of the save file.
- **Money:** integer minor units (`i64` cents) everywhere; never floats. Conversion at the UI boundary only.
- **Events:** typed enums on the bus; string topics are UI/debug sugar only.

---

## 3. Warden AI — Data Model (Directive §9)

### 3.1 WardenAgent

```rust
WardenAgent {
    id: WardenId,
    name: String,
    generated_at: SimTime,
    personality: WardenPersonality,     // §3.2
    objectives: WardenObjectives,       // §3.3 — live weights
    beliefs: BeliefSet,                 // §4 — knowledge & stances
    leadership_philosophy: Philosophy,  // derived, cached
    reputation: InstitutionReputation,  // §6.3
    attention: AttentionBudget,         // §15.3 — admin capacity
    corruption_state: CorruptionState,  // §6.4
    journal_ref: JournalId,             // §6.2
    decision_cooldowns: HashMap<ActionKind, SimTime>,
    tenure_days: u32,
    performance_history: Vec<PerformanceSample>, // trailing audit scores
}
```

Exactly one warden is active per institution at a time. Warden succession (firing/resignation/death, corruption scandal) is a supported event that spawns a replacement with a different personality — see §6.4 and §17.1.

### 3.2 WardenPersonality — the eleven directive traits

| Trait | 0 means | 100 means | Primary influence |
|---|---|---|---|
| `strictness` | Permissive, hands-off | Zero-tolerance, punitive | Security weight, privilege severity, discipline policy |
| `compassion` | Indifferent to prisoner welfare | Deep concern for welfare | Welfare weight, medical/food spend, clemency actions |
| `political_ambition` | Content, low profile | Seeks higher office/approval | Reputation weight, public-facing decisions, grand gestures |
| `financial_discipline` | Spends freely, gambles | Hawkish, austerity-prone | Budget stringency, expansion appetite, deficit tolerance |
| `risk_tolerance` | Paralysis under uncertainty | Aggressive bet-taker | Risk aversion λ, construction appetite, security gambles |
| `reform_orientation` | Status quo, punishment-first | Transformative, rehab-first | Rehab/education spend, program introduction, parole leniency |
| `authoritarian_tendencies` | Delegates, empowers staff | Centralized command, distrusts | Delegation rate, negotiation willingness, policy rigidity |
| `staff_loyalty` | Expendable labor | Defender of the guard corps | Firing reluctance, morale spend, backing guards in disputes |
| `corruption_susceptibility` | Incorruptible | Easily bought | Temptation acceptance probability (§6.4) |
| `reputation_concerns` | Ignores public opinion | Image-conscious | PR actions, penalty avoidance, audit investment |
| `competence` | Bungles, misjudges | Crisp, well-calibrated | Decision quality, info-processing error rate, plan coherence |

Generation: traits drawn from a joint distribution with deliberate correlations rather than independent uniforms, so archetypes emerge:

- Authoritarian Reformer: high authoritarian + high reform (reforms imposed top-down, resented)
- Careerist Survivor: high political_ambition + high reputation_concerns + high corruption_susceptibility
- Quiet Competent: high competence + high financial_discipline + low ambition
- Hardliner: high strictness + high authoritarian + low compassion
- Well-Meaning Incompetent: high compassion + high reform + low competence

Implementation: sample from a small set of archetype templates with per-trait jitter, or from a fitted multivariate lognormal if population generation is needed at scale. Trait correlations are config (§18).

**Trait drift.** Traits are not frozen. Extreme experiences nudge them: a riot survived raises `strictness` and `risk_tolerance` modestly; a successful rehab cohort raises `reform_orientation`; a bribery scandal and exoneration raises `reputation_concerns`. Drift is small (±0.5–2.0 per formative event, clamped) and logged to the journal as *character development*, visible in the UI.

**Derived leadership philosophy** (cached enum, recomputed on drift):
`Punitive | Custodial | Rehabilitative | Political | Managerial | Reformist | CorruptPatron` — used for UI labels and as a prior in AI prompt context; never used to hard-code behavior.

### 3.3 WardenObjectives — competing, trait-weighted

Eleven objectives are tracked as **live satisfaction signals** `s_i ∈ [0,100]`, updated from world state each tactical tick:

| Objective | Fed by (examples) |
|---|---|
| `security` | staffing coverage, contraband rate, escape risk index, lockdown discipline |
| `financial_sustainability` | cash runway, burn rate, deficit events, audit score |
| `staff_morale` | guard morale mean/variance, turnover, overtime load, misconduct |
| `prisoner_welfare` | food/medical quality, needs satisfaction, deaths, illness |
| `public_reputation` | media sentiment, scandal count, awards, escape publicity |
| `institutional_stability` | incident rate, protest frequency, unresolved grievances |
| `rehabilitation` | program completions, recidivism markers, parole-ready count |
| `escape_prevention` | escape risk index, attempt count, post coverage of weak points |
| `violence_reduction` | assault rate, gang tension, injury count |
| `political_pressure` | oversight heat, funding-review status, ministerial attention |
| `facility_expansion` | utilization %, intake backlog, construction milestones |

Each objective also carries a **weight** `w_i` used by the decision engine:

```
w_i = base_i(config) × trait_modifiers_i(personality) × context_modifiers_i(state)
```

Example modifiers (illustrative, §18-tunable):

- `security` × (1 + strictness/100) × (1 + authoritarian/200)
- `prisoner_welfare` × (1 + compassion/100)
- `rehabilitation` × (1 + reform_orientation/100)
- `public_reputation` × (1 + reputation_concerns/100) × (1 + political_ambition/150)
- `financial_sustainability` × (1 + financial_discipline/100)
- `facility_expansion` × (1 + risk_tolerance/150) × (1 − financial_discipline/200)
- `escape_prevention` × (1 + risk_tolerance/100), `violence_reduction` × (1 + compassion/150)
- `political_pressure` is inverted as *relief*: actions that lower pressure gain utility proportional to current pressure level.

Weights are normalized to sum 1.0 per decision context so scores are comparable across situations. Conflict is intentional and preserved — no objective may be zeroed out entirely, so no warden ever ignores welfare completely, only under-weights it.

**Context modifiers** make the same warden behave differently under crisis: e.g., during an active disturbance, `security` and `escape_prevention` weights are temporarily ×2.5 and `facility_expansion` ×0.1. Crisis multipliers are config tables keyed by `WorldCondition::Crisis(kind)`.

### 3.4 WardenGoals (long-horizon intent)

Distinct from per-decision objectives, the warden holds 1–3 active **goals** with horizons (quarter/year): e.g., "Reach 92% occupancy without a major incident", "Bring the medical wing up to standard", "Exit the deficit before inspection". Goals are set at strategic cadence by the AI subagent, validated for reachability, scored for alignment with objectives, and recorded in the journal. Goals bias candidate-action generation (§5.4) — e.g., a construction goal raises the salience of expansion actions in the candidate set. Goal failure/completion is a journal event and nudges trait drift.

---

## 4. Warden Knowledge Model — Incomplete Information (Directive §9)

### 4.1 Principle

The warden models the institution through **claims**, never through raw world state. A claim is a proposition with provenance and confidence. Any decision-time snapshot (`WardenView`) contains only what the warden knows, suspects, or has been told.

### 4.2 Core types

```rust
Claim {
    id: ClaimId,
    subject: ClaimSubject,          // Prisoner(id) | Guard(id) | Wing(id) | Institution | Incident(id)
    predicate: ClaimPredicate,      // e.g. ContrabandPossession, GangLeadership, Injury, PolicyViolation...
    polarity: bool,
    confidence: Confidence,         // 0.0..=1.0
    source: ClaimSource,            // Report(ReportId) | Observation | Inference | Rumor | Hearsay | CommonKnowledge
    standing: ClaimStanding,        // Confirmed | Probable | Suspected | Rumored | Disputed | Refuted
    acquired_at: SimTime,
    expires_at: Option<SimTime>,    // staleness decay
}

WardenView {                       // built per decision cycle; immutable snapshot
    time: SimTime,
    population: PopulationStats,    // counts/occupancy are institution-level facts, always known
    finances: LedgerSnapshot,       // the warden is always owed the ledger summary
    staffing: StaffingSnapshot,
    facilities: FacilitySnapshot,
    incidents: Vec<IncidentDigest>, // summarized, confidence-annotated
    claims: Vec<Claim>,             // everything above threshold + top-N by salience
    objectives: Vec<(Objective, f32)>,
    active_goals: Vec<Goal>,
    recent_decisions: Vec<JournalEntry>,
}
```

Institution-level aggregates (population, cash, staffing counts, occupancy) are treated as **common knowledge** — the warden is owed accurate books. Everything *about persons* is claim-mediated: who is injured, who leads which gang, who brought contraband in. Patient zero for staleness: confidence decays multiplicatively (e.g., ×0.97 per day) unless refreshed by a matching report, and expiry demotes `Probable → Suspected → Rumored`.

### 4.3 Information pipelines into the warden

1. **Guard reports** (§10) — the dominant channel. Carry credibility (§10.4).
2. **Staff & committee briefings** — scheduled periodic summaries (security chief, medical officer, budget officer). Generated deterministically from sim state with noise injected toward the briefing officer's competence/loyalty. These are the main anti-drift mechanism: without them the warden could be catastrophically wrong for weeks.
3. **Prisoner representatives** — negotiation channel delivers grievances/claims from below (§17.1).
4. **Investigations** — the warden's instrument for converting `Rumored/Suspected` into `Confirmed/Refuted` (§5.6).
5. **Rumor mill** — stochastic claim generation from world events, weighted by salience and network centrality of originators; rumors mutate as they propagate (Social Systems owns propagation; warden only receives the degraded copies).

### 4.4 Standing transitions

```
Rumored ──(corroborating report / investigation)──► Suspected ──► Probable ──► Confirmed
Confirmed ──(counter-evidence)──► Disputed ──► (resolution) ──► Confirmed | Refuted
any ──(staleness expiry)──► decay one rank; confidence *= decay
```

### 4.5 Calibration & the competence trait

The warden's `competence` trait modulates (a) **processing error** — probability a claim is misread/misfiled; (b) **prior weighting** — how strongly new evidence updates beliefs vs. prior conviction; (c) **plan coherence** — how reliably chosen actions achieve intent vs. backfire via side effects. Low competence therefore produces confidently wrong wardens, which is a feature: it generates realistic institutional failure arcs, not random noise.

---

## 5. Warden AI — Decision Engine Specification (Directive §9)

### 5.1 Cadences — three gears, one engine

| Gear | Cadence (configurable) | Trigger | Decision mechanism | Cost |
|---|---|---|---|---|
| **Reflex** | immediate (same tick) | Emergency events (violence, fire, escape alarm, medical emergency) | Deterministic rule table + authority matrix; warden may only *ratify or tighten* posture (lockdown level, medical response) | free |
| **Tactical** | hourly–daily | Schedule/routine, budget deltas, staffing gaps, report triage | Utility scoring over candidate actions (§5.3–5.5), no LLM | cheap |
| **Strategic** | weekly–monthly, or on major event | Goal setting, expansion, program introduction, funding requests, parole posture | LLM subagent (§17.3) with schema-validated plan; utility engine arbitrates conflicts; fallback to utility-only if AI unavailable/invalid | expensive |

All three gears emit the same `InstitutionCommand` type through the same validation pipeline. Strategic gear may *advise*; it may not bypass precondition checks.

### 5.2 Decision loop (per directive §9: "decisions from incomplete information")

```
1. OBSERVE   — build WardenView (§4.2). Apply fog-of-war filters.
2. ASSESS    — estimate satisfaction s_i for each objective; flag crises and deltas.
3. GENERATE  — enumerate candidate actions from ActionCatalog (§5.4),
               filtered by preconditions, cooldowns, budget, authority.
4. EVALUATE  — score each candidate (§5.5) under uncertainty;
               for strategic gear, LLM proposes and critique-passes a shortlist.
5. SELECT    — argmax utility + personality-weighted stochastic tie-breaking (softmax, temperature by risk_tolerance).
6. VALIDATE  — engine checks preconditions, costs, cooldowns, corruption policy, quorum rules.
7. EXECUTE   — commit; emit InstitutionEvents; schedule follow-ups.
8. LEARN     — outcome observed later (1 tick to N days); update beliefs, journal,
               trait drift, reputation, and calibration stats.
```

Steps 1–6 run synchronously in one decision pass; step 8 is asynchronous via an outcome-tracking queue keyed by `DecisionId` with expected-outcome predictions recorded at decision time (so surprise is computable: |predicted − actual| feeds calibration).

### 5.3 Utility scoring function

```
U(a | V) = Σ_i ŵ_i(V) · Δŝ_i(a, V)          // objective impact, fog-of-war scaled
         − λ_risk · RiskCost(a, personality)  // appetite-modulated risk aversion
         − λ_fin  · FinancialStrain(a, V)     // liquidity-weighted cost pain
         − λ_att  · AttentionCost(a)          // admin capacity load (§15.3)
         + InfoValue(a, V)                    // investigation/briefing bonus when uncertain
         + GoalAlignment(a, V)                // active-goal bias (§3.4)
         + Tier ε (~ N(0, σ(risk_tolerance, competence)))   // tie-breaking noise

where confidence scaling: for any claim-dependent term, multiply by claim.confidence;
expected impact computed over the claim's standing distribution when disputed.
```

`RiskCost` grows with outcome variance, not just downside mean — a high-`risk_tolerance` warden haircuts it. `FinancialStrain` is convex in deficit depth: spending when runway < 30 days hurts far more than spending with 300. `InfoValue` is positive only when the decision's EV swing between belief-states exceeds the cost of the information action — this is what makes investigations *rational under uncertainty* rather than scripted busywork.

### 5.4 Action catalog (the directive's listed actions, fully mapped)

Each action is a struct with: `preconditions`, `cost { cash, attention, cooldown }`, `predicted_effects`, `emits`, `gear`. Actions below are the *complete* directive enumeration plus administrative necessities; anything else is rejected at validation.

| # | Directive action | Command variant | Gear | Key preconditions | Notable effects / side effects |
|---|---|---|---|---|---|
| 1 | Expand the prison | `CommissionExpansion { spec }` | Strategic | funds or financing, land/space, capacity demand ≥ threshold | capacity ↑ (delayed), intake backlog relieved, staffing need ↑ (§14, §15) |
| 2 | Hire guards | `HireStaff { role, count }` | Tactical | payroll headroom, recruiting pool | coverage ↑, morale via relieved overtime, payroll ↑ |
| 3 | Fire guards | `TerminateStaff { ids, reason }` | Tactical | cause or restructure justification | payroll ↓, morale ↓, loyalty risk, reputation ± |
| 4 | Promote staff | `PromoteStaff { id, to_rank }` | Tactical | vacancy, competence/experience check | morale ↑ for promotee, coverage of senior posts |
| 5 | Change policies | `SetPolicy { PolicyId, value }` | Tactical/Strategic | — | prisoner behavior & incidents shift (Simulation); audit effects |
| 6 | Increase/reduce privileges | `AdjustPrivileges { cohort, deltas }` | Tactical | cohort exists | welfare/morale ±, infraction risk ± |
| 7 | Introduce rehabilitation programs | `IntroduceProgram { kind, budget }` | Strategic | funding, facilities, staff | rehab objective ↑ over weeks; parole-ready pipeline ↑ (§17.2) |
| 8 | Change work assignments | `ReassignWork { cohort | id }` | Tactical | — | income (contracts) ±, satisfaction ±, gang co-location effects |
| 9 | Increase security | `RaiseSecurityPosture { level }` | Tactical | — | escape risk ↓, welfare/welfare-cost ↑, staff stress ↑ |
| 10 | Reduce expenses | `CutExpense { category, pct }` | Tactical | — | burn ↓; quality/welfare/morale degradation curves (§12) |
| 11 | Improve infrastructure | `CommissionMaintenance { scope }` | Tactical | funds | infrastructure condition ↑; failure odds ↓ |
| 12 | Respond to disturbances | `RespondToDisturbance { mode }` | **Reflex** | active disturbance | deterministic posture selection + guard FSM handoff (§9) |
| 13 | Negotiate with prisoner representatives | `OpenNegotiation { channel, agenda }` | Tactical | acknowledged rep, stalemate or grievance backlog | grievance resolution; requires Social Systems channel (§17.1) |
| 14 | Investigate misconduct | `OpenInvestigation { target, scope }` | Tactical | suspicion confidence > τ | converts rumor→fact; attention cost; target stress; if target is staff: integrity exposure |
| 15 | Adjust parole recommendations | `SetParoleRecommendation { prisoner, stance }` | Tactical | eligibility window | influences (never decides) the independent parole board (§17.2) |
| 16 | Request additional funding | `RequestFunding { ask, justification }` | Strategic | political capital, scandal/incident leverage | grant settled by oversight AI/module; failure raises political_pressure |
| + | Approve/deny construction milestones, set budget allocations, trigger lockdown, declare emergency, censor/authorize mail, order audit, schedule inspection | additional administrative commands | mixed | — | required to make the above coherent |

### 5.5 Candidate generation details

- Generation is **constraint-first**: preconditions and cooldowns prune the space before scoring. Typical candidate set: 20–80 actions.
- **Salience boosting:** active goals (§3.4) and crises add their aligned actions to the front of the shortlist; nothing is scored below the visibility cutoff.
- **Repeat-suppression:** the same action on the same target within its cooldown is excluded; oscillation damping penalizes reversing a recent decision by >u20% in utility unless state changed materially (this kills dithering without forbidding adaptation).
- **Dignity of refusal:** sometimes the correct decision is `NoAction { rationale }`. Explicit "hold steady" is a first-class citizen, scored like any action.

### 5.6 Investigations as first-class decisions

`OpenInvestigation` deserves detail because it closes the incomplete-information loop:

```
Investigation {
    target: ClaimSubject, scope: Scope, assigned: Investigator (guard sergeant+ or external),
    budget, started_at, eta, status: Open|Reported|Closed,
    yields: Vec<Claim>  // written back into KnowledgeStore with upgraded standing
}
```

Outcomes: `Confirmed`, `Refuted`, `Inconclusive` (no information gain, attention wasted), `Compromised` (leaked — target warned via rumor mill, confidence of all claims vs. target −30). Corrupt wardens may open *sham* investigations to harass enemies — visible to astute observers, no information gain, morale/reputation cost. Investigation is also the only reliable counter to the corruption state (§6.4): audits triggered by outside oversight or loyal lieutenants surface embezzlement.

### 5.7 Validation pipeline (engine-side, non-negotiable)

```
InstitutionCommand → schema check → authority check (warden scope) → precondition check
→ budget check (can the ledger pay?) → cooldown check → corruption/policy audit (§6.4)
→ apply → emit events → record JournalEntry(decision, rationale_or_"utility: X", predicted_effects)
```

Rejected commands return `RejectionReason` to the caller; strategic AI retries ≤2 times with the reason in context, then falls back to utility-only selection. Repeated rejection is logged as an AI fault for tooling/debug (§19).

---

## 6. Warden Memory, Journal & Corruption (Directive §9, §18)

### 6.1 Memory layers

- **Short-term:** last N decisions + their pending outcome checks (ring buffer, in-memory + save).
- **Long-term:** personally formative entries — first riot, first successful negotiation, a scandal, a beloved guard's death — promoted by salience, never fully expired.
- **Institutional memory:** the shared prison history lives in world history (§18 directive); the warden holds *references* (`HistoryRef`) rather than copies, but consults it at strategic time for pattern-matching ("last time privileges were cut during a food shortage, it ended in a disturbance").

### 6.2 Decision journal

Every decision (all gears) appends an immutable `JournalEntry`:

```
{ id, time, gear, action, rationale, warden_view_digest, predicted_effects,
  confidence, dissent (if staff advice diverged), outcome: Option<OutcomeReview>,
  outcome_delta: Option<f32> }
```

The journal powers: (a) outcome-learning (§5.2 step 8); (b) the UI "Warden" panel — the observer can read *why*, which is essential for the directive's "understandable even as complexity increases"; (c) succession — a replacement warden inherits the journal's institutional facts but not its personality, so a hardliner successor inherits a diplomat's obligations, not his instincts.

### 6.3 Reputation

`InstitutionReputation { public, oversight, staff, prisoners }` — four axes, each `0..100`. Moved by escapes, scandals, inspections, audits, released-prisoner outcomes (§17.2), awards. Feeds `public_reputation` and `political_pressure` objective satisfaction and `RequestFunding` success probability.

### 6.4 Corruption susceptibility — modeled, never railroaded

`CorruptionState { temptation_exposure: u32, embezzlement_total: i64, complicit_guards: Vec<GuardId>, risk_of_discovery: f32, last_temptation: Option<SimTime> }`

- **Temptation events** fire stochastically (contractor kickback offers, "look away" payments on contraband ledgers, padded vendor invoices), frequency scaled by `corruption_susceptibility` and by financial desperation (deficit wardens get bolder offers).
- Acceptance is a utility decision like any other (it is *not* a dice roll against a hidden flag): the candidate action `AcceptKickback { amount, terms }` is injected into the tactical candidate set only for susceptible wardens, scored by personal gain vs. `reputation_concerns`, `staff_loyalty` (complicity drags loyal officers), and `risk_of_discovery`.
- Consequences of discovery (investigation, audit, journal leak, oversight): reputation collapse on all axes, political_pressure spike, forced resignation, criminal abstraction, ascent of a successor warden with low corruption tolerance (who may purge the complicit).
- Clean wardens are never *tempted* — the action simply never enters their candidate set. This keeps the mechanic emergent, not punitive.

---

## 7. Guard Subagents — Data Model (Directive §10)

### 7.1 GuardAgent

```rust
GuardAgent {
    id: GuardId,
    name: String,
    rank: Rank,                     // Officer | SeniorOfficer | Sergeant | Lieutenant | Captain
    personality: GuardPersonality,  // §7.2
    competence: f32,                // baseline skill, 0..100
    experience_days: u32,
    morale: f32,                    // 0..100, live
    fatigue: f32,                   // 0..100, live (higher = worse)
    stress: f32,                    // 0..100, live (higher = worse)
    loyalty: f32,                   // to institution & to warden personally (two sub-scores)
    integrity: f32,                 // resistance to corruption & policy violation
    relationships: Vec<RelationshipHandle>,   // guard↔prisoner, guard↔guard, guard↔warden (Social owns the edges)
    officer_since: SimTime,
    post: Option<PostId>,           // current assignment
    shift: ShiftId,
    current_task: Task,             // Patrol | Observation | Escort | Intervention | ReportWriting | Training ...
    duties: Vec<Duty>,              // derived from rank + post + policy
    history: GuardHistory,          // commendations, reprimands, use-of-force incidents, reports filed
    conditions: Vec<Condition>,     // e.g. Suspended, OnLeave, Injury(reason)
}
```

### 7.2 GuardPersonality — the directive's traits, operationalized

`GuardPersonality { courage, empathy, cynicism, diligence, sociability, volatility, authoritarian_streak, humor? }` — deliberately smaller than the warden's: guards are *operators*, not strategists. The directive's "independent personalities, competence, morale, relationships, fatigue, loyalty, professionalism, and memories" map as:

- professionalism = f(diligence, integrity, training) — suppresses shortcut-taking;
- memories: guards keep short-term incident memory (72h) and long-term notable-events memory (first use of force, favorite/feared inmates) — a subset of the prisoner memory model, smaller capacity;
- relationships reference Social Systems handles (§17.1) rather than a private copy.

**Rank ladder & authority ceiling** (see AuthorityMatrix §9.2):

| Rank | Authority cap | Typical post weighting | Report quality | Command scope |
|---|---|---|---|---|
| Officer | L2 (coop/escort) | Wings, corridors, yard, work | Standard | None |
| Senior Officer | L3 (physical intervention) | Any operational | Good | Buddy officer informal lead |
| Sergeant | L3+ (command on-scene) | Shift lead, reception, medical | High | Squad (4–8) |
| Lieutenant | L4 (emergency authorization) | Area commander (wing/block) | Very high | Area, multi-squad |
| Captain | L5 (institution commander, in warden's stead) | Duty officer / admin | Debrief-grade | Institution-wide in emergency |

### 7.3 Trait dynamics (live, deterministic)

Per tick, guard state evolves:

```
fatigue += shift_load(post, hours, activity) − recovery(rest quality)
stress  += incident_exposure × volatility − decompression(time_off, morale, empathy support)
morale  += f(pay equity, fair scheduling, leadership(warden decisions touching staff),
             witnessing misconduct, victories/defeats) − fatigue×0.01
issuance risk (burnout, quit, misconduct) = g(fatigue, stress, morale, integrity,
             personality volatility, loyalty) — threshold crossings emit events
```

**Misconduct model.** When stress/morale/integrity cross bounds, guards may: sleep on post, skip counts, accept bribes, brutalize prisoners, leak info, or quit. These are *simulation events with consequences* (investigation, termination, criminal abstraction), never scripted villainy. High-integrity exhausted guards still degrade — just toward errors, not malice.

---

## 8. Guards — Posts, Shifts & Assignment (Directive §10)

### 8.1 Post catalog (the nine directive posts, operationalized)

```rust
enum PostType {
    Tower,        // overwatch; high visibility, low contact; counts & alarm response
    Wing,         // housing unit supervision; counts, escorts, conflict defusal
    Corridor,     // movement control; choke-point presence
    Yard,         // recreation overwatch; highest incident density per hour
    Reception,    // intake processing; screening, property, classification support
    Medical,      // clinic security; patient watch, emergency standby
    WorkArea,     // shops/kitchens/laundries; contract compliance, tool control
    VisitorArea,  // visiting hall; searches, contraband interdiction, family dynamics
    Administration, // HQ, records, control room; clerical + command support
}
```

Each post type defines: `min_staffing(security_level)`, `coverage_hours` (24/7 vs business vs program-hours), `risk_profile { violence, contraband, escape, medical }`, `required_rank_min` (e.g., Medical requires ≥ Senior Officer for patient watch), `fatigue_coefficient`, `visibility` (how much the post observes — matters for reports, §10).

### 8.2 Instances & coverage model

`PostInstance { id, post_type, location_ref (building/wing), assigned: Vec<GuardId>, authorized_strength }`. Coverage is computed per shift rotation (3×8h default; night shift ×0.75 staffing with tower emphasis). The institution maintains an **uncovered-post deficit** metric; it feeds `security` objective and escape risk index directly, and the warden sees it in every `StaffingSnapshot`.

### 8.3 Assignment algorithm (warden sets posture; engine assigns; guards accept/refuse)

1. Constraints: rank eligibility, fatigue cap (a guard >80 fatigue cannot hold Tower or Yard without sign-off), discipline status (suspended guards unassignable), medical restrictions.
2. Score each guard–post pair: `fit = w1·competence + w2·experience(post-type) + w3·preference Match + w4·(1−fatigue) − w5·recent_same_post_penalty` (fair rotation) `+ w6·relationship_context` (e.g., avoid assigning a guard with an open feud to that same wing).
3. Solve as greedy bipartite matching with stability preference (keep continuity where possible — a guard's presence itself deters/predicts) rather than global re-optimum every day.
4. **Refusal & friction:** a guard may refuse (low morale × high-risk post) → loyalty/discipline check; repeated refusals → insubordination event. Assignments appear as orders; the warden's `staff_loyalty` trait influences how hard refusal is punished.
5. Reassignment cadence: tactical tick, plus emergency surges (disturbance → pull corridors to Yard/Wing, open Tower reinforcement).

### 8.4 Work conduct & schedule

Guards follow the same `SimTime` schedule framework as prisoners: shift blocks, roll-call at shift start (counts verified against roster — a failed count is an incident), meal/relief windows, debrief at shift end (report writing — report quality scales with remaining diligence).

---

## 9. Guards — Duty Loop, Authority Matrix & Autonomous Response

### 9.1 The guard duty loop (mirrors prisoner loop, cheaper)

```
PERCEIVE  — scan current post zone: entities present, activities, anomalies
ASSESS    — classify: routine | notable | violation | emergency; salience-ranked
ACT       — per authority matrix & task priority:
              routine → continue patrol/observation, log periodically
              notable  → approach, observe closer, log detail
              violation→ intervene at authorized level, warn, escort, seize contraband
              emergency→ incident FSM (§9.3), call for backup by radio channel
REPORT    — file/submit reports; request assistance; handoff notes at shift change
EVOLVE    — update fatigue/stress/relationships/opinions
```

Perception is range/line-of-sight bounded. Guards do **not** see through walls, do not know prisoner intent, and misjudge with probability from their `competence`/`stress`/`fatigue`. Tower guards have broad sightlines but cannot hear; wing guards see wings deeply but not corridors; Yard is high-signal/high-noise (everything looks like an incident in a crowd).

### 9.2 Authority matrix (who may do what, and when)

| Level | Action | Officer | Sr.Officer | Sergeant | Lieutenant | Captain |
|---|---|---|---|---|---|---|
| L0 | Observe, log | ✔ | ✔ | ✔ | ✔ | ✔ |
| L1 | Verbal directive, warning | ✔ | ✔ | ✔ | ✔ | ✔ |
| L2 | Escort, seize contraband, deny entry, count order | ✔ | ✔ | ✔ | ✔ | ✔ |
| L3 | Physical intervention, restraint, cell extraction | — | ✔ | ✔ | ✔ | ✔ |
| L4 | Emergency authorization: area lockdown, medical override, outside agencies | — | — | — | ✔ (with notification) | ✔ |
| L5 | Institution-wide command in warden's absence/incapacity | — | — | — | — | ✔ |

- **One-level exception:** in a verified emergency (fire, assault in progress, escape in progress), any guard may act one level above cap *once*, then must justify in report; automatic review fires (use-of-force review). This is the "split-second decision" valve — it must exist, be logged, and be scrutinized.
- **Policy modulation:** warden policy (§5.4 #5/#6, force posture) shifts columns at runtime (e.g., elevated posture allows L3 at Officer level with mandatory body-cam analogue — abstract "witnessed" flag improving report credibility).
- **Inmates'-eyes rule:** every use of force emits events consumed by Social (relationship damage with witnesses) and by oversight (audit heat).

### 9.3 Incident response FSM (autonomous, no warden required)

```
Idle → Detected → Assessing → { Deescalate | Intervene | RequestBackup }
Intervene → { Resolved | Escalating | MedicalNeeded }
RequestBackup → AwaitingAssist → InterveneAssisted → Resolved
any → ReportFiling → Closed
```

Key properties: guards **call for assistance early** under uncertainty (voice/radio event with location); nearby guards' response latency depends on their post, assignment, and current task (an escorting officer ignores a yard fight unless L3-worthy); a guard who resolves alone gains reputation with sergeants and stress in equal measure. Group disturbances hand off to warden `RespondToDisturbance` at the posture level while guards execute L3–L4 tactically (§17.2, §17.4).

### 9.4 Mistakes, opinions, relationships

- **Mistakes:** misidentification (wrong inmate escorted), missed contraband, false alarm, excessive/insufficient force — all probability-weighted by state, all generating events and journal/history entries. Mistakes are *content*, not bugs.
- **Opinions:** guards form cheap judgments (`respects|dislikes|fears|underestimates`) toward prisoners after N interactions — stored as relationship *attitude* tags (Social §17.1), coloring future interactions and report `subject_framing` ("troublemaker" vs "subject").
- **Relationships:** guard–prisoner rapport can be protective (trusted officer defuses) or corrupting (guard owes favors); guard–guard bonds affect backup response reliability; guard–warden loyalty is personal as well as institutional — a guard may serve an institution he despises out of loyalty to *this* warden, and morale craters non-linearly when that warden falls from grace.

---

## 10. Guard → Warden Reporting Pipeline (Directive §10)

### 10.1 Report model

```rust
Report {
    id: ReportId,
    reporter: GuardId,
    post: PostId,
    time: SimTime,
    kind: ReportKind,          // Incident | PatrolLog | Contraband | WelfareConcern | PolicyViolation |
                               // StaffingRequest | UseOfForce | EscapeAttempt | MedicalEmergency | Grievance...
    severity: Severity,        // Info | Notable | Serious | Critical
    subjects: Vec<ClaimSubject>,
    narrative_digest: String,  // templated or AI-phrased (§17.3), always engine-summarized
    credibility: f32,          // computed, §10.2
    detail_level: DetailLevel, // derived from reporter state + time spent writing
    witnessed: Witnessed,      // Direct | Hearsay | Inferred — feeds ClaimSource
    follow_up: Option<FollowUp>,  // warden may request clarification/investigation
}
```

### 10.2 Credibility scoring

```
credibility = clamp(
      0.45 × competence/100
    + 0.20 × diligence
    + 0.15 × integrity
    + 0.10 × (1 − fatigue/100)
    + 0.10 × witness_directness    // Direct=1, Hearsay=0.5, Inferred=0.3
    − relationship_bias            // feud with subject → −0.2; friend → +0.1 (capped)
    − 0.25 × stress_impairment    // high stress degrades accuracy
, 0.05, 0.99)
```

Reports with credibility < 0.35 are flagged `LowConfidence` in the warden's inbox; the warden's `competence` trait determines whether he *notices* the flag or takes it at face value (miscalibrated wardens catastrophize on bad reports — realistic).

### 10.3 The pipeline: event → eyes → paper → inbox

```
WorldEvent (anything notable)
   ↓ 1. Perception check: which guards' zones contain it? Roll salience×attention.
   ↓ 2. Observer selected (nearest, most alert — or none: not every event is seen!)
   ↓ 3. Report drafted after resolution (or immediately for Critical); quality from state
   ↓ 4. Routed: post → ShiftSergeant (may annotate) → Lieutenant (may escalate) → WardenInbox
   ↓ 5. WardenTriage: severity × credibility × unseen-before; Critical = immediate interrupt
   ↓ 6. Warden may: Acknowledge | RequestClarification | OpenInvestigation | Act (reflex/tactical)
   ↓ 7. Non-observed events generate *absence*, not reports. Patrol-log gaps are detectable.
```

**The warden is structurally blind between reports.** Long silence is ambiguous: calm or unobserved? Wardens with high `competence` request patrol confirmations when indices drift without reports (e.g., tension high, no violence reports → "are my guards asleep?"); low competence reads silence as health. Briefings (§4.3) exist precisely to bound this uncertainty to hours, not weeks.

### 10.4 Rumor vs. report

Reports enter the KnowledgeStore as `Claim`s with `source: Report`. Prisoner chatter enters as `source: Rumor` (lower prior confidence, mutable). When a rumor and a report coincide, confidence stacks; when they conflict, `Disputed` standing — the warden prefers the report unless the reporter's credibility is poor, in which case rumor may win the argument *and be wrong*, which is the intended texture.

### 10.5 Report load & attention

Unfiltered reporting will drown the warden. Triage enforces: Critical always surfaces; Serious surfaces if credibility ≥ 0.4 or repeat-frequency high; Notable is batched into daily digest; Info is filed to history only. Digest generation is deterministic (template) with optional AI phrasing; the underlying facts are never AI-invented — the AI phrases, the engine cites.

---

## 11. Administration & Finances — Ledger Model (Directive §11)

### 11.1 Money model

Double-entry in spirit, single ergonomic API: `Ledger { accounts: Map<AccountId, Account>, op_log: Vec<Operation> }`. Every transaction references a cause (`OperationCause::Funding | Expense(category) | Construction | Penalty | Bonus | Grant | Contract | Correction`) for auditability — the op log *is* the financial history (directive §18) and feeds UI and investigations.

```rust
Account { id, kind: Asset|Liability|Revenue|Expense, balance: i64 /* minor units */,
          category: AccountCategory /* see §11.3 */, ytd: i64 }
```

No floating point. Transfers between accounts are atomic operations on the world tick; the finance service runs inside the deterministic core, so balances are replay-stable.

### 11.2 Tick & cycle structure

- **Per tick:** utilities/food/medical accrue continuously; staffing costs accrue continuously.
- **Daily close:** revenue recognition (per-capita funding for the day, program participation fees), expense posting, degradation accrual (§12.3), penalty/bonus evaluation triggers.
- **Monthly settlement:** government grant disbursement, contract payments, loan interest, supplier invoices, audit events, budget variance report → tactical decision input.

### 11.3 Chart of accounts (maps directive §11 verbatim)

**Revenue (`Revenue*`)**
| Account | Rule (configurable, §18) |
|---|---|
| `Revenue::PerCapitaFunding` | `population × per_prisoner_daily`; scaled by `occupancy_efficiency` (penalize >95% cram and <70% underuse via config curve) |
| `Revenue::GovernmentGrants` | monthly base allocation + negotiated supplements; reduced by penalty events |
| `Revenue::PerformanceBonuses` | paid when KPI thresholds met (§13.4): e.g., zero escapes, audit clean, rehab cohort graduations |
| `Revenue::PenaltiesLevied`* | *contra-revenue*: deductions for escapes, riots, lawsuits, audit findings — booked as contra-revenue so net funding is honest |
| `Revenue::Contracts` | work-program contracts (workshops, kitchen commissary, laundry) — scales with WorkArea capacity, skilled labor share, and security incidents disrupting work |
| `Revenue::RehabilitationFunding` | per-participant-day funding for enrolled rehab/education seats |
| `Revenue::SpecialPrograms` | earmarked grants (e.g., addiction recovery initiative) with use-it-or-lose-it conditions; diverted funds are an audit finding |

**Expenses (`Expense*`)**
| Account | Driver & degradation coupling |
|---|---|
| `Expense::Food` | `population × per_capita_food × quality_tier`; quality tier ← budget allocation (§§ deficit → quality ↓ → aggression/health ↑) |
| `Expense::Water` | `population × rate + facilities load` |
| `Expense::Utilities` | buildings × base_load + occupancy; expands with construction |
| `Expense::Salaries` | Σ(guards, staff) by rank/pay band + benefits load + overtime premium (chronic understaffing → overtime spiral) |
| `Expense::MedicalCare` | base per capita + acuity (illness prevalence) + underfunding backlog interest (§12.3) |
| `Expense::Maintenance` | Σ(buildings × upkeep) + deferred-maintenance multiplier (skipped maintenance compounds) |
| `Expense::Construction` | active `ConstructionSite` draws against budget (§14) |
| `Expense::SecurityEquipment` | cameras, locks, radios, restraints — refresh cycle + incident-driven replacement |
| `Expense::Education` | per-seat program costs + materials |
| `Expense::Rehabilitation` | counselors, therapy, addiction programs, mentorship stipends |
| `Expense::Transportation` | intake/release/court/medical transfers — scales with population churn |
| `Expense::Supplies` | consumables; scales with occupancy and construction |
| `Expense::Administrative` | licensing, records, legal, insurance; scales with building count (bureaucracy, §15.3) |

### 11.4 The warden's books — what the warden actually sees

Every `WardenView.finances` contains: `cash`, `monthly_burn`, `runway_days`, `budget_allocations` (vs. actuals by category), `forecast_90d` (simple linear + construction-aware), `pending_penalties/bonuses`, `debt_service`. The warden does **not** see the ledger op-log by default — he sees his monthly settlement report, prepared by the (AI or simulated) finance officer, with the same fallibility rules as any briefing (§4.3): embezzlement is *hidden* in it until an audit or suspicious variance tips it off.

---

## 12. Revenue & Expense Dynamics — Formulas & Feedback Loops (Directive §11)

### 12.1 Net position

```
NetDaily = ΣRevenue(daily-ized) − ΣExpenses(daily)
Runway   = cash / max(monthly_burn, ε)
Health   = f(runway, debt_service, penalty_trajectory)  // Surplus | Balanced | Strained | Deficit | Insolvent
```

### 12.2 Occupancy–revenue coupling

Revenue scales with population, but the directive's own warning is the design law: **more prisoners ≠ more money, freely.** Per-capita funding has diminishing returns beyond rated capacity; pushing past 100% occupancy pays a *crowding penalty* per prisoner-day above capacity (grant clawbacks, litigation risk) while expenses scale super-linearly (fights, illness, maintenance, overtime). Optimal operating point typically sits near 90–98% of rated capacity — close enough to full to fund itself, loose enough to stay stable. This single curve forces real expansion decisions (§14) rather than infinite cramming.

### 12.3 Degradation & neglect curves (the "you can't just starve it" mechanics)

Underfunding does not merely save money; it accrues damage with interest:

| Starved category | Degradation path | Terminal consequences |
|---|---|---|
| Maintenance | `condition -= (required − allocated) × k` per month; failed-component probability = g(100 − condition) | Cell door failure → escape risk ↑; utilities outage; injury events; emergency reconstruction at 2× cost |
| Medical | `backlog += acuity_unserved`; prisoner health ↓ | Disease outbreak, wrongful-death abstraction → lawsuit (big penalty), reputational collapse, oversight intervention |
| Food | `quality_tier` drops one step per sustained underspend | Aggression +, illness +, contraband (hunger economy) +, protest risk + |
| Security equipment | equipment refresh lags | Contraband interdiction ↓; false alarms ↑; guard frustration |
| Rehabilitation | seats collapse to zero | Parole pipeline dries up → population ↑ → revenue ↑ but stability/safety ↓ (§16) |

Each curve emits `InstitutionEvent::DegradationWarning { category, level }` at 25/50/75% thresholds so the warden (and the player) get signal before collapse — and low-`competence` wardens ignore the warnings.

### 12.4 Bonuses & penalties — KPI-linked settlement (directive §11)

```
KPIs (trailing quarter): assault_rate, escape_attempts, escapes_successful, audit_score,
                         rehab_graduates, occupancy_efficiency, staff_turnover, grievance_closures
Bonus  if: escapes == 0 AND audit_score ≥ 85 AND turnover ≤ target      → +X% of base grant
Penalty if: successful escape                                            → flat fine + grant −Y% (Z months)
Penalty if: riot/disturbance > threshold                                 → fine + oversight review flag
Bonus  if: rehab_graduates ≥ quota                                       → per-graduate bonus
Penalty if: wrongful death / abuse finding                               → major fine + leadership review
```

All thresholds/amounts in config. Penalties hit *net revenue* (contra-revenue account), so they appear in the same line the warden watches — no hidden costs.

### 12.5 Contracts & work programs

Contract income requires: WorkArea capacity, enough healthy willing workers (skills × discipline), security to run it, and **absence of disruption**. A contract breach (disturbance destroys a workshop order) forfeits payment and blacklists rebidding for a config period. Savvy wardens expand workshops before they expand beds — counter-intuitive, correctly emergent.

---

## 13. Administration — Budget Process & Financial Crises (Directive §11)

### 13.1 The budget cycle

1. **Annual plan (strategic):** warden proposes `BudgetPlan { allocations: Map<Category, i64>, capital_projects: Vec<ProjectId> }` via AI subagent; validated (allocations sum ≤ projected revenue + opening reserve); recorded in journal.
2. **Monthly variance report** (finance officer briefing): actuals vs plan, flagged variances, recommended reallocations.
3. **In-cycle reallocation (tactical):** warden may move allocations between categories with an `attention` cost and audit visibility; big mid-cycle swings without crisis justification add oversight heat.
4. **Reserve policy:** config reserve ratio (default 8–12% of monthly burn); below reserve → `FinancialState::Strained`; capital spending locked except emergency maintenance/security.

### 13.2 The five financial states

```
Surplus → Balanced → Strained → Deficit → Insolvent
```

Transitions driven by runway/reserve/net-flow hysteresis (enter at worse value than exit — avoids flapping). Mechanics per state:

- **Surplus:** bonuses accrue to reserve; expansion appetite legitimately up-weighted; small one-time quality-of-life spends (morale) cheap and effective.
- **Balanced:** normal play; optimization is about tradeoffs, not survival.
- **Strained:** expansion locked; hiring freezes except security-critical; maintenance deferred at rising penalty risk; UI banners.
- **Deficit:** automatic spending cuts (reverse-priority list: capex → programs → supplies → *never below* food safety/medical minimums while solvent-ish); overdraft interest accrues; `political_pressure` +; warden must `RequestFunding` or cut harder.
- **Insolvent:** outside administration appointed (state oversight) — the warden loses capital authority and possibly post (abstraction); penalty-driven; reputation floors at zero; recovery possible only via: emergency grant, major contract win, population reduction (transfers), or austerity survival for N months. Bankruptcy is a **soft-fail, not a game over**: the prison persists, the warden's career may not.

### 13.3 Request-funding politics

`RequestFunding` success = f(political_pressure [lower is better], public_reputation, audit_score, crisis_severity, political_ambition weight, amount). Success: supplement disbursed over months. Failure: oversight visit scheduled (audit event in 2–6 weeks; findings move reputation both ways). Repeated failure + worsening KPIs → leadership review. Corrupt inflows (kickbacks) can *also* paper over deficits short-term — at escalating discovery risk (§6.4).

### 13.4 Financial decision support

Decision engine sees forecasts, not certainties: `forecast_90d` is a distribution (percentile bands), so "approve expansion?" is scored as expected utility over the band; high-deficit trajectories make capital projects utility-negative even with good long-term payoffs, and high-`risk_tolerance` wardens still take them. Financial AI failure mode to avoid: forecasts are *deterministic projections with stated assumptions*, clearly labeled, never fabricated precision.

---

## 14. Expansion — Building Catalog & Construction Lifecycle (Directive §12)

### 14.1 Building catalog (directive's twelve options + required support)

`BuildingSpec { cost, build_days, capacity_delta, upkeep_monthly, staffing_delta: Vec<(Role, i16)>, utility_load, unlocks, prerequisites, risk_notes }`

| # | Building | Cost (cfg units*) | Time | Capacity | Ops effects | Unlocks |
|---|---|---|---|---|---|---|
| 1 | Cell Block (housing) | 480k | 120d | +48 beds | +housing, +utilities | intake demand |
| 2 | Dormitory | 260k | 75d | +60 beds (min security only) | cheaper/bed, lower privacy, disputes + | low-security intake |
| 3 | Medical Facility | 520k | 130d | — | acuity capacity ↑, medical degradation ↓ | contracts for forensic exams?; audit score ↑ |
| 4 | Kitchen | 240k | 60d | — | food service for +120; hygiene grade | special contracts |
| 5 | Workshop | 310k | 80d | — | contract capacity +X | work-program revenue ↑ |
| 6 | Recreation Area | 180k | 45d | — | stimulation/hope needs ↑ | protest frequency ↓ |
| 7 | Education Building | 390k | 100d | 24 classroom seats | education program seats | recidivism ↓, parole pipeline ↑ |
| 8 | Administrative Building | 440k | 110d | — | admin capacity +40% (§15.3) | more posts, more programs, audit resilience |
| 9 | Isolation Facility (SHU) | 500k | 110d | 12 isolation cells | severe-discipline housing | gang leadership suppression; welfare cost, lawsuit risk ↑ |
| 10 | Visitor Facilities | 210k | 55d | — | visit sittings ↑, contraband risk shifts | family-contact need satisfaction, morale ↑ |
| 11 | Staff Facilities | 280k | 70d | — | guard rest quality ↑ | fatigue recovery ↑, turnover ↓, misconduct ↓ |
| 12 | Yard Expansion | 150k | 40d | — | yard capacity +100 | yard-density incident rate ↓ |

*Units illustrative; real values are config (§18). Every spec also carries `maintenance_weight` feeding Expense::Maintenance automatically — construction is never free money later.

Prerequisites examples: Workshop requires Kitchen (workers eat) and power headroom; Medical Facility requires Admin (licensing) at high tier; SHU requires a Cell Block; Education Building requires Admin or Recreation (enrollment draws); Yard Expansion requires perimeter integrity ≥ threshold. Prerequisite failures are surfaced at proposal time, not mid-dig.

### 14.2 Construction lifecycle

```
Proposed → Feasibility (funds ✓? prereqs ✓? staff ✓? utilities ✓?)
        → Approved (capital committed to reserve escrow)
        → SitePrep → UnderConstruction (daily draws on Expense::Construction,
            progress events, disruption modifiers: noise/dust/lockdowns, incident risk +)
        → Commissioning (staffing hired & trained, inspected)
        → Operational   (capacity live; ledger & capacity registries updated)
Cancelled/Deferred possible from any pre-Operational state (sunk cost rule: 40% forfeiture).
```

Multiple concurrent projects allowed but attention-limited: each active site consumes `AttentionBudget` (§15.3); two+ simultaneous sites degrade progress stochasticly (contractor contention, inspection backlog) unless Admin Building capacity is high. Expansion therefore has a natural concurrency ceiling without an arbitrary cap.

### 14.3 What expansion actually unlocks (constraint, not convenience)

- **Population intake:** new beds are *assigned demand*, not instant fill — intake queue (§16) must exist, and transfers must be accepted (receiving facility willingness, connected to oversight relationship).
- **Program & contract capacity:** Education/Workshop/Kitchen/Medical each *enable* revenue or KPI lines that didn't exist before; a prison that builds beds but not programs starves its own rehab funding.
- **Staff demand:** every building lists staffing deltas; an unstaffed building is worse than none (health hazard, inspection failure).

---

## 15. Expansion Consequences & Scaling Challenges (Directive §12)

### 15.1 The scaling law the directive demands

New buildings increase capacity, which increases population, which increases: staffing requirements, maintenance load, utilities, conflict surface, and **administrative complexity** — while returns on scale are diminishing and risks are increasing. Concretely, per building/population tier the engine applies:

```
complexity = α·building_count + β·population + γ·active_projects − δ·admin_capacity_from_AdminBuildings
incidents_baseline ∝ complexity;  staffing_need = Σ posts' authorized strength (already §8)
admin_overload    = max(0, complexity − admin_capacity)
```

### 15.2 Consequences tables

**Higher population:** more needs pressure on kitchen/medical/yard (per-capita quality falls if unmatched builds don't follow); more gangs (emergent — Social threshold scales with cohort size and diversity); more intake processing; more releases/parole churn; more transport.

**More staff requirements:** payroll nonlinearity — the *second* yard needs officers the *first* yard trained; onboarding lag means new capacity comes online before proven staff, raising incident odds for the commissioning window (a designed "growth spurt of danger").

**New social groups:** larger prisons break single-faction equilibrium — competing gang territories emerge by wing (Social Systems consumes wing-assignment density for conflict generation); factional grievances now compete for warden attention (§15.3); inter-gang mediation load rises.

**Increased administrative pressure:** decision cadence stays daily but decisions grow in scope — a warden who managed 80 beds personally must govern 400 through officers; error rates climb if `competence` is unsupported by Admin Buildings and lieutenants.

**Increased resources:** utilities, maintenance, supplies scale; a budget that worked at 120 beds fails at 260 without revenue-side expansion (workshop contracts become mandatory, not optional).

### 15.3 Administrative capacity — the warden attention budget (hard constraint)

```rust
AttentionBudget { points: f32, /* refilled daily: base 100 × (0.7 + 0.6·competence/100) */ }
```

Costs: Tactical decision = 1–3; Strategic decision = 8–15; Open investigation = 4/wk; Crisis = 20 flat; each active construction site = 2/day. When points hit zero: the engine *defers* non-critical decisions (they queue to next cycle), decision latency rises, and low-salience reports go unread. This is the mechanism that makes a 500-bed institution run differently from a 120-bed one **without nerfing the warden with a hidden cap** — he simply ran out of Tuesday.

Mitigations the warden can buy: Administrative Building (+40% points/day), Lieutenants/Captain delegation (delegated domains get decisions inside guard-authority bounds with no attention cost), and Policy automation (standing policies resolve routine cases for free, at the cost of rigidity — a policy that forbids early lockdown release cannot *ever* release early).

### 15.4 Soft and hard limits ("must not expand infinitely")

- **Soft:** budget (12.x), attention (15.3), construction concurrency, oversight land/permits, political capital for big builds.
- **Hard (config):** absolute capacity ceiling (default 600 — beyond this the scenario is a different game); max buildings per type; minimum reserve after any capital commitment.
- **Failure texture:** a prison that builds past its maintenance/staffing reality doesn't stop existing — it *fails upward into crisis management*: chronic incidents, penalties, oversight receivership. Expansion addiction is a viable, dramatizable, and punishable strategy.

---

## 16. Population & Intake Administration (§13-adjacent)

The directive assigns deep intake design to other agents; this layer owns the *administrative* surface:

### 16.1 PopulationAdmin service

```rust
PopulationAdmin {
    capacity: CapacityRegistry,          // from operational buildings
    intake_queue: VecDeque<IntakeRequest>, // from outside oversight/courts (abstracted)
    occupancy() -> f32,
    accept_transfer(req) -> Decision,    // warden-visible decision: Accept | Waitlist | Decline(reason)
    schedule_release(prisoner, kind),    // parole | sentence-end | transfer-out | medical
    forecast_population(90d) -> Band,    // feeds finance forecast (§13.4)
}
```

### 16.2 Admission & transfer decisions

Intake decisions are warden-relevant (not warden-solo): `Accept` raises revenue and population pressure; `Decline` lowers political_pressure reward but protects stability; pattern of declines → oversight friction. New admissions trigger Reception post processing (§8), classification (SecurityClassification drives cell/wing assignment consumed by Simulation/Social), and *immediately* feed Social Systems for social-connection pre-seeding (existing rivalries from shared criminal history — Social owns detection).

Population flows: admissions, transfers-in/out, parole releases, sentence completions, deaths (abstracted, minimal ceremony, history-logged). Every population delta emits events consumed by Finance (per-capita revenue mid-month true-up), Simulation (needs re-scaling), Social (cohort shifts), and UI.

### 16.3 Instability management

Sustained occupancy >100% forces the warden to choose: decline intakes (political cost), request expansion (capital cost), or accept crowding (stability cost). Sustained <70% forces: accept transfers aggressively, offer contracts to attract labor, or survive on reserve — with bonuses at risk. The system always presents *three priced doors*, never one obvious answer.

---

## 17. Integration Points

### 17.1 Social Systems agent (directive §6–§8)

| Direction | Contract |
|---|---|
| Institution → Social | Guard/warden ↔ prisoner relationships as first-class edges (`RelationshipKind::GuardPrisonerTrust | Hostility | Obligation | Fear | Respect`); warden–prisoner contact is rare and high-salience (negotiation sessions, inspections); gang membership changes near guards matter |
| Institution → Social | Negotiation channel: `OpenNegotiation` spawns a mediated session entity; Social produces stances/demands per representative; outcome rewrites grievances & gang tension; failure → protest escalation path |
| Social → Institution | Gang intelligence: wardens receive *derived* claims ("Wing C leadership contested") never raw graph internals; Social computes salience, passes annotated claims |
| Social → Institution | Informants: prisoners offering guard/warden info are *relationship transactions*; corruption dyads form between guards and gangs when integrity/morale allow |
| Both | Reputation axes (`staff`, `prisoners` in §6.3) are jointly maintained; Social writes interaction deltas, Institution applies decay & policy effects |
| Both | Use-of-force witnessed events → relationship damage with all witnesses (Social) + audit heat (Institution) |

### 17.2 Prison Simulation agent (directive §3–§5, §13–§16)

| Direction | Contract |
|---|---|
| Institution → Simulation | Schedule/policy changes: lockdown levels, privilege cohorts, work reassignments, program rosters, intake acceptance — all as commands the Simulation validates against its own constraints (a wing can't run yard if its guards are at a disturbance) |
| Simulation → Institution | Disturbance/escape/protest events fire `RespondToDisturbance` candidacy; the warden picks posture (Contain | Negotiate | Extract | CallMutualAid); guards execute tactically via §9.3 FSM; outcomes (injuries, damage, contraband found) return as events |
| Simulation → Institution | Needs/welfare telemetry feeds objective satisfaction (§3.3): food quality tier from budget → Simulation computes hunger/aggression deltas → reports back via welfare indices |
| Both | **Parole (§15):** Simulation/Law owns the independent parole board; the warden may only `SetParoleRecommendation`. Board reads the prisoner record (behavior, rehab, risk). Outcomes hit: population (releases), finance (per-capita true-up, transport), history. Post-release tracking feeds reputation.
| Both | **Escapes (§16):** Simulation detects attempt conditions (desperation, opportunity, security gaps from staffing coverage §8.2); Institution supplies security-posture modifiers; outcome flows to finance penalties, political pressure, warden policy response (investigations, staffing changes) |
| Both | **Medical emergencies/deaths:** Simulation emits; Institution books costs, investigates if neglected, feeds audit/reputation |
| Both | **Intake:** §16 above; Simulation applies personality/needs generation; Institution assigns capacity and Reception processing |

### 17.3 AI Systems agent (directive §19)

- **Strategic warden sessions:** the AI Systems agent constructs prompts from `WardenView` (never raw world state!) + journal digest + goal list + personality brief. Output schema:

```json
{ "goal_updates": [...], "recommended_actions": [InstitutionCommand...],
  "rationale": "...", "dissent_expected": bool, "confidence": 0.0-1.0 }
```

Validated field-by-field (unknown commands rejected; enums checked); the utility engine re-scores all recommended actions with the same formula as tactical gear; anything failing validation/scoring is dropped, not "fixed". ≤2 retry with rejection reasons in context, then utility-only fallback.
- **Negotiation dialogue & report phrasing:** AI may *author prose* (negotiation lines, report narratives, hearing summaries) but every factual token is engine-sourced; invented specifics (names, dates, items not in state) are detected by a constraint checker and stripped/flagged.
- **Token budget:** strategic sessions are rate-limited (default 1 per warden per 7 sim-days + event-triggered interrupts ≤1/day); caching per `WardenView` hash; executor logs token spend to world history.
- **No direct state mutation, ever.** AI output is inert data until the validation pipeline (§5.7) commits it.

### 17.4 Economy / other agents

- **Economy agent** (if split from this layer): co-owns `finance/`; owns KPI computation and penalty/bonus settlement tables; Institution owns ledger mechanics and warden-facing forecasts. Interface: `FinanceService::accrual_tick()`, `settlement_month()`, `forecast()`.
- **Testing agent:** owns scenario suites — e.g., "Starvation spiral: cut food 30% → expect protest within N days", "Expansion overreach → insolvency within Y months" — run headless before merges (§19).
- **UI agent:** consumes journals, reports, ledger snapshots, construction progress; §20 UI hooks.

### 17.5 Persistence & event bus

- All institution state (warden, guards, posts, inbox/unread reports, knowledge store, ledger + op log, budget plan, construction sites, investigation registry, population admin) is in the save world. Save/load mid-construction resumes draws correctly; in-flight AI requests are cancelled on save and rebuilt from `WardenView` hash on load (no half-applied AI).
- Events emitted (non-exhaustive): `GuardReportFiled`, `ReportEscalated`, `WardenDecisionMade`, `WardenDecisionOutcome`, `PostCoverageChanged`, `GuardMisconduct`, `InvestigationOpened/Closed`, `BudgetSettled`, `PenaltyApplied`, `BonusApplied`, `FinancialStateChanged`, `ConstructionProposed/Approved/Progress/Commissioned`, `BuildingOperational`, `IntakeAccepted/Declined`, `PrisonerReleased`, `LockdownTriggered`, `NegotiationOpened/Concluded`. Consumers: History (all), UI (all), Social (relationships-relevant), Simulation (command-relevant).

### 17.6 UI hooks (directive §20)

- **Warden panel:** portrait/personality radar (11 traits), live objectives with satisfaction bars, active goals, decision journal timeline (with outcomes & calibration score), attention budget meter, corruption-risk indicators (subtle — audits pending, variance flags).
- **Guards panel:** roster by post, coverage heatmap over 24h cycles, per-guard dossier (morale/fatigue/stress bars, history, post fit), report inbox with credibility badges.
- **Finance panel:** cash, burn, runway, allocation vs actuals, penalty/bonus ledger, 90-day forecast band, budget plan vs reality variance.
- **Expansion panel:** capacity vs population sparkline, catalog with prereq checks green/red, active sites with progress & burn, complexity/admin-overload gauge.
- **Event timeline:** institution events interleaved with world events; filterable by actor/system.

---

## 18. Configuration (excerpt, TOML)

```toml
[warden.traits]            # generation archetypes & correlation matrix
archetype_jitter = 12.0
[warden.decisions]
tactical_cadence_hours = 6
strategic_cadence_days = 7
temperature_base = 0.35    # tie-breaking softmax; scaled by risk_tolerance
[warden.attention]
base_points = 100.0
competence_gain = 0.6
tactical_cost = 2.0
strategic_cost = 12.0
crisis_cost = 20.0
[finance.funding]
per_prisoner_daily_cents = 14350
crowding_penalty_start = 1.00   # × rated capacity
crowding_penalty_rate  = 0.15   # per unit over capacity
reserve_ratio          = 0.10
[finance.kpi]
escape_penalty_cents      = 250000_00
escape_grant_reduction    = 0.20
audit_bonus_threshold     = 85
rehab_graduate_bonus_cents = 1500_00
[guards.fatigue]
shift_load_yard = 9.0
recovery_per_off_hour = 6.5
[expansion.hard_limits]
max_total_capacity = 600
max_concurrent_sites = 3
```

Every constant above is hot-reloadable for experimentation but versioned into saves (a save stores the config hash it was created with; loading with mismatched config warns).

---

## 19. Testing Strategy & Failure Modes (Directive §24 Phase 4)

### 19.1 Unit & property tests

- Finance: accrual conservation (books always balance); forecast sanity; penalty application idempotence; month settlement golden-file.
- Construction: prereq evaluation truth table; progress monotonicity; cancel forfeiture math; attention drawn and refunded.
- Reports: credibility bounds; staleness decay; standing transitions never skip ranks except via investigation; rumor/report conflict → Disputed.
- Decision engine: utility function determinism given RNG seed; cooldown suppression; oscillation damping; `NoAction` selection under total uncertainty tie; rejected command never mutates state.
- Guards: authority matrix completeness (every level × rank pair defined); FSM deadlock-freedom (model-checked or exhaustive small-state test); fatigue/stress drift bounds (no NaN, no runaway).
- Population: capacity never exceeded by `Accept`; revenue true-up sums exactly to population-days.

### 19.2 Soak & balance scenarios (headless, long-run)

| Scenario | Pass criterion |
|---|---|
| Steady state 90% occupancy, competent warden | No deadlock; finances oscillate within ±15% of Balanced; zero insolvency over 12 sim-years |
| Austerity spiral (food −35%) | Protest/disturbance within N days; warden receives degradation warnings before collapse; recovery possible via re-funding |
| Expansion overreach (build to 500 on 120-bed budget) | Insolvency arc emerges; oversight receivership triggers; no NaN/ panic paths |
| Corruption arc (max susceptibility warden) | Embezzlement eventually discovered or warden promoted/retired; books reconcile; successor differs in personality |
| Guard collapse (double shifts, no overtime relief) | Burnout → misconduct events → investigation chain; no infinite escalation; institution reaches new equilibrium |
| Zero reports (all guards on strike abstraction) | Warden uncertainty grows; briefings still bound; governor cannot reach absurd conclusions without evidence — only bad ones |

### 19.3 Known failure modes to actively hunt

1. **Utility degeneracy** — all candidates score ≈ equal → decision noise; fix: entropy monitoring, minimum score gaps, goal salience boosting.
2. **Reward hacking by the warden AI** — e.g., `RequestFunding` spam; fix: political capital cooldowns + oversight heat per request.
3. **Report starvation** — warden literally cannot see; fix: mandatory briefing floor, patrol-gap detection as a claim (`Claim::PatrolsAbsent`).
4. **Construction deadlock** — every project stalls awaiting attention; fix: minimum attention reserve for commissioning.
5. **Save/load drift** — AI in flight across save; fix: cancel-and-rebuild (§17.5) and determinism tests after load.
6. **Numeric rot** — i64 overflow in ledger after decades of cents; use checked math in debug, saturating in release with logged clamp.
7. **AI hallucinated commands** — unknown enum variants; fix: schema validation rejects; metrics count rejections; high rejection rate auto-falls back to utility-only for the session.

---

## 20. Implementation Phasing (aligns with Directive §24)

1. **Phase A — Foundations:** IDs, `InstitutionEvent` bus wiring, `InstitutionCommand` + validation skeleton, ledger with full op-log, save/load of institution slice. *(Depends on: core persistence & event bus.)*
2. **Phase B — Guards:** posts/shifts/assignment, FSM response, fatigue/stress, reports → warden inbox with credibility, briefings. *(Simulation core not required yet — test against event fixtures.)*
3. **Phase C — Warden core:** personality generation, objectives with live satisfaction, knowledge store, utility decision engine (tactical gear), journal, notifications.
4. **Phase D — Finance depth:** funding/penalty/bonus settlement, degradation curves, budget plan & variance, financial states, request-funding. *(Economy agent pairing.)*
5. **Phase E — Expansion & population admin:** catalog, lifecycle, capacity registry, intake/transfer decisions, attention budget. *(Depends on Simulation intake hooks.)*
6. **Phase F — Strategic AI gear:** AI prompt/validation path, negotiation channel (with Social), parole recommendation bridge, corruption arc, trait drift. *(Depends on AI Systems agent.)*
7. **Phase G — Hardening:** soak scenarios §19.2, UI panels §17.6, save migration tests, tuning pass on §18 constants.

File ownership: this layer touches `src/institution/**`, `src/save/institution_*.rs` (schema contributions), UI panel files (with UI agent), and config. It must not edit Simulation core, Social, or AI orchestrator internals — integration via events/commands only.

---

## 21. Open Questions / Deferred Decisions

1. **Warden succession UX:** does the observer appoint/elect the next warden, or is it automatic from oversight? Design supports either; default proposal: oversight-appointed with 3-candidate shortlist shown to observer.
2. **Multi-warden history:** can a deposed warden return (appeal)? Out of scope v1; journal supports it naturally.
3. **Guard unionization / collective action:** rich vein, deferred — morale/turnover mechanics stand in for v1.
4. **Privatization track:** a for-profit operator warden variant (profit-max traits) — flagged as future archetype, all mechanics support it today.
5. **Exact grant/bonus magnitudes:** require economy balancing pass against §11 examples after first soak; all values live in config, so this is tuning, not redesign.
6. **Investigator staffing:** internal sergeant investigators vs external investigators — internal is cheaper but capturable; external costs cash + attention. Default: both, policy-selectable.
7. **Death/abstraction boundary:** warden/guard death handling follows directive §13's "appropriate abstraction" — events, history, succession; no gore, no mechanics beyond administrative fallout.

---

*End of design document. This author wrote no source files; all implementation notes await the implementation phase and the (currently absent) `architecture-report.md (removed)`. Where that report and this document disagree on module placement, the report wins; where they disagree on behavior, escalate to the lead before merging.*
