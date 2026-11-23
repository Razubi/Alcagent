//! Warden AI per Directive §9.

pub mod expansion;
pub mod finance;

pub use expansion::AttentionBudget;

use alc_core::{RngStream, SimTime, WardenId};
use alc_economy::Economy;
use serde::{Deserialize, Serialize};

/// Warden personality traits (all 0..1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WardenTraits {
    pub strictness: f32,
    pub compassion: f32,
    pub political_ambition: f32,
    pub financial_discipline: f32,
    pub risk_tolerance: f32,
    pub reform_orientation: f32,
    pub authoritarian_tendencies: f32,
    pub staff_loyalty: f32,
    pub corruption_susceptibility: f32,
    pub reputation_concerns: f32,
    pub competence: f32,
}

impl WardenTraits {
    pub fn randomized(rng: &mut RngStream) -> Self {
        let r = |rng: &mut RngStream| rng.range(0.1, 0.95) as f32;
        Self {
            strictness: r(rng), compassion: r(rng), political_ambition: r(rng),
            financial_discipline: r(rng), risk_tolerance: r(rng), reform_orientation: r(rng),
            authoritarian_tendencies: r(rng), staff_loyalty: r(rng),
            corruption_susceptibility: r(rng), reputation_concerns: r(rng), competence: r(rng),
        }
    }
}

/// Competing institutional objectives (Directive §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Objective {
    Security,
    FinancialSustainability,
    StaffMorale,
    PrisonerWelfare,
    PublicReputation,
    InstitutionalStability,
    Rehabilitation,
    EscapePrevention,
    ViolenceReduction,
    PoliticalPressure,
    FacilityExpansion,
}

/// Action the warden can take.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WardenAction {
    Expand { building: String },
    HireGuards { count: u32 },
    FireGuards { count: u32 },
    PromoteStaff { guard: u64 },
    ChangePolicy { policy: String, value: f64 },
    IncreaseSecurity,
    ReduceExpenses,
    ImproveInfrastructure { facility: u64 },
    RespondToDisturbance { event_id: u64 },
    NegotiateWithRepresentatives,
    InvestigateMisconduct { target: u64 },
    AdjustParoleRecommendation { prisoner: u64, lean: f32 },
    RequestFunding { amount_major: i64 },
    /// Explicit "hold steady" — a first-class decision (design §5.5).
    NoAction { rationale: String },
}

/// The warden agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warden {
    pub id: WardenId,
    pub name: String,
    pub traits: WardenTraits,
    pub philosophy: String,
    pub inbox: Vec<u64>,          // event ids the warden has "received" (credibility-scored)
    pub last_decision_at: SimTime,
    pub pending_actions: Vec<WardenAction>,
    /// Attention budget: 0..1 remaining administrative capacity.
    pub attention: f32,
    /// Rated bed capacity the warden is currently managing.
    #[serde(default)]
    pub capacity: u32,
    /// Security incidents observed in the trailing 7-day window.
    #[serde(default)]
    pub security_incidents_this_week: u32,
    /// Shorter alias for `security_incidents_this_week` (kept in sync).
    #[serde(default)]
    pub incidents_this_week: u32,
    /// Last simulated day the warden took an action (cooldown tracking).
    #[serde(default)]
    pub last_action_day: u64,
}

impl Warden {
    pub fn new(id: WardenId, name: impl Into<String>, traits: WardenTraits) -> Self {
        Self {
            id,
            name: name.into(),
            philosophy: derive_philosophy(&traits),
            traits,
            inbox: Vec::new(),
            last_decision_at: SimTime::EPOCH,
            pending_actions: Vec::new(),
            attention: 1.0,
            capacity: 0,
            security_incidents_this_week: 0,
            incidents_this_week: 0,
            last_action_day: 0,
        }
    }

    /// Update the institutional state the warden arbitrates over.
    pub fn set_context(&mut self, capacity: u32, security_incidents_this_week: u32) {
        self.capacity = capacity;
        self.security_incidents_this_week = security_incidents_this_week;
        self.incidents_this_week = security_incidents_this_week;
    }

    /// Record one additional security incident in the trailing window.
    pub fn note_security_incident(&mut self) {
        self.security_incidents_this_week = self.security_incidents_this_week.saturating_add(1);
        self.incidents_this_week = self.security_incidents_this_week;
    }

    /// Clear the trailing-week incident counter (call on a weekly boundary).
    pub fn reset_weekly_incidents(&mut self) {
        self.security_incidents_this_week = 0;
        self.incidents_this_week = 0;
    }

