# Spec 003 â€” Persistence durability

Status: done

## Requirements
Save/load must be durable, version-aware, and observable.

## Acceptance criteria
- [x] Loaded saves restore simulation speed from `World.speed`.
- [x] Saves fsync the temp file before rename.
- [x] Autosave failures are surfaced through `Sim::last_autosave_error` and logged; no silent failures.
- [x] v3 bincode saves migrate to v4 with a fixture test.
- [x] State digest covers decision audit, contraband resolution, construction queue, rehab records, and post-release records.
- [x] `cargo test -p sumo --test alcagent_005_regressions` and `cargo test -p alc-persistence` pass.
- [x] `cargo test --workspace` passes.

Completion signal: `<promise>DONE</promise>`

