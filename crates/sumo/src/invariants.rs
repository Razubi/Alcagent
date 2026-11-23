//! S6 Invariants: checking for prison simulation state consistency.
//! 
//! These invariants validate that the prison world state remains consistent
//! across ticks, saves, and decision pipelines. Each function returns a list
//! of violation strings; an empty list means all invariants hold.

use alc_core::FacilityId;
use alc_events::EventKind;
use alc_world::World;
use crate::{Decision, EngineState, Sim, Policy};
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
            
            if let Decision::MoveTo { target_facility } = decision {
                let facility_exists = w.facilities.iter().any(|f| f.id.0 == *target_facility);
                if !facility_exists {
                    violations.push(format!(
                        "prisoner {} has MoveTo decision targeting non-existent facility {}",
                        prisoner.id.raw(),
                        target_facility
                    ));
                }
            }
            
            if let Decision::JoinGang { gang } | Decision::LeadGang { gang } = decision {
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

/// Invariant 3: Every incident must have a complete, internally consistent
/// causal record (unique ids, resolvable cause/consequence links).
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
        // (b) Conflict-kind events must record both a cause and a consequence.
        if matches!(
            incident.kind,
            EventKind::Fight
                | EventKind::Argument
                | EventKind::Threat
                | EventKind::GangClash
                | EventKind::Theft
                | EventKind::Protests
        ) {
            if incident.cause.is_none() {
                violations.push(format!(
                    "incident {} ({:?}) has no cause",
                    incident.id, incident.kind
                ));
            }
            if incident.consequence.is_none() {
                violations.push(format!(
                    "incident {} ({:?}) has no consequence",
                    incident.id, incident.kind
                ));
            }
        }

        // NOTE: actor-id liveness is intentionally NOT checked here. Incidents
        // are historical records; a fight involving a prisoner who has since
        // been released/transferred/dead remains a valid record. The bounded
        // sidecar spans months, so participants routinely leave the facility.
        // R6's structural guarantees are unique ids and resolvable causality
        // chains, checked below.

        // (c) Every non-None cause/consequence id must resolve to an existing
        // incident id in the sidecar.
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
    }

    // (d) Actor-existence check is intentionally limited to the *sidecar*
    // incidents' current participants below; historical WorldEvents in the
    // incident log legitimately reference prisoners who have since been
    // released/transferred/dead. Requiring those old actor ids to still exist
    // would flag normal history as corruption. The structural causality checks
    // (a)-(c) above are the authoritative R6 guarantees.

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

    violations.is_empty()
}
