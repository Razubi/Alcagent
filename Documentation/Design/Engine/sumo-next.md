# Sumo — Next Version Plan (Deep-Planning Draft)

- **Baseline:** `sumo` `0.5.0-beta.1` (beta 0.05) — delivered/planned scope per `Documentation/Design/Planning/beta-0.05.md`.
- **Next-version candidate:** `0.6.0`.
- **Mode:** **PLAN ONLY** — no implementation code changes. Informed by deep research + phase reports.
- **Finalization status:** FINALIZED — deep-planning deliverable (2026-11-20). No implementation code was modified.

## 1. Purpose

Define the feature set, sequencing, and open questions for the version of `sumo` that follows beta 0.05, consolidating the requested capability areas:

1. GUI / rendering capacities
2. Better AI support (guards / warden / prisoners)
3. Economy gameplay engine support
4. Better gameworld handling
5. Proper ETS (event transaction flow)

## 2. Grounding — current state (deep-research verified)

### 2.1 Workspace & crate topology

- 14 workspace members (`alc-core, alc-world, alc-agents, alc-prisoner, alc-social, alc-economy, alc-warden, alc-guard, alc-events, alc-persistence, alc-ui, alc-testing, alcagent, sumo`). **`alc-gui` is NOT a workspace member** — the experimental crate was removed; design-status.md was updated accordingly; re-introduction is the beta-0.05 deliverable.
- Workspace GUI pins: `egui = "0.37.0"`, `eframe = "0.37.0"` (`Cargo.toml`). Design `Documentation/Design/Planning/beta-0.05.md` had planned egui **0.36.2** → version drift to resolve in beta-0.05 implementation.

### 2.2 `sumo::Sim` API surface (`crates/sumo/src/lib.rs`)

| Concern | Methods | Lines |
|---|---|---|
| Time | `new`, `set_speed`, `speed`, `tick`, `run_until` | 56 / 66-69 / 70 / 73-84 / 88-99 |
| World access | `world`, `world_mut`, `days_simulated` | 101-103 |
| Events | `events`, `events_since` | 106-111 |
| Decisions | `deliver_decision` | 114-116 |
| Observation/stats | `observation`, `stats_since` | 119 / 122-138 |
| Persistence | `save`, `load`, `enable/disable/maybe_autosave` | 150-186 |
| Invariants/report | `check_invariants`, `report` | 190-234 |
| Engine emit | `emit_event` (pub), `emit_events` (`pub(crate)`) | — / 237-242 |
| Time expansion | `advance_by_secs`, `run_days`, `pause`, `resume`, `wait_for_event` | 251-289 |
| Guard drivers | `guard_duty_cycle`, `run_guard_duty`, `deliver_guard_report` | 302-314 |
| Warden drivers | `warden_cycle`, `apply_warden_action`, `warden_pending` | 319-336 |
| Parole | `parole_due`, `start_parole_hearing`, `grant_parole`, `deny_parole`, `parole_status` | 341-401 |
| Rehab | `apply_rehab_progress`, `rehab_score` | 407-424 |
| Observation | `agent_observation`, `leader_board` | 429-451 |
| Determinism | `trace_hash` | 457-470 |

- `Sim::tick` only calls `world.tick()` + population-history + autosave — it does **not** invoke guard/warden drivers (manual entry points only).
- `Sim::load` resets engine-scoped parole ledger + population history (persistence gap).
- `wait_for_event` unbounded loop guarded only by pause check (footgun).

### 2.3 Decision pipeline & engine gaps (`crates/sumo/src/decisions.rs`)

- `deliver` → `validate` → `apply`; `DecisionOutcome { agent, decision, result, applied }`.
- `engine_rules` ends with catch-all `_ => ok()` — every unhandled decision variant accepted (gap).
- Existing gates: `MoveTo` (lockdown/unknown facility); `Socialize|Trade|SpreadRumor|JoinGang|LeadGang` targets; `EscapeAttempt` lockdown.
- `MoveTo` writes `Prisoner.schedule.wing` (approximation); **`Trade` only applies relationship familiarity delta (effect-free)**; fight/escape event mapping exists.

### 2.4 World tick & day boundary (`crates/alc-world/src/lib.rs`)

- `World::tick` = six steps: needs drift; guard sweep (hourly, via `last_guard_duty_hour`); schedule resolution; need-driven decisions; co-presence encounters; day boundary.
- `on_day_boundary` = funding/expense accrual; facility degradation; sentence progress; escape; parole; security relax/lockdown expiry; roster right-sizing; gang formation; warden arbitrate+apply; orchestrator reset; intake; prune.
- Warden apply logic duplicated in `alc_world::World::apply_warden_action` and `sumo::drivers::apply_warden_action` → divergence risk.

