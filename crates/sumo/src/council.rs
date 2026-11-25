//! Council of institutional drives (R3): a deterministic, RNG-free evaluator
//! for warden decisions.
//!
//! The warden's [`alc_warden::arbitrate`] path picks an action from a small
//! objective list using utility scoring plus an RNG tie-break. The council is a
//! second, fully deterministic decision path used by
//! [`crate::PolicyMode::Council`] and watched (but not applied) by
//! [`crate::PolicyMode::Shadow`]:
//!
//! 1. [`candidate_actions`] derives a fixed-shape menu of legally applicable
//!    [`WardenAction`]s from the current [`World`] (plus `NoAction` always).
//! 2. [`DriveWeights::from_warden`] turns the warden's personality traits into
//!    per-drive voting weights — a strict warden weights security heavily, a
//!    reformist weights welfare/rehabilitation.
//! 3. [`deliberate`] scores every candidate against the seven [`Drive`]s in
//!    `0..=1` and takes the dot product with the weights. Ties break on the
//!    action's debug string so the outcome is reproducible across runs and
//!    platforms.
//!
//! Drive scores are *marginal*: `0.5` means "no better and no worse than
//! holding steady", above `0.5` means the action improves that drive, below
//! `0.5` means it costs the drive. [`WardenAction::NoAction`] therefore scores
//! exactly `0.5` everywhere, and the council only acts when a candidate's
//! weighted benefits out-earn its weighted costs instead of preferring action
//! (or inaction) for structural reasons.
//!
//! No model, network call, or RNG is involved: given the same world snapshot
//! the council always returns the same [`CouncilVote`]. That is what makes
//! `PolicyMode::Deterministic` trace parity and shadow A/B comparison possible.
//!
//! The council only *proposes*; [`crate::drivers::warden_cycle`] validates the
//! winner through the engine-side applier and emits a `PolicyChange` event
//! carrying the rationale (Directive §19: agents never mutate world state).

use alc_warden::{Warden, WardenAction};
use alc_world::World;
use std::cmp::Ordering;

/// Institutional drives the council weighs against one another.
///
/// Each drive maps onto one or more of the warden's competing objectives
/// (Directive §9) — security posture, solvency, staff morale, prisoner welfare,
/// public reputation, rehabilitation and institutional stability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Drive {
    /// Order, discipline, escape prevention, incident suppression.
    Security,
    /// Solvency, runway, payroll and construction affordability.
    Finance,
    /// Staff morale, workload and retention.
    Morale,
    /// Prisoner welfare: crowding, conditions, dignity.
    Welfare,
    /// Public reputation and political standing.
    Reputation,
    /// Rehabilitation, education and program funding.
    Rehab,
    /// Institutional stability: predictability, low unrest, continuity.
    Stability,
}

/// Canonical index of [`Drive::Security`] in score vectors.
const SECURITY: usize = Drive::Security.index();
/// Canonical index of [`Drive::Finance`] in score vectors.
const FINANCE: usize = Drive::Finance.index();
/// Canonical index of [`Drive::Morale`] in score vectors.
const MORALE: usize = Drive::Morale.index();
/// Canonical index of [`Drive::Welfare`] in score vectors.
const WELFARE: usize = Drive::Welfare.index();
/// Canonical index of [`Drive::Reputation`] in score vectors.
const REPUTATION: usize = Drive::Reputation.index();
/// Canonical index of [`Drive::Rehab`] in score vectors.
const REHAB: usize = Drive::Rehab.index();
/// Canonical index of [`Drive::Stability`] in score vectors.
const STABILITY: usize = Drive::Stability.index();

impl Drive {
    /// Number of drives (size of a score vector).
    pub const COUNT: usize = 7;

    /// Every drive in canonical order. Score vectors are indexed by this order.
    pub const ALL: [Drive; Self::COUNT] = [
        Drive::Security,
        Drive::Finance,
        Drive::Morale,
        Drive::Welfare,
        Drive::Reputation,
        Drive::Rehab,
        Drive::Stability,
    ];

