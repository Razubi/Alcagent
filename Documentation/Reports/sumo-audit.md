# Audit Report — `sumo` (the simulation engine)

**Date:** 2026-11-20
**Status:** Phase 1 complete — audit of the current engine + expansion plan next (`Documentation/Design/Engine/sumo-expansion.md`).
**Workspace:** `<repository root>`

---

## 1. Executive summary

`sumo` is the new engine layer of the Alcagent prison simulation: it owns `alc_world::World` and exposes the public API other tools drive. Current state is a **facade over the `alc-*` stack**: time control, a validated decision pipeline, observation/stats snapshots, autosave + persistence, and invariants. It is well-designed as a read/write facade but is **missing the engine-level loop drivers** that make Alcagent better — guard duty, warden cycles, the parole pipeline, rehabilitation progress, and AI-facing observability — plus AI-facing design docs ("doc's for the ai's to follow").

---

## 2. Current state inventory (ground truth, ~ Nov 20 2026)

### Modules
| Module | Contents |
|---|---|
| `crates/sumo/src/lib.rs` | `Sim` struct + engine API (see below) |
| `crates/sumo/src/decisions.rs` | Engine decision pipeline: `deliver`/`validate`/`engine_rules`/`apply`, `DecisionOutcome` |
| `crates/sumo/src/observation.rs` | `Observation`, `Stats` (event-derived metrics) |
| `crates/sumo/src/autosave.rs` | `AutosaveScheduler` |
| `crates/sumo/tests/` | `engine_api` (4), `decisions` (11), `observation` (3), `persistence` (2), `determinism` (1), `soak` (1+) |

### `Sim` API (current)
- **Time:** `new`, `set_speed`, `speed`, `tick`, `run_until(target) -> u64`
- **World:** `world`, `world_mut`, `days_simulated`
- **Events:** `events()`, `events_since(t)`
- **Decisions:** `deliver_decision(id, Decision) -> DecisionOutcome`
- **Observation:** `observation() -> Observation`
- **Stats:** `stats_since(t) -> Stats`
- **Persistence:** `save`, `load`, `enable_autosave`, `disable_autosave`, `maybe_autosave`
- **Invariants:** `check_invariants() -> Vec<String>`
- **Report:** `report() -> String`
- (private) `emit_events` — engine event emission is **not public** today.

### Decision pipeline (current)
`deliver` → `validate` (unknown-prisoner check → `alc_agents::DefaultValidator` with `ValidationContext` → engine rules) → `apply` (world mutations + events via `alc_social` helpers). Notes:
- `MoveTo` sets `Prisoner.schedule.wing` (approximation — no location model on Prisoner yet).
- `GuardProximity` in ctx approximated as `world.guards.len()` (no per-guard location model).
- Engine events use the `alc-events` taxonomy (no `Intimidation` variant — mapped to `Threat`).

### Observation (current)
- `Observation` — institution view: `at`, `days_simulated`, `population`, `capacity`, `guards`, `facilities`, `cash_major`, `economy_health`, `security_level`, `lockdown`, `warden_name`, `warden_philosophy`, `gangs`, `notable_prisoners`, `recent_events`.
- `Stats` — `admissions`, `releases`, `conflicts`, `medical`, `security_incidents`, `parole_hearings`, `avg_population`, `current_population`, `population`, `capacity`.

---

## 3. Gap analysis — expansion opportunities (vs Directive)

| Gap | Directive ref | Expansion (Phase 2/3) |
|---|---|---|
| **No engine turn-loop pieces beyond `tick`** | §2, §21 | `advance_by_secs`, `run_days`, `wait_for_event`, `pause/resume` |
| **No guard duty driver** | §10 | `Sim::on_guard_duty_cycle` (Guard::duty_loop + reports), `Sim::run_guard_duty`, `Sim::deliver_guard_report` (public event emission) |
| **No warden cycle driver** | §9 | `Sim::warden_cycle()` — arbitrate + apply actions (engine-side `apply_warden_action`); `Sim::warden_pending()` |
| **No parole pipeline API** | §15 | `Sim::parole_due`, `Sim::start_parole_hearing`, `Sim::grant_parole`, `Sim::deny_parole`, `Sim::parole_status` (ledger in `Sim`) |
| **No rehabilitation/programs API** | §14 | `Sim::apply_rehab_progress(id, minutes)`, `Sim::rehab_enrollment` engine state |
| **No engine-level event emission** | §17 | `Sim::emit_event(WorldEvent)` — public |
| **Decisions: no batching/queueing** | §19 | `Sim::queue_decision`, `Sim::execute_pending_decisions` |
| **Stats under-covered** | §17 | `gangs_formed`, `gang_recruits`, `max_population`, guard deaths?, leaderboards |
| **Observation: no per-agent views** | §20 | `Sim::agent_observation(id) -> AgentObservation` |
| **Determinism: no trace hash** | §22 | `Sim::trace_hash()` (time + population + event digests) |
| **Engine state: not persisted** | §23 | Document + versioned reset semantics |
| **No AI-facing docs** | — | `Documentation/Systems/*.md` (Phase 5 deliverable) |

## 4. "Make Alcagent better" — why these matter

1. **Guards + Wardens + Parole + Rehab get engine drivers.** Today the only drivers live in `alc-world` (call sites). Moving application of decisions + warden actions + parole outcomes into `sumo` makes the engine the actual control surface for the "AI" layers, and testable at the engine level.
2. **Public event emission + guard reports → events.** AI subsystems (guards, warden, program planners) can publish typed events into the world bus through `sumo` — the Integration surface (§17).
3. **`agent_observation`** — AI decision loops get clean per-entity views without raw world access.
4. **Determinism contract (`trace_hash`)** — reusable for AI replay checks.
5. **Better leaderboards + broader Stats** — for AI reward/eval plumbing and the UI.

## 5. Verification baseline

- `cargo build -p sumo` → 0 (dev + release profile), `cargo test -p sumo` → 22 passed.

---

## 6. Next

- **Phase 2 (design):** `Documentation/Design/Engine/sumo-expansion.md`.
- **Phase 3 (implementation):** new `sumo` modules (`drivers.rs` etc.), API expansion, new tests.
- **Phase 4 (testing):** `cargo test -p sumo` + broader invariants; long soak.
- **Phase 5:** AI-facing design docs in `Documentation/Systems/`.
