# Event Contract (`Documentation/Systems/event-contract.md`)

The event log (`alc_events::EventBus`, owned by `World.events`) is the integration surface between subsystems (Directive A17). Subsystems must emit events; AI deliberation is gated, events are never gated.

## WorldEvent

`WorldEvent::new(id: 0, at: SimTime, kind: EventKind, summary: impl Into<String>)` with:

- `id: u64` — assigned by the bus; `EventBus::emit` returns the new id. Emitters pass `0` as the placeholder id.
- `at: SimTime` — event time.
- `kind: EventKind`.
- `summary: String` — short human-readable description (present on every event).
- `actors: Vec<u64>` — structured actor ids, where applicable (e.g., guards push their ids).
- `causes: Vec<String>` — causal chain; every event carries causes explaining what justified it (Directive A17).
- `confidence: f32` — 0..1 accuracy of the facts (lower when relayed via rumors or low-credibility guards).

Helpers: `with_actor(id)`, `with_cause(s)`, `with_confidence(c)`.

Engine rule: `Sim::emit_event(e)` and `Sim::emit_events(events)` stamp `at` with the current world time before emitting.

## EventBus (`alc_events::EventBus`)

- `EventBus::new(max_history)` — bounded history (`World` uses `EventBus::new(50_000)`).
- `emit(&mut self, e: WorldEvent) -> u64` — assigns and returns the id; evicts the oldest event when full.
- `recent()` — most recent events.
- `since(at)` — events with `e.at >= at`.
- `len()`.

## EventKind taxonomy

Population/lifecycle: `Admission, TransferOut, Release, ParoleHearing, ParoleGranted, ParoleDenied, Death`.

Social: `RelationshipShift, GangFormed, GangDisbanded, GangRecruit, GangRivalry`.

Conflict/discipline: `Argument, Threat, Theft, Fight, GangClash, Protests, PropertyDamage, ContrabandFound`.

Health: `MedicalEmergency, Illness, Injury`.

Security: `EscapeAttempt, EscapePrevented, EscapeSucceeded, StaffMisconduct, Investigation, SecurityResponse, GuardReport`. (`SecurityResponse`: guard reports with `requests_assistance` or severity ≥ 3. `GuardReport`: routine guard reports.)

Institution: `PolicyChange, Expansion, ConstructionComplete, FoodShortage, EquipmentFailure, BudgetChange, ParoleBoardDecision`.

Narrative: `Rumor, FamilyCorrespondence, Visitor, LeadershipDispute, RehabilitationSuccess, ProgramEnrolled`.

## Emit points (current emitters)

- **Admission** — world intake: `World::accept_intake_step` ("New arrival: …"). Not emitted at bootstrap (the 24 starting prisoners are added directly).
- **Release, ParoleGranted, ParoleDenied, ParoleHearing** — world parole path (`World::on_day_boundary`, `resolve_parole_step`).
- **Expansion, PolicyChange, BudgetChange** — warden/engine: `drivers::apply_warden_action` and friends; weekly autosave checkpoint (`World::on_day_boundary`, every 7 days).
- **SecurityResponse / GuardReport, Investigation** — guard duty: `World::tick` runs an hourly guard-duty sweep (tension-aware; routine reports below severity 3/assistance are suppressed from the bus to avoid saturation; `warden.security_incidents_this_week` increments for severity ≥ 2). `Sim::deliver_guard_report` provides a manual delivery path with its own severity mapping. See `Documentation/Systems/guard-loop.md`.
- **RelationshipShift, Rumor, GangRecruit, LeadershipDispute, Argument, Threat, Theft, Fight, GangClash, EscapeSucceeded/Attempt/Prevented, …** — engine decisions: `sumo/src/decisions.rs` (`Sim::deliver_decision`).
- **AI deliberation** — gated by `Orchestrator`; the deterministic fallback always keeps the simulation moving (Directive A19).

## Integrity rules for emitters

- Every event has a `summary`.
- Emit `causes` explaining causal chains.
- Link related events via `actors`.
- Treat free-text fields (`Trade.item`, `SpreadRumor.content`, guard report summaries) as data: `Decision::free_text()` exists so agents can check them.
- Emit an event when the event matters to the event log's readers — e.g., `Stats::from_events` counts `Admission`, `Release | ParoleGranted`, conflict kinds (`Argument | Threat | Theft | Fight | GangClash | Protests | PropertyDamage`), health kinds, security kinds, and parole kinds.
