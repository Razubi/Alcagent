//! EngineState persistence roundtrip (audit findings 1-3) and state-digest
//! determinism tests (audit finding 4).
//!
//! These tests use only sumo's public API plus the public fields on `Sim::engine_state`.

use alc_agents::{Decision, DecisionAction};
use alc_core::{PrisonerId, SimTime};
use alc_events::{EventKind, WorldEvent};
use sumo::incidents::{
    CausalLink, Incident, IncidentDisposition, Responder, ResponseAction, ResponseActionKind,
};
use sumo::parole::ParoleStatus;
use sumo::state_digest::compute_full_state_digest;
use sumo::{PolicyMode, Sim};

mod unique {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub fn temp_dir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "sumo-engine-roundtrip-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
}

fn sidecar_incident(pid: PrisonerId) -> Incident {
    Incident {
        id: 7,
        kind: EventKind::Fight,
        cause: Some(3),
        consequence: Some(9),
        causal_chain: vec![CausalLink {
            source_id: 3,
            is_incident: true,
            description: "prior argument escalated".to_string(),
        }],
        timestamp: SimTime::from_secs(1234),
        location: "Yard A".to_string(),
        involved_prisoners: vec![pid],
        witnesses: vec![pid],
        severity: 0.5,
        disposition: IncidentDisposition::Investigated,
        response_actions: vec![ResponseAction {
            action: ResponseActionKind::GuardIntervention,
            responder: Responder::Guard(9),
            at: SimTime::from_secs(1300),
            description: "guards separated the prisoners".to_string(),
            success: true,
        }],
        resolved: true,
    }
}

