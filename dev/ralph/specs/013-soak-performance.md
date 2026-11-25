# Spec 013 — Long-run soak and performance

Status: pending

## Requirements
The living institution must remain stable and fast over long periods.

## Acceptance criteria
- [ ] Run at least 365 simulated days headlessly with no invariant violations.
- [ ] Save/load after the run continues deterministically for a fixed seed.
- [ ] Logs, memories, decision audit, incidents, and notifications remain bounded.
- [ ] No economic collapse, population instability, or unbounded memory growth.
- [ ] Record observed ticks/sec and event count in dev/ralph/history.txt.
- [ ] Full workspace tests pass.

Completion signal: `<promise>DONE</promise>`