    /// Canonical index of this drive inside score vectors and [`DriveWeights`].
    pub const fn index(self) -> usize {
        match self {
            Drive::Security => 0,
            Drive::Finance => 1,
            Drive::Morale => 2,
            Drive::Welfare => 3,
            Drive::Reputation => 4,
            Drive::Rehab => 5,
            Drive::Stability => 6,
        }
    }

    /// Lower-case label used in rationale strings, events and logs.
    pub const fn label(self) -> &'static str {
        match self {
            Drive::Security => "security",
            Drive::Finance => "finance",
            Drive::Morale => "morale",
            Drive::Welfare => "welfare",
            Drive::Reputation => "reputation",
            Drive::Rehab => "rehab",
            Drive::Stability => "stability",
        }
    }
}

/// Per-drive voting weights for the council.
///
/// Values are positive priorities (clamped to `0.05..=2.0`) derived
/// deterministically from the warden's personality; a weight of `1.0` means
/// "neutral". They are deliberately not normalised: the total expresses how
/// much attention each drive commands from this particular warden.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriveWeights {
    /// Weight given to security and discipline.
    pub security: f32,
    /// Weight given to solvency and fiscal control.
    pub finance: f32,
    /// Weight given to staff morale and retention.
    pub morale: f32,
    /// Weight given to prisoner welfare and conditions.
    pub welfare: f32,
    /// Weight given to public reputation and political capital.
    pub reputation: f32,
    /// Weight given to rehabilitation and program funding.
    pub rehab: f32,
    /// Weight given to institutional stability and continuity.
    pub stability: f32,
}

impl Default for DriveWeights {
    /// A flat council: every drive votes with equal weight.
    fn default() -> Self {
        Self {
            security: 1.0,
            finance: 1.0,
            morale: 1.0,
            welfare: 1.0,
            reputation: 1.0,
            rehab: 1.0,
            stability: 1.0,
        }
    }
}

impl DriveWeights {
    /// Weight for a single drive.
    pub const fn get(&self, drive: Drive) -> f32 {
        match drive {
            Drive::Security => self.security,
            Drive::Finance => self.finance,
            Drive::Morale => self.morale,
            Drive::Welfare => self.welfare,
            Drive::Reputation => self.reputation,
            Drive::Rehab => self.rehab,
            Drive::Stability => self.stability,
        }
    }

    /// Derive voting weights from the warden's personality traits.
    ///
    /// The mapping is a pure function of [`alc_warden::WardenTraits`] (all
    /// traits are `0..=1`), so two wardens with the same traits always vote the
    /// same way:
    ///
    /// | Drive | Dominant traits |
    /// | --- | --- |
    /// | security | `strictness`, `authoritarian_tendencies`, low `compassion` |
    /// | finance | `financial_discipline`, low `risk_tolerance` |
    /// | morale | `staff_loyalty`, `competence` |
    /// | welfare | `compassion`, `reform_orientation`, low authoritarianism |
    /// | reputation | `reputation_concerns`, `political_ambition` |
    /// | rehab | `reform_orientation`, `compassion`, low authoritarianism |
    /// | stability | `competence`, low `risk_tolerance`, `strictness` |
    ///
    /// Every weight is clamped into `0.05..=2.0` so no drive is silenced (the
    /// council must always have a second preference to cite) and none dominates
    /// absolutely.
    pub fn from_warden(warden: &Warden) -> Self {
        let t = &warden.traits;
        let clamp = |v: f32| v.clamp(0.05, 2.0);
        Self {
            security: clamp(
                0.35 + 0.70 * t.strictness
                    + 0.30 * t.authoritarian_tendencies
                    + 0.15 * (1.0 - t.compassion),
            ),
            finance: clamp(0.30 + 0.85 * t.financial_discipline + 0.15 * (1.0 - t.risk_tolerance)),
            morale: clamp(0.35 + 0.75 * t.staff_loyalty + 0.25 * t.competence),
            welfare: clamp(
                0.30 + 0.85 * t.compassion + 0.20 * t.reform_orientation
                    - 0.15 * t.authoritarian_tendencies,
            ),
            reputation: clamp(0.25 + 0.90 * t.reputation_concerns + 0.30 * t.political_ambition),
            rehab: clamp(
                0.25 + 0.95 * t.reform_orientation + 0.25 * t.compassion
                    - 0.25 * t.authoritarian_tendencies,
            ),
            stability: clamp(
                0.40 + 0.70 * t.competence + 0.20 * (1.0 - t.risk_tolerance) + 0.10 * t.strictness,
            ),
        }
    }

