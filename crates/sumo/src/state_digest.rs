//! State digest module for richer trace_hash fingerprinting (R6: invariant coverage).
//!
//! Computes a stable hash of critical simulation state that can be used to
//! detect divergence, verify determinism, and provide richer trace fingerprinting.

use crate::{EngineState, PolicyState, ParoleLedger};
use alc_agents::Decision;
use std::hash::{Hash, Hasher};

/// Compute a deterministic hash of the engine state for trace verification.
pub fn compute_state_digest(engine_state: &EngineState) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    
    // Parole ledger state
    hash_parole_ledger(&engine_state.parole, &mut hasher);
    
    // Population history
    for (time, pop) in &engine_state.population_history {
        time.secs.hash(&mut hasher);
        pop.hash(&mut hasher);
    }
    
    // Incidents (WorldEvent)
    for inc in &engine_state.incidents {
        inc.kind.hash(&mut hasher);
        inc.at.secs.hash(&mut hasher);
        inc.summary.hash(&mut hasher);
    }
    
    // Incident sidecar
    for inc in &engine_state.incident_sidecar {
        inc.id.hash(&mut hasher);
        inc.kind.hash(&mut hasher);
        inc.severity.to_bits().hash(&mut hasher);
        inc.cause.hash(&mut hasher);
        inc.consequence.hash(&mut hasher);
        inc.timestamp.secs.hash(&mut hasher);
        inc.location.hash(&mut hasher);
        inc.resolved.hash(&mut hasher);
        for pid in &inc.involved_prisoners {
            pid.raw().hash(&mut hasher);
        }
    }
    
    // Decision log - hash the Decision enum variants
    for dec in &engine_state.decision_log {
        hash_decision(dec, &mut hasher);
    }
    
    // Post-release tracking (sorted by key for a reproducible digest)
    let mut post_release: Vec<_> = engine_state.post_release.iter().collect();
    post_release.sort_by_key(|(pid, _)| pid.raw());
    for (pid, success) in post_release {
        pid.raw().hash(&mut hasher);
        success.hash(&mut hasher);
    }
    
    // Policy state
    hash_policy_state(&engine_state.policy_state, &mut hasher);
    
    // Policy mode
    engine_state.policy_mode.hash(&mut hasher);
    
    hasher.finish()
}

fn hash_decision(dec: &Decision, hasher: &mut impl Hasher) {
    match dec {
        Decision::MoveTo { target_facility } => {
            0u8.hash(hasher); // variant discriminant
            target_facility.hash(hasher);
        }
        Decision::Socialize { with } => {
            1u8.hash(hasher);
            with.hash(hasher);
        }
        Decision::Fight { target } => {
            2u8.hash(hasher);
            target.hash(hasher);
        }
        Decision::Trade { with, item } => {
            3u8.hash(hasher);
            with.hash(hasher);
            item.hash(hasher);
        }
        Decision::Comply => { 4u8.hash(hasher); }
        Decision::Refuse => { 5u8.hash(hasher); }
        Decision::EscapeAttempt => { 6u8.hash(hasher); }
        Decision::RequestMedical => { 7u8.hash(hasher); }
        Decision::JoinGang { gang } => {
            8u8.hash(hasher);
            gang.hash(hasher);
        }
        Decision::LeaveGang => { 9u8.hash(hasher); }
        Decision::LeadGang { gang } => {
            10u8.hash(hasher);
            gang.hash(hasher);
        }
        Decision::SpreadRumor { about, content } => {
            11u8.hash(hasher);
            about.hash(hasher);
            content.hash(hasher);
        }
        Decision::None => { 12u8.hash(hasher); }
    }
}

fn hash_parole_ledger(parole: &ParoleLedger, hasher: &mut impl Hasher) {
    // Each map is iterated in sorted key order: HashMap's iteration order is
    // randomized per process, which would make identical states hash differently.
    let mut status: Vec<_> = parole.status.iter().collect();
    status.sort_by_key(|(pid, _)| pid.raw());
    for (pid, s) in status {
        pid.raw().hash(hasher);
        s.hash(hasher);
    }

    let mut adjustments: Vec<_> = parole.adjustments.iter().collect();
    adjustments.sort_by_key(|(pid, _)| pid.raw());
    for (pid, adj) in adjustments {
        pid.raw().hash(hasher);
        adj.to_bits().hash(hasher);
    }

    let mut hearing_at: Vec<_> = parole.hearing_at.iter().collect();
    hearing_at.sort_by_key(|(pid, _)| pid.raw());
    for (pid, time) in hearing_at {
        pid.raw().hash(hasher);
        time.secs.hash(hasher);
    }

    parole.last_decision_at.map(|t| t.secs).hash(hasher);
}

