# Specification: Event-History Compaction

## Feature: Bounded World Event History

### Overview
The event bus is bounded (50k), but `World::history: Vec<WorldEvent>` grows indefinitely. Over long simulations this causes unbounded memory growth. This spec adds compaction: keep the last N days of events plus all major events, prune insignificant ones.

### User Stories
- As an observer, I want to view recent event history without memory blow-up over years of simulation.
- As a developer, I want the world to remain stable during 730+ day soaks.

---

## Functional Requirements

### FR-1: Add `max_history_days` config
Configurable maximum days of full-resolution event history to retain. Default: 30 days.

**Acceptance Criteria:**
- [ ] Config field exists with serde default
- [ ] Default value is 30 days

### FR-2: Compact on day boundary
On each day boundary, prune events older than `max_history_days` unless they are marked as major (escape, death, gang formation, parole, policy change, construction complete).

**Acceptance Criteria:**
- [ ] Events older than threshold are removed
- [ ] Major events are always retained
- [ ] Compaction runs on day boundary
- [ ] No unbounded growth over 730-day soak

### FR-3: Major event tagging
Events have a `major` flag. Major events include: EscapeSucceeded, Death, GangFormed, GangSplit, ParoleGranted, PolicyChange, ConstructionComplete, TransferOut, Riot/GroupDisturbance.

**Acceptance Criteria:**
- [ ] `WorldEvent` has `major: bool` field with `#[serde(default)]`
- [ ] Major events are auto-tagged at emission time
- [ ] Compaction preserves all major events regardless of age

### FR-4: UI reflects compaction
Event timeline shows "compacted" indicator when history has been pruned.

**Acceptance Criteria:**
- [ ] UI shows total event count including compacted
- [ ] No broken references in incident causal chains after compaction

---

## Success Criteria

- 730-day soak uses bounded memory (< 100MB for event history)
- No broken incident/event causal chain references after compaction
- All existing tests pass

---

## Dependencies
- `alc-world` (World, WorldEvent, EventBus)
- `alc-events` (EventKind, WorldEvent)
- `sumo` (observation DTOs)

## Assumptions
- Compaction runs on day boundary, not per tick
- Major event classification is based on EventKind

---

## Completion Signal

### Implementation Checklist
- [ ] `max_history_days` config field added
- [ ] `WorldEvent.major` field added with serde default
- [ ] Major events auto-tagged at emission
- [ ] Compaction logic on day boundary
- [ ] Causal chain references validated after compaction
- [ ] Tests for compaction correctness and bounded growth
- [ ] All existing tests pass

<!-- NR_OF_TRIES: 0 -->