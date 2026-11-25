# `sumo` Engine API Guide

> This guide describes the `sumo` engine (the `sumo` subfolder of this repository). References to
> `alcagent` / `alc-ui` describe the game and observer in the parent folders of this repo.

`sumo` is the public Rust API of Project Alcagent. It owns an `alc_world::World` and drives it
forward in deterministic ticks. All world mutations are performed by the engine; agents submit
*decisions*, which are validated and applied by the engine. AGENTS.md §19 ("Agents must not
modify arbitrary simulation state") is enforced by this boundary.

- **Crate:** `sumo` (`crates/sumo/`), version `0.5.0-beta.1` (workspace version), edition 2024, MSRV 1.85.
- **Add as a path dependency:** `sumo = { path = "crates/sumo" }`.
- **Features:** `default = []`; `gui` is a stub (`SumoApp` unit struct and `launch_gui()`
  returning `Ok(())`) and is not recommended — use the `alc-ui` ratatui observer instead.
- **Source of truth for signatures:** `crates/sumo/src/*.rs`; this guide reflects the codebase
  as of 2026-11-25.

> The 0.05 gameplay systems (policy controls, decision/incident audit, queued construction,
> daily rehab/contraband steps, parole, report-driven warden strategy and save format v5) have
> landed and are documented in section 12. The workspace builds and tests green at commit `f525e76`.

---

## 1. Minimal example

```rust
use alc_core::TimeSpeed;
use sumo::Sim;

fn main() {
    let mut sim = Sim::new(0xA1C_0A_6E_17_0);

    // Sim::new starts PAUSED. You must set a speed before any time advances.
    sim.set_speed(TimeSpeed::FastForward);

    // Advance 10 simulated days (one tick = 5 minutes at FastForward).
    sim.run_until(sim.world().time.add_days(10));

    println!("{}", sim.report());
    assert!(sim.check_invariants().is_empty(), "{:?}", sim.check_invariants());
    println!("{:#?}", sim.observation());
}
```

Important: **`Sim::new` starts paused**, and `tick()`, `run_until()`, `run_days()`,
`advance_by_secs()` and `wait_for_event()` are all no-ops / return early while paused.
`Sim::load()` also resets speed to `Paused` — call `set_speed()` again after loading.

---

## 2. Time control

| Method | Behavior |
|---|---|
| `Sim::new(seed: u64) -> Sim` | Boots a fresh world: 6 starting facilities, 4 guards, 24 correlated-intake prisoners; speed `Paused`. |
| `set_speed(&mut self, TimeSpeed)` / `speed(&self) -> TimeSpeed` | Sets both engine and world speed. |
| `pause(&mut self)` / `resume(&mut self)` | `Paused` / `RealTime` (not `Extreme`). |
| `tick(&mut self)` | One step; no-op while paused. Also reconciles the policy mirror, mirrors conflict events into engine logs, records population history and attempts autosave. |
| `run_until(&mut self, SimTime) -> u64` | Ticks until target time or pause; returns tick count; breaks if a tick makes no progress. |
| `advance_by_secs(&mut self, secs: u64) -> u64` | Target = current time + secs. |
| `run_days(&mut self, days: u64) -> u64` | `advance_by_secs(days * 86_400)`. |
| `wait_for_event(&mut self, after: SimTime, pred: impl Fn(&WorldEvent) -> bool) -> Option<WorldEvent>` | Ticks until a matching event appears; `None` on pause/no progress. Beware: the event bus is bounded at 50,000 and evicts old events. |
| `days_simulated(&self) -> u64` | Number of day boundaries crossed. |
| `SimTime` (re-export) | `add_secs`, `add_mins`, `add_days`, `as_secs`, `as_days`, `minute_of_day`, `EPOCH`, `Display` (`Day NNNN HH:MM`). |

`TimeSpeed` (from `alc_core`): `Paused` (0 min/tick), `RealTime` (1), `FastForward` (5),
`Extreme` (30), `EventDriven` (1).

---

## 3. Reading the world

### 3.1 Direct world access and read-only snapshots

```rust
let world: &alc_world::World = sim.world();
let world_mut: &mut alc_world::World = sim.world_mut(); // use sparingly; prefer decisions
```

`Observation` — `Sim::observation()`: a cheap, cloneable snapshot with:
`at`, `days_simulated`, `population`, `capacity`, `guards`, `facilities`, `cash_major`,
`economy_health`, `security_level`, `lockdown`, `warden_name`, `warden_philosophy`, `gangs`,
`notable_prisoners` (top 5 by reputation), `recent_events` (last 8 summaries), `gang_members`,
`leader_board` (top 5 name/reputation pairs).

`AgentObservation` — `Sim::agent_observation(id)`: `id`, `name`, `age`, `classification`,
`needs`, `condition`, `goals`, `gang_affiliation`, `aggression_modifier`, `social_drive`,
`memory_summary`, `current_intention`.

`Stats` — `Sim::stats_since(at)`: counts since a time point: `admissions`, `releases`,
`conflicts`, `medical`, `security_incidents`, `parole_hearings`, `gangs_formed`,
`gang_recruits`, `deaths`, `transfers`, `policy_changes`, plus `avg_population`,
`current_population` (alias `population`), `capacity`, `max_population`, `since`.
Source: `Observation::capture` and `Stats::from_events` (`src/observation.rs`).

### 3.2 UI summary helpers

`Sim::prisoners()`, `gangs()`, `guards()`, `warden()`, `economy()`, `social_edges()`,
`facilities()` return serializable summary structs (`PrisonerSummary`, `GangSummary`,
`GuardSummary`, `WardenSummary`, `EconomySummary`, `SocialEdge`, `FacilitySummary`) used by the
TUI. Caveats: `WardenSummary.inbox` is always empty, `FacilitySummary.wing`/`staff_required`
are always 0, and gang rivalry/alliance intensities are simplified to `1.0`.

### 3.3 Live map (`MapFrame`)

```rust
let frame: sumo::MapFrame = sim.map_frame();
// frame.prisoners / frame.guards: Vec<PawnPosition> (id, name, is_prisoner,
// node_id, edge_progress, target_node, activity, x, y, gang, classification,
// mood, intention, threat, post)
// frame.facility_positions: HashMap<u64, (f32, f32)>, frame.edges: Vec<NavEdge>,
// frame.locked_nodes, frame.incidents: Vec<Incident>
```

This is the documented entry point for rendering a live map without mutable world access.
Both frontends (the ratatui TUI in `alc-ui` and the egui GUI in `alc-gui`) render a live
spatial map from it: the TUI draws facilities (G/C/D/... glyphs), pawns (`P`/`G`), incident
overlays (`!`), locked nodes (`L`) and nav edges (`·`) on a character grid; the egui GUI
draws nodes, pawns, edges, incident badges, rosters and capacity/utilisation bars with
pan/zoom and hover/click inspectors. Note: the navigation graph is still a **placeholder**
— facility positions are a synthetic 4-column grid and `edge_progress` is always `None`
until real pathfinding lands. `MapFrame::capture(&World)` is available but omits incidents;
`Sim::map_frame()` includes them.

---

## 4. Decisions (agent → engine)

### 4.1 Submitting a decision

```rust
use alc_agents::Decision;
use alc_core::PrisonerId;

let p1 = sim.world().prisoners[0].id;
let p2 = sim.world().prisoners[1].id;
let outcome = sim.deliver_decision(p1, Decision::Fight { target: p2.raw() });
assert_eq!(outcome.applied, true);
println!("{:?} -> {:?}", outcome.result, outcome.decision);
```

`DecisionOutcome { agent, decision, result: ValidationResult, applied: bool }`.
`ValidationResult` is re-exported: `Accepted(Decision) | Repaired(Decision) | Rejected { reason }`.
Applied decisions enter `sim.decision_log()`; every outcome, including rejected proposals, enters `sim.decision_audit_log()` with its validation status and reason.

### 4.2 The 13 decision variants and their effects

| Variant | Engine behavior |
|---|---|
| `MoveTo { target_facility }` | Rejected during lockdown / unknown facility. Updates the prisoner's authoritative `world.prisoner_location` (and schedule wing). |
| `Socialize { with }` | Unknown target rejected. Adds familiarity/friendship/trust to the social dyad; emits `RelationshipShift`. |
| `Trade { with, item }` | Unknown target or empty item rejected. Records a balanced pair of 1-unit economy ledger entries and a social delta; emits `RelationshipShift`. (No inmate wallets yet.) |
| `Fight { target }` | Resolves via `alc_social::resolve_conflict` using aggression/discipline and a seeded RNG; applies relationship/gang outcomes; emits `Argument`/`Threat`/`Theft`/`Fight`/`GangClash`. If more than one guard is nearby, validation *repairs* the fight into `Socialize { with: 0 }`. |
| `Comply` / `Refuse` / `None` | Accepted; no world mutation. |
| `EscapeAttempt` | Rejected during lockdown; otherwise resolves via `resolve_escape_attempt` + `apply_escape_consequences`; emits `EscapeAttempt`/`EscapePrevented`/`EscapeSucceeded`. |
| `RequestMedical` | Raises `medical_need` by 0.35 (clamped to 1.0). |
| `JoinGang { gang }` / `LeaveGang` | Sets/clears `prisoner.gang_affiliation`; join emits `GangRecruit`. Requires matching current membership per the validator. |
| `LeadGang { gang }` | Requires membership and leader authority; changes gang leader; emits `LeadershipDispute`. |
| `SpreadRumor { about, content }` | Unknown target or empty/injection-like content rejected. Emits `Rumor`. |

**Validation** (`alc_agents::validate_decision`) applies eight stages: format, semantic,
authority, world-state (lockdown), resource (stamina), legality (pass-through), safety
(self-target, repetition, free-text length/injection markers) and clamp/repair. The engine
then applies its own rules (unknown ids, facilities, gangs, lockdown). Needs are *deficit
pressures*: a high `safety` value means the prisoner feels unsafe.

### 4.3 Brain inspector / dry-run proposals (no mutation)

```rust
use sumo::{DecisionType, ValidationResult};

let proposal = sim.propose_decision(p1, DecisionType::Socialize, Some(p2)).unwrap();
let result: ValidationResult = sim.validate_only(p1, &proposal);
println!("{}", proposal.explanation());

let all = sumo::decisions::evaluate_candidates(sim.world(), p1);
for candidate in all.iter().take(3) {
    println!("{}", candidate.explanation());
}
```

`DecisionProposal` includes `decision_type`, `target`, deterministic `rationale`,
`estimated_outcome`, `scores: Vec<EvaluatorScore>` (drive/score/weight), `confidence`,
`memory_records: Vec<MemoryCitation>` (max 5) and `trace_hash_before`. `DecisionType` is a
13-variant serializable mirror of `Decision`; `target` is an opaque `PrisonerId` slot whose
meaning depends on the variant (prisoner id for social, facility id for `MoveTo`, gang id for
join/lead). `evaluate_candidates` is pure and records `trace_hash_before = 0` (no `Sim` handle).
`propose_decision` returns `None` for missing prisoners, during lockdown (except
Comply/Refuse/None) or if the prisoner has a pending decision.

---

## 5. Observation of events and incidents

- `sim.events()` → `&EventBus`; `sim.events_since(at)` → `Vec<WorldEvent>`;
  `sim.emit_event(event)` for public emission.
- `WorldEvent { id, at, kind, summary, actors, causes, confidence }` with builders
  `with_actor`, `with_cause`, `with_confidence`. Every event carries causes (AGENTS.md §17).
- `EventKind` (39 variants): population (`Admission`, `TransferOut`, `Release`,
  `ParoleHearing`, `ParoleGranted`, `ParoleDenied`, `Death`), social (`RelationshipShift`,
  `GangFormed`, `GangDisbanded`, `GangRecruit`, `GangRivalry`), conflict (`Argument`, `Threat`,
  `Theft`, `Fight`, `GangClash`, `Protests`, `PropertyDamage`, `ContrabandFound`), health
  (`MedicalEmergency`, `Illness`, `Injury`), security (`EscapeAttempt`, `EscapePrevented`,
  `EscapeSucceeded`, `StaffMisconduct`, `Investigation`, `SecurityResponse`, `GuardReport`),
  institution (`PolicyChange`, `Expansion`, `ConstructionComplete`, `FoodShortage`,
  `EquipmentFailure`, `BudgetChange`, `ParoleBoardDecision`), narrative (`Rumor`,
  `FamilyCorrespondence`, `Visitor`, `LeadershipDispute`, `RehabilitationSuccess`,
  `ProgramEnrolled`) and `Dialogue`/`Interaction` (appended last to preserve bincode indices).
- The bus is bounded at **50,000** events; older ones are evicted.
- Engine-side logs: `sim.record_incident(event)`, `sim.incident_log()`,
  `sim.record_sidecar_incident(incident)`, `sim.incidents_since(at)`, `sim.record_decision(d)`,
  `sim.decision_log()`. Conflict events are mirrored into both logs automatically.
  `Incident` (`src/incidents.rs`) adds causality fields (`cause`, `consequence`, `severity`,
  `location`, `involved_prisoners`, `resolved`). Mirrored incidents currently use a
  self-referential cause/consequence pair (own id) as a placeholder.

---

## 6. Policy, guards and warden

### 6.1 Policy

```rust
use sumo::{Policy, PolicyMode};
sim.set_policy(Policy::Enhanced);       // syncs world.policies: security 0.8, privileges 0.3
sim.set_lockdown(true);                // security +0.2, privileges -0.2, PolicyChange event
sim.set_policy_mode(PolicyMode::Shadow);
```

`Policy ∈ {Normal, Enhanced, Lockdown}`. `PolicyState { policy, lockdown, security_level: u32
(0-5), last_changed }` is accessible via `sim.policy_state()` / `policy_state_mut()`.
`PolicyMode ∈ {Deterministic, Council, Shadow}`; `set_policy_mode()` changes it.

**Behavior:** `Deterministic` uses historical warden arbitration. `Council` uses deterministic
drive weights (`sumo::council::DriveWeights`, seven drives) to choose a warden action and logs
the rationale. `Shadow` runs both on the same snapshot, applies only the baseline and logs a
`SHADOW divergence|agreement` event — useful for A/B comparison without changing the world.

### 6.2 Guard drivers

- `sim.guard_duty_cycle() -> Vec<(GuardId, GuardReport)>` — one duty loop per guard.
- `sim.run_guard_duty(guard_id, minutes) -> Vec<GuardReport>`.
- `sim.deliver_guard_report(guard_id, report) -> u64` — severity 0 → no event; 1 → `Argument`;
  2 → `Threat`; ≥3 → `Investigation`; sets credibility and pushes the event id to `warden.inbox`.

> **Double-driving warning:** `World::tick` *already* runs an hourly guard-duty sweep and a
> daily warden arbitration cycle. `Sim::tick` deliberately does **not** call these drivers.
> Calling `guard_duty_cycle()` / `warden_cycle()` is an *additional explicit* invocation for
> orchestration or tests — doing so every tick will double-report/double-apply actions.

### 6.3 Warden drivers

- `sim.warden_cycle() -> Vec<WardenAction>` — arbitrate (per policy mode) and apply.
- `sim.apply_warden_action(action)` — engine applier; emits events, resyncs the policy mirror,
  records `AdjustParoleRecommendation` lean. All 15 `WardenAction` variants are handled.
- `sim.warden_pending() -> Vec<WardenAction>`.

Note: use `Sim::set_lockdown(true)` rather than
`apply_warden_action(WardenAction::ChangePolicy { policy: "lockdown", .. })` — the latter
mutates only `world.policies`, bypassing the engine policy mirror.

---

## 7. Parole and rehabilitation

```rust
for id in sim.parole_due() {
    sim.start_parole_hearing(id);
    // review behavior/rehab/risk via sim.agent_observation(id), sim.rehab_score(id)
    sim.grant_parole(id);   // or sim.deny_parole(id, "risk");
}
sim.apply_rehab_progress(id, 60); // 60 minutes of program participation
```

- `parole_due()` lists eligible prisoners (`sentence_served/sentence >= 0.6`, no discipline)
  without an engine ledger record. `start_parole_hearing`, `grant_parole`, `deny_parole`
  mutate the `ParoleLedger` and emit events. `parole_status(id)` and `sim.parole()` read it.
- `World::resolve_parole_step()` reconciles its daily hearing into the engine ledger, and
  `WardenAction::AdjustParoleRecommendation` lean is recorded on the prisoner record.
- Rehab: `apply_rehab_progress(id, minutes)` relieves Purpose 0.05/min, Respect 0.04,
  Stimulation 0.04, Safety 0.03, Companionship 0.03; `rehab_score(id)` returns `1 - deficit/5`.
  Enrollment, attendance, completion and failure are persisted in `World::rehab_records` and
  surfaced through validated controls (spec 005).

---

## 8. Persistence and autosave

```rust
let dir = "saves/campaign";
sim.save(dir)?;
// ... later ...
let mut sim = Sim::load(dir)?;     // speed is Paused again
sim.set_speed(TimeSpeed::Extreme);
sim.enable_autosave("saves/campaign", 7); // every 7 simulated days
sim.run_days(365);
```

- Files: `world.bin` (versioned bincode, atomic tmp+rename) and `world.debug.json` (debug
  sidecar). Current format is **v5**; v1/v3/v4 saves are migrated by `load_with_engine`.
- `Sim::save` persists both `World` and `EngineState` (parole ledger and conditions, population
  history, incident logs, decision audit, post-release tracking, policy state/mode, autosave
  interval and directory).
- `Sim::enable_autosave(dir, interval_days)`, `disable_autosave()`, `maybe_autosave()`;
  `tick()` checks autosave each tick. Both interval and directory survive save/load, and
  `last_autosave_error()` exposes the most recent failure.
- Maps in the persisted EngineState use `BTreeMap` for reproducible save bytes.

---

## 9. Invariants, determinism and reporting

- `sim.check_invariants() -> Vec<String>` — population ≤ capacity, integral non-negative
  cash, unique entity ids, plus state-consistency checks: one place per pawn, legal intention
  targets, incident cause/consequence resolution, suspicion references, policy mirror sync,
  bounded logs (max 10,000 entries). A soak test expects this to be empty.
- `sim.trace_hash() -> u64` — legacy FNV-1a fingerprint of time, population, event summaries
  and incident sidecar (kept stable for old seeds).
- `sim.trace_hash_v2() -> u64` — additionally mixes in `state_digest::compute_full_state_digest`;
  use for full-state replay comparison. (`trace_hash_v2` may legitimately differ across
  save/load while legacy `trace_hash` remains equal.)
- `sim.report() -> String` — short human-readable summary.

Determinism is seeded: every RNG stream derives from `(seed, entity_id ^ salt)`, so identical
seeds replay identically. The 730-day three-seed chaos soak and save→load→continue regression
tests in `crates/sumo/tests/` verify this.

---

## 10. Gotchas / footguns

1. **Paused by default** — always `set_speed()` after `new()` and after `load()`.
2. **Do not call `guard_duty_cycle()`/`warden_cycle()` from `tick()`** — `World::tick` already
   drives both; explicit calls are additive only.
3. **Policy mirror** — `World::tick` can mutate `world.policies` directly; `Sim::tick` and
   `apply_warden_action` resync `engine_state.policy_state`. Use the public `set_policy` /
   `set_lockdown` methods for engine-initiated changes.
4. **Decision audit semantics:** `decision_log()` contains applied decisions;
   `decision_audit_log()` also contains rejected/repaired proposals with validation status and
   reason. Prefer the audit log when investigating why an action was not applied.
5. **Autosave** keeps both its interval and directory across load. Failures are retained in
   `last_autosave_error()` and emitted via `tracing::warn!`; they are never silently dropped.
6. **Incident causality** uses structured cause/consequence links and is invariant-checked;
   bounded-log trimming scrubs stale references.
7. **Target ids in proposals** are opaque `PrisonerId`s; a missing target becomes id 0 and
   validation normally rejects it.
8. **Map geometry**: synthetic grid + ring graph, still without real pathfinding; both
   frontends (TUI and egui GUI) already render this via `MapFrame`.
9. **Warden inbox** now resolves from actually delivered guard reports; it is intentionally
   not a complete world-event feed.
10. **The `sumo` `gui` feature is a no-op stub**; use the `alc-gui` egui binary or `alc-ui` TUI
    for a real observer. Both render the shared `MapFrame`.
11. **Contraband/capabilities** are wired into daily `contraband_step`; validation reads the
   authoritative contraband state rather than the old skill proxy.
12. **Event bus eviction** at 50k can make `events_since` / `wait_for_event` miss old events.

---

## 11. Testing your integration

```powershell
cargo test --workspace                       # full workspace, including long-run soaks
cargo test -p sumo --test engine_api          # time control
cargo test -p sumo --test decisions           # decision pipeline
cargo test -p sumo --test map_frame           # map snapshot
cargo test -p sumo --test integration_wave4   # end-to-end flow
cargo test -p sumo --test chaos_soak -- --nocapture  # 730-day, 3 seeds
```

The ignored test (`logs_over_limit_are_detected`) is a 10k-entry stress case. All tests use
fixed seeds and id/time-derived RNG streams; none require a network connection or an AI model.

For design rationale, see [`../alcagent/Design/Engine/sumo-engine.md`](../alcagent/Design/Engine/sumo-engine.md)
and [`../alcagent/Systems/engine-api.md`](../alcagent/Systems/engine-api.md).

---

## 12. 0.05 gameplay APIs (stable)

The following APIs shipped in the 0.05 Ralph wave and are covered by workspace tests.

### Policy controls

```rust
pub struct PolicySummary {
    pub policy: Policy,
    pub lockdown: bool,
    pub security_level: f32,            // 0.0–1.0
    pub privileges_level: f32,          // 0.0–1.0
    pub rehab_funding_multiplier: f32,  // 0.0–3.0
    pub intake_paused: bool,            // lockdown || security>=0.9 || manual pause
}

sim.policy_summary() -> PolicySummary
sim.set_security_level(level: f32)      // clamped 0..1; emits PolicyChange
sim.set_privileges_level(level: f32)
sim.set_rehab_funding(multiplier: f32)  // clamped 0..3
sim.set_intake_paused(paused: bool)
```

Security level also selects a policy band: ≥0.9 or lockdown → `Lockdown`, ≥0.7 → `Enhanced`,
otherwise `Normal`. `PolicySummary::intake_paused` is derived from lockdown, maximum security,
and the engine's manual intake flag; it is not a separate persisted world field.

### Structured audit records

```rust
sim.decision_audit_log() -> &[DecisionAuditRecord]
sim.record_decision_audit(record: DecisionAuditRecord)
sim.incident_records() -> Vec<IncidentRecord>
```

`DecisionAuditRecord { time, actor, source, action, summary, validation, reason, rationale,
repair_reason, outcome }` — actor is
`"warden"`, `"prisoner:<id>"` or `"guard:<id>"`; source is `deterministic`, `council`, `shadow`
or `agent`; validation is `accepted` / `repaired` / `rejected`. `IncidentRecord { id, at,
kind, severity, location, involved_prisoners, cause, consequence, resolved }`.
`sumo::incidents::IncidentFilter` and `filter_incidents(&[Incident], &IncidentFilter)` support
time/kind/severity/prisoner/location/resolved filtering with a most-recent `limit`.

### Construction queue

```rust
sim.construction_queue() -> Vec<ConstructionSummary>
```

`ConstructionSummary { id, kind, wing, capacity, total_days, progress_days, cost, started_day }`.
`WardenAction::Expand` now **queues** a project (7–14 days depending on facility type) instead
of adding the facility immediately; completion emits `ConstructionComplete`.

### Daily rehab / contraband steps

`alc-world::World::tick` now runs daily `rehab_step`, `contraband_step` and `construction_step`
alongside the existing parole/escape boundary. Rehab auto-enrolls motivated prisoners and emits
`ProgramEnrolled` / `RehabilitationSuccess`; contraband discovery can change disciplinary
status and security level. New `EventKind` variants: `RehabilitationProgress`,
`RehabilitationFailure`, `PostRelease`. New typed payloads in `alc-events`:
`RehabProgressEvent`, `RehabFailureEvent` (`RehabFailureReason`), `PostReleaseRecord`.

### Persistence

`SAVE_FORMAT_VERSION` is **5**: v4 added structured parole decisions/conditions/release times;
v5 persists the full parole ledger and post-release reconciliation. Older v1/v3/v4 saves are
migrated by `alc_persistence`. `Economy::spend(...)` charges construction costs.

### Warden strategy

```rust
sim.warden_strategy_snapshot() -> alc_warden::WardenStrategySnapshot
sim.decide_warden_strategy() -> alc_warden::StrategyDecision
sim.warden_strategy_cycle() -> Vec<WardenAction>
sim.apply_strategy_decision(decision, source) -> WardenAction
```

The snapshot is assembled only from observer DTOs and delivered guard-report ids, so strategy
cannot read raw prisoner/gang state. Conflicts, finance state and rationale are persisted in
`DecisionAuditRecord` and in the world-owned `WardenAuditLog`; both frontends render the latest
decision. `PolicyMode::Deterministic` keeps the original arbitration path; Council and Shadow
use the report-driven strategy while Shadow still applies the deterministic baseline.

### AI orchestration runtime (spec 011)

```rust
use alc_core::config::AiConfig;

let mut sim = sumo::Sim::new_with_config(seed, &AiConfig::default());
let mut loaded = sumo::Sim::load_with_config("saves/campaign", &AiConfig::default())?;

sim.deliver_decision_batch(vec![(prisoner_id, decision)]) -> DecisionBatchOutcome;
sim.ai_runtime_snapshot() -> alc_agents::AiRuntimeSnapshot;
```

`AiConfig` now carries provider/model selection, per-agent and global token ceilings, rate-limit
burst/refill, retry count, cache TTL/size, batch size, decision timeout and memory-context cap.
All later fields are serde-defaulted, so older config files still load. Runtime policy is applied
at construction/load and is deliberately not persisted in the v5 bincode world; the save layout is
unchanged. Unknown providers/models degrade to deterministic utility decisions without network
access or panics.

`DecisionRequest`/`ProviderContext` carry a bounded, sorted epistemic context with separate
`direct_knowledge`, `rumor`, `suspicion`, `belief` and `confirmed_fact` buckets. A deterministic
context hash invalidates cached or pending decisions when observations change. `deliver_decision`
and `deliver_decision_batch` share one canonical validation gate: contexts are captured before
mutation, repaired decisions are revalidated, rejected decisions never mutate world state, and
world-tick fallbacks are audited as `world_tick_fallback`/`deterministic_fallback`. Batch size,
cache entries, context memories and audit DTOs are bounded. `AiRuntimeSnapshot` exposes the active
provider/model, budgets, rate tokens, cache hits/misses, retries, provider failures, batch counts
and tier decisions to both frontends.
