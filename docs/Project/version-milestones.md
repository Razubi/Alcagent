# Version Milestones — Project Alcagent

**Ownership:** docs only; kept in sync with project state (`docs/Project/PROJECT.md`).

## Current version

- **Version:** 0.05 (active development, Wave 4 engine integration complete).
- **State (verified 2026-11-23):** `cargo build --workspace` / `cargo test --workspace`
  are green: 184 passed, 0 failed, 1 ignored, including the 730-day 3-seed chaos
  soak, save/load determinism, council/shadow modes and brain-inspector proposals.
  Pre-Wave-4 baseline was 136/136 tests passing.
- **Active work:** Wave 4 — intentions, council of drives, memory-driven choices, navigation
  graph, real-time pawn positions, structured conflicts, live map. Engine surface R1–R7 is
  largely implemented; Wave 5 items remain. See
  `docs/Communication/Note's/wave4-implementation-status.md` and
  `docs/Communication/Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md`.
- Note: root `Cargo.toml` workspace `version` is `0.1.0`; reconciliation with the 0.05 label
  is pending.

## Milestones

| Version | Status | Summary |
|---|---|---|
| **0.1 beta** | **Planned — next milestone** | Complete Wave 4 engine/world state additions and close the remaining Phase 4+ gaps; make the prison a fully self-contained, observable, content-complete living institution. |

## Version 0.1 beta — planned scope

Derived from `docs/Project/design-status.md` and the Wave 4 requests.

### Already resolved (not in 0.1 beta scope)

Schedule resolution and need-driven decisions in tick; escape/parole intake gating; emergent
gangs; personality-biased decisions; all 15 `WardenAction` variants; interactive TUI; economy
sustainability fixes.

### Remaining scope

1. Full `KnowledgeStore` (design §9) — claims/provenance/standing; warden currently uses an inbox of IDs.
2. Wave 4 engine support (R1–R6): **done in-house this pass** — map frame, decision proposal/dry-run, policy switch API, structured incidents with causality, lossless persistence, new invariants. Remaining Wave 5 gaps: real council/shadow *behavior* (currently state-only), real nav graph, UI map switch to `MapFrame`, driver-authority unification.
3. Contraband and rehab full world lifecycle wiring.
4. Post-release outcome tracking (design §15).
5. AI-generated narrative text (no provider client wired).
6. `World::summary()` / CLI polish; reconcile with `sumo::Observation`.
7. `alc-gui` decision — integrate as a buildable workspace member or explicitly defer/retire.
8. Doc drift cleanup and test-suite green restoration. **(Done 2026-11-24: 184/0/1 green.)**

### Acceptance criteria

- `cargo build --workspace` and `cargo test --workspace` green.
- Long-duration soak runs (≥365 days; 730-day chaos/determinism coverage) stable.
- `docs/Project/design-status.md`, `docs/Systems/*.md`, `PROJECT.md` re-audited.
- Save format version/migration handled for new state; `CHANGELOG.md` updated at release.
