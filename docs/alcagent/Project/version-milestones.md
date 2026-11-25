# Version Milestones — Project Alcagent

**Ownership:** docs only; kept in sync with project state (`../Project/PROJECT.md`).

## Current version

- **Version:** 0.05 (active development, Wave 4 engine integration complete).
- **State (verified 2026-11-23):** `cargo build --workspace` / `cargo test --workspace`
  are green: 184 passed, 0 failed, 1 ignored, including the 730-day 3-seed chaos
  soak, save/load determinism, council/shadow modes and brain-inspector proposals.
  Pre-Wave-4 baseline was 136/136 tests passing.
- **Active work:** 0.05 completion is now blocked by the **major UI/GUI overhaul and real game map**.
  The TUI map and the buildable egui GUI both render a spatial map from `sumo::MapFrame`
  (accepted in 0.05, see `dev/ralph/specs/002-map-gui-acceptance.md`). Wave 4 engine surface
  R1–R7 is largely implemented; remaining Wave 5 items continue (real nav pathfinding, deeper
  map fidelity). See `../Communication/Note's/wave4-implementation-status.md` and
  `../Communication/Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md`.
- Note: root `Cargo.toml` workspace `version` is `0.1.0`; reconciliation with the 0.05 label
  is pending.

## Milestones

| Version | Status | Summary |
|---|---|---|
| **0.1 beta** | **Planned — next milestone** | Complete Wave 4 engine/world state additions, ship the major UI/GUI overhaul and real game map, and close the remaining Phase 4+ gaps; make the prison a fully self-contained, observable, content-complete living institution. |

## Version 0.1 beta — planned scope

Derived from `../Project/design-status.md` and the Wave 4 requests.

### Already resolved (not in 0.1 beta scope)

Schedule resolution and need-driven decisions in tick; escape/parole intake gating; emergent
gangs; personality-biased decisions; all 15 `WardenAction` variants; interactive TUI; economy
sustainability fixes.

### Remaining scope

1. **Major UI/GUI overhaul + real game map:** complete — `alc-gui` is a buildable workspace member sharing DTOs/controls with the TUI; both render `sumo::MapFrame` with facility occupancy, pawn positions, overlays, pan/zoom and hover detail. Only real nav pathfinding remains as Wave 5 depth.
2. Full `KnowledgeStore` (design §9) — claims/provenance/standing; warden currently uses an inbox of delivered report ids rather than omniscient state.
3. Wave 4 engine support (R1–R7): complete. Remaining Wave 5 gaps: real nav graph and driver-authority unification.
4. Contraband and rehab full world lifecycle wiring: complete (spec 005).
5. Post-release outcome tracking (design §15): complete (spec 005).
6. AI-generated narrative text (no provider client wired).
7. `World::summary()` / CLI polish; reconcile with `sumo::Observation`.
8. `alc-gui` integration — resolved by item 1: it must be buildable and in scope for 0.05, not deferred or retired.
9. Doc drift cleanup and test-suite green restoration. **(Done 2026-11-24: 184/0/1 green; re-audit required after the UI/GUI/map requirement is implemented.)**

### Acceptance criteria

- `cargo build --workspace` and `cargo test --workspace` green, including the buildable `alc-gui` workspace member.
- Long-duration soak runs (≥365 days; 730-day chaos/determinism coverage) stable.
- Real game map accepted in TUI and GUI: spatial cells/wings, pawn positions, movement/path hints, incident overlays, facility condition/capacity/utilization, pan/zoom and hover detail from `sumo::MapFrame`.
- UI audit accepted: no clipped critical panels/help/tabs at target terminal sizes; search, selection and notifications behave consistently across views.
- `../Project/design-status.md`, `../Systems/*.md`, `PROJECT.md` re-audited.
- Save format version/migration handled for new state; `CHANGELOG.md` updated at release.
