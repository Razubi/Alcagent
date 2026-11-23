# sumo Engine API (`Documentation/Systems/engine-api.md`)

**Status (verified 2026-11-21):** this document describes the implemented `sumo` engine API. It also notes planned-but-not-implemented items; do not treat them as available.

Not implemented as public engine/world APIs at this time: `Sim::population_history()`, `Sim::set_rehab_program()`, `Sim::rehab_tick()`, `World::incidents`, `World::decision_log`, `World::programs`, `World::post_release`, `World::under_construction`, `World::contraband_step`, `World::set_policy`/`World::set_lockdown`. See `Documentation/Project/design-status.md` and `Documentation/Reports/beta-0.05-integration.md`.

The simulation engine is `sumo::Sim` (`crates/sumo/src/lib.rs`). All world state lives in `Sim.world: World` (`alc-world`); the engine owns world mutations, decision validation, and event emission. Agents never mutate world state directly (Directive A19).

## Time control

- `Sim::new(seed: u64) -> Sim` — world bootstraps with starting facilities (CellBlock 80 beds, Yard, Kitchen, Medical, Admin, Recreation) and 24 intake prisoners (Directive A2 bootstrap).
- `Sim::set_speed(speed)` / `Sim::speed()` — `TimeSpeed`: `Paused`, `RealTime` (1 min/tick), `FastForward` (5 min/tick), `Extreme` (30 min/tick), `EventDriven` (1 min/tick).
- `Sim::tick()` — one step (no-op while paused).
- `Sim::run_until(target: SimTime) -> u64` — ticks until `target` or pause.
- Expansion API: `advance_by_secs(secs)`, `run_days(days)`, `pause()`, `resume()`, `wait_for_event(after, pred) -> Option<WorldEvent>`.
- `Sim::world()` / `Sim::world_mut()` — direct read/mutate access (facilities, prisoners, gangs, economy, policies, events, guards, warden).
- `Sim::days_simulated()`.

## World state (`alc_world::World`)

Fields: `seed`, `time: SimTime`, `speed`, `next_facility_id` / `next_guard_id` / `next_prisoner_id`, `facilities: Vec<Facility>`, `prisoners: Vec<Prisoner>`, `guards: Vec<Guard>`, `warden: Warden`, `social: SocialGraph`, `gangs: Vec<Gang>`, `economy: Economy`, `policies: Policies`, `events: EventBus`, `history: Vec<WorldEvent>`, `orchestrator: Orchestrator`, `days_simulated`, `current_activity`, `current_decisions`, `prisoner_location`.

`Policies`: `lockdown: bool`, `privileges_level`, `rehab_funding_multiplier`, `security_level` (defaults: false, 0.5, 1.0, 0.6).

`Facility`: `id: FacilityId`, `kind: FacilityKind` (CellBlock, Dormitory, Medical, Kitchen, Workshop, Recreation, Education, Admin, Isolation, Visitor, Staff, Yard), `wing`, `capacity`, `condition: f32` (0..1, degrades with neglect), `staff_required: u32`.

AI deliberation is gated by `world.orchestrator` (per-agent token budget, `Orchestrator::new(4_000)`); the deterministic fallback keeps the sim moving (Directive A19).

## Decision pipeline (Directive A19)

`Sim::deliver_decision(id: PrisonerId, decision: Decision) -> DecisionOutcome`.

`alc_agents::Decision`: `MoveTo { target_facility }`, `Socialize { with }`, `Trade { with, item }`, `Fight { target }`, `Comply`, `Refuse`, `EscapeAttempt`, `RequestMedical`, `JoinGang { gang }`, `LeaveGang`, `LeadGang { gang }`, `SpreadRumor { about, content }`, `None`. Each decision has `action_name()`, `target_id()`, `entity_target()`, `free_text()` (free text is data), `stamina_cost()`, `is_high_stakes()`.

Pipeline: `validate` (`DefaultValidator.validate` + engine rules — lockdown blocks `MoveTo`/`EscapeAttempt`; unknown facilities/gangs/targets rejected) → on reject, `DecisionOutcome.applied == false`; on accept, `apply` → world mutation + `Sim::emit_events`.

`ValidationContext` (supplied by the engine to validators): `current_facility`, `lockdown`, `stamina`, `tokens_remaining_today`, `guards_nearby`, `agent_id`, `role` (`Role::Prisoner` for engine calls), `in_gang`, `gang_id`, `is_gang_leader`, `visible_entities`, `known_facilities`, `has_contraband`, `last_decision`, `now_secs`.

`ValidationResult`: `Accepted(Decision)` / `Repaired(Decision)` / `Rejected { reason }`.

## Observation and stats (Directive A20)

