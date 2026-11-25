//! Spec 006 acceptance coverage: validated policy/construction/parole controls.
//!
//! Focused smoke tests for the institutional control surfaces:
//!
//! * policy bands, lockdown, privileges, rehab funding and security changes
//!   are clamped, audited and recorded in world history,
//! * construction projects traverse request -> approved -> active ->
//!   complete/cancel/fail with events and a capacity effect,
//! * parole eligibility -> hearing -> decision -> conditions -> release is
//!   deterministic, validated and survives a save/load roundtrip,
//! * world-driven parole hearings mirror into the persisted engine ledger.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use alc_core::{PrisonerId, TimeSpeed};
use alc_events::EventKind;
use sumo::{ParoleConditions, ParoleStatus, Policy, Sim};

mod unique {
    use super::*;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub fn temp_dir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("sumo-alc006-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
}

/// Make one prisoner eligible by advancing their served-time record. This is
/// test setup only; all subsequent mutations go through validated controls.
fn make_parole_eligible(sim: &mut Sim) -> PrisonerId {
    let id = sim.world().prisoners[0].id;
    let total = sim.world().prisoners[0].sentence_days;
    if let Some(p) = sim.world_mut().prisoners.iter_mut().find(|p| p.id == id) {
        p.sentence_served_days = (total * 7 / 10).max(1);
        p.schedule.disciplinary_status = false;
    }
    id
}

#[test]
fn policy_controls_validate_clamp_and_record_history_and_audit() {
    let mut sim = Sim::new(0x0601_0001);
    let audit_before = sim.decision_audit_log().len();
    let history_before = sim.world().history.len();

    sim.set_policy(Policy::Enhanced);
    sim.set_lockdown(true);
    sim.set_lockdown(false);
    sim.set_security_level(1.5);
    sim.set_privileges_level(-0.4);
    sim.set_rehab_funding(99.0);
    sim.set_intake_paused(true);

    // Out-of-range inputs are clamped into the documented bands.
    assert_eq!(sim.policy_summary().security_level, 1.0);
    assert_eq!(sim.policy_summary().privileges_level, 0.0);
    assert_eq!(sim.policy_summary().rehab_funding_multiplier, 3.0);
    assert!(sim.policy_summary().intake_paused);

    let records = &sim.decision_audit_log()[audit_before..];
    assert!(records.iter().any(|r| r.action == "SetPolicy"));
    assert!(records.iter().any(|r| r.action == "SetLockdown"));
    assert!(records.iter().any(|r| r.action == "SetSecurityLevel"));
    assert!(records.iter().any(|r| r.action == "SetPrivilegesLevel"));
    assert!(records.iter().any(|r| r.action == "SetRehabFunding"));
    assert!(records.iter().any(|r| r.action == "SetIntakePaused"));
    assert!(
        records.iter().any(|r| r.validation == "repaired"),
        "clamped controls must be recorded as repaired"
    );

    // Institutional memory (world.history) carries the changes too, not just
    // the engine decision-audit store.
    let history = &sim.world().history[history_before..];
    assert!(
        history.iter().any(|e| e.kind == EventKind::PolicyChange),
        "policy controls must emit auditable PolicyChange events into world history"
    );

    // An observer-imposed lockdown anchors its expiry clock.
    sim.set_lockdown(true);
    assert_eq!(sim.world().lockdown_since_day, sim.world().days_simulated);

    assert!(
        sim.check_invariants().is_empty(),
        "policy controls left invariant violations: {:?}",
        sim.check_invariants()
    );
}

#[test]
fn construction_lifecycle_request_approved_active_complete_and_terminal_paths() {
    let mut sim = Sim::new(0x0601_0002);
    let capacity_before = sim.world().capacity();

    // request
    let id = sim.enqueue_construction(alc_world::FacilityKind::CellBlock, 1, 30);
    let project = sim.construction_queue().into_iter().find(|p| p.id == id).unwrap();
    assert_eq!(project.status, alc_world::ConstructionStatus::Queued);
    assert!(sim.events().recent().any(|e| e.kind == EventKind::Expansion));

    // approved
    assert!(sim.approve_construction(id, "observer validated prereqs"));
    let project = sim.construction_queue().into_iter().find(|p| p.id == id).unwrap();
    assert_eq!(project.status, alc_world::ConstructionStatus::Approved);
    assert!(sim.events().recent().any(|e| {
        e.kind == EventKind::Expansion && e.summary.contains("approved")
    }));
    assert!(!sim.approve_construction(id, "duplicate"), "approval is one-shot");

    // active
    assert!(sim.advance_construction(id, 1));
    let project = sim.construction_queue().into_iter().find(|p| p.id == id).unwrap();
    assert_eq!(project.status, alc_world::ConstructionStatus::InProgress);
    assert_eq!(project.progress_days, 1);

    // complete — capacity effect and completion event
    let total = project.total_days;
    assert!(sim.advance_construction(id, total - project.progress_days));
    let project = sim.construction_queue().into_iter().find(|p| p.id == id).unwrap();
    assert_eq!(project.status, alc_world::ConstructionStatus::Completed);
    assert!(sim.events().recent().any(|e| e.kind == EventKind::ConstructionComplete));
    assert!(
        sim.world().capacity() > capacity_before,
        "completing a housing project must increase bed capacity"
    );

    assert!(
        sim.decision_audit_log().iter().any(|r| {
            r.action == "AdvanceConstruction" && r.summary.contains("(completed)")
        }),
        "audit should recognize terminal completion"
    );

    // cancel and fail are terminal, validated and evented
    let cancelled = sim.enqueue_construction(alc_world::FacilityKind::Medical, 1, 10);
    assert!(sim.cancel_construction(cancelled, "observer cancelled"));
    assert_eq!(
        sim.construction_queue().into_iter().find(|p| p.id == cancelled).unwrap().status,
        alc_world::ConstructionStatus::Cancelled
    );
    let failed = sim.enqueue_construction(alc_world::FacilityKind::Education, 1, 10);
    assert!(sim.fail_construction(failed, "observer failed"));
    assert_eq!(
        sim.construction_queue().into_iter().find(|p| p.id == failed).unwrap().status,
        alc_world::ConstructionStatus::Failed
    );
    assert!(sim.events().recent().any(|e| e.kind == EventKind::ConstructionCancelled));
    assert!(sim.events().recent().any(|e| e.kind == EventKind::ConstructionFailed));
    let violations = sim.check_invariants();
    assert!(violations.is_empty(), "construction terminal paths left violations: {violations:?}");
}

#[test]
fn parole_lifecycle_is_deterministic_validated_and_persisted() {
    let mut sim = Sim::new(0x0601_0003);
    let id = make_parole_eligible(&mut sim);
    assert!(sim.parole_due().contains(&id));

    // hearing records a date
    assert!(sim.start_parole_hearing(id).is_some());
    assert_eq!(sim.parole_status(id), Some(ParoleStatus::Scheduled));
    assert!(sim.parole().hearing_at(id).is_some());
    assert!(!sim.start_parole_hearing(id).is_some(), "duplicate hearing rejected");
    assert!(sim.parole_due().is_empty(), "scheduled prisoner is no longer due");

    // decision + conditions
    assert!(sim.grant_parole(id).is_some());
    assert_eq!(sim.parole_status(id), Some(ParoleStatus::Granted));
    let conditions = sim.parole_conditions(id).cloned().expect("granted conditions");
    assert!(conditions.is_normalized());
    assert!(sim.parole_decision_reason(id).is_some());
    assert!(!sim.parole_due().contains(&id));

    // invalid inputs are repaired and recorded
    let mut raw = ParoleConditions {
        reporting_per_month: 99,
        supervision_months: 0,
        ..Default::default()
    };
    let audit_before = sim.decision_audit_log().len();
    assert!(sim.set_parole_conditions(id, raw.clone()));
    raw.normalize();
    assert_eq!(sim.parole_conditions(id), Some(&raw));
    assert!(sim.decision_audit_log()[audit_before..].iter().any(|r| r.validation == "repaired"));

    // release moves the prisoner out of custody and records the time
    assert!(sim.release_on_parole(id));
    assert!(!sim.world().prisoners.iter().any(|p| p.id == id));
    let released_at = sim.parole_released_at(id).expect("release time");
    assert_eq!(released_at, sim.world().time);
    assert!(sim.post_release_status(id).is_some(), "engine post-release ledger mirrors the world release");
    assert!(!sim.release_on_parole(id), "release is one-shot");

    // save/load preserves the full record and the state fingerprint
    let dir = unique::temp_dir("parole");
    sim.save(&dir).expect("save parole lifecycle");
    let loaded = Sim::load(&dir).expect("load parole lifecycle");
    assert_eq!(loaded.parole_status(id), Some(ParoleStatus::Granted));
    assert_eq!(loaded.parole_conditions(id), Some(&raw));
    assert_eq!(loaded.parole_decision_reason(id), sim.parole_decision_reason(id));
    assert_eq!(loaded.parole_released_at(id), Some(released_at));
    assert_eq!(loaded.parole().hearing_at(id), sim.parole().hearing_at(id));
    assert_eq!(loaded.trace_hash_v2(), sim.trace_hash_v2());
    std::fs::remove_dir_all(&dir).ok();

    // Two identical worlds derive identical conditions from identical inputs.
    let mut other = Sim::new(0x0601_0003);
    let other_id = make_parole_eligible(&mut other);
    assert!(other.grant_parole(other_id).is_some());
    assert_eq!(other.parole_conditions(other_id), Some(&conditions));
}

#[test]
fn rejected_parole_controls_are_audited_without_mutating() {
    let mut sim = Sim::new(0x0601_0004);
    let ineligible = sim.world().prisoners[0].id;
    let audit_before = sim.decision_audit_log().len();

    assert!(sim.start_parole_hearing(ineligible).is_none());
    assert!(sim.grant_parole(ineligible).is_none());
    assert!(sim.deny_parole(ineligible, "test").is_none());
    assert!(!sim.release_on_parole(ineligible));

    assert!(sim.parole_status(ineligible).is_none());
    let rejected: Vec<_> = sim.decision_audit_log()[audit_before..].iter()
        .filter(|r| r.validation == "rejected")
        .collect();
    assert_eq!(rejected.len(), 4, "every rejected control is audited: {rejected:?}");
    assert!(sim.check_invariants().is_empty());
}

#[test]
fn world_driven_hearings_mirror_into_the_persisted_ledger() {
    let mut sim = Sim::new(0x0601_0005);
    let id = make_parole_eligible(&mut sim);
    sim.set_speed(TimeSpeed::Extreme);
    sim.run_days(3);

    // The world runs its own deterministic daily hearing. Its outcome must be
    // visible in the engine ledger (and therefore in saves and the UI).
    assert!(
        sim.parole_status(id).is_some(),
        "world hearing should reconcile into the engine parole ledger"
    );
    assert!(sim.parole().hearing_at(id).is_some(), "hearing date persisted");
    let status = sim.parole_status(id).unwrap();
    if status == ParoleStatus::Granted {
        assert!(sim.parole_conditions(id).is_some(), "grants carry conditions");
        assert!(sim.parole_decision_reason(id).is_some(), "grants carry a reason");
    } else {
        assert!(sim.parole_decision_reason(id).is_some(), "denials carry a reason");
    }
    assert!(sim.check_invariants().is_empty());
}
