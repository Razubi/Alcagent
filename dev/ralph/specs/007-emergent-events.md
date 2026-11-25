# Spec 007 — Emergent event system

Status: done

## Requirements
The institution should generate explainable events from world conditions, not only scripted timing.

## Acceptance criteria
- [x] Add condition-driven generators for at least four of: rumors, family correspondence, staff misconduct, equipment failure, food shortage, medical emergency, protest, contraband discovery.
- [x] Each generated event records causal inputs and is visible in incident/event views.
- [x] Event generation is deterministic for a fixed seed and bounded per sim day.
- [x] Add focused tests proving the same conditions produce the same events and that events alter later state.
- [x] Full workspace tests pass.

Completion signal: `<promise>DONE</promise>`
