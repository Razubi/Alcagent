# Version Milestones — Project Alcagent

**Ownership:** docs only; kept in sync with project state (`../Project/PROJECT.md`).

## Current version

- **Version:** 0.05 / workspace `0.5.0-beta.1` (release candidate).
- **State at the 0.05 release (verified 2026-11-25):** `cargo fmt --all -- --check`,
  `cargo build --workspace --all-targets` and `cargo test --workspace` are green, including the
  365-day and 730-day 3-seed chaos soaks, save/load determinism, council/shadow modes,
  brain-inspector proposals, gang politics and AI orchestration suites. All 14 Ralph specs are
  `done`.
- **Status:** the major UI/GUI overhaul and real game map are implemented and accepted in
  0.05 (see `dev/ralph/specs/002-map-gui-acceptance.md`); both frontends render the shared
  `sumo::MapFrame`. The historical Wave 4 engine surface R1–R7 is complete. Real nav pathfinding
  and deeper map fidelity remain future depth, outside the 0.05 exit criteria.
- Root `Cargo.toml` workspace `version` is reconciled to `0.5.0-beta.1`.

## Milestones

| Version | Status | Summary |
|---|---|---|
| **0.1 beta** | **Planned — future milestone** | Add real navigation/pathfinding depth, full `KnowledgeStore` claims/provenance/standing, provider-backed narrative adapters and CLI polish. The 0.05 exit criteria are closed. |

## Version 0.1 beta — planned scope

Derived from `../Project/design-status.md` and the historical Wave 4 requests.

### Already resolved (not in 0.1 beta scope)

Schedule resolution and need-driven decisions in tick; escape/parole intake gating; emergent
gangs; personality-biased decisions; all 14 `WardenAction` variants; interactive TUI; economy
sustainability fixes.

### Remaining scope

1. **Major UI/GUI overhaul + real game map:** complete — `alc-gui` is a buildable workspace member sharing DTOs/controls with the TUI; both render `sumo::MapFrame` with facility occupancy, pawn positions, overlays, pan/zoom and hover detail. Only real nav pathfinding remains as future depth.
2. Full `KnowledgeStore` (design §9) — claims/provenance/standing; warden currently uses an inbox of delivered report ids rather than omniscient state.
3. Historical Wave 4 engine support (R1–R7): complete. Remaining future gaps: real nav graph and full driver-authority unification.
4. Contraband and rehab full world lifecycle wiring: complete (spec 005).
5. Post-release outcome tracking (design §15): complete (spec 005).
6. AI-generated narrative text (no provider client wired).
7. `World::summary()` / CLI polish; reconcile with `sumo::Observation`.
8. `alc-gui` integration — resolved by item 1: it must be buildable and in scope for 0.05, not deferred or retired.
9. Doc drift cleanup and test-suite green restoration. **(Done for the 0.05 release 2026-11-25; the full workspace suite was green and all specs and active docs were re-audited.)**

### Acceptance criteria

- `cargo build --workspace` and `cargo test --workspace` green, including the buildable `alc-gui` workspace member.
- Long-duration soak runs (≥365 days; 730-day chaos/determinism coverage) stable.
- Real game map accepted in TUI and GUI: spatial cells/wings, pawn positions, movement/path hints, incident overlays, facility condition/capacity/utilization, pan/zoom and hover detail from `sumo::MapFrame`.
- UI audit accepted: no clipped critical panels/help/tabs at target terminal sizes; search, selection and notifications behave consistently across views.
- `../Project/design-status.md`, `../Systems/*.md`, `PROJECT.md` re-audited.
- Save format version/migration handled for new state; `CHANGELOG.md` updated at release.
