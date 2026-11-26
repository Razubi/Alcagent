//! Warden AI per Directive §9.

pub mod expansion;
pub mod finance;

pub use expansion::AttentionBudget;

use alc_core::{RngStream, SimTime, WardenId};
use alc_economy::Economy;
use serde::{Deserialize, Serialize};

use crate::expansion::find_building_spec;
pub use crate::finance::FinanceHealthState;

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
            strictness: r(rng),
            compassion: r(rng),
            political_ambition: r(rng),
            financial_discipline: r(rng),
            risk_tolerance: r(rng),
            reform_orientation: r(rng),
            authoritarian_tendencies: r(rng),
            staff_loyalty: r(rng),
            corruption_susceptibility: r(rng),
            reputation_concerns: r(rng),
            competence: r(rng),
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

impl Objective {
    /// All objectives in canonical reporting order.
    pub const ALL: [Objective; 11] = [
        Objective::Security,
        Objective::FinancialSustainability,
        Objective::StaffMorale,
        Objective::PrisonerWelfare,
        Objective::PublicReputation,
        Objective::InstitutionalStability,
        Objective::Rehabilitation,
        Objective::EscapePrevention,
        Objective::ViolenceReduction,
        Objective::PoliticalPressure,
        Objective::FacilityExpansion,
    ];
}

/// Action the warden can take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WardenAction {
    Expand {
        building: String,
    },
    HireGuards {
        count: u32,
    },
    FireGuards {
        count: u32,
    },
    PromoteStaff {
        guard: u64,
    },
    ChangePolicy {
        policy: String,
        value: f64,
    },
    IncreaseSecurity,
    ReduceExpenses,
    ImproveInfrastructure {
        facility: u64,
    },
    RespondToDisturbance {
        event_id: u64,
    },
    NegotiateWithRepresentatives,
    InvestigateMisconduct {
        target: u64,
    },
    AdjustParoleRecommendation {
        prisoner: u64,
        lean: f32,
    },
    RequestFunding {
        amount_major: i64,
    },
    /// Explicit "hold steady" — a first-class decision (design §5.5).
    NoAction {
        rationale: String,
    },
}

impl WardenAction {
    /// Short UI label for the action.
    pub fn label(&self) -> &'static str {
        match self {
            WardenAction::Expand { .. } => "Expand facility",
            WardenAction::HireGuards { .. } => "Hire guards",
            WardenAction::FireGuards { .. } => "Fire guards",
            WardenAction::PromoteStaff { .. } => "Promote staff",
            WardenAction::ChangePolicy { .. } => "Change policy",
            WardenAction::IncreaseSecurity => "Increase security",
            WardenAction::ReduceExpenses => "Reduce expenses",
            WardenAction::ImproveInfrastructure { .. } => "Improve infrastructure",
            WardenAction::RespondToDisturbance { .. } => "Respond to disturbance",
            WardenAction::NegotiateWithRepresentatives => "Negotiate with representatives",
            WardenAction::InvestigateMisconduct { .. } => "Investigate misconduct",
            WardenAction::AdjustParoleRecommendation { .. } => "Adjust parole recommendation",
            WardenAction::RequestFunding { .. } => "Request funding",
            WardenAction::NoAction { .. } => "Hold steady",
        }
    }

    /// Default audit reason string for the action (used when no custom reason
    /// is supplied). Keeps arbitration itself unchanged.
    pub fn default_reason(&self) -> &'static str {
        match self {
            WardenAction::Expand { .. } => {
                // Callers can format the building name dynamically if needed.
                "Expand facility to relieve crowding"
            }
            WardenAction::HireGuards { .. } => "Increase guard staffing",
            WardenAction::FireGuards { .. } => "Reduce guard staffing",
            WardenAction::PromoteStaff { .. } => "Recognize strong staff performance",
            WardenAction::ChangePolicy { .. } => "Adjust institutional policy",
            WardenAction::IncreaseSecurity => "Raise security posture after incidents",
            WardenAction::ReduceExpenses => "Cut spending to protect solvency",
            WardenAction::ImproveInfrastructure { .. } => "Repair and upgrade facilities",
            WardenAction::RespondToDisturbance { .. } => "Respond to an active disturbance",
            WardenAction::NegotiateWithRepresentatives => "Engage prisoner representatives",
            WardenAction::InvestigateMisconduct { .. } => {
                "Investigate staff or prisoner misconduct"
            }
            WardenAction::AdjustParoleRecommendation { .. } => "Adjust parole recommendation",
            WardenAction::RequestFunding { .. } => "Request additional government funding",
            WardenAction::NoAction { .. } => "Defer decision; no action required",
        }
    }

    /// Attention cost in points (see [`AttentionBudget`]). Used by control
    /// panels to show whether the warden can afford the action.
    pub fn attention_cost(&self) -> f32 {
        match self {
            WardenAction::NoAction { .. } => 0.0,
            WardenAction::RequestFunding { .. } => AttentionBudget::strategic_cost(),
            WardenAction::Expand { .. }
            | WardenAction::HireGuards { .. }
            | WardenAction::FireGuards { .. } => AttentionBudget::strategic_cost(),
            _ => AttentionBudget::tactical_cost(),
        }
    }

    /// Whether the action is an emergency/security response that bypasses
    /// normal attention gating in arbitration.
    pub fn is_emergency(&self) -> bool {
        matches!(
            self,
            WardenAction::IncreaseSecurity
                | WardenAction::ReduceExpenses
                | WardenAction::RespondToDisturbance { .. }
        )
    }
}