    /// Sum of all weights (attention mass behind the council), for reports.
    pub fn total(&self) -> f32 {
        Drive::ALL.iter().map(|d| self.get(*d)).sum()
    }
}

/// The council's decision: the winning action plus the deliberation record.
#[derive(Debug, Clone)]
pub struct CouncilVote {
    /// The action the council selected.
    pub action: WardenAction,
    /// Per-drive score of `action` in `0..=1`, in [`Drive::ALL`] order.
    /// `0.5` is neutral (no better or worse than holding steady); values above
    /// `0.5` mean the action serves that drive, values below mean it costs it.
    pub scores: Vec<(Drive, f32)>,
    /// Weighted total (`sum(weight * score)`) that decided the vote. Holding
    /// steady always totals `0.5 * weights.total()`.
    pub total: f32,
    /// Human-readable explanation naming the drives that carried the vote.
    pub rationale: String,
}

impl CouncilVote {
    /// Score for one drive from the winning candidate's deliberation record.
    pub fn score(&self, drive: Drive) -> f32 {
        self.scores
            .iter()
            .find(|(d, _)| *d == drive)
            .map(|(_, s)| *s)
            .unwrap_or(0.0)
    }
}

/// Short, stable label for a [`WardenAction`] variant (no payload).
///
/// Used in event summaries and rationale strings so observers can read a
/// decision without parsing the debug dump (and so string comparisons on
/// actions are stable across runs).
pub fn action_label(action: &WardenAction) -> &'static str {
    match action {
        WardenAction::Expand { .. } => "Expand",
        WardenAction::HireGuards { .. } => "HireGuards",
        WardenAction::FireGuards { .. } => "FireGuards",
        WardenAction::PromoteStaff { .. } => "PromoteStaff",
        WardenAction::ChangePolicy { .. } => "ChangePolicy",
        WardenAction::IncreaseSecurity => "IncreaseSecurity",
        WardenAction::ReduceExpenses => "ReduceExpenses",
        WardenAction::ImproveInfrastructure { .. } => "ImproveInfrastructure",
        WardenAction::RespondToDisturbance { .. } => "RespondToDisturbance",
        WardenAction::NegotiateWithRepresentatives => "NegotiateWithRepresentatives",
        WardenAction::InvestigateMisconduct { .. } => "InvestigateMisconduct",
        WardenAction::AdjustParoleRecommendation { .. } => "AdjustParoleRecommendation",
        WardenAction::RequestFunding { .. } => "RequestFunding",
        WardenAction::NoAction { .. } => "NoAction",
    }
}

/// Structural equality for [`WardenAction`], which does not implement
/// `PartialEq` in `alc-warden`.
///
/// The debug representation covers every payload field; `NoAction`'s rationale
/// string is included, which is what the council wants (a different "hold
/// steady" justification is a different decision).
pub fn actions_equivalent(a: &WardenAction, b: &WardenAction) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

