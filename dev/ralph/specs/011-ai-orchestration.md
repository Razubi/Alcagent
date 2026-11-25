# Spec 011 — AI orchestration and decision batching

Status: done

## Requirements
Hybrid deterministic/utility/AI decision architecture with strict validation.

## Acceptance criteria
- [x] Decision proposals are batched, validated through one canonical context, and applied only when accepted.
- [x] Token budget, rate limiting, retry, caching, and model selection are configurable and observable.
- [x] Agent outputs are structured decisions that cannot mutate arbitrary state.
- [x] Context construction retrieves relevant memories and separates direct knowledge, rumor, suspicion, and belief.
- [x] Add tests for validation parity, batching, bounds, and retry behavior; full workspace passes.

Validation (2026-11-25): orchestration policy 7/7, decision batch 5/5, batching parity 4/4,
AI observability 5/5, validation consistency 14/14, both frontend suites green, and the full
workspace gate green. Repaired decisions are revalidated before apply, budgets are enforced
per agent, stale contexts are rejected, and departed agent state is pruned.

Completion signal: `<promise>DONE</promise>`
