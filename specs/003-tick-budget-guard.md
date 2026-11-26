# Specification: Tick Budget Guard

## Feature: Per-Tick Operation Budget

### Overview
Large populations can cause a single tick to do excessive work. This spec adds a configurable `max_ops_per_tick` guard that defers remaining work to the next tick, ensuring the simulation never stalls.

### User Stories
- As a developer, I want the simulation to remain responsive at 500+ agents.
- As an observer, I want playback speed to be stable regardless of population.

---

## Functional Requirements

### FR-1: Add `max_ops_per_tick` config
Configurable maximum operations per tick. Default: 10000.

**Acceptance Criteria:**
- [ ] Config field exists with serde default
- [ ] Default value is 10000

### FR-2: Track operations in tick
Count operations (prisoner iterations, guard sweeps, encounter pairs, etc.) during `World::tick()`. When the budget is exhausted, defer remaining work.

**Acceptance Criteria:**
- [ ] Operation counter increments for each major step
- [ ] When budget exhausted, remaining prisoners/guards/encounters are deferred
- [ ] Deferred work resumes on next tick
- [ ] Deterministic ordering of deferred work

### FR-3: Expose budget usage
`Sim` exposes current tick budget usage and deferral count.

**Acceptance Criteria:**
- [ ] `Sim::tick_budget_usage()` returns (used, total)
- [ ] `Sim::tick_deferrals()` returns count of deferred items
- [ ] UI can display budget usage

### FR-4: No behavior change for small populations
For populations under 200, tick budget should never be exhausted.

**Acceptance Criteria:**
- [ ] All existing tests pass without deferral
- [ ] 730-day soak at 24 prisoners has zero deferrals

---

## Success Criteria

- 500-agent simulation maintains stable tick rate
- No deadlock or starvation from deferred work
- All existing tests pass

---

## Dependencies
- `alc-world` (World::tick, all step functions)
- `sumo` (Sim, observation DTOs)
- `alc-core` (Config)

## Assumptions
- Deferred work resumes in the same order on next tick
- Budget resets each tick

---

## Completion Signal

### Implementation Checklist
- [ ] `max_ops_per_tick` config field
- [ ] Operation counter in tick
- [ ] Deferral mechanism for each major step
- [ ] Resume deferred work next tick
- [ ] Budget usage exposed via Sim
- [ ] Tests for budget enforcement and determinism
- [ ] All existing tests pass

<!-- NR_OF_TRIES: 0 -->