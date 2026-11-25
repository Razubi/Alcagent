//! Social layer per Directive §6–§8, §16.
//!
//! Complete implementation: relationships, gangs, conflict, escape modeling.

pub mod conflict;
pub mod contraband;
pub mod escape;
pub mod gang;
pub mod parole;
pub mod rehab;

// Inline relationships module per Phase 3 plan (preserve public API).
pub mod relationships {
    use alc_core::PrisonerId;
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    /// Directed per-dyad relationship edge (stored per pair in legacy-compatible graph API).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Relationship {
        pub trust: f32,
        pub friendship: f32,
        pub fear: f32,
        pub respect: f32,
        pub loyalty: f32,
        pub hostility: f32,
        pub attraction: f32,
        pub resentment: f32,
        pub dependency: f32,
        pub rivalry: f32,
        pub debt: f32, // >0 = A owes B
        pub gratitude: f32,
        pub familiarity: f32,
    }

    impl Default for Relationship {
        fn default() -> Self {
            Self {
                trust: 0.0,
                friendship: 0.0,
                fear: 0.0,
                respect: 0.0,
                loyalty: 0.0,
                hostility: 0.0,
                attraction: 0.0,
                resentment: 0.0,
                dependency: 0.0,
                rivalry: 0.0,
                debt: 0.0,
                gratitude: 0.0,
                familiarity: 0.0,
            }
        }
    }

    impl Relationship {
        pub fn adjust(&mut self, d: RelationshipDelta) {
            self.trust = (self.trust + d.trust).clamp(0.0, 1.0);
            self.friendship = (self.friendship + d.friendship).clamp(0.0, 1.0);
            self.fear = (self.fear + d.fear).clamp(0.0, 1.0);
            self.respect = (self.respect + d.respect).clamp(0.0, 1.0);
            self.loyalty = (self.loyalty + d.loyalty).clamp(0.0, 1.0);
            self.hostility = (self.hostility + d.hostility).clamp(0.0, 1.0);
            self.attraction = (self.attraction + d.attraction).clamp(0.0, 1.0);
            self.resentment = (self.resentment + d.resentment).clamp(0.0, 1.0);
            self.dependency = (self.dependency + d.dependency).clamp(0.0, 1.0);
            self.rivalry = (self.rivalry + d.rivalry).clamp(0.0, 1.0);
            self.debt = (self.debt + d.debt).max(0.0);
            self.gratitude = (self.gratitude + d.gratitude).clamp(0.0, 1.0);
            self.familiarity = (self.familiarity + d.familiarity).clamp(0.0, 1.0);
        }

        /// Aggregate bond score for heuristics (higher = more affiliative).
        pub fn bond_score(&self) -> f32 {
            self.friendship + self.trust + self.loyalty + self.gratitude
                - self.hostility
                - self.resentment
                - self.rivalry
        }

        /// Aggregate threat pressure for heuristics.
        pub fn threat_score(&self) -> f32 {
            self.fear + self.hostility + self.rivalry + self.resentment * 0.5
        }
    }

    // Re-exported from `alc-prisoner`: the relationship-delta type is owned by
    // the prisoner crate now (alc-social → alc-prisoner; see alc-social/Cargo.toml).
    pub use alc_prisoner::RelationshipDelta;

    /// Social memory event tags (Directive §6).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum SocialMemoryTag {
        Helped,
        Betrayed,
        Humiliated,
        Protected,
        StoleFrom,
        SharedResources,
        LiedTo,
        SupportedInCrisis,
    }

    /// Dyad-keyed relationship store.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SocialGraph {
        /// key = (min(a,b), max(a,b)) — legacy-compatible pair storage.
        edges: HashMap<(u64, u64), Relationship>,
    }

    impl Default for SocialGraph {
        fn default() -> Self {
            Self {
                edges: HashMap::new(),
            }
        }
    }

    impl SocialGraph {
        pub fn get(&self, a: PrisonerId, b: PrisonerId) -> &Relationship {
            static DEFAULT: std::sync::LazyLock<Relationship> =
                std::sync::LazyLock::new(Relationship::default);
            self.edges.get(&key(a, b)).unwrap_or(&DEFAULT)
        }
        pub fn get_mut(&mut self, a: PrisonerId, b: PrisonerId) -> &mut Relationship {
            self.edges.entry(key(a, b)).or_default()
        }
        pub fn record(&mut self, a: PrisonerId, b: PrisonerId, d: RelationshipDelta) {
            self.get_mut(a, b).adjust(d);
        }

        /// Remove every dyad that references a prisoner not in `live`.
        ///
        /// Called when prisoners are released/transferred/dead so stale social
        /// edges never survive in a persisted world (R6 invariant: suspicion
        /// and claim records must reference existing pawns).
        pub fn retain(&mut self, live: &std::collections::HashSet<PrisonerId>) {
            self.edges.retain(|&(a, b), _| {
                live.contains(&PrisonerId(a)) && live.contains(&PrisonerId(b))
            });
        }

        /// Iterate all stored dyads in deterministic key order (reproducible traces).
        pub fn iter(&self) -> impl Iterator<Item = ((u64, u64), &Relationship)> {
            let mut items: Vec<_> = self.edges.iter().map(|(k, v)| (*k, v)).collect();
            items.sort_by_key(|(k, _)| *k);
            items.into_iter()
        }

        /// All other prisoner ids that have a stored relationship edge with `id`.
        pub fn known_contacts(&self, id: PrisonerId) -> Vec<PrisonerId> {
            let mut out: Vec<PrisonerId> = self
                .edges
                .keys()
                .filter_map(|&(a, b)| {
                    if a == id.0 {
                        Some(PrisonerId(b))
                    } else if b == id.0 {
                        Some(PrisonerId(a))
                    } else {
                        None
                    }
                })
                .collect();
            out.sort_by_key(|p| p.0);
            out.dedup();
            out
        }
    }

    fn key(a: PrisonerId, b: PrisonerId) -> (u64, u64) {
        if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) }
    }
}

