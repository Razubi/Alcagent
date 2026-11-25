# REQUEST: Engine support required by the next update (Wave 4)

> **Historical handoff (retained; not active):** Wave 4 R1–R7 landed, council/shadow behavior and
> the UI map switch shipped in 0.05, and remaining depth is tracked in
> [`../../Project/design-status.md`](../../Project/design-status.md). The body below is preserved
> as the original request.

**From:** Lead / simulation team
**To:** Sumo AI team (owner of `crates/sumo`)
**Status:** OPEN — nothing here is implemented; aligned with the finalized beta-0.05 update list and next-version plan.
**Blocks:** Wave 4 cards WRLD-01, UI-01, UI-02, UI-05, NPC-02, NPC-03, QA-01, QA-02
**Related:** `../Project/PROJECT.md` (Wave 3 baseline), `../Communication/Note's/wave4-support-needed.md`

## Summary

`sumo` currently supports the *spine* of the simulation — time control, event bus with ids and
`trace_hash`, decision validation and application, warden/guard drivers, save/load and invariants.
It does **not** support the state or read surfaces the next update introduces.

Wave 4 adds persistent intentions, an in-house council of drives with rationales, memory-driven
choices, a spatial navigation graph, real-time pawn positions, structured conflicts and a live
map screen. Most of that state will be built in the `alc-*` crates. Without the extensions below,
`sumo` stops being the engine and becomes a test harness while the UI reaches around it into
`World` directly. That is the failure mode this request exists to prevent.

None of these require an external AI model or provider.

---

## 0. Merged context — audit, update lists, and communication state

This request is the engine-support layer of the **beta 0.05** update line
(`Design/Planning/beta-0.05.md`) and the **next-version** plan
(`Design/Engine/sumo-next.md`). It consolidates the merged findings of the
Phase-1 audit (`../Reports/audit-report.md`, `../../sumo/sumo-engine-research.md`, `../../sumo/SUMO_ROADMAP.md`)
and the deep-research pass, and absorbs the cross-team framing from
`Communication/Note's/wave4-support-needed.md` (ordering traps, ownership split).

### 0.1 Audit / validation state feeding this request

- Phase-1 audit: build broken → fixed; `sumo` facade added; baseline green.
- Beta-0.05 engine fixes shipping: `EventKind::SecurityResponse`/`GuardReport` (dup tail removed), `EventBus::new(50_000)`, `AgentObservation` restored, workspace + `sumo` build/test green.
- Deep-research ground truth for the engine surface: `Sim::tick` does not run guard/warden drivers; `Sim::load` resets parole ledger + population history; decision pipeline ends in `_ => ok()` catch-all; `MoveTo` writes `schedule.wing`; `Trade` is effect-free; `World.history` never appended; warden applier duplicated in `alc-world` + `sumo::drivers`.

### 0.2 Engine work items already covered / aligned

- R5 persistence migration is required before new state (map frame, intentions, incidents) serializes.
- R6 invariants must cover new state (position coherence, intentions legality, incident causality).
- R7 extension points to reserve (crowd/riot, informant trees, warden claims, credit/debt, post-release).

### 0.3 Merge with communication notes

- Ordering traps from `Note's/wave4-support-needed.md` apply here: do not build the map before the navigation graph; do not build the inspector before the council exposes rationales; R5 must land before new state serialization.

---

## R1 — Map frame API (blocking UI-01, UI-05)

**Need:** a point-in-time read of every pawn suitable for rendering a live map.

`Sim::map_frame() -> MapFrame` exposing, per pawn:
- current node (facility/cell/corridor/yard) **and** progress along the connecting edge
- pawn id, faction/gang, classification, mood, current intention, threat/risk status
- guards and prisoners distinguished, with post/patrol state for guards
- active incidents at or near each node (from R4)

Also expose the graph itself: nodes, edges, sightline links, and which nodes are locked/restricted.

**Why:** the live map cannot render moving dots from `Observation`, which is facility-level and
carries no position or movement state.

**Acceptance:** a UI can render one frame and correctly draw every pawn mid-movement between two
nodes, without touching `World`. Frame is cheap to clone and deterministic for a given sim time.

---

## R2 — Decision proposal / dry-run API (blocking NPC-02, UI-02, QA-01)

**Need:** ask what a pawn *would* do without doing it.

`Sim::propose_decision(pawn) -> DecisionProposal` returning, for each candidate action:
decision, council evaluator scores, the winning rationale string, confidence, and the memory
records that influenced the choice. No world mutation, no event emission, no budget charge.

