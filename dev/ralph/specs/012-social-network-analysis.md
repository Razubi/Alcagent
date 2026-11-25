# Spec 012 — Social-network analysis

Status: done

## Requirements
Emergent social structures must be detectable from relationships and interactions.

## Acceptance criteria
- [x] Detect friends, rivals, enemies, allies, informants, brokers, group leaders, isolated prisoners, and vulnerable prisoners.
- [x] Detection is derived from interaction history and relationship strengths, not hardcoded labels.
- [x] Results are exposed in the Social Network view in both frontends.
- [x] Metrics are bounded and deterministic for a fixed seed.
- [x] Add tests covering network classification changes after interactions; full workspace passes.

Completion signal: `<promise>DONE</promise>`
