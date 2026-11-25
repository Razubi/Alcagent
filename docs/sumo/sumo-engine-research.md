# Deep Research Report — What `sumo` Should Become

> **Historical research snapshot (2026-11-24):** the findings below are retained as written.
> Current code has 25 stored need fields, 51 `EventKind` variants, 14 `WardenAction` variants,
> save format v5, and struct-based `Decision::new(DecisionAction::...)` construction. For current
> API/behavior use [`SUMO_API.md`](SUMO_API.md) and
> [`../alcagent/Project/PROJECT.md`](../alcagent/Project/PROJECT.md).

**Date:** 2026-11-24 · **Status:** report only (no code changed)
**Owner's requirements driving this research:**
1. `sumo` must run entirely separate from `alcagent`.
2. `alcagent` must use `sumo` for all game/world functions.
3. `sumo` must be usable for multiple different project types.
4. `sumo` must have a GUI interface.
5. `sumo` must be a proper engine for game development.

**Method:** three parallel research tracks: (A) primary-source research on modern engine
architecture (Bevy, Unity DOTS, eframe, bevy_inspector_egui, tracing, and Glenn Fiedler's
canonical articles); (B) a code audit of the standalone `sumo` repo; (C) an audit of Alcagent's
gameplay/controls and doc accuracy. Web *search* was unavailable; primary pages were fetched
directly. Findings are labelled **[Proven]** (primary source), **[Codebase]** (verified in code),
or **[Speculation]** (extrapolation).

---

## 1. Executive summary

`sumo` already has the hard part right: a deterministic, validated, observable simulation core
with a real invariant suite, state digests, replay fingerprints and 730-day soak tests
**[Codebase]**. What blocks it from being a *general engine* is structural, not functional:

- `Sim` is hard-wired to one concrete prison `World`; every public type is domain-typed
  (`PrisonerId`, `Decision`, `ParoleLedger`, `NeedTag`, `WardenAction`, `EventKind`) **[Codebase]**.
- No generic/trait seams: no `World<T>`, no component registry, no event trait, no system
  scheduler **[Codebase]**.
- The GUI feature is a 13-line no-op; the real observer lives in a separate crate **[Codebase]**.
- The tick loop conflates simulation and presentation concerns; there is no fixed-step driver
  with accumulator/interpolation, and no input replay **[Codebase]**.

**Recommendation:** do **not** rewrite as an ECS engine. Stage a refactor that preserves the
existing 184-test baseline: generic `Sim<W: WorldModel>` shell + `alcagent` as one world preset,
single driver authority, fixed-step driver + replay, thin CLI/GUI frontends over snapshots, then
plugins/typed registries, and generic events/IDs last (highest risk).

---

## 2. What `sumo` is today (verified baseline)

- `Sim { pub world: World, speed: TimeSpeed, pub engine_state: EngineState }`; `World` holds
  prisoners, guards, facilities, economy, social graph, gangs, warden, policies, event bus
  **[Codebase]**.
- Tick advances 1/5/30 simulated minutes by speed; `Sim::new` and `Sim::load` start **Paused**;
  all advance methods no-op while paused **[Codebase]**.
- Decision boundary: agents submit decisions; 8-stage validation + engine rules; only the engine
  mutates the world. `propose_decision` / `validate_only` / `evaluate_candidates` provide a pure
  "brain inspector" with drive scores, confidence, memory citations and rationale **[Codebase]**.
- Bounded 50k event bus with causes; structured incident sidecar with causality fields; engine
  incident/decision audit logs **[Codebase]**.
- Versioned bincode save + JSON sidecar; BTreeMaps for reproducible bytes; autosave scheduler
  **[Codebase]**.
- Determinism: seeded `RngStream` keyed by `(seed, id)`, `trace_hash` and `trace_hash_v2`,
  730-day 3-seed chaos soak + save/load replay regression **[Codebase]**.
- Invariants: population/capacity, money, unique IDs, pawn location, intention targets, incident
  causality, policy sync, bounded logs; negative tests inject violations **[Codebase]**.

**Strengths to preserve:** decision validation + explainable proposals, causality-carrying events,
state digest, invariant suite with negative tests, stable save bytes, soak tests,
`deny(missing_docs)` / `deny(unused_crate_dependencies)`.

---

## 3. Engine architecture patterns (primary-source research)