/// Snapshot of the institutional pressure state the council reads.
///
/// All features are clamped to `0..=1`. Computed once per deliberation so every
/// candidate is scored against the same picture (and so the function is cheap).
#[derive(Debug, Clone, Copy)]
struct StateView {
    /// Population as a fraction of rated bed capacity (1.0 when no beds).
    crowding: f32,
    /// Financial stress: `0` = a month of runway or more, `1` = broke.
    cash_stress: f32,
    /// Security shortfall against the 0.8 target posture.
    security_deficit: f32,
    /// Staffing shortfall (facility coverage and prisoners-per-guard load).
    staff_deficit: f32,
    /// Worst facility condition deficit (`1 - min(condition)`).
    facility_wear: f32,
    /// Combined incident and crowding pressure.
    unrest: f32,
    /// How far rehab funding sits below its baseline multiplier.
    rehab_need: f32,
}

impl StateView {
    fn from_world(world: &World) -> Self {
        let population = world.population();
        let capacity = world.capacity();
        let crowding = if capacity == 0 {
            1.0
        } else {
            (population as f32 / capacity as f32).clamp(0.0, 1.0)
        };

        let health = world.economy.health().clamp(0.0, 1.0) as f32;
        let cash_stress = 1.0 - health;

        let security_deficit = ((0.8 - world.policies.security_level) / 0.8).clamp(0.0, 1.0);

        let required_staff: u32 = world.facilities.iter().map(|f| f.staff_required).sum();
        let coverage = if required_staff == 0 {
            1.0
        } else {
            (world.guards.len() as f32 / required_staff as f32).min(1.0)
        };
        let prisoner_load = population as f32 / world.guards.len().max(1) as f32;
        let load_deficit = ((prisoner_load - 6.0) / 12.0).clamp(0.0, 1.0);
        let staff_deficit = (1.0 - coverage).clamp(0.0, 1.0).max(load_deficit);

        let worst_condition = world
            .facilities
            .iter()
            .map(|f| f.condition)
            .fold(1.0_f32, f32::min);
        let facility_wear = (1.0 - worst_condition).clamp(0.0, 1.0);

        let incidents = world
            .warden
            .security_incidents_this_week
            .max(world.warden.incidents_this_week);
        let unrest =
            ((incidents as f32 / 8.0).clamp(0.0, 1.0) * 0.7 + crowding * 0.3).clamp(0.0, 1.0);

        let rehab_need = ((1.0 - world.policies.rehab_funding_multiplier) / 0.8).clamp(0.0, 1.0);

        Self {
            crowding,
            cash_stress,
            security_deficit,
            staff_deficit,
            facility_wear,
            unrest,
            rehab_need,
        }
    }
}

