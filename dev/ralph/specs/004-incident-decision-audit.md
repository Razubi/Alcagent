# Spec 004 — Incident and decision audit depth

Status: pending

## Requirements
Major incidents and decisions must be traceable, bounded, and deduplicated.

## Acceptance criteria
- [ ] Incident causal chains never retain dangling incident references after log trimming.
- [ ] Decision audit log is bounded inside `record_decision_audit`.
- [ ] Public event emission mirrors conflict kinds into incident stores with dedupe by world event id.
- [ ] TUI decision drill-down uses a unique key and follows the cursor.
- [ ] Incident/decision detail panels show cause, actors, validation, and outcome in both frontends.
- [ ] Targeted regression tests pass and full workspace is green.

Completion signal: `<promise>DONE</promise>`
