//! S6 Invariants: checking for prison simulation state consistency.
//! 
//! These invariants validate that the prison world state remains consistent
//! across ticks, saves, and decision pipelines. Each function returns a list
//! of violation strings; an empty list means all invariants hold.

use alc_agents::DecisionAction;
use alc_core::FacilityId;
use alc_events::EventKind;
use alc_world::World;
use crate::{EngineState, Sim, Policy};
use std::collections::HashSet;

/// Maximum number of entries permitted in the incident/decision logs.
/// Kept as a named constant so both the invariant and its negative tests
/// refer to the same bound.
pub const MAX_LOG_ENTRIES: usize = 10_000;

/// Invariant 1: Every prisoner and guard must be in exactly one location.
pub fn invariant_pawn_in_exactly_one_place(world: &World) -> bool {
    let mut violations = Vec::new();
    let w = world;

    // Check prisoners: each must have exactly one location
    let mut prisoner_locations = HashSet::new();
    for prisoner in &w.prisoners {
        let loc = w.prisoner_location.get(&prisoner.id);
        match loc {
            Some(&facility_id) => {
                if !prisoner_locations.insert((prisoner.id, facility_id)) {
                    violations.push(format!(
                        "duplicate prisoner location mapping for prisoner {} at facility {}",
                        prisoner.id.raw(),
                        facility_id.0
                    ));
                }
                if !w.facilities.iter().any(|f| f.id == facility_id) {
                    violations.push(format!(
                        "prisoner {} located at non-existent facility {}",
                        prisoner.id.raw(),
                        facility_id.0
                    ));
                }
            }
            None => {
                violations.push(format!(
                    "prisoner {} has no assigned location",
                    prisoner.id.raw()
                ));
            }
        }
    }

    // Every key in prisoner_location must correspond to a current prisoner.
    for pid in w.prisoner_location.keys() {
        if !w.prisoners.iter().any(|p| p.id == *pid) {
            violations.push(format!(
                "stale prisoner_location entry for non-existent prisoner {}",
                pid.raw()
            ));
        }
    }

    // Check guards: every guard must have a location entry, and every entry
    // must refer to a current guard and a real facility. No stale ids.
    let guard_ids: HashSet<_> = w.guards.iter().map(|g| g.id).collect();
    for guard in &w.guards {
        match w.guard_location.get(&guard.id) {
            Some(&FacilityId(fid)) => {
                if !w.facilities.iter().any(|f| f.id.0 == fid) {
                    violations.push(format!(
                        "guard {} located at non-existent facility {}",
                        guard.id.raw(),
                        fid
                    ));
                }
            }
            None => {
                violations.push(format!(
                    "guard {} has no assigned location",
                    guard.id.raw()
                ));
            }
        }
    }
    for gid in w.guard_location.keys() {
        if !guard_ids.contains(gid) {
            violations.push(format!(
                "stale guard_location entry for non-existent guard {}",
                gid.raw()
            ));
        }
    }

    violations.is_empty()
}

/// Invariant 2: Every prisoner's intention target must exist and be legal.
pub fn invariant_intention_target_exists_and_is_legal(world: &World) -> bool {
    let mut violations = Vec::new();
    let w = world;

    for prisoner in &w.prisoners {
        if let Some(decision) = w.current_decisions.get(&prisoner.id) {
            if let Some(target_id) = decision.entity_target() {
                let target_exists = w.prisoners.iter().any(|p| p.id.raw() == target_id);
                if !target_exists {
                    violations.push(format!(
                        "prisoner {} has decision targeting non-existent prisoner ID {}",
                        prisoner.id.raw(),
                        target_id
                    ));
                }
            }
            
            if let DecisionAction::MoveTo { target_facility } = &decision.action {
                let facility_exists = w.facilities.iter().any(|f| f.id.0 == *target_facility);
                if !facility_exists {
                    violations.push(format!(
                        "prisoner {} has MoveTo decision targeting non-existent facility {}",
                        prisoner.id.raw(),
                        target_facility
                    ));
                }
            }
            
            if let DecisionAction::JoinGang { gang } | DecisionAction::LeadGang { gang } = &decision.action {
                let gang_exists = w.gangs.iter().any(|g| g.id.0 == *gang);
                if !gang_exists {
                    violations.push(format!(
                        "prisoner {} has gang decision referencing non-existent gang {}",
                        prisoner.id.raw(),
                        gang
                    ));
                }
            }
        }
    }

    violations.is_empty()
}

