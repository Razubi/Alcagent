# Project Alcagent — Autonomous Living Prison Simulation

Master Directive lives in `AGENTS.md` (mirror of the original `agents.md` directive).

**Canonical docs:** `../Project/` (status, milestones, design-status), `../Systems/` (interfaces), `../Design/` (intent/plans), `../Reports/` (findings), `../Communication/` (AI-to-AI handoffs).
**Release/change notes:** `CHANGELOG.md` at repo root.

## Status (version 0.05 — release candidate, all specs complete)

The simulation is playable and the workspace **builds**. The 0.05 release added intentions, a
council of drives, memory-driven choices, a navigation-graph-shaped map surface with real-time
pawn positions, structured conflicts, gang politics, AI orchestration/batching, long-run soak
acceptance and release documentation; all 14 Ralph specs in `dev/ralph/specs` are `done`.

**0.05 status (R0):** the major UI/GUI overhaul and real game map are implemented — the TUI and
the egui GUI (`alc-gui`, a buildable workspace member) both render a spatial map with cell/wing
occupancy, prisoner/guard pawn positions, movement/path hints and incident overlays from
`sumo::MapFrame`. See `dev/ralph/specs/002-map-gui-acceptance.md`.

**Verified at the 0.05 release (2026-11-25):**
- `cargo fmt --all -- --check`, `cargo build --workspace --all-targets` and
  `cargo test --workspace` — **green**, with no build warnings. The full suite includes the
  10k-entry `logs_over_limit_are_detected` stress test, the 365-day and 730-day 3-seed chaos
  soaks, save/load determinism, engine integration suites (`engine_state_roundtrip`,
  `invariants_negative`, `log_wiring`, `map_frame`, `integration_wave4`), gang politics and AI
  orchestration suites.
- Earlier baselines: 184 passed after Wave 4, 136 passed before Wave 4.

## Quick start

```powershell
cargo build --workspace
cargo test --workspace        # full workspace, including long-run soaks
cargo run -- run 365          # 1-year headless evolution
cargo run -- observe          # interactive ratatui observer (default)
cargo run -- --help           # CLI + TUI keybindings
```

## Current milestone

- **Current version:** 0.05 (`0.5.0-beta.1`, reconciled in the workspace manifest).
- **0.05 exit criteria:** all Ralph specs done, green workspace, 365+ day soak; both TUI and GUI
  already render the shared `sumo::MapFrame`.
- **Next milestone:** **0.1 beta** — scope in `../Project/version-milestones.md`.

## Where to look

| Topic | Document |
|---|---|
| Current status/design cross-reference | `../Project/design-status.md` |
| Version history and milestones | `../Project/version-milestones.md` |
| Engine API | `../../sumo/SUMO_API.md` |
| Guard/warden/parole/rehab contracts | `../Systems/` |
| Design intent | `../Design/` |
| Historical Wave 4 engine support requests | `../Communication/Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md` |
| Historical cross-team Wave 4 note | `../Communication/Note's/wave4-support-needed.md` |
| Historical Wave 4 implementation status (driver/integration pass) | `../Communication/Note's/wave4-implementation-status.md` |
| Audit and soak reports | `../Reports/` |

## Wave 4 driver / integration pass (historical; retained for context)

> This section records the earlier Wave 4 handoff. R1–R7 and council/shadow behavior landed, and
> later 0.05 specs reconciled policy/action application; do not read the Wave 5 suggestions below
> as current open work.

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
