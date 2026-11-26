# sumo — Engine Product & System Design

`sumo` is a deterministic, event-driven **simulation game engine** written in Rust. This document
describes the product: what it is, its architecture, its subsystems, its public API and how to
use it. It is deliberately separate from the prison game built on top of it
([Project Alcagent](../README.md)); the same engine is intended to be reusable for other
simulation projects.

- **Crate:** `sumo` 0.5.0-beta.1 (workspace version; edition 2024, MSRV 1.85, MIT).
- **Repo:** this workspace (`crates/sumo` plus its `alc-*` dependency crates). An out-of-tree
  snapshot may exist at `../sumo`, but the docs in this repository are canonical.
- **Baseline:** builds cleanly; workspace tests are green, including 365/730-day soaks.
- **Public API guide:** [`SUMO_API.md`](SUMO_API.md) — exact signatures and usage examples.

---

## 1. Product vision

Today `sumo` powers a prison simulation. Its goal is to become a **general-purpose engine for
deterministic, agent-based simulations** with:

1. **Separation from any one game** — the core runs headless with no CLI/UI dependency.
2. **One authoritative core, many views** — one `alcagent` binary exposes the egui graphical
   observer (default), the ratatui debug observer (`--tui`) and headless runs, all over the same
   validated `Sim` API.
3. **Determinism and replay** — seeded RNG, fixed timestep, state fingerprints.
4. **Emergent behavior** — autonomous agents, events with causes, consequences that ripple.
5. **Observability** — read-only snapshots, event logs, audit records, invariants, tracing.
6. **Persistence** — versioned saves with migration and autosave.
7. **Extensibility** — plugins/templates that let different project types reuse the engine.

---

## 2. Architecture overview

```
       ┌────────────── Frontends (one binary, adapter crates) ──────────────┐
       │ alcagent GUI  │  alcagent --tui  │  headless run                   │
       │  (alc-gui)    │    (alc-ui)      │  (alcagent)                     │
       └──────────────────────────┬──────────────────────────────────────────┘
                                  │  shared display models: sumo::graphics
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
and runs the wired rehab/contraband/construction steps. Weekly gang politics runs inside the
day-boundary phase as well.

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
facility nodes, campus zones, schematic roads, locked nodes and incidents for a live map renderer.
Shared display models (palette, view catalog, world-map layout/zones, camera math and the
construction-kind catalog) live in `sumo::graphics`; frontends are rendering/input adapters and
never mutate arbitrary world state.

### 3.5 Persistence

`alc-persistence` writes version-5 directories containing `world.bin` (versioned bincode, atomic
tmp+rename) plus a JSON debug sidecar. `EngineState` (parole ledger, population history,
incident/decision/audit logs, post-release, policy state/mode, autosave interval and directory)
round-trips alongside the world; v1/v3/v4 saves are migrated on load. Autosave runs on a
configurable day interval and its interval/directory survive save/load. Save discovery
(`list_saves` / `probe_save_dir`) exposes version, compatibility status, population and sim time.
Graphical observer preferences are separate JSON under `saves/alc-gui/settings.json`.

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

### 3.9 Observers (outside `sumo`)

`sumo` has no GUI feature or `SumoApp`; the shipped observers live in the `alcagent` binary. The
egui/eframe graphical observer is the default (`alc-gui` adapter, Main Menu with a seed field +
**Start** (new world), Load Existing Save, Settings and Quit); `alcagent --tui` opens the ratatui
debug observer (`alc-ui` adapter). Both use `sumo::graphics` models and validated `Sim` controls.
A future engine-side editor/inspector is not part of the current API — see
[`SUMO_ROADMAP.md`](SUMO_ROADMAP.md) for planning context.

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
| [`../alcagent/Design/Core/`](../alcagent/Design/Core/) | Game/domain design rationale |
| [`../alcagent/Systems/`](../alcagent/Systems/) | Event contract, guard loop, warden cycle, determinism, etc. |
| [`agents.md`](../../agents.md) | Master development directive |