use alc_core::{GangId, PrisonerId};
use std::collections::HashMap;

// Backward-compatible public API re-exports.
pub use conflict::{
    ConflictEscalationStage, ConflictIncident, ConflictOutcome, ConflictOutcomeEffects,
    ConflictPrisonerProfile, DisciplineProposal, IncidentKind, InjuryRecord, ReputationDelta,
    apply_outcome, escalate_conflict, maybe_generate_conflict, resolve_conflict,
};
pub use contraband::{
    ContrabandFound, ContrabandItem, ContrabandPrisonerProfile, TradeOffer, contraband_discovery,
    contraband_market,
};
pub use escape::{
    EscapeConsequences, EscapeMotivation, EscapeMotivationInput, EscapeOutcome, EscapeVector,
    apply_escape_consequences, compute_escape_motivation, resolve_escape_attempt,
};
pub use gang::{
    Gang, GangFormationCandidate, GroupDirective, LeaderMemberResolution, MemberDecision,
    MemberDecisionContext, MemberRecord, PrisonerGangProfile, Rank, form_gang_if_drivers,
    maybe_form_gang, register_rivalry,
};
pub use parole::{
    ParoleBoard, ParoleCondition, ParoleDecision, ParoleSubject, PostReleaseOutcome,
    PostReleaseRecord, recidivism_score,
};
pub use rehab::{
    RehabOutcome, RehabPrisonerState, RehabProgram, apply_program_progress, enroll_in_program,
};
pub use relationships::{Relationship, RelationshipDelta, SocialGraph, SocialMemoryTag};

/// Emergent social role detection (Directive §6, no predefined categories).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SocialRole {
    Leader,
    Influencer,
    Broker,
    Fighter,
    Victim,
    Loner,
    InformantSuspect,
    // Appended for serde/bincode stability (Spec 012).
    Ally,
    Vulnerable,
}

// --- Dyad/social classifiers (Directive §6) --------------------------------

/// True if `a` views the relationship with `b` as friendly/affiliative.
pub fn is_friend(graph: &SocialGraph, a: PrisonerId, b: PrisonerId) -> bool {
    let r = graph.get(a, b);
    r.bond_score() >= 0.5 && r.friendship >= 0.3 && r.hostility < 0.35
}

/// True if `a` views `b` as a rival (competitive/resentful, not purely hostile threat).
pub fn is_rival(graph: &SocialGraph, a: PrisonerId, b: PrisonerId) -> bool {
    let r = graph.get(a, b);
    r.rivalry >= 0.3 && r.hostility < 0.75 && (r.resentment >= 0.15 || r.familiarity >= 0.1)
}

/// True if `a` views `b` as an enemy (active hostility/threat).
pub fn is_enemy(graph: &SocialGraph, a: PrisonerId, b: PrisonerId) -> bool {
    let r = graph.get(a, b);
    r.hostility >= 0.45 || (r.threat_score() >= 0.9 && r.friendship < 0.2)
}

/// Friends of `id` within the provided population.
pub fn friends_of(graph: &SocialGraph, id: PrisonerId, ids: &[PrisonerId]) -> Vec<PrisonerId> {
    let mut out: Vec<PrisonerId> = ids
        .iter()
        .copied()
        .filter(|&other| other != id && is_friend(graph, id, other))
        .collect();
    out.sort_by_key(|p| p.0);
    out
}