/// Derive the council's candidate menu from live world state.
///
/// The menu is a fixed shape (no RNG, no sampling): each pressure that crosses
/// a documented threshold contributes exactly one applicable action, and
/// [`WardenAction::NoAction`] is always present so the council can vote to hold
/// steady. Candidates are appended in a stable order, so equal scores always
/// break the same way.
///
/// Thresholds:
/// - crowding `>= 0.85` → `Expand` (a cell block).
/// - [`StateView::security_deficit`] `>= 0.30` or more than 3 incidents this
///   week → `IncreaseSecurity`.
/// - cash stress `>= 0.5` or cash below $250k → `ReduceExpenses`.
/// - staffing shortfall `>= 0.25` → `HireGuards { count: 2 }`.
/// - facility wear `>= 0.2` → `ImproveInfrastructure` for the worst facility.
/// - cash below $100k or cash stress `>= 0.6` → `RequestFunding`.
/// - rehab funding multiplier below `0.9` → `ChangePolicy` raising it by 0.1.
pub fn candidate_actions(world: &World) -> Vec<WardenAction> {
    let view = StateView::from_world(world);
    let cash_major = world.economy.cash.as_major();
    let mut candidates = Vec::new();

    if view.crowding >= 0.85 {
        candidates.push(WardenAction::Expand {
            building: "cell_block".into(),
        });
    }

    if view.security_deficit >= 0.30 || world.warden.security_incidents_this_week > 3 {
        candidates.push(WardenAction::IncreaseSecurity);
    }

    if view.cash_stress >= 0.5 || cash_major < 250_000.0 {
        candidates.push(WardenAction::ReduceExpenses);
    }

    if view.staff_deficit >= 0.25 {
        candidates.push(WardenAction::HireGuards { count: 2 });
    }

    if view.facility_wear >= 0.2 {
        // Worst-off facility first; ties resolve to the lowest facility id
        // because `facilities` is append-ordered by id.
        if let Some(worst) = world.facilities.iter().min_by(|a, b| {
            a.condition
                .partial_cmp(&b.condition)
                .unwrap_or(Ordering::Equal)
        }) {
            candidates.push(WardenAction::ImproveInfrastructure {
                facility: worst.id.0,
            });
        }
    }

    if cash_major < 100_000.0 || view.cash_stress >= 0.6 {
        candidates.push(WardenAction::RequestFunding {
            amount_major: 250_000,
        });
    }

    if world.policies.rehab_funding_multiplier < 0.9 {
        candidates.push(WardenAction::ChangePolicy {
            policy: "rehab_funding".into(),
            value: f64::from(world.policies.rehab_funding_multiplier + 0.1),
        });
    }

    candidates.push(WardenAction::NoAction {
        rationale: "hold steady; no institutional pressure crosses the council threshold".into(),
    });

    candidates
}

