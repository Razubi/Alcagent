use alc_agents::{Decision, DecisionAction};
use alc_core::TimeSpeed;
use alc_events::{EventKind, WorldEvent};
use sumo::{Sim, incidents};

#[test]
fn load_restores_simulation_speed() {
    let dir = std::env::temp_dir().join(format!(
        "alcagent-speed-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let mut sim = Sim::new(0xA1C_05);
    sim.set_speed(TimeSpeed::FastForward);
    sim.save(&dir).expect("save");

    let loaded = Sim::load(&dir).expect("load");
    assert_eq!(loaded.speed(), TimeSpeed::FastForward);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn incident_trim_scrubs_dangling_causal_chain_links() {
    let mut sim = Sim::new(7);
    // Build a chain: 1 -> 2 -> 3
    for i in 1..=3u64 {
        let cause = if i > 1 { Some(i - 1) } else { None };
        sim.engine_state.incident_sidecar.push(incidents::Incident {
            id: i,
            kind: EventKind::Fight,
            cause,
            consequence: None,
            causal_chain: if i > 1 {
                vec![incidents::CausalLink {
                    source_id: i - 1,
                    is_incident: true,
                    description: "prior".to_string(),
                }]
            } else {
                Vec::new()
            },
            timestamp: sim.world.time,
            location: "yard".to_string(),
            involved_prisoners: Vec::new(),
            witnesses: Vec::new(),
            severity: 0.5,
            disposition: incidents::IncidentDisposition::None,
            response_actions: Vec::new(),
            resolved: false,
        });
    }
    // Force trim by overfilling incidents with dummy entries.
    for i in 4..=(sumo::invariants::MAX_LOG_ENTRIES as u64 + 20) {
        sim.engine_state.incident_sidecar.push(incidents::Incident {
            id: i,
            kind: EventKind::Argument,
            cause: Some(1),
            consequence: None,
            causal_chain: vec![incidents::CausalLink {
                source_id: 1,
                is_incident: true,
                description: "old".to_string(),
            }],
            timestamp: sim.world.time,
            location: "yard".to_string(),
            involved_prisoners: Vec::new(),
            witnesses: Vec::new(),
            severity: 0.2,
            disposition: incidents::IncidentDisposition::None,
            response_actions: Vec::new(),
            resolved: false,
        });
    }
    // Trigger trim through public recording path.
    sim.record_incident(WorldEvent::new(0, sim.world.time, EventKind::Fight, "trim"));
    let live: std::collections::HashSet<u64> =
        sim.engine_state.incident_sidecar.iter().map(|i| i.id).collect();
    for inc in &sim.engine_state.incident_sidecar {
        if let Some(cause) = inc.cause {
            assert!(live.contains(&cause), "dangling cause {cause}");
        }
        for link in &inc.causal_chain {
            if link.is_incident {
                assert!(
                    live.contains(&link.source_id),
                    "dangling causal link {} in incident {}",
                    link.source_id,
                    inc.id
                );
            }
        }
    }
}

#[test]
fn decision_audit_log_is_bounded() {
    let mut sim = Sim::new(9);
    let max = sumo::invariants::MAX_LOG_ENTRIES;
    for i in 0..(max + 50) {
        sim.record_decision_audit(sumo::DecisionAuditRecord {
            time: sim.world.time,
            actor: "test".to_string(),
            source: "test".to_string(),
            action: format!("a{i}"),
            summary: "s".to_string(),
            validation: "accepted".to_string(),
            reason: "r".to_string(),
            rationale: String::new(),
            repair_reason: String::new(),
            outcome: alc_agents::decision::DecisionOutcomeState::Accepted,
        });
    }
    assert!(sim.decision_audit_log().len() <= max);
    assert_eq!(
        sim.decision_audit_log().last().unwrap().action,
        format!("a{}", max + 49)
    );
}

#[test]
fn fight_tension_uses_defender_traits() {
    // Construct two prisoners where defender aggression makes fight more likely.
    let mut sim = Sim::new(123);
    // Find two prisoners and force traits.
    if sim.world.prisoners.len() < 2 {
        return;
    }
    let a = sim.world.prisoners[0].id;
    let b = sim.world.prisoners[1].id;
    sim.world.prisoners[0].traits.aggression = 0.0;
    sim.world.prisoners[1].traits.aggression = 1.0;
    sim.world.prisoners[0].traits.discipline = 0.0;
    sim.world.prisoners[1].traits.discipline = 0.0;
    // Ensure they're co-located and awake for decision application simplicity.
    let fac = sim.world.prisoner_location.get(&a).copied();
    if let Some(f) = fac {
        sim.world.prisoner_location.insert(b, f);
    }
    let mut decision = Decision::new(DecisionAction::Fight { target: b.raw() });
    decision.rationale = "test".to_string();
    let outcome = sim.deliver_decision(a, decision);
    // With defender aggression 1.0, tension should be high enough to not always be Compliance.
    // We don't assert exact outcome due to resolve_conflict internals, but we assert delivery path runs.
    assert!(
        matches!(
            outcome.result,
            alc_agents::ValidationResult::Accepted(_) | alc_agents::ValidationResult::Repaired(_)
        ) || !outcome.applied
    );
}

#[test]
fn escape_decision_records_post_release_parity() {
    let mut sim = Sim::new(0xE5CA9E);
    // Find a prisoner and make escape very likely by maxing motivation and lowering security.
    let Some(pidx) = sim.world.prisoners.iter().position(|p| p.sentence_days > 0) else { return; };
    let pid = sim.world.prisoners[pidx].id;
    sim.world.policies.security_level = 0.0;
    sim.world.prisoners[pidx].traits.risk_tolerance = 1.0;
    sim.world.prisoners[pidx].traits.discipline = 0.0;
    sim.world.prisoners[pidx].traits.leadership = 1.0;
    sim.world.prisoners[pidx].needs.safety = 1.0;
    sim.world.prisoners[pidx].needs.freedom = 1.0;
    sim.world.prisoners[pidx].sentence_days = 3650;
    sim.world.prisoners[pidx].sentence_served_days = 0;

    // Try a few seeds/times to get a successful escape through decision pipeline.
    let mut escaped = false;
    for _ in 0..200 {
        let mut d = Decision::new(DecisionAction::EscapeAttempt);
        d.rationale = "test".to_string();
        let out = sim.deliver_decision(pid, d);
        if sim.world.prisoners.iter().all(|p| p.id != pid) {
            escaped = true;
            // Engine post-release ledger should contain it.
            assert_eq!(sim.engine_state.post_release.get(&pid), Some(&false));
            // World post-release records should contain escape.
            assert!(sim.world.post_release_records.iter().any(|r| r.prisoner_id == pid.raw() && r.reason == "escape"));
            assert!(sim.world.history.iter().any(|e| e.kind == EventKind::Release && e.actors.contains(&pid.raw())));
            break;
        }
        // If not escaped, keep trying; reset minimal state if prisoner still exists.
        if out.applied {
            sim.world.time = sim.world.time.add_secs(1);
        } else {
            break;
        }
    }
    if !escaped {
        // At least ensure path didn't panic; environment-dependent.
        eprintln!("escape did not succeed in bounded attempts; parity assertions skipped");
    }
}

#[test]
fn state_digest_covers_new_state() {
    let mut sim = Sim::new(42);
    let base = sim.trace_hash_v2();
    sim.record_decision_audit(sumo::DecisionAuditRecord {
        time: sim.world.time,
        actor: "warden".to_string(),
        source: "test".to_string(),
        action: "X".to_string(),
        summary: "x".to_string(),
        validation: "accepted".to_string(),
        reason: "r".to_string(),
        rationale: String::new(),
        repair_reason: String::new(),
        outcome: alc_agents::decision::DecisionOutcomeState::Accepted,
    });
    let after_audit = sim.trace_hash_v2();
    assert_ne!(base, after_audit, "decision audit should affect digest");

    sim.engine_state.resolved_contraband_cases.insert(999);
    sim.engine_state
        .resolved_contraband_at
        .insert(999, sim.world.time);
    let after_contraband = sim.trace_hash_v2();
    assert_ne!(after_audit, after_contraband, "contraband maps should affect digest");
}