### ECS vs OOP
**[Proven]** Bevy's ECS is the dominant Rust pattern: components are plain structs, systems are
functions, systems run in parallel by default, ordering via system sets/`.chain()`. Resources
hold globally unique data (time, assets, config) — directly analogous to sumo's `EngineState`,
clock and policy state. Unity DOTS uses the same data-oriented ECS model.
Sources: <https://bevy.org/learn/quick-start/getting-started/ecs/>,
<https://bevy.org/learn/quick-start/getting-started/resources/>,
<https://docs.unity3d.com/Packages/com.unity.entities@1.0/manual/index.html>.

**Recommendation [Speculation grounded in sources]:** do **not** convert the prison `World` to
ECS — its social graph, gangs and parole ledger are densely relational and already tested.
Offer an optional ECS *adapter* for rendering: simulation stays authoritative; Bevy/engine
entities consume `MapFrame`-style snapshots only.

### Event-driven & decoupling
**[Proven]** Bevy's plugin model: "Want a headless server? Don't register the `RenderPlugin`";
`MinimalPlugins` exists for exactly this. Plugins are `impl Plugin for P { fn build(&self,
app: &mut App) }`, support generics (`YourPlugin<T>`), and should use small crates and documented
compatible versions. Source: <https://bevy.org/learn/quick-start/getting-started/plugins/>,
<https://bevy.org/learn/quick-start/plugin-development/>.

**Recommendation:** one core, many frontends:
```
sumo-core      (no GUI/CLI deps): Sim<W>, traits, event/action, digest, invariants, save
sumo-cli       (thin): headless soak, replay, diagnostics, fuzz entrypoint
sumo-gui       (thin, optional): eframe/egui observer over Observation/MapFrame snapshots
alcagent       (game crate): alc-* domain crates implement WorldModel for the prison world
```
Feature flags: `default = []`, `gui = ["sumo-gui"]`, `alc = ["alcagent"]` — mirroring Bevy's
headless/default-features guidance. This satisfies requirements (a) and (b): `sumo-core` has no
`alc-*` dependency; `alcagent` plugs its world into the engine.

### Fixed timestep & jobs
**[Proven]** Glenn Fiedler's accumulator pattern: renderer produces time, simulation consumes
it in fixed `dt` steps; clamp large frame deltas to avoid the "spiral of death"; cap steps per
frame; render with interpolation alpha. Source:
<https://www.gafferongames.com/post/fix_your_timestep/>.
**Recommendation:** add `FixedStepDriver` around `Sim<W>` with accumulator, `max_steps_per_frame`
and `interpolation_alpha()`. Interpolation is display-only; never feed it back into simulation
state. Parallel/job execution (Bevy systems) is valuable but conflicts with strict determinism —
defer until after the determinism contract is enforced.

---

## 4. Generality: the required seams