/// Marginal utility of `action` on each drive relative to holding steady.
///
/// `+1.0` = strongly improves the drive, `0.0` = no change, `-1.0` = strongly
/// costs the drive. `NoAction` is the reference point and is identically zero,
/// so the council acts only when a candidate's weighted benefits out-earn its
/// weighted costs. Effects scale with the relevant pressure (`crowding`,
/// `cash_stress`, ...) — an empty prison gains nothing from expanding.
fn marginal_effect(view: &StateView, action: &WardenAction) -> [f32; Drive::COUNT] {
    let mut u = [0.0_f32; Drive::COUNT];
    match action {
        WardenAction::Expand { .. } => {
            u[SECURITY] = 0.20 * view.crowding;
            // Construction is a capital cost; free cash makes it easier.
            u[FINANCE] = -0.35 - 0.15 * view.cash_stress;
            u[MORALE] = 0.10 * view.crowding;
            u[WELFARE] = 0.70 * view.crowding;
            u[REPUTATION] = 0.20 * view.crowding;
            u[STABILITY] = 0.40 * view.crowding;
        }
        WardenAction::IncreaseSecurity => {
            u[SECURITY] = 0.50 + 0.50 * view.security_deficit;
            u[FINANCE] = -0.20;
            u[MORALE] = -0.15;
            // Tighter control costs welfare, but an unsafe prison has less to
            // lose from permissiveness than a safe one.
            u[WELFARE] = -0.35 + 0.15 * view.security_deficit;
            u[REPUTATION] = -0.05;
            u[REHAB] = -0.10;
            u[STABILITY] = 0.10 + 0.30 * view.unrest;
        }
        WardenAction::ReduceExpenses => {
            u[SECURITY] = -0.10;
            u[FINANCE] = 0.50 + 0.50 * view.cash_stress;
            u[MORALE] = -0.20;
            u[WELFARE] = -0.30;
            u[REPUTATION] = -0.10;
            u[REHAB] = -0.35;
            u[STABILITY] = -0.10;
        }
        WardenAction::HireGuards { .. } => {
            u[SECURITY] = 0.25 + 0.35 * view.staff_deficit;
            u[FINANCE] = -0.30;
            u[MORALE] = 0.30 * view.staff_deficit;
            u[WELFARE] = 0.10 * view.staff_deficit;
            u[REHAB] = -0.05;
            u[STABILITY] = 0.20;
        }
        WardenAction::ImproveInfrastructure { .. } => {
            u[SECURITY] = 0.10;
            u[FINANCE] = -0.25;
            u[MORALE] = 0.15;
            u[WELFARE] = 0.40 + 0.40 * view.facility_wear;
            u[REPUTATION] = 0.15;
            u[REHAB] = 0.05;
            u[STABILITY] = 0.20;
        }
        WardenAction::RequestFunding { .. } => {
            u[FINANCE] = 0.50 + 0.50 * view.cash_stress;
            u[MORALE] = 0.05;
            u[WELFARE] = 0.10;
            // Asking for a bailout looks weak to oversight.
            u[REPUTATION] = -0.25;
            u[REHAB] = 0.20;
            u[STABILITY] = 0.05;
        }
        WardenAction::ChangePolicy { policy, .. } => {
            if policy == "rehab_funding" {
                u[SECURITY] = -0.05;
                u[FINANCE] = -0.25;
                u[MORALE] = 0.05;
                u[WELFARE] = 0.20 + 0.20 * view.rehab_need;
                u[REPUTATION] = 0.10;
                u[REHAB] = 0.60 + 0.40 * view.rehab_need;
            }
            // Unknown/future policies are neutral: 0.5 on every drive.
        }
        WardenAction::PromoteStaff { .. } => {
            u[SECURITY] = 0.10;
            u[FINANCE] = -0.10;
            u[MORALE] = 0.45;
            u[REPUTATION] = 0.10;
            u[STABILITY] = 0.15;
        }
        WardenAction::FireGuards { .. } => {
            u[SECURITY] = -0.45;
            u[FINANCE] = 0.40;
            u[MORALE] = -0.35;
            u[WELFARE] = -0.15;
            u[REPUTATION] = -0.10;
            u[REHAB] = -0.10;
            u[STABILITY] = -0.25;
        }
        WardenAction::RespondToDisturbance { .. } => {
            u[SECURITY] = 0.40;
            u[FINANCE] = -0.05;
            u[MORALE] = 0.10;
            u[WELFARE] = -0.05;
            u[REPUTATION] = 0.15;
            u[REHAB] = -0.05;
            u[STABILITY] = 0.35;
        }
        WardenAction::NegotiateWithRepresentatives => {
            u[SECURITY] = -0.10;
            u[FINANCE] = -0.05;
            u[MORALE] = 0.15;
            u[WELFARE] = 0.40;
            u[REPUTATION] = 0.25;
            u[REHAB] = 0.15;
            u[STABILITY] = 0.40;
        }
        WardenAction::InvestigateMisconduct { .. } => {
            u[SECURITY] = 0.20;
            u[FINANCE] = -0.10;
            u[MORALE] = -0.05;
            u[REPUTATION] = 0.30;
            u[STABILITY] = 0.10;
        }
        WardenAction::AdjustParoleRecommendation { .. } => {
            u[SECURITY] = -0.10;
            u[FINANCE] = 0.25;
            u[WELFARE] = 0.30;
            u[REPUTATION] = 0.10;
            u[REHAB] = 0.45;
        }
        WardenAction::NoAction { .. } => {
            // The reference point: holding steady neither helps nor hurts.
        }
    }
    u
}

/// Score a candidate's marginal effect on each drive as `0..=1`.
///
/// `0.5 + 0.5 * marginal_effect` maps the signed utility onto the documented
/// score range without changing the ranking: because holding steady scores
/// exactly `0.5` everywhere, the weighted dot product is a strict cost/benefit
/// comparison against inaction.
fn score_action(view: &StateView, action: &WardenAction) -> [f32; Drive::COUNT] {
    let effect = marginal_effect(view, action);
    let mut scores = [0.5_f32; Drive::COUNT];
    for (score, delta) in scores.iter_mut().zip(effect.iter()) {
        *score = (0.5 + 0.5 * delta).clamp(0.0, 1.0);
    }
    scores
}