/// Source of a warden decision, for auditability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionSource {
    AiArbitration,
    PlayerDirective,
    Council,
    System,
}

impl DecisionSource {
    /// Short observer-facing label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            DecisionSource::AiArbitration => "AI",
            DecisionSource::PlayerDirective => "Observer",
            DecisionSource::Council => "Council",
            DecisionSource::System => "System",
        }
    }
}

/// One audited warden decision. Recorded by [`Warden::record_decision`];
/// arbitration itself is unchanged and never writes to the log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WardenDecisionRecord {
    pub id: u64,
    pub time: SimTime,
    pub warden_id: WardenId,
    pub warden_name: String,
    pub action: WardenAction,
    /// Final rationale for the decision.
    #[serde(default)]
    pub rationale: String,
    /// For legacy reasons this field is kept, but [`rationale`] is preferred.
    pub reason: String,
    pub source: DecisionSource,
    /// Outcome state of the decision (accepted/rejected/etc).
    #[serde(default)]
    pub outcome: alc_agents::decision::DecisionOutcomeState,
    pub applied: bool,
    /// If repaired/clamped, the reason for the repair.
    #[serde(default)]
    pub repair_reason: String,
}

/// Append-only audit log of warden decisions, bounded to keep memory stable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WardenAuditLog {
    pub records: Vec<WardenDecisionRecord>,
    next_id: u64,
    #[serde(default = "default_max_audit_records")]
    max_records: usize,
}

fn default_max_audit_records() -> usize {
    10_000
}

impl Default for WardenAuditLog {
    fn default() -> Self {
        Self::with_capacity(default_max_audit_records())
    }
}

impl WardenAuditLog {
    pub fn with_capacity(max_records: usize) -> Self {
        let max_records = max_records.max(1);
        Self {
            records: Vec::with_capacity(max_records.min(100_000)),
            next_id: 1,
            max_records,
        }
    }

    pub fn record(
        &mut self,
        time: SimTime,
        warden_id: WardenId,
        warden_name: impl Into<String>,
        action: WardenAction,
        reason: impl Into<String>,
        source: DecisionSource,
        applied: bool,
    ) -> u64 {
        let r_str = reason.into();
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let record = WardenDecisionRecord {
            id,
            time,
            warden_id,
            warden_name: warden_name.into(),
            action,
            rationale: r_str.clone(),
            reason: r_str,
            source,
            outcome: if applied {
                alc_agents::decision::DecisionOutcomeState::Accepted
            } else {
                alc_agents::decision::DecisionOutcomeState::Rejected
            },
            applied,
            repair_reason: String::new(),
        };
        let cap = self.max_records.max(1);
        while self.records.len() >= cap {
            self.records.remove(0);
        }
        self.records.push(record);
        id
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Most recent `n` records (newest first).
    pub fn recent(&self, n: usize) -> Vec<&WardenDecisionRecord> {
        let start = self.records.len().saturating_sub(n);
        self.records[start..].iter().rev().collect()
    }

    /// Records matching a particular action variant.
    pub fn by_action(&self, action: &WardenAction) -> Vec<&WardenDecisionRecord> {
        self.records
            .iter()
            .filter(|r| std::mem::discriminant(&r.action) == std::mem::discriminant(action))
            .collect()
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.next_id = 1;
    }
}

/// Control-panel snapshot: everything an observer/UI needs to render the
/// warden control surface without mutating state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WardenControlSummary {
    pub attention: f32,
    pub attention_points: f32,
    pub can_decide_tactical: bool,
    pub can_decide_strategic: bool,
    pub concurrent_decisions_remaining_tactical: u32,
    pub pending_actions: usize,
    pub recent_decisions: usize,
    pub security_incidents_this_week: u32,
    pub last_action_day: u64,
    pub financial_health: f64,
}

/// One report the warden actually received. Strategy never sees the raw world
/// behind this signal; credibility filtering happens before it is stored in
/// the inbox.
#[derive(Debug, Clone, PartialEq)]
pub struct WardenReportSignal {
    pub id: u64,
    pub at: SimTime,
    pub summary: String,
    pub severity: u8,
    pub credibility: f32,
    pub requests_assistance: bool,
}