/// Invariant 3: Every incident must have an internally consistent causal record.
///
/// The old rule ("every conflict incident must have a cause and a consequence")
/// forced producers to write self-referential placeholders (`cause ==
/// consequence == own id`), which hid the absence of real causality. This
/// invariant now checks only *honest* structure:
///
/// * every sidecar id is unique;
/// * any non-`None` `cause`/`consequence` resolves to an existing incident id;
/// * no incident points to itself as both cause and consequence (or, more
///   strictly, as either one);
/// * a conflict-kind incident is not fully orphaned — it must carry at least
///   one piece of causal context (a resolvable cause, a consequence, or a
///   non-empty `causal_chain`). `cause`/`consequence` may be `None`; only the
///   complete absence of causal information is rejected.
///
/// Actor-id liveness is deliberately not checked: incidents are historical
/// records and routinely reference prisoners who have since left the facility.
pub fn invariant_every_incident_has_cause_and_consequence(sim: &Sim) -> bool {
    let mut violations = Vec::new();

    // (a) Every incident id must be unique.
    let mut ids: HashSet<u64> = HashSet::new();
    for incident in sim.incident_sidecar() {
        if !ids.insert(incident.id) {
            violations.push(format!(
                "duplicate incident id {} in incident_sidecar",
                incident.id
            ));
        }
    }

    for incident in sim.incident_sidecar() {
        // (b) Any non-None cause/consequence id must resolve to an existing
        // incident id in the sidecar. `None` is always legitimate.
        if let Some(cause) = incident.cause {
            if !ids.contains(&cause) {
                violations.push(format!(
                    "incident {} references non-existent cause incident {}",
                    incident.id, cause
                ));
            }
        }
        if let Some(consequence) = incident.consequence {
            if !ids.contains(&consequence) {
                violations.push(format!(
                    "incident {} references non-existent consequence incident {}",
                    incident.id, consequence
                ));
            }
        }

        // (c) No self-referential causality. This catches the legacy
        // placeholder (cause == consequence == own id) and any stricter
        // one-sided self-loop.
        if incident.cause == Some(incident.id) || incident.consequence == Some(incident.id) {
            violations.push(format!(
                "incident {} has self-referential cause/consequence",
                incident.id
            ));
        }

        // (d) A conflict incident must have *some* causal context. A real
        // cause is NOT required: an incident with `cause: None` and
        // `consequence: None` is valid as long as it records a causal chain
        // (e.g. a provenance link to its originating world event). Fully
        // orphaned conflict records are rejected.
        if matches!(
            incident.kind,
            EventKind::Fight
                | EventKind::Argument
                | EventKind::Threat
                | EventKind::GangClash
                | EventKind::Theft
                | EventKind::Protests
        ) && incident.cause.is_none()
            && incident.consequence.is_none()
            && incident.causal_chain.is_empty()
        {
            violations.push(format!(
                "incident {} ({:?}) is orphaned: no cause, consequence, or causal chain",
                incident.id, incident.kind
            ));
        }

        // NOTE: actor-id liveness is intentionally NOT checked here. Incidents
        // are historical records; a fight involving a prisoner who has since
        // been released/transferred/dead remains a valid record. The bounded
        // sidecar spans months, so participants routinely leave the facility.
    }

    violations.is_empty()
}