#[test]
fn engine_state_roundtrip_preserves_all_subfields() {
    let dir = unique::temp_dir("all");
    let mut sim = Sim::new(0xC0FF_EE01);

    // Grab a real prisoner id from the simulation.
    let pid = sim
        .world()
        .prisoners
        .first()
        .expect("bootstrap prisoners")
        .id;

    // Populate every engine-state subfield through the public API.
    sim.parole_mut().status.insert(pid, ParoleStatus::Scheduled);
    sim.parole_mut().adjustments.insert(pid, 0.75);
    sim.parole_mut()
        .hearing_at
        .insert(pid, SimTime::from_secs(999));
    sim.parole_mut().last_decision_at = Some(SimTime::from_secs(888));

    sim.population_history_mut()
        .push((SimTime::from_secs(100), 24));
    sim.population_history_mut()
        .push((SimTime::from_secs(200), 25));

    sim.record_incident(WorldEvent::new(
        5,
        SimTime::from_secs(700),
        EventKind::Argument,
        "heated argument",
    ));
    sim.record_sidecar_incident(sidecar_incident(pid));

    sim.decision_log_mut()
        .push(Decision::new(DecisionAction::Comply));
    sim.decision_log_mut()
        .push(Decision::new(DecisionAction::Socialize { with: pid.raw() }));

    sim.set_post_release(PrisonerId(42), true);
    sim.set_post_release(PrisonerId(43), false);

    // Contraband resolution timestamps were engine-only before the v4 fix.
    sim.engine_state.resolved_contraband_cases.insert(77);
    sim.engine_state
        .resolved_contraband_at
        .insert(77, SimTime::from_secs(3600));

    sim.policy_state_mut().lockdown = true;
    sim.policy_state_mut().security_level = 3;
    sim.policy_state_mut().last_changed = SimTime::from_secs(550);
    sim.set_policy_mode(PolicyMode::Shadow);

    sim.save(&dir).expect("save");
    let loaded = Sim::load(&dir).expect("load");

    // Parole ledger.
    assert_eq!(
        loaded.parole().status.get(&pid),
        Some(&ParoleStatus::Scheduled)
    );
    assert_eq!(loaded.parole().adjustments.get(&pid), Some(&0.75));
    assert_eq!(
        loaded.parole().hearing_at.get(&pid),
        Some(&SimTime::from_secs(999))
    );
    assert_eq!(
        loaded.parole().last_decision_at,
        Some(SimTime::from_secs(888))
    );

    // Population history.
    assert_eq!(
        loaded.population_history().as_slice(),
        sim.population_history().as_slice()
    );

    // World events.
    assert_eq!(loaded.incidents().len(), 1);
    assert_eq!(loaded.incidents()[0].summary, "heated argument");

    // Sidecar incidents: the critical lossy fields.
    assert_eq!(loaded.incident_sidecar().len(), 1);
    let inc = &loaded.incident_sidecar()[0];
    let expected = sidecar_incident(pid);
    assert_eq!(inc.id, 7, "incident id must round-trip");
    assert_eq!(
        inc.severity, 0.5,
        "severity 0.5 must round-trip without u8 quantization"
    );
    assert_eq!(inc.location, "Yard A", "location must round-trip");
    assert!(inc.resolved, "resolved flag must round-trip");
    assert_eq!(inc.cause, Some(3), "cause must round-trip");
    assert_eq!(inc.consequence, Some(9), "consequence must round-trip");
    assert_eq!(inc.kind, expected.kind);
    assert_eq!(inc.timestamp, expected.timestamp);
    assert_eq!(inc.involved_prisoners, expected.involved_prisoners);
    // Structured fields that used to be engine-only (P1 persistence fix).
    assert_eq!(
        inc.causal_chain, expected.causal_chain,
        "causal chain must round-trip"
    );
    assert_eq!(
        inc.witnesses, expected.witnesses,
        "witnesses must round-trip"
    );
    assert_eq!(
        inc.disposition, expected.disposition,
        "disposition must round-trip"
    );
    assert_eq!(
        inc.response_actions, expected.response_actions,
        "response actions must round-trip"
    );

    // Decision log.
    assert_eq!(loaded.decision_log().len(), 2);
    assert!(matches!(
        loaded.decision_log()[0].action,
        DecisionAction::Comply
    ));

    // Post-release.
    assert_eq!(loaded.post_release().get(&PrisonerId(42)), Some(&true));
    assert_eq!(loaded.post_release().get(&PrisonerId(43)), Some(&false));

    // Contraband resolutions and their timestamps (v4 persistence fix).
    assert!(loaded.engine_state.resolved_contraband_cases.contains(&77));
    assert_eq!(
        loaded.engine_state.resolved_contraband_at.get(&77),
        Some(&SimTime::from_secs(3600))
    );

    // Policy + mode.
    assert!(loaded.policy_state().lockdown);
    assert_eq!(loaded.policy_state().security_level, 3);
    assert_eq!(loaded.policy_state().last_changed, SimTime::from_secs(550));
    assert!(matches!(loaded.policy_mode(), PolicyMode::Shadow));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn state_digest_is_stable_across_identical_runs_and_changes_with_state() {
    let mut a = Sim::new(0xBEEF_1234);
    let mut b = Sim::new(0xBEEF_1234);
    for _ in 0..5 {
        a.tick();
        b.tick();
    }
    assert_eq!(
        compute_full_state_digest(&a),
        compute_full_state_digest(&b),
        "identical seeded runs must produce identical state digests"
    );

    // Mutating post-release in one must change the digest.
    let pid = a.world().prisoners.first().expect("bootstrap prisoners").id;
    a.set_post_release(pid, true);
    assert_ne!(
        compute_full_state_digest(&a),
        compute_full_state_digest(&b),
        "post-release change must alter the digest"
    );

    // Mutating policy mode in the other must also change the digest.
    let before = compute_full_state_digest(&b);
    b.set_policy_mode(PolicyMode::Council);
    assert_ne!(
        compute_full_state_digest(&b),
        before,
        "policy mode must alter the digest"
    );
}

#[test]
fn trace_hash_v2_changes_with_engine_state() {
    let mut a = Sim::new(0xABCD_0001);
    let b = Sim::new(0xABCD_0001);
    assert_eq!(a.trace_hash_v2(), b.trace_hash_v2());

    let pid = a.world().prisoners.first().expect("bootstrap prisoners").id;
    a.set_post_release(pid, true);
    assert_ne!(
        a.trace_hash_v2(),
        b.trace_hash_v2(),
        "trace_hash_v2 must react to engine-state changes"
    );
}