/// Report-only decision context. Built by the engine from observer DTOs so the
/// warden cannot read prisoner internals it was not told about.
#[derive(Debug, Clone)]
pub struct WardenStrategySnapshot {
    pub population: u32,
    pub capacity: u32,
    pub cash_major: f64,
    pub health: f64,
    pub runway_days: f64,
    pub security_incidents_this_week: u32,
    pub attention: f32,
    pub financial_health: FinanceHealthState,
    pub reports: Vec<WardenReportSignal>,
    pub active_construction: u32,
    pub future_capacity: u32,
    pub guard_count: u32,
    pub warnings: Vec<String>,
    /// Highest-credibility guard id available for promotion.
    pub top_guard_id: Option<u64>,
    /// Worst-condition facility id available for infrastructure work.
    pub top_facility_id: Option<u64>,
    /// A prisoner id suitable for parole recommendation adjustment.
    pub parole_candidate: Option<u64>,
}

impl WardenStrategySnapshot {
    /// Recompute the derived finance classification after observer values are
    /// updated. Strategy callers should use [`Self::new`]; this is for tests
    /// and DTO builders that assemble fields incrementally.
    pub fn refresh_finance(&mut self) {
        self.financial_health =
            Self::classify_finance(self.health, self.cash_major, self.runway_days);
    }

    /// Attention expressed in the same point units as [`AttentionBudget`].
    #[must_use]
    pub fn attention_points(&self) -> f32 {
        let base = AttentionBudget::default();
        (self.attention * base.base_refill).clamp(0.0, base.max_points)
    }

    /// Whether a decision of this attention cost fits the remaining budget.
    #[must_use]
    pub fn can_afford_attention(&self, points: f32) -> bool {
        self.attention_points() + f32::EPSILON >= points.max(0.0)
    }

    /// Credible, high-severity pressure the warden can lawfully react to.
    #[must_use]
    pub fn report_pressure(&self) -> u32 {
        self.reports
            .iter()
            .filter(|r| r.severity >= 2 && (r.requests_assistance || r.credibility >= 0.25))
            .map(|r| u32::from(r.severity))
            .sum()
    }

    /// Highest-severity credible report id, used for targeted disturbance
    /// responses. Uses the same predicate as [`Self::report_pressure`] so a
    /// report counted as pressure is always actionable.
    #[must_use]
    pub fn priority_report(&self) -> Option<u64> {
        self.reports
            .iter()
            .filter(|r| r.severity >= 2 && (r.requests_assistance || r.credibility >= 0.25))
            .max_by_key(|r| (r.severity, r.credibility >= 0.25))
            .map(|r| r.id)
    }

    fn classify_finance(health: f64, cash_major: f64, runway_days: f64) -> FinanceHealthState {
        let reserve_ratio = if runway_days.is_finite() && runway_days > 0.0 {
            (runway_days / 365.0).clamp(0.0, 1.0)
        } else {
            (cash_major.max(0.0) / 500_000.0).clamp(0.0, 1.0)
        };
        FinanceHealthState::classify(health, reserve_ratio)
    }

    /// Build a snapshot from plain observer values. Kept here so every caller
    /// classifies finance identically.
    #[must_use]
    pub fn new(
        population: u32,
        capacity: u32,
        cash_major: f64,
        health: f64,
        runway_days: f64,
        security_incidents_this_week: u32,
        attention: f32,
        reports: Vec<WardenReportSignal>,
        active_construction: u32,
        future_capacity: u32,
        guard_count: u32,
        warnings: Vec<String>,
        top_guard_id: Option<u64>,
        top_facility_id: Option<u64>,
        parole_candidate: Option<u64>,
    ) -> Self {
        let financial_health = Self::classify_finance(health, cash_major, runway_days);
        Self {
            population,
            capacity,
            cash_major,
            health,
            runway_days,
            security_incidents_this_week,
            attention,
            financial_health,
            reports,
            active_construction,
            future_capacity,
            guard_count,
            warnings,
            top_guard_id,
            top_facility_id,
            parole_candidate,
        }
    }
}

/// Structured warden decision with the reasoning that produced it. Only the
/// `action` is applied; scores and conflicts are observer-facing evidence.
#[derive(Debug, Clone)]
pub struct StrategyDecision {
    pub action: WardenAction,
    pub rationale: String,
    pub objective_scores: Vec<(Objective, f32)>,
    pub conflicts: Vec<String>,
    pub finance_state: FinanceHealthState,
}

