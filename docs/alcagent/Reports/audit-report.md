# Audit Report — Project Alcagent

**Date:** 2026-11-20
**Status:** Complete — findings + fixes + new project `sumo` (see below).
**Workspace:** `<repository root>` (Windows/PowerShell, cargo 1.100.0, rustc 1.100.0)

---

## 1. Executive summary

The workspace contains **13 existing crates** plus the new **`sumo`** engine crate. The audit found:

1. **Build was broken**: `cargo build` failed with 15 compile errors in `alc-social` (missing `Default` impls for `PrisonerId`/`IncidentKind`/`EscapeOutcome`/`SimTime`, missing serde derives on `RelationshipDelta`, `SimTime` type mismatches in `gang.rs`) and 2 missing `Default` impls in `alc-core` (`PrisonerId`). **Fixed.**
2. **Warning cleanup**: removed unused workspace deps (`alcagent`, `chrono`, `parking_lot`), unused crate deps, and added `[lints] workspace = true` to all crates.
3. **Added `sumo`** — the simulation engine: owns the `alc_world::World` and exposes the engine API (time control, validated decision pipeline, observation/stats, persistence + autosave, invariants). Added `gangs`/`next_gang_id` to `alc_world::World` so gangs are world state.
4. **Baseline**: `cargo build` now exits 0; `cargo test` exits 0 (alc-agents 18, alc-world 4, alc-prisoner 16, alc-social 7, sumo 22 tests, plus doctests).

---

## 2. Existing codebase inventory (Phase 1, current disk state)

| Crate | Files (current) | Responsibility (summary) |
|---|---|---|
| `alc-core` | 6 (`lib.rs`, `ids.rs`, `rng.rs`, `simtime.rs`, `config.rs`) | Time (`SimTime`), RNG (`RngStream`), IDs, `Config`. No simulation logic. |
| `alc-world` | 1 (`lib.rs`, ~1.2 KB) | `World`: facilities, population, tick loop, economy, warden, social graph, events, history, orchestrator. |
| `alc-agents` | 4 (`lib.rs`, `decision.rs`, `memory.rs`, `orchestrator.rs`) | Structured `Decision` + 8-stage validation pipeline (`decision.rs`), 4-layer memory, 3-tier orchestrator (`orchestrator.rs`). |
| `alc-prisoner` | 4 (`lib.rs`, `needs.rs`, `schedule.rs`, `intake.rs`) | Prisoner entities, needs (drift/relief/effects), schedules, intake generation. |
| `alc-social` | 4 (`lib.rs`, `conflict.rs`, `escape.rs`, `gang.rs`) | Relationships, gangs (hierarchy, fractures, alliances), conflict/escape resolution. |
| `alc-economy` | 1 | Ledger, revenue/expense engine, health heuristics. |
| `alc-warden` | 3 (`lib.rs`, `expansion.rs`, `finance.rs`) | Warden AI (traits, objectives), expansion catalog, annual budget plan. |
| `alc-guard` | 1 | Guard entities: ranks, posts, authority levels, shifts, duty loop, reports. |
| `alc-events` | 1 | Event bus + event taxonomy (`WorldEvent`, `EventKind`). |
| `alc-persistence` | 1 | Versioned save (bincode + atomic rename + JSON sidecar), SaveManager. |
| `alc-ui` | 1 | View modes (§20 observation surface), minimal. |
| `alc-testing` | 1 | Invariants + soak scenarios. |
| `alcagent` | 1 (`main.rs`) | CLI: `run` (headless), `observe`, `docs`. |
| **`sumo`** | `Cargo.toml` + `src/{lib,decisions,observation,autosave}.rs` + `tests/{engine_api,decisions,observation,persistence,determinism,soak}.rs` | **New: the simulation engine.** See below. |

Note: several files evolved during the audit (tooling staleness; the `read` tool and `edit` tool disagreed with on-disk state at times). `cargo`/`rustc` were treated as the source of truth.

## 3. Build status (fixed)

**Before:** `cargo build` → exit 101, 15 errors in `alc-social` (E0277 `PrisonerId: Default` in `InjuryRecord`/`DisciplineProposal`/`ReputationDelta`/`ConflictOutcomeEffects`/`GangFormationCandidate`; E0277 `IncidentKind: Default`; E0277 `EscapeOutcome: Default`; E0277 `RelationshipDelta: Serialize/Deserialize`; E0277 `SimTime: Default` in `gang.rs`; E0308 type mismatches).
**After:** `cargo build` → exit 0, `cargo test` → exit 0.

