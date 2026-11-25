# sumo Engine Roadmap — 0.05 completion → 0.06+ addons & mass bug-fixing

**Status:** planning document · **Date:** 2026-11-24 · **No code changed.**
**Scope:** the `sumo` engine (the `sumo` subfolder of this repository). Gameplay content
(prison/town) belongs to Alcagent; this roadmap covers engine capabilities, stability and tooling.

## 0. Assumptions and principles

1. **0.05 gameplay needs land first** in the game part of this repo. sumo's 0.05 role is to remain
   the stable engine those sit on.
2. **Bug-fixing is continuous and front-loaded**, not a phase at the end. Every release has a
   hardening gate that must pass before features ship.
3. **The engine grows infrastructure** (abstractions, GUI shell, replay, tooling); the game grows
   content. Dependency direction is always `game → engine`.
4. **Determinism is non-negotiable** — changes that alter replay digests need a versioned contract
   and golden-replay update, never a silent break.
5. **Small, reversible steps.** Each phase ends with green build/tests, longer soaks, and a
   docs/CHANGELOG update. No big-bang rewrites.

Effort: S (≤1 day), M (2–5 days), L (1–3 weeks), XL (multi-week). One focused developer.

---

## 1. Baseline

Verified green build. Deterministic `Sim` façade over a concrete, prison-shaped `alc_world::World`:
time control, validated decisions, observation snapshots, bounded event bus, versioned persistence
(v3 in flight), invariants, `trace_hash`/`trace_hash_v2`, 730-day multi-seed soaks. Both frontends
render the spatial map: the egui GUI (`alc-gui`, buildable workspace member) with pan/zoom,
hover/click inspectors, rosters and capacity bars; the ratatui TUI (`alc-ui`) with facility glyphs,
pawns, nav edges, incident overlays and locked nodes (R0, spec `dev/ralph/specs/002-map-gui-acceptance.md`).
Generalization blockers: no `WorldModel`
seam, domain-typed API, duplicated tick/driver authority, self-referential incident causality,
autosave dir lost on load, policy-mirror drift, summary stubs, placeholder nav geometry, event-bus
eviction, partial non-finite guards, non-portable `DefaultHasher` digest.
Sources: `sumo-engine-research.md`, `SUMO_API.md`, `SUMO_ENGINE.md`.

---

## 2. Mass bug-fixing campaign (starts now, runs continuously)

Every bug gets: reproduction (seed + tick + last N inputs), a regression test, a fix, and a
CHANGELOG entry.

### 2.1 P0 correctness (before 0.6 feature work)
| # | Bug | Evidence | Effort | Gate |
|---|---|---|---|---|
| B1 | Double-driving: `World::tick` and `sumo::drivers` both run guard/warden cycles | `drivers.rs:16-27` | M | Single authority; explicit drivers additive only; no-double-apply test |
| B2 | Self-referential incident causality (`cause = consequence = own id`) | `lib.rs`, `decisions.rs` | M | Real cause chains from `WorldEvent.causes`; acyclicity invariant |
| B3 | Autosave dir lost on load; `maybe_autosave()` errors swallowed | `lib.rs` converters and `tick()` | S | Dir persisted/re-supplied; errors surfaced |
| B4 | Policy-mirror drift (engine 0–5 int vs world 0.0–1.0 float) | `invariants.rs`, `lib.rs` | M | One authoritative policy; remove tolerance hacks |
| B5 | Summary stubs (empty warden inbox, zeroed facility wing/staff, flattened gang intensities, dropped `involved_guards`, `MapFrame::capture` omits incidents) | `lib.rs`, `map_frame.rs` | S | All fields populated or removed; tests assert real values |
| B6 | Escape-path divergence (engine discards consequences; world removes prisoner) | `decisions.rs` | M | One escape authority; consistent state either path |
| B7 | Unbounded `wait_for_event`/`run_until`/`advance_by_secs` | `lib.rs` | S | Tick budget + clear error |
| B8 | Event-bus eviction (50k) silently drops history | `alc-events` | S–M | Configurable capacity + retention; observability of drops |

### 2.2 P1 robustness
| # | Bug | Effort | Fix |
|---|---|---|---|
| B9 | Non-finite values poison sorts/rankings | S | Engine-wide finite guards |
| B10 | `DefaultHasher` in digest is non-portable | S | Stable FNV-1a/xxHash + cross-platform test |
| B11 | Validation-context duplication across proposal APIs | S | One canonical `validation_context()` |
| B12 | `ChangePolicy "lockdown"` bypasses engine mirror | S | `&mut Sim` applier or route via `set_lockdown` |
| B13 | `Trade` economically effect-free / empty item in proposals | S | Real effect or documented no-op |
| B14 | `has_contraband` proxy (`!skills.is_empty()`) | M | Real state or explicit "unsupported" |
| B15 | One-boolean post-release ledger; three incompatible `PostReleaseRecord` types | M | Single rich record + migration |
| B16 | `release_event_info` string-parses summaries | S | Durable typed bridge |

