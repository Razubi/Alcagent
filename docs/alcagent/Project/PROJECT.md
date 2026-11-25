# Project Alcagent — Autonomous Living Prison Simulation

Master Directive lives in `AGENTS.md` (mirror of the original `agents.md` directive).

**Canonical docs:** `../Project/` (status, milestones, design-status), `../Systems/` (interfaces), `../Design/` (intent/plans), `../Reports/` (findings), `../Communication/` (AI-to-AI handoffs).
**Release/change notes:** `CHANGELOG.md` at repo root.

## Status (version 0.05 — active development / Wave 4)

The simulation is playable and the workspace **builds**. The current work wave (Wave 4) added
intentions, a council of drives, memory-driven choices, a navigation graph, real-time pawn
positions, structured conflicts and an engine-level map surface (see `../Communication/Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md`).

**0.05 completion blocker:** the major UI/GUI overhaul and a real game map are now required. The
TUI map is still a schematic facility table and `alc-gui` is not a buildable workspace member. 0.05
is not complete until the GUI observer is buildable and both frontends render a spatial map with
cell/wing occupancy, prisoner/guard pawn positions, movement/path hints and incident overlays from
`sumo::MapFrame`.

**Verified 2026-11-23:**
- `cargo build --workspace` / `cargo test --workspace` — **green**. 184 tests passed,
  0 failed, 1 ignored (the 10k-entry `logs_over_limit_are_detected` stress test).
  Includes the 730-day 3-seed chaos soak, save/load determinism, and all Wave 4
  suites (`engine_state_roundtrip`, `invariants_negative`, `log_wiring`,
  `map_frame`, `integration_wave4`).
- Only benign unused-dependency warnings remain (feature-gated egui/eframe and
  workspace-level entries); no source-code warnings.
- Earlier baseline (before Wave 4) was 136/136 tests passing.

## Quick start

```powershell
cargo build --workspace
cargo test --workspace        # 184 passed, 0 failed, 1 ignored
cargo run -- run 365          # 1-year headless evolution
cargo run -- observe          # interactive ratatui observer (default)
cargo run -- --help           # CLI + TUI keybindings
```

## Current milestone

- **Current version:** 0.05
- **0.05 exit blocker:** major UI/GUI overhaul + real game map implementation.
- **Next milestone:** **0.1 beta** — scope in `../Project/version-milestones.md`.

## Where to look

| Topic | Document |
|---|---|
| Current status/design cross-reference | `../Project/design-status.md` |
| Version history and milestones | `../Project/version-milestones.md` |
| Engine API | `../Systems/engine-api.md` |
| Guard/warden/parole/rehab contracts | `../Systems/` |
| Design intent | `../Design/` |
| Wave 4 engine support requests | `../Communication/Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md` |
| Cross-team Wave 4 note | `../Communication/Note's/wave4-support-needed.md` |
| Wave 4 implementation status (driver/integration pass) | `../Communication/Note's/wave4-implementation-status.md` |
| Audit and soak reports | `../Reports/` |

## Wave 4 driver / integration pass (added by Integration & Docs engineer)

`sumo/src/drivers.rs` hardening:
- `apply_warden_action` now emits events for previously silent actions
  (`PromoteStaff`, `ChangePolicy`, `IncreaseSecurity`, `ImproveInfrastructure`);
  `RespondToDisturbance` uses its `event_id` in the summary.
- Guard hire/fire maintain `guard_location` entries so the pawn-location
  invariant stays clean across warden staffing actions.
- `ChangePolicy { policy: "lockdown" }` cannot sync `engine_state.policy_state`
  with only `&mut World`; callers must use `Sim::set_lockdown(true)`. A
  `&mut Sim` applier signature is recommended for Wave 5.
- Module note: `World::tick` also runs guard/warden cycles; `Sim::tick`
  deliberately does not call these drivers. Unify authority in Wave 5.

New public APIs to build against (Wave 4): `Sim::map_frame()` (R1 read-only
pawn map snapshot), `Sim::trace_hash_v2()` (full-state fingerprint; legacy
`trace_hash()` retained), and the incident/decision log accessors
(`incident_log`, `incidents_since`, `decision_log`) wired from the decision
pipeline. `PolicyMode::Council`/`Shadow` now have real behavior (deterministic drive vote;
shadow applies the baseline only and logs divergence). See `sumo::council` and the
Wave 4 status note.

`crates/sumo/tests/integration_wave4.rs` adds four end-to-end tests: full
30-day flow (invariants/map/save-load/trace), warden action event emission,
guard report → warden inbox, and shadow-mode safety. The two world-level bugs
these tests exposed (stale social dyads after release; world-tick policy drift)
were fixed during integration — see the implementation status note linked above.