/// Decide from a report-only snapshot. No `World` is available here; every
/// input was assembled by the caller from bounded observer summaries.
#[must_use]
pub fn decide_strategy(
    snapshot: &WardenStrategySnapshot,
    traits: &WardenTraits,
    rng: &mut RngStream,
) -> StrategyDecision {
    let mut conflicts = Vec::new();
    let pressure =
        snapshot.report_pressure() + snapshot.security_incidents_this_week.saturating_mul(2);
    let crowding = if snapshot.capacity == 0 {
        0.0
    } else {
        snapshot.population as f64 / snapshot.capacity as f64
    };

    macro_rules! decisive {
        ($action:expr, $rationale:expr $(,)?) => {
            StrategyDecision {
                action: $action,
                rationale: $rationale,
                objective_scores: Vec::new(),
                conflicts: conflicts.clone(),
                finance_state: snapshot.financial_health,
            }
        };
    }

    // 1. Solvency outranks every other objective.
    if snapshot.cash_major < 0.0 {
        conflicts.push("Solvency overrides security and welfare".into());
        return decisive!(
            WardenAction::ReduceExpenses,
            "Solvency emergency: cash balance is negative".into(),
        );
    }
    if snapshot.financial_health.rank() <= FinanceHealthState::Receivership.rank()
        || snapshot.runway_days < 10.0
    {
        conflicts.push("Funding need overrides expansion and staffing".into());
        let amount_major = 250_000_i64.max((snapshot.runway_days.max(0.0) * 10_000.0) as i64);
        if snapshot
            .can_afford_attention(WardenAction::RequestFunding { amount_major }.attention_cost())
        {
            return decisive!(
                WardenAction::RequestFunding { amount_major },
                format!(
                    "Financial state {} with {:.1} days of runway; requesting funding",
                    snapshot.financial_health.label(),
                    snapshot.runway_days
                ),
            );
        }
        conflicts.push("Funding request deferred by exhausted attention".into());
        return decisive!(
            WardenAction::ReduceExpenses,
            "Funding unavailable this cycle; cutting discretionary spending".into(),
        );
    }
    if snapshot.financial_health == FinanceHealthState::Critical {
        conflicts.push("Expense cuts reduce welfare and rehabilitation".into());
        return decisive!(
            WardenAction::ReduceExpenses,
            "Financial state Critical: deferring discretionary spending".into(),
        );
    }

    // 2. Credible, high-severity reports can trigger a targeted response.
    // A single severity-3 call for help is already a crisis; two lower-grade
    // signals are needed before the warden diverts from other objectives.
    let crisis_report = snapshot
        .reports
        .iter()
        .any(|r| r.severity >= 3 && r.requests_assistance && r.credibility >= 0.25);
    if pressure >= 6 || crisis_report {
        if let Some(event_id) = snapshot.priority_report() {
            conflicts.push("Security response may reduce prisoner welfare".into());
            return decisive!(
                WardenAction::RespondToDisturbance { event_id },
                format!(
                    "Report pressure {pressure}; responding to credible disturbance {event_id}"
                ),
            );
        }
    }

    // 3. Expansion is allowed only with funding headroom, attention, and no
    // duplicate site. The catalog is the authority for cost and staffing.
    let can_spend = snapshot.financial_health.rank() >= FinanceHealthState::Watch.rank();
    if crowding > 0.9 && can_spend && snapshot.active_construction == 0 {
        if let Some(spec) = find_building_spec("cell_block") {
            let staff_need: u32 = spec.staff_required.iter().map(|r| r.count).sum();
            let action = WardenAction::Expand {
                building: spec.key.to_string(),
            };
            if snapshot.can_afford_attention(action.attention_cost())
                && (snapshot.cash_major * 100.0) as i64 >= spec.cost_minor
            {
                conflicts.push("Expansion improves welfare but strains finances".into());
                if snapshot.guard_count < staff_need {
                    conflicts.push("Expansion requires additional staffing".into());
                }
                return decisive!(
                    action,
                    format!(
                        "Crowding {crowding:.2}; queuing {} (capacity +{}, cost ${})",
                        spec.name,
                        spec.capacity,
                        spec.cost_minor / 100
                    ),
                );
            }
            conflicts.push("Expansion deferred by budget or attention constraint".into());
        }
    }

    // 4. Staffing pressure, again gated by finance and attention.
    let hire = WardenAction::HireGuards { count: 2 };
    if can_spend
        && snapshot.capacity > snapshot.guard_count.saturating_mul(25)
        && snapshot.can_afford_attention(hire.attention_cost())
    {
        conflicts.push("Hiring improves morale and security but raises payroll".into());
        return decisive!(
            hire,
            format!(
                "{} guards for {} beds; adding two officers",
                snapshot.guard_count, snapshot.capacity
            ),
        );
    }

    // Targeted operational actions that are more specific than the objective
    // vote. These keep the warden's full action surface reachable.
    if snapshot.report_pressure() >= 3 {
        if let Some(event_id) = snapshot.priority_report() {
            conflicts.push("Investigation may divert attention from routine duties".into());
            return decisive!(
                WardenAction::InvestigateMisconduct { target: event_id },
                format!("Report pressure {}; investigating misconduct", snapshot.report_pressure()),
            );
        }
    }
    if pressure >= 4 {
        conflicts.push("Negotiation may concede ground to prisoner demands".into());
        return decisive!(
            WardenAction::NegotiateWithRepresentatives,
            format!("Report pressure {pressure}; negotiating with representatives"),
        );
    }
    if snapshot.report_pressure() >= 2 {
        if let Some(guard) = snapshot.top_guard_id {
            conflicts.push("Promotion raises payroll expectations".into());
            return decisive!(
                WardenAction::PromoteStaff { guard },
                format!("Report pressure {}; promoting guard {}", snapshot.report_pressure(), guard),
            );
        }
    }
    if snapshot.guard_count > snapshot.capacity / 15 + 1 && snapshot.runway_days < 30.0 {
        conflicts.push("Staff cuts reduce coverage and morale".into());
        return decisive!(
            WardenAction::FireGuards { count: 1 },
            format!(
                "{} guards for {} beds with {:.1} runway days; trimming staff",
                snapshot.guard_count, snapshot.capacity, snapshot.runway_days
            ),
        );
    }
    if snapshot.active_construction == 0
        && crowding > 0.75
        && crowding <= 0.9
        && can_spend
    {
        if let Some(facility) = snapshot.top_facility_id {
            conflicts.push("Infrastructure work competes with expansion".into());
            return decisive!(
                WardenAction::ImproveInfrastructure { facility },
                format!("Crowding {crowding:.2}; improving facility {facility}"),
            );
        }
    }
    if snapshot.population > (snapshot.capacity as f64 * 0.8) as u32 && snapshot.runway_days > 30.0 {
        if let Some(prisoner) = snapshot.parole_candidate {
            conflicts.push("Parole adjustment may affect security".into());
            return decisive!(
                WardenAction::AdjustParoleRecommendation {
                    prisoner,
                    lean: 0.7,
                },
                format!("Population {}; adjusting parole recommendation", snapshot.population),
            );
        }
    }

    // 5. Non-emergency decisions: score all objectives, then let personality
    // break ties deterministically. Conflicts name the losing pressure.
    if !traits.is_finite() || !snapshot.attention.is_finite() {
        conflicts.push("Invalid strategy input; no objective scored".into());
        return decisive!(
            WardenAction::NoAction {
                rationale: "invalid strategy input".into(),
            },
            "Invalid strategy input; holding steady".into(),
        );
    }
    if !snapshot.can_afford_attention(AttentionBudget::tactical_cost()) && pressure == 0 {
        conflicts.push("Attention retained instead of taking a low-value action".into());
        return decisive!(
            WardenAction::NoAction {
                rationale: "attention budget exhausted".into(),
            },
            "Attention budget exhausted; holding steady".into(),
        );
    }

    let cash = snapshot.cash_major as f32;
    let mut objective_scores: Vec<(Objective, f32)> = Objective::ALL
        .iter()
        .copied()
        .map(|obj| {
            let mut score = objective_weight(traits, obj, snapshot.population, cash);
            if matches!(obj, Objective::Security) {
                score += 0.06 * pressure.min(20) as f32;
            }
            if matches!(obj, Objective::FinancialSustainability) && snapshot.health < 0.5 {
                score += 0.3;
            }
            if matches!(obj, Objective::FacilityExpansion) && crowding > 0.75 {
                score += 0.2;
            }
            if matches!(obj, Objective::Rehabilitation | Objective::PrisonerWelfare)
                && snapshot.financial_health == FinanceHealthState::Tight
            {
                score -= 0.15;
            }
            (obj, score)
        })
        .collect();
    objective_scores.sort_by(|a, b| b.1.total_cmp(&a.1));

    // Feasibility is part of strategy: an objective that cannot be actioned
    // under current finance/attention must not silently win the vote.
    let mut candidates: Vec<(Objective, f32, WardenAction, String)> = objective_scores
        .iter()
        .filter_map(|(obj, score)| {
            feasible_action(*obj, snapshot, can_spend, crowding)
                .map(|(action, rationale)| (*obj, *score, action, rationale))
        })
        .collect();
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    if candidates.is_empty() {
        conflicts.push("No feasible objective remained after constraints".into());
        return decisive!(
            WardenAction::NoAction {
                rationale: "all objectives constrained".into(),
            },
            "No feasible strategic action; holding steady".into(),
        );
    }

    let best_score = candidates[0].1;
    let ties: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| (best_score - c.1).abs() <= 1e-4)
        .map(|(i, _)| i)
        .collect();
    let best_idx = if ties.len() > 1 {
        ties[(rng.next_u64() as usize) % ties.len()]
    } else {
        ties[0]
    };
    let best = candidates[best_idx].0;
    if let Some(runner_up) = candidates.iter().find(|c| c.0 != best) {
        if (best_score - runner_up.1).abs() < 0.12 {
            conflicts.push(format!(
                "{best:?} and {:?} have nearly equal priority",
                runner_up.0
            ));
        }
    }
    if conflicts.is_empty() {
        conflicts.push(format!("{best:?} outranks the other objectives"));
    }

    let (action, rationale) = (
        candidates[best_idx].2.clone(),
        candidates[best_idx].3.clone(),
    );

    StrategyDecision {
        action,
        rationale,
        objective_scores,
        conflicts,
        finance_state: snapshot.financial_health,
    }
}

