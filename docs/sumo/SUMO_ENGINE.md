# sumo — Engine Product & System Design

`sumo` is a deterministic, event-driven **simulation game engine** written in Rust. This document
describes the product: what it is, its architecture, its subsystems, its public API and how to
use it. It is deliberately separate from the prison game built on top of it
([Project Alcagent](../README.md)); the same engine is intended to be reusable for other
simulation projects.

- **Crate:** `sumo` 0.1.0 (edition 2024, MSRV 1.85, MIT).
- **Repo:** standalone (`../sumo`), containing `sumo` plus 11 `alc-*` dependency crates.
- **Baseline:** builds cleanly; last verified green suite 184 passed / 0 failed / 1 ignored.
- **Public API guide:** [`SUMO_API.md`](SUMO_API.md) — exact signatures and usage examples.

---

## 1. Product vision

Today `sumo` powers a prison simulation. Its goal is to become a **general-purpose engine for
deterministic, agent-based simulations** with:

1. **Separation from any one game** — the core runs headless with no CLI/UI dependency.
2. **One authoritative core, many frontends** — CLI, terminal UI and (planned) GUI all drive the
   same `Sim` API.
3. **Determinism and replay** — seeded RNG, fixed timestep, state fingerprints.
4. **Emergent behavior** — autonomous agents, events with causes, consequences that ripple.
5. **Observability** — read-only snapshots, event logs, audit records, invariants, tracing.
6. **Persistence** — versioned saves with migration and autosave.
7. **Extensibility** — plugins/templates that let different project types reuse the engine.

---

## 2. Architecture overview

```
       ┌──────────────── Frontends (separate crates / repos) ────────────────┐
       │  alcagent CLI   │   alc-ui ratatui   │   (planned) GUI editor       │
       └──────────────────────────┬──────────────────────────────────────────┘
                                  │  uses only the public API
                          ┌───────▼────────┐
                          │   sumo::Sim    │  ← façade / engine control surface
                          └───────┬────────┘
   ┌──────────────┬───────────────┼───────────────┬──────────────┬─────────────┐
   │ Time & RNG   │ Decision/     │ Observation & │ Persistence  │ Invariants/ │
   │ (alc-core)   │ validation    │ events        │ (alc-persist)│ determinism │
   │              │ (alc-agents)  │ (alc-events)  │              │ (alc-testing│
   └──────────────┴───────────────┴───────────────┴──────────────┴─────────────┘
                          ┌───────▼────────┐
                          │  alc_world     │  ← domain World + tick loop
                          │  (domain layer)│    (currently prison-specific)
                          └───────┬────────┘
        ┌──────────┬───────────┬───────────┬───────────┬──────────┬──────────┐
        │ prisoner │ social    │ economy   │ warden    │ guard    │ agents   │
        └──────────┴───────────┴───────────┴───────────┴──────────┴──────────┘
```

**Dependency direction:** frontends depend on `sumo`; `sumo` depends on the domain crates; no
domain crate imports `sumo`, a UI crate, or the CLI. This is verified in the source (`use`
statements and Cargo manifests) and is the key property that lets `sumo` run entirely on its own.

---

## 3. System designs

### 3.1 Simulation loop (`Sim`/`World`)

`Sim::tick()` advances the world by one timestep determined by `TimeSpeed`
(`RealTime`=1 min, `FastForward`=5, `Extreme`=30, `EventDriven`=1). A tick:
drifts agent needs, runs a throttled hourly guard-duty sweep, resolves schedules, chooses
need-driven actions (trait-biased; AI deliberation is rate/token-gated with a deterministic
fallback), resolves co-presence encounters, and — on day boundaries — accrues finances,
progresses sentences, resolves escapes/parole, arbitrates warden policy, admits new agents,
and runs the in-flight rehab/contraband/construction steps.

`Sim` also reconciles engine-side policy state after the world mutates it, mirrors conflict
events into structured incident logs, records population history and attempts autosave.
`run_until`, `run_days`, `advance_by_secs` and `wait_for_event` are the bulk-advance APIs.
All time APIs are no-ops while paused (`Sim::new` and `Sim::load` start paused).

### 3.2 Decision pipeline (agent → engine)

Agents submit `alc_agents::Decision`s; the engine validates and applies them. Validation has
eight stages (format, semantic, authority, world-state, resource, legality, safety,
clamp/repair) followed by engine rules (existence of targets, lockdown restrictions, gang
membership). `DecisionOutcome` reports accepted/repaired/rejected and whether it applied.
Applied decisions are audited; conflict outcomes are mirrored to the incident log.