- `Sim::observation() -> Observation` — world snapshot: `at`, `days_simulated`, `population`, `capacity`, `guards`, `facilities`, `cash_major`, `economy_health`, `security_level`, `lockdown`, `warden_name`, `warden_philosophy`, `gangs`, `notable_prisoners: Vec<String>`, `recent_events`, `gang_members`, `leader_board: Vec<(String, f32)>`.
- `Sim::stats_since(at) -> Stats` — from the event log since `at`: `admissions`, `releases`, `conflicts`, `medical`, `security_incidents`, `parole_hearings`, plus `avg_population`, `current_population`/`population`, `capacity`, `gangs_formed`, `gang_recruits`, `max_population`.
- `Sim::agent_observation(id) -> Option<AgentObservation>` — per-agent read-only view: `id`, `name`, `age`, `classification`, `needs`, `condition`, `goals`, `gang_affiliation`, `aggression_modifier`, `social_drive`.
- `Sim::leader_board(n) -> Vec<(String, f32)>` — top `n` by reputation.

## Events

See `Documentation/Systems/event-contract.md`.

## Persistence

- `Sim::save(dir) -> Result<(), SaveError>`, `Sim::load(dir) -> Result<Sim, SaveError>`.
- `Sim::load` restores world state from `alc-persistence`; engine-side runtime state is reset (parole ledger, population history, autosave scheduler, incidents/decision/post-release/policy engine state). Treat world saves as the source of truth unless/until engine-state persistence is added.
- `Sim::enable_autosave(dir, interval_days)`, `Sim::disable_autosave()`, `Sim::maybe_autosave() -> Result<bool, SaveError>` — writes when the autosave interval (in simulated days) has elapsed.

## Invariants and reports

- `Sim::check_invariants() -> Vec<String>` — population ≤ capacity, integral money, unique prisoner/guard/facility ids.
- `Sim::report() -> String` — human-readable summary.

## Guard duty (Directive A10)

- `Sim::guard_duty_cycle() -> Vec<(GuardId, GuardReport)>` — one duty loop per guard (tension-aware context).
- `Sim::run_guard_duty(guard_id, minutes) -> Vec<GuardReport>`.
- `Sim::deliver_guard_report(guard_id, report) -> u64` — maps severity → event kind, sets guard credibility, pushes the event id onto `warden.inbox`; returns the new event id (0 when the report was informational).
- During ticking, `World::tick` runs guard duty **once per simulated hour** (not every tick), using tension-aware observed pressures. Only critical reports (`severity >= 3`) or assistance requests are emitted to the world event bus; routine severity < 3 reports are suppressed from the bus to avoid saturation. See `Documentation/Systems/guard-loop.md`.

## Warden (Directive A9)

- `Sim::warden_cycle() -> Vec<WardenAction>` — `alc_warden::arbitrate` + engine apply.
- `Sim::apply_warden_action(action)` — engine-side application (see `Documentation/Systems/warden-cycle.md`).
- `Sim::warden_pending() -> Vec<WardenAction>`.

## Parole (Directive A15)

- `Sim::parole_due() -> Vec<PrisonerId>` — eligible and not yet recorded in the engine ledger.
- `Sim::start_parole_hearing(id) -> Option<u64>` / `Sim::grant_parole(id) -> Option<u64>` / `Sim::deny_parole(id, reason) -> Option<u64>`.
- `Sim::parole_status(id) -> Option<ParoleStatus>` — engine ledger (`Scheduled` → `InHearing` → `Granted`/`Denied`).
- Social-side board: `alc_social::ParoleBoard::review(&ParoleSubject, behavior, rehab, risk) -> ParoleDecision` (Grant/Defer/Deny) — see `Documentation/Systems/parole-pipeline.md`.

## Rehab (Directive A14)

- `Sim::apply_rehab_progress(id, minutes) -> f32` — deterministic relief per minute (Purpose −0.05m, Respect −0.04m, Stimulation −0.04m, Safety −0.03m, Companionship −0.03m).
- `Sim::rehab_score(id) -> f32` — aggregate of the five satisfied needs.
- See `Documentation/Systems/rehab.md`.

## Engine-side logs and policy state (current Sim helpers)

These APIs exist on `Sim` (engine-side state); they are not full world-persistence contracts unless explicitly saved.

- `Sim::record_incident(event)` / `Sim::incident_log() -> Vec<WorldEvent>` — engine incident log.
- `Sim::record_decision(decision)` / `Sim::decision_log() -> Vec<Decision>` — decision audit log.
- `Sim::set_post_release(prisoner, success) -> Option<bool>` / `Sim::post_release_status(prisoner) -> Option<bool>` — post-release tracking hook.
- `Sim::set_policy(policy)` / `Sim::set_lockdown(lockdown)` — updates engine `PolicyState`, emits `PolicyChange` events when state changes. (World policy fields remain the authoritative simulation policy in `world.policies`; callers should understand which layer they are mutating.)

## Events and determinism

- `Sim::events() -> &EventBus`, `Sim::events_since(at) -> Vec<WorldEvent>`, `Sim::emit_event(e)`.
- `Sim::trace_hash() -> u64` — FNV-1a fingerprint (time, population, recent event summaries) for replay checks (A22). See `Documentation/Systems/determinism.md`.

## Tests (`crates/sumo/tests`)

`engine_api` (11), `decisions` (1), `determinism` (4), `observation` (3: world state, stats window, population), `persistence`, `review_agent`, `soak` (5 passed, 2 ignored).