/// Convene the council: score every candidate and return the winning vote.
///
/// `total = sum(weights[drive] * score[drive])`. Ties are broken by the
/// action's debug string (ascending) so the same snapshot always produces the
/// same winner, on any platform. Passing an empty `candidates` slice returns a
/// `NoAction` vote rather than panicking — the engine never has to special-case
/// a pressure-free menu.
pub fn deliberate(
    world: &World,
    weights: &DriveWeights,
    candidates: &[WardenAction],
) -> CouncilVote {
    if candidates.is_empty() {
        return CouncilVote {
            action: WardenAction::NoAction {
                rationale: "no candidates offered".into(),
            },
            scores: Drive::ALL.iter().map(|d| (*d, 0.5_f32)).collect(),
            total: 0.5 * weights.total(),
            rationale: "council had no candidates; holding steady".to_string(),
        };
    }

    let view = StateView::from_world(world);

    // (debug string, total, action, scores) — the debug string is kept beside
    // the total so the sort does not have to re-format during comparisons.
    let mut votes: Vec<(String, f32, WardenAction, Vec<(Drive, f32)>)> = candidates
        .iter()
        .map(|action| {
            let raw = score_action(&view, action);
            let scores: Vec<(Drive, f32)> =
                Drive::ALL.iter().map(|d| (*d, raw[d.index()])).collect();
            let total: f32 = weighted_total(weights, &raw);
            (format!("{action:?}"), total, action.clone(), scores)
        })
        .collect();

    votes.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    let (_, total, action, scores) = votes.remove(0);

    // Name the two drives that carried the vote (score, then canonical order).
    let mut ranked = scores.clone();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    let leader = ranked[0];
    let runner_up = ranked[1];

    let rationale = format!(
        "council selected {label}: {lead} {lead_score:.2} and {second} {second_score:.2} carried the vote (total {total:.2})",
        label = action_label(&action),
        lead = leader.0.label(),
        lead_score = leader.1,
        second = runner_up.0.label(),
        second_score = runner_up.1,
        total = total,
    );

    CouncilVote {
        action,
        scores,
        total,
        rationale,
    }
}

