# Alcagent 0.05 Game-Side Update Gap Report

- **Date:** 2026-11-23
- **Scope:** Game-side 0.05 update for `alcagent` (playable/observable prison simulation product). 
- **Focus:** `alcagent`, `alc-ui`, `alc-world`, `alc-prisoner`, `alc-social`, `alc-warden`, `alc-guard`, `alc-economy`, `alc-events`. 
- **Sumo included only where necessary:** engine/API gaps that block alcagent game features.
- **Baseline checked:** workspace builds and tests pass; `cargo run -- --no-tui` runs a 30-day headless sim through `sumo::Sim`; default `alcagent` launches the ratatui TUI observer.

## Executive Summary

Alcagent has a working **observer foundation** (CLI + headless run + 11-view ratatui TUI) and a broad set of **simulation primitives** (needs, schedules, intake, relationships, gangs, conflict, escape, parole, warden/economy/guards, event bus). It is **not yet at a 0.05 "Living Institution" game update**.

The main gap is not missing engine scaffolding; it is **game-system wiring, persistence/player-facing controls, deeper simulation lifecycles, and a major UI/GUI overhaul**. The current TUI map is still schematic and `alc-gui` is not a buildable workspace member, so a real game map and the GUI implementation are **0.05 blockers, not deferred polish**. The 0.05 update should prioritize: authoritative game-state mutation/control, auditable incidents/decisions, policy/construction/rehab/contraband/post-release lifecycles, save/load UX, a buildable GUI observer, and a real spatial map with cell/wing activity.

## Current Game-Side Baseline

| Area | Current capability | Key files / symbols | 0.05 readiness |
|---|---|---|---|
| Alcagent CLI/runtime | Clap CLI: global `--seed`, `--speed`, `--no-tui`; commands `observe` (default), `run <days>`, `docs`. TUI uses `EngineDriver: SimDriver`; headless runs `sumo::Sim` for N days and prints summary/recent events. | `crates/alcagent/src/main.rs` (`Cli`, `Cmd`, `EngineDriver`, `launch_tui`, `run_headless`) | Partial - no save/load, no scenario/config, no GUI/TUI feature flags beyond no-tui, no player controls. |
| Observer UI shell | Read-only ratatui app with pause/resume, speed, 1-9 view switch, cursor selection, status bar. `SimDriver` consumes observation/summaries/events/map/stats; no raw `World` mutation. | `crates/alc-ui/src/lib.rs` (`ViewMode`, `UiState`), `crates/alc-ui/src/app.rs` (`SimDriver`, `App`, `run_tui`) | Good base - needs control actions and richer data contracts. |
| Observer views | 11 views: Institution Overview, Prisoner, Gang, Staff, Warden, Financial, Event Timeline, Social Network, Map, Incident Reports, Decision Audit. Prisoner detail includes traits/needs/goals/schedule assignment/skills/memory. Map is still schematic; timeline is event log; financial is ledger. | `crates/alc-ui/src/views.rs` (`ALL_MODES`, `draw`, `render_*`) | Partial - incident/decision/policy/construction/rehab/contraband/post-release views exist, but cell-level game map is still missing. |
| UI/GUI overhaul | Ratatui TUI is the only buildable observer. `alc-gui` exists as data/scaffolding but is not a workspace member and does not build. | `crates/alc-ui/src/*`, `crates/alc-gui/src/*`, `sumo::MapFrame` | Blocker - 0.05 requires a buildable GUI and a major UI overhaul, including a real game map. |
| World simulation core | Deterministic seeded world; bootstrap facilities/guards/24 prisoners; tick handles needs drift, guard duty sweep, schedule resolution, utility decisions with orchestrator gate + deterministic fallback, encounters, daily boundary (funding/expenses/decay/sentences/escapes/parole/warden actions/intake/gang maintenance). | `crates/alc-world/src/lib.rs` (`World`, `bootstrap`, `tick`, `drift_needs_step`, `resolve_schedules_step`, `make_decisions_step`, `resolve_encounters_step`, `on_day_boundary`) | Partial - core loop exists, but many 0.05 lifecycles are missing or shallow. |
| Prisoner model | Identity, traits, condition, 29-need model, goals, schedule assignment, skills, memory summary; intake generation is correlated/deterministic; schedule resolution has 10 attendance outcomes. | `crates/alc-prisoner/src/lib.rs`, `needs.rs`, `schedule.rs`, `intake.rs` | Partial - needs/schedule rich, but player-facing inspection and lifecycle wiring need depth. |
| Social/conflict/escape | Social graph, emergent gangs, conflict escalation/outcomes, escape motivation/consequences, role detection; world wires encounters, weekly gang formation/purge, daily escape attempts. | `alc-social/src/*.rs`, `alc-world/src/lib.rs` (`resolve_encounters_step`, `maybe_form_gang_step`, `resolve_escape_step`) | Partial - key systems exist but not fully integrated into a coherent auditable game loop. |
| Contraband/rehab/parole libraries | `alc-social` has contraband market/discovery, rehab enrollment/progress, parole board review. World does custom parole and intake; contraband/rehab not wired into world tick. | `alc-social/src/contraband.rs`, `rehab.rs`, `parole.rs`; `alc-world/src/lib.rs` (`resolve_parole_step`, `accept_intake_step`) | Missing wiring - major 0.05 gap. |
| Warden/guard/economy | Warden traits/objectives/attention/arbitration, many WardenActions applied; guard duty/authority/shift/report credibility; economy ledger/funding/expenses/health. Warden has emergency overrides and weekly de-escalation/right-sizing. | `alc-warden/src/lib.rs`, `expansion.rs`, `finance.rs`; `alc-guard/src/lib.rs`; `alc-economy/src/lib.rs`; `alc-world/src/lib.rs` (`apply_warden_action`) | Partial - strategic layer exists, but player/control/feedback loops incomplete. |
| Events | Rich `EventKind` enum and bounded `EventBus`; world emits subset. Dead/underused kinds exist (`Death`, `TransferOut`, `ConstructionComplete`, etc.). | `crates/alc-events/src/lib.rs`, `crates/alc-world/src/lib.rs` | Partial - taxonomy ahead of actual game wiring. |

