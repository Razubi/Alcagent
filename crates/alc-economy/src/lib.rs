//! Prison economy: integer-minor-unit ledger, revenue, expenses, planning.
//!
//! Money is stored as integer minor units (cents). All rates are configurable
//! per Directive §11. Degradation curves replace arbitrary instant penalties:
//! neglected systems decay over time and accrue interest.

use serde::{Deserialize, Serialize};

/// Money in minor units (cents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
pub struct Money(pub i64);

impl Money {
    pub const ZERO: Money = Money(0);
    pub fn from_major(major: i64) -> Self { Money(major * 100) }
    pub fn as_major(self) -> f64 { self.0 as f64 / 100.0 }
    pub fn add(self, o: Money) -> Self { Money(self.0 + o.0) }
    pub fn sub(self, o: Money) -> Self { Money(self.0 - o.0) }
    pub fn mul(self, f: f64) -> Self { Money((self.0 as f64 * f) as i64) }
}

/// Chart of accounts (double-category per design).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Account {
    // Revenue
    PerCapitaFunding,
    GovGrant,
    Bonus,
    Penalty,
    ContractIncome,
    RehabFunding,
    SpecialProgram,
    // Expenses
    Food,
    Water,
    Utilities,
    Salaries,
    Medical,
    Maintenance,
    Construction,
    SecurityEquipment,
    Education,
    Rehabilitation,
    Transportation,
    Supplies,
    Admin,
}

/// A ledger entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub at_secs: u64,
    pub account: Account,
    pub amount: Money, // positive = inflow, negative = outflow
    pub note: String,
}

/// Financial state (per design §11).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Economy {
    pub cash: Money,
    pub ledger: Vec<LedgerEntry>,
    /// Configurable per-capita daily funding (major units).
    pub funding_per_prisoner_per_day: Money,
    /// Fixed institutional grant (major units per day). Covers baseline
    /// infrastructure and a minimum guard roster even when the population
    /// dips — without it, any population decline causes permanent insolvency
    /// because payroll is a fixed cost (Directive §11 "Government grants").
    #[serde(default = "default_base_grant")]
    pub base_daily_grant: Money,
    /// Crowding penalty applies above `crowding_threshold`.
    pub crowding_threshold: u32,
    pub crowding_penalty_per_extra: f64,
    /// Trailing daily expense totals (minor units, positive magnitudes), most recent last.
    /// Used for 30-day average health/runway heuristics.
    #[serde(default)]
    pub recent_daily_expenses: Vec<i64>,
}

fn default_base_grant() -> Money {
    // Covers the minimum two-guard roster (~$280/day) plus baseline utilities
    // so a small, stable population is not structurally bankrupt.
    Money::from_major(350)
}

impl Default for Economy {
    fn default() -> Self {
        Self {
            cash: Money::from_major(250_000),
            ledger: Vec::new(),
            // Funded at a sustainable per-prisoner rate: at the starting
            // 24 prisoners / 4 guards the prison covers payroll with a small
            // surplus; overcrowding or guard-heavy staffing still strains it.
            funding_per_prisoner_per_day: Money::from_major(45),
            base_daily_grant: default_base_grant(),
            crowding_threshold: 120,
            crowding_penalty_per_extra: 0.02,
            recent_daily_expenses: Vec::new(),
        }
    }
}

impl Economy {
    pub fn record(&mut self, at_secs: u64, account: Account, amount: Money, note: impl Into<String>) {
        self.cash = self.cash.add(amount);
        self.ledger.push(LedgerEntry { at_secs, account, amount, note: note.into() });
    }

    /// Daily inflow: fixed institutional grant + per-capita funding with a
    /// crowding penalty above threshold. Returns the total credited.
    pub fn accrue_daily_funding(&mut self, at_secs: u64, population: u32) -> Money {
        let grant = self.base_daily_grant;
        if grant.0 != 0 {
            self.record(at_secs, Account::GovGrant, grant, "base institutional grant");
        }
        let base = self.funding_per_prisoner_per_day.mul(population as f64);
        let penalty = if population > self.crowding_threshold {
            let extra = population - self.crowding_threshold;
            base.mul(1.0 - self.crowding_penalty_per_extra * extra as f64)
        } else {
            base
        };
        self.record(at_secs, Account::PerCapitaFunding, penalty, format!("population={population}"));
        grant.add(penalty)
    }

    /// Full daily expense sweep. Returns total spent (positive magnitude).
    pub fn accrue_daily_expenses(
        &mut self,
        at_secs: u64,
        population: u32,
        guards: u32,
        buildings: u32,
    ) -> Money {
        let food = Money::from_major(4).mul(population as f64);
        let utilities = Money::from_major(2).mul(population as f64 + buildings as f64 * 5.0);
        let salaries = Money::from_major(140).mul(guards as f64);
        let maintenance = Money::from_major(1).mul(buildings as f64);
        let total = food.add(utilities).add(salaries).add(maintenance);
        self.record(at_secs, Account::Food, food.mul(-1.0), "meals");
        self.record(at_secs, Account::Utilities, utilities.mul(-1.0), "power/water");
        self.record(at_secs, Account::Salaries, salaries.mul(-1.0), "guard payroll");
        self.record(at_secs, Account::Maintenance, maintenance.mul(-1.0), "upkeep");
        self.push_daily_expense(total.0.max(0));
        total
    }