Fixes applied (minimal, additive):
- `alc-core/src/ids.rs`: `impl Default for PrisonerId { Self(0) }`.
- `alc-core/src/simtime.rs`: `Default` derive on `SimTime`.
- `alc-social/src/lib.rs` (relationships): `RelationshipDelta` — serde derives added.
- `alc-social/src/conflict.rs`: `IncidentKind` — `#[default]` variant added.
- `alc-social/src/escape.rs`: `EscapeOutcome` — `#[default]` variant added.
- `alc-world/src/lib.rs`: added `gangs: Vec<Gang>` + `next_gang_id` to `World` (gangs are world state per Directive §7).

## 4. Warning cleanup

- Root `Cargo.toml`: removed unused workspace deps (`alcagent`, `chrono`, `parking_lot`); added `sumo`.
- Removed unused deps: `alc-core` (anyhow, serde_json, thiserror, tracing, uuid), `alc-economy` (alc-events), `alc-guard` (alc-events), `alc-ui` (alc-world, ratatui, crossterm), `alc-world` (bincode), `alcagent` (alc-testing, alc-ui), `alc-social` (alc-economy, alc-warden, alc-guard, serde_json, thiserror, tracing — its Cargo.toml evolved to a smaller dep set).
- Added `[lints] workspace = true` to every crate.

## 5. The new project: `sumo` (the simulation engine)

**Design:** `../Design/Engine/sumo-engine.md` (written in Phase 2).
**Implementation:** `crates/sumo/` — `Sim` owns `alc_world::World` and drives it:

- **Time control** — `tick`, `run_until(target)`, `speed` (`TimeSpeed`).
- **Decision pipeline** — `Sim::deliver_decision(id, Decision)`: agents propose `alc_agents::Decision`s; the engine validates (`alc_agents::DefaultValidator` + engine rules) and applies (world mutations + events). Agents never mutate world state (Directive §19).
- **Observation** — `Observation::capture(&World)`: time, days, population/capacity, guards, facilities, cash, economy health, security level, lockdown, warden identity/philosophy, gangs, notable prisoners, recent events.
- **Stats** — `Stats::from_events`: admissions, releases, conflicts, medical, security incidents, parole hearings, avg population.
- **Events** — `events()` / `events_since(t)` (Directive §17).
- **Persistence** — `save`/`load` via `alc-persistence`; **autosave** via `AutosaveScheduler` (`enable_autosave`/`maybe_autosave`).
- **Invariants** — `check_invariants()` (capacity, money integral, unique ids) via `alc-testing`.
- **Report** — `report()`.

Dependencies: all `alc-*` crates (`alc-core`, `alc-world`, `alc-agents`, `alc-prisoner`, `alc-social`, `alc-economy`, `alc-warden`, `alc-guard`, `alc-events`, `alc-persistence`, `alc-ui`, `alc-testing`) + `serde`, `tracing`.

**Tests:** `cargo test -p sumo` — 22 passing (engine_api 4, decisions 11, observation 3, persistence 2, determinism 1, soak 1).

## 6. Discrepancies and notes

- `architecture-report.md (removed)` is **stale** relative to the code (describes a different `SimTime`/`WorldView` model than `alc-core`/`alc-world`). Not updated; superseded by `../Reports/audit-report.md` + `../Design/Engine/sumo-engine.md`.
- `../Design/*.md` (prison-simulation, social-systems, warden-system, ai-agent-systems) are design docs ahead of code; kept as-is.
- The real `alc-agents` API is richer than an early read suggested: 8-stage validation pipeline, memory, orchestrator, `DecisionOutcome` etc. (read fresh during audit).
- Save format: v1 kept; new `World.gangs`/`next_gang_id` fields are `#[serde(default)]` so v1 saves load.

## 7. Gaps noticed (not fixed — out of scope)

- `World::tick` still lacks guard patrol routines, prisoner AI decisions, etc. — subsystems are what `sumo` will drive once the agent layers exist.
- `Prisoner` has no location field; `Decision::MoveTo` application is wing-only (documented approximation).
- No post-release tracking (Directive §15) yet.

## 8. Verification

- `cargo build` → **exit 0** (`dev` profile).
- `cargo test` → **exit 0** (all targets; totals: alc-agents 18, alc-world 4, alc-prisoner 16, alc-social 7, sumo 22 + doctests 0).

## 9. Recommendations

- Wire `sumo` into `alcagent` (use `Sim` instead of direct `World`) — planned incrementally.
- Add an engine `lib` unit-test for `Sim::report`/autosave scheduling cadence.
- Re-audit `../Design/` docs once `sumo` lands; delete or mark stale `architecture-report.md (removed)`.
