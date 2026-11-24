//! Engine drivers (§2, §9, §10): guard duty cycles, warden cycles, engine-side
//! WardenAction application, guard report → event delivery.
//!
//! The engine is the control surface: guards tick + run their duty loop, the
//! warden arbitrates and has its actions applied by the engine (§9: "Actions
//! proposed by AI are validated by the engine before application").
//!
//! # Policy modes (R3)
//!
//! [`warden_cycle`] dispatches on [`crate::PolicyMode`]: `Deterministic` keeps
//! the historical `alc_warden::arbitrate` path byte-for-byte, `Council` applies
//! the deterministic drive vote from [`crate::council`], and `Shadow` runs both
//! and applies only the baseline while logging the comparison as an event.
//! Neither non-default mode uses an RNG, so same-seed runs stay reproducible.
//!
//! # Known architectural duplication (Wave 5 cleanup)
//!
//! `alc_world::World::tick` already runs its own hourly guard-duty sweep and
//! daily warden-arbitration cycle inside the world loop. These drivers expose
//! the same capabilities as explicit engine entry points
//! (`Sim::guard_duty_cycle`, `Sim::warden_cycle`) and are invoked by callers
//! such as tests and AI orchestration, but `Sim::tick` deliberately does **not**
//! call them — calling both per tick would double-report duty observations and
//! double-apply warden actions. This means there are currently two authorities
//! for the same behavior. Unifying them behind a single engine-owned driver
//! (and making the world tick delegate to it, or removing one path) is a Wave 5
//! task; do not "fix" this by simply calling these drivers from `Sim::tick`.

use crate::council::{self, DriveWeights};
use crate::{PolicyMode, Sim};
use alc_core::{GuardId, RngStream, SimTime};
use alc_events::{EventKind, WorldEvent};
use alc_guard::{DutyContext, Guard, GuardReport, GuardRank, PostType};
use alc_warden::WardenAction;
use alc_world::{FacilityKind, World};

/// Build the tension-aware guard context from current world state. Prisoner
/// need pressure is the best available proxy for local friction; the duty tick
/// derives incident load from its average/peak plus security posture.
fn guard_context(sim: &Sim, at: SimTime) -> (Vec<f32>, DutyContext) {
    let tensions: Vec<f32> = sim
        .world
        .prisoners
        .iter()
        .map(|p| p.needs.total_pressure().clamp(0.0, 1.0))
        .collect();
    let ctx = DutyContext {
        at,
        lockdown: sim.world.policies.lockdown,
        security_level: sim.world.policies.security_level,
    };
    (tensions, ctx)
}

/// Run the duty loop of every guard once (§10): tick + observe + report +
/// enforce + request_assistance; reports are collected. Uses the same
/// tension-aware duty tick as the live world loop (no fixed-load spam).
pub fn guard_duty_cycle(sim: &mut Sim) -> Vec<(GuardId, GuardReport)> {
    let at = sim.world.time;
    let seed = sim.world.seed;
    let (tensions, ctx) = guard_context(sim, at);
    let mut rng = RngStream::new(seed, at.as_secs() ^ 0x6A11);
    let mut out = Vec::new();
    for g in &mut sim.world.guards {
        for report in g.duty_tick(&tensions, ctx, &mut rng) {
            out.push((g.id, report));
        }
    }
    out
}

/// Run one guard's duty loop for `minutes` minutes; reports collected.
pub fn run_guard_duty(sim: &mut Sim, guard_id: GuardId, minutes: u64) -> Vec<GuardReport> {
    let seed = sim.world.seed;
    let start = sim.world.time;
    let mut out = Vec::new();
    for minute in 0..minutes {
        let at = start.add_mins(minute);
        let (tensions, ctx) = guard_context(sim, at);
        let Some(g) = sim.world.guards.iter_mut().find(|g| g.id == guard_id) else { continue };
        let mut rng = RngStream::new(seed, at.as_secs() ^ g.id.raw());
        out.extend(g.duty_tick(&tensions, ctx, &mut rng));
    }
    out
}

