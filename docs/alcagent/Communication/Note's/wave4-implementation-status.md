# NOTE: Wave 4 implementation status (engine integration pass)

> **Historical handoff (retained; not active):** R1–R7, council/shadow behavior, brain-inspector
> data and the UI map switch landed after this note. The `OPEN`/Wave 5 language below describes the
> state at the time; current status is in
> [`../../Project/design-status.md`](../../Project/design-status.md).

**From:** Integration / Drivers & Docs engineer (sumo dev team)
**To:** Lead, Cognition, World/Space, UI, Social/Economy, QA workers
**Date:** 2026-11-23
**Status:** OPEN (Wave 5 items listed below)
**Blocks:** Final Wave 4 acceptance (`wave4-support-needed.md`)
**Owner of the fix:** split by section below

## What I found / intend

The Wave 4 engine surface (R1–R7) is largely present in `sumo`, but before this
pass several warden actions applied silently, guard hire/fire left location
invariant stale entries, and no end-to-end test exercised the full flow. This
note records what landed in the integration/driver/docs pass, what remains for
other teams, and the open items deferred to Wave 5.

## R1–R7 status table

| Item | Request | Implemented in this pass | Remaining / owner |
|---|---|---|---|
| R1 | Live map frame | `Sim::map_frame()` + `MapFrame` (lead); new integration test asserts prisoners/guards pawns are returned | UI worker: switch live map screen to consume `MapFrame` (do not reach into `World`); World/Space: real nav graph — currently a grid of facility positions |
| R2 | Decision proposals + audit | `Sim::propose_decision`, `validate_only`, `DecisionProposal`, decision audit log already present; integration test delivers decisions through `Sim::deliver_decision` | Cognition: expose council scores/rationales + recalled memories; UI: pawn-brain inspector |
| R3 | Policy / policy mode | `Policy`, `PolicyMode`, `set_policy`, `set_lockdown`, `set_policy_mode` present; integration test switches to `Shadow` and runs 7 days clean | **Inert behavior**: `Council`/`Shadow` are persisted/queryable only. Real A/B shadow path is a Wave 5 open item |
| R4 | Incidents + causality | `record_incident`, `record_sidecar_incident`, `incidents_since` present; log agent wired applied decisions into logs; invariant ensures cause/consequence resolution | Social: structured argument/fight/retaliation incident handlers |
| R5 | Persistence / determinism | Persistence agent rebuilding full `EngineState` round-trip; lead added `Sim::trace_hash_v2()`; integration test asserts legacy `trace_hash` equality across save/load and checks v2 opportunistically | Stabilize versioned save format; confirm `trace_hash_v2` equality in final CI once migration lands |
| R6 | Invariants + state digest | `check_invariants` extended (pawn location incl. stale guards, intention targets, incident causality, suspicion refs, policy sync, log bounds); `state_digest` present | QA: re-run 730-day 3-seed soak after every new state field |
| R7 | Extension points reserved (design-only) | Not part of this pass (crowd/riot, informant trees, warden claims, credit/debt, post-release hooks). Driver/log wiring landed under R4/R6: `drivers.rs` emits events for `PromoteStaff`, `ChangePolicy`, `IncreaseSecurity`, `ImproveInfrastructure`; uses `RespondToDisturbance.event_id`; `guard_location` seeded on hire and cleaned on firing; module doc records the `World::tick` duplication | Wave 5: unify driver authority; reserve R7 extension hooks when corresponding state lands |

## Concrete changes in `sumo/src/drivers.rs`

- Added `WorldEvent`s (EventKind `PolicyChange` / `BudgetChange`) for previously
  silent actions: `PromoteStaff`, `ChangePolicy`, `IncreaseSecurity`,
  `ImproveInfrastructure`.
- `RespondToDisturbance { event_id }` summary now includes the id.
- `HireGuards` inserts a `guard_location` entry for each new guard (first
  facility / facility 0 fallback); `FireGuards` removes the fired guards'
  entries so no stale pawn locations remain.
- `ChangePolicy { policy: "lockdown" }` still cannot sync
  `engine_state.policy_state` (applier only receives `&mut World`). A doc
  comment warns callers to use `Sim::set_lockdown(true)`; lead may change the
  signature to `&mut Sim` in Wave 5.
- Module-level doc note: `World::tick` already runs duty/warden cycles, and
  `Sim::tick` deliberately does not call these drivers. Unify in Wave 5.

## New integration test

`crates/sumo/tests/integration_wave4.rs`:
- `wave4_full_flow_runs_and_stays_invariant_clean`
- `warden_actions_apply_and_emit_events`
- `guard_report_delivery_informs_warden`
- `policy_mode_switch_does_not_break_simulation`

