# Changelog — Project Alcagent

## [Unreleased]

### Frontends / launch
- Consolidated to one `alcagent` binary (workspace `default-members`): `cargo run` opens the egui/eframe
  graphical observer at a Main Menu (seed field + Start, Load Existing Save, Settings, Quit); `--tui` opens the
  ratatui debug observer; `run <days>` remains headless. The GUI is button/menu-only with no hidden
  keyboard shortcuts; TUI keybinds remain available in `--tui` mode and its `?` help overlay.
- Shared display/graphics models (palette, view catalog, world-map layout/zones, camera math,
  construction-kind catalog) now live in `sumo::graphics`; `alc-gui` and `alc-ui` are rendering/input
  adapters over `sumo::Sim` and never mutate arbitrary world state.
- The map is a layered campus-style world map with zones, roads, facilities and pawns, with explicit
  on-screen camera controls. The save browser uses `alc_persistence::list_saves`/`probe_save_dir` to
  show version, status, population and simulation time.
- Saves remain v5 directories with `world.bin`; GUI settings are separate JSON at
  `saves/alc-gui/settings.json` and do not alter world saves. `--load`, `--seed`, `--speed`,
  `--save-dir` and `--autosave-days` continue to work; `--no-tui` is a legacy headless-report alias.
- GUI preferences are authoritative on restart: omitted startup flags inherit saved settings while
  explicit flags win. `--tui`/`--no-tui` are observer-only and rejected by `run`/`docs`.
  `sumo::Sim::world_mut` is gated behind the `test-support` feature so frontends cannot bypass
  validated controls.
- The save browser now labels v2 binary saves as **Incompatible** instead of **Migratable**; the
  loader rejects them with an actionable reason, matching the supported v1/v3/v4 migration set.
- Manual saves no longer replace the persisted save root: generated world directories are tracked
  separately, so the save browser keeps listing worlds after a restart.

## [0.5.0-beta.1] — "The Living Institution" (0.05 release candidate; status 2026-11-25)

### Build / tests
- Final 2026-11-25 gates are green: `cargo fmt --all -- --check`,
  `cargo build --workspace --all-targets` (no warnings) and `cargo test --workspace` — the full
  workspace test suite is green at 0.05, including the 365-day and 730-day chaos soaks,
  review-agent suites, save/load determinism and both frontend suites.
- All 14 Ralph specs in `dev/ralph/specs` are `done`.
- Workspace manifest version reconciled from `0.1.0` to `0.5.0-beta.1`; the workspace has 15
  members, including the buildable `alc-gui` observer.

### Repository split
- Historical 0.05 note: an out-of-tree `../sumo` snapshot was used for independent publishing.
  This repository remains the full application (`alcagent` CLI + `alc-ui` TUI + `alc-gui` observer
  + engine), and its `docs/` are canonical rather than shared/synced with that snapshot.

### Implemented (pre-Wave-4)
- Capability catalog + trait deltas (`alc-core::capability`, 23 kinds).
- Contraband market + discovery library (`alc-social::contraband`).
- Rehabilitation program state machine (`alc-social::rehab`).
- Daily schedule resolution + prisoner placement (`alc-world::resolve_schedules_step`, `prisoner_location`).
- Simplified rehab progress API (`sumo::Sim::apply_rehab_progress`, `rehab_score`).
- `alc-gui` pure data-extraction layer (`data.rs`) — pre-Wave-5 state, superseded below by its
  buildable workspace-member GUI.

### Implemented (Wave 5 — R0 map + GUI acceptance, spec 002)
- Both frontends render a real spatial map from `sumo::MapFrame`: the egui GUI (`alc-gui`, a
  buildable workspace member) with pan/zoom, hover/click inspectors, cell/wing rosters,
  capacity/utilisation bars, legend and fit/reset; the ratatui TUI (`alc-ui`) with facility
  glyphs, prisoner/guard pawns, nav edges, incident overlays, locked nodes and selected
  pawn/facility detail. 11 observation views in both frontends; UI changes route through
  validated `Sim`/`SimDriver` controls only.