/// Warden cycle (§9): choose the next institutional action according to
/// [`PolicyMode`](crate::PolicyMode) and apply it with the engine-side applier.
/// Returns the applied action(s).
///
/// - [`PolicyMode::Deterministic`](crate::PolicyMode::Deterministic) — the
///   historical path, byte-for-byte unchanged: `alc_warden::arbitrate` picks the
///   action (utility scoring + RNG tie-break) and the engine applies it. Trace
///   parity with earlier builds depends on this branch not changing.
/// - [`PolicyMode::Council`](crate::PolicyMode::Council) — the deterministic
///   council in [`crate::council`] derives a candidate menu from world state,
///   weighs it with the warden's personality, and the engine applies the winner.
///   A `PolicyChange` event carries the rationale for observers.
/// - [`PolicyMode::Shadow`](crate::PolicyMode::Shadow) — both paths run on the
///   same snapshot; **only** the deterministic baseline is applied, and a
///   `PolicyChange` event records `SHADOW divergence`/`SHADOW agreement` with
///   both actions and the council's rationale. The council never mutates world
///   state, so the baseline trajectory is preserved exactly.
pub fn warden_cycle(sim: &mut Sim) -> Vec<WardenAction> {
    match sim.policy_mode() {
        PolicyMode::Deterministic => vec![run_deterministic_path(sim)],
        PolicyMode::Council => vec![run_council_path(sim)],
        PolicyMode::Shadow => vec![run_shadow_path(sim)],
    }
}

/// Baseline warden decision: `alc_warden::arbitrate` with the engine's RNG
/// seeding. Does not mutate the world — the caller applies the result.
fn deterministic_arbitration(sim: &mut Sim) -> WardenAction {
    let seed = sim.world.seed;
    let secs = sim.world.time.secs;
    let pop = sim.world.population();
    alc_warden::arbitrate(
        &mut sim.world.warden,
        &sim.world.economy,
        pop,
        &mut RngStream::new(seed, secs),
    )
}

/// Deterministic mode: arbitrate, apply, emit through the engine API so
/// policy mirror and decision audit stay consistent (0.05).
fn run_deterministic_path(sim: &mut Sim) -> WardenAction {
    let action = deterministic_arbitration(sim);
    sim.apply_warden_action(action.clone());
    action
}

/// Council mode: deliberate from world state, apply the winner, emit rationale.
fn run_council_path(sim: &mut Sim) -> WardenAction {
    let weights = DriveWeights::from_warden(&sim.world.warden);
    let candidates = council::candidate_actions(&sim.world);
    let vote = council::deliberate(&sim.world, &weights, &candidates);
    let action = vote.action.clone();

    // Apply through the engine API so the policy mirror is reconciled
    // (`Sim::apply_warden_action` is the validated application surface).
    sim.apply_warden_action(action.clone());

    let summary = format!(
        "Council decision: {} — {} (weights: {})",
        council::action_label(&action),
        vote.rationale,
        summarise_weights(&weights),
    );
    let at = sim.world.time;
    sim.emit_events(vec![
        WorldEvent::new(0, at, EventKind::PolicyChange, summary)
            .with_cause("council-mode deliberation; deterministic drive dot product"),
    ]);

    action
}

/// Shadow mode: run both paths, apply only the baseline, record the comparison.
fn run_shadow_path(sim: &mut Sim) -> WardenAction {
    // Both decisions read the same pre-application snapshot, so the comparison
    // is fair and the council cannot perturb the baseline trajectory.
    let baseline = deterministic_arbitration(sim);
    let weights = DriveWeights::from_warden(&sim.world.warden);
    let candidates = council::candidate_actions(&sim.world);
    let vote = council::deliberate(&sim.world, &weights, &candidates);

    // Baseline is authoritative in shadow mode; it is applied exactly as the
    // deterministic path would apply it.
    sim.apply_warden_action(baseline.clone());

    let verdict = if council::actions_equivalent(&baseline, &vote.action) {
        "agreement"
    } else {
        "divergence"
    };
    let summary = format!(
        "SHADOW {verdict}: baseline={baseline:?} council={council_action:?} rationale={rationale}",
        council_action = vote.action,
        rationale = vote.rationale,
    );
    let at = sim.world.time;
    sim.emit_events(vec![
        WorldEvent::new(0, at, EventKind::PolicyChange, summary).with_cause(format!(
            "shadow A/B: council total {:.2}; weights {}",
            vote.total,
            summarise_weights(&weights)
        )),
    ]);

    baseline
}