The **brain inspector** (`propose_decision`, `validate_only`, `evaluate_candidates`) is a pure,
read-only path that produces a `DecisionProposal` with drive scores, confidence, cited memories
and a rationale — designed for AI orchestration and inspector UIs without mutating the world.

### 3.3 Event bus and incidents

`alc_events::EventBus` is a bounded (50k) log of `WorldEvent { id, at, kind, summary, actors,
causes, confidence }`. Every event carries causes (the "explainable by underlying causes"
principle). A structured `Incident` sidecar adds causality (`cause`/`consequence`), severity,
location and involved agents, with filters (`IncidentFilter`) and audit records for decisions.
Caveat: mirrored conflict incidents currently use a self-referential cause/consequence placeholder.

### 3.4 Observation snapshots

`Observation`, `AgentObservation`, `Stats`, `MapFrame` and summary structs (`PrisonerSummary`,
`GangSummary`, `GuardSummary`, `WardenSummary`, `EconomySummary`, `SocialEdge`,
`FacilitySummary`, `ConstructionSummary`) are cheap, serializable, read-only projections of
world state. They are the only surface frontends need. `MapFrame` supplies pawn positions,
facility nodes, placeholder nav edges, locked nodes and incidents for a live map renderer.

### 3.5 Persistence

`alc-persistence` writes `world.bin` (versioned bincode, atomic tmp+rename) plus a JSON debug
sidecar. `EngineState` (parole ledger, population history, incident/decision/audit logs,
post-release, policy state/mode, autosave interval) round-trips alongside the world. Format
version 3; v1/v2 require migration/regeneration. Autosave runs on a configurable day interval;
the autosave directory is runtime-only and must be re-enabled after load.

### 3.6 Determinism

All stochastic behavior uses `RngStream` seeded from `(world seed, entity/time id)`, never
wall-clock time or hash iteration order (keys are sorted before use). `trace_hash()` (legacy
FNV fingerprint of time/population/events/incidents) and `trace_hash_v2()` (also mixes a full
state digest) let tests assert replay equality. 730-day three-seed chaos soaks verify stability.

### 3.7 Invariants and safety

`check_invariants()` checks population/capacity, integral non-negative money, unique IDs, one
place per pawn, legal intention targets, incident causality, suspicion references, policy-mirror
sync and bounded logs (max 10,000). Soak/invariant-negative tests inject bad states to prove the
checks fire.

### 3.8 Policy modes (multi-frontened decision making)

`PolicyMode::{Deterministic, Council, Shadow}` selects how warden decisions are made:
historical utility arbitration; deterministic council of seven drives with rationales; or shadow
mode which runs both, applies only the baseline and logs divergence for A/B evaluation.

### 3.9 GUI (current stub → planned)

The `gui` feature currently compiles a stub (`SumoApp` unit struct, `launch_gui()` returning
`Ok(())`); egui/eframe dependencies were removed from the standalone manifest. The planned GUI
is an editor/inspector over the public API — see [`SUMO_ENGINE_ROADMAP.md`](SUMO_ENGINE_ROADMAP.md)
and the deep research report in the parent repo. The shipped observer today is the parent's
`alc-ui` ratatui TUI.

---

## 4. Using the engine

```rust
use alc_core::TimeSpeed;
use sumo::Sim;

let mut sim = Sim::new(42);
sim.set_speed(TimeSpeed::FastForward);
sim.run_until(sim.world().time.add_days(30));
let view = sim.observation();
assert!(sim.check_invariants().is_empty());
sim.save("saves/demo")?;
```

Add it with `sumo = { path = "crates/sumo" }`. Full signatures, gotchas and test commands are in
[`SUMO_API.md`](SUMO_API.md).

---

## 5. What is domain-specific today (and what generalization requires)

The world model is currently prison-shaped: `Prisoner`, `Guard`, `Warden`, `Facility`, needs,
gangs, parole, economy of a prison. The *engine services* (time, decisions, events, observation,
persistence, determinism, invariants) are general. Making `sumo` usable across project types
means: a generic world/component registry behind traits, plugin registration for systems,
serializable arbitrary components, an app builder that selects domain packs, and a real GUI
editor. These are the subject of the deep research report; nothing is implemented yet.

---

## 6. Documentation map

| Document | Purpose |
|---|---|
| [`SUMO_API.md`](SUMO_API.md) | Exact API reference with examples and gotchas |
| [`SUMO_ENGINE.md`](SUMO_ENGINE.md) | This document: product and system design |
| [`Design/Engine/`](Design/Engine/) | Engine design rationale |
| [`Systems/`](Systems/) | Event contract, guard loop, warden cycle, determinism, etc. |
| [`agents.md`](../../agents.md) | Master development directive |
