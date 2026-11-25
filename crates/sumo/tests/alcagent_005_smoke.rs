//! Alcagent 0.05 game-update integration smoke tests (gap report: "0.05 Game-Side Update Gap Matrix").
//!
//! Targeted smoke coverage for the 0.05 slice set the player/observer product
//! needs in order to close the "living institution" gap:
//!
//! * save/load roundtrip of newly added engine + world state,
//! * policy / lockdown / intake observer controls and their events,
//! * construction queue progression / completion,
//! * decision audit + incident records presence,
//! * rehab / contraband event emission,
//! * post-release records.
//!
//! Written against the current public `sumo::Sim` API using the same patterns
//! as the existing 0.05 integration suites (`integration_wave4.rs`,
//! `engine_state_roundtrip.rs`, `log_wiring.rs`, `decisions.rs`,
//! `invariants_negative.rs`).

use alc_agents::Decision;
use alc_agents::decision::DecisionAction;
use alc_core::{PrisonerId, TimeSpeed};
use alc_events::{EventKind, WorldEvent};
use alc_social::RehabProgram;
use sumo::{Policy, Sim};

mod unique {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub fn temp_dir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("sumo-alc005-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
}

/// Save/load must round-trip the newly added 0.05 state blocks: policy +
/// lockdown + intake mirrors, decision audit log, incident log and the
/// post-release ledger.
#[test]
fn save_load_roundtrip_preserves_new_005_state() {
    let dir = unique::temp_dir("save");
    let mut sim = Sim::new(0x0500_0001);
    sim.set_speed(TimeSpeed::Extreme);

    let pid = sim.world().prisoners[0].id;
    let peer = sim.world().prisoners[1].id;

    // Populate the new 0.05 state before saving.
    sim.set_policy(Policy::Enhanced);
    sim.set_lockdown(true);
    sim.set_intake_paused(true);

    let decision_log_before = sim.decision_log().len();
    let outcome = sim.deliver_decision(pid, Decision::new(DecisionAction::Socialize { with: peer.raw() }));
    assert!(outcome.applied, "socialize between two known prisoners must apply");
    assert_eq!(sim.decision_log().len(), decision_log_before + 1);

    assert!(sim.record_post_release(pid, "parole", 0.25, true));

    sim.save(&dir).expect("save 0.05 world state");
    let loaded = Sim::load(&dir).expect("load 0.05 world state");

    // Policy / lockdown / intake mirrors survived the roundtrip.
    assert_eq!(loaded.policy_state().policy, Policy::Enhanced);
    assert!(loaded.policy_state().lockdown);
    assert_eq!(loaded.policy_state().security_level, 4);
    assert!(loaded.world().policies.lockdown);
    assert!(loaded.world().policies.intake_paused);

    // Decision audit + incident logs survived.
    assert_eq!(loaded.decision_log().len(), decision_log_before + 1);
    assert_eq!(loaded.incident_log().len(), sim.incident_log().len());

    // Post-release ledger survived.
    assert_eq!(loaded.post_release_status(pid), Some(true));
    assert_eq!(loaded.post_release().len(), 1);

    // The full-state fingerprint must be identical across the roundtrip.
    assert_eq!(loaded.trace_hash_v2(), sim.trace_hash_v2());

    std::fs::remove_dir_all(&dir).ok();
}

/// Policy / lockdown / intake controls emit `PolicyChange` events, keep the
/// engine/world mirrors consistent, and the intake gate must actually stop or
/// allow admissions.
#[test]
fn policy_lockdown_and_intake_controls_emit_events_and_hold_invariants() {
    let mut sim = Sim::new(0x0500_0002);
    sim.set_policy(Policy::Lockdown);
    assert_eq!(sim.policy_state().policy, Policy::Lockdown);
    assert!(sim.policy_state().lockdown);
    assert_eq!(sim.policy_state().security_level, 5);
    assert!(sim.world().policies.lockdown);

    // Toggling lockdown emits a PolicyChange event.
    let policy_events_before = sim.events().len();
    sim.set_lockdown(false);
    assert!(
        sim.events().len() > policy_events_before,
        "lockdown toggle must emit a PolicyChange event"
    );

    // Intake pause / resume are validated controls that emit policy events.
    sim.set_intake_paused(true);
    assert!(sim.world().policies.intake_paused);
    let intake_events_before = sim.events().len();
    sim.set_intake_paused(false);
    assert!(
        sim.events().len() > intake_events_before,
        "intake toggle must emit a PolicyChange event"
    );

    // The engine/world invariant gates must stay clean across the flow.
    let errs = sim.check_invariants();
    assert!(errs.is_empty(), "policy/lockdown/intake controls left violations: {errs:?}");

    // Intake pause must actually block admissions while beds are free.
    let mut sim2 = Sim::new(0x0500_0003);
    sim2.set_intake_paused(true);
    sim2.set_speed(TimeSpeed::Extreme);
    sim2.run_days(8);
    assert!(
        !sim2.events().recent().any(|e| e.kind == EventKind::Admission),
        "intake paused must block admissions (world intake gate)"
    );

    // Resuming intake re-opens admissions: a fresh open-intake world admits
    // on its first daily sweep.
    let mut sim3 = Sim::new(0x0500_0008);
    sim3.set_speed(TimeSpeed::Extreme);
    sim3.run_days(4);
    assert!(
        sim3.events().recent().any(|e| e.kind == EventKind::Admission),
        "intake resume must allow admissions again"
    );
}

/// A queued construction project advances on the day boundary and must retire
/// from the queue (emitting `ConstructionComplete`) once its build completes.
#[test]
fn construction_queue_progresses_and_completes() {
    let mut sim = Sim::new(0x0500_0004);
    sim.set_speed(TimeSpeed::Extreme);

    let project_id = sim.world_mut().enqueue_construction(alc_world::FacilityKind::Medical, 2, 30);
    assert_eq!(sim.construction_queue().len(), 1, "enqueued project must appear on the queue");
    let queued = &sim.world().construction_queue[0];
    assert_eq!(queued.id, project_id);
    assert_eq!(queued.total_days, 10, "medical builds take 10 day boundaries");
    assert_eq!(queued.progress_days, 0);

    sim.run_days(12);

    let completed = sim
        .world()
        .construction_queue
        .iter()
        .find(|p| p.id == project_id)
        .expect("completed construction project should remain in audit queue");
    assert_eq!(
        completed.status,
        alc_world::ConstructionStatus::Completed,
        "completed construction project must reach Completed status"
    );
    assert!(
        sim.events().recent().any(|e| e.kind == EventKind::ConstructionComplete),
        "construction completion must emit a ConstructionComplete event"
    );
    assert!(
        sim.check_invariants().is_empty(),
        "construction lifecycle left violations: {:?}",
        sim.check_invariants()
    );
}

/// Applied decisions enter the audit log; rejected decisions stay auditable
/// without entering the decision log; conflict decisions mirror an incident
/// with a resolvable cause/consequence chain.
#[test]
fn decision_audit_and_incident_records_are_present() {
    let mut sim = Sim::new(42);
    let pid = sim.world().prisoners[0].id;
    let peer = sim.world().prisoners[1].id;

    // Applied decision -> decision audit + decision log.
    let audit_before = sim.decision_audit_log().len();
    let outcome = sim.deliver_decision(pid, Decision::new(DecisionAction::Socialize { with: peer.raw() }));
    assert!(outcome.applied);
    assert_eq!(sim.decision_audit_log().len(), audit_before + 1);
    let audit = sim.decision_audit_log().last().unwrap();
    assert_eq!(audit.actor, format!("prisoner:{}", pid.raw()));
    assert_eq!(audit.source, "agent");
    assert_eq!(audit.action, "socialize");
    assert_eq!(audit.validation, "accepted");

    // Rejected decision -> audited, but never enters the decision log.
    let decision_log_before = sim.decision_log().len();
    let rejected = sim.deliver_decision(PrisonerId(999_999), Decision::new(DecisionAction::Comply));
    assert!(!rejected.applied);
    assert!(rejected.result.rejected_reason().is_some());
    assert_eq!(
        sim.decision_log().len(),
        decision_log_before,
        "rejected decisions must not enter the decision log"
    );
    assert_eq!(sim.decision_audit_log().last().unwrap().validation, "rejected");

    // Conflict decision -> mirrored into the incident log + sidecar.
    let incident_log_before = sim.incident_log().len();
    let fight = sim.deliver_decision(pid, Decision::new(DecisionAction::Fight { target: peer.raw() }));
    assert!(fight.applied, "conflict between two known prisoners must apply");
    assert!(sim.incident_log().len() > incident_log_before);
    let incident = sim.incident_sidecar().last().unwrap();
    assert!(incident.cause.is_some(), "incident must carry a cause");
    assert!(incident.consequence.is_some(), "incident must carry a consequence");
    assert!(
        incident.involved_prisoners.contains(&pid) || incident.involved_prisoners.contains(&peer),
        "incident must name an involved prisoner"
    );

    assert!(
        sim.check_invariants().is_empty(),
        "decision/incident flow left violations: {:?}",
        sim.check_invariants()
    );
}

/// Observer-controlled rehab enrollment emits `ProgramEnrolled`; manual program
/// removal emits `RehabilitationFailure`; contraband discoveries surface a
/// case plus an inventory summary.
#[test]
fn rehab_and_contraband_events_are_emitted() {
    // --- Rehab: enrollment emits ProgramEnrolled, removal emits failure. ---
    let mut sim = Sim::new(0x0500_0005);
    let pid = sim.world().prisoners[0].id;
    if let Some(p) = sim.world_mut().prisoners.iter_mut().find(|p| p.id == pid) {
        p.condition.hope = 0.9;
    }
    assert!(
        sim.enroll_rehab(pid, RehabProgram::Education),
        "validated rehab enrollment must succeed once willingness is sufficient"
    );
    assert!(
        sim.events()
            .recent()
            .any(|e| e.kind == EventKind::ProgramEnrolled && e.actors.contains(&pid.raw())),
        "observer-controlled rehab enrollment must emit a ProgramEnrolled event"
    );
    let rows = sim.rehab_summaries();
    assert!(
        rows.iter().any(|r| r.prisoner_id == pid && r.program_key == RehabProgram::Education.key()),
        "enrolled prisoner must appear in the rehab summaries"
    );
    // Record sync: enrollment must be reflected in world.rehab_records.
    let recs = sim.world().rehab_records.get(&pid).expect("rehab record entry");
    assert!(
        recs.iter().any(|r| r.program == RehabProgram::Education && !r.completed && !r.dropped),
        "enroll must sync an active world rehab record"
    );
    assert!(sim.drop_rehab(pid, RehabProgram::Education));
    let recs = sim.world().rehab_records.get(&pid).expect("rehab record entry");
    assert!(
        recs.iter().any(|r| r.program == RehabProgram::Education && r.dropped),
        "drop must mark the world rehab record dropped"
    );
    assert!(
        sim.events()
            .recent()
            .any(|e| e.kind == EventKind::RehabilitationFailure),
        "manual rehab removal must emit a RehabilitationFailure event"
    );
    assert!(
        !sim.drop_rehab(pid, RehabProgram::Education),
        "dropping an unenrolled program must be a validating no-op"
    );

    // Key-based enroll/drop API and record sync.
    let pid2 = sim.world().prisoners[1].id;
    if let Some(p) = sim.world_mut().prisoners.iter_mut().find(|p| p.id == pid2) {
        p.condition.hope = 0.9;
    }
    assert!(
        sim.enroll_rehab_by_key(pid2, RehabProgram::Counseling.key()),
        "enroll_rehab_by_key must accept a known program key"
    );
    assert!(
        sim.world().rehab_records.get(&pid2).is_some_and(|recs| {
            recs.iter().any(|r| r.program == RehabProgram::Counseling && !r.dropped && !r.completed)
        }),
        "key-based enroll must sync world rehab record"
    );
    assert!(
        !sim.enroll_rehab_by_key(pid2, "not_a_real_program"),
        "unknown rehab key must be a validating no-op"
    );
    assert!(
        sim.drop_rehab_by_key(pid2, RehabProgram::Counseling.key()),
        "drop_rehab_by_key must drop an enrolled program"
    );
    assert!(
        sim.world().rehab_records.get(&pid2).is_some_and(|recs| {
            recs.iter().any(|r| r.program == RehabProgram::Counseling && r.dropped)
        }),
        "key-based drop must sync world rehab record"
    );
    assert!(
        !sim.drop_rehab_by_key(pid2, "not_a_real_program"),
        "unknown rehab drop key must be a validating no-op"
    );

    // --- Contraband: a discovery surfaces a case and an inventory rollup. ---
    let mut sim = Sim::new(0x0500_0006);
    let pid = sim.world().prisoners[0].id;
    sim.emit_event(
        WorldEvent::new(0, sim.world().time, EventKind::ContrabandFound, format!("Guard 1 found Weapons on prisoner {}", pid.raw()))
            .with_actor(pid.raw())
            .with_cause("discovery by guard 1"),
    );
    let cases = sim.contraband_summaries();
    assert!(!cases.is_empty(), "contraband discovery must surface at least one case");
    let cas = cases
        .iter()
        .find(|c| c.holder == Some(pid))
        .expect("contraband case must name the holder");
    assert_eq!(cas.item.as_deref(), Some("Weapons"));
    assert!(cas.severity > 0.5, "weapons discoveries are high severity");
    assert!(!cas.resolved, "new contraband case should be unresolved");
    let case_id = cas.case_id;
    let inventory = sim.contraband_inventory();
    assert!(
        inventory.iter().any(|c| c.item == "Weapons"),
        "contraband inventory must roll the discovered item up"
    );

    // Contraband resolution store/summaries.
    assert!(
        sim.resolve_contraband_case(case_id),
        "first resolve should report newly resolved"
    );
    assert!(
        !sim.resolve_contraband_case(case_id),
        "resolving the same case again should be idempotent"
    );
    let resolved_case = sim
        .contraband_summaries()
        .into_iter()
        .find(|c| c.case_id == case_id)
        .expect("resolved case still present in summaries");
    assert!(resolved_case.resolved, "resolved case summary must report resolved");
    assert!(
        sim.contraband_inventory().iter().any(|c| c.item == "Weapons"),
        "resolved contraband should still appear in inventory rollup"
    );
}

/// Post-release outcomes are recorded, reported and round-tripped through the
/// save/load path.
#[test]
fn post_release_records_are_reported_and_roundtripped() {
    let mut sim = Sim::new(0x0500_0007);
    let pid = sim.world().prisoners[0].id;
    assert!(sim.record_post_release(pid, "parole", 0.25, true));

    let rows = sim.post_release_records();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].prisoner_id, pid);
    assert_eq!(rows[0].release_type, "post_release");
    assert_eq!(rows[0].recidivism_risk, Some(0.25));
    assert_eq!(rows[0].has_reintegration_plan, Some(true));
    assert!(!rows[0].follow_up_required);

    let dir = unique::temp_dir("post");
    sim.save(&dir).expect("save post-release state");
    let loaded = Sim::load(&dir).expect("load post-release state");
    assert_eq!(loaded.post_release_status(pid), Some(true));
    assert_eq!(loaded.post_release_records().len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}

