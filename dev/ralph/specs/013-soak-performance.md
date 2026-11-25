# Spec 013 — Long-run soak and performance

Status: done

## Requirements
The living institution must remain stable and fast over long periods.

## Acceptance criteria
- [x] Run at least 365 simulated days headlessly with no invariant violations.
- [x] Save/load after the run continues deterministically for a fixed seed.
- [x] Logs, memories, decision audit, incidents, and notifications remain bounded.
- [x] No economic collapse, population instability, or unbounded memory growth.
- [x] Record observed ticks/sec and event count in dev/ralph/history.txt.
- [x] Full workspace tests pass.

Validation (2026-11-25): 17,520 ticks / 365 days at ~632 ticks/sec with 27,041 retained
events, population 15/80 and cash 42,826,600 cents for seed `0xA1C_0A_6E_17_0` (debug profile,
shared host). 730-day three-seed soak and save/load continuation are green; final full workspace
gate is green. Metrics are recorded in `dev/ralph/history.txt`.

Completion signal: `<promise>DONE</promise>`
