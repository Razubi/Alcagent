# Guard Duty Loop (`../Systems/guard-loop.md`)

Guards are autonomous agents (`alc_guard::Guard`, `crates/alc-guard/src/lib.rs`). Their duty cycle is `Guard::duty_loop` / `Guard::duty_tick`, driven during `World::tick` (`g.duty_tick(&observed, guard_policy, &mut rng)`) or called manually via `Sim::guard_duty_cycle()` / `Sim::run_guard_duty()`.

## DutyContext

`DutyContext { at: SimTime, lockdown: bool, security_level: f32 }` — the minimal policy snapshot guards decide with.

## Guard

Fields: `id: GuardId`, `name: String`, `rank: GuardRank` (Officer/Sergeant/Lieutenant/Captain — authority caps L2/L3/L4/L5), `personality` (0 rigid by-the-book, 1 lenient), `competence`, `experience`, `morale`, `fatigue`, `stress`, `loyalty`, `integrity`, `post: PostType`, `current_task: String`, `shift: u8`, `history: Vec<String>`, `authority: AuthorityLevel` (L0–L5, normally `rank.authority_cap()`), `rotation: ShiftRotation` (`current_shift` 0–2 by convention, `ticks_on_shift`, `rotation_period_ticks = 480`, `consecutive_shifts`; `advance()`/`rest()`), `report_credibility: f32`.

`PostType`: `Tower, Wing, Corridor, Yard, Reception, Medical, Work, Visitor, Admin` — with `is_24h()`, `min_staffing()`, `risk_profile()` (Yard 0.9, Wing 0.7, Reception 0.6, Work 0.5, Visitor 0.5, Corridor 0.4, Medical 0.4, Tower 0.3, Admin 0.2) and `coverage_hours()` ("program-hours", "visitation-hours", "24/7", "scheduled").

## Duty step (`Guard::duty_tick`)

1. Compute `incident_load` from the observed world view: `avg_tension * 0.6 + peak_tension * 0.4 + post.risk_profile() * 0.15 + security_level * 0.1 + lockdown * 0.08`.
2. `tick(at, incident_load)` — rotation advance; `fatigue += 0.008 + risk_profile * 0.004`; `stress += incident_load * 0.05`; morale decay when fatigue > 0.8; shift-rotation experience bump; consecutive-shifts stress penalty; refresh `report_credibility`.
3. `observe(at, incident_load)` — salience = incident_load + risk_profile * 0.1 + stress * 0.05; notable when > 0.35 → observation note.
4. `notable` when an observation, or `peak_tension > 0.55`, or `incident_load > 0.45`.
   - severity: 3 when `peak_tension > 0.85 || incident_load > 0.85`; 2 when `> 0.65 || > 0.6`; else 1.
   - `witnessed_directly = rng.chance(0.55 + competence * 0.35)`; `report(...)` produces the `GuardReport`.
   - `enforce(...)` when `incident_load > 0.6 || peak_tension > 0.7` (requires authority ≥ L3 for high-risk posts, else L2).
   - assistance: `stress_high = stress > 0.75 || (stress > 0.6 && incident_load > 0.55)`; `overwhelmed = peak_tension > 0.9 || (incident_load > 0.8 && authority < L3)`; if either → `request_assistance(...)` — report with `requests_assistance = true`, severity ≥ 3.
   - non-notable but `stress > 0.85` → `request_assistance("guard stress critical; relief requested")`.

`GuardReport { at, post, summary, severity (0..3), credibility, witnessed_directly, requests_assistance }`.

Credibility: `0.45 * competence + 0.20 * integrity + 0.15 * experience + 0.10 * witness + 0.10 * morale − min(stress, 0.25) − fatigue * 0.10`, clamped [0.05, 0.99].

## Manual duty loop (`Guard::duty_loop`)

observe → report (severity 1/2/3 by incident_load) → enforce (load > 0.6) → request_assistance (load > 0.85).

## World::tick integration

- Guard duty runs **once per simulated hour** (throttled by `World.last_guard_duty_hour`), not every tick. This prevents routine reports from flooding the bounded event bus.
- Each duty sweep: guards receive tension-aware observations derived from prisoner need pressure/respect/safety/compliance; `duty_tick` runs with `rng = RngStream::new(world.seed, time ^ guard_id << 8 ^ 0xA11CE)`.
- Report handling: `security_incidents_this_week`/`incidents_this_week` are incremented for reports with `severity >= 2`. **Bus emission is throttled:** only reports with `requests_assistance || severity >= 3` are emitted as `EventKind::SecurityResponse`; routine reports below that threshold are not emitted to the event bus. Emitted event IDs are pushed to `warden.inbox` (capped at 200).
- `Guard` includes an escalation cooldown (`last_escalation_hour`) to suppress repeated assistance escalation spam.

## Manual delivery (`Sim::deliver_guard_report`)

`Sim::deliver_guard_report` is a separate manual/engine path: severity 0 returns 0; severity 1 -> `Argument`; 2 -> `Threat`; >=3 -> `Investigation`. It sets guard credibility and pushes the event ID to `warden.inbox`.

## Delivering reports to the warden (`Sim::deliver_guard_report`)

- severity 0 → return 0 (informational, no event).
- severity 1 → `EventKind::Argument`; 2 → `EventKind::Threat`; ≥3 → `EventKind::Investigation`.
- Sets `g.report_credibility = report.credibility.clamp(0.05, 0.99)`; pushes the event id to `warden.inbox`; returns the new event id.
