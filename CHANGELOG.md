# Changelog — Project Alcagent

## [0.05] — "The Living Institution" (in development; status 2026-11-25)

### Build / tests
- Verified green baseline after spec 009 (2026-11-25): `cargo fmt --all -- --check`,
  `cargo build --workspace --all-targets` and `cargo test --workspace` all pass, including the
  365-day chaos soak, review-agent suites, save/load determinism and both frontend suites.
- The Ralph plan tracks 14 specs in `dev/ralph/specs`; 001–007, 009 and 012 are `done`.
  Specs 008, 010, 011, 013 and 014 remain in progress/pending for the 0.05 release.

### Repository split
- The `sumo` engine and its 11 `alc-*` dependencies were extracted into a standalone repository
  at `../sumo` for independent publishing. This repository remains the full application
  (`alcagent` CLI + `alc-ui` TUI + engine).

### Implemented (pre-Wave-4)
- Capability catalog + trait deltas (`alc-core::capability`, 23 kinds).
- Contraband market + discovery library (`alc-social::contraband`).
- Rehabilitation program state machine (`alc-social::rehab`).
- Daily schedule resolution + prisoner placement (`alc-world::resolve_schedules_step`, `prisoner_location`).
- Simplified rehab progress API (`sumo::Sim::apply_rehab_progress`, `rehab_score`).
- `alc-gui` pure data-extraction layer (`data.rs`) — crate not a workspace member.

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

### Remaining 0.05 work
- Spec 008: gang lifecycle and internal politics; 010: report-driven warden strategy;
  011: AI orchestration/batching; 013: long-run soak/performance acceptance; 014: release docs.
- Deferred beyond 0.05: real navigation graph beyond the synthesized grid; full `KnowledgeStore`
  claims/provenance/standing; council/shadow mode behavior beyond state.

### Housekeeping (2026-11-23)
- Removed obsolete root-level duplicate summaries (`IMPLEMENTATION_SUMMARY*.md`, `S0_STATUS.md`),
  stale `design/` session artifacts, random `.log` files and `build_output.txt`.
- Added `*.log` and `build_output.txt` to `.gitignore`.