    /// View the warden's 0..1 attention as a point-based attention budget.
    pub fn attention_budget(&self) -> AttentionBudget {
        let base = AttentionBudget::default();
        let base_refill = base.base_refill;
        let max_points = base.max_points;
        let mut budget = base;
        budget.points = (self.attention * base_refill).clamp(0.0, max_points);
        budget
    }

    /// Whether the warden can afford a decision of `cost` attention points.
    pub fn can_decide(&self, cost: f32) -> bool {
        self.attention_budget().can_afford(cost)
    }

    /// Charge attention for a decision; returns false when exhausted.
    pub fn spend_attention(&mut self, cost: f32) -> bool {
        let base = AttentionBudget::default();
        let mut budget = self.attention_budget();
        if budget.spend(cost) {
            self.attention = (budget.points / base.base_refill).clamp(0.0, 1.0);
            true
        } else {
            false
        }
    }

    /// Daily attention refill, accounting for competence and active sites.
    pub fn refill_attention(&mut self, active_construction_sites: u32) {
        let base = AttentionBudget::default();
        let mut budget = AttentionBudget::new(self.attention * base.base_refill);
        budget.refill_daily(self.traits.competence, active_construction_sites);
        self.attention = (budget.points / base.base_refill).clamp(0.0, 1.0);
    }

    /// Number of concurrent decisions the warden can still afford.
    pub fn concurrent_decisions_remaining(&self, cost: f32) -> u32 {
        self.attention_budget().max_concurrent_decisions(cost)
    }

    /// Utility score for an objective given current (partial) state.
    /// Fog-of-war is applied by the caller via inbox contents.
    pub fn objective_utility(&self, obj: Objective, population: u32, cash_major: f64) -> f32 {
        let t = &self.traits;
        match obj {
            Objective::Security => t.strictness * 0.7 + t.authoritarian_tendencies * 0.3,
            Objective::FinancialSustainability => t.financial_discipline,
            Objective::StaffMorale => t.staff_loyalty,
            Objective::PrisonerWelfare => t.compassion,
            Objective::PublicReputation => t.reputation_concerns,
            Objective::InstitutionalStability => (t.competence + t.strictness) * 0.5,
            Objective::Rehabilitation => t.reform_orientation,
            Objective::EscapePrevention => t.risk_tolerance * 0.6 + t.strictness * 0.4,
            Objective::ViolenceReduction => t.compassion * 0.6 + t.competence * 0.4,
            Objective::PoliticalPressure => t.political_ambition,
            Objective::FacilityExpansion => {
                // expansion attractive when cash-rich and population pressure high
                let cash_factor = (cash_major / 500_000.0).min(1.0);
                let pop_factor = (population as f32 / 200.0).min(1.0);
                (cash_factor as f32 * 0.6 + pop_factor * 0.4) * (0.5 + t.risk_tolerance * 0.5)
            }
        }
    }
}

fn derive_philosophy(t: &WardenTraits) -> String {
    if t.reform_orientation > 0.65 && t.compassion > 0.6 {
        "Reformist: believes rehabilitation over punishment."
    } else if t.strictness > 0.7 && t.authoritarian_tendencies > 0.6 {
        "Iron-fist: order through strict discipline."
    } else if t.financial_discipline > 0.7 {
        "Steward: institutional survival through fiscal control."
    } else {
        "Pragmatist: balances competing pressures case by case."
    }
    .to_string()
}

/// Select the next action from the warden's live context + economy state.
///
/// State-aware overrides take precedence over the static utility weights:
///
/// 1. Negative cash is a solvency emergency → [`WardenAction::ReduceExpenses`].
/// 2. More than five security incidents in the trailing week →
///    [`WardenAction::IncreaseSecurity`].
/// 3. Population above 90% of rated capacity → [`WardenAction::Expand`].
/// 4. An exhausted attention budget → [`WardenAction::NoAction`].
///
/// Capacity and the incident count are read from the [`Warden`] state; use
/// [`arbitrate_with_context`] to supply them explicitly.
pub fn arbitrate(warden: &mut Warden, economy: &Economy, population: u32, rng: &mut RngStream) -> WardenAction {
    let incidents = warden
        .security_incidents_this_week
        .max(warden.incidents_this_week);
    arbitrate_with_context(warden, economy, population, warden.capacity, incidents, rng)
}