/// Map an objective to a legal action under the current report-derived
/// constraints. Returns `None` when the objective is real but unactionable.
fn feasible_action(
    obj: Objective,
    snapshot: &WardenStrategySnapshot,
    can_spend: bool,
    crowding: f64,
) -> Option<(WardenAction, String)> {
    match obj {
        Objective::Security | Objective::EscapePrevention | Objective::ViolenceReduction => Some((
            WardenAction::IncreaseSecurity,
            format!(
                "Security objective weighted highest under pressure {}",
                snapshot.report_pressure()
            ),
        )),
        Objective::FinancialSustainability => Some((
            WardenAction::ReduceExpenses,
            format!(
                "Financial state {} constrains discretionary action",
                snapshot.financial_health.label()
            ),
        )),
        Objective::FacilityExpansion => {
            let spec = find_building_spec("cell_block")?;
            let action = WardenAction::Expand {
                building: spec.key.to_string(),
            };
            if !can_spend
                || snapshot.active_construction > 0
                || !snapshot.can_afford_attention(action.attention_cost())
                || ((snapshot.cash_major * 100.0) as i64) < spec.cost_minor
            {
                return None;
            }
            Some((
                action,
                format!("Expansion objective selected at crowding {crowding:.2}"),
            ))
        }
        Objective::Rehabilitation => Some((
            WardenAction::ChangePolicy {
                policy: "rehab_funding".into(),
                value: 1.1,
            },
            "Rehabilitation objective selected by reform-oriented strategy".into(),
        )),
        Objective::PublicReputation | Objective::PoliticalPressure => Some((
            WardenAction::NegotiateWithRepresentatives,
            "Reputation objective selected; engaging representatives".into(),
        )),
        Objective::StaffMorale => {
            let action = WardenAction::HireGuards { count: 1 };
            if !can_spend || !snapshot.can_afford_attention(action.attention_cost()) {
                return None;
            }
            Some((
                action,
                "Staff morale objective selected; adding one officer".into(),
            ))
        }
        Objective::PrisonerWelfare | Objective::InstitutionalStability => Some((
            WardenAction::NegotiateWithRepresentatives,
            "Stability objective selected; opening dialogue with representatives".into(),
        )),
    }
}

