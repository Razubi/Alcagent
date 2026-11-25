# Spec 006 â€” Policy / construction / parole controls

Status: done

## Requirements
Institutional control surfaces must be validated, auditable, and consistent across frontends.

## Acceptance criteria
- [x] Policy bands, lockdown, privileges, rehab funding, and security level changes are validated and recorded.
- [x] Construction projects have request -> approved -> active -> complete/cancel/fail transitions with events and capacity effects.
- [x] Parole eligibility, hearing, decision, conditions, and release are deterministic and persisted.
- [x] TUI and GUI expose the same controls through validated APIs; no raw world mutation.
- [x] Full workspace tests pass.

Completion signal: `<promise>DONE</promise>`