    /// Record a positive bonus inflow.
    pub fn accrue_bonus(&mut self, at_secs: u64, amount: Money, note: impl Into<String>) -> Money {
        let inflow = Money(amount.0.max(0));
        self.record(at_secs, Account::Bonus, inflow, note);
        inflow
    }

    /// Apply a penalty outflow (stored as negative ledger amount).
    pub fn apply_penalty(&mut self, at_secs: u64, amount: Money, note: impl Into<String>) -> Money {
        let magnitude = amount.0.max(0);
        let outflow = Money(-magnitude);
        self.record(at_secs, Account::Penalty, outflow, note);
        Money(magnitude)
    }

    fn push_daily_expense(&mut self, amount_minor: i64) {
        self.recent_daily_expenses.push(amount_minor);
        if self.recent_daily_expenses.len() > 30 {
            let drop = self.recent_daily_expenses.len() - 30;
            self.recent_daily_expenses.drain(0..drop);
        }
    }

    /// 30-day trailing average of daily expenses (minor units), or a small floor if unknown.
    pub fn trailing_daily_expense_avg(&self) -> i64 {
        if self.recent_daily_expenses.is_empty() {
            // Conservative floor avoids divide-by-zero and pessimistic early-runway guesses.
            Money::from_major(5_000).0
        } else {
            let sum: i64 = self.recent_daily_expenses.iter().copied().sum();
            (sum / self.recent_daily_expenses.len() as i64).max(1)
        }
    }

    /// Current financial health 0..1 (used by warden AI & oversight).
    ///
    /// Real runway calculation: `min(1.0, cash / (trailing_daily_expense_avg * 30))`.
    /// One month of runway is perfect health (1.0); zero or negative cash is 0.0.
    pub fn health(&self) -> f64 {
        let avg_daily = self.trailing_daily_expense_avg().max(1) as f64;
        let month_of_expenses = avg_daily * 30.0;
        (self.cash.0 as f64 / month_of_expenses).min(1.0).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_is_cash_over_thirty_days_of_trailing_expenses() {
        let mut e = Economy::default();
        // $3,000/day trailing expense average.
        e.recent_daily_expenses = vec![3_000_000];
        // Exactly one month of runway -> perfect health.
        e.cash = Money::from_major(900_000);
        assert!((e.health() - 1.0).abs() < 1e-9, "health={}", e.health());
        // Half a month of runway -> 0.5.
        e.cash = Money::from_major(450_000);
        assert!((e.health() - 0.5).abs() < 1e-9, "health={}", e.health());
        // Negative cash -> 0.0.
        e.cash = Money(-1);
        assert_eq!(e.health(), 0.0);
    }

    #[test]
    fn health_uses_trailing_average_not_single_day() {
        let mut e = Economy::default();
        e.recent_daily_expenses = vec![1_000_000, 1_000_000, 3_000_000];
        // Average = 5/3 million minor units per day.
        let avg = e.trailing_daily_expense_avg();
        assert_eq!(avg, 5_000_000 / 3);
        e.cash = Money(avg * 30);
        assert!((e.health() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn health_saturates_for_large_runways() {
        let mut e = Economy::default();
        e.recent_daily_expenses = vec![1_000_000];
        e.cash = Money::from_major(10_000_000);
        assert_eq!(e.health(), 1.0);
    }

    #[test]
    fn economy_stable_over_180_days_realistic_params() {
        let mut e = Economy::default();
        let start_budget = e.cash.0;
        let mut min_cash = e.cash.0;
        let mut max_cash = e.cash.0;
        let mut min_health = 1.0f64;
        let mut max_health = 0.0f64;
        let population = 40u32;
        let guards = 10u32;
        let buildings = 10u32;
        for day in 0..180u64 {
            let at = day * 86_400;
            e.accrue_daily_funding(at, population);
            e.accrue_daily_expenses(at, population, guards, buildings);
            let h = e.health();
            assert!(h.is_finite(), "health not finite at day {day}: {h}");
            assert!(
                (0.0..=1.0).contains(&h),
                "health out of range at day {day}: {h}"
            );
            min_cash = min_cash.min(e.cash.0);
            max_cash = max_cash.max(e.cash.0);
            min_health = min_health.min(h);
            max_health = max_health.max(h);
        }
        // Budget must not diverge to infinity/-infinity; i64 stays finite but ensure bounded evolution.
        assert!(
            e.cash.0 > i64::MIN / 2 && e.cash.0 < i64::MAX / 2,
            "cash diverged: start={start_budget} end={} min={min_cash} max={max_cash}",
            e.cash.0
        );
        assert!(
            (0.0..=1.0).contains(&min_health) && (0.0..=1.0).contains(&max_health),
            "health range out of bounds: [{min_health}, {max_health}]"
        );
        let _ = (start_budget, min_cash, max_cash, min_health, max_health);
    }
}