//! State digest module for richer trace_hash fingerprinting (R6: invariant coverage).
//!
//! Computes a stable hash of critical simulation state that can be used to
//! detect divergence, verify determinism, and provide richer trace fingerprinting.
//!
//! # Stability note
//!
//! This module deliberately uses a locally implemented FNV-1a hasher instead of
//! Rust's standard-library hasher, which is explicitly *not* portable across
//! Rust versions or platforms. All integer writes are
//! normalized to big-endian byte order and all map/collection iterations are
//! performed in sorted (deterministic) key order, so identical logical states
//! always produce identical digests.
//!
//! Because the digest values feed [`crate::Sim::trace_hash_v2`], switching to
//! this stable hasher changes the concrete `u64` values those functions return
//! compared to earlier builds that used the standard-library hasher. This is intentional:
//! historical `trace_hash_v2` values are not part of the persistent save format
//! and are only meaningful for comparing runs made with the same build.

use crate::{EngineState, PolicyState, ParoleLedger};
use alc_agents::{Decision, DecisionAction};
use std::hash::{Hash, Hasher};

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// A deterministic, platform-independent FNV-1a hasher.
///
/// Integer writes are normalized to big-endian bytes before being mixed in,
/// so the resulting digest does not depend on the host's endianness or on the
/// (unspecified) `Hash` implementations of primitive types. `str` values are
/// hashed as their raw UTF-8 bytes followed by the conventional `0xff`
/// terminator emitted by `str::hash`, which is stable across toolchains.
#[derive(Default)]
struct StableHasher {
    state: u64,
}

impl StableHasher {
    fn new() -> Self {
        Self { state: FNV_OFFSET_BASIS }
    }

    /// Mix a single byte into the running FNV-1a state.
    #[inline]
    fn write_byte(&mut self, byte: u8) {
        self.state ^= u64::from(byte);
        self.state = self.state.wrapping_mul(FNV_PRIME);
    }

    /// Mix a sequence of bytes into the running FNV-1a state.
    #[inline]
    fn write_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_byte(byte);
        }
    }
}

impl Hasher for StableHasher {
    fn finish(&self) -> u64 {
        self.state
    }

    /// Raw byte writes (used by derived `Hash` for structs with `&[u8]` etc.).
    /// We do not add a length prefix here because callers hash fixed-layout
    /// values; variable-length strings use `Hasher::write_str`.
    fn write(&mut self, bytes: &[u8]) {
        self.write_bytes(bytes);
    }

    // Normalize every integer write to big-endian bytes so the digest is
    // independent of the machine's native byte order.

    fn write_u8(&mut self, i: u8) {
        self.write_byte(i);
    }

    fn write_u16(&mut self, i: u16) {
        self.write_bytes(&i.to_be_bytes());
    }

    fn write_u32(&mut self, i: u32) {
        self.write_bytes(&i.to_be_bytes());
    }

    fn write_u64(&mut self, i: u64) {
        self.write_bytes(&i.to_be_bytes());
    }

    fn write_u128(&mut self, i: u128) {
        self.write_bytes(&i.to_be_bytes());
    }

    fn write_usize(&mut self, i: usize) {
        self.write_bytes(&i.to_be_bytes());
    }

    fn write_i8(&mut self, i: i8) {
        self.write_byte(i as u8);
    }

    fn write_i16(&mut self, i: i16) {
        self.write_bytes(&i.to_be_bytes());
    }

    fn write_i32(&mut self, i: i32) {
        self.write_bytes(&i.to_be_bytes());
    }

    fn write_i64(&mut self, i: i64) {
        self.write_bytes(&i.to_be_bytes());
    }

    fn write_i128(&mut self, i: i128) {
        self.write_bytes(&i.to_be_bytes());
    }

    fn write_isize(&mut self, i: isize) {
        self.write_bytes(&i.to_be_bytes());
    }

}