/// Invariant 4: Suspicion or claims between prisoners must reference valid
/// prisoner IDs.
pub fn invariant_suspicion_claim_records_reference_existing_pawns(world: &World) -> bool {
    let mut violations = Vec::new();
    let w = world;
    let social = &w.social;

    let prisoner_ids: HashSet<u64> = w.prisoners.iter().map(|p| p.id.raw()).collect();

    for (dyad, relationship) in social.iter() {
        let (a, b) = dyad;

        // A relationship dyad referring to a released/deceased prisoner is a
        // violation: stale edges must be pruned when a prisoner leaves.
        if !prisoner_ids.contains(&a) {
            violations.push(format!(
                "social graph dyad ({}, {}) references non-existent prisoner {}",
                a, b, a
            ));
        }
        if !prisoner_ids.contains(&b) {
            violations.push(format!(
                "social graph dyad ({}, {}) references non-existent prisoner {}",
                a, b, b
            ));
        }
        
        let problematic_fields = [
            "trust", "friendship", "fear", "respect", "loyalty",
            "hostility", "attraction", "resentment", "dependency",
            "rivalry", "debt", "gratitude", "familiarity",
        ];
        
        for field in &problematic_fields {
            let val = match *field {
                "trust" => relationship.trust,
                "friendship" => relationship.friendship,
                "fear" => relationship.fear,
                "respect" => relationship.respect,
                "loyalty" => relationship.loyalty,
                "hostility" => relationship.hostility,
                "attraction" => relationship.attraction,
                "resentment" => relationship.resentment,
                "dependency" => relationship.dependency,
                "rivalry" => relationship.rivalry,
                "debt" => relationship.debt,
                "gratitude" => relationship.gratitude,
                "familiarity" => relationship.familiarity,
                _ => 0.0,
            };
            if !val.is_finite() {
                violations.push(format!(
                    "social graph relationship has non-finite {} value {:?} for dyad {:?}",
                    field, val, dyad
                ));
            }
        }
    }

    for gang in &w.gangs {
        for &member_id in &gang.members {
            let member_exists = w.prisoners.iter().any(|p| p.id == member_id);
            if !member_exists {
                violations.push(format!(
                    "gang {} has member {:?} who is not a current prisoner",
                    gang.id.0, member_id
                ));
            }
        }
    }

    violations.is_empty()
}

/// Invariant 5: EngineState.policy_state must match world.policies.
///
/// `set_policy` and `set_lockdown` keep the two structures synchronized, but
/// they use *different scales*: the engine stores security as a 0-5 integer
/// while the world stores a 0.0-1.0 float, and the two are not a simple
/// division (Normal maps engine 1 -> world 0.6, Enhanced 3 -> 0.8,
/// Lockdown 5 -> 1.0). We therefore derive the expected world security from
/// the engine's policy enum and allow a small tolerance for the incremental
/// `set_lockdown(true/false)` adjustments (+0.2 / -0.1 on the world side and
/// +1 on the engine side). Lockdown state must match exactly.
pub fn invariant_policy_state_matches_world_policies(
    engine_state: &EngineState,
    world: &World,
) -> bool {
    if engine_state.policy_state.lockdown != world.policies.lockdown {
        return false;
    }

    // Base world-security value for each policy level as established by
    // `Sim::set_policy`.
    let base_world_security = match engine_state.policy_state.policy {
        Policy::Normal => 0.6_f32,
        Policy::Enhanced => 0.8_f32,
        Policy::Lockdown => 1.0_f32,
    };
    // Base engine-security value (0-5 scale) for each policy level.
    let base_engine_security = match engine_state.policy_state.policy {
        Policy::Normal => 1_u32,
        Policy::Enhanced => 3_u32,
        Policy::Lockdown => 5_u32,
    };

    // A single `set_lockdown(true)` raises engine security by 1 and world
    // security by 0.2; `set_lockdown(false)` lowers world security by 0.1 but
    // leaves the engine value unchanged. Accept a one-step engine drift and a
    // 0.25 world drift to cover both directions.
    let engine_delta = engine_state
        .policy_state
        .security_level
        .abs_diff(base_engine_security);
    if engine_delta > 1 {
        return false;
    }
    if (world.policies.security_level - base_world_security).abs() > 0.25 {
        return false;
    }

    true
}

