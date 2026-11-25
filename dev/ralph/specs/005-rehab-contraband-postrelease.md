# Spec 005 â€” Rehab / contraband / post-release lifecycles

Status: done

## Requirements
These systems must have end-to-end world state transitions, not isolated primitives.

## Acceptance criteria
- [x] Rehab enrollment, progress, completion/failure, and goal effects are persisted and visible.
- [x] Contraband discovery, resolution timestamp, and consequences are persisted and visible.
- [x] Successful escapes and releases create post-release records/outcomes and purge gang state consistently.
- [x] Both frontends expose validated controls and detail views for these lifecycles.
- [x] `cargo test -p sumo --test alcagent_005_smoke` and full workspace pass.

Completion signal: `<promise>DONE</promise>`

