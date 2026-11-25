# sumo GUI & Long-Term Support Plan

**Status:** planning input for the v0.5 world/gameplay plan · **Date:** 2026-11-24
**Owner's directive:** sumo must keep evolving to support future needs and capabilities while
Alcagent grows in gameplay/features. Specifically: **address sumo's GUI concerns**.

This document is a working annex to the world plan. It will be reconciled with the deep research
reports when they land.

---

## 1. Principle: two release tracks, one dependency direction

```
sumo (engine)        — stable API, general-purpose, forward-compatible, owns GUI/editor layer
   ▲ depends-on
   │
alcagent (game)      — gameplay content, world/town, prison, families, schools, jobs, crime
```

- **sumo changes** are infrastructure: engine APIs, world-model abstractions, event/persistence
  contracts, replay/determinism, debugging and **the GUI/editor shell**.
- **Alcagent changes** are content and rules: prison society, town/citizens, gameplay systems,
  view-specific panels that sit *inside* the sumo GUI shell or the ratatui TUI.
- Dependency direction stays `game → engine`. The engine must never import gameplay code or a
  game-specific UI crate. This is what lets sumo support *multiple* future projects.

## 2. GUI concerns to resolve (current state)

- `crates/sumo/src/gui.rs` is a 13-line no-op: `SumoApp` unit struct, `launch_gui() -> Ok(())`.
- The `gui` cargo feature is empty (`gui = []`) and the egui/eframe deps were removed from the
  standalone manifest; docs call it a stub and point to the game's `alc-ui` ratatui observer.
- The real observer (`alc-ui`) lives in the **parent game repo** and depends on `sumo`, so the
  engine currently has *no* GUI of its own.
- `MapFrame` geometry is a placeholder (synthetic grid + ring of facility ids), so a real map
  needs a spatial/nav model before a GUI map is useful.

## 3. Target: a real engine GUI (owned by sumo, generic over the world)

The GUI is an **engine product**: it renders snapshots and sends commands through a stable
`SimDriver`-style trait; it knows nothing about prisoners or schools. Game-specific panels are
registered by the game (Alcagent) via a plugin/panel trait.

Required capabilities (priority order):
1. **Playback bar** — pause/resume, step tick, step day, speed selector, seek by replay.
2. **Entity inspector** — generic object list + detail (identity, needs/goals/state, current
   intention, relationships, memory) rendered from generic observation types.
3. **Event / incident timeline** with filters and cause→consequence chain view.
4. **Map / scene viewport** — pan/zoom, layers, picking, overlays; fed by a real spatial model.
5. **Stats/invariant/debug overlays** — trace spans, invariant violations, digest/replay status,
   tick rate, event-bus fill.
6. **Settings/project loader** — seed, speeds, autosave, world/project selection.
7. **Plugin panel slots** — game-specific panels (prison, town, family, school) register into the
   shell from the game repo.

Tech choice: **egui/eframe** (native + wasm from one codebase) with `egui_dock` panels; keep
toolkit deps in a `sumo-gui` crate, not the engine core. A Bevy/2D-3D adapter is a later option,
only if rendering becomes a requirement; it must not break determinism.

## 4. Support/forward-compatibility contract for sumo

- **Generic seams first:** `WorldModel`/`Engine<W>` trait, generic event/action types, plugin
  registry, typed component serialization. Game rules live in Alcagent.
- **Versioned contracts:** semver for the engine API, versioned save format with migration
  chain (not "regenerate saves"), documented compatibility window.
- **Determinism contract:** same seed + same recorded input stream + same build ⇒ same digest;
  replay logs include UI commands so GUI actions are reproducible.
- **Observability:** `tracing` spans per subsystem, structured violations carrying replay
  tickets; GUI debug panels consume these.
- **Test matrix:** engine unit/integration + soak + replay golden + a *second* trivial project
  that proves sumo is game-agnostic; GUI smoke/golden-image tests.
- **Release cadence:** sumo minor releases for capabilities/backward-compatible additions;
  major for ABI/API breaks with migration guide; Alcagent can release gameplay updates between
  sumo minors.

## 5. Consequences for the v0.5 world plan

- Town/prison gameplay is implemented in **Alcagent**, using engine hooks; it must not force
  sumo to know what a school or prisoner is.
- New world state (citizens, families, jobs, map nodes) must be serializable through generic
  registration so saves survive engine upgrades.
- The GUI rewrite happens **in sumo's repo**, with game panels contributed by Alcagent — so
  the GUI grows with future projects rather than being rewritten per game.