### 2.5 Guard AI

- `Guard::duty_loop` (**stub**) vs `Guard::duty_tick` (**rich**) → unification is the top tactical AI item.
- Live path uses `duty_tick`; `duty_loop` is public contract but dead in live loop.
- Report → warden delivery: sev 0 none; sev 1 → `Argument`; sev 2 → `Threat`; ≥3 → `Investigation`.

### 2.6 Warden AI

- `Warden::inbox: Vec<u64>` (stub knowledge source); full `KnowledgeStore` (claims/provenance/standing/rumor) deferred — design-status gap #1.
- `arbitrate` state-aware overrides (cash<0 → ReduceExpenses; incidents>5 → IncreaseSecurity; pop>0.9×cap → Expand{cell_block}); `arbitrate_with_context`.

### 2.7 Economy / events / observation / persistence

- Economy core (money, accrual, health, budget plan/state/line) implemented; not wired into gameplay/UI.
- Events: taxonomy + `WorldEvent{causes,actors,confidence}`; bus bounded 50k; `World.history` declared but **never appended** (dead field).
- Observation: `Observation`, `AgentObservation` (thin — no traits/relationships/memories), `Stats`, `leader_board`.
- Persistence: `SaveManager` bincode+JSON, version 1, migration rejects unknown versions; parole ledger/population history not persisted.

## 3. Capability areas (plan)

### 2.1 GUI / rendering capacities (next evolution of `alc-gui`)

- Beta-0.05 delivered: egui App over `Sim`, HUD, control windows, phased panels. Next: rich rendering — map pan/zoom + layers, live charts (population/cash/events), inspector dock, minimap, timeline scrubber, save/load UI, speed control, theme; web via egui web later.
- Research: `SimDriver` trait (alc-ui/app.rs:27-38) is the engine-agnostic seam the egui App should implement/wrap; `alc-gui/src/data.rs` purity issue (egui `Color32` in pure layer); no cash-flow time series yet (`Economy` lacks history) → ledger-derived series needed.

### 2.2 Better AI support (guards / warden / prisoners)

- Unify `duty_loop` ↔ `duty_tick` (top tactical item); complete autonomy (authority, credibility, escalation); warden KnowledgeStore (claims/provenance/standing/rumor); prisoner AI capability gating (`CellEscape` — not yet enforced in `sumo::decisions::engine_rules`) + rehab-driven shifts.

### 2.3 Economy gameplay engine support

- Wire budget plan/state/line into world+UI; employment economy; contraband economy; fiscal feedback loops.

### 2.4 Better gameworld handling

- Consolidate day-cycle steps; intake driver; construction lifecycle; population management; correctness backlog (escape divergence, parole ledger, Trade effect-free, MoveTo wing approximation).

### 2.5 Proper ETS — event transaction flow

- Formalize Decision(command) → validate → apply → emit events(causes/actors/confidence) → projections; add tx id/idempotency/structured causes/rollback/compensation; full log + snapshotting; retention.

## 4. Deep-research-derived prioritized findings (next version)

**Phase A — foundation correctness (recommended first):**
1. Escape-path divergence (world removes escaped; engine path does not).
2. Parole ledger persistence on `Sim::load`; `ParoleDenied` reason loss.
3. Guard duty unification (`duty_loop` → `duty_tick`); World↔Sim driver duplication; report-mapping consistency.
4. `engine_rules` catch-all; capability-gated escape enforcement wiring; contraband world-step wiring; rehab Sim API decision; `emit_events` visibility.
5. Warden applier duplication (alc-world vs sumo/drivers).

**Phase B — gameworld handling:** intake/construction/population/day-cycle consolidation; `World::under_construction`; step-function extraction.

**Phase C — AI/economy depth:** warden KnowledgeStore; guard autonomy/relationships; finance wiring; employment/contraband economies.

**Phase D — rendering/GUI:** alc-gui module completion; data purity; charts; map camera; inspector; save/load UX; web; egui version strategy.

**ETS:** formalize command → transaction → events → projections.

## 5. Open research questions / decisions

1. ECS vs current struct-based world (design prison-simulation §6 strategy).
2. Full event sourcing vs hybrid transaction-log + mutable state.
3. Depth of warden knowledge & guard autonomy in v0.6.0.
4. egui version strategy (0.37 pinned vs 0.36.2 designed), web scope.
5. Charting library; cash-flow time-series design.

## 6. Next steps

- Finalize this plan from deep-research findings.
- Promote/refresh version-plan docs; proceed to Phase 3 implementation via the 5-subagent team.