## 0.05 Game-Side Gap Matrix

### A. Player/Observer Product Gaps (Alcagent + UI)

| Gap | Current state | Missing for 0.05 | Primary files | Priority |
|---|---|---|---|---|
| Save/load/autosave UX | Engine has save/load/autosave capabilities in design; alcagent CLI/TUI expose none. | CLI commands/flags for save/load/autosave path/interval; TUI keybind/panel for save/load and autosave status; safe error reporting. | `crates/alcagent/src/main.rs`, `crates/alc-ui/src/app.rs`, `crates/alc-ui/src/views.rs`, necessary sumo driver API | P0 |
| Policy/control panel | TUI is read-only except pause/speed/view/mode. `SimDriver` exposes `policy_mode/set_policy_mode` only. | Player/observer controls for lockdown, security, privileges, rehab funding, intake policy; clear validation feedback and event feedback. | `crates/alc-ui/src/app.rs`, `views.rs`, `crates/alc-world/src/lib.rs` (`Policies`, `apply_warden_action`), sumo control API | P0 |
| Incident reports view | Event timeline exists; no incident aggregation/filter. | Incident Reports view/panel: by kind, severity, participants, location, cause, consequence, disposition; per-prisoner/staff incident drilldown. | `crates/alc-ui/src/views.rs`, `crates/alc-ui/src/lib.rs`, `crates/alc-events/src/lib.rs`, `crates/alc-world/src/lib.rs` | P0 |
| Decision audit view | World keeps only latest `current_decisions`; UI has no decision log. | Decision/audit panel showing proposed/accepted/rejected/repaired decisions, reason, actor, time, policy/council rationale. | `crates/alc-world/src/lib.rs`, `crates/alc-ui/src/views.rs`, sumo decision log/proposal API | P0 |
| Expansion/construction tools | Facilities are added instantly; no build queue/progress UI. | Expansion panel with catalog, cost/time/staff requirements, under-construction status, completion/cancel/failure feedback. | `crates/alc-warden/src/expansion.rs`, `crates/alc-world/src/lib.rs`, `crates/alc-ui/src/views.rs` | P1 |
| Full schedule inspector | Prisoner view shows assignment, not resolved daily timeline. | Daily schedule timeline per prisoner/wing: resolved blocks, cancellations, attendance outcomes, reasons (lockdown, staff shortage, discipline, medical). | `crates/alc-prisoner/src/schedule.rs`, `crates/alc-world/src/lib.rs`, `crates/alc-ui/src/views.rs` | P1 |
| Richer map/cell view | Map is still a schematic facility table + guard posts, not a real game map. | Cell/wing occupancy, prisoner/guard pawn placement, movement/path hints, incident overlays, facility condition/capacity/utilization; consume `sumo::MapFrame` in both TUI and GUI. | `crates/alc-world/src/lib.rs` (`prisoner_location`, `guard_location`), `crates/alc-ui/src/views.rs`, `crates/alc-gui/src/*`, `sumo::MapFrame` | **P0 / 0.05 blocker** |
| Finance dashboard depth | Treasury/health/funding/ledger only. | Budget plan, variance/trend, payroll/maintenance/rehab/contraband costs, health-state explanation, forecast/warnings. | `crates/alc-economy/src/lib.rs`, `crates/alc-warden/src/finance.rs`, `crates/alc-ui/src/views.rs` | P1 |
| Observer UX quality | Search/filter/bookmark/help/notification feed exist but are not fully verified for legibility and view scope. | Fix clipped panels/help/tab bar, unify search scope, stabilize selection across modes, and validate all controls at target terminal sizes. | `crates/alc-ui/src/app.rs`, `views.rs`, `lib.rs` | P1 |
| Major UI/GUI overhaul + real game map | TUI has 11 views but the map is a facility table; `alc-gui` is non-buildable scaffolding. | Buildable GUI observer sharing DTOs and validated controls with the TUI; real spatial map with cell/wing occupancy, pawn positions for prisoners/guards, movement/path hints, incident overlays, facility condition/capacity/utilization, pan/zoom and hover detail. | `crates/alc-ui/src/views.rs`, `crates/alc-gui/src/*`, `crates/sumo/src/map_frame.rs`, `crates/alc-world/src/lib.rs` | **P0 / 0.05 blocker** |

