# Design Status — Implementation Cross-Reference

Maps each design document to implemented state. Maintained docs only; status reflects
2026-11-25 (all 14 Ralph specs complete). For version history see `version-milestones.md`.

## Design → implementation summary

| Design doc | Implemented in | Notes |
|---|---|---|
| `Design/Core/prison-simulation.md` | `alc-prisoner` | Prisoner model, 25 stored need-deficit pressures (design called for 34), schedules (10 attendance outcomes), correlated intake. |
| `Design/Core/social-systems.md` | `alc-social`, `alc-world` | Relationships, emergent gangs and internal politics, conflict escalation/outcomes, escape motivation/consequences, role detection. Gang history is event-sourced in `World`; info model lives in `alc-agents/memory.rs`. |
| `Design/Core/warden-system.md` | `alc-warden`, `alc-guard`, `alc-economy` | Warden traits/objectives/arbitration, guard duty loop/authority/shifts, money ledger/health, 12-building expansion catalog. Fog of war = inbox of report IDs (no full KnowledgeStore). |
| `Design/Core/ai-agent-systems.md` | `alc-agents`, `alc-world`, `sumo` | 14 decisions, 8-stage validation, 4-layer memory with epistemic buckets, offline deterministic provider, bounded batching/cache/retry/rate limits, personality `Disposition`, runtime telemetry. |
| `Design/Engine/sumo-engine.md` | `sumo`, `alc-world` | `Sim` owns `World`: time, decisions, observation/stats, events, save/load/autosave, invariants. Engine validates; agents never mutate world directly. |
| `Design/Planning/beta-0.05.md` / `../../sumo/SUMO_ROADMAP.md` | implemented / future roadmap | Historical Wave 4 work landed; future engine scope is in the roadmap. |

## Verified implemented (current codebase)

- **`alc-world::World::tick`**: needs drift, hourly tension-aware guard duty sweep (only critical/assistance reports emitted to the bus), schedule resolution, need-driven utility decisions (canonical orchestrator gate + deterministic fallback with trait bias), co-presence encounters, daily boundary (funding/expenses/decay, sentences, escapes, parole, warden arbitration, capacity-gated intake), and weekly gang formation/politics (disputes, leadership changes, splits, mergers, rivalries/alliances, event-sourced history).
- **All 14 `WardenAction` variants** applied; economy sustainability (base grant, de-escalation, lockdown expiry, staff right-sizing).
- **`sumo` engine API**: time control, validated decisions, observation/stats, events, persistence + autosave, invariants, report, trace hash, guard/warden drivers, parole ledger/API, simple rehab relief, engine-side incident/decision/post-release/policy helpers (`record_incident`/`incident_log`, `record_decision`/`decision_log`, `set_post_release`/`post_release_status`, `set_policy`/`set_lockdown`).
- **`alcagent` + `alc-ui` + `alc-gui`**: one binary (`cargo run`): graphical egui observer at the
  Main Menu by default, ratatui debug observer via `--tui`, headless `run` and `docs`. Both
  frontends are rendering/input adapters over `sumo::Sim`, sharing `sumo::graphics` display models.
- **Persistence** (`alc-persistence`): bincode save format v5 with versioned header, atomic writes, JSON debug sidecar, v1/v3/v4 migration, autosave day-dedup and surfaced failures. `Sim` has EngineState converters for engine-side state. Save discovery (`list_saves`/`probe_save_dir`) feeds the GUI save browser with version, status, population and sim time; GUI preferences are separate JSON (`saves/alc-gui/settings.json`).

## Recent integrations (0.05)

- **Historical Wave 4 additions (landed):** engine R1–R7 surface is present (`Sim::map_frame`, `propose_decision`, policy modes, structured incident logs, extended invariants/state digest, driver event wiring); persistence converter edits were applied and the workspace was green at release. See the historical `../Communication/Note's/wave4-implementation-status.md`.
- **Contraband/rehab** are wired into daily world ticks and validated controls with persisted state and both-frontend views (spec 005).
- **Post-release tracking** persists community outcomes and feeds reputation/policy (spec 005).
- **`alc-gui`** is a library egui adapter (no separate binary) rendering the layered campus-style world map from shared `sumo::graphics` models; `alc-ui` is the `--tui` debug adapter. See spec `dev/ralph/specs/002-map-gui-acceptance.md` for the accepted R0 state.

## Remaining work (post-0.05)

1. **Real navigation-graph depth:** the accepted 0.05 map consumes the existing synthetic `MapFrame` graph; true nodes/edges and pathfinding remain future work.
2. **Full `KnowledgeStore` (design §9):** the warden uses delivered-report IDs and budget summaries rather than omniscient state (spec 010); claim/provenance/standing depth remains future work.
3. **AI-generated narrative text and provider-backed model adapters:** deterministic/utility deliberation is authoritative; no provider client is wired.
4. **`World::summary()` / CLI ergonomics:** `alcagent --ui` is not present. `cargo run` opens the
   graphical observer, `--tui` opens the terminal debug observer, and `run <days>` is headless.
5. **Full driver-authority unification:** warden action application has one authoritative world-side applier, but world ticks still run their own autonomous cycles that `Sim::tick` reconciles.

Completed 0.05 scope (retained from the earlier gap list): major UI/GUI overhaul and spatial map,
historical Wave 4 engine support R1–R7, contraband/rehab lifecycle wiring, post-release tracking,
and warden strategy with finance/attention gates and persisted rationale.

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
