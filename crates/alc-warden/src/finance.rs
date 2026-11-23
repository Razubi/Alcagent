//! Warden-facing finance planning: annual budget, forecasts, variance, and health states.

use alc_economy::Money;
use serde::{Deserialize, Serialize};

/// Hysteresis-bounded financial health states for institutional planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinanceHealthState {
    Healthy,
    Watch,
    Tight,
    Critical,
    Receivership,
}

impl FinanceHealthState {
    /// Rank for comparison (higher = healthier).
    pub fn rank(self) -> i32 {
        match self {
            FinanceHealthState::Receivership => 0,
            FinanceHealthState::Critical => 1,
            FinanceHealthState::Tight => 2,
            FinanceHealthState::Watch => 3,
            FinanceHealthState::Healthy => 4,
        }
    }
}

/// Forecast and actuals for a budget line/category bucket.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetLine {
    pub label: String,
    /// Forecast amount in minor units for the period (positive magnitude).
    pub forecast_minor: i64,
    /// Actual amount in minor units for the period to date (positive magnitude).
    pub actual_minor: i64,
}

impl BudgetLine {
    pub fn new(label: impl Into<String>, forecast: Money) -> Self {
        Self {
            label: label.into(),
            forecast_minor: forecast.0.max(0),
            actual_minor: 0,
        }
    }

    pub fn variance_minor(&self) -> i64 {
        self.actual_minor - self.forecast_minor
    }

    /// Positive means over/under relative to forecast magnitude depending on context;
    /// for expenses positive variance = overspend, for revenue positive = overperformance.
    pub fn variance_ratio(&self) -> f64 {
        if self.forecast_minor == 0 {
            if self.actual_minor == 0 { 0.0 } else { 1.0 }
        } else {
            self.variance_minor() as f64 / self.forecast_minor as f64
        }
    }

    pub fn record_actual(&mut self, amount: Money) {
        self.actual_minor = self.actual_minor.saturating_add(amount.0.max(0));
    }
}

/// Annual budget plan with revenue/expense forecasts and variance tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnualBudgetPlan {
    pub fiscal_year_label: String,
    pub revenue_forecast: Vec<BudgetLine>,
    pub expense_forecast: Vec<BudgetLine>,
    /// Opening reserve/cash assumption (minor units) for planning context.
    pub opening_reserve_minor: i64,
    pub notes: Vec<String>,
    pub state: FinanceHealthState,
}

impl Default for AnnualBudgetPlan {
    fn default() -> Self {
        Self::new("FY-1", Money::from_major(250_000))
    }
}

impl AnnualBudgetPlan {
    pub fn new(fiscal_year_label: impl Into<String>, opening_reserve: Money) -> Self {
        Self {
            fiscal_year_label: fiscal_year_label.into(),
            revenue_forecast: vec![
                BudgetLine::new("Per-capita funding", Money::from_major(1_200_000)),
                BudgetLine::new("Grants & supplements", Money::from_major(150_000)),
                BudgetLine::new("Contracts & programs", Money::from_major(120_000)),
            ],
            expense_forecast: vec![
                BudgetLine::new("Staff salaries", Money::from_major(900_000)),
                BudgetLine::new("Food & water", Money::from_major(180_000)),
                BudgetLine::new("Utilities & supplies", Money::from_major(120_000)),
                BudgetLine::new("Medical & welfare", Money::from_major(90_000)),
                BudgetLine::new("Maintenance & security", Money::from_major(80_000)),
                BudgetLine::new("Programs & education", Money::from_major(70_000)),
                BudgetLine::new("Administration & transport", Money::from_major(50_000)),
            ],
            opening_reserve_minor: opening_reserve.0,
            notes: Vec::new(),
            state: FinanceHealthState::Healthy,
        }
    }

    pub fn total_revenue_forecast(&self) -> Money {
        Money(self.revenue_forecast.iter().map(|l| l.forecast_minor).sum())
    }

    pub fn total_expense_forecast(&self) -> Money {
        Money(self.expense_forecast.iter().map(|l| l.forecast_minor).sum())
    }

    pub fn total_revenue_actual(&self) -> Money {
        Money(self.revenue_forecast.iter().map(|l| l.actual_minor).sum())
    }

    pub fn total_expense_actual(&self) -> Money {
        Money(self.expense_forecast.iter().map(|l| l.actual_minor).sum())
    }

    /// Forecast net (revenue - expense) in minor units.
    pub fn forecast_net_minor(&self) -> i64 {
        self.total_revenue_forecast().0 - self.total_expense_forecast().0
    }

    /// Actual net to date (revenue - expense) in minor units.
    pub fn actual_net_minor(&self) -> i64 {
        self.total_revenue_actual().0 - self.total_expense_actual().0
    }

    /// Overall net variance (actual net - forecast net).
    pub fn net_variance_minor(&self) -> i64 {
        self.actual_net_minor() - self.forecast_net_minor()
    }

    /// Expense variance ratio to date: positive means overspending vs forecast.
    pub fn expense_variance_ratio(&self) -> f64 {
        let f = self.total_expense_forecast().0 as f64;
        if f <= 0.0 {
            0.0
        } else {
            (self.total_expense_actual().0 - self.total_expense_forecast().0) as f64 / f
        }
    }

    /// Update hysteresis-bounded state from a 0..1 financial health score and reserve ratio.
    ///
    /// `reserve_ratio` is cash or reserve divided by expected annual/period burn proxy (0..inf).
    /// Thresholds differ for worsening vs improving transitions to avoid flapping.
    ///
    /// Transitions are monotonic within a fiscal year: the state may worsen but never
    /// improve until a new fiscal year/plan resets it.
    pub fn update_state(&mut self, health_score: f64, reserve_ratio: f64) {
        let h = health_score.clamp(0.0, 1.0);
        let r = reserve_ratio.max(0.0);
        let current = self.state;

        // Determine candidate when conditions worsen or improve with hysteresis bands.
        let candidate = match current {
            FinanceHealthState::Healthy => {
                if h < 0.55 || r < 0.20 {
                    FinanceHealthState::Watch
                } else {
                    FinanceHealthState::Healthy
                }
            }
            FinanceHealthState::Watch => {
                if h < 0.35 || r < 0.10 {
                    FinanceHealthState::Tight
                } else if h >= 0.70 && r >= 0.30 {
                    FinanceHealthState::Healthy
                } else {
                    FinanceHealthState::Watch
                }
            }
            FinanceHealthState::Tight => {
                if h < 0.18 || r < 0.05 {
                    FinanceHealthState::Critical
                } else if h >= 0.50 && r >= 0.18 {
                    FinanceHealthState::Watch
                } else {
                    FinanceHealthState::Tight
                }
            }
            FinanceHealthState::Critical => {
                if h < 0.06 || r < 0.01 {
                    FinanceHealthState::Receivership
                } else if h >= 0.32 && r >= 0.10 {
                    FinanceHealthState::Tight
                } else {
                    FinanceHealthState::Critical
                }
            }
            FinanceHealthState::Receivership => {
                // Recovery requires sustained improvement above critical exit band.
                if h >= 0.22 && r >= 0.06 {
                    FinanceHealthState::Critical
                } else {
                    FinanceHealthState::Receivership
                }
            }
        };

        // Monotonic within a fiscal year: never allow improvement in health rank.
        self.state = if candidate.rank() <= current.rank() {
            candidate
        } else {
            current
        };
    }

    pub fn push_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }
}
