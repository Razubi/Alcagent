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
use alc_guard::{DutyContext, GuardReport};
use alc_warden::{DecisionSource, WardenAction};
use alc_world::World;

/// Duty context for an explicit engine-driven guard cycle.
fn guard_context(sim: &Sim, at: SimTime) -> DutyContext {
    DutyContext {
        at,
        lockdown: sim.world.policies.lockdown,
        security_level: sim.world.policies.security_level,
    }
}

/// Run the duty loop of every guard once (§10): tick + observe + report +
/// enforce + request_assistance. Observations use the same area-scoped helper
/// as the live world sweep, so a guard only perceives its assigned facility.
pub fn guard_duty_cycle(sim: &mut Sim) -> Vec<(GuardId, GuardReport)> {
    let at = sim.world.time;
    let seed = sim.world.seed;
    let ctx = guard_context(sim, at);
    let plans: Vec<(GuardId, Vec<alc_guard::GuardObservation>)> = sim
        .world
        .guards
        .iter()
        .map(|g| (g.id, sim.world.guard_observations(g.id)))
        .collect();
    let mut rng = RngStream::new(seed, at.as_secs() ^ 0x6A11);
    let mut out = Vec::new();
    for (gid, observations) in plans {
        let Some(g) = sim.world.guards.iter_mut().find(|g| g.id == gid) else {
            continue;
        };
        let result = g.duty_tick_area(&observations, ctx, &mut rng);
        out.extend(result.reports.into_iter().map(|r| (gid, r)));
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
        let observations = sim.world.guard_observations(guard_id);
        let ctx = guard_context(sim, at);
        let Some(g) = sim.world.guards.iter_mut().find(|g| g.id == guard_id) else {
            continue;
        };
        let mut rng = RngStream::new(seed, at.as_secs() ^ g.id.raw());
        out.extend(
            g.duty_tick_area(&observations, ctx, &mut rng)
                .reports
                .into_iter(),
        );
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

/// Council mode: deliberate from report/budget summaries, apply the winner,
/// and emit observer-facing rationale. The institutional drive weights remain
/// visible so the council's competing pressures stay explainable.
fn run_council_path(sim: &mut Sim) -> WardenAction {
    let weights = DriveWeights::from_warden(&sim.world.warden);
    let decision = sim.decide_warden_strategy();
    let rationale = decision.rationale.clone();
    let conflicts = decision.conflicts.clone();
    let action = sim.apply_strategy_decision(decision, DecisionSource::Council);

    let summary = format!(
        "Council decision (council strategy): {} — {} | constraints: {} (weights: {})",
        council::action_label(&action),
        rationale,
        if conflicts.is_empty() {
            "none".to_string()
        } else {
            conflicts.join("; ")
        },
        summarise_weights(&weights),
    );
    let at = sim.world.time;
    let mut events = vec![
        WorldEvent::new(0, at, EventKind::PolicyChange, summary)
            .with_cause("warden strategy over inbox and budget summaries; council drive weights"),
    ];
    sim.emit_events(&mut events);

    action
}

/// Explicit strategy cycle: decide from the report-only context and apply the
/// result with a full audit trail. This is the non-modal counterpart to the
/// Council policy mode and is useful for scripted observers/tests.
pub fn warden_strategy_cycle(sim: &mut Sim) -> Vec<WardenAction> {
    let decision = sim.decide_warden_strategy();
    vec![sim.apply_strategy_decision(decision, DecisionSource::AiArbitration)]
}

/// Shadow mode: run both paths, apply only the baseline, record the comparison.
fn run_shadow_path(sim: &mut Sim) -> WardenAction {
    // Both decisions read the same pre-application snapshot, so the comparison
    // is fair and the council cannot perturb the baseline trajectory.
    let baseline = deterministic_arbitration(sim);
    let weights = DriveWeights::from_warden(&sim.world.warden);
    let decision = sim.decide_warden_strategy();
    let alternative = decision.action.clone();
    let rationale = decision.rationale.clone();

    // Baseline is authoritative in shadow mode; it is applied exactly as the
    // deterministic path would apply it.
    sim.apply_warden_action(baseline.clone());

    let verdict = if council::actions_equivalent(&baseline, &alternative) {
        "agreement"
    } else {
        "divergence"
    };
    let summary = format!(
        "SHADOW {verdict}: baseline={baseline:?} council={council_action:?} rationale={rationale}",
        council_action = alternative,
    );
    let at = sim.world.time;
    let mut events = vec![
        WorldEvent::new(0, at, EventKind::PolicyChange, summary).with_cause(format!(
            "shadow A/B: report-driven strategy; weights {}",
            summarise_weights(&weights)
        )),
    ];
    sim.emit_events(&mut events);

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
pub fn apply_warden_action(
    world: &mut World,
    _at: SimTime,
    action: WardenAction,
) -> Vec<WorldEvent> {
    // Single authoritative applier: the world owns all state mutations and
    // emits its own events. The engine mirrors them from the world event bus.
    world.apply_warden_action(action);
    Vec::new()
}

/// Deliver a guard duty report to the world: map severity → event kind,
/// update guard credibility, push the event id onto the warden inbox.
/// Returns the new event id (0 when the report was informational).
pub fn deliver_guard_report(sim: &mut Sim, guard_id: GuardId, report: GuardReport) -> u64 {
    // Warden receives reports, not omniscient events: implausible reports are
    // rejected at the engine boundary unless the guard is calling for help.
    if report.severity == 0 || (!report.requests_assistance && report.credibility < 0.25) {
        return 0;
    }
    let kind = match report.severity {
        1 => EventKind::Argument,
        2 => EventKind::Threat,
        _ => EventKind::Investigation,
    };
    let witness = u8::from(report.witnessed_directly);
    let cause = format!(
        "guard duty report; post={:?} sev={} cred={:.2} witness={} assist={} mistake={}",
        report.post,
        report.severity,
        report.credibility,
        witness,
        u8::from(report.requests_assistance),
        u8::from(report.summary.starts_with("Unverified"))
    );
    let id = sim.world.events.emit(
        WorldEvent::new(0, report.at, kind, report.summary.clone())
            .with_actor(guard_id.raw())
            .with_cause(cause)
            .with_confidence(report.credibility),
    );
    if let Some(g) = sim.world.guards.iter_mut().find(|g| g.id == guard_id) {
        g.report_credibility = report.credibility.clamp(0.05, 0.99);
    }
    if id != 0 {
        sim.world.warden.inbox.push(id);
        if sim.world.warden.inbox.len() > 200 {
            let drop = sim.world.warden.inbox.len() - 200;
            sim.world.warden.inbox.drain(0..drop);
        }
    }
    id
}