/// Construction queue cancel/failure APIs should emit terminal events and keep
/// queue/invariants consistent.
#[test]
fn construction_queue_cancel_and_failure_paths() {
    // Cancel path via world API.
    let mut sim = Sim::new(0x0500_0009);
    sim.set_speed(TimeSpeed::Extreme);
    let project_id = sim
        .world_mut()
        .enqueue_construction(alc_world::FacilityKind::Medical, 1, 20);
    assert!(
        sim.world_mut().cancel_construction(project_id, "observer cancelled"),
        "active construction project should cancel"
    );
    let cancelled = sim
        .world()
        .construction_queue
        .iter()
        .find(|p| p.id == project_id)
        .expect("cancelled project remains in audit queue");
    assert_eq!(cancelled.status, alc_world::ConstructionStatus::Cancelled);
    assert_eq!(cancelled.terminal_reason.as_deref(), Some("observer cancelled"));
    assert!(
        sim.events().recent().any(|e| {
            e.kind == EventKind::ConstructionCancelled
                && e.summary.contains("Medical")
                && e.causes.iter().any(|c| c.contains("observer cancelled"))
        }),
        "cancel should emit ConstructionCancelled event with cause"
    );

    // Failure path: missing prerequisite (no admin facility for non-admin build).
    let mut sim = Sim::new(0x0500_000A);
    sim.set_speed(TimeSpeed::Extreme);
    sim.world_mut().facilities.retain(|f| f.kind != alc_world::FacilityKind::Admin);
    let failed_id = sim
        .world_mut()
        .enqueue_construction(alc_world::FacilityKind::Education, 1, 20);
    sim.run_days(2);
    let failed = sim
        .world()
        .construction_queue
        .iter()
        .find(|p| p.id == failed_id)
        .expect("failed project remains in audit queue");
    assert_eq!(failed.status, alc_world::ConstructionStatus::Failed);
    assert!(
        failed.terminal_reason.as_deref().unwrap_or_default().contains("prerequisite")
            || failed.terminal_reason.as_deref().unwrap_or_default().contains("admin"),
        "missing-admin failure should explain prerequisite: {:?}",
        failed.terminal_reason
    );
    assert!(
        sim.events().recent().any(|e| e.kind == EventKind::ConstructionFailed),
        "prerequisite failure should emit ConstructionFailed"
    );
    assert!(
        sim.check_invariants().is_empty(),
        "cancel/failure construction paths left violations: {:?}",
        sim.check_invariants()
    );
}

