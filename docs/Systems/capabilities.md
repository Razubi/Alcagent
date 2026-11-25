# Capabilities (`docs/Systems/capabilities.md`)

Design note for the W3 backlog item: capabilities, contraband and rehabilitation
engine integration. New public API documented fully in [`engine-api.md`](engine-api.md).

**Status (beta-0.05, 2026-11-20):** `alc-core::capability` (catalog + delta table) and
`alc-social::{contraband, rehab}` library modules are implemented and tested.
World/engine wiring described below (`World::contraband_step`, `Prisoner::contraband_hidden`,
`Prisoner::capabilities`, `World::programs`, `Sim::set_rehab_program`) is **planned, not yet
wired** — treat those as target contracts, not current API.

## Capability system (`alc_core`)

```rust
pub struct Capability {
    pub name: String,        // canonical, derived from `kind`
    pub kind: CapabilityKind,
}

pub enum CapabilityKind {
    Physical(Physical),      // Strong, Fit, Fast, Durable
    Cellular(Cellular),      // CellEscape, Dig, HideContraband, HoldHideout, FakeId
    Practical(Practical),    // Trade, PickLocks, Repair, Smuggle, FitLocker, ScoreKey
    Social(Social),          // Persuade, Charm, Intimidate, Inform, Influence, Leadership
    Medical(Medical),        // FirstAid, Evaluation
}

pub struct CapabilityDeltas {
    aggression, risk_tolerance, sociability, manipulation, ambition, loyalty,
    empathy, discipline, intelligence, emotional_stability, impulsiveness, risk,
}

impl Capability {
    pub fn modifiers(&self, traits: &Traits) -> CapabilityDeltas; // static table per kind
}
```

- `Traits` moved to `alc_core` (re-exported from `alc_prisoner`) so the capability
  model lives beside the delta table it produces. `Traits::risk: f32` (serde
  default 0.5) carries the general risk propensity; `apply_capability_modifiers`
  folds deltas into traits clamped to 0..1.
- `Intimidate` → `+aggression 0.20`, `+risk_tolerance 0.15` (the "Respect-flavored
  component" reads as risk tolerance: the relationship-level Respect axis is a
  `Needs` concept — see `alc_social::relationships`).
- Deltas are clamped at apply time (`Smuggle → -discipline 0.05` clamps at 0).

## Intake (`alc_prisoner::generate_intake_with`)

Baseline pool only: ~25% of new arrivals get `CellEscape`, an independent ~15%
get `PickLocks` (max 2 per prisoner). Zero additional systemic intake change.

## Contraband (R3, world-side)

`World::contraband_step()` runs on the daily boundary:

1. `contraband_market(&profiles, &mut rng)` → offers; for each offer the seller's
   `Prisoner::contraband_hidden` grows by 1.
2. Guard sweep via `contraband_discovery(&guards, &profiles, &mut rng)`; a hit
   emits `EventKind::ContrabandFound` (guard + holder actors, severity cause) and
   reduces the holder's `contraband_hidden` by 1.
3. While `contraband_hidden > 0`: `needs.impulse(Freedom, 0.05)` — held stock is
   a slight risk signal (used by `escape_motivation`).

`Prisoner::contraband_hidden: u32` is the world-side stock counter (the user's
"contraband_on_body" idea — direct observer ship was rejected for simplicity).

## Rehabilitation (R1, engine-side)

`alc_world::RehabEnrollment { program: RehabProgram, state: RehabPrisonerState }`
in `World::rehab_enrollments: HashMap<PrisonerId, RehabEnrollment>`
(`#[serde(default)]` — old saves load with no enrollments).

- `Sim::set_rehab_program(id, program) -> bool` — builds state from the prisoner
  (`RehabPrisonerState::from_prisoner`) when absent, calls
  `enroll_in_program` (willingness threshold per program difficulty), inserts the
  enrollment, emits `EventKind::ProgramEnrolled`.
- `Sim::rehab_tick(minutes)` → `World::rehab_tick` — per enrolled prisoner:
  `apply_program_progress(state, program, minutes)` → syncs `RehabPrisonerState`
  back into `Prisoner` (traits, condition, needs, skills). `RehabOutcome::Completed`
  removes the enrollment and emits `EventKind::RehabilitationSuccess` (defined in
  `alc_events`, previously unused — this is its first emission point); `Dropped`
  silently removes it.
- State→Prisoner sync is the "translation" layer between the social-system state
  model and the engine's `Prisoner` model.

## Decision gating (`sumo::decisions::engine_rules`)

`Decision::EscapeAttempt` now requires the prisoner to hold a capability with
kind `Cellular::CellEscape`; otherwise rejected ("no cell-escape capability").
`Prisoner::has_capability(kind)` helper.

## Stats (`sumo::Stats`)

New counters from the event log: `contraband_found`, `program_enrolled`,
`rehab_completed`, `escape_attempts`.

## Tests

- `alc-core/src/capability.rs` — `capability_modifiers_table`: every kind's exact
  delta table + `Capability::new` names.
- `alc-core/src/traits.rs` — default/randomized values incl. new `risk`.
- `alc-prisoner` — intake grants baseline capabilities; `apply_capability_modifiers`
  folds+clamps.
- `alc-world` — `contraband_step` emits `ContrabandFound` + freedom pressure when
  discovery hits; `RehabPrisonerState::from_prisoner` round trip.
- `sumo` (`tests/rehab.rs`) — `set_rehab_program` enrollment + emission,
  `rehab_tick` progress + sync + `RehabilitationSuccess` on completion, escape
  gating with/without `CellEscape`.
- `sumo::Stats::from_events` counts the new kinds.