**[Proven mechanisms]** Bevy generic plugins (`Plugin<T>`), Resources for singletons, and
`bevy_inspector_egui`'s `Reflect`/`register_type::<T>()` approach to inspecting arbitrary types
(<https://docs.rs/bevy_inspector_egui/latest/bevy_inspector_egui/>). Serde is already in sumo.

**Proposed generality seam (design, not implemented):**
```rust
pub trait WorldModel: Serialize + DeserializeOwned {
    type Event: EngineEvent;
    type Action: Action;
    type Config: Default;
    fn tick(&mut self, dt: SimDuration, ctx: &TickContext);
    fn observation(&self) -> Observation;
    fn events_since(&self, cursor: EventCursor) -> Vec<Self::Event>;
    fn digest(&self) -> u64;
    fn check_invariants(&self) -> Vec<Violation>;
}
pub struct Sim<W: WorldModel> { world: W, clock: Clock, plugins: PluginRegistry<W>, ... }
```
Game-specific content (prisoners, gangs, parole) becomes one `WorldModel` impl. Keep engine
services generic: validation + dry-run, event bus, digest/replay, autosave, invariant registry,
structured audit. Provide application templates (minimal, agent-sim, replay CLI) to prove
generality. Scripting/modding hooks should produce validated `Action`s only — never arbitrary
state writes (generalizes AGENTS.md §19).

---

## 5. GUI/editor interface

**[Proven]** `eframe` implements `App` + `App::ui`, runs natively or wasm from one codebase, with
optional state persistence and an `inspection` feature
(<https://docs.rs/eframe/latest/eframe/>). `bevy_inspector_egui` provides drop-in
world/resource/state/asset inspectors and pairs with `egui_dock` for docked editors in <100
lines. Current sumo GUI is a stub: `SumoApp` unit struct, `launch_gui() -> Ok(())` **[Codebase]**.

**Recommended minimal real GUI (priority):**
1. Playback bar (pause/resume/step/1-tick/1-day/speed/seek by replay) — unique to a sim engine.
2. Entity/agent inspector (identity, needs, goals, intention, relationships, memory citations) —
   reuse `AgentObservation` and `DecisionProposal::explanation()`.
3. Event timeline / incident log with `IncidentFilter`, cause→consequence chain, click-to-focus.
4. Map view from `MapFrame` — but replace the placeholder grid/ring nav first **[Codebase]**.
5. Stats/finance dashboards, social network, warden decision log, settings (seed/speeds/autosave).
6. Debug overlays: trace spans, invariant violations, digest/replay status, tick rate, bus fill.

The GUI must depend only on snapshots and command objects — UI sends `Command`s consumed by the
engine at tick boundaries, never `&mut Sim` directly. This keeps UI input in the replay stream.

---

## 6. Determinism & replay

**[Proven]** Deterministic lockstep = same initial condition + same inputs ⇒ bit-identical
result; record *inputs*, not state; fixed `dt` is a precondition; floats are the main pitfall —
cross-compiler/cross-architecture FP determinism requires strict IEEE-754, no FMA/reassociation,
no transcendentals, controlled rounding, and is "incompatible with most forms of parallelism".
Sources: <https://www.gafferongames.com/post/deterministic_lockstep/>,
<https://www.gafferongames.com/post/floating_point_determinism/>.

**Sumo's position [Codebase]:** seeded streams + digest + soak are strong, but `f64`
needs/traits feed RNG thresholds; cross-machine determinism is **not guaranteed** by seeds alone.
`trace_hash_v2` may legitimately differ across save/load while legacy `trace_hash` matches —
investigate non-canonical iteration/float formatting.

**Recommendations:**
1. Publish a determinism contract: same seed + same input stream + same build/ISA ⇒ same digest
   per tick; add CI golden-replay.
2. Add input/action replay logging: `(tick, agent_id, action)` + warden/guard/UI commands;
   `Sim::record_replay()` / `Sim::replay()`; use the 730-day soak as a golden fixture.
3. Quantize RNG-threshold comparisons to fixed-point integers where cheap; keep `f64` for display.
   Use deterministic iteration (BTreeMap) in digests — partially done.
4. UI inputs become frame-tagged commands in the replay stream from day one.
5. Extend Shadow mode to replay divergence detection at checkpoints.

---

## 7. Engineering practices for engines

**[Proven]** `tracing` provides spans/events/structured fields/subscribers; libraries should
depend only on `tracing`, executables install a subscriber (<https://docs.rs/tracing/latest/tracing/>).
Sumo already has one `tracing::debug!` in `tick()` but no subsystem spans **[Codebase]**.

**Recommendations:**
- `#[tracing::instrument]` on `tick`, drivers, `deliver_decision`, save/load; ship
  `sumo-cli diag` with a tracing subscriber.
- Make invariant violations carry replay tickets `(seed, tick, last N inputs)`.
- Surface autosave errors (currently swallowed with `let _ =`) and fix the lost autosave
  directory on load **[Codebase]**.
- Replace self-referential incident causes with real chains from event `causes` **[Codebase]**.
- Add fuzz/property tests for the decision validator (the validated boundary makes this cheap),
  memory-growth assertions, configurable event-bus capacity, and a stable cross-platform digest
  (replace `DefaultHasher` in `state_digest.rs`, which is explicitly non-portable **[Codebase]**).

---

## 8. Feature catalog: what similar engines have that sumo lacks

Priority: **Must** (needed for the owner's goals), **Should** (high value), **Could** (nice-to-have).
Effort: S (<1 wk), M (1–4 wk), L (>1 mo), one developer.

| # | Feature | sumo today | Priority | Effort |
|---|---|---|---|---|
| 1 | `WorldModel` trait + generic `Sim<W>` | Concrete `World` | **Must** | M |
| 2 | Core crate with no `alc-*` deps | Re-exports 8 domain crates | **Must** | M |
| 3 | Frontend crates behind features (CLI/GUI) | GUI stub; TUI external | **Must** | M |
| 4 | Fixed-step driver + accumulator + interpolation alpha | Logical tick + speed enum | **Must** | S–M |
| 5 | Input/action replay recording & playback | Digest + soak, no input log | **Must** | M |
| 6 | Determinism contract + golden replay CI | Implicit | **Must** | S |
| 7 | Single driver authority (no double-driving) | `World::tick` + explicit drivers duplicate | **Must** | M |
| 8 | Fix incident causality / autosave / mirror / summary stubs | Placeholders and footguns | **Must** | S–M |
| 9 | Structured `tracing` spans per subsystem | One debug line | **Should** | S |
| 10 | Plugin + typed component registry | Hard-coded modules | **Should** | M |
| 11 | Real GUI (playback/inspector/timeline/map/dashboards) | Stub | **Should** | M (L full) |
| 12 | Real nav geometry/pathfinding for `MapFrame` | Placeholder grid/ring | **Should** | M |
| 13 | Single event source of truth + real cause chains | Bus + 2 mirrors, self-causes | **Should** | M |
| 14 | Versioned pluggable persistence + migration chain | v1→v2 only, duplicated schema | **Should** | M |
| 15 | Engine-wide finite guards + tick budgets | Partial | **Should** | S |
| 16 | Generic event payloads with prison taxonomy as plugin | Closed 39-variant enum | **Could** | L (high risk) |
| 17 | Generic identity layer (`EntityId`) | Domain IDs everywhere | **Could** | L (high risk) |
| 18 | ECS-lite component storage | None | **Could** | L (high risk) |
| 19 | Assets/scenes, scripting, networking, undo/redo | None | **Could** | L each |
| 20 | Fuzz/property tests, cross-platform digest | Partial | **Could** | M |

---

## 9. Codebase audit: current generalization blockers (P0/P1/P2)

### P0 — blockers (≈6–10 weeks)
1. **Positioning decision** — either reposition sumo as "Alcagent simulation engine" (honest,
   cheap) or commit to generality: update `Cargo.toml:6`, README and stale docs (some still say
   sumo depends on `alc-ui`, save format v2 vs v3) **[Codebase]**.
2. **Single driver authority** — unify `World::tick` guard/warden cycles with `simo::drivers`;
   test the no-double-drive invariant.
3. **Correctness footguns** — real incident causality; autosave directory + error surfacing;
   policy-mirror drift (two scales 0–5 vs 0..1); complete summary fields (`WardenSummary.inbox`
   empty, facility wing/staff zero, gang intensities flattened, `involved_guards` dropped);
   escape-path divergence (engine discards consequences; world removes prisoners); configurable
   event retention.
4. **Generic `Engine<W,S>` + `SimBuilder`** with a prison preset preserving today's API;
   `Sim::new(seed)` delegates to the preset.
5. **Adapter split** — move prison observation/summary/`report()` into a `sumo-views` crate or
   feature; keep generic snapshot traits in core.

### P1 — make extensibility real (≈6–10 weeks)
6. Plugin/system registry: `System`, `DecisionHandler`, `Invariant`, `Observer` traits with
   deterministic ordering; migrate the tick loop and invariants.
7. Pluggable persistence with per-component providers, independent versions, schema hash, real
   binary migration fixtures; remove the sumo↔alc_persistence schema duplication.
8. Generic event payloads with the prison taxonomy as one plugin (explicit version bump).
9. Engine-wide finite/bounded guards; tick budgets in `wait_for_event`/`run_until`.
10. GUI seam: expose a `SimDriver`-style trait; retire or properly implement the `gui` feature;
   no toolkit deps in the engine core.

### P2 — polish/scale (≈4–8 weeks)
11. Generic identity layer + generic `SocialGraph`/`Incident`/`MapFrame`.
12. ECS-lite component storage with serde registration.
13. Parallel system execution where deterministic; benchmark harness; stable digest (replace
    `DefaultHasher`).
14. **A second game preset** — the real proof of generality (e.g. a generic "colony" sim).

---

## 10. Gameplay/doc audit findings (track C)

- Alcagent has **11 live views** (9 core + Incident Reports + Decision Audit), not 9 **[Codebase]**.
- ~14 active TUI keys were undocumented at audit time (`Ctrl-L`, `x/X`, `t/T`, `r/R`, `f/F`,
  `m/M`, `o/O`, `d/D`, `i/I`, lowercase `a`); `run` defaults to 30 days; `--speed 0` is a hard
  error. Current keys are listed in `crates/alcagent/src/main.rs` help and the gameplay manual.
- Code has **25 stored need fields**, not 29; `EventKind` has **51 variants** today (the audit
  counted 47, not the 39 in older docs).
- `UiState.search` now has `/` bound; `Ctrl-W` clears the query.
- Parole has three disagreeing authorities (world's automatic daily hearing, `ParoleBoard`,
  sumo manual ledger); the warden's recommendation is recorded but not applied to release chance.
- The in-flight wave changed `Decision` from an enum to a struct; the 0.05 release is green with
  `Decision::new(DecisionAction::...)` construction.

Gameplay/control corrections are maintained in this repo's
[`../alcagent/ALCAGENT_GAMEPLAY.md`](../alcagent/ALCAGENT_GAMEPLAY.md) and
[`../alcagent/ALCAGENT.md`](../alcagent/ALCAGENT.md); engine findings are reflected in
[`SUMO_ENGINE.md`](SUMO_ENGINE.md), [`SUMO_API.md`](SUMO_API.md) and the root README.

---

## 11. Recommended sequence (foundations first)

- **Phase A — Decouple:** `sumo-core` with `WorldModel` and core traits; domain types behind
  feature/`alcagent`; prove with existing tests.
- **Phase B — Loop & replay:** `FixedStepDriver`, input log/replay, determinism contract, golden
  replay CI.
- **Phase C — Frontends:** `sumo-cli` (headless/diagnostics/replay/fuzz), `sumo-gui` (eframe +
  panels over snapshots).
- **Phase D — Extensibility:** plugins, typed registries, real event causes, migration chain,
  tracing spans.
- **Phase E — Optional depth:** ECS adapter, assets/scenes, scripts, parallel scheduling —
  only after the determinism contract is enforced.

Do **not** start with a Bevy rewrite or a full GUI: both depend on the abstraction and loop work.

---

## 12. Questions for the owner (please answer before implementation)

1. **Positioning:** is sumo a *simulation framework* (headless core + observer GUI) or a *general
   game engine* that must render 2D/3D scenes and own assets? This decides whether ECS/assets are
   in scope at all. (My recommendation: simulation framework with plugin/frontend seams.)
2. **Determinism bar:** must it hold across different machines/compilers/build profiles, or only
   same-binary replay? (Same-build is cheap; cross-build needs fixed-point hygiene.)
3. **World abstraction boundary:** should gameplay rules (needs, gangs, parole) live entirely in
   `alcagent`, with sumo keeping only time/events/actions/persistence/invariants? (Recommended.)
4. **Event bus ownership after generalization:** core `Sim` or each `WorldModel` impl? Single bus
   with real cause chains, or keep mirrored logs? (Recommendation: single bus, real chains.)
5. **GUI stack:** eframe/egui native-only, native+wasm, or Bevy + `bevy_inspector_egui`?
   (Recommendation: eframe native+wasm first; Bevy only if 2D/3D rendering is required.)
6. **Stub `gui` feature:** delete outright, or replace in place behind opt-in deps?
7. **Save-format policy:** strict version chain with migrations forever, or regenerate old saves
   (docs currently suggest regenerate for v3)? This affects arbitrary plugin-component saving.
8. **UI input in replay from day one?** Strongly recommended — it is what makes bug reproduction
   and timeline scrubbing possible.
9. **Scale target before ECS/jobs:** dozens, hundreds or thousands of agents? Parallelism may not
   pay for itself and conflicts with strict determinism.
10. **`alc-ui` destination:** should its panels migrate into `sumo-gui`, or stay a separate
    consumer talking to sumo only through snapshots? (Requirement (a) favors keeping it external.)
11. **Verification bar before publishing docs:** require a green `cargo build --workspace &&
    cargo test --workspace` and re-verify every key/flag against `--help`, or document intended
    in-flight behavior? (Recommendation: green-first for "stable" labels.)

---

## 13. Sources

- Bevy ECS/Resources/Plugins/authoring: <https://bevy.org/learn/quick-start/getting-started/ecs/>,
  <https://bevy.org/learn/quick-start/getting-started/resources/>,
  <https://bevy.org/learn/quick-start/getting-started/plugins/>,
  <https://bevy.org/learn/quick-start/plugin-development/>
- Unity DOTS Entities: <https://docs.unity3d.com/Packages/com.unity.entities@1.0/manual/index.html>
- bevy_inspector_egui: <https://docs.rs/bevy_inspector_egui/latest/bevy_inspector_egui/>
- eframe: <https://docs.rs/eframe/latest/eframe/> · tracing: <https://docs.rs/tracing/latest/tracing/>
- Glenn Fiedler: <https://www.gafferongames.com/post/fix_your_timestep/>,
  <https://www.gafferongames.com/post/deterministic_lockstep/>,
  <https://www.gafferongames.com/post/floating_point_determinism/>
- Local codebases: `C:\Users\matth\OneDrive\Desktop\sumo` and
  `C:\Users\matth\OneDrive\Desktop\Project Alcagent` (source, tests, docs, `git diff`).

**Confidence note:** architecture, loop, determinism, GUI-library and tracing claims are backed by
the primary sources above. Comparisons involving Godot/Unreal internals and Rust ECS crate
popularity were not web-searchable and are omitted or marked speculative; verify before any
irreversible dependency choice.