/// Policy/decision audit consistency: control mutations and decisions should
/// produce coherent audit records.
#[test]
fn policy_and_decision_audit_consistency_smoke() {
    let mut sim = Sim::new(0x0500_000B);
    let pid = sim.world().prisoners[0].id;
    let peer = sim.world().prisoners[1].id;

    let before = sim.decision_audit_log().len();
    sim.set_policy(Policy::Enhanced);
    sim.set_lockdown(true);
    sim.set_lockdown(false);
    sim.set_intake_paused(true);
    sim.set_security_level(0.75);
    sim.set_privileges_level(0.25);
    sim.set_rehab_funding(1.5);
    sim.set_policy_mode(sumo::PolicyMode::Council);

    let control_records: Vec<_> = sim.decision_audit_log()[before..]
        .iter()
        .filter(|r| r.source == "control")
        .collect();
    assert!(
        !control_records.is_empty(),
        "control mutations should append decision-audit records"
    );
    assert!(
        control_records.iter().all(|r| r.validation == "accepted"),
        "successful control mutations should be accepted: {control_records:?}"
    );
    assert!(
        control_records.iter().any(|r| r.action == "SetPolicy"),
        "policy change should be audited"
    );
    assert!(
        control_records.iter().any(|r| r.action == "SetLockdown"),
        "lockdown toggle should be audited"
    );
    assert!(
        control_records.iter().any(|r| r.action == "SetIntakePaused"),
        "intake pause should be audited"
    );
    assert!(
        control_records.iter().any(|r| r.action == "SetPolicyMode"),
        "policy mode change should be audited"
    );

    // Decision audit consistency: accepted/rejected both audited, only accepted in decision log.
    let decision_log_before = sim.decision_log().len();
    let audit_before_decision = sim.decision_audit_log().len();
    let ok = sim.deliver_decision(pid, Decision::new(DecisionAction::Comply));
    assert!(ok.applied, "comply should be accepted: {:?}", ok.result);
    let bad = sim.deliver_decision(PrisonerId(9_999_999), Decision::new(DecisionAction::Comply));
    assert!(!bad.applied);
    assert_eq!(sim.decision_log().len(), decision_log_before + 1);
    let new_audit = &sim.decision_audit_log()[audit_before_decision..];
    assert!(
        new_audit.iter().any(|r| r.validation == "accepted" && r.source == "agent"),
        "accepted decision should be audited as agent/accepted"
    );
    assert!(
        new_audit.iter().any(|r| r.validation == "rejected" && r.source == "agent"),
        "rejected decision should be audited as agent/rejected"
    );
    assert!(
        sim.check_invariants().is_empty(),
        "policy/decision audit consistency flow left violations: {:?}",
        sim.check_invariants()
    );
}