### 2.3 Bug infrastructure
Replay tickets on violations (M); `#[tracing::instrument]` on tick/drivers/decisions/save/load +
`sumo-cli diag` (S); fuzz/property tests on saves and decisions (M); golden replay CI (M);
730-day ×3-seed soak matrix with memory/bus assertions (S–M).

**Bug policy:** not closed without a replayable repro, a failing-then-passing regression test, a
CHANGELOG entry, and a soak re-run. P0 blocks releases; P1 blocks minors; P2 backlog.

---

## 3. Version roadmap

### 0.5.x — Stabilize and instrument (~4–6 weeks)
All P0 (B1–B8) and most P1 (B9–B16); bug infrastructure shipped; **determinism contract v1**
(same seed + inputs + build ⇒ same digest); `sumo-cli diag`/`replay`; semver policy and tested
save-migration chain (no more "regenerate saves"). Gate: zero known P0 bugs, all soaks green.

### 0.6.0 — Generic core (~6–10 weeks)
`sumo-core` with `WorldModel`, generic `Sim<W>`, generic event/action/observation traits and
invariant registry; domain types behind an `alc` feature / moved to the game side.
`FixedStepDriver` (accumulator, max steps/frame, interpolation alpha — display only). Input/action
replay (`InputLog`, frame-tagged UI commands). Crate split (`sumo-core`, `sumo-cli`, compat `sumo`
façade keeping the current API). Gate: unchanged game runs via façade + a second trivial project
(e.g. minimal colony preset) builds and soaks.

### 0.6.x — Addons (independently shippable)
1. **sumo-gui (flagship)** — eframe/egui + `egui_dock`, playback bar, generic inspector, event
   timeline, map viewport with pan/zoom/pick/overlays, debug overlays, settings/project loader;
   game panels via plugin trait; no toolkit deps in core. (L; MSRV decision: egui 0.37 needs 1.95+)
2. **sumo-replay** — deterministic seek (keyframes 1/sim-day + re-simulation), event markers,
   baseline/council/shadow diff. (M–L)
3. **sumo-nav** — real spatial graph (typed nodes/edges, cost/capacity/restrictions), traversal,
   area queries; replaces MapFrame grid. (M)
4. **sumo-plugin** — plugin/system registry, typed component serialization, deterministic ordering. (M)
5. **sumo-events extension** — typed payloads with prison taxonomy as one plugin, real cause
   chains, configurable retention. (M)
6. **sumo-persist v4** — pluggable per-component providers, independent versions, schema hash,
   migration fixtures. (M)
7. **sumo-diag** — tracing UI, violation browser, digest/replay status, fuzz harness, soak dashboard. (M)
8. **Later:** Bevy adapter, assets/scenes, scripting (validated actions only), networking, undo/redo. (L each)

### 0.7+ — Scale and depth (after contract holds)
Parallel read-only analysis applied in sorted id order; stable cross-platform digests; benchmark
harness; thousands-of-agent stress; ECS-lite if profiling justifies; multiple project presets.
Gameplay rules never enter sumo.

---

## 4. Release gates
Green build/tests on supported platforms; 730-day ×3-seed soak; save→load→continue replay
equality; golden replay CI; digest stable; zero known P0 bugs; semver-labeled API changes;
migrations tested from every supported prior version; docs updated; second project preset for 0.6+.

## 5. Risks and mitigations
| Risk | Mitigation |
|---|---|
| Generic refactor breaks the game | Compat façade; unchanged game runs against it; staged changes |
| Determinism drift | Contract + golden CI; digests extended with every new state |
| GUI recouples engine to gameplay | Snapshot/command boundary; plugin panels; no toolkit deps in core |
| MSRV/egui conflict | Decide early: bump to 1.95+ or pin older egui |
| Bugfix competes with features | Standing bug window each sprint; P0 always blocks |
| Scope creep into gameplay | Engine backlog excludes needs/gangs/parole rules |
| Multiple code copies drift | This repo canonical; one source for API docs |

## 6. Owner decisions before 0.6
1. MSRV: bump to 1.95+ for egui 0.37, or pin older egui?
2. Keep current `sumo::Sim` API source-compatible via façade, or clean break with migration guide?
   (Recommendation: façade.)
3. GUI vs replay/nav priority for 0.6?
4. Generic events/IDs now or 0.7? (Recommendation: defer — high blast radius.)
5. Single-threaded deterministic core for 0.6, or partitioned determinism now? (Recommendation: single-threaded.)
6. Replay seek: arbitrary tick or bounded snapshot ring for 0.6?
7. Who decides P0/P1 severity?

## 7. Phase order
```
0.05 landed (game) → 0.5.x harden+bugfix+contract → 0.6.0 generic core+replay
  → 0.6.x addons (GUI, replay, nav, plugins, events, persist, diag)
  → 0.7+ scale/depth (parallel analysis, ECS-lite, examples)
  [bug-fixing campaign runs continuously across all phases]
```
