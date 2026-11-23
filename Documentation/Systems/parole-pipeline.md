# Parole Pipeline (`Documentation/Systems/parole-pipeline.md`)

## Eligibility (`alc_prisoner::Prisoner::is_eligible_for_parole`)

`sentence_served_days / sentence_days >= 0.6` and not `schedule.disciplinary_status`.

## Social-side board (`alc_social::parole`)

- `ParoleSubject { sentence_days, sentence_served_days, disciplinary_status, aggression }` — `served_fraction()`, `is_eligible()`.
- `ParoleBoard::review(prisoner, behavior, rehab, risk) -> ParoleDecision` — clamps inputs to 0..1:
  - `behavior`: observed institutional behavior quality (higher is better).
  - `rehab`: rehabilitation/program progress and engagement (0..1).
  - `risk`: risk to public/institution on release (higher is riskier).
  - Returns `Grant { conditions: Vec<ParoleCondition> }` / `Defer { reason, review_days }` / `Deny { reason }`.
  - Defaults: insufficient progress or ineligibility → `Defer` (180 days when served < 0.3, 90 otherwise); active disciplinary status → `Deny`; grant when `support >= 0.68 && risk <= 0.45` with conditions (`RegularSupervision` + `MandatoryReporting` always; `TreatmentProgram` when rehab < 0.7; `Curfew` when risk > 0.25 or behavior < 0.8; `EmploymentRequirement` when served < 0.85; `NoContactOrder` when aggression > 0.6 or risk > 0.35); `Defer` when `support >= 0.5 && aggravators < 0.65`; else `Deny`.
- `ParoleCondition`: `RegularSupervision, Curfew, EmploymentRequirement, TreatmentProgram, NoContactOrder, MandatoryReporting`.

## Engine ledger (`sumo::ParoleLedger`, `crates/sumo/src/parole.rs`)

- `ParoleStatus`: `Scheduled` (hearing scheduled), `InHearing`, `Granted`, `Denied`.
- `ParoleLedger`: `status(id) -> Option<ParoleStatus>`, `has(id) -> bool`, `set(id, status, at)`, `hearing_at(id) -> Option<SimTime>`, `last_decision_at()`, `adjustment(id) -> f32`, `set_adjustment(id, lean)` (lean clamped to [-1, 1]).

## Sim API (`sumo::Sim`)

- `Sim::parole_due() -> Vec<PrisonerId>` — prisoners eligible in world state and not in the engine ledger.
- `Sim::start_parole_hearing(id) -> Option<u64>` — requires the prisoner to be present, no existing record; sets `Scheduled`; emits `EventKind::ParoleHearing`.
- `Sim::grant_parole(id) -> Option<u64>` — sets `InHearing` (if not yet set) then `Granted`; emits `EventKind::ParoleGranted`.
- `Sim::deny_parole(id, reason) -> Option<u64>` — sets `Denied`; emits `EventKind::ParoleDenied`.
- `Sim::parole_status(id) -> Option<ParoleStatus>`.
- `Sim::apply_warden_action(WardenAction::AdjustParoleRecommendation { prisoner, lean })` — records the warden's adjustment in the ledger.

## Process (Directive A15)

1. Eligibility detection — `Sim::parole_due()`.
2. Record a prisoner (`start_parole_hearing`).
3. Review institutional history — events since the hearing (`Sim::events_since`).
4. Review behavior — `Prisoner::needs`, `aggression_modifier()`, discipline.
5. Review rehabilitation — `Sim::rehab_score(id)`.
6. Review risk — e.g. `Prisoner::escape_motivation()`.
7. Hearing (AI-gated; engine-validated via `Sim::deliver_decision` / `Sim::warden_cycle`).
8. Decision — `Sim::grant_parole` / `Sim::deny_parole`.
9. Conditions — recorded as ledger adjustments.
10. Release or continued incarceration.

Note: engine ledger state is not persisted — world saves are the source of truth; `Sim::load` resets the ledger (see `Documentation/Reports/sumo-audit.md`).