### B. Core Simulation Gaps (Game Systems)

| Gap | Current state | Missing for 0.05 | Primary files | Priority |
|---|---|---|---|---|
| Incident lifecycle | Events emitted; no persistent incident model. | First-class incident records: kind, severity, participants, witnesses, location, time, cause chain, response, outcome, disciplinary/medical follow-up, resolution state. | `crates/alc-world/src/lib.rs`, `crates/alc-events/src/lib.rs`, `crates/alc-social/src/conflict.rs`, `crates/alc-guard/src/lib.rs` | P0 |
| Decision lifecycle | Latest decision map only; immediate apply. | Auditable decision records, validation outcome, repair/rejection reason, source (AI/deterministic/council/player), batch/queue option for player/agent actions. | `crates/alc-world/src/lib.rs` (`current_decisions`, `make_decisions_step`), `crates/alc-agents/*`, sumo decisions API | P0 |
| Policy/regime system | Policies struct exists; limited warden actions; lockdown auto-expires weekly logic. | Explicit policy state machine: lockdown, security level, privileges, visitation, work/education/rehab policy; guardrails; cooldowns; visible consequences. | `crates/alc-world/src/lib.rs`, `crates/alc-warden/src/lib.rs`, `crates/alc-events/src/lib.rs` | P0 |
| Construction lifecycle | `add_facility` instant; `Expand` instant. | `under_construction` queue, build progress, cost/time, material/staff prerequisites, completion/cancellation/failure, `ConstructionComplete` event, facility activation. | `crates/alc-world/src/lib.rs`, `crates/alc-warden/src/expansion.rs`, `alc-economy/src/lib.rs` | P1 |
| Rehab lifecycle | `alc-social::rehab` library exists but not wired. | Program catalog, enrollment eligibility/capacity, attendance/progress, completion/failure/dropout, effects on behavior/parole/needs/reputation, events. | `crates/alc-social/src/rehab.rs`, `crates/alc-world/src/lib.rs`, `crates/alc-prisoner/src/lib.rs`, `crates/alc-events/src/lib.rs` | P1 |
| Contraband lifecycle | `alc-social::contraband` library exists but not wired. | Contraband items/inventory/market, trade, discovery, confiscation, investigations, disciplinary/security impact, events. | `crates/alc-social/src/contraband.rs`, `crates/alc-world/src/lib.rs`, `crates/alc-guard/src/lib.rs`, `crates/alc-events/src/lib.rs` | P1 |
| Post-release lifecycle | Parole/escape remove prisoners; no post-release state. | Post-release records/outcomes for parole/escape/transfer/death where applicable; recidivism/reintegration hooks; history visible in UI. | `crates/alc-world/src/lib.rs`, `crates/alc-social/src/parole.rs`, `crates/alc-events/src/lib.rs`, sumo post-release API | P1 |
| Death/transfer/medical outcomes | `Death`/`TransferOut` event kinds exist but world does not fully use them. | Explicit medical severity, injury/illness progression, hospitalization, death/transfer rules, cause-of-death/transfer records and events. | `crates/alc-world/src/lib.rs`, `crates/alc-prisoner/src/needs.rs`, `crates/alc-events/src/lib.rs` | P1 |
| Warden knowledge/fog-of-war | Warden inbox stores event IDs only. | Knowledge store: direct observations, reports, rumors, confidence, expiry, provenance; decisions based on known info. | `crates/alc-warden/src/lib.rs`, `crates/alc-guard/src/lib.rs`, `crates/alc-world/src/lib.rs`, `crates/alc-agents/src/memory.rs` | P2 |
| Richer social routines | Encounters emit basic argument/rumor/relationship shift. | Structured social activities: meals/recreation/work/education interactions, friendships/rivalries/mentorship, gang recruitment pressure, staff-prisoner interactions. | `crates/alc-world/src/lib.rs`, `crates/alc-social/src/lib.rs`, `crates/alc-prisoner/src/schedule.rs` | P2 |