/// Rivals of `id` within the provided population.
pub fn rivals_of(graph: &SocialGraph, id: PrisonerId, ids: &[PrisonerId]) -> Vec<PrisonerId> {
    let mut out: Vec<PrisonerId> = ids
        .iter()
        .copied()
        .filter(|&other| other != id && is_rival(graph, id, other))
        .collect();
    out.sort_by_key(|p| p.0);
    out
}

/// Enemies of `id` within the provided population.
pub fn enemies_of(graph: &SocialGraph, id: PrisonerId, ids: &[PrisonerId]) -> Vec<PrisonerId> {
    let mut out: Vec<PrisonerId> = ids
        .iter()
        .copied()
        .filter(|&other| other != id && is_enemy(graph, id, other))
        .collect();
    out.sort_by_key(|p| p.0);
    out
}

/// Prisoners with few strong ties and low familiarity across the population.
pub fn isolated_prisoners(graph: &SocialGraph, ids: &[PrisonerId]) -> Vec<PrisonerId> {
    if ids.len() <= 1 {
        return ids.to_vec();
    }
    let mut out = Vec::new();
    for &id in ids {
        let mut strong_ties = 0usize;
        let mut fam_sum = 0.0f32;
        let mut others = 0usize;
        for &other in ids {
            if other == id {
                continue;
            }
            others += 1;
            let r = graph.get(id, other);
            fam_sum += r.familiarity;
            if r.bond_score() >= 0.5 || r.friendship >= 0.35 {
                strong_ties += 1;
            }
        }
        let avg_fam = if others == 0 {
            0.0
        } else {
            fam_sum / others as f32
        };
        if strong_ties == 0 && avg_fam < 0.2 {
            out.push(id);
        }
    }
    out.sort_by_key(|p| p.0);
    out
}

/// Influential prisoners based on emergent social roles (leaders/influencers).
pub fn influential_prisoners(
    graph: &SocialGraph,
    ids: &[PrisonerId],
    gangs: &[Gang],
) -> Vec<(PrisonerId, f32)> {
    let roles = detect_roles(graph, ids, gangs);
    let mut out: Vec<(PrisonerId, f32)> = roles
        .into_iter()
        .filter_map(|(id, rs)| {
            let mut score = 0.0f32;
            for (role, s) in rs {
                if matches!(role, SocialRole::Leader | SocialRole::Influencer) {
                    score = score.max(s);
                }
            }
            (score > 0.0).then_some((id, score))
        })
        .collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.0.cmp(&b.0.0))
    });
    out
}

