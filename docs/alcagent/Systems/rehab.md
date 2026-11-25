# Rehabilitation (`../Systems/rehab.md`)

**Status (0.05, 2026-11-25):** `alc-social::rehab` (program state machine),
`World::rehab_records`, daily `rehab_step`, and `Sim::apply_rehab_progress`/`Sim::rehab_score`
are wired. Enrollment, attendance, completion and failure are persisted and exposed through
validated controls; `ProgramEnrolled`, `RehabilitationProgress`, `RehabilitationSuccess` and
`RehabilitationFailure` events are emitted.

## Sim-level API (`sumo`)

- `Sim::apply_rehab_progress(id: PrisonerId, minutes: u64) -> f32` — deterministic relief applied to the five satisfied needs per minute of program participation: Purpose −0.05m, Respect −0.04m, Stimulation −0.04m, Safety −0.03m, Companionship −0.03m (each clamped at 0). Returns `Sim::rehab_score(id)`.
- `Sim::rehab_score(id) -> f32` — `1.0 − (deficit / 5.0)` over the aggregates `(purpose, respect, stimulation, safety, companionship)` (each a 0..1 satisfaction level).
- Policy hooks: `WardenAction::ChangePolicy { policy: "rehab_funding", value: f64 }` — `policies.rehab_funding_multiplier` (0.2..3.0; see `../Systems/warden-cycle.md`).

## Needs engine (`alc-prisoner::Needs`)

NeedTags:

- Physical: `Hunger, Thirst, Sleep, Fatigue, Hygiene, Health, Medical, Exercise, Temperature, Bathroom`
- Psychological: `Safety, Companionship, Respect, Freedom, Autonomy, Purpose, Stimulation, Hope, Stability, Privacy, Belonging`
- Social: `Friendship, Romance, FamilyContact, GroupAcceptance, Status, Loyalty, Protection, Recognition`

`Needs` fields (deficit pressure, 0 = satisfied): physical — `hunger, thirst, sleep, fatigue, hygiene, medical_need, exercise, temperature_comfort, bathroom`; psychological — `safety, companionship, respect, freedom, autonomy, purpose, stimulation, stability, privacy, belonging`; social — `friendship, family_contact, group_acceptance, status, protection, recognition`. `URGENT_THRESHOLD = 0.75` (a need at or above this is urgent); `DECISION_THRESHOLD = 0.05` (below this contributes no decision noise).

Key operations:

- `Needs::drift_with(dt_mins, ctx: &NeedDriftContext)` — continuous growth per minute, modulated by personality (`Traits` via `ctx.traits`), the activity being performed (`ctx.activity: DriftActivity` — `None, Work, Yard, Recreation, Eat, Sleep, Medical, Conflict, Visitor, Lockdown`), the environment (`crowd_density`, `heat`, `staff_coverage`, `lockdown`), minus relief that succeeded this tick (`ctx.relief: &[NeedTag]`). Legacy `Needs::drift(dt_mins)` uses a neutral context.
- `Needs::relieve(tag, amount)` — actual relief.
- `Needs::impulse(tag, amount)` — discrete jumps (bounded to 0..1).
- `total_pressure() -> f32` — weighted sum (capped at 1.0); feeds guard tension observations.
- `urgent() -> Vec<(NeedTag, f32)>` — urgent needs, worst-first.
- Derived behaviors: `irritability()` (drives `Prisoner::aggression_modifier`), `alertness()`, `desperation()`/`depression_risk()`, `despair_pressure(hope)`, `gang_seeking_pressure()` (safety/protection/belonging), `rule_breaking_urge(discipline)`, `rehab_motivation(hope)` = `hope * (1 − desperation * 0.35) * (1 − freedom * 0.1)`, `medical_neglect()`.
- `Needs::effects(traits, hope) -> Vec<NeedsEffect>` — auditable bundle (Irritability, LowAlertness, DepressionRisk, GangSeeking, RuleBreakingUrge, LowRehabMotivation, MedicalNeglect) for Social/Conflict/Rehab/Health systems.

## Rehab program API (`alc-social::rehab`)

- `RehabProgram`: `Education, Vocational, Counseling, AddictionRecovery, AngerManagement` — `key()` (e.g. `"rehab_education"`), `difficulty()` (0.5–0.7).
- `RehabPrisonerState` — prisoner state for program simulation (`disciplinary_status, education, skills, hope, mental_health, health, aggression, emotional_stability, impulsiveness, discipline, intelligence, reputation, fatigue, purpose_need, stability_need, respect_need, rehab_motivation, desperation`).
- `enroll_in_program(&mut RehabPrisonerState, program) -> bool` — refuses when `disciplinary_status`; requires willingness ≥ threshold = `0.35 + difficulty * 0.2`, where willingness = `0.5 * rehab_motivation + 0.2 * discipline + 0.15 * hope + 0.15 * (1 − desperation)`; enrollment grants a small purpose/hope lift.
- `apply_program_progress(&mut RehabPrisonerState, program, dt_minutes) -> RehabOutcome` — `NoEnrollment | Dropped | Completed | Progress(f32)`:
  - Engagement = `0.45 * rehab_motivation + 0.2 * discipline + 0.15 * intelligence + 0.1 * mental_health + 0.1 * (1 − fatigue)` (clamped 0..1).
  - Dropout risk = `(1 − engagement) * 0.2 + desperation * 0.15`; drops when risk > 0.85 and engagement < 0.2.
  - Rate = `0.01 + engagement * 0.03`, scaled by `(1 − difficulty * 0.4)` and hours (`dt/60`); progress capped at 1.0 per step.
  - Per-program effects: Education → skill + hope + purpose_need relief; Vocational → skill + hope + purpose relief; Counseling → mental_health + hope + stability relief; AddictionRecovery → health + hope + impulsiveness relief; AngerManagement → aggression − + emotional_stability + + respect_need relief.

## Rehab-related events

`EventKind::RehabilitationSuccess` and `EventKind::ProgramEnrolled` are defined in the event taxonomy for rehab completion/enrollment lifecycle tracking (see `../Systems/event-contract.md`).