/// State-aware arbitration with explicit institutional context.
///
/// `security_incidents_this_week` is the trailing 7-day incident count.
pub fn arbitrate_with_context(
    warden: &mut Warden,
    economy: &Economy,
    population: u32,
    capacity: u32,
    security_incidents_this_week: u32,
    rng: &mut RngStream,
) -> WardenAction {
    // 1. Solvency emergency: the prison cannot pay its bills.
    if economy.cash.0 < 0 {
        return WardenAction::ReduceExpenses;
    }

    // 2. Security crisis: incident rate above the crisis threshold.
    if security_incidents_this_week > 5 {
        return WardenAction::IncreaseSecurity;
    }

    // 3. Crowding: population above 90% of rated capacity.
    if capacity > 0 && (population as f64) > 0.9 * (capacity as f64) {
        return WardenAction::Expand {
            building: "cell_block".into(),
        };
    }

    // 4. Attention exhaustion: defer non-critical decisions.
    if !warden.can_decide(AttentionBudget::tactical_cost()) {
        return WardenAction::NoAction {
            rationale: "attention budget exhausted".into(),
        };
    }

    let cash = economy.cash.as_major();
    let objs = [
        Objective::Security,
        Objective::FinancialSustainability,
        Objective::FacilityExpansion,
        Objective::Rehabilitation,
        Objective::PublicReputation,
    ];

    // Score each objective, then use the RNG only to break exact ties so the
    // same static weights cannot pin the warden to one action forever.
    let mut best_score = f32::NEG_INFINITY;
    let mut ties: Vec<Objective> = Vec::new();
    for obj in objs {
        let mut score = warden.objective_utility(obj, population, cash);
        // Light state-aware nudges in the non-emergency regime.
        if matches!(obj, Objective::FinancialSustainability) && economy.health() < 0.4 {
            score += 0.25;
        }
        if matches!(obj, Objective::Security | Objective::EscapePrevention | Objective::ViolenceReduction)
            && security_incidents_this_week > 0
        {
            score += 0.05 * security_incidents_this_week.min(5) as f32;
        }
        if matches!(obj, Objective::FacilityExpansion)
            && capacity > 0
            && (population as f64) > 0.75 * (capacity as f64)
        {
            score += 0.2;
        }
        if score > best_score + 1e-6 {
            best_score = score;
            ties.clear();
            ties.push(obj);
        } else if (score - best_score).abs() <= 1e-6 {
            ties.push(obj);
        }
    }
    let best = if ties.len() > 1 {
        ties[(rng.next_u64() as usize) % ties.len()]
    } else {
        ties[0]
    };

    match best {
        Objective::Security | Objective::EscapePrevention | Objective::ViolenceReduction => {
            WardenAction::IncreaseSecurity
        }
        Objective::FinancialSustainability => WardenAction::ReduceExpenses,
        Objective::FacilityExpansion => WardenAction::Expand {
            building: "cell_block".into(),
        },
        Objective::Rehabilitation => WardenAction::ChangePolicy {
            policy: "rehab_funding".into(),
            value: 1.1,
        },
        Objective::StaffMorale => WardenAction::HireGuards { count: 2 },
        Objective::PublicReputation
        | Objective::PoliticalPressure
        | Objective::InstitutionalStability
        | Objective::PrisonerWelfare => WardenAction::NegotiateWithRepresentatives,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::WardenId;
    use alc_economy::Money;

    fn warden() -> Warden {
        Warden::new(WardenId(0), "Test Warden", WardenTraits::randomized(&mut RngStream::new(7, 0)))
    }

    #[test]
    fn negative_cash_forces_expense_reduction() {
        let mut w = warden();
        let mut e = Economy::default();
        e.cash = Money(-1);
        let action = arbitrate(&mut w, &e, 24, &mut RngStream::new(1, 1));
        assert!(matches!(action, WardenAction::ReduceExpenses), "{action:?}");
    }

    #[test]
    fn incident_spike_forces_security() {
        let mut w = warden();
        w.set_context(100, 6);
        let e = Economy::default();
        let action = arbitrate(&mut w, &e, 24, &mut RngStream::new(2, 2));
        assert!(matches!(action, WardenAction::IncreaseSecurity), "{action:?}");
    }

    #[test]
    fn crowding_forces_expansion() {
        let mut w = warden();
        w.set_context(100, 0);
        let e = Economy::default();
        let action = arbitrate(&mut w, &e, 95, &mut RngStream::new(3, 3));
        assert!(
            matches!(action, WardenAction::Expand { ref building } if building == "cell_block"),
            "{action:?}"
        );
    }

    #[test]
    fn below_crowding_threshold_does_not_force_expansion() {
        let mut w = warden();
        w.set_context(100, 0);
        let e = Economy::default();
        let action = arbitrate(&mut w, &e, 89, &mut RngStream::new(4, 4));
        assert!(!matches!(action, WardenAction::Expand { .. }), "{action:?}");
    }

    #[test]
    fn exhausted_attention_defers_decisions() {
        let mut w = warden();
        w.set_context(100, 0);
        w.attention = 0.0;
        let e = Economy::default();
        let action = arbitrate(&mut w, &e, 24, &mut RngStream::new(5, 5));
        assert!(matches!(action, WardenAction::NoAction { .. }), "{action:?}");
    }

    #[test]
    fn attention_budget_limits_concurrent_decisions() {
        let mut w = warden();
        assert!(w.can_decide(AttentionBudget::tactical_cost()));
        assert_eq!(w.concurrent_decisions_remaining(AttentionBudget::tactical_cost()), 50);
        w.attention = 0.05; // 5 points
        assert_eq!(w.concurrent_decisions_remaining(AttentionBudget::tactical_cost()), 2);
        assert!(!w.can_decide(AttentionBudget::strategic_cost()));
    }

    fn is_engine_applicable(action: &WardenAction) -> bool {
        // All current WardenAction variants are handled by sumo::drivers::apply_warden_action.
        // (alc-world handles a subset and ignores others, but the engine driver covers all.)
        matches!(
            action,
            WardenAction::Expand { .. }
                | WardenAction::HireGuards { .. }
                | WardenAction::FireGuards { .. }
                | WardenAction::PromoteStaff { .. }
                | WardenAction::ChangePolicy { .. }
                | WardenAction::IncreaseSecurity
                | WardenAction::ReduceExpenses
                | WardenAction::ImproveInfrastructure { .. }
                | WardenAction::RespondToDisturbance { .. }
                | WardenAction::NegotiateWithRepresentatives
                | WardenAction::InvestigateMisconduct { .. }
                | WardenAction::AdjustParoleRecommendation { .. }
                | WardenAction::RequestFunding { .. }
                | WardenAction::NoAction { .. }
        )
    }

    #[test]
    fn arbitrate_across_states_never_panics_and_returns_engine_applicable_action() {
        let contexts: Vec<(u32, u32, u32, i64, f32)> = vec![
            // (population, capacity, incidents, cash_minor, attention)
            (40, 100, 0, 25_000_000, 1.0),   // healthy
            (0, 100, 0, 25_000_000, 1.0),    // empty
            (40, 100, 0, -1, 1.0),           // bankrupt
            (95, 100, 0, 25_000_000, 1.0),   // overcrowded
            (40, 100, 8, 25_000_000, 1.0),   // high incidents / crisis
            (40, 100, 3, 25_000_000, 1.0),   // low morale proxy / moderate unrest
            (40, 0, 0, 25_000_000, 1.0),     // no capacity info
            (40, 100, 0, 25_000_000, 0.0),   // exhausted attention
            (200, 100, 12, -500_000, 0.0),   // worst case
            (10, 100, 1, 500_000, 0.3),      // tight finances
        ];

        let seeds = [1u64, 7, 42, 99, 1234];
        for (population, capacity, incidents, cash_minor, attention) in contexts {
            for seed in seeds {
                let mut w = warden();
                w.set_context(capacity, incidents);
                w.attention = attention;
                let mut e = Economy::default();
                e.cash = Money(cash_minor);
                // Vary economy health context for "low morale / tight" scenarios.
                if cash_minor > 0 && cash_minor < 5_000_000 {
                    e.recent_daily_expenses = vec![400_000];
                }
                let mut rng = RngStream::new(seed, population as u64 + incidents as u64);
                let action = arbitrate(&mut w, &e, population, &mut rng);
                assert!(
                    is_engine_applicable(&action),
                    "non-applicable action for pop={population} cap={capacity} inc={incidents} cash={cash_minor} att={attention}: {action:?}"
                );

                // Also exercise explicit-context path.
                let mut w2 = warden();
                w2.attention = attention;
                let action2 = arbitrate_with_context(
                    &mut w2,
                    &e,
                    population,
                    capacity,
                    incidents,
                    &mut RngStream::new(seed + 1, 0),
                );
                assert!(
                    is_engine_applicable(&action2),
                    "non-applicable action (explicit ctx): {action2:?}"
                );
            }
        }
    }
}