fn hash_policy_state(policy_state: &PolicyState, hasher: &mut impl Hasher) {
    policy_state.policy.hash(hasher);
    policy_state.lockdown.hash(hasher);
    policy_state.security_level.hash(hasher);
    policy_state.last_changed.secs.hash(hasher);
}

/// Compute a state digest for the world (prisoners, guards, facilities, etc.)
pub fn compute_world_digest(world: &alc_world::World) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    
    // World time
    world.time.secs.hash(&mut hasher);
    world.days_simulated.hash(&mut hasher);
    world.seed.hash(&mut hasher);
    
    // Facilities
    for f in &world.facilities {
        f.id.0.hash(&mut hasher);
        f.kind.hash(&mut hasher);
        f.capacity.hash(&mut hasher);
    }
    
    // Prisoners (sorted by ID for determinism)
    let mut prisoners: Vec<_> = world.prisoners.iter().collect();
    prisoners.sort_by_key(|p| p.id.0);
    for p in prisoners {
        p.id.0.hash(&mut hasher);
        p.classification.hash(&mut hasher);
        p.needs.hunger.to_bits().hash(&mut hasher);
        p.needs.thirst.to_bits().hash(&mut hasher);
        p.needs.sleep.to_bits().hash(&mut hasher);
        p.gang_affiliation.hash(&mut hasher);
    }
    
    // Guards (sorted by ID for determinism)
    let mut guards: Vec<_> = world.guards.iter().collect();
    guards.sort_by_key(|g| g.id.0);
    for g in guards {
        g.id.0.hash(&mut hasher);
        g.post.hash(&mut hasher);
        g.morale.to_bits().hash(&mut hasher);
        g.fatigue.to_bits().hash(&mut hasher);
        g.stress.to_bits().hash(&mut hasher);
    }
    
    // Gangs
    for g in &world.gangs {
        g.id.hash(&mut hasher);
        g.leader.hash(&mut hasher);
        for m in &g.members {
            m.hash(&mut hasher);
        }
    }
    
    // Policies
    world.policies.lockdown.hash(&mut hasher);
    world.policies.security_level.to_bits().hash(&mut hasher);
    world.policies.privileges_level.to_bits().hash(&mut hasher);
    world.policies.rehab_funding_multiplier.to_bits().hash(&mut hasher);
    
    // Economy
    world.economy.cash.0.hash(&mut hasher);
    
    // Events (recent only to avoid unbounded growth)
    for e in world.events.recent() {
        e.kind.hash(&mut hasher);
        e.at.secs.hash(&mut hasher);
        e.summary.hash(&mut hasher);
    }
    
    // Social graph relationships. `SocialGraph::iter()` yields dyads in a
    // deterministic (sorted) key order; hash the key and every relationship
    // field so social divergence is reflected in the world digest.
    for ((a, b), rel) in world.social.iter() {
        a.hash(&mut hasher);
        b.hash(&mut hasher);
        rel.trust.to_bits().hash(&mut hasher);
        rel.friendship.to_bits().hash(&mut hasher);
        rel.fear.to_bits().hash(&mut hasher);
        rel.respect.to_bits().hash(&mut hasher);
        rel.loyalty.to_bits().hash(&mut hasher);
        rel.hostility.to_bits().hash(&mut hasher);
        rel.attraction.to_bits().hash(&mut hasher);
        rel.resentment.to_bits().hash(&mut hasher);
        rel.dependency.to_bits().hash(&mut hasher);
        rel.rivalry.to_bits().hash(&mut hasher);
        rel.debt.to_bits().hash(&mut hasher);
        rel.gratitude.to_bits().hash(&mut hasher);
        rel.familiarity.to_bits().hash(&mut hasher);
    }
    
    // Prisoner locations
    let mut locs: Vec<_> = world.prisoner_location.iter().collect();
    locs.sort_by_key(|(k, _)| k.0);
    for (pid, fid) in locs {
        pid.0.hash(&mut hasher);
        fid.0.hash(&mut hasher);
    }
    
    // Guard locations
    let mut guard_locs: Vec<_> = world.guard_location.iter().collect();
    guard_locs.sort_by_key(|(k, _)| k.0);
    for (gid, fid) in guard_locs {
        gid.0.hash(&mut hasher);
        fid.0.hash(&mut hasher)
    }
    
    hasher.finish()
}

/// Combined state digest for both engine and world state.
pub fn compute_full_state_digest(sim: &crate::Sim) -> u64 {
    let engine_digest = compute_state_digest(&sim.engine_state);
    let world_digest = compute_world_digest(&sim.world);
    
    // Combine using FNV-1a
    let mut h = engine_digest;
    h ^= world_digest;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    h
}