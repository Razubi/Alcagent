# Warden Cycle (`docs/Systems/warden-cycle.md`)

The warden is an independent AI agent (`alc_warden::Warden`). The engine drives its cycle: `Sim::warden_cycle()` runs `alc_warden::arbitrate` and applies the action with `Sim::apply_warden_action`; `Sim::warden_pending()` reads pending actions.

## WardenTraits

`strictness`, `compassion`, `political_ambition`, `financial_discipline`, `risk_tolerance`, `reform_orientation`, `authoritarian_tendencies`, `staff_loyalty`, `corruption_susceptibility`, `reputation_concerns`, `competence` (0..1). `derive_philosophy()` renders the `philosophy` string.

## Objectives (`arbitrate`)

`Objective`: `Security, FinancialSustainability, StaffMorale, PrisonerWelfare, PublicReputation, InstitutionalStability, Rehabilitation, EscapePrevention, ViolenceReduction, PoliticalPressure, FacilityExpansion`.

`arbitrate(warden, economy, population, rng) -> WardenAction` — state-aware overrides first:

1. `economy.cash.0 < 0` → `WardenAction::ReduceExpenses` (solvency emergency).
2. `security_incidents_this_week > 5` → `WardenAction::IncreaseSecurity`.
3. `capacity > 0 && population > 0.9 * capacity` → `WardenAction::Expand { building: "cell_block" }`.
4. `!warden.can_decide(AttentionBudget::tactical_cost())` → `WardenAction::NoAction { rationale: "attention budget exhausted" }`.

Otherwise: score objectives via `Warden::objective_utility` with light state nudges (FinancialSustainability +0.25 when `economy.health() < 0.4`; Security/EscapePrevention/ViolenceReduction +0.05 per incident up to 5; FacilityExpansion +0.2 at > 0.75 capacity). Pick the best — RNG breaks exact ties only. Mapping: Security/EscapePrevention/ViolenceReduction → IncreaseSecurity; FinancialSustainability → ReduceExpenses; FacilityExpansion → Expand{cell_block}; Rehabilitation → ChangePolicy{rehab_funding: 1.1}; StaffMorale → HireGuards{2}; PublicReputation/PoliticalPressure/InstitutionalStability/PrisonerWelfare → NegotiateWithRepresentatives.

`arbitrate_with_context(warden, economy, population, capacity, security_incidents_this_week, rng)` — explicit inputs (fog of war).

## WardenAction

- `Expand { building: String }` — engine maps `"medical" | "education" | "isolation" | "visitor" | "workshop" | "recreation"` → corresponding `FacilityKind` (else `CellBlock`); adds facility (`world.add_facility(kind, 1, 40)`); emits `EventKind::Expansion`.
- `HireGuards { count }` / `FireGuards { count }` — create/remove guards; emit `PolicyChange`.
- `PromoteStaff { guard: u64 }` — rank bump: Officer→Sergeant→Lieutenant→Captain→Captain; no event.
- `ChangePolicy { policy, value: f64 }` — `"privileges"` → `policies.privileges_level` (clamped 0..1); `"security"` → `policies.security_level` (0..1); `"rehab_funding"` → `policies.rehab_funding_multiplier` (0.2..3.0); emits `PolicyChange`.
- `IncreaseSecurity` — `security_level + 0.1`, capped 1.0.
- `ReduceExpenses` — `rehab_funding_multiplier − 0.1`, floored 0.2.
- `ImproveInfrastructure { facility: u64 }` — `condition + 0.05` capped 1.0.
- `RespondToDisturbance { .. }` — emits `Investigation`.
- `NegotiateWithRepresentatives` — emits `PolicyChange`.
- `InvestigateMisconduct { target: u64 }` — emits `Investigation`.
- `AdjustParoleRecommendation { prisoner, lean: f32 }` — engine records the adjustment in the `ParoleLedger`; emits `ParoleBoardDecision`.
- `RequestFunding { amount_major: i64 }` — emits `BudgetChange`; `warden.attention −= 0.05`.
- `NoAction { rationale }` — hold steady.

## Warden state (`Warden`)

`Warden { id, name, traits, philosophy, inbox: Vec<u64>, last_decision_at, pending_actions, attention: f32 (0..1), capacity: u32, security_incidents_this_week, incidents_this_week, last_action_day }`. Set each day from world state via `warden.set_context(capacity, security_incidents_this_week)` (done in `World::on_day_boundary`).

## Attention budget (`alc_warden::expansion::AttentionBudget`)

Fields: `points`, `max_points` (120), `base_refill` (100), `competence_gain` (0.6), `per_site_drain` (2.0).

- `Warden::attention_budget() -> AttentionBudget` — `points = (attention * base_refill).clamp(0, max_points)`.
- `can_afford(cost)`, `spend(cost)` (returns false when exhausted), `Warden::can_decide(cost)`, `Warden::spend_attention(cost) -> bool` (returns false when exhausted), `Warden::concurrent_decisions_remaining(cost) -> u32` (`(points / cost).floor()`).
- `AttentionBudget::new(points)` — clamps to [0, max_points].
- `refill_daily(competence_0_to_1, active_construction_sites)` — `refill = base_refill * (0.7 + competence_gain * comp)`; `drain = per_site_drain * sites`; `points = (points + refill − drain).clamp(0, max_points)`.
- `can_afford`, `spend`, `reserve(minimum)`, `max_concurrent_decisions(cost)`.

Cost constants: `tactical_cost() = 2.0`, `strategic_cost() = 12.0`, `crisis_cost() = 20.0`, `investigation_weekly_cost() = 4.0`.

## Expansion catalog (`alc_warden::expansion`)

`BuildingSpec { key, name, cost_minor (cents), build_days, capacity, staff_required: Vec<StaffRequirement{role, count}>, prerequisites }`. `building_catalog()` — 12 building types:

| key | cost_minor | build_days | capacity |
|---|---|---|---|
| cell_block | 48_000_000 | 120 | 48 |
| dormitory | 26_000_000 | 75 | 60 |
| medical_facility | 52_000_000 | 130 | 12 |
| kitchen | 24_000_000 | 60 | 120 |
| workshop | 31_000_000 | 80 | 30 |
| recreation_area | 18_000_000 | 45 | 80 |
| education_building | 39_000_000 | 100 | 24 |
| administrative_building | 44_000_000 | 110 | 0 |
| isolation_facility | 50_000_000 | 110 | 12 |
| visitor_facilities | 21_000_000 | 55 | 20 |
| staff_facilities | 28_000_000 | 70 | 0 |
| yard_expansion | 15_000_000 | 40 | 100 |

`find_building_spec(key) -> Option<BuildingSpec>`; `BuildingSpec::catalog()` (alias). Prerequisites referenced: `power_headroom`, `perimeter_integrity`, `administrative_building`, `water`, `utilities`, `reception`, `screening`, `yard_or_space`.

`WardenTraits` example values are drawn from the vetted design; the live generator randomizes them per world seed.