/// Compute a deterministic hash of the engine state for trace verification.
pub fn compute_state_digest(engine_state: &EngineState) -> u64 {
    let mut hasher = StableHasher::new();

    // Parole ledger state
    hash_parole_ledger(&engine_state.parole, &mut hasher);

    // Population history (Vec, already deterministic order)
    for (time, pop) in &engine_state.population_history {
        time.secs.hash(&mut hasher);
        pop.hash(&mut hasher);
    }

    // Incidents (WorldEvent; Vec, already deterministic order)
    for inc in &engine_state.incidents {
        inc.kind.hash(&mut hasher);
        inc.at.secs.hash(&mut hasher);
        inc.summary.hash(&mut hasher);
    }

    // Incident sidecar (Vec, already deterministic order)
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

    // Decision audit (Vec, append order is meaningful)
    for rec in &engine_state.decision_audit {
        rec.time.secs.hash(&mut hasher);
        rec.actor.hash(&mut hasher);
        rec.source.hash(&mut hasher);
        rec.action.hash(&mut hasher);
        rec.summary.hash(&mut hasher);
        rec.validation.hash(&mut hasher);
        rec.reason.hash(&mut hasher);
        rec.rationale.hash(&mut hasher);
        rec.repair_reason.hash(&mut hasher);
        format!("{:?}", rec.outcome).hash(&mut hasher);
    }

    // Resolved contraband maps (deterministic ordering)
    for case_id in &engine_state.resolved_contraband_cases {
        case_id.hash(&mut hasher);
    }
    for (case_id, at) in &engine_state.resolved_contraband_at {
        case_id.hash(&mut hasher);
        at.secs.hash(&mut hasher);
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
    match &dec.action {
        DecisionAction::MoveTo { target_facility } => {
            0u8.hash(hasher); // variant discriminant
            target_facility.hash(hasher);
        }
        DecisionAction::Socialize { with } => {
            1u8.hash(hasher);
            with.hash(hasher);
        }
        DecisionAction::Fight { target } => {
            2u8.hash(hasher);
            target.hash(hasher);
        }
        DecisionAction::Trade { with, item } => {
            3u8.hash(hasher);
            with.hash(hasher);
            item.hash(hasher);
        }
        DecisionAction::Comply => { 4u8.hash(hasher); }
        DecisionAction::Refuse => { 5u8.hash(hasher); }
        DecisionAction::EscapeAttempt => { 6u8.hash(hasher); }
        DecisionAction::RequestMedical => { 7u8.hash(hasher); }
        DecisionAction::JoinGang { gang } => {
            8u8.hash(hasher);
            gang.hash(hasher);
        }
        DecisionAction::LeaveGang => { 9u8.hash(hasher); }
        DecisionAction::LeadGang { gang } => {
            10u8.hash(hasher);
            gang.hash(hasher);
        }
        DecisionAction::SpreadRumor { about, content } => {
            11u8.hash(hasher);
            about.hash(hasher);
            content.hash(hasher);
        }
        DecisionAction::None => { 12u8.hash(hasher); }
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
    let mut hasher = StableHasher::new();

    // World time
    world.time.secs.hash(&mut hasher);
    world.days_simulated.hash(&mut hasher);
    world.seed.hash(&mut hasher);

    // Facilities (Vec, already deterministic order)
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

    // Gangs (Vec, already deterministic order)
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
        fid.0.hash(&mut hasher);
    }

    // Construction queue (order is meaningful)
    for p in &world.construction_queue {
        p.id.hash(&mut hasher);
        format!("{:?}", p.kind).hash(&mut hasher);
        p.wing.hash(&mut hasher);
        p.capacity.hash(&mut hasher);
        p.total_days.hash(&mut hasher);
        p.progress_days.hash(&mut hasher);
        p.cost.to_bits().hash(&mut hasher);
        p.started_day.hash(&mut hasher);
        format!("{:?}", p.status).hash(&mut hasher);
        p.terminal_reason.hash(&mut hasher);
        p.staff_allocated.hash(&mut hasher);
        p.materials_ready.hash(&mut hasher);
        p.prereqs_met.hash(&mut hasher);
    }

    // Contraband cases (deterministic by case id)
    let mut contraband_cases: Vec<_> = world.contraband_cases.iter().collect();
    contraband_cases.sort_by_key(|c| c.case_id);
    for c in contraband_cases {
        c.case_id.hash(&mut hasher);
        c.discovered_at.secs.hash(&mut hasher);
        c.discovering_guard.0.hash(&mut hasher);
        c.holder.0.hash(&mut hasher);
        format!("{:?}", c.item).hash(&mut hasher);
        c.severity.to_bits().hash(&mut hasher);
        format!("{:?}", c.disposition).hash(&mut hasher);
        format!("{:?}", c.disciplinary_action).hash(&mut hasher);
        c.summary.hash(&mut hasher);
    }

    // Contraband state (sorted by prisoner id)
    let mut contraband_state: Vec<_> = world.contraband_state.iter().collect();
    contraband_state.sort_by_key(|(pid, _)| pid.0);
    for (pid, st) in contraband_state {
        pid.0.hash(&mut hasher);
        st.flags_confiscated.hash(&mut hasher);
        st.flagged_days.hash(&mut hasher);
        st.discovered_count.hash(&mut hasher);
        let mut inv: Vec<_> = st.inventory.iter().collect();
        inv.sort_by_key(|(k, _)| format!("{:?}", k));
        for (item, count) in inv {
            format!("{:?}", item).hash(&mut hasher);
            count.hash(&mut hasher);
        }
    }

    // World-side contraband resolution timestamps (BTreeMap iterates in sorted
    // case-id order). The engine keeps its own mirror, but this map is the
    // world-authoritative record and is persisted independently, so divergence
    // here must be visible in the digest too.
    for (case_id, at) in &world.resolved_contraband_at {
        case_id.hash(&mut hasher);
        at.secs.hash(&mut hasher);
    }

    // Rehab records (sorted by prisoner id; record order preserved)
    let mut rehab: Vec<_> = world.rehab_records.iter().collect();
    rehab.sort_by_key(|(pid, _)| pid.0);
    for (pid, records) in rehab {
        pid.0.hash(&mut hasher);
        for r in records {
            format!("{:?}", r.program).hash(&mut hasher);
            r.enrolled_day.hash(&mut hasher);
            r.progress_days.hash(&mut hasher);
            r.progress_sum.to_bits().hash(&mut hasher);
            r.completed.hash(&mut hasher);
            r.dropped.hash(&mut hasher);
            r.attendance_days.hash(&mut hasher);
            r.last_attended_day.hash(&mut hasher);
            format!("{:?}", r.failure_reason).hash(&mut hasher);
        }
    }

    // Post-release records/outcomes (deterministic by id)
    let mut post_release_records: Vec<_> = world.post_release_records.iter().collect();
    post_release_records.sort_by_key(|r| r.id);
    for r in post_release_records {
        r.id.hash(&mut hasher);
        r.prisoner_id.hash(&mut hasher);
        r.name.hash(&mut hasher);
        r.reason.hash(&mut hasher);
        r.day.hash(&mut hasher);
        r.sentence_info.sentence_days.hash(&mut hasher);
        r.sentence_info.sentence_served_days.hash(&mut hasher);
        format!("{:?}", r.sentence_info.classification).hash(&mut hasher);
        r.sentence_info.escape_motivation_at_exit.to_bits().hash(&mut hasher);
        r.outcome_seed.hash(&mut hasher);
        r.employment.hash(&mut hasher);
        r.housing.hash(&mut hasher);
        r.violation.hash(&mut hasher);
        r.return_to_custody.hash(&mut hasher);
    }
    let mut post_release_outcomes: Vec<_> = world.post_release_outcomes.iter().collect();
    post_release_outcomes.sort_by_key(|r| r.id);
    for r in post_release_outcomes {
        r.id.hash(&mut hasher);
        r.post_release_id.hash(&mut hasher);
        r.prisoner_id.hash(&mut hasher);
        r.name.hash(&mut hasher);
        r.day.hash(&mut hasher);
        r.outcome_seed.hash(&mut hasher);
        r.employment.hash(&mut hasher);
        r.housing.hash(&mut hasher);
        r.violation.hash(&mut hasher);
        r.return_to_custody.hash(&mut hasher);
    }

    hasher.finish()
}

/// Combined state digest for both engine and world state.
pub fn compute_full_state_digest(sim: &crate::Sim) -> u64 {
    let engine_digest = compute_state_digest(&sim.engine_state);
    let world_digest = compute_world_digest(&sim.world);

    // Combine deterministically with FNV-1a multiplication.
    let mut h = engine_digest;
    h ^= world_digest;
    h = h.wrapping_mul(FNV_PRIME);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_hasher_matches_fnv1a_known_vector() {
        // FNV-1a 64-bit of the empty byte string is the offset basis.
        let h = StableHasher::new();
        assert_eq!(h.finish(), FNV_OFFSET_BASIS);

        // FNV-1a 64-bit of "a" is 0xaf63dc4c8601ec8c.
        let mut h = StableHasher::new();
        h.write_byte(b'a');
        assert_eq!(h.finish(), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn stable_hasher_integer_writes_are_endian_independent() {
        // The big-endian byte sequence for 0x0102_0304_0506_0708.
        let mut a = StableHasher::new();
        a.write_u64(0x0102_0304_0506_0708);

        let mut b = StableHasher::new();
        b.write_bytes(&0x0102_0304_0506_0708u64.to_be_bytes());

        assert_eq!(a.finish(), b.finish());
    }
}
