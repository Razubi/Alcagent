# Changelog — Project Alcagent

## [0.05] — "The Living Institution" (in development; status 2026-11-24)

### Build / tests
- Last verified green baseline (2026-11-23): `cargo build --workspace` / `cargo test --workspace`
  184 passed, 0 failed, 1 ignored (the 10k-entry `logs_over_limit_are_detected` stress test),
  including the 730-day 3-seed chaos soak, save/load determinism and all Wave 4 suites.
- Pre-Wave-4 baseline was 136/136 passing.
- **2026-11-24:** an in-flight gameplay wave is mid-edit in the working tree (policy controls,
  construction queue, rehab/contraband steps, save format v3). It may not compile until the
  edits land; see `docs/SUMO_API.md` §12 and `docs/ALCAGENT.md` for the new APIs/controls.

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

### Present but not fully wired (deferred)
- `alc-gui` is not a workspace member and is not buildable as-is (missing rendering modules / theme integration).
- Contraband and rehab library modules exist, but full world-tick/program lifecycle wiring is incomplete.
- Post-release outcome tracking partial; persistent community outcomes not fully wired.
- Some world-level APIs remain unimplemented (`World::summary`, world program/contraband/post-release lifecycle wiring).

### Open work (Wave 5 / 0.1 beta)
- Real navigation graph (currently a grid synthesized from facility positions); UI map switch to consume `MapFrame`.
- Council/shadow modes are behaviorally inert (state only); driver authority unification (`World::tick` vs `sumo` drivers); `&mut Sim` applier signature.
- Full `KnowledgeStore` (claims/provenance/standing).
- `docs/Communication/Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md` and
  `docs/Communication/Note's/wave4-implementation-status.md` track the details.

### Housekeeping (2026-11-23)
- Removed obsolete root-level duplicate summaries (`IMPLEMENTATION_SUMMARY*.md`, `S0_STATUS.md`),
  stale `design/` session artifacts, random `.log` files and `build_output.txt`.
- Added `*.log` and `build_output.txt` to `.gitignore`.
