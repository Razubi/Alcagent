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

    /// Short UI label for the state.
    pub fn label(self) -> &'static str {
        match self {
            FinanceHealthState::Healthy => "Healthy",
            FinanceHealthState::Watch => "Watch",
            FinanceHealthState::Tight => "Tight",
            FinanceHealthState::Critical => "Critical",
            FinanceHealthState::Receivership => "Receivership",
        }
    }

    /// Plain-language description of what the state means for the institution.
    pub fn description(self) -> &'static str {
        match self {
            FinanceHealthState::Healthy => {
                "Cash comfortably covers operating expenses; no constraints."
            }
            FinanceHealthState::Watch => {
                "Finances are stable but worth monitoring; avoid discretionary spending."
            }
            FinanceHealthState::Tight => {
                "Cash is strained; defer non-essential construction and hiring."
            }
            FinanceHealthState::Critical => {
                "Cash is critically low; emergency expense reduction required."
            }
            FinanceHealthState::Receivership => {
                "Insolvent; the institution is under receivership and cannot spend freely."
            }
        }
    }

    /// Pure (no-hysteresis) classification from a health score and reserve ratio.
    ///
    /// Useful for snapshots where the hysteresis-bounded `update_state` is not
    /// desired; `update_state` remains the authoritative in-year transition.
    pub fn classify(health_score: f64, reserve_ratio: f64) -> Self {
        let h = health_score.clamp(0.0, 1.0);
        let r = reserve_ratio.max(0.0);
        if h < 0.06 || r < 0.01 {
            FinanceHealthState::Receivership
        } else if h < 0.18 || r < 0.05 {
            FinanceHealthState::Critical
        } else if h < 0.35 || r < 0.10 {
            FinanceHealthState::Tight
        } else if h < 0.55 || r < 0.20 {
            FinanceHealthState::Watch
        } else {
            FinanceHealthState::Healthy
        }
    }
}

/// Per-line variance detail for the UI finance view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineVariance {
    pub label: String,
    /// Revenue or expense.
    pub kind: BudgetLineKind,
    /// Forecast amount in minor units (positive magnitude).
    pub forecast_minor: i64,
    /// Actual amount to date in minor units (positive magnitude).
    pub actual_minor: i64,
    /// Actual minus forecast. Positive = overspend for expenses, overperformance for revenue.
    pub variance_minor: i64,
    /// Variance divided by forecast (0 when forecast is 0).
    pub variance_ratio: f64,
}

/// Direction of a budget line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BudgetLineKind {
    #[default]
    Revenue,
    Expense,
}

/// Forecast and actuals for a budget line/category bucket.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetLine {
    pub label: String,
    /// Revenue or expense.
    #[serde(default)]
    pub kind: BudgetLineKind,
    /// Forecast amount in minor units for the period (positive magnitude).
    pub forecast_minor: i64,
    /// Actual amount in minor units for the period to date (positive magnitude).
    pub actual_minor: i64,
}

impl BudgetLine {
    pub fn new(label: impl Into<String>, forecast: Money, kind: BudgetLineKind) -> Self {
        Self {
            label: label.into(),
            kind,
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
                BudgetLine::new(
                    "Per-capita funding",
                    Money::from_major(1_200_000),
                    BudgetLineKind::Revenue,
                ),
                BudgetLine::new(
                    "Grants & supplements",
                    Money::from_major(150_000),
                    BudgetLineKind::Revenue,
                ),
                BudgetLine::new(
                    "Contracts & programs",
                    Money::from_major(120_000),
                    BudgetLineKind::Revenue,
                ),
            ],
            expense_forecast: vec![
                BudgetLine::new(
                    "Staff salaries",
                    Money::from_major(900_000),
                    BudgetLineKind::Expense,
                ),
                BudgetLine::new(
                    "Food & water",
                    Money::from_major(180_000),
                    BudgetLineKind::Expense,
                ),
                BudgetLine::new(
                    "Utilities & supplies",
                    Money::from_major(120_000),
                    BudgetLineKind::Expense,
                ),
                BudgetLine::new(
                    "Medical & welfare",
                    Money::from_major(90_000),
                    BudgetLineKind::Expense,
                ),
                BudgetLine::new(
                    "Maintenance & security",
                    Money::from_major(80_000),
                    BudgetLineKind::Expense,
                ),
                BudgetLine::new(
                    "Programs & education",
                    Money::from_major(70_000),
                    BudgetLineKind::Expense,
                ),
                BudgetLine::new(
                    "Administration & transport",
                    Money::from_major(50_000),
                    BudgetLineKind::Expense,
                ),
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

    /// Revenue variance ratio to date: positive means overperforming vs forecast.
    pub fn revenue_variance_ratio(&self) -> f64 {
        let f = self.total_revenue_forecast().0 as f64;
        if f <= 0.0 {
            0.0
        } else {
            (self.total_revenue_actual().0 - self.total_revenue_forecast().0) as f64 / f
        }
    }

    /// Overall net variance ratio (actual net - forecast net) divided by forecast net magnitude.
    pub fn net_variance_ratio(&self) -> f64 {
        let f = self.forecast_net_minor().abs() as f64;
        if f <= 0.0 {
            0.0
        } else {
            self.net_variance_minor() as f64 / f
        }
    }

    /// Project year-end net from the current actuals run rate.
    ///
    /// `days_elapsed` is the number of days into the fiscal year to date.
    /// Returns `None` when no days have elapsed (no run rate can be derived).
    pub fn projected_net_at_year_end(&self, days_elapsed: u32) -> Option<Money> {
        if days_elapsed == 0 {
            return None;
        }
        let run_rate = self.actual_net_minor() as f64 / days_elapsed as f64;
        let projected = run_rate * 365.0;
        Some(Money(projected.round() as i64))
    }

    /// Per-line variance detail, sorted by absolute variance descending.
    pub fn line_variances(&self) -> Vec<LineVariance> {
        let mut lines: Vec<LineVariance> = self
            .revenue_forecast
            .iter()
            .map(|l| LineVariance {
                label: l.label.clone(),
                kind: l.kind,
                forecast_minor: l.forecast_minor,
                actual_minor: l.actual_minor,
                variance_minor: l.variance_minor(),
                variance_ratio: l.variance_ratio(),
            })
            .chain(self.expense_forecast.iter().map(|l| LineVariance {
                label: l.label.clone(),
                kind: l.kind,
                forecast_minor: l.forecast_minor,
                actual_minor: l.actual_minor,
                variance_minor: l.variance_minor(),
                variance_ratio: l.variance_ratio(),
            }))
            .collect();
        lines.sort_by(|a, b| b.variance_minor.abs().cmp(&a.variance_minor.abs()));
        lines
    }

    /// Plain-language explanation of the overall budget variance position.
    pub fn variance_explanation(&self) -> String {
        let net = self.net_variance_minor();
        let exp_ratio = self.expense_variance_ratio();
        let rev_ratio = self.revenue_variance_ratio();
        let projected = self
            .projected_net_at_year_end(30)
            .map(|m| m.as_major())
            .unwrap_or(0.0);
        let direction = if net > 0 {
            "ahead of plan"
        } else if net < 0 {
            "behind plan"
        } else {
            "on plan"
        };
        format!(
            "Budget is {direction}: net variance ${:.0} (expense variance {exp_ratio:.0}%, revenue variance {rev_ratio:.0}%). Projected year-end net at the current 30-day run rate: ${:.0}.",
            net as f64 / 100.0,
            projected
        )
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
