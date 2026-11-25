# Spec 003 — Persistence durability

Status: pending

## Requirements
Save/load must be durable, version-aware, and observable.

## Acceptance criteria
- [ ] Loaded saves restore simulation speed from `World.speed`.
- [ ] Saves fsync the temp file before rename.
- [ ] Autosave failures are surfaced through `Sim::last_autosave_error` and logged; no silent failures.
- [ ] v3 bincode saves migrate to v4 with a fixture test.
- [ ] State digest covers decision audit, contraband resolution, construction queue, rehab records, and post-release records.
- [ ] `cargo test -p sumo --test alcagent_005_regressions` and `cargo test -p alc-persistence` pass.
- [ ] `cargo test --workspace` passes.

Completion signal: `<promise>DONE</promise>`