fn objective_weight(
    traits: &WardenTraits,
    obj: Objective,
    population: u32,
    cash_major: f32,
) -> f32 {
    match obj {
        Objective::Security => traits.strictness * 0.7 + traits.authoritarian_tendencies * 0.3,
        Objective::FinancialSustainability => traits.financial_discipline,
        Objective::StaffMorale => traits.staff_loyalty,
        Objective::PrisonerWelfare => traits.compassion,
        Objective::PublicReputation => traits.reputation_concerns,
        Objective::InstitutionalStability => (traits.competence + traits.strictness) * 0.5,
        Objective::Rehabilitation => traits.reform_orientation,
        Objective::EscapePrevention => traits.risk_tolerance * 0.6 + traits.strictness * 0.4,
        Objective::ViolenceReduction => traits.compassion * 0.6 + traits.competence * 0.4,
        Objective::PoliticalPressure => traits.political_ambition,
        Objective::FacilityExpansion => {
            let cash_factor = (cash_major / 500_000.0).clamp(0.0, 1.0);
            let pop_factor = (population as f32 / 200.0).clamp(0.0, 1.0);
            (cash_factor * 0.6 + pop_factor * 0.4) * (0.5 + traits.risk_tolerance * 0.5)
        }
    }
}

impl WardenTraits {
    fn is_finite(&self) -> bool {
        [
            self.strictness,
            self.compassion,
            self.political_ambition,
            self.financial_discipline,
            self.risk_tolerance,
            self.reform_orientation,
            self.authoritarian_tendencies,
            self.staff_loyalty,
            self.corruption_susceptibility,
            self.reputation_concerns,
            self.competence,
        ]
        .iter()
        .all(|v| v.is_finite())
    }
}