### Implemented (Wave 4 engine integration)
- `Sim::map_frame()` / `MapFrame` (R1), `Sim::propose_decision` / `DecisionProposal` (R2),
  `Policy`/`PolicyMode` switching (R3), structured incident logs + causality (R4),
  versioned persistence round-trip + `trace_hash_v2()` (R5), extended invariants +
  `state_digest` (R6), driver event/log wiring (R7).
- Warden actions now emit events for previously silent variants; guard hire/fire maintains
  `guard_location` so pawn-location invariants stay clean.
- New integration suites: `engine_state_roundtrip`, `invariants_negative`, `log_wiring`,
  `map_frame`, `integration_wave4`. Fixed world-level bugs they exposed (stale social
  dyads after release; world-tick policy drift).

### Implemented (Ralph 0.05 wave: specs 001–007, 009, 012)
- Persistence durability: atomic versioned saves (format v5), v1/v3/v4 migration, full-state digest
  coverage, autosave interval persistence and surfaced autosave errors (spec 003).
- Incident and decision audit: bounded causal chains, authoritative event ids, stable content keys,
  and drill-down in both frontends (spec 004).
- Rehab, contraband and post-release lifecycles are wired into world ticks and validated controls;
  resolved cases and community outcomes are persisted and surfaced in both UIs (spec 005).
- Policy, construction and parole controls are clamped/audited, with an explicit construction
  `Approved` phase and deterministic persisted parole conditions (spec 006).
- Condition-driven emergent events feed world effects and incident views (spec 007).
- Guard autonomy: area-scoped perception, fallible reports with credibility filtering, bounded
  opinions/mistakes/violations, assistant requests, and measurable effects on prisoner needs and
  local incident pressure (spec 009).
- Social-network analysis: authoritative social roles and derived dyad labels exposed in TUI/GUI
  (spec 012).
- Warden strategy: report-only objective scoring with explicit conflicts, finance/attention gates,
  catalog-driven expansion planning, funding/hiring consequences, and rationale persisted in both
  audit stores and exposed in TUI/GUI (spec 010). The engine construction queue itself still uses
  hard-coded duration/cost rules by facility type and capacity.

### Implemented (final 0.05 wave: specs 008, 011, 013, 014)
- Gang politics: authoritative membership reconciliation, event-sourced gang history, disputes,
  leadership challenges, splits, mergers, alliances/rivalries, bounded security pressure and
  gang-controlled contraband; both frontends show relation scores, pressure and history (spec 008).
- AI orchestration: configurable provider/model, budgets, rate limits, retries, cache and batch
  limits; canonical epistemic contexts and one validation gate for single/batched decisions;
  repaired decisions revalidated before apply; runtime telemetry exposed in both frontends (spec 011).
- Long-run soak: 365-day instrumented run at ~632 ticks/sec with 27,041 events and bounded logs,
  plus 730-day three-seed and save/load continuation coverage (spec 013).
- Release docs: version, API, status, milestone, gameplay and changelog reconciled to the
  actual 0.05 implementation; v5 save format unchanged (spec 014).
- Fixed integration regressions found by the final gates: escape outcome auditing, guarded-fight
  repair target, medical-request need relief, and legacy audit-label assertions.

### Deferred beyond 0.05
- Real navigation graph beyond the synthesized grid; full `KnowledgeStore` claims/provenance/standing;
  council/shadow mode behavior beyond state; network-backed AI providers.

### Housekeeping (2026-11-23)
- Removed obsolete root-level duplicate summaries (`IMPLEMENTATION_SUMMARY*.md`, `S0_STATUS.md`),
  stale `design/` session artifacts, random `.log` files and `build_output.txt`.
- Added `*.log` and `build_output.txt` to `.gitignore`.
