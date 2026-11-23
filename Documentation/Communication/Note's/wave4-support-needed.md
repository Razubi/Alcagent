# NOTE: Wave 4 support needed across teams

**From:** Lead
**To:** Cognition, World/Space, UI, Social/Economy, QA workers, and the Sumo AI team
**Date:** 2026-11-20
**Status:** OPEN
**Blocks:** Wave 4 vertical slice (intentions, council, memory, space, live map, conflicts)
**Owner of the fix:** split by section below

Formal engine API requests are in `../Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md`. This note covers the
work that is **not** a `sumo` task, plus the cross-team ordering traps.

## What I found

The body and society work; the top of the brain and the spatial/observer surfaces don't.
Pawns re-decide every tick, memories don't steer choices, "space" is a facility id, conflicts are
prose strings, and there is no live map. The `sumo` control surface can't describe any of the new
state (see the engine request), but most of that state has to be built first in the `alc-*` crates.

## What needs doing

### Cognition worker — `alc-prisoner`, `alc-agents`
1. **Intention state on `Prisoner`** — a held plan with target, age, and interruption cause. Must be
   `#[serde(default)]` so Wave 3 saves load. Blocks: NPC-01, and the inspector display.
2. **Council of drives** — replace single-score choice with evaluators (Survival, Social,
   Fear/Opportunity, Duty, Institution) returning score **and rationale**. Trait weights decide the
   winner. Must expose scores/rationales without applying so `sumo` R2 can surface them.
3. **Memory recall in candidate generation** — ranked memories and epistemic confidence feed the
   council; a believed false rumor must cause wrong behavior someone else remembers.
4. **Mood with duration** and **`VoiceProfile`** speech acts — deferred within the wave, but the
   state fields should be added now to avoid a second migration.

### World/Space worker — `alc-world`, `alc-guard`, `alc-warden`
5. **Navigation graph** in world state — nodes, edges, sightlines, travel progress. `sumo` should
   only read it (engine request Q4). Blocks: WRLD-01, UI-01, and all proximity-based conflict.
6. **Guard suspicion + patrol intention** on `Guard`, with serde defaults.
7. **Warden KnowledgeStore** — claims with provenance, confidence, source standing; warden acts on
   belief, which can be wrong.
8. **Structured conflict handlers** — argument topics, staged fights, gang retaliation — writing
   incident records once `sumo` R4 lands. Don't invent a parallel event bus.

### UI worker — `alc-ui`, `alcagent`
9. **Live map screen** — consume `sumo` R1 `MapFrame`; do **not** reach into `World` directly. If
   R1 isn't ready when you are, leave a placeholder and escalate rather than bypass the engine.
10. **Pawn brain inspector** — consume `sumo` R2 `DecisionProposal`: needs, mood, intention, last
    memories, relationships, council rationale.
11. **Conflict overlay + critical ticker** — consume R1 markers + R4 incident records.

### Social/Economy worker — `alc-social`, `alc-economy`
12. **Argument topics and claims**, witness updates, staged fight escalation, gang retaliation
    cycles. Emit structured incidents (R4), not free-text summaries.
13. Keep the sensitive abuse card (CON-05) deferred, but ensure the incident schema can carry
    victim/witness/accountability fields when it is implemented. No reward loop, no graphic output.

### QA worker — `sumo/tests`, `alc-testing`
14. **Shadow-mode harness** once R2/R3 land; deterministic CI stays model-free.
15. **Conflict causality audit** once R4/R6 land: no orphan fights, no consequence-free incidents.
16. Re-run the 730-day three-seed soak after each new state field is introduced — migrations are
    the most likely place for a regression.

## Ordering trap

Do **not** build the map before the navigation graph, and do **not** build the inspector before
the council exposes rationales. Either order produces a beautiful screen full of fake data that
then has to be thrown away. R5 (persistence) must land before any new state is serialized.

## Acceptance criteria

- A Wave 3 save loads into the Wave 4 binary and replays to the same `trace_hash`.
- Selecting any pawn on the live map explains what it wants, what it's doing and why.
- A sampled fight can be traced: need → memory/rumor → argument topic → witnesses → outcome →
  consequence.

## Resolution

<pending>