### C. Data/Contract Gaps Needed by Alcagent

| Gap | Why it matters | Missing contract | Primary files |
|---|---|---|---|
| Stable observer DTOs for 0.05 | UI currently renders from summaries/observation; new features need consistent data. | DTOs for incidents, decisions, schedules, construction jobs, rehab enrollments, contraband cases, post-release records, policy state, budget snapshots. | `crates/alc-ui/src/app.rs`, `crates/sumo/src/lib.rs` (summary types), view models |
| Command/control API | Alcagent can only tick/pause/speed/mode; cannot drive game controls. | Validated commands: set policy/lockdown, order expansion, enroll rehab, transfer/discipline where appropriate, save/load, inspect detail by ID. | `crates/alc-ui/src/app.rs` `SimDriver`, `crates/alcagent/src/main.rs`, sumo control methods |
| Event semantics | Many event kinds exist but not emitted consistently; UI cannot rely on them. | Event emission matrix: when each `EventKind` is emitted, payload/summary conventions, causal links/confidence. | `crates/alc-events/src/lib.rs`, `crates/alc-world/src/lib.rs`, `crates/sumo/src/incidents.rs` |
| Persistence migration for 0.05 state | New game state will break save/load unless planned. | Save format/version bump, migration for incidents/decisions/policy/construction/rehab/contraband/post-release, deterministic roundtrip tests. | `crates/alc-persistence/*`, sumo save/load, world state structs |

## Necessary Sumo Support (Only What Alcagent Needs)

These are engine-side changes needed because alcagent is driven through `sumo::Sim`/`SimDriver`, not because sumo should own game design.

| Sumo support needed | Why alcagent needs it | Minimal expectation | Evidence |
|---|---|---|---|
| Single authoritative tick/control path | Avoid double-driving world vs sumo drivers; controls must behave consistently. | Documented/implemented authoritative path for guard/warden/policy/construction/rehab/contraband updates; no conflicting duplicate logic. | `crates/alcagent/src/main.rs` uses `sumo::Sim`; `design-status.md` notes duplicated driver authority. |
| Control mutation API for SimDriver | TUI/CLI need to apply player/observer actions safely. | Add validated methods for save/load, set policy/lockdown, expansion/construction commands, rehab enrollment, maybe manual incident/decision inspection queries. | `crates/alc-ui/src/app.rs` `SimDriver`; sumo has limited control surface. |
| Rich read models | 9-view UI needs more than basic summaries. | Stable DTOs/accessors for incidents, decision audit, schedule timeline, construction queue, rehab enrollments, contraband cases, post-release, budget/health snapshots, population history. | `crates/sumo/src/lib.rs`, `observation.rs`, `map_frame.rs` |
| Persistence/converter completeness | 0.05 state must save/load reliably. | Complete EngineState/world conversion, migrations, and roundtrip tests for new 0.05 fields. | `design-status.md` Wave 4/persistence notes, `crates/alc-persistence/*` |
| Deterministic command validation | Player/agent commands must not corrupt state. | All new controls routed through validated engine commands with clear rejection reasons and events/audit records. | `crates/sumo/src/decisions.rs`, `drivers.rs`, `lib.rs` |

