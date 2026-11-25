# Spec 012 — Social-network analysis

Status: claimed

## Requirements
Emergent social structures must be detectable from relationships and interactions.

## Acceptance criteria
- [ ] Detect friends, rivals, enemies, allies, informants, brokers, group leaders, isolated prisoners, and vulnerable prisoners.
- [ ] Detection is derived from interaction history and relationship strengths, not hardcoded labels.
- [ ] Results are exposed in the Social Network view in both frontends.
- [ ] Metrics are bounded and deterministic for a fixed seed.
- [ ] Add tests covering network classification changes after interactions; full workspace passes.

Completion signal: `<promise>DONE</promise>`
