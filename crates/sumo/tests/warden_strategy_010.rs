//! Spec 010 — warden institutional strategy.
//!
//! These tests exercise report-only deliberation, conflicting objectives,
//! financial gating, real budget consequences, attention costs, rationale
//! persistence, and long-run stability.

use alc_core::{GuardId, RngStream, TimeSpeed};
use alc_economy::{Account, Money};
use alc_guard::{GuardReport, PostType};
use alc_warden::{DecisionSource, FinanceHealthState, WardenAction, decide_strategy};
use alc_world::FacilityKind;
use std::sync::atomic::{AtomicUsize, Ordering};
use sumo::{PolicyMode, Sim};

static TEMP_SEQ: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(name: &str) -> std::path::PathBuf {
    let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("alcagent-010-{name}-{}-{n}", std::process::id()))
}

fn prepared(seed: u64, mode: PolicyMode) -> Sim {
    let mut sim = Sim::new(seed);
    sim.set_policy_mode(mode);
    sim.set_speed(TimeSpeed::Extreme);
    sim
}

fn corridor_guard(sim: &Sim) -> GuardId {
    sim.world()
        .guards
        .iter()
        .find(|g| g.post == PostType::Corridor)
        .expect("bootstrap should include a corridor guard")
        .id
}

fn credible_report(sim: &Sim, summary: &str, severity: u8, assistance: bool) -> GuardReport {
    GuardReport {
        at: sim.world().time,
        post: PostType::Yard,
        summary: summary.to_string(),
        severity,
        credibility: 0.92,
        witnessed_directly: true,
        requests_assistance: assistance,
    }
}

#[test]
fn strategy_context_is_built_from_delivered_reports_only() {
    let mut sim = Sim::new(1001);
    let gid = corridor_guard(&sim);
    let event_id = sim.deliver_guard_report(gid, credible_report(&sim, "yard fight", 3, true));
    assert!(event_id > 0);

    let snapshot = sim.warden_strategy_snapshot();
    assert_eq!(snapshot.reports.len(), 1);
    assert_eq!(snapshot.reports[0].id, event_id);
    assert_eq!(snapshot.reports[0].severity, 3);
    assert!(snapshot.reports[0].requests_assistance);
    assert!(snapshot.report_pressure() >= 3);
    assert_eq!(snapshot.priority_report(), Some(event_id));
}

#[test]
fn report_pressure_overrides_low_priority_objectives() {
    let mut sim = Sim::new(1002);
    let gid = corridor_guard(&sim);
    let event_id = sim.deliver_guard_report(
        gid,
        credible_report(&sim, "assistance requested in yard", 3, true),
    );
    let decision = sim.decide_warden_strategy();
    assert_eq!(
        decision.action,
        WardenAction::RespondToDisturbance { event_id }
    );
    assert!(decision.rationale.contains("Report pressure"));
    assert!(
        decision
            .conflicts
            .iter()
            .any(|c| c.contains("prisoner welfare"))
    );
}

#[test]
fn negative_cash_forces_expense_cut_before_expansion() {
    let mut sim = Sim::new(1003);
    sim.world_mut().economy.cash = Money::from_major(-500);
    sim.world_mut().warden.set_context(48, 0);

    let decision = sim.decide_warden_strategy();
    assert_eq!(decision.action, WardenAction::ReduceExpenses);
    assert_eq!(decision.finance_state, FinanceHealthState::Receivership);
    assert!(decision.conflicts.iter().any(|c| c.contains("Solvency")));
}

#[test]
fn low_runway_requests_funding_with_real_cash_effect_and_cooldown() {
    let mut sim = Sim::new(1004);
    {
        let world = sim.world_mut();
        world.economy.cash = Money::from_major(100);
        world.economy.record(
            world.time.as_secs(),
            Account::Maintenance,
            Money::from_major(-1_000),
            "test expense",
        );
        // Keep the institution solvent but with only a day of ledger runway.
        world.economy.cash = Money::from_major(100);
    }
    let decision = sim.decide_warden_strategy();
    let amount = match decision.action {
        WardenAction::RequestFunding { amount_major } => amount_major,
        ref other => panic!("low runway should request funding, got {other:?}"),
    };

    let cash_before = sim.world().economy.cash.0;
    sim.apply_strategy_decision(decision.clone(), DecisionSource::AiArbitration);
    let cash_after = sim.world().economy.cash.0;
    assert_eq!(cash_after - cash_before, Money::from_major(amount).0);
    assert!(
        sim.world()
            .economy
            .ledger
            .iter()
            .any(|e| e.note.starts_with("Warden funding request"))
    );

    // A second request on the same day must not mint money again.
    sim.apply_strategy_decision(decision, DecisionSource::AiArbitration);
    assert_eq!(sim.world().economy.cash.0, cash_after);
    assert!(
        sim.events()
            .recent()
            .any(|e| e.summary.contains("Funding request deferred"))
    );
}