## Findings from the integration test — RESOLVED by lead during integration

- **Stale social dyads after release**: FIXED. `SocialGraph::retain(&HashSet<PrisonerId>)`
  was added in `alc-social` and is called from `alc-world::prune_agent_state`, so
  relationships to released/transferred/dead prisoners are pruned alongside
  activity/decisions/locations. The new invariant now stays clean on 730-day soaks.
- **Policy-state drift from the world tick**: FIXED. `Sim::sync_policy_from_world()`
  runs after every `world.tick()` and after `Sim::apply_warden_action`, deriving
  the engine policy band from `world.policies.security_level` and snapping drifted
  world values back into the band. 730-day 3-seed soaks are invariant-clean.
- **Historical actor ids in incident logs**: the causality invariant deliberately
  no longer checks actor liveness in WorldEvents or sidecar incidents — incidents
  are historical records and legitimately reference released prisoners. The
  invariant's R6 guarantees are unique ids and resolvable cause/consequence links.
- **Unbounded engine audit logs**: `Sim::trim_engine_logs()` caps incidents,
  incident sidecar and decision log at `MAX_LOG_ENTRIES` (10,000), invoked from
  the tick event pump and all `record_*` methods.

## Council / Shadow / brain-inspector pass (2026-11-24, resumed)

The previously inert `PolicyMode` now has real behavior:
- `sumo/src/council.rs` — deterministic, RNG-free council of seven institutional
  drives (`Security`, `Finance`, `Morale`, `Welfare`, `Reputation`, `Rehab`,
  `Stability`). `DriveWeights::from_warden` derives weights from warden
  personality; `candidate_actions` builds a state-driven menu; `deliberate`
  scores candidates with stable tie-breaks and a human-readable rationale.
- `sumo/src/drivers.rs` — `warden_cycle` dispatches on `PolicyMode`:
  `Deterministic` keeps the original arbitrate path (trace parity preserved),
  `Council` applies the council winner and emits a rationale event, `Shadow`
  runs both and applies the baseline while emitting a `SHADOW divergence` event.
- `DecisionProposal` now carries `scores: Vec<EvaluatorScore>`, `confidence`,
  and `memory_records: Vec<MemoryCitation>`, plus `explanation()` for the brain
  inspector. `evaluate_candidates(world, id)` returns scored alternatives with
  real targets. `AgentObservation` gained `memory_summary` and
  `current_intention`.

New tests: `council_shadow.rs` (5), `proposal_inspector.rs` (5),
`agent_observation_includes_brain_inspector_data`. Workspace now: **184 passed,
0 failed, 1 ignored**.

## Open items for Wave 5

1. **Real navigation graph** — World/Space worker to add nodes/edges/sightlines;
   `MapFrame` currently synthesizes a grid from facility list.
2. **UI map switch** — UI worker to consume `Sim::map_frame()` rather than
   reading `World` directly; also render `DecisionProposal::explanation()` in a
   brain-inspector panel.
3. **Real per-prisoner memory retrieval** — `memory_citations` currently uses
   `prisoner.memory_summary` plus event-bus history as a documented approximation;
   replace with `alc_agents::Memory` once prisoners own one.
4. **Shadow diff structured API** — currently divergence is an event summary;
   add an `EngineState.shadow_diff` field and `Sim::shadow_diff()` if A/B tooling
   needs machine-readable comparisons.
5. **Driver authority unification** — decide whether `World::tick` delegates to
   the `sumo` drivers or the drivers are removed; avoid double-reporting duty
   observations and double-applying warden actions.
6. **Applier signature** — change `apply_warden_action` to take `&mut Sim` so
   `ChangePolicy "lockdown"` can keep `engine_state.policy_state` in sync.

## Acceptance criteria

- `cargo test -p sumo --test integration_wave4` green after all agents' edits land.
- `cargo test --workspace` green with no new warnings in `drivers.rs`.
- A Wave 3 save loads into the Wave 4 binary and replays to the same trace hash
  (QA acceptance from `wave4-support-needed.md`).

## Resolution

Wave 4 engine integration plus the council/shadow and brain-inspector pass is
**complete in-house** as of 2026-11-24. Workspace: **184 passed, 0 failed,
1 ignored** (`logs_over_limit_are_detected`, a 10k-entry stress test run
explicitly). 730-day 3-seed soak, save/load determinism, and all Wave 4 +
council test suites are green. Remaining items (real nav graph, UI map/brain
panels, real per-prisoner memory, structured shadow diff, driver-authority
unification, applier signature) are tracked as Wave 5 above.