/// The warden agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warden {
    pub id: WardenId,
    pub name: String,
    pub traits: WardenTraits,
    pub philosophy: String,
    pub inbox: Vec<u64>, // event ids the warden has "received" (credibility-scored)
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
    /// Auditable decision log (append-only, bounded). New field in 0.05.
    #[serde(default)]
    pub audit_log: WardenAuditLog,
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
            audit_log: WardenAuditLog::default(),
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

    /// Record an audited warden decision. Returns the record id. Arbitration
    /// itself is unchanged: callers (engine/UI) invoke this after applying an
    /// action so the control panel and decision audit view have a trail.
    pub fn record_decision(
        &mut self,
        time: SimTime,
        action: WardenAction,
        reason: impl Into<String>,
        source: DecisionSource,
        applied: bool,
    ) -> u64 {
        self.audit_log.record(
            time,
            self.id,
            self.name.clone(),
            action,
            reason,
            source,
            applied,
        )
    }

    /// Most recent `n` decision records (newest first).
    pub fn recent_decisions(&self, n: usize) -> Vec<&WardenDecisionRecord> {
        self.audit_log.recent(n)
    }

    /// Borrow the full audit log.
    pub fn decision_audit_log(&self) -> &WardenAuditLog {
        &self.audit_log
    }

    /// Control-panel snapshot for the UI: attention, affordance, pending and
    /// recent decision counts, incidents, and financial health. Pure read.
    pub fn control_panel_summary(&self, financial_health: f64) -> WardenControlSummary {
        let base = AttentionBudget::default();
        WardenControlSummary {
            attention: self.attention,
            attention_points: (self.attention * base.base_refill).clamp(0.0, base.max_points),
            can_decide_tactical: self.can_decide(AttentionBudget::tactical_cost()),
            can_decide_strategic: self.can_decide(AttentionBudget::strategic_cost()),
            concurrent_decisions_remaining_tactical: self
                .concurrent_decisions_remaining(AttentionBudget::tactical_cost()),
            pending_actions: self.pending_actions.len(),
            recent_decisions: self.audit_log.len(),
            security_incidents_this_week: self.security_incidents_this_week,
            last_action_day: self.last_action_day,
            financial_health,
        }
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
pub fn arbitrate(
    warden: &mut Warden,
    economy: &Economy,
    population: u32,
    rng: &mut RngStream,
) -> WardenAction {
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
        if matches!(
            obj,
            Objective::Security | Objective::EscapePrevention | Objective::ViolenceReduction
        ) && security_incidents_this_week > 0
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
        Warden::new(
            WardenId(0),
            "Test Warden",
            WardenTraits::randomized(&mut RngStream::new(7, 0)),
        )
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
        assert!(
            matches!(action, WardenAction::IncreaseSecurity),
            "{action:?}"
        );
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
        assert!(
            matches!(action, WardenAction::NoAction { .. }),
            "{action:?}"
        );
    }

    #[test]
    fn attention_budget_limits_concurrent_decisions() {
        let mut w = warden();
        assert!(w.can_decide(AttentionBudget::tactical_cost()));
        assert_eq!(
            w.concurrent_decisions_remaining(AttentionBudget::tactical_cost()),
            50
        );
        w.attention = 0.05; // 5 points
        assert_eq!(
            w.concurrent_decisions_remaining(AttentionBudget::tactical_cost()),
            2
        );
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
            (40, 100, 0, 25_000_000, 1.0), // healthy
            (0, 100, 0, 25_000_000, 1.0),  // empty
            (40, 100, 0, -1, 1.0),         // bankrupt
            (95, 100, 0, 25_000_000, 1.0), // overcrowded
            (40, 100, 8, 25_000_000, 1.0), // high incidents / crisis
            (40, 100, 3, 25_000_000, 1.0), // low morale proxy / moderate unrest
            (40, 0, 0, 25_000_000, 1.0),   // no capacity info
            (40, 100, 0, 25_000_000, 0.0), // exhausted attention
            (200, 100, 12, -500_000, 0.0), // worst case
            (10, 100, 1, 500_000, 0.3),    // tight finances
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

    fn balanced_traits() -> WardenTraits {
        WardenTraits {
            strictness: 0.5,
            compassion: 0.5,
            political_ambition: 0.5,
            financial_discipline: 0.5,
            risk_tolerance: 0.5,
            reform_orientation: 0.5,
            authoritarian_tendencies: 0.5,
            staff_loyalty: 0.5,
            corruption_susceptibility: 0.2,
            reputation_concerns: 0.5,
            competence: 0.5,
        }
    }

    fn healthy_snapshot() -> WardenStrategySnapshot {
        WardenStrategySnapshot::new(
            50,
            100,
            1_000_000.0,
            0.8,
            180.0,
            0,
            1.0,
            Vec::new(),
            0,
            0,
            8,
            Vec::new(),
            None,
            None,
            None,
        )
    }

    #[test]
    fn audit_log_default_can_record_without_panic() {
        let mut log = WardenAuditLog::default();
        let id = log.record(
            SimTime::EPOCH,
            WardenId(0),
            "Test Warden",
            WardenAction::ReduceExpenses,
            "test rationale",
            DecisionSource::AiArbitration,
            true,
        );
        assert_eq!(id, 1);
        assert_eq!(log.len(), 1);
        assert_eq!(log.recent(1)[0].rationale, "test rationale");
    }

    #[test]
    fn strategy_decision_is_deterministic_and_names_conflicts() {
        let snapshot = healthy_snapshot();
        let traits = balanced_traits();
        let first = decide_strategy(&snapshot, &traits, &mut RngStream::new(11, 0));
        let second = decide_strategy(&snapshot, &traits, &mut RngStream::new(11, 0));
        assert_eq!(first.action.label(), second.action.label());
        assert_eq!(first.rationale, second.rationale);
        assert_eq!(first.conflicts, second.conflicts);
        assert!(!first.conflicts.is_empty());
        assert_eq!(first.finance_state, FinanceHealthState::Healthy);
    }

    #[test]
    fn critical_finance_requests_funding_before_expansion() {
        let mut snapshot = healthy_snapshot();
        snapshot.cash_major = 1_000.0;
        snapshot.health = 0.1;
        snapshot.runway_days = 5.0;
        snapshot.population = 95;
        let decision = decide_strategy(&snapshot, &balanced_traits(), &mut RngStream::new(2, 0));
        assert!(
            matches!(decision.action, WardenAction::RequestFunding { amount_major } if amount_major > 0),
            "{:?}",
            decision.action
        );
        assert!(decision.conflicts.iter().any(|c| c.contains("Funding")));
    }

    #[test]
    fn credible_reports_trigger_targeted_response() {
        let mut snapshot = healthy_snapshot();
        snapshot.reports = vec![
            WardenReportSignal {
                id: 7,
                at: SimTime::EPOCH,
                summary: "yard disturbance".into(),
                severity: 3,
                credibility: 0.7,
                requests_assistance: false,
            },
            WardenReportSignal {
                id: 42,
                at: SimTime::EPOCH,
                summary: "assistance requested".into(),
                severity: 3,
                credibility: 0.95,
                requests_assistance: true,
            },
        ];
        let decision = decide_strategy(&snapshot, &balanced_traits(), &mut RngStream::new(3, 0));
        assert_eq!(
            decision.action,
            WardenAction::RespondToDisturbance { event_id: 42 }
        );
        assert!(decision.rationale.contains("Report pressure"));
    }

    #[test]
    fn credible_severity_two_reports_count_as_pressure_and_targets() {
        let mut snapshot = healthy_snapshot();
        snapshot.reports = vec![WardenReportSignal {
            id: 17,
            at: SimTime::EPOCH,
            summary: "serious disturbance".into(),
            severity: 2,
            credibility: 0.6,
            requests_assistance: false,
        }];
        assert_eq!(snapshot.report_pressure(), 2);
        assert_eq!(snapshot.priority_report(), Some(17));

        let mut three = snapshot.clone();
        three.reports = vec![
            snapshot.reports[0].clone(),
            WardenReportSignal {
                id: 18,
                ..snapshot.reports[0].clone()
            },
            WardenReportSignal {
                id: 19,
                ..snapshot.reports[0].clone()
            },
        ];
        let decision = decide_strategy(&three, &balanced_traits(), &mut RngStream::new(5, 0));
        // Equal keys keep the last maximum (`max_by_key` semantics); what
        // matters is that the target is one of the three pressure reports.
        let targeted = matches!(
            &decision.action,
            WardenAction::RespondToDisturbance {
                event_id: 17 | 18 | 19,
            }
        );
        assert!(targeted, "unexpected action: {:?}", decision.action);
    }

    #[test]
    fn crowding_expansion_requires_finance_headroom() {
        let mut snapshot = healthy_snapshot();
        snapshot.population = 95;
        let rich = decide_strategy(&snapshot, &balanced_traits(), &mut RngStream::new(4, 0));
        assert!(
            matches!(rich.action, WardenAction::Expand { ref building } if building == "cell_block"),
            "{:?}",
            rich.action
        );

        snapshot.health = 0.3;
        snapshot.runway_days = 50.0;
        snapshot.refresh_finance();
        let strained = decide_strategy(&snapshot, &balanced_traits(), &mut RngStream::new(4, 0));
        assert!(
            !matches!(strained.action, WardenAction::Expand { .. }),
            "Tight finance must block expansion: {:?}",
            strained.action
        );
    }
}
