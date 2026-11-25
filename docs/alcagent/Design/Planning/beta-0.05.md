# Sumo Beta 0.05 — Update List (Deep-Planning Draft)

- **Target version:** `sumo` **0.5.0-beta.1** (semver prerelease of `0.5.0`; iteration path `0.5.0-beta.2` → … → `0.5.0`)
- **Release theme:** "The Living Institution"
- **Sole focus:** the `sumo` engine crate plus its immediate observation surface (`alc-gui`), and the minimal engine fixes required to ship it.
- **Planning phase:** Phase 2 of the AGENTS.md autonomous workflow — deep-planning deliverable. No implementation coding in this phase.

---

## 1. Headline feature — GUI (`alc-gui`)

### 1.1 Decision
Add a **desktop GUI** for the sumo engine as the headline of beta 0.05. The existing observer (`alc-ui`) is a read-only ratatui TUI over `&World`; it has no GUI counterpart.

GUI framework decision (from UI research, verified): **egui `0.36.2` + eframe `0.36.2`**.
- egui 0.36.2 is present in the local cargo registry cache; MSRV Rust ≥ 1.95, toolchain is 1.100.0 → compatible (workspace `rust-version = 1.85` stays as a floor).
- winit 0.30.13 and raw-window-handle 0.6.2 already cached (eframe's native backend chain partially cached; `eframe`, `egui_winit`, `egui_glow` fetch on first build; network available).
- Alternatives rejected: Bevy + bevy_egui (heavy), hand-rolled winit (more code).

### 1.2 Crate / workspace integration
- New workspace member: `crates/alc-gui`.
- Dependencies: `egui`, `eframe`, `sumo`, `alc-ui`, `alc-world`, `alc-core`, `alc-social`, `alc-economy`, `alc-warden`, `alc-guard`, `alc-prisoner`, `serde`.
- New leaf crate → no dependency cycles (`alc-gui -> sumo -> alc-ui` direction preserved; `alc-ui -> sumo` remains forbidden by design).

### 1.3 Architecture
- `App` (implements `eframe::App`) owns a `sumo::Sim`.
- `update()`:
  - `sim.tick()` when not paused.
  - `ctx.request_repaint_every(Duration::from_millis(~16))` for ~60 Hz polling.
- HUD: `sim.observation()`.
- Per-mode data: `sim.world()`.
- Playback: `TimeSpeed` (Paused / RealTime / FastForward) via `Sim::speed()` / `Sim::set_speed()`.
- Control windows: warden objectives (`Warden::objective_utility`), speed slider, save/load (`Sim::save` / `Sim::load`).

### 1.4 Screens — all 9 `alc_ui::ViewMode`s ported to egui panels
1. Institution Overview
2. Prisoner
3. Gang
4. Staff
5. Warden
6. Financial
7. Event Timeline
8. Social Network
9. Prison Map

### 1.5 Beta panels
| # | Panel | Data source |
|---|---|---|
| 1 | Event Timeline | `Sim::events_since(at)` |
| 2 | Incident Reports | aggregate typed incidents (Fights / Arguments / Threats / GangClashes / Escapes / Contraband / Misconduct / Security) |
| 3 | Policy Management | buttons → security / lockdown / privileges / rehab funding → `EventKind::PolicyChange` via `Sim::apply_warden_action` |
| 4 | Expansion Tools | buttons per `alc_warden::building_catalog()` → `WardenAction::Expand` |
| 5 | Daily Schedule | `World::current_activity` per prisoner/tick; unresolved templates via `resolve_schedule` |
| 6 | Guard Roster | guards × posts / shift / fatigue / stress / history |
| 7 | Stats Dashboard | `Stats` (13 fields) + `leader_board(10)`; population-over-time via proposed `Sim::population_history()` |
| 8 | Social Network | `SocialGraph` + `alc_social::detect_roles` |
| 9 | Map | schematic with pan/zoom; port of TUI `render_map`; hover tooltips |

### 1.6 Phase plan
- **Phase 1:** App + HUD + Institution Overview + Warden control windows.
- **Phase 2:** remaining panels (timeline, incidents, policy, expansion, schedule, roster, stats, social, map).
- **Phase 3:** map pan/zoom, population graph, stats windows, save/load UI.

## 2. Required features (beta core)

### 2.1 R1 — World-level incident log + "Incident Reports" view
- `World::incidents` store; capture in `make_decisions_step` + `resolve_encounters_step`.
- Per-prisoner queryability for disciplinary review.
- `Stats` additions: `incidents_by_kind`, `injuries`.
- UI: Incident Reports panel on Prisoner view.

### 2.2 R2 — Agent decision audit log
- `World::decision_log: Vec<DecisionRecord>` + `World::record_decision`.
- `Sim::decision_log()` accessor.
- Migrate `World::current_decisions` (overwritten each tick) into the persistent log.

### 2.3 R3 — Rehab program effects wired into the world
- `Sim::set_rehab_program(&mut self, id, program) -> bool` (via `alc_social::rehab::enroll_in_program`).
- World-side program state (`World::programs`) + daily progress step (`Sim::rehab_tick(&mut self, minutes)`).
- Completion → goal progress + events (`ProgramEnrolled`, `RehabilitationSuccess`).

### 2.4 R4 — Post-release tracking
- `World::post_release: HashMap<PrisonerId, PostReleaseState>`.
- Wire previously-dead `Death`, `TransferOut` event kinds.

### 2.5 R5 — Decision batching queue
- Engine decision pipeline: batch validate/apply (hybrid deterministic + AI decision architecture, AGENTS.md §19).

## 3. High-value features (H1–H8)

- **H1 — Policy management / regime changes:** `World::set_policy`, `World::set_lockdown`, `WardenAction::SetLockdown`; Sim apply + emit `PolicyChange`; guard impossible combinations.
- **H2 — Expansion tools GUI + construction lifecycle:** `World::under_construction`; construction steps; emit `ConstructionComplete`; Warden-panel buttons.
- **H3 — Stats gaps:** deaths, transfers, guard injuries, contraband discoveries, policy changes, energy/rest metrics; wire dead event kinds (`Death`, `ConstructionComplete`, `GangDisbanded`, `FamilyCorrespondence`, `RehabilitationSuccess`); extend `Stats::from_events`.
- **H4 — Contraband in the world:** world market state + daily generation via `alc_social::contraband_market` / `contraband_discovery`; detection → `ContrabandFound` events; freedom-need escalation.
- **H5 — Warden KnowledgeStore:** direct knowledge + rumors + standing/expiry with provenance; warden weighs it.
- **H6 — Full daily schedule display:** `World::schedules`, `resolve_schedules_step`, Prisoner schedule timeline.
- **H7 — Map / cells view:** prisoner placement on map + per-wing cell rosters (`World::prisoner_location`).
- **H8 — Finance dashboard enhancements:** wire `AnnualBudgetPlan` / `FinanceHealthState` / `BudgetLine`; budget plan to world; variance/trend lines; guard payroll; health states.

## 4. Nice-to-have (N1–N6)

- **N1 — Text renderers + `alcagent --ui` flag:** `Renderer` trait implementations; design-status gaps #2, #9.
- **N2 — `World::summary()`.**
- **N3 — Full guard duty sweep in `sumo::guard_duty_cycle`** (unify stubbed `Guard::duty_loop` with richer `duty_tick`; fill 5 stub markers in `alc-guard`).
- **N4 — Guard reliability dashboard.**
- **N5 — Decision-log viewer.**
- **N6 — Superseded-doc cleanup** (`architecture-report.md (removed)`).

## 5. Proposed beta 0.05 ship list (priority order)

1. **Must-have (headline + required):** GUI (`alc-gui`, Phase 1–3 panels) · R1 incident log · R2 decision audit log · R3 rehab wiring · R4 post-release tracking · R5 decision batching.
2. **Small, user-visible, ship now:** R2, R1, H1, H6, H7.
3. **Core systems (ship if capacity allows):** R3, R4, H2, H4.
4. **High-value stretch:** H3, H5, H8.

Exec order (Features agent): R4/R5 → R1 → R2 → R3 → H1–H3 → H4–H6 → N1–N5.

## 6. Engine fixes shipping in beta 0.05 (audit / unblock phase)

- `alc-social` dependency cycle fixed.
- `EventKind`: added `SecurityResponse`, `GuardReport` (removed enum-tail duplicates).
- `EventBus::new(50_000)` (was 5_000 — admission-stats history eviction).
- `AgentObservation` restored.
- Workspace `cargo build` + `cargo test` green; `cargo build -p sumo` green; all sumo tests passing.

## 7. Risks / open design questions (beta-relevant)

- **GUI:** eframe fetch on first build; headless CI (keep TUI path); MSRV interplay (egui ≥1.95 floor); control panel mutates world — engine is read-oriented (AGENTS §19), treat as feature addition.
- **Engine:** catch-all `_ => ok()` in decision acceptance; parole ledger not persisted on `Sim::load`; `ParoleDenied` loses reason; escape-path divergence (world tick removes escaped; engine path does not); `MoveTo` sets `schedule.wing` approximation; `Trade` effect-free; `stats_since` population history only fills on tick.
- **Ops:** `Sim::wait_for_event` unbounded-loop footgun; `full_365_day_run_no_invariant_violations` 30 s wall-clock assert (timing flake risk); invariant coverage gaps (decisions/determinism/engine_api/observation/persistence tests).
- **Social/AI:** double-driving guards (`World::tick` vs `Sim::guard_duty_cycle`); guard-report 3-mappings inconsistency; doc drift (`../sumo/docs/../sumo/../Reports/sumo-audit.md` missing expansion APIs).

## 8. AGENTS.md phase mapping for beta 0.05

| Phase | Status | Beta-0.05 output |
|---|---|---|
| 1 — Investigation | DONE | `../sumo/docs/../sumo/../Reports/sumo-audit.md`, `../Reports/audit-report.md`, `../sumo/../Design/Engine/sumo-engine.md`, `../sumo/../Design/Engine/sumo-expansion.md`; build/test green |
| 2 — Design | IN PROGRESS | This update list / deep plan |
| 3 — Implementation | PLANNED | 5-subagent team, disjoint file ownership, engine decision validation |
| 4 — Testing | PLANNED | new test wave (drivers, parole, expansion_api), invariants, soak/stress |
| 5 — Autonomous Review | PLANNED | bug-hunt pass on beta feature set |

Dev/research prereqs (per AGENTS.md): autonomous living-prison simulation directive — investigation before rewrite, incremental implementation, documented interfaces, deterministic seeded RNG, tested, modular.

## 9. Next steps

1. Phase 2 (design): finalize this update list → promote to `../sumo/../Design/Engine/sumo-expansion.md` cross-refs; DevOps: version bump `0.5.0-beta.1`, root `CHANGELOG.md`, doc status flips, tests wave.
2. Phase 3 (implementation): re-launch the 5-subagent developer team with disjoint file ownership (GUI / interactions / capabilities-social-rehab / testing / integration-docs).
3. Phase 4 (testing): build + test + soak.
4. Phase 5 (autonomous review).