## Recommended 0.05 Implementation Slices (Game-Side First)

### Slice 0 - Contract & Safety (P0 prerequisite)
- **Goal:** Make alcagent able to observe and control 0.05 features safely.
- **Deliver:** SimDriver/control DTO expansion; save/load CLI+TUI hooks; policy/lockdown controls; authoritative tick/control path decision.
- **Acceptance:** Save/load roundtrip from alcagent; policy change reflected in observation/events; no duplicate driver behavior.

### Slice 1 - Auditable Prison Life (P0)
- **Goal:** Make events understandable and inspectable.
- **Deliver:** Incident records + Incident view; decision audit log + Decision view; event causality/confidence conventions; per-prisoner/staff history drilldown.
- **Acceptance:** Player can explain major incidents and decisions from UI, not logs only.

### Slice 2 - Institutional Control & Construction (P1)
- **Goal:** Give the prison meaningful strategic management.
- **Deliver:** Policy/regime controls; construction queue/lifecycle; expansion tools UI; budget/finance depth; staff assignment impacts.
- **Acceptance:** Warden/player actions have delayed costs, visible progress, and consequences.

### Slice 3 - Rehab, Contraband, Post-Release (P1)
- **Goal:** Complete the "living institution" social/economic loop.
- **Deliver:** Rehab enrollment/progress/outcomes; contraband market/discovery/discipline; post-release records and recidivism/reintegration hooks.
- **Acceptance:** Prisoners can move through credible arcs: intake -> behavior -> rehab/discipline -> parole/post-release.

### Slice 4 - Major UI/GUI Overhaul + Spatial & Social Depth (0.05 blocker)
- **Goal:** Make the world feel physically and socially alive and observable in both supported frontends.
- **Deliver:** Buildable `alc-gui` workspace member; shared DTO/control contract with the TUI; real game map using `sumo::MapFrame` with pawn positions, cell/wing rosters, movement/path hints, incident overlays and facility condition; fixed clipping/help/search/selection UX; schedule timeline, richer encounters/routines and knowledge/fog-of-war.
- **Acceptance:** Observer can follow individuals and groups through place, time, relationships, and institutional response in the TUI and in the GUI, using the same validated controls and map data.

## Risks and Dependencies

| Risk | Impact | Mitigation |
|---|---|---|
| Driver/tick authority split | Controls/features behave inconsistently or duplicate. | Define one authoritative simulation/control path before adding 0.05 controls. |
| UI grows faster than data contracts | Fragile views, direct world access, regressions. | Add summary/DTO contracts first; keep `SimDriver` observation-driven. |
| New state without migration | Save/load breaks across 0.05. | Versioned persistence and roundtrip tests for every new state block. |
| Event richness without structure | Timeline noisy, hard to debug. | Incident/decision records with causal links; event taxonomy discipline. |
| Feature breadth overload | Half-wired systems. | Ship by slices with acceptance tests and soak runs per slice. |

## Suggested Acceptance Criteria for "Alcagent 0.05"

Alcagent can be considered 0.05-ready (game-side) when:

1. **Control:** Player/observer can save/load, pause/speed, inspect, and apply validated policy/construction/rehab controls from CLI/TUI.
2. **Auditability:** Major incidents and decisions are visible with cause, actors, validation, and outcome.
3. **Lifecycle depth:** Construction, rehab, contraband, parole/post-release have end-to-end state transitions and events.
4. **Observer quality:** The 11-view TUI exposes schedules, incidents, decisions, finance depth, and meaningful map/cell activity without clipped critical panels.
5. **UI/GUI overhaul and real game map:** 0.05 requires a buildable GUI observer plus a major UI overhaul. The map must be a real spatial view with cell/wing occupancy, prisoner/guard pawn positions, movement/path hints, incident overlays, facility condition/capacity/utilization, pan/zoom and hover detail, driven from `sumo::MapFrame` and shared DTOs—not a facility table or schematic placeholder.
6. **Stability:** Deterministic tests, save/load roundtrip, and long-run soaks pass with new 0.05 systems enabled.