/// Invariant 6: incident_log and decision_log must not grow unboundedly.
///
/// This function is also the wiring point for the 0.05 audit/lifecycle
/// invariants (see [`invariant_audit_and_lifecycle_state_sane`]):
/// `Sim::check_invariants` calls this function, so keeping the aggregate check
/// inside it makes the new invariants observable without engine-side changes.
pub fn invariant_logs_and_incident_store_bounded(sim: &Sim) -> bool {
    let mut violations = Vec::new();

    let incident_count = sim.incident_log().len();
    if incident_count > MAX_LOG_ENTRIES {
        violations.push(format!(
            "incident_log has {} entries, exceeding maximum {}",
            incident_count, MAX_LOG_ENTRIES
        ));
    }
    let sidecar_count = sim.incident_sidecar().len();
    if sidecar_count > MAX_LOG_ENTRIES {
        violations.push(format!(
            "incident_sidecar has {} entries, exceeding maximum {}",
            sidecar_count, MAX_LOG_ENTRIES
        ));
    }

    let decision_count = sim.decision_log().len();
    if decision_count > MAX_LOG_ENTRIES {
        violations.push(format!(
            "decision_log has {} entries, exceeding maximum {}",
            decision_count, MAX_LOG_ENTRIES
        ));
    }

    // The decision audit log is a fourth bounded audit store (R2 / 0.05).
    let audit_count = sim.decision_audit_log().len();
    if audit_count > MAX_LOG_ENTRIES {
        violations.push(format!(
            "decision_audit has {} entries, exceeding maximum {}",
            audit_count, MAX_LOG_ENTRIES
        ));
    }

    if !invariant_audit_and_lifecycle_state_sane(sim) {
        violations.push(
            "0.05 audit/lifecycle invariant violated (incident sidecar consistency, \
             post-release ids, construction queue sanity)"
                .to_string(),
        );
    }

    violations.is_empty()
}

/// Aggregate gate for the 0.05 audit/lifecycle invariants.
///
/// Kept as a single function so a caller that wants a precise boolean can use
/// it directly, while [`invariant_logs_and_incident_store_bounded`] routes it
/// into `Sim::check_invariants`.
pub fn invariant_audit_and_lifecycle_state_sane(sim: &Sim) -> bool {
    invariant_incident_sidecar_consistent(sim)
        && invariant_post_release_ids_valid(sim)
        && invariant_construction_queue_sane(sim.world())
        && invariant_contraband_resolution_state_sane(sim)
}

/// Invariant 0.05: contraband resolution state must be consistent and bounded.
///
/// Resolved case IDs are engine-state only. Timestamps, when present, must
/// correspond to resolved cases and all maps must remain bounded.
pub fn invariant_contraband_resolution_state_sane(sim: &Sim) -> bool {
    let resolved = &sim.engine_state.resolved_contraband_cases;
    let at = &sim.engine_state.resolved_contraband_at;
    if resolved.len() > MAX_LOG_ENTRIES || at.len() > MAX_LOG_ENTRIES {
        return false;
    }
    // Every timestamped resolution must be marked resolved.
    for id in at.keys() {
        if !resolved.contains(id) {
            return false;
        }
    }
    true
}

/// Invariant 7: the construction queue must contain only sane pending work.
///
/// A queued project is work that has not completed yet: its progress must be
/// strictly below its total, its cost must be finite and non-negative, and
/// project ids must be unique. `World::construction_step` retires projects as
/// soon as `progress_days >= total_days`, so a completed project still in the
/// queue means that retirement failed.
pub fn invariant_construction_queue_sane(world: &World) -> bool {
    let mut violations = Vec::new();
    let mut ids: HashSet<u64> = HashSet::new();

    for project in &world.construction_queue {
        if !ids.insert(project.id) {
            violations.push(format!(
                "duplicate construction project id {} in queue",
                project.id
            ));
        }
        if project.total_days == 0 {
            violations.push(format!(
                "construction project {} has zero total_days",
                project.id
            ));
        }
        if project.progress_days > project.total_days {
            violations.push(format!(
                "construction project {} has progress_days {} > total_days {}",
                project.id, project.progress_days, project.total_days
            ));
        }
        let terminal = matches!(project.status, alc_world::ConstructionStatus::Completed | alc_world::ConstructionStatus::Cancelled | alc_world::ConstructionStatus::Failed);
        if !terminal && project.progress_days >= project.total_days {
            violations.push(format!(
                "non-terminal construction project {} has progress {} >= total {}",
                project.id, project.progress_days, project.total_days
            ));
        }
        if matches!(project.status, alc_world::ConstructionStatus::Completed) && project.progress_days < project.total_days {
            violations.push(format!(
                "completed construction project {} has progress {} < total {}",
                project.id, project.progress_days, project.total_days
            ));
        }
        if !project.cost.is_finite() || project.cost < 0.0 {
            violations.push(format!(
                "construction project {} has invalid cost {}",
                project.id, project.cost
            ));
        }
    }

    violations.is_empty()
}

/// Invariant 8: the structured decision audit log must stay bounded.
///
/// The engine trims this log when recording new entries; an over-long log
/// indicates a bypass of that path (for example a direct state mutation or a
/// loaded save from a build without the bound).
pub fn invariant_decision_audit_bounded(sim: &Sim) -> bool {
    sim.decision_audit_log().len() <= MAX_LOG_ENTRIES
}

/// Invariant 9: incident sidecar records must be internally consistent.
///
/// Structural causality (unique ids, resolvable cause/consequence links and
/// mandatory cause/consequence on conflict kinds) is covered by
/// [`invariant_every_incident_has_cause_and_consequence`]. This invariant adds
/// the 0.05 consistency rules: severity is finite and within 0.0..=1.0, and
/// every referenced prisoner id was actually allocated by the world. Released
/// prisoners keep their historical ids below `next_prisoner_id`, so this does
/// not invalidate genuine historical records.
pub fn invariant_incident_sidecar_consistent(sim: &Sim) -> bool {
    let mut violations = Vec::new();
    let next_pid = sim.world.next_prisoner_id;

    for incident in sim.incident_sidecar() {
        if !incident.severity.is_finite() || !(0.0..=1.0).contains(&incident.severity) {
            violations.push(format!(
                "incident {} has out-of-range severity {}",
                incident.id, incident.severity
            ));
        }
        if next_pid > 0 {
            for pid in &incident.involved_prisoners {
                let raw = pid.raw();
                if raw == 0 || raw >= next_pid {
                    violations.push(format!(
                        "incident {} references never-allocated prisoner id {}",
                        incident.id, raw
                    ));
                }
            }
        }
    }

    violations.is_empty()
}

/// Invariant 10: post-release records must reference valid prisoner ids.
///
/// Post-release records are allowed to be absent entirely (no requirement to
/// track every release), but any record that is present must point at an id
/// the world actually allocated. Historical ids of released prisoners remain
/// below `next_prisoner_id`, so this accepts genuine post-release history.
pub fn invariant_post_release_ids_valid(sim: &Sim) -> bool {
    let mut violations = Vec::new();
    let next_pid = sim.world.next_prisoner_id;

    for pid in sim.post_release().keys() {
        let raw = pid.raw();
        if raw == 0 {
            violations.push("post-release record references prisoner id 0".to_string());
        } else if next_pid > 0 && raw >= next_pid {
            violations.push(format!(
                "post-release record references never-allocated prisoner id {} (next id {})",
                raw, next_pid
            ));
        }
    }

    violations.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DecisionAuditRecord, Sim};
    use alc_core::PrisonerId;
    use alc_world::{ConstructionProject, FacilityKind};

    fn project(id: u64) -> ConstructionProject {
        ConstructionProject {
            id,
            kind: FacilityKind::CellBlock,
            wing: 1,
            capacity: 40,
            total_days: 10,
            progress_days: 3,
            cost: 2_500.0,
            started_day: 0,
            status: alc_world::ConstructionStatus::InProgress,
            terminal_reason: None,
            staff_allocated: true,
            materials_ready: true,
            prereqs_met: true,
        }
    }

    #[test]
    fn fresh_sim_passes_all_new_invariants() {
        let sim = Sim::new(0xC0FF_EE);
        assert!(invariant_construction_queue_sane(sim.world()));
        assert!(invariant_decision_audit_bounded(&sim));
        assert!(invariant_incident_sidecar_consistent(&sim));
        assert!(invariant_post_release_ids_valid(&sim));
        assert!(invariant_audit_and_lifecycle_state_sane(&sim));
        assert!(invariant_logs_and_incident_store_bounded(&sim));
    }

    #[test]
    fn valid_construction_project_is_accepted() {
        let mut sim = Sim::new(1);
        sim.world.construction_queue.push(project(7));
        assert!(invariant_construction_queue_sane(sim.world()));
    }

    #[test]
    fn construction_progress_over_total_is_detected() {
        let mut sim = Sim::new(2);
        let mut p = project(1);
        p.progress_days = p.total_days + 1;
        sim.world.construction_queue.push(p);
        assert!(!invariant_construction_queue_sane(sim.world()));
        assert!(!invariant_logs_and_incident_store_bounded(&sim));
    }

    #[test]
    fn lingering_completed_project_is_detected() {
        let mut sim = Sim::new(3);
        let mut p = project(1);
        p.progress_days = p.total_days;
        sim.world.construction_queue.push(p);
        assert!(!invariant_construction_queue_sane(sim.world()));
    }

    #[test]
    fn negative_or_non_finite_construction_cost_is_detected() {
        let mut sim = Sim::new(4);
        let mut p = project(1);
        p.cost = -1.0;
        sim.world.construction_queue.push(p);
        assert!(!invariant_construction_queue_sane(sim.world()));

        let mut sim2 = Sim::new(4);
        let mut p2 = project(1);
        p2.cost = f32::NAN;
        sim2.world.construction_queue.push(p2);
        assert!(!invariant_construction_queue_sane(sim2.world()));
    }

    #[test]
    fn duplicate_construction_ids_are_detected() {
        let mut sim = Sim::new(5);
        sim.world.construction_queue.push(project(9));
        sim.world.construction_queue.push(project(9));
        assert!(!invariant_construction_queue_sane(sim.world()));
    }

    #[test]
    fn decision_audit_bound_is_detected() {
        let mut sim = Sim::new(6);
        for _ in 0..=MAX_LOG_ENTRIES {
            sim.engine_state.decision_audit.push(DecisionAuditRecord::default());
        }
        assert!(!invariant_decision_audit_bounded(&sim));
        assert!(!invariant_logs_and_incident_store_bounded(&sim));
    }

    #[test]
    fn incident_sidecar_severity_and_prisoner_id_are_checked() {
        let mut sim = Sim::new(7);
        let mut bad = crate::incidents::Incident::default();
        bad.id = 1;
        bad.severity = f32::NAN;
        sim.engine_state.incident_sidecar.push(bad);
        assert!(!invariant_incident_sidecar_consistent(&sim));
        assert!(!invariant_logs_and_incident_store_bounded(&sim));

        let mut sim2 = Sim::new(7);
        let mut bad = crate::incidents::Incident::default();
        bad.id = 1;
        bad.severity = 0.5;
        bad.involved_prisoners.push(PrisonerId(u64::MAX));
        sim2.engine_state.incident_sidecar.push(bad);
        assert!(!invariant_incident_sidecar_consistent(&sim2));
    }

    #[test]
    fn post_release_records_must_reference_allocated_ids() {
        let mut sim = Sim::new(8);
        let next = sim.world.next_prisoner_id;
        sim.engine_state.post_release.insert(PrisonerId(next), false);
        assert!(!invariant_post_release_ids_valid(&sim));
        assert!(!invariant_logs_and_incident_store_bounded(&sim));

        // A currently allocated id is fine for a historical post-release record.
        sim.engine_state.post_release.remove(&PrisonerId(next));
        sim.engine_state.post_release.insert(PrisonerId(1), true);
        assert!(invariant_post_release_ids_valid(&sim));
    }
}
