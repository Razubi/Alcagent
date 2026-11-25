# Spec 007 — Emergent event system

Status: pending

## Requirements
The institution should generate explainable events from world conditions, not only scripted timing.

## Acceptance criteria
- [ ] Add condition-driven generators for at least four of: rumors, family correspondence, staff misconduct, equipment failure, food shortage, medical emergency, protest, contraband discovery.
- [ ] Each generated event records causal inputs and is visible in incident/event views.
- [ ] Event generation is deterministic for a fixed seed and bounded per sim day.
- [ ] Add focused tests proving the same conditions produce the same events and that events alter later state.
- [ ] Full workspace tests pass.

Completion signal: `<promise>DONE</promise>`
