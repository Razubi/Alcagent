# Spec 011 — AI orchestration and decision batching

Status: pending

## Requirements
Hybrid deterministic/utility/AI decision architecture with strict validation.

## Acceptance criteria
- [ ] Decision proposals are batched, validated through one canonical context, and applied only when accepted.
- [ ] Token budget, rate limiting, retry, caching, and model selection are configurable and observable.
- [ ] Agent outputs are structured decisions that cannot mutate arbitrary state.
- [ ] Context construction retrieves relevant memories and separates direct knowledge, rumor, suspicion, and belief.
- [ ] Add tests for validation parity, batching, bounds, and retry behavior; full workspace passes.

Completion signal: `<promise>DONE</promise>`
