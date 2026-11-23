# sumo Engine Expansion Design (v2)

**Purpose:** Expand `sumo` from a read/write facade over the `alc-*` stack into the **engine that makes Alcagent better**: adds engine-level loop drivers (guard duty, warden cycles, parole pipeline, rehabilitation), expands observability (agent views, leaderboards, determinism traces), and adds public engine APIs — plus AI-facing design docs (`Documentation/Systems/`).

**Method:** Per §24, work solely on `crates/sumo` (+ docs). Phase 1 audit: `Documentation/Reports/sumo-audit.md`.

**Status:** Design for implementation.

---

## 1. Goals

1. Engine-level drivers so AI/subsystem loops call `sumo::Sim` APIs instead of scattered `alc-world` call sites: guards, warden, parole, rehab.
2. "Agents propose, engine disposes" extended to guards/warden: their proposed actions are applied by the engine (validated, evented).
3. Public, documented engine API expansion for time control, events, observation, determinism.
4. AI-facing design docs in `Documentation/Systems/`.

## 2. New/changed state

### `Sim` gains (struct)
- `parole: ParoleLedger` — engine-level parole pipeline state (world has no parole state).
- (transient) engine telemetry for traces.

### New modules
| Module | Contents |
|---|---|
| `src/drivers.rs` | `guard_duty_cycle`, `warden_cycle`, `apply_warden_action` (engine impl.), `deliver_guard_report` |
| `src/parole.rs` | `ParoleLedger`, `ParoleStatus`, pipeline APIs |

### Engine API additions (§4 below)
All in `Sim`.

## 3. Architecture deltas

- **Guard duty**: `Sim::on_guard_duty_cycle()` — for each guard, `tick` + `duty_loop`; reports → events via `Sim::deliver_guard_report`. Engine-side replacement for ad-hoc loops in `alc-world`.
- **Warden**: `Sim::warden_cycle()` — `alc_warden::arbitrate` + engine-side `apply_warden_action` (implementation moved from `alc-world`, now public). Actions returned for AI consumers; applied + evented.
- **Parole**: engine-side pipeline over `is_eligible_for_parole` (§15): due → hearing (event) → grant/deny (events).
- **Rehab**: deterministic relief moves (§14): `apply_rehab_progress(id, minutes)`.
- **Events**: public `Sim::emit_event`.
- **Persistence note**: engine state (parole ledger, telemetry) is `Sim`-scoped; `save()` writes world only (v1). `Sim::load` resets engine state. Document in `Documentation/Systems/determinism.md`.

## 4. Engine API additions (spec)

### 4.1 Time control (extends §3)
```rust
pub fn advance_by_secs(&mut self, secs: u64) -> u64   // tick while time advances
pub fn run_days(&mut self, days: u64) -> u64          // advance_by_secs(days * 86_400)
pub fn pause(&mut self);
pub fn resume(&mut self);                              // set_speed(RealTime)
pub fn wait_for_event(&mut self, after: SimTime,
                      pred: impl Fn(&WorldEvent) -> bool) -> Option<WorldEvent>  // event-driven advance
```

### 4.2 Events
```rust
pub fn emit_event(&mut self, e: WorldEvent);           // public engine event emission
```

### 4.3 Guard duty
```rust
pub fn guard_duty_cycle(&mut self) -> Vec<(GuardId, GuardReport)>;  // all guards: tick + duty_loop
pub fn run_guard_duty(&mut self, guard_id: GuardId, minutes: u64) -> Vec<GuardReport>;
pub fn deliver_guard_report(&mut self, guard_id: GuardId, report: GuardReport) -> u64; // event id
```

### 4.4 Warden
```rust
pub fn warden_cycle(&mut self) -> Vec<WardenAction>;   // arbitrate + apply
pub fn apply_warden_action(&mut self, action: WardenAction);  // public engine-side applier
pub fn warden_pending(&self) -> Vec<WardenAction>;
```

### 4.5 Parole (§15)
```rust
pub enum ParoleStatus { NotDue, Due, Scheduled, InHearing, Granted, Denied { reason: String } }
pub fn parole_due(&self) -> Vec<PrisonerId>;
pub fn start_parole_hearing(&mut self, id: PrisonerId) -> Option<u64>;
pub fn grant_parole(&mut self, id: PrisonerId) -> Option<u64>;
pub fn deny_parole(&mut self, id: PrisonerId, reason: impl Into<String>) -> Option<u64>;
pub fn parole_status(&self, id: PrisonerId) -> Option<ParoleStatus>;
```

### 4.6 Rehab (§14)
```rust
pub fn apply_rehab_progress(&mut self, id: PrisonerId, minutes: u64) -> f32; // returns new progress score
pub fn rehab_score(&self, id: PrisonerId) -> f32;
```

### 4.7 Observation
```rust
pub struct AgentObservation { /* prisoner view */ }
pub fn agent_observation(&self, id: PrisonerId) -> Option<AgentObservation>;
pub fn leader_board(&self, n: usize) -> Vec<(String, f32)>;
```
`Observation` gains: `leader_board: Vec<(String, f32)>`, `gang_members: u32`.
`Stats` gains: `gangs_formed`, `gang_recruits`, `max_population`.

### 4.8 Determinism (§22)
```rust
pub fn trace_hash(&self) -> u64;   // FNV-1a over (time, population, recent event summaries)
```

## 5. AI-facing design docs (`Documentation/Systems/`)

| Doc | Content |
|---|---|
| `engine-api.md` | How AI orchestrators use `Sim`: time, decisions, observation, stats, events |
| `guard-loop.md` | Guard duty loop contract (§10) |
| `warden-cycle.md` | Warden cycle + application contract (§9) |
| `parole-pipeline.md` | Parole pipeline contract (§15) |
| `rehab.md` | Rehab progress + program API (§14) |
| `event-contract.md` | Event emission + reporting contract (§17) |
| `determinism.md` | Determinism + trace_hash contract (§22) |

## 6. Implementation plan

Phase 3:
1. `crates/sumo/src/parole.rs` (ParoleLedger, pipeline).
2. `crates/sumo/src/drivers.rs` (guard duty, warden cycle + apply).
3. Extend `crates/sumo/src/observation.rs` (AgentObservation, Stats/Observation fields).
4. Extend `crates/sumo/src/lib.rs` (Sim methods: 4.1-4.8).
5. Tests: `tests/drivers.rs`, `tests/parole.rs`, `tests/expansion_api.rs`; extend stats/observation tests.

Phase 5: `Documentation/Systems/*.md`.

## 7. "Make Alcagent better"

- Guards/Wardens/Parole/Rehab loops become `sumo::Sim`-driven (one engine surface for AI's).
- Guard reports flow into events; wardens see them via inbox; AI's query world via `Observation`/`AgentObservation`.
- Determinism checks available to AI replay tests (`trace_hash`).