**Why:** the pawn inspector needs to show "what it wants / what it's doing / why", and shadow mode
needs to compare the council's proposal against the deterministic baseline side by side.

**Acceptance:** calling `propose_decision` twice at the same sim state returns identical output;
calling it does not change `trace_hash`, event count, budget or world state.

---

## R3 — Policy switch for deterministic / council modes (blocking QA-01)

**Need:** `Sim::set_policy(PolicyMode)` with at least `Deterministic` and `Council`, plus a way to
run both against the same seed and diff `trace_hash()` and event distributions.

**Why:** CI and the 730-day soak must stay reproducible and model-free. Evaluation needs an
honest A/B of the new reasoner without polluting the baseline.

**Acceptance:** `Deterministic` reproduces today's traces exactly for existing seeds; the mode is
serialized in saves (or explicitly documented as engine-scoped if not); switching mode mid-run is
either supported or returns a clear error.

---

## R4 — Structured incidents with causality (blocking CON-01, CON-02, CON-03, UI-05, QA-02)

**Need:** conflict events carry machine-readable data, not only `summary` + `cause` strings.

An incident record should include: kind (argument/threat/fight/gang clash), topic, initiator and
target, witnesses, location node, outcome, injuries, and links to the cause event and subsequent
events (investigation, discipline, retaliation). Keep `EventKind` enum indices stable for old saves.

**Why:** "specific arguments", witness-dependent testimony, retaliation chains and the causality
audit are impossible if the only record is prose. This is also what lets the map show an incident
marker with meaningful detail.

**Acceptance:** given any `Fight` or `Argument` event, a test can name its topic, participants,
witnesses and preceding cause; no incident is orphaned.

---

## R5 — Persistence migration for new fields (blocking all of the above in saved worlds)

**Need:** every new field on `Prisoner`, `Guard`, `Warden`, `World` and events must deserialize from
existing saves. Use `#[serde(default)]` with sensible values, or a version bump plus migration.

New state to account for: intention, mood, council weights, memory recall cache, node + edge
position, patrol route, suspicion map, KnowledgeStore claims, structured incidents.

**Why:** the project promises persistent worlds. A save from Wave 3 that fails to load after
Wave 4 is a regression, not an inconvenience.

**Acceptance:** a Wave 3 save file loads into the Wave 4 binary, invariants are clean, and a
subsequent save/load replay produces the same `trace_hash`.

---

## R6 — New invariants (blocking QA-02)

**Need:** extend `Sim::check_invariants()` with engine checks for the new state:
- a pawn is in exactly one place (node/edge progress is coherent)
- every intention references an existing target and a legal action
- every fight/riot/abuse event has a cause and at least one consequence recorded
- no two pawns occupy the same discrete point (or an explicit, bounded overlap rule)
- suspicion/claim records reference existing pawns

**Why:** the soak tests are how we detect deadlocks, drift and contradictions. New state without
new invariants is invisible to them.

**Acceptance:** injecting a broken position, a dangling intention or an orphan fight makes
`check_invariants()` fail with a specific message.

---

## R7 — Extension points to reserve now (not this update)

Please leave room in the observation/driver layer for later cards so they don't require a second
engine rewrite:
- crowd state and riot phases (CON-04)
- guard Cultivation of informants / suspicion trees (POL-02)
- warden claims with provenance and standing (GRD-02)
- prisoner credit/debt ledgers (WRLD-03)
- post-release outcome records (WRLD-06)

No implementation needed now — just avoid APIs that assume the current flat shapes are permanent.

---

## Dependency order

```
R5 (persistence)  ──► everything (saves must survive)
R1 (map frame)    ──► UI-01, UI-05
R4 (incidents)    ──► R1 markers, UI-05, QA-02
R2 (proposal)     ──► UI-02, QA-01
R3 (policy)       ──► QA-01
R6 (invariants)   ──► QA-02, all soak tests
```

## Open questions for the sumo team

1. Should `MapFrame` be part of `Observation` or a separate type? (Recommendation: separate —
   `Observation` is already a cheap stat snapshot.)
2. Is engine policy mode serialized, or does load always reset to the binary default?
3. Can `EventKind` gain structured payloads without breaking bincode saves by adding a sidecar
   `Vec<Incident>` keyed by event id rather than changing the enum?
4. Who owns the navigation graph: `alc-world` state or `sumo` engine state? (Recommendation:
   `alc-world`, with `sumo` exposing read access only.)
