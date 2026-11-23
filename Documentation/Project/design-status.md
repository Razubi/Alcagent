# Design Status — Implementation Cross-Reference

Maps each design document to implemented state. Maintained docs only; status reflects
2026-11-23 (Wave 4 in progress). For version history see `version-milestones.md`.

## Design → implementation summary

| Design doc | Implemented in | Notes |
|---|---|---|
| `Design/Core/prison-simulation.md` | `alc-prisoner` | Prisoner model, 29-need pressure engine (design called for 34), schedules (10 attendance outcomes), correlated intake. |
| `Design/Core/social-systems.md` | `alc-social` | Relationships, emergent gangs, conflict escalation/outcomes, escape motivation/consequences, role detection. Info model lives in `alc-agents/memory.rs`. |
| `Design/Core/warden-system.md` | `alc-warden`, `alc-guard`, `alc-economy` | Warden traits/objectives/arbitration, guard duty loop/authority/shifts, money ledger/health, 12-building expansion catalog. Fog of war = inbox of report IDs (no full KnowledgeStore). |
| `Design/Core/ai-agent-systems.md` | `alc-agents` | 14 decisions, 8-stage validation, 4-layer memory, orchestrator/token/rate limiting, personality `Disposition`. |
| `Design/Engine/sumo-engine.md` | `sumo`, `alc-world` | `Sim` owns `World`: time, decisions, observation/stats, events, save/load/autosave, invariants. Engine validates; agents never mutate world directly. |
| `Design/Planning/beta-0.05.md` / `Design/Engine/sumo-next.md` | planned/in progress | Wave 4 work; see `Documentation/Communication/Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md`. |

## Verified implemented (current codebase)

- **`alc-world::World::tick`**: needs drift, hourly tension-aware guard duty sweep (only critical/assistance reports emitted to the bus), schedule resolution, need-driven utility decisions (orchestrator-gated + deterministic fallback with trait bias), co-presence encounters, daily boundary (funding/expenses/decay, sentences, escapes, parole, warden arbitration, capacity-gated intake, weekly gang formation/maintenance).
- **All 15 `WardenAction` variants** applied; economy sustainability (base grant, de-escalation, lockdown expiry, staff right-sizing).
- **`sumo` engine API**: time control, validated decisions, observation/stats, events, persistence + autosave, invariants, report, trace hash, guard/warden drivers, parole ledger/API, simple rehab relief, engine-side incident/decision/post-release/policy helpers (`record_incident`/`incident_log`, `record_decision`/`decision_log`, `set_post_release`/`post_release_status`, `set_policy`/`set_lockdown`).
- **`alc-ui` + `alcagent`**: interactive ratatui observer (`observe`, default), headless `run`, `docs`.
- **Persistence** (`alc-persistence`): bincode save format v2 with versioned header, atomic writes, JSON debug sidecar, migration hook, autosave day-dedup. `Sim` has EngineState converters for engine-side state.

## Present but not fully integrated

- **Wave 4 additions (landed):** engine R1–R7 surface is present (`Sim::map_frame`, `propose_decision`, policy modes, structured incident logs, extended invariants/state digest, driver event wiring); persistence converter edits were applied and the workspace is green. See `Documentation/Communication/Note's/wave4-implementation-status.md`.
- **Contraband/rehab** library modules exist (`alc-social::contraband`, `alc-social::rehab`) but full world-tick lifecycle wiring is incomplete.
- **Post-release tracking** partial (sumo engine-side helpers; persistent community outcomes not fully wired).
- **`alc-gui`** crate exists at `crates/alc-gui` but is not a workspace member and does not build standalone.

## Known gaps / Wave 4 work items (deferred to 0.1 beta or later)

1. Full `KnowledgeStore` (design §9) — warden uses inbox IDs.
2. Wave 4 engine support R1–R6: map frame, decision proposal/dry-run, policy mode switch, structured incidents with causality, persistence migration, new invariants.
3. Contraband and rehab full lifecycle wiring.
4. Post-release outcome tracking (design §15).
5. AI-generated narrative text (orchestrator has no provider client).
6. `World::summary()` / CLI ergonomics; `alcagent --ui` not present (TUI is `observe`).
7. `alc-gui` integration decision + doc cleanup.
8. Wave 5 items: real navigation graph, UI map switch to `MapFrame`, council/shadow behavior, driver authority unification, `&mut Sim` applier signature.

## Wave 4 driver / integration pass (Integration & Docs engineer)

`sumo/src/drivers.rs` changes in this pass:
- `apply_warden_action` emits `WorldEvent`s for previously silent actions
  (`PromoteStaff`, `ChangePolicy`, `IncreaseSecurity`, `ImproveInfrastructure`);
  `RespondToDisturbance` includes its `event_id` in the summary.
- `HireGuards`/`FireGuards` maintain `guard_location` so the pawn-location
  invariant stays clean across staffing actions.
- `ChangePolicy { policy: "lockdown" }` cannot sync `engine_state.policy_state`
  with only `&mut World`: callers must use `Sim::set_lockdown(true)`. A Wave 5
  `&mut Sim` applier signature is recommended.
- Module doc records the duplicated driver authority (`World::tick` runs its
  own guard/warden cycles; `Sim::tick` does not call these drivers). Unify in
  Wave 5.

Integration coverage: `crates/sumo/tests/integration_wave4.rs` adds four tests
(full 30-day flow with invariants/map/save-load/trace, warden-action event
emission, guard report → warden inbox, shadow-mode safety). All four pass; the
full workspace is green (184 passed, 0 failed, 1 ignored). See
`Documentation/Communication/Note's/wave4-implementation-status.md` for the R1–R7
breakdown and how the two world-level bugs these tests exposed (stale social
dyads after release; world-tick policy drift) were fixed during integration.