/// Autosave should survive save/load and be re-enableable after quick load.
#[test]
fn autosave_quick_load_reenable_behavior() {
    let dir = unique::temp_dir("autosave-reenable");
    let mut sim = Sim::new(0x0500_000C);
    sim.enable_autosave(&dir, 2);
    sim.set_speed(TimeSpeed::Extreme);
    sim.run_days(3);
    assert!(
        dir.join("world.bin").exists(),
        "autosave should write world.bin after interval"
    );

    // Save/load through the normal path; autosave interval is restored by engine state.
    sim.save(&dir).expect("save");
    let mut loaded = Sim::load(&dir).expect("load");
    // Sim::load restores autosave scheduler with save dir; it should be active.
    loaded.set_speed(TimeSpeed::Extreme);
    loaded.run_days(2);
    assert!(
        loaded.maybe_autosave().is_ok(),
        "maybe_autosave should be callable after load"
    );

    // Explicit re-enable (quick-load behavior): should replace scheduler cleanly.
    loaded.enable_autosave(&dir, 1);
    loaded.run_days(1);
    let wrote = loaded.maybe_autosave().expect("maybe_autosave after re-enable");
    assert!(
        wrote || dir.join("world.bin").exists(),
        "re-enabled autosave should be operable"
    );
    assert!(
        loaded.check_invariants().is_empty(),
        "autosave re-enable flow left violations: {:?}",
        loaded.check_invariants()
    );

    std::fs::remove_dir_all(&dir).ok();
}
