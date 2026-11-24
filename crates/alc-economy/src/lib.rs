//! Prison economy: integer-minor-unit ledger, revenue, expenses, planning.
//!
//! Money is stored as integer minor units (cents). All rates are configurable
//! per Directive §11. Degradation curves replace arbitrary instant penalties:
//! neglected systems decay over time and accrue interest.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// Direction of a budget line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetLineKind {
    Revenue,
    Expense,
}

/// One account's total within a budget breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryLine {
    pub account: Account,
    pub label: String,
    pub kind: BudgetLineKind,
    /// Magnitude in minor units (positive). For expenses this is the spend
    /// magnitude; for revenue it is the inflow.
    pub amount: Money,
}

/// A finance warning produced from a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinanceWarning {
    pub kind: WarningKind,
    pub explanation: String,
}

/// Kind of finance warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningKind {
    LowCash,
    NegativeRunway,
    DeficitSpending,
    CrowdingPenalty,
    HighConstructionCosts,
    HighMedicalCosts,
    HighRehabCosts,
}

/// A point-in-time budget snapshot computed deterministically from the ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    pub as_of_secs: u64,
    pub cash: Money,
    pub income_by_category: Vec<CategoryLine>,
    pub expenses_by_category: Vec<CategoryLine>,
    pub total_income: Money,
    pub total_expenses: Money,
    pub net: Money,
    pub trailing_daily_expense_avg: i64,
    pub trailing_daily_income_avg: i64,
    pub net_daily_burn: i64,
    pub runway_days: f64,
    pub health: f64,
    pub warnings: Vec<FinanceWarning>,
}

impl Account {
    /// All accounts in canonical order.
    pub const ALL: [Account; 20] = [
        Account::PerCapitaFunding,
        Account::GovGrant,
        Account::Bonus,
        Account::Penalty,
        Account::ContractIncome,
        Account::RehabFunding,
        Account::SpecialProgram,
        Account::Food,
        Account::Water,
        Account::Utilities,
        Account::Salaries,
        Account::Medical,
        Account::Maintenance,
        Account::Construction,
        Account::SecurityEquipment,
        Account::Education,
        Account::Rehabilitation,
        Account::Transportation,
        Account::Supplies,
        Account::Admin,
    ];