/// Compact weight profile for event summaries/logs.
fn summarise_weights(weights: &DriveWeights) -> String {
    let mut s = String::new();
    for drive in council::Drive::ALL {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&format!("{}={:.2}", drive.label(), weights.get(drive)));
    }
    s
}


/// Warden actions still pending (§9).
pub fn warden_pending(sim: &Sim) -> Vec<WardenAction> {
    sim.world.warden.pending_actions.clone()
}

/// Engine-side application of a WardenAction (§9: engine validates + applies).
/// Returns events for the world bus.
pub fn apply_warden_action(world: &mut World, at: SimTime, action: WardenAction) -> Vec<WorldEvent> {
    let mut events = Vec::new();
    match action {
        WardenAction::Expand { building } => {
            let kind = match building.as_str() {
                "medical" => FacilityKind::Medical,
                "education" => FacilityKind::Education,
                "isolation" => FacilityKind::Isolation,
                "visitor" => FacilityKind::Visitor,
                "workshop" => FacilityKind::Workshop,
                "recreation" => FacilityKind::Recreation,
                _ => FacilityKind::CellBlock,
            };
            // Queue construction (0.05) rather than instant build; enqueue_construction
            // emits its own event to world.events.
            let _pid = world.enqueue_construction(kind, 1, 40);
        }
        WardenAction::HireGuards { count } => {
            // Every pawn must be in exactly one place (invariant
            // `pawn_in_exactly_one_place`). New guards are not placed by the
            // world's schedule-based location seeding until the next tick, so
            // seed a location here immediately. Facility 0 (or the first
            // facility when ids are sparse) is the safe default for a Corridor
            // post; the next world tick re-derives locations from posts.
            let home_facility = world.facilities.first().map(|f| f.id).unwrap_or(alc_core::FacilityId(0));
            for _ in 0..count {
                let id = world.next_guard_id;
                world.next_guard_id += 1;
                let guard_id = GuardId(id);
                world.guards.push(Guard::new(
                    guard_id,
                    format!("Guard {id}"),
                    GuardRank::Officer,
                    PostType::Corridor,
                    &mut RngStream::new(world.seed, id),
                ));
                world.guard_location.insert(guard_id, home_facility);
            }
            events.push(WorldEvent::new(0, at, EventKind::PolicyChange, format!("Hired {count} guards")));
        }
        WardenAction::FireGuards { count } => {
            for _ in 0..count.min(world.guards.len() as u32) {
                if let Some(guard) = world.guards.pop() {
                    // Remove the departing guard's location too: a stale entry
                    // would violate the "exactly one place" invariant for a
                    // pawn that no longer exists.
                    world.guard_location.remove(&guard.id);
                }
            }
            events.push(WorldEvent::new(0, at, EventKind::PolicyChange, format!("Fired guards (up to {count})")));
        }
        WardenAction::PromoteStaff { guard } => {
            let mut summary = format!("Promotion ordered for guard {guard}");
            if let Some(g) = world.guards.iter_mut().find(|g| g.id.raw() == guard) {
                g.rank = match g.rank {
                    GuardRank::Officer => GuardRank::Sergeant,
                    GuardRank::Sergeant => GuardRank::Lieutenant,
                    GuardRank::Lieutenant => GuardRank::Captain,
                    GuardRank::Captain => GuardRank::Captain,
                };
                summary = format!("Guard {} promoted to {:?}", g.id.raw(), g.rank);
            } else {
                summary = format!("Promotion ordered for unknown guard {guard} (no change)");
            }
            events.push(WorldEvent::new(0, at, EventKind::PolicyChange, summary));
        }
        WardenAction::ChangePolicy { policy, value } => {
            let value = value as f32;
            let mut applied = true;
            match policy.as_str() {
                "privileges" => world.policies.privileges_level = value.clamp(0.0, 1.0),
                "security" => world.policies.security_level = value.clamp(0.0, 1.0),
                "rehab_funding" => world.policies.rehab_funding_multiplier = value.clamp(0.2, 3.0),
                "lockdown" => {
                    // NOTE: this driver only has `&mut World`, so it cannot
                    // update `Sim::engine_state.policy_state` (the authoritative
                    // policy/lockdown mirror used by `check_invariants`).
                    // Callers MUST use `Sim::set_lockdown(true)` instead of
                    // applying this action directly when a real lockdown is
                    // intended. The emitted event records the attempted change
                    // so it is visible to the warden/UI; Wave 5 should change
                    // the applier signature to take `&mut Sim` and sync state.
                    world.policies.lockdown = true;
                }
                _ => applied = false,
            }
            events.push(WorldEvent::new(
                0,
                at,
                EventKind::PolicyChange,
                format!(
                    "Policy change: {policy} = {value:.2}{}",
                    if applied { "" } else { " (unknown policy, ignored)" }
                ),
            ));
        }
        WardenAction::IncreaseSecurity => {
            world.policies.security_level = (world.policies.security_level + 0.1).min(1.0);
            events.push(WorldEvent::new(
                0,
                at,
                EventKind::PolicyChange,
                format!("Security increased to {:.2}", world.policies.security_level),
            ));
        }
        WardenAction::ReduceExpenses => {
            // Death-spiral guard: only cut rehab spending every 7 days, and
            // only when financial health is genuinely poor (mirrors the
            // authoritative alc-world implementation).
            let days_since = world.days_simulated.saturating_sub(world.warden.last_action_day);
            if world.economy.health() < 0.4 && days_since >= 7 {
                world.policies.rehab_funding_multiplier =
                    (world.policies.rehab_funding_multiplier - 0.1).max(0.2);
                world.warden.last_action_day = world.days_simulated;
                world.warden.last_decision_at = at;
                events.push(WorldEvent::new(0, at, EventKind::PolicyChange,
                    "Expenses reduced; rehab funding cut"));
            }
        }
        WardenAction::ImproveInfrastructure { facility } => {
            let summary = if let Some(f) = world.facilities.iter_mut().find(|f| f.id.0 == facility) {
                f.condition = (f.condition + 0.05).min(1.0);
                format!("Infrastructure improved at facility {facility} (condition {:.2})", f.condition)
            } else {
                format!("Infrastructure improvement ordered for unknown facility {facility}")
            };
            events.push(WorldEvent::new(0, at, EventKind::BudgetChange, summary));
        }
        WardenAction::RespondToDisturbance { event_id } => {
            events.push(WorldEvent::new(
                0,
                at,
                EventKind::Investigation,
                format!("Warden responded to disturbance (event {event_id})"),
            ));
        }
        WardenAction::NegotiateWithRepresentatives => {
            events.push(WorldEvent::new(0, at, EventKind::PolicyChange, "Warden negotiated with prisoner representatives"));
        }
        WardenAction::InvestigateMisconduct { target } => {
            events.push(WorldEvent::new(0, at, EventKind::Investigation, format!("Misconduct investigation initiated: {target}")));
        }
        WardenAction::AdjustParoleRecommendation { prisoner, lean } => {
            events.push(WorldEvent::new(
                0,
                at,
                EventKind::ParoleBoardDecision,
                format!("Parole recommendation adjusted: {prisoner} lean={lean:.2}"),
            ));
        }
        WardenAction::RequestFunding { amount_major } => {
            events.push(WorldEvent::new(0, at, EventKind::BudgetChange, format!("Funding request: {amount_major}")));
            world.warden.attention = (world.warden.attention - 0.05).max(0.0);
        }
        WardenAction::NoAction { .. } => {}
    }
    events
}

/// Deliver a guard duty report to the world: map severity → event kind,
/// update guard credibility, push the event id onto the warden inbox.
/// Returns the new event id (0 when the report was informational).
pub fn deliver_guard_report(sim: &mut Sim, guard_id: GuardId, report: GuardReport) -> u64 {
    let kind = match report.severity {
        0 => return 0,
        1 => EventKind::Argument,
        2 => EventKind::Threat,
        _ => EventKind::Investigation,
    };
    sim.world.events.emit(
        WorldEvent::new(0, report.at, kind, report.summary.clone()).with_actor(guard_id.raw()),
    );
    let id = sim.world.events.recent().last().map(|e| e.id).unwrap_or(0);
    if let Some(g) = sim.world.guards.iter_mut().find(|g| g.id == guard_id) {
        g.report_credibility = report.credibility.clamp(0.05, 0.99);
    }
    sim.world.warden.inbox.push(id);
    id
}