/// Weighted dot product of the warden's weights against a candidate's scores.
fn weighted_total(weights: &DriveWeights, scores: &[f32; Drive::COUNT]) -> f32 {
    Drive::ALL
        .iter()
        .map(|d| weights.get(*d) * scores[d.index()])
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::WardenId;
    use alc_warden::WardenTraits;

    fn traits(strictness: f32, compassion: f32, reform: f32) -> WardenTraits {
        WardenTraits {
            strictness,
            compassion,
            political_ambition: 0.5,
            financial_discipline: 0.5,
            risk_tolerance: 0.5,
            reform_orientation: reform,
            authoritarian_tendencies: 0.5,
            staff_loyalty: 0.5,
            corruption_susceptibility: 0.2,
            reputation_concerns: 0.5,
            competence: 0.5,
        }
    }

    fn warden(t: WardenTraits) -> Warden {
        Warden::new(WardenId(0), "Test Warden", t)
    }

    #[test]
    fn drive_weights_follow_personality_traits() {
        let strict = DriveWeights::from_warden(&warden(traits(0.95, 0.1, 0.1)));
        let reformist = DriveWeights::from_warden(&warden(traits(0.1, 0.95, 0.95)));

        assert_ne!(strict, reformist, "different wardens must vote differently");
        assert!(
            strict.security > reformist.security,
            "strict warden must weight security higher ({} vs {})",
            strict.security,
            reformist.security
        );
        assert!(
            reformist.welfare > strict.welfare,
            "reformist warden must weight welfare higher ({} vs {})",
            reformist.welfare,
            strict.welfare
        );
        assert!(
            reformist.rehab > strict.rehab,
            "reformist warden must weight rehab higher ({} vs {})",
            reformist.rehab,
            strict.rehab
        );
        for drive in Drive::ALL {
            for w in [strict, reformist] {
                let v = w.get(drive);
                assert!(
                    (0.05..=2.0).contains(&v),
                    "{:?} weight {v} out of range",
                    drive
                );
            }
        }
    }

    #[test]
    fn candidate_menu_always_offers_a_hold_steady_option() {
        let world = World::new(7);
        let candidates = candidate_actions(&world);
        assert!(
            candidates
                .iter()
                .any(|a| matches!(a, WardenAction::NoAction { .. })),
            "NoAction must always be on the ballot: {candidates:?}"
        );
    }

    #[test]
    fn holding_steady_scores_exactly_neutral() {
        let world = World::new(13);
        let view = StateView::from_world(&world);
        let scores = score_action(
            &view,
            &WardenAction::NoAction {
                rationale: "test".into(),
            },
        );
        for score in scores {
            assert!(
                (score - 0.5).abs() < f32::EPSILON,
                "expected 0.5, got {score}"
            );
        }
    }

    #[test]
    fn deliberation_is_deterministic_for_one_snapshot() {
        let world = World::new(11);
        let weights = DriveWeights::from_warden(&world.warden);
        let candidates = candidate_actions(&world);

        let first = deliberate(&world, &weights, &candidates);
        let second = deliberate(&world, &weights, &candidates);

        assert_eq!(first.total.to_bits(), second.total.to_bits());
        assert_eq!(first.rationale, second.rationale);
        assert!(
            actions_equivalent(&first.action, &second.action),
            "same snapshot must yield the same action"
        );
        assert_eq!(first.scores.len(), Drive::COUNT);
    }

    #[test]
    fn empty_ballot_holds_steady() {
        let world = World::new(5);
        let weights = DriveWeights::default();
        let vote = deliberate(&world, &weights, &[]);
        assert!(matches!(vote.action, WardenAction::NoAction { .. }));
        assert!(vote.rationale.contains("no candidates"));
    }

    #[test]
    fn crowding_makes_expansion_worth_it() {
        let mut world = World::new(17);
        // Fill the cell block to its rated capacity. `generate_intake` is
        // private in alc-world, so clone a seeded prisoner with fresh ids —
        // the council only reads population/capacity, not intake specifics.
        let capacity = world.capacity();
        let template = world.prisoners[0].clone();
        while world.population() < capacity {
            let id = world.next_prisoner_id;
            world.next_prisoner_id += 1;
            let mut p = template.clone();
            p.id = alc_core::PrisonerId(id);
            world.prisoners.push(p);
        }

        let balanced = DriveWeights::default();
        let expand = WardenAction::Expand {
            building: "cell_block".into(),
        };
        let hold = WardenAction::NoAction {
            rationale: "hold".into(),
        };

        // An equal-weight council must prefer expansion for a full prison: the
        // welfare/stability gains outweigh the capital cost.
        let vote = deliberate(&world, &balanced, &[expand.clone(), hold.clone()]);
        assert!(
            matches!(vote.action, WardenAction::Expand { .. }),
            "a full prison should expand under balanced weights: {}",
            vote.rationale
        );
        assert!(
            vote.score(Drive::Welfare) > 0.5,
            "crowding must make expansion welfare-positive: {:?}",
            vote.scores
        );

        // In an almost-empty prison the capital cost must dominate: the
        // welfare gain scales with crowding, so with few prisoners expansion
        // loses to holding steady.
        let mut roomy = World::new(17);
        roomy.prisoners.truncate(1);
        let calm = deliberate(&roomy, &balanced, &[expand, hold]);
        assert!(
            matches!(calm.action, WardenAction::NoAction { .. }),
            "expansion must not win when there is plenty of room: {}",
            calm.rationale
        );
    }
}