    /// Human-readable label for the account.
    pub fn label(self) -> &'static str {
        match self {
            Account::PerCapitaFunding => "Per-capita funding",
            Account::GovGrant => "Government grant",
            Account::Bonus => "Bonus",
            Account::Penalty => "Penalty",
            Account::ContractIncome => "Contract income",
            Account::RehabFunding => "Rehabilitation funding",
            Account::SpecialProgram => "Special program",
            Account::Food => "Food",
            Account::Water => "Water",
            Account::Utilities => "Utilities",
            Account::Salaries => "Salaries",
            Account::Medical => "Medical",
            Account::Maintenance => "Maintenance",
            Account::Construction => "Construction",
            Account::SecurityEquipment => "Security equipment",
            Account::Education => "Education",
            Account::Rehabilitation => "Rehabilitation",
            Account::Transportation => "Transportation",
            Account::Supplies => "Supplies",
            Account::Admin => "Administration",
        }
    }

    /// Whether the account represents revenue (inflow) or expense (outflow).
    pub fn kind(self) -> BudgetLineKind {
        match self {
            Account::PerCapitaFunding
            | Account::GovGrant
            | Account::Bonus
            | Account::ContractIncome
            | Account::RehabFunding
            | Account::SpecialProgram => BudgetLineKind::Revenue,
            Account::Penalty
            | Account::Food
            | Account::Water
            | Account::Utilities
            | Account::Salaries
            | Account::Medical
            | Account::Maintenance
            | Account::Construction
            | Account::SecurityEquipment
            | Account::Education
            | Account::Rehabilitation
            | Account::Transportation
            | Account::Supplies
            | Account::Admin => BudgetLineKind::Expense,
        }
    }

    pub fn is_revenue(self) -> bool {
        self.kind() == BudgetLineKind::Revenue
    }

    pub fn is_expense(self) -> bool {
        self.kind() == BudgetLineKind::Expense
    }
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

    /// Spend cash on a construction/expense item. Returns amount spent.
    pub fn spend(&mut self, at_secs: u64, amount: Money, account: Account, note: impl Into<String>) -> Money {
        let magnitude = amount.0.max(0);
        let outflow = Money(-magnitude);
        self.record(at_secs, account, outflow, note);
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

    /// Sum of signed ledger amounts for `account`; optionally only entries
    /// within the trailing `window_secs` before `as_of_secs`.
    pub fn category_total(
        &self,
        account: Account,
        as_of_secs: u64,
        window_secs: Option<u64>,
    ) -> Money {
        let mut total = 0i64;
        for entry in &self.ledger {
            if entry.account != account {
                continue;
            }
            if let Some(w) = window_secs {
                if entry.at_secs > as_of_secs || as_of_secs - entry.at_secs > w {
                    continue;
                }
            }
            total = total.saturating_add(entry.amount.0);
        }
        Money(total)
    }

    /// Magnitude (absolute value) of the signed total for `account`.
    pub fn category_magnitude(
        &self,
        account: Account,
        as_of_secs: u64,
        window_secs: Option<u64>,
    ) -> Money {
        Money(self.category_total(account, as_of_secs, window_secs).0.abs())
    }

    /// Revenue accounts only, positive magnitudes, sorted largest first.
    /// `window_secs` restricts to the trailing window before `as_of_secs`;
    /// `None` uses the whole ledger.
    pub fn income_by_category(
        &self,
        as_of_secs: u64,
        window_secs: Option<u64>,
    ) -> Vec<CategoryLine> {
        let mut lines: Vec<CategoryLine> = Vec::new();
        for acct in Account::ALL {
            if !acct.is_revenue() {
                continue;
            }
            let total = self.category_total(acct, as_of_secs, window_secs);
            if total.0 > 0 {
                lines.push(CategoryLine {
                    account: acct,
                    label: acct.label().to_string(),
                    kind: BudgetLineKind::Revenue,
                    amount: Money(total.0),
                });
            }
        }
        lines.sort_by(|a, b| b.amount.0.cmp(&a.amount.0));
        lines
    }

    /// Expense accounts only, positive magnitudes, sorted largest first.
    /// `window_secs` restricts to the trailing window before `as_of_secs`;
    /// `None` uses the whole ledger.
    pub fn expenses_by_category(
        &self,
        as_of_secs: u64,
        window_secs: Option<u64>,
    ) -> Vec<CategoryLine> {
        let mut lines: Vec<CategoryLine> = Vec::new();
        for acct in Account::ALL {
            if !acct.is_expense() {
                continue;
            }
            let total = self.category_total(acct, as_of_secs, window_secs);
            if total.0 < 0 {
                lines.push(CategoryLine {
                    account: acct,
                    label: acct.label().to_string(),
                    kind: BudgetLineKind::Expense,
                    amount: Money((-total.0).max(0)),
                });
            }
        }
        lines.sort_by(|a, b| b.amount.0.cmp(&a.amount.0));
        lines
    }

    /// Construction spending magnitude in the window.
    pub fn construction_costs(
        &self,
        as_of_secs: u64,
        window_secs: Option<u64>,
    ) -> Money {
        self.category_magnitude(Account::Construction, as_of_secs, window_secs)
    }

    /// Medical spending magnitude in the window.
    pub fn medical_costs(
        &self,
        as_of_secs: u64,
        window_secs: Option<u64>,
    ) -> Money {
        self.category_magnitude(Account::Medical, as_of_secs, window_secs)
    }

    /// Rehabilitation spending magnitude in the window.
    pub fn rehabilitation_costs(
        &self,
        as_of_secs: u64,
        window_secs: Option<u64>,
    ) -> Money {
        self.category_magnitude(Account::Rehabilitation, as_of_secs, window_secs)
    }

    /// 30-day trailing average of daily income (minor units), zero when there
    /// is no income history.
    pub fn trailing_daily_income_avg(&self) -> i64 {
        let as_of = self.ledger.last().map(|e| e.at_secs).unwrap_or(0);
        self.trailing_daily_income_avg_at(as_of)
    }

    fn trailing_daily_income_avg_at(&self, as_of_secs: u64) -> i64 {
        let window = 30 * 86_400;
        let mut day_sums: BTreeMap<u64, i64> = BTreeMap::new();
        for entry in &self.ledger {
            if !entry.account.is_revenue() || entry.amount.0 <= 0 {
                continue;
            }
            if entry.at_secs > as_of_secs || as_of_secs - entry.at_secs > window {
                continue;
            }
            let day = entry.at_secs / 86_400;
            *day_sums.entry(day).or_insert(0) =
                day_sums.get(&day).copied().unwrap_or(0).saturating_add(entry.amount.0);
        }
        if day_sums.is_empty() {
            return 0;
        }
        let sum: i64 = day_sums.values().copied().sum();
        sum / day_sums.len() as i64
    }

    /// Net daily burn in minor units: trailing expense average minus income
    /// average. Positive means spending more than coming in.
    pub fn net_daily_burn(&self) -> i64 {
        self.trailing_daily_expense_avg().saturating_sub(self.trailing_daily_income_avg())
    }

    /// Estimated runway in days: cash divided by net daily burn (at least 1
    /// minor unit/day divisor). Zero when cash is non-positive.
    pub fn runway_days(&self) -> f64 {
        if self.cash.0 <= 0 {
            return 0.0;
        }
        self.cash.0 as f64 / self.net_daily_burn().max(1) as f64
    }

    /// Full budget snapshot at `as_of_secs`, computed deterministically from
    /// the ledger. `window_secs` restricts category totals to a trailing
    /// window; `None` uses the whole ledger.
    pub fn budget_snapshot(
        &self,
        as_of_secs: u64,
        window_secs: Option<u64>,
    ) -> BudgetSnapshot {
        let income = self.income_by_category(as_of_secs, window_secs);
        let expenses = self.expenses_by_category(as_of_secs, window_secs);
        let total_income = Money(income.iter().map(|l| l.amount.0).sum::<i64>());
        let total_expenses = Money(expenses.iter().map(|l| l.amount.0).sum::<i64>());
        let net = Money(total_income.0 - total_expenses.0);
        let expense_avg = self.trailing_daily_expense_avg();
        let income_avg = self.trailing_daily_income_avg();
        let net_burn = expense_avg.saturating_sub(income_avg);
        let runway = if self.cash.0 <= 0 {
            0.0
        } else {
            self.cash.0 as f64 / net_burn.max(1) as f64
        };
        let health = self.health();
        let warnings = self.warnings(as_of_secs, window_secs);
        BudgetSnapshot {
            as_of_secs,
            cash: self.cash,
            income_by_category: income,
            expenses_by_category: expenses,
            total_income,
            total_expenses,
            net,
            trailing_daily_expense_avg: expense_avg,
            trailing_daily_income_avg: income_avg,
            net_daily_burn: net_burn,
            runway_days: runway,
            health,
            warnings,
        }
    }

    /// Plain-language explanation of the current financial health score.
    pub fn health_explanation(&self) -> String {
        let health = self.health();
        let cash = self.cash.as_major();
        let avg = self.trailing_daily_expense_avg() as f64 / 100.0;
        let runway = self.runway_days();
        let base = format!(
            "Financial health is {health:.2} (cash ${cash:.0}, {runway:.0} days of runway \
             at ${avg:.0}/day typical expense)."
        );
        match health {
            h if h >= 0.9 => format!("{base} Comfortable position."),
            h if h >= 0.6 => format!("{base} Adequate runway; monitor spending."),
            h if h >= 0.35 => {
                format!("{base} Tight; consider reducing expenses or securing funding.")
            }
            h if h > 0.0 => format!("{base} Critical; near insolvency. Immediate action required."),
            _ => format!("{base} Insolvent; negative cash."),
        }
    }

    /// Finance warnings derived from the current ledger state.
    pub fn warnings(
        &self,
        as_of_secs: u64,
        window_secs: Option<u64>,
    ) -> Vec<FinanceWarning> {
        let mut out: Vec<FinanceWarning> = Vec::new();
        let expense_avg = self.trailing_daily_expense_avg();
        let income_avg = self.trailing_daily_income_avg();
        let net_burn = expense_avg.saturating_sub(income_avg);
        let cash = self.cash.0;

        if cash < 0 {
            out.push(FinanceWarning {
                kind: WarningKind::NegativeRunway,
                explanation: format!(
                    "Cash is negative (${:.0}); the prison is insolvent.",
                    self.cash.as_major()
                ),
            });
        } else if cash < expense_avg * 7 {
            out.push(FinanceWarning {
                kind: WarningKind::LowCash,
                explanation: format!(
                    "Cash covers less than one week of typical expenses (${:.0}).",
                    self.cash.as_major()
                ),
            });
        }

        if net_burn > 0 {
            let runway = if cash <= 0 {
                0.0
            } else {
                cash as f64 / net_burn as f64
            };
            if runway < 30.0 {
                out.push(FinanceWarning {
                    kind: WarningKind::DeficitSpending,
                    explanation: format!(
                        "Expenses exceed income by ${:.0}/day; runway is {runway:.0} days.",
                        net_burn as f64 / 100.0
                    ),
                });
            }
        }

        let material = |amount: Money, label: &str| -> Option<FinanceWarning> {
            if amount.0 > 0 && amount.0 > expense_avg * 3 {
                Some(FinanceWarning {
                    kind: match label {
                        "medical" => WarningKind::HighMedicalCosts,
                        "construction" => WarningKind::HighConstructionCosts,
                        "rehabilitation" => WarningKind::HighRehabCosts,
                        _ => WarningKind::DeficitSpending,
                    },
                    explanation: format!("{label} spending is ${:.0} in the window.", amount.as_major()),
                })
            } else {
                None
            }
        };

        if let Some(w) = material(self.medical_costs(as_of_secs, window_secs), "medical") {
            out.push(w);
        }
        if let Some(w) = material(
            self.construction_costs(as_of_secs, window_secs),
            "construction",
        ) {
            out.push(w);
        }
        if let Some(w) = material(
            self.rehabilitation_costs(as_of_secs, window_secs),
            "rehabilitation",
        ) {
            out.push(w);
        }

        out
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

    #[test]
    fn income_and_expenses_by_category_split_correctly() {
        let mut e = Economy::default();
        let at = 100_000u64;
        e.record(at, Account::GovGrant, Money::from_major(350), "grant");
        e.record(at, Account::PerCapitaFunding, Money::from_major(45 * 24), "per capita");
        e.record(at, Account::Food, Money::from_major(4 * 24).mul(-1.0), "meals");
        e.record(at, Account::Salaries, Money::from_major(140 * 4).mul(-1.0), "payroll");
        e.record(at, Account::Construction, Money::from_major(5_000).mul(-1.0), "new wing");

        let inc = e.income_by_category(at, None);
        assert!(inc.iter().any(|l| l.account == Account::GovGrant));
        assert!(inc.iter().all(|l| matches!(l.kind, BudgetLineKind::Revenue)));

        let exp = e.expenses_by_category(at, None);
        assert!(exp.iter().any(|l| l.account == Account::Construction));
        assert!(exp.iter().all(|l| matches!(l.kind, BudgetLineKind::Expense)));
        assert_eq!(
            e.construction_costs(at, None),
            Money::from_major(5_000),
            "construction costs should equal the recorded magnitude"
        );
        assert_eq!(
            e.medical_costs(at, None),
            Money::ZERO,
            "no medical spending recorded"
        );
        assert_eq!(
            e.rehabilitation_costs(at, None),
            Money::ZERO,
            "no rehab spending recorded"
        );
    }

    #[test]
    fn window_restricts_category_totals() {
        let mut e = Economy::default();
        let early = 1000u64;
        let late = 200_000u64;
        e.record(early, Account::Food, Money::from_major(100).mul(-1.0), "old meals");
        e.record(late, Account::Food, Money::from_major(400).mul(-1.0), "new meals");

        let all = e.category_magnitude(Account::Food, late, None);
        assert_eq!(all, Money::from_major(500));

        // Window of one day before `late` excludes the early entry.
        let window = e.category_magnitude(Account::Food, late, Some(86_400));
        assert_eq!(window, Money::from_major(400));
    }

    #[test]
    fn snapshot_runway_and_warnings_are_deterministic() {
        let mut e = Economy::default();
        e.cash = Money::from_major(100_000);
        e.recent_daily_expenses = vec![10_000]; // $100/day typical expense
        e.record(1000, Account::GovGrant, Money::from_major(350), "grant");
        // $40,000 medical > 3 days of typical expense ($30,000) -> material warning.
        e.record(1000, Account::Medical, Money::from_major(40_000).mul(-1.0), "medical");

        let snap = e.budget_snapshot(2000, None);
        assert!((0.0..=1.0).contains(&snap.health));
        assert!(snap.runway_days.is_finite());
        assert_eq!(snap.trailing_daily_expense_avg, 10_000);
        assert!(e.medical_costs(2000, None) == Money::from_major(40_000));

        let snap2 = e.budget_snapshot(2000, None);
        assert_eq!(snap.warnings.len(), snap2.warnings.len()); // deterministic
        assert!(snap
            .warnings
            .iter()
            .any(|w| matches!(w.kind, WarningKind::HighMedicalCosts)));
    }

    #[test]
    fn negative_cash_produces_insolvency_warning() {
        let mut e = Economy::default();
        e.cash = Money(-1);
        e.recent_daily_expenses = vec![1_000_000];
        let snap = e.budget_snapshot(1000, None);
        assert_eq!(snap.health, 0.0);
        assert_eq!(snap.runway_days, 0.0);
        assert!(snap
            .warnings
            .iter()
            .any(|w| matches!(w.kind, WarningKind::NegativeRunway)));
        assert!(!e.health_explanation().is_empty());
    }

    #[test]
    fn account_labels_and_kinds_are_complete() {
        for acct in Account::ALL {
            let _ = acct.label();
            let _ = acct.kind();
            let _ = acct.is_revenue();
            let _ = acct.is_expense();
        }
        assert_eq!(Account::ALL.len(), 20);
    }
}