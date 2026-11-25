# Design Status — Implementation Cross-Reference

Maps each design document to implemented state. Maintained docs only; status reflects
2026-11-25 (all 14 Ralph specs complete). For version history see `version-milestones.md`.

## Design → implementation summary

| Design doc | Implemented in | Notes |
|---|---|---|
| `Design/Core/prison-simulation.md` | `alc-prisoner` | Prisoner model, 29-need pressure engine (design called for 34), schedules (10 attendance outcomes), correlated intake. |
| `Design/Core/social-systems.md` | `alc-social`, `alc-world` | Relationships, emergent gangs and internal politics, conflict escalation/outcomes, escape motivation/consequences, role detection. Gang history is event-sourced in `World`; info model lives in `alc-agents/memory.rs`. |
| `Design/Core/warden-system.md` | `alc-warden`, `alc-guard`, `alc-economy` | Warden traits/objectives/arbitration, guard duty loop/authority/shifts, money ledger/health, 12-building expansion catalog. Fog of war = inbox of report IDs (no full KnowledgeStore). |
| `Design/Core/ai-agent-systems.md` | `alc-agents`, `alc-world`, `sumo` | 14 decisions, 8-stage validation, 4-layer memory with epistemic buckets, offline deterministic provider, bounded batching/cache/retry/rate limits, personality `Disposition`, runtime telemetry. |
| `Design/Engine/sumo-engine.md` | `sumo`, `alc-world` | `Sim` owns `World`: time, decisions, observation/stats, events, save/load/autosave, invariants. Engine validates; agents never mutate world directly. |
| `Design/Planning/beta-0.05.md` / `Design/Engine/sumo-next.md` | implemented | Wave 4/5 work landed; see `../Communication/Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md`. |

## Verified implemented (current codebase)

- **`alc-world::World::tick`**: needs drift, hourly tension-aware guard duty sweep (only critical/assistance reports emitted to the bus), schedule resolution, need-driven utility decisions (canonical orchestrator gate + deterministic fallback with trait bias), co-presence encounters, daily boundary (funding/expenses/decay, sentences, escapes, parole, warden arbitration, capacity-gated intake), and weekly gang formation/politics (disputes, leadership changes, splits, mergers, rivalries/alliances, event-sourced history).
- **All 15 `WardenAction` variants** applied; economy sustainability (base grant, de-escalation, lockdown expiry, staff right-sizing).
- **`sumo` engine API**: time control, validated decisions, observation/stats, events, persistence + autosave, invariants, report, trace hash, guard/warden drivers, parole ledger/API, simple rehab relief, engine-side incident/decision/post-release/policy helpers (`record_incident`/`incident_log`, `record_decision`/`decision_log`, `set_post_release`/`post_release_status`, `set_policy`/`set_lockdown`).
- **`alc-ui` + `alcagent`**: interactive ratatui observer (`observe`, default), headless `run`, `docs`.
- **Persistence** (`alc-persistence`): bincode save format v5 with versioned header, atomic writes, JSON debug sidecar, v1/v3/v4 migration, autosave day-dedup and surfaced failures. `Sim` has EngineState converters for engine-side state.

## Present but not fully integrated

- **Wave 4 additions (landed):** engine R1–R7 surface is present (`Sim::map_frame`, `propose_decision`, policy modes, structured incident logs, extended invariants/state digest, driver event wiring); persistence converter edits were applied and the workspace is green. See `../Communication/Note's/wave4-implementation-status.md`.
- **Contraband/rehab** are wired into daily world ticks and validated controls with persisted state and both-frontend views (spec 005).
- **Post-release tracking** persists community outcomes and feeds reputation/policy (spec 005).
- **`alc-gui`** is a buildable workspace member rendering the spatial map; see spec `dev/ralph/specs/002-map-gui-acceptance.md` for the accepted R0 state.

## Known gaps and remaining Wave 4/5 work items

1. **Major UI/GUI overhaul + real game map:** complete — the TUI and egui GUI both render a spatial map from `sumo::MapFrame` (cell/wing occupancy, pawn positions, movement/path hints, incident overlays, facility condition/capacity/utilization, pan/zoom, hover/click detail). Only real nav pathfinding remains as Wave 5 depth.
2. Full `KnowledgeStore` (design §9) — warden uses delivered-report IDs and budget summaries rather than omniscient state (spec 010); deeper claim/provenance standing remains future work.
3. Wave 4 engine support R1–R7: complete.
4. Contraband and rehab full lifecycle wiring: complete (spec 005).
5. Post-release outcome tracking (design §15): complete (spec 005).
6. Warden institutional strategy, finance/attention gates and persisted rationale: complete (spec 010).
7. AI-generated narrative text and provider-backed model adapters remain future work; deterministic/utility deliberation is authoritative.
8. `World::summary()` / CLI ergonomics; `alcagent --ui` not present (TUI is `observe`).
9. Warden action application now has one authoritative world-side applier; world ticks still run their own autonomous cycles, which `Sim::tick` reconciles.
10. Real navigation-graph depth remains Wave 5 work; the required 0.05 map consumes the existing `MapFrame` surface.

## Wave 4 driver / integration pass (historical; superseded by spec 010)

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
  own guard/warden cycles; `Sim::tick` does not call these drivers). Superseded
  by spec 010: all explicit warden actions now delegate to one authoritative
  `World::apply_warden_action`, and `Sim` reconciles the policy mirror after
  world-tick arbitration.

Integration coverage: `crates/sumo/tests/integration_wave4.rs` adds four tests
(full 30-day flow with invariants/map/save-load/trace, warden-action event
emission, guard report → warden inbox, shadow-mode safety). All four pass; the
full workspace is green (including long-run soaks). See
`../Communication/Note's/wave4-implementation-status.md` for the R1–R7
breakdown and how the two world-level bugs these tests exposed (stale social
dyads after release; world-tick policy drift) were fixed during integration.
