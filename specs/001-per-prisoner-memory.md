# Specification: Per-Prisoner Memory Persistence

## Feature: Per-Prisoner Long-Term Memory

### Overview
Each prisoner must possess a persistent, serializable memory system that survives save/load and influences future AI decisions. Currently, `alc-agents` has a sophisticated four-layer memory system (sensory, short-term, long-term, institutional) with importance scoring, decay, consolidation, and bounded recall, but `alc-world` only stores a `memory_summary: String` on each `Prisoner` — no actual `Memory` or `LayeredMemory` is persisted per agent. This spec wires per-prisoner memory into the world simulation, persistence, and AI decision pipeline.

### User Stories
- As an observer, I want each prisoner to remember important experiences so their behavior evolves over time.
- As a developer, I want prisoner memory to be serialized and restored on save/load so worlds remain coherent across sessions.
- As an AI agent, I want to retrieve relevant memories when making decisions so choices reflect past experiences.

---

## Functional Requirements

### FR-1: Add `Memory` field to `Prisoner`
Store a `LayeredMemory` (from `alc-agents`) on each `Prisoner` in `alc-prisoner`. Must serialize/deserialize with `#[serde(default)]` for backward compat.

**Acceptance Criteria:**
- [ ] `Prisoner` has a `memory: LayeredMemory` field with `#[serde(default)]`
- [ ] Default memory is empty and bounded
- [ ] Existing saves without the field load with default empty memory
- [ ] Round-trip serialization preserves memory contents

### FR-2: Initialize memory on intake
When a prisoner is generated via `generate_intake`, create an initial memory with a `Sensory`/`ShortTerm` record of arrival (background, sentence, classification).

**Acceptance Criteria:**
- [ ] New prisoners have at least one memory record about their arrival
- [ ] Memory is seeded with prisoner identity (name, background, sentence)
- [ ] Memory uses per-agent `RngStream` for deterministic seeding

### FR-3: Record experiences into memory
During world simulation, record significant events into the relevant prisoner's memory:
- Co-presence encounters (friendship, argument, fight, trade, rumor)
- Gang recruitment/leadership changes
- Rehabilitation enrollment/completion
- Parole hearings (grant/deny/defer)
- Escape attempts and consequences
- Medical emergencies
- Disciplinary actions
- Guard interactions (positive/negative)

**Acceptance Criteria:**
- [ ] At least 5 event types create memory records
- [ ] Memory records include emotional valence and intensity
- [ ] Records are tagged with relevant `MemoryKind` (Episodic, Social, Emotional, etc.)
- [ ] Memory is bounded (does not grow unbounded)

### FR-4: Consolidate memory daily
Run memory consolidation on each day boundary for all prisoners. Consolidation promotes salient short-term memories to long-term, merges duplicates, decays old records, and enforces capacity caps.

**Acceptance Criteria:**
- [ ] `consolidate()` called for every prisoner on day boundary
- [ ] Long-term memory cap (500 records) enforced
- [ ] Short-term memory cap (9 slots) enforced
- [ ] Memory decay applied based on `decay_half_life_secs`

### FR-5: Wire memory into AI decision pipeline
When building `ValidationContext` or `ProviderContext` for a prisoner, include their memory records as epistemic context. The `EpistemicContext` buckets (DirectKnowledge, Rumor, Suspicion, Belief, ConfirmedFact) should be populated from memory recall.

**Acceptance Criteria:**
- [ ] `validation_context()` includes memory-derived epistemic data
- [ ] `provider_context_with_candidates()` includes memory records
- [ ] Memory recall is bounded (top-k per bucket)
- [ ] Decisions are measurably influenced by memory (test: prisoner with fight memory avoids same target)

### FR-6: Persist and restore memory
Memory must serialize/deserialize as part of `World` save. Loading a save restores all prisoner memories.

**Acceptance Criteria:**
- [ ] `World` save includes prisoner memory
- [ ] Load restores memory contents exactly
- [ ] `LayeredMemory` implements `Serialize`/`Deserialize`
- [ ] Old saves without memory load with empty default

### FR-7: UI observation
Prisoner detail panel shows memory summary (already exists via `memory_summary`). Optionally show recent memory records.

**Acceptance Criteria:**
- [ ] `memory_summary` is populated from memory consolidation output
- [ ] At least the most recent 3 memory records are visible in prisoner detail

---

## Success Criteria

- Prisoners remember significant experiences and their decisions reflect past encounters
- Memory persists across save/load with exact fidelity
- Memory is bounded (no unbounded growth over 730-day soak)
- At least one test demonstrates memory influencing a decision

---

## Dependencies
- `alc-agents` crate (LayeredMemory, MemoryKind, MemoryLayer, consolidate, recall)
- `alc-prisoner` crate (Prisoner struct, generate_intake)
- `alc-world` crate (World tick, validation_context, provider_context_with_candidates)
- `alc-persistence` crate (save/load)

## Assumptions
- `LayeredMemory` already implements `Serialize`/`Deserialize` (verify)
- Memory records use `SimTime` for timestamps
- Memory consolidation runs once per day boundary, not per tick

---

## Completion Signal

### Implementation Checklist
- [ ] `Prisoner` has `memory: LayeredMemory` field with `#[serde(default)]`
- [ ] `generate_intake` seeds initial memory
- [ ] World records encounters/gang/rehab/parole/escape/medical/discipline into memory
- [ ] Daily consolidation runs for all prisoners
- [ ] Memory wired into `validation_context` and `provider_context`
- [ ] Memory persists and restores on save/load
- [ ] `memory_summary` populated from consolidation
- [ ] Tests added for memory persistence, consolidation, and decision influence
- [ ] All existing tests pass

### Testing Requirements

#### Code Quality
- [ ] All existing unit tests pass
- [ ] All existing integration tests pass
- [ ] New tests added for new functionality
- [ ] No lint errors

#### Functional Verification
- [ ] All acceptance criteria verified
- [ ] Edge cases handled (empty memory, max capacity, decay to zero)
- [ ] Error handling in place

---

## Completion Signal

**Only when ALL checks pass, output:** `<promise>DONE</promise>`

<!-- NR_OF_TRIES: 0 -->