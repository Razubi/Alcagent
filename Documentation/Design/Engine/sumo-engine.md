# sumo — the Simulation Engine Design

**Project:** Project Alcagent — Autonomous Living Prison Simulation (Rust)
**Scope:** Engine for the simulation API (crate `sumo`): owns the `alc_world::World` and exposes the public API used by observers, the headless CLI, and future tools.
**Status:** Design for implementation.
**References:** `AGENTS.md` §2 (World simulation), §19 (hybrid AI — "agents propose, the engine disposes"), §22 (technical requirements), `Documentation/Project/PROJECT.md` (crate layout).

---

## 1. Purpose and scope

`sumo` is the engine layer of the Alcagent prison simulation. It is the layer other code drives:

- It **owns** the simulation state (`alc_world::World`) and **owns the time**: `tick()`, `advance_to()`, `run_until()` — with the `TimeSpeed` modes defined by `alc_core`.
- It exposes the **decision pipeline**: AI agents emit `alc_agents::Decision`s; the engine validates them (`alc_agents::DecisionValidator` + engine preconditions) and applies them. Agents never mutate world state directly (§19).
- It exposes **read-only observation** (`Observation` snapshot, `alc-ui` `ViewMode`s) and **metrics** (`Stats` from the event log).
- It provides **persistence** via `alc-persistence` (save/load, autosave scheduling).
- It provides **invariants** checking via `alc-testing`.
- It provides a **report** summary (headless/CLI parity with the `alcagent` binary).

Non-goals: the engine is not the AI — delimiting what agents *want* is `alc-agents`/`alc-*` systems' job; `sumo` decides what an approved decision *does*. The engine does not add new simulation systems; it composes the existing `alc-*` crates through their public APIs.

## 2. Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│ sumo (engine)                                                    │
│ ┌────────────┐ ┌─────────────┐ ┌────────────┐ ┌────────────────┐│
│ │ Sim        │ │ decisions   │ │ observation│ │ autosave       ││
│ │ time, tick,│ │ validate+   │ │ Observation│ │ AutosaveSched  ││
│ │ events,    │ │ apply       │ │ Stats      │ │                ││
│ │ decision,  │ └─────────────┘ └────────────┘ └────────────────┘│
│ │ observation│ ┌─────────────┐ ┌────────────┐ ┌────────────────┐│
│ │ stats,     │ │ validation  │ │ invariants │ │ report         ││
│ │ save/load  │ │ (engine     │ │ (alc-      │ │                ││
│ │ autosave   │ │  rules)     │ │  testing)  │ │                ││
│ │ invariants │ └─────────────┘ └────────────┘ └────────────────┘│
│ └────────────┘         │                                          │
│                        ▼ (uses, no mutation)                      │
│              alc-world · alc-agents · alc-social · alc-economy    │
│              alc-warden · alc-guard · alc-events · alc-persistence│
│              alc-ui · alc-core                                    │
│              alc-core: time, rng, ids (time source authority)     │
└──────────────────────────────────────────────────────────────────┘
```

`Sim` (in `src/engine.rs`... actually `src/lib.rs` root: `Sim` + `src/decisions.rs`, `src/observation.rs`, `src/autosave.rs`).

Module split:

| Module | Responsibility |
|---|---|
| `src/lib.rs` | `Sim` struct: time control (`tick`, `run_until`, `speed`), world accessors, events stream (`events`, `events_since`), decision delivery (`deliver_decision`), observation/stats, save/load, autosave, invariants, report. Re-exports (`Observation`, `Stats`, `DecisionOutcome`, `AutosaveScheduler`). |
| `src/decisions.rs` | Engine decision pipeline: `deliver` / `validate` / `apply` — validate via `alc_agents::DefaultValidator` + engine rules, apply via world mutations + event emission (`alc_social::resolve_conflict` / `apply_outcome` / `resolve_escape_attempt` / `apply_escape_consequences`). |
| `src/observation.rs` | `Observation` (read-only snapshot), `Stats` (metrics from `alc_events::EventBus`). |
| `src/autosave.rs` | `AutosaveScheduler`: due/mark_saved. |

Dependencies: **all** `alc-*` crates (`alc-core`, `alc-world`, `alc-agents`, `alc-prisoner`, `alc-social`, `alc-economy`, `alc-warden`, `alc-guard`, `alc-events`, `alc-persistence`, `alc-ui`, `alc-testing`) + the underlying libs they rely on (`serde`, `serde_json`, `bincode`, `thiserror`, `tracing`, `anyhow`, `rand`, `rand_pcg`).

## 3. Engine API (public surface)

```rust
pub struct Sim { pub world: World, ... }

impl Sim {
    // Time control (§21, §22)
    pub fn new(seed: u64) -> Sim;
    pub fn set_speed(&mut self, speed: TimeSpeed);
    pub fn speed(&self) -> TimeSpeed;
    pub fn tick(&mut self);                       // advance one tick (no-op if paused)
    pub fn run_until(&mut self, target: SimTime) -> u64;   // ticks until target or paused

    // World access
    pub fn world(&self) -> &World;
    pub fn world_mut(&mut self) -> &mut World;
    pub fn days_simulated(&self) -> u64;

    // Events (§17 — integration surface)
    pub fn events(&self) -> &EventBus;
    pub fn events_since(&self, at: SimTime) -> Vec<WorldEvent>;

    // Decisions (§19 — agents propose, engine disposes)
    pub fn deliver_decision(&mut self, id: PrisonerId, d: Decision) -> DecisionOutcome;

    // Observation (§20 — read-only views for observers/UI)
    pub fn observation(&self) -> Observation;
    pub fn stats_since(&self, at: SimTime) -> Stats;

    // Persistence (§23)
    pub fn save(&self, dir: impl AsRef<Path>) -> Result<(), SaveError>;
    pub fn load(dir: impl AsRef<Path>) -> Result<Sim, SaveError>;
    pub fn enable_autosave(&mut self, dir, interval_days: u64);
    pub fn disable_autosave(&mut self);
    pub fn maybe_autosave(&mut self) -> Result<bool, SaveError>;

    // Invariants (§22, §23)
    pub fn check_invariants(&self) -> Vec<String>;

    // Reporting (parity with alcagent run_headless)
    pub fn report(&self) -> String;
}
```

`Observation` (`src/observation.rs`):

```rust
pub struct Observation {
    pub at: SimTime,
    pub days_simulated: u64,
    pub population: u32,
    pub capacity: u32,
    pub guards: u32,
    pub facilities: u32,
    pub cash_major: f64,
    pub economy_health: f64,
    pub security_level: f32,
    pub lockdown: bool,
    pub warden_name: String,
    pub warden_philosophy: String,
    pub gangs: u32,
    pub notable_prisoners: Vec<String>, // top-N by reputation
    pub recent_events: Vec<String>,     // last few event summaries
}
```

`Stats` (`src/observation.rs`): compact metrics derived from the event log over a window (§17):

```rust
pub struct Stats {
    pub since: SimTime,
    pub admissions: u32,
    pub releases: u32,
    pub conflicts: u32,        // arguments, threats, fights, disturbances...
    pub medical: u32,          // medical emergencies, illnesses, injuries
    pub security_incidents: u32, // escapes, contraband, misconduct...
    pub parole_hearings: u32,
    pub avg_population: f64,
    pub current_population: u32,
    pub capacity: u32,
}
```

`DecisionOutcome` (`src/decisions.rs`): `{ agent: PrisonerId, decision: Decision, result: ValidationResult, applied: bool }`.

`AutosaveScheduler` (`src/autosave.rs`): `{ dir, interval_days, last_save_day }` with `due(days_simulated)`, `mark_saved(days_simulated)`.

## 4. Decision pipeline details

`decisions::deliver(sim, id, decision)`:

1. **Validate** (`validate(world, id, d) -> (bool, ValidationResult)`):
   - Unknown prisoner → `Rejected`.
   - Run `alc_agents::DefaultValidator` with an `alc_agents::ValidationContext` built from the world: `lockdown`, `guards_nearby` (guards count — world has no per-guard location model yet; approximation: all guards), `stamina` (1.0).
   - If the `DefaultValidator` accepts/repairs → run **engine rules** (`engine_rules`):
     - `MoveTo` — facility must exist; lockdown → reject.
     - `Socialize`/`Trade`/`SpreadRumor`/`JoinGang`/`LeadGang` — target must exist; `SpreadRumor` content non-empty; `JoinGang` gang exists; `LeadGang`: member.
     - `EscapeAttempt` — rejected during lockdown.
     - `None`/`Comply`/`Refuse`/`RequestMedical`/`LeaveGang` — accept.
2. **Apply** (`apply(world, id, result) -> Vec<WorldEvent>`):
   - `MoveTo` → set `Prisoner.schedule.wing` to the target facility's wing.
   - `Socialize` → `world.social.record(a, b, RelationshipDelta { familiarity: +0.08, friendship: +0.04, trust: +0.02 })`.
   - `Trade` → relationship record `familiarity +0.04`.
   - `Fight` → `alc_social::resolve_conflict(rng, tension=0.5, staff_presence, aggression, discipline, intervention)` → `alc_social::apply_outcome(graph, gangs, participants, outcome, severity, None)`; emit event per outcome kind (Argument/Threat/Fight/GangClash; skip Compliance).
   - `EscapeAttempt` → `alc_social::compute_escape_motivation` → `alc_social::resolve_escape_attempt` → `alc_social::apply_escape_consequences`; emit `EscapeSucceeded`/`EscapeAttempt`/`EscapePrevented`.
   - `SpreadRumor` → emit `EventKind::Rumor`.
   - `JoinGang` → set `Prisoner.gang_affiliation`; emit `GangRecruit`.
   - `LeadGang` → `Gang::change_leader(id)`; emit `LeadershipDispute`.
   - `RequestMedical` → `Prisoner.needs.medical_need += 0.35`.
   - `Comply`/`Refuse`/`LeaveGang`/`None` → no mutation.
3. Emit collected events with `world.time` (IDs assigned by `EventBus::emit`).

Event kinds used come from the `alc_events` taxonomy (`EventKind::RelationshipShift`, `GangRecruit`, `Argument`, `Threat`, `Theft`, `Fight`, `GangClash`, `EscapeAttempt`, `EscapePrevented`, `EscapeSucceeded`, `LeadershipDispute`, `Rumor`).

## 5. Persistence

`Sim::save(dir)` → `alc_persistence::SaveManager::new(dir).save(&self.world)` (bincode + atomic rename + JSON sidecar, `SAVE_FORMAT_VERSION = 1`). New `World.gangs`/`next_gang_id` fields are `#[serde(default)]` so v1 saves load (gangs empty).

`Sim::load(dir)` → `SaveManager::load()`; `Sim::maybe_autosave` — saves when `days_simulated - last_save_day >= interval_days` (interval 0 disables).

## 6. Invariants

`Sim::check_invariants()` — uses `alc_testing` (`invariant_capacity`, `invariant_money_integral`):

- population ≤ capacity
- money integral and non-negative
- unique entity ids

## 7. Testing (Phase 4)

- `tests/engine_api.rs` — new/paused/tick/run_until behavior.
- `tests/decisions.rs` — validate/apply matrix (accept, reject, repair, no-op).
- `tests/observation.rs` — Observation/Stats capture.
- `tests/persistence.rs` — save/load roundtrip via `alc_persistence`.
- `tests/determinism.rs` — same seed → identical events/time/population.
- `tests/soak.rs` — performance smoke: long fast-forward run, invariant checks, ticks/sec.
- Unit tests in `src/decisions.rs`.

## 8. Implementation order

1. `crates/sumo/Cargo.toml` — manifest (all `alc-*` deps + libs; `[lints] workspace = true`).
2. `src/lib.rs` — `Sim` + time control + world access + events + save/load/report.
3. `src/autosave.rs`, `src/observation.rs`.
4. `src/decisions.rs`.
5. `tests/*.rs`.
6. `Documentation/Reports/audit-report.md` (Phase 1 deliverable), `Documentation/Project/PROJECT.md` update.
