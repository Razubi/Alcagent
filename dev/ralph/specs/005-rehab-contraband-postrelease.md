# Spec 005 — Rehab / contraband / post-release lifecycles

Status: pending

## Requirements
These systems must have end-to-end world state transitions, not isolated primitives.

## Acceptance criteria
- [ ] Rehab enrollment, progress, completion/failure, and goal effects are persisted and visible.
- [ ] Contraband discovery, resolution timestamp, and consequences are persisted and visible.
- [ ] Successful escapes and releases create post-release records/outcomes and purge gang state consistently.
- [ ] Both frontends expose validated controls and detail views for these lifecycles.
- [ ] `cargo test -p sumo --test alcagent_005_smoke` and full workspace pass.

Completion signal: `<promise>DONE</promise>`