#[test]
fn hiring_guards_has_recruitment_cost_and_bounded_count() {
    let mut sim = Sim::new(1005);
    let cash_before = sim.world().economy.cash.0;
    let guards_before = sim.world().guards.len();

    sim.apply_warden_action(WardenAction::HireGuards { count: 2 });

    assert_eq!(sim.world().guards.len(), guards_before + 2);
    assert_eq!(cash_before - sim.world().economy.cash.0, 400_000);
    assert!(
        sim.world()
            .economy
            .ledger
            .iter()
            .any(|e| e.note.starts_with("Warden hired 2 guards"))
    );
}

#[test]
fn active_construction_blocks_catalog_expansion() {
    let mut sim = Sim::new(1006);
    sim.enqueue_construction(FacilityKind::CellBlock, 1, 40);

    let mut snapshot = sim.warden_strategy_snapshot();
    assert_eq!(snapshot.active_construction, 1);
    snapshot.population = snapshot.capacity.saturating_add(10);
    let decision = decide_strategy(
        &snapshot,
        &sim.world().warden.traits,
        &mut RngStream::new(106, 0),
    );
    assert!(
        !matches!(decision.action, WardenAction::Expand { .. }),
        "an in-flight project must prevent a duplicate expansion: {:?}",
        decision.action
    );
}

#[test]
fn exhausted_attention_defers_non_emergency_strategy() {
    let mut sim = Sim::new(1007);
    sim.world_mut().warden.attention = 0.0;
    sim.world_mut().warden.security_incidents_this_week = 0;
    sim.world_mut().warden.inbox.clear();

    let applied = sim.warden_strategy_cycle();
    assert_eq!(applied.len(), 1);
    assert!(matches!(applied[0], WardenAction::NoAction { .. }));
    let decision = &sim.warden().recent_decisions[0];
    assert!(decision.rationale.to_lowercase().contains("attention"));
}

#[test]
fn council_rationale_is_visible_in_both_audit_stores() {
    let mut sim = prepared(1008, PolicyMode::Council);
    sim.run_days(1);
    sim.warden_cycle();

    let warden_summary = sim.warden();
    let decision = warden_summary
        .recent_decisions
        .first()
        .expect("warden decision history should be populated");
    assert!(decision.rationale.contains("constraints:"));
    assert_eq!(decision.source, DecisionSource::Council);

    let audit = sim
        .decision_audit_log()
        .last()
        .expect("engine decision audit should be populated");
    assert_eq!(audit.actor, "warden");
    assert_eq!(audit.rationale, decision.rationale);
}

#[test]
fn strategy_rationale_survives_save_and_load() {
    let mut sim = prepared(1009, PolicyMode::Council);
    sim.run_days(1);
    sim.warden_cycle();
    let expected = sim.warden().recent_decisions[0].rationale.clone();

    let dir = temp_dir("save-load");
    let _ = std::fs::remove_dir_all(&dir);
    sim.save(&dir).expect("save strategy rationale");
    let loaded = Sim::load(&dir).expect("load strategy rationale");

    assert_eq!(loaded.warden().recent_decisions[0].rationale, expected);
    assert_eq!(
        loaded.decision_audit_log().last().unwrap().rationale,
        expected
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn long_council_run_remains_bounded_and_consistent() {
    let mut sim = prepared(1010, PolicyMode::Council);
    for _ in 0..30 {
        sim.run_days(1);
        sim.warden_cycle();
    }
    // Resync is safe: NoAction does not mutate state, but it reconciles the
    // engine policy mirror after world-tick warden arbitration.
    sim.apply_warden_action(WardenAction::NoAction {
        rationale: "test policy mirror resync".into(),
    });
    assert!(sim.check_invariants().is_empty());
    assert!(sim.decision_audit_log().len() <= sumo::invariants::MAX_LOG_ENTRIES);
    assert!(sim.warden().recent_decisions.len() <= 20);
}