/// Detect roles via graph heuristics over relationships + gang memberships.
/// Extended with Broker and InformantSuspect heuristics.
pub fn detect_roles(
    graph: &SocialGraph,
    ids: &[PrisonerId],
    gangs: &[Gang],
) -> HashMap<PrisonerId, Vec<(SocialRole, f32)>> {
    let mut out: HashMap<PrisonerId, Vec<(SocialRole, f32)>> = HashMap::new();
    if ids.is_empty() {
        return out;
    }
    let n = ids.len() as f32;

    // Precompute memberships and simple cross-gang linkage.
    let mut member_of: HashMap<PrisonerId, Vec<GangId>> = HashMap::new();
    for g in gangs {
        for &m in &g.members {
            member_of.entry(m).or_default().push(g.id);
        }
    }

    for &id in ids {
        let mut friends = 0.0f32;
        let mut feared = 0.0f32;
        let mut lonely_count = 0.0f32;
        let mut hostility_in = 0.0f32;
        let mut familiarity_sum = 0.0f32;
        let mut strong_ties = 0usize;
        let mut contact_spread = 0usize;
        let mut bridge_score = 0.0f32;
        let mut staff_proximity_proxy = 0.0f32; // suspicion heuristic from unusual low-hostility/high-fear asymmetry cues
        let mut in_group_trust_proxy = 0.0f32;
        let mut ally_score = 0.0f32;
        let mut protection_sum = 0.0f32;

        let my_gangs = member_of.get(&id).cloned().unwrap_or_default();

        for &other in ids {
            if other == id {
                continue;
            }
            let r = graph.get(id, other);
            friends += r.friendship + r.trust * 0.5 + r.loyalty * 0.3;
            feared += r.fear;
            hostility_in += r.hostility + r.resentment * 0.5;
            familiarity_sum += r.familiarity;
            if r.familiarity < 0.1 {
                lonely_count += 1.0;
            } else {
                contact_spread += 1;
            }
            if r.bond_score() > 0.8 {
                strong_ties += 1;
            }
            // No direct protection edge exists; loyalty/dependency are the
            // observable proxies for a protective tie in the dyad record.
            protection_sum += (r.loyalty * 0.6 + r.dependency * 0.4).clamp(0.0, 1.0);
            // Allies are derived from repeated affiliative exchanges, not from
            // a hard-coded roster: trust + friendship must coexist with low hostility.
            if r.friendship >= 0.35 && r.trust >= 0.35 && r.hostility < 0.25 {
                ally_score += 0.4 + r.friendship * 0.35 + r.trust * 0.25;
            }

            // Broker heuristic: ties across different gang memberships / otherwise separated actors.
            let other_gangs = member_of.get(&other).cloned().unwrap_or_default();
            let crosses = !my_gangs.is_empty()
                && !other_gangs.is_empty()
                && my_gangs.iter().all(|g| !other_gangs.contains(g));
            if crosses && r.familiarity > 0.2 && r.bond_score() > 0.2 {
                bridge_score += 0.4 + r.familiarity * 0.4 + r.trust * 0.2;
            } else if my_gangs.is_empty() && !other_gangs.is_empty() && r.familiarity > 0.25 {
                bridge_score += 0.15 + r.familiarity * 0.2;
            } else if !my_gangs.is_empty() && other_gangs.is_empty() && r.familiarity > 0.25 {
                bridge_score += 0.1 + r.familiarity * 0.15;
            }

            // Informant-suspect proxy (uncertain): low in-group bonding signals but unusual access/calm around authority-like pressure.
            // Without staff graph here, use asymmetric fear/low resentment + moderate familiarity as weak proxy.
            if r.fear > 0.45 && r.hostility < 0.2 && r.familiarity > 0.2 {
                staff_proximity_proxy += 0.2;
            }
            if !my_gangs.is_empty() && other_gangs.iter().any(|g| my_gangs.contains(g)) {
                in_group_trust_proxy += r.trust + r.loyalty;
            }
        }

        let mut roles = Vec::new();
        let avg_familiarity = familiarity_sum / (n - 1.0).max(1.0);

        if feared > 2.0 {
            roles.push((SocialRole::Fighter, (feared / n).clamp(0.0, 1.0)));
        }
        if hostility_in > 2.5 {
            roles.push((SocialRole::Victim, (hostility_in / n).clamp(0.0, 1.0)));
        }
        if friends > 3.0 {
            roles.push((SocialRole::Leader, (friends / n).clamp(0.0, 1.0)));
        }
        if lonely_count > n * 0.7 {
            roles.push((SocialRole::Loner, (lonely_count / n).clamp(0.0, 1.0)));
        }
        if ally_score >= 0.9 {
            roles.push((SocialRole::Ally, (ally_score / n).clamp(0.25, 1.0)));
        }
        let protection_avg = protection_sum / (n - 1.0).max(1.0);
        let vulnerability = (feared / n) * 0.45
            + (hostility_in / n) * 0.35
            + (1.0 - protection_avg.clamp(0.0, 1.0)) * 0.20;
        if vulnerability > 0.35 && strong_ties <= 1 {
            roles.push((SocialRole::Vulnerable, vulnerability.clamp(0.0, 1.0)));
        }

        // Broker: moderate-to-high bridge score with spread contacts.
        if bridge_score > 0.8 && contact_spread >= 2 {
            let score = (bridge_score / (1.0 + n * 0.15)).clamp(0.0, 1.0);
            roles.push((SocialRole::Broker, score.max(0.35)));
        }

        // InformantSuspect: weak/uncertain behavioral anomaly heuristic only.
        let low_in_group = if my_gangs.is_empty() {
            strong_ties <= 1 && friends < 1.5
        } else {
            in_group_trust_proxy < (my_gangs.len() as f32 * 0.8)
        };
        if low_in_group && staff_proximity_proxy > 0.6 && avg_familiarity > 0.15 {
            let score = ((staff_proximity_proxy * 0.25) + (0.4 - in_group_trust_proxy * 0.05))
                .clamp(0.1, 0.75);
            roles.push((SocialRole::InformantSuspect, score));
        }

        for g in gangs {
            if g.leader == Some(id) {
                roles.push((SocialRole::Leader, 0.9));
            }
            if g.members.contains(&id) && g.members.len() > 5 {
                roles.push((SocialRole::Influencer, 0.6));
            }
        }

        // De-duplicate by keeping max score per role.
        let mut best: HashMap<u8, (SocialRole, f32)> = HashMap::new();
        for (role, score) in roles {
            let k = role as u8;
            match best.get(&k) {
                Some((_, s)) if *s >= score => {}
                _ => {
                    best.insert(k, (role, score));
                }
            }
        }
        let mut compact: Vec<(SocialRole, f32)> = best.into_values().collect();
        compact.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| (a.0 as u8).cmp(&(b.0 as u8)))
        });
        out.insert(id, compact);
    }
    out
}
