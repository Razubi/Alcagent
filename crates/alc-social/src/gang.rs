//! Gang system per Directive §7.

use crate::relationships::SocialGraph;
use alc_core::{GangId, PrisonerId, RngStream, SimTime};
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_sim_time() -> SimTime {
    SimTime::EPOCH
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rank {
    Leader,
    Lieutenant,
    Enforcer,
    Member,
    Associate,
    Prospect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberRecord {
    pub prisoner: PrisonerId,
    pub joined_at: SimTime,
    pub rank: Rank,
    pub commitment: f32,   // 0..1
    pub influence: f32,    // 0..1 within gang
    pub grievances: f32,   // 0..1
    pub expected_protection: f32,
    pub contributions: f32,
}

impl MemberRecord {
    pub fn new(prisoner: PrisonerId, joined_at: SimTime, rank: Rank) -> Self {
        Self {
            prisoner,
            joined_at,
            rank,
            commitment: 0.5,
            influence: match rank {
                Rank::Leader => 1.0,
                Rank::Lieutenant => 0.7,
                Rank::Enforcer => 0.55,
                Rank::Member => 0.4,
                Rank::Associate => 0.25,
                Rank::Prospect => 0.15,
            },
            grievances: 0.0,
            expected_protection: 0.5,
            contributions: 0.0,
        }
    }
}

/// Emergent gang organization (Directive §7), backward-compatible core fields preserved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gang {
    pub id: GangId,
    pub name: String,
    pub leader: Option<PrisonerId>,
    pub members: Vec<PrisonerId>,
    pub culture: String,
    pub reputation: f32,
    pub resources: f32,
    pub rivalries: Vec<GangId>,
    pub alliances: Vec<GangId>,
    // Extended Phase 3 fields (additive).
    #[serde(default = "default_sim_time")]
    pub formed_at: SimTime,
    #[serde(default)]
    pub hierarchy: Vec<MemberRecord>,
    #[serde(default)]
    pub internal_disputes: Vec<String>,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default)]
    pub goals: Vec<String>,
}

impl Gang {
    pub fn new(id: GangId, name: impl Into<String>, founder: PrisonerId) -> Self {
        Self {
            id,
            name: name.into(),
            leader: Some(founder),
            members: vec![founder],
            culture: "new".into(),
            reputation: 0.5,
            resources: 0.0,
            rivalries: Vec::new(),
            alliances: Vec::new(),
            formed_at: SimTime::EPOCH,
            hierarchy: vec![MemberRecord::new(founder, SimTime::EPOCH, Rank::Leader)],
            internal_disputes: Vec::new(),
            active: true,
            goals: vec!["survival".into(), "protection".into()],
        }
    }

    pub fn with_formed_at(
        id: GangId,
        name: impl Into<String>,
        founder: PrisonerId,
        formed_at: SimTime,
    ) -> Self {
        let mut g = Self::new(id, name, founder);
        g.formed_at = formed_at;
        if let Some(r) = g.hierarchy.get_mut(0) {
            r.joined_at = formed_at;
        }
        g
    }

    pub fn recruit(&mut self, p: PrisonerId) {
        if self.members.contains(&p) {
            return;
        }
        self.members.push(p);
        self.hierarchy
            .push(MemberRecord::new(p, self.formed_at, Rank::Prospect));
        self.resources += 0.02;
    }

    pub fn lose(&mut self, p: PrisonerId) {
        self.members.retain(|m| *m != p);
        self.hierarchy.retain(|m| m.prisoner != p);
        if self.leader == Some(p) {
            self.leader = self.pick_new_leader();
            if let Some(new_leader) = self.leader {
                if let Some(rec) = self
                    .hierarchy
                    .iter_mut()
                    .find(|r| r.prisoner == new_leader)
                {
                    rec.rank = Rank::Leader;
                    rec.influence = rec.influence.max(0.9);
                }
            }
        }
        if self.members.is_empty() {
            self.active = false;
        }
    }

    pub fn fracture(&mut self, splinter: &[PrisonerId]) -> Option<Gang> {
        let split: Vec<PrisonerId> = splinter
            .iter()
            .copied()
            .filter(|p| self.members.contains(p))
            .collect();
        if split.is_empty() || split.len() >= self.members.len() {
            return None;
        }
        let new_id = GangId(self.id.0.saturating_mul(31).saturating_add(split[0].0).saturating_add(1));
        let founder = split[0];
        let mut new_gang = Gang::with_formed_at(new_id, format!("{} splinter", self.name), founder, self.formed_at);
        new_gang.culture = self.culture.clone();
        new_gang.reputation = (self.reputation * 0.6).clamp(0.1, 1.0);
        new_gang.resources = self.resources * 0.3;
        self.resources *= 0.7;
        for &p in &split[1..] {
            new_gang.recruit(p);
        }
        for &p in &split {
            self.lose(p);
        }
        new_gang.rivalries.push(self.id);
        if !self.rivalries.contains(&new_id) {
            self.rivalries.push(new_id);
        }
        self.record_dispute("fracture");
        Some(new_gang)
    }

    pub fn change_leader(&mut self, new_leader: PrisonerId) -> bool {
        if !self.members.contains(&new_leader) {
            return false;
        }
        if let Some(old) = self.leader {
            if let Some(rec) = self.hierarchy.iter_mut().find(|r| r.prisoner == old) {
                if rec.rank == Rank::Leader {
                    rec.rank = Rank::Lieutenant;
                }
            }
        }
        self.leader = Some(new_leader);
        if let Some(rec) = self.hierarchy.iter_mut().find(|r| r.prisoner == new_leader) {
            rec.rank = Rank::Leader;
            rec.influence = rec.influence.max(0.9);
        } else {
            self.hierarchy.push(MemberRecord::new(new_leader, self.formed_at, Rank::Leader));
        }
        self.reputation = (self.reputation - 0.05).max(0.0);
        true
    }

    pub fn form_alliance(&mut self, other: &mut Gang) {
        if self.id == other.id {
            return;
        }
        if !self.alliances.contains(&other.id) {
            self.alliances.push(other.id);
        }
        if !other.alliances.contains(&self.id) {
            other.alliances.push(self.id);
        }
        self.rivalries.retain(|g| *g != other.id);
        other.rivalries.retain(|g| *g != self.id);
    }

    pub fn form_rivalry(&mut self, other: &mut Gang) {
        if self.id == other.id {
            return;
        }
        if !self.rivalries.contains(&other.id) {
            self.rivalries.push(other.id);
        }
        if !other.rivalries.contains(&self.id) {
            other.rivalries.push(self.id);
        }
        self.alliances.retain(|g| *g != other.id);
        other.alliances.retain(|g| *g != self.id);
    }

    /// Record an internal dispute note with bounded history (prevents unbounded growth).
    pub fn record_dispute(&mut self, note: impl Into<String>) {
        const MAX_DISPUTES: usize = 64;
        self.internal_disputes.push(note.into());
        if self.internal_disputes.len() > MAX_DISPUTES {
            let drop = self.internal_disputes.len() - MAX_DISPUTES;
            self.internal_disputes.drain(0..drop);
        }
    }

    fn pick_new_leader(&self) -> Option<PrisonerId> {
        self.hierarchy
            .iter()
            .filter(|r| self.members.contains(&r.prisoner))
            .max_by(|a, b| {
                (a.influence + a.commitment - a.grievances)
                    .partial_cmp(&(b.influence + b.commitment - b.grievances))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.prisoner)
            .or_else(|| self.members.first().copied())
    }
}

/// Candidate data for emergent gang formation (caller supplies from prisoner profiles/history).
#[derive(Debug, Clone)]
pub struct GangFormationCandidate {
    pub id: PrisonerId,
    pub protection_need: f32, // 0..1
    pub ambition: f32,        // 0..1
    pub background_tag: Option<String>,
    pub common_enemy: Option<PrisonerId>,
    pub already_in_gang: bool,
}

/// Emergent formation heuristic (protection-seeking, shared background, friendship clusters, ambition, common enemy).
pub fn form_gang_if_drivers(
    graph: &SocialGraph,
    candidates: &[GangFormationCandidate],
    existing_gangs: &[Gang],
    next_gang_id: GangId,
    now: SimTime,
) -> Option<Gang> {
    let free: Vec<&GangFormationCandidate> = candidates.iter().filter(|c| !c.already_in_gang).collect();
    if free.len() < 3 {
        return None;
    }

    // Find best seed by high need+ambition and strong local friendships.
    let mut best_seed: Option<(PrisonerId, Vec<PrisonerId>, f32, String)> = None;
    for seed in &free {
        let mut cluster = vec![seed.id];
        let mut score_sum = seed.protection_need * 0.35 + seed.ambition * 0.25;
        let mut shared_bg = 0usize;
        let mut common_enemy_hits = 0usize;
        for other in &free {
            if other.id == seed.id {
                continue;
            }
            let rel = graph.get(seed.id, other.id);
            let friend = rel.friendship + rel.trust * 0.5 + rel.loyalty * 0.4;
            let bg_match = seed
                .background_tag
                .as_ref()
                .zip(other.background_tag.as_ref())
                .map(|(a, b)| a == b)
                .unwrap_or(false);
            let enemy_match = seed.common_enemy.is_some() && seed.common_enemy == other.common_enemy;
            let driver = friend * 0.35
                + other.protection_need * 0.25
                + if bg_match { 0.2 } else { 0.0 }
                + if enemy_match { 0.2 } else { 0.0 };
            if driver > 0.45 || (friend > 0.8 && (bg_match || enemy_match || other.protection_need > 0.6)) {
                cluster.push(other.id);
                score_sum += driver + other.ambition * 0.1;
                if bg_match {
                    shared_bg += 1;
                }
                if enemy_match {
                    common_enemy_hits += 1;
                }
            }
        }
        if cluster.len() < 3 {
            continue;
        }
        let avg = score_sum / cluster.len() as f32;
        let culture = if common_enemy_hits >= 2 {
            "retaliatory".into()
        } else if shared_bg >= 2 {
            seed.background_tag.clone().unwrap_or_else(|| "kinship".into())
        } else if avg > 0.7 {
            "predatory".into()
        } else {
            "protective".into()
        };
        let qualifies = avg > 0.5
            || (seed.protection_need > 0.65 && cluster.len() >= 3)
            || (shared_bg >= 2 && cluster.len() >= 3)
            || (seed.ambition > 0.7 && cluster.len() >= 3 && avg > 0.45);
        if !qualifies {
            continue;
        }
        // Avoid duplicating an identical large cluster from existing gang.
        let duplicate = existing_gangs.iter().any(|g| {
            let overlap = cluster.iter().filter(|m| g.members.contains(m)).count();
            overlap >= cluster.len().saturating_sub(1)
        });
        if duplicate {
            continue;
        }
        match &best_seed {
            Some((_, _, best, _)) if *best >= avg => {}
            _ => best_seed = Some((seed.id, cluster, avg, culture)),
        }
    }

    let (founder, members, avg, culture) = best_seed?;
    let mut gang = Gang::with_formed_at(next_gang_id, format!("Crew {}", next_gang_id.0), founder, now);
    gang.culture = culture;
    gang.reputation = (0.4 + avg * 0.3).clamp(0.2, 0.9);
    gang.resources = (avg * 0.2).clamp(0.0, 0.5);
    for m in members {
        if m != founder {
            gang.recruit(m);
        }
    }
    Some(gang)
}

/// Directive/order from gang leadership context.
#[derive(Debug, Clone, Default)]
pub struct GroupDirective {
    pub description: String,
    pub pressure: f32,      // 0..1 coercion/expectation
    pub expected_gain: f32, // 0..1
    pub personal_risk: f32, // 0..1
}

/// Inputs for leader-vs-member motivation conflict resolution.
#[derive(Debug, Clone, Default)]
pub struct MemberDecisionContext {
    pub loyalty: f32,
    pub fear_of_gang: f32,
    pub autonomy: f32,
    pub empathy: f32,
    pub discipline: f32,
    pub relationship_to_leader: f32, // -1..1
    pub goal_conflict: f32,          // 0..1
    pub moral_discomfort: f32,       // 0..1
    pub retaliation_fear: f32,       // 0..1
    pub deception_opportunity: f32,  // 0..1
    pub negotiation_leverage: f32,   // 0..1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberDecision {
    Comply,
    Refuse,
    Covert,
    Negotiate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderMemberResolution {
    pub decision: MemberDecision,
    pub comply_score: f32,
    pub refuse_score: f32,
    pub covert_score: f32,
    pub negotiate_score: f32,
}

/// Resolve leader-vs-member motivation conflict (comply/refuse/covert/negotiate).
pub fn resolve_leader_member_conflict(
    directive: &GroupDirective,
    ctx: &MemberDecisionContext,
) -> LeaderMemberResolution {
    let leader_rel = (ctx.relationship_to_leader + 1.0) * 0.5;
    let comply = (ctx.loyalty * 0.35 + leader_rel * 0.2 + directive.pressure * 0.2 + directive.expected_gain * 0.15
        - ctx.goal_conflict * 0.2
        - directive.personal_risk * 0.2
        - ctx.autonomy * 0.1
        + ctx.fear_of_gang * 0.15)
        .clamp(0.0, 1.5);
    let refuse = (ctx.goal_conflict * 0.35 + ctx.moral_discomfort * 0.2 + ctx.autonomy * 0.2
        - ctx.retaliation_fear * 0.25
        - directive.pressure * 0.15
        - ctx.fear_of_gang * 0.15
        + ctx.discipline * 0.05)
        .clamp(0.0, 1.5);
    let covert = (ctx.deception_opportunity * 0.4
        + ctx.goal_conflict * 0.2
        + ctx.moral_discomfort * 0.15
        + directive.pressure * 0.1
        - ctx.discipline * 0.15
        - ctx.empathy * 0.05)
        .clamp(0.0, 1.5);
    let negotiate = (ctx.negotiation_leverage * 0.4
        + ctx.discipline * 0.15
        + (1.0 - ctx.goal_conflict) * 0.1
        + leader_rel * 0.15
        + directive.expected_gain * 0.1
        - directive.pressure * 0.1)
        .clamp(0.0, 1.5);

    let mut best = (MemberDecision::Comply, comply);
    for (d, s) in [
        (MemberDecision::Refuse, refuse),
        (MemberDecision::Covert, covert),
        (MemberDecision::Negotiate, negotiate),
    ] {
        if s > best.1 {
            best = (d, s);
        }
    }
    LeaderMemberResolution {
        decision: best.0,
        comply_score: comply,
        refuse_score: refuse,
        covert_score: covert,
        negotiate_score: negotiate,
    }
}

/// Utility: update gang rivalry graph when two gangs conflict.
pub fn register_rivalry(gangs: &mut [Gang], a: GangId, b: GangId) {
    for g in gangs.iter_mut() {
        if g.id == a && !g.rivalries.contains(&b) {
            g.rivalries.push(b);
        }
        if g.id == b && !g.rivalries.contains(&a) {
            g.rivalries.push(a);
        }
    }
}

/// Minimal prisoner profile needed for emergent gang formation scans.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrisonerGangProfile {
    pub id: PrisonerId,
    pub gang_affiliation: Option<u64>,
    pub background: String,
    /// 0..1 pressure to seek protection/group safety.
    pub protection_need: f32,
    /// 0..1 unmet safety pressure.
    pub safety_need: f32,
    pub ambition: f32,
    pub sociability: f32,
}

/// Emergent gang formation scan.
///
/// Scans for clusters of 3+ prisoners with high mutual friendship, shared
/// background, and protection-seeking need, and forms a new gang if drivers are
/// met.
pub fn maybe_form_gang(
    world_prisoners: &[PrisonerGangProfile],
    world_social: &SocialGraph,
    world_gangs: &[Gang],
    rng: &mut RngStream,
) -> Option<Gang> {
    if world_prisoners.len() < 3 {
        return None;
    }

    // Eligible prisoners are not already affiliated with a gang.
    let mut in_any_gang = std::collections::HashSet::new();
    for g in world_gangs {
        for &m in &g.members {
            in_any_gang.insert(m);
        }
    }
    let mut eligible: Vec<&PrisonerGangProfile> = world_prisoners
        .iter()
        .filter(|p| p.gang_affiliation.is_none() && !in_any_gang.contains(&p.id))
        .collect();
    if eligible.len() < 3 {
        return None;
    }

    // Randomize search order for emergent variety.
    for i in (1..eligible.len()).rev() {
        let j = (rng.next_u64() as usize) % (i + 1);
        eligible.swap(i, j);
    }

    let next_id = world_gangs.iter().map(|g| g.id.0).max().unwrap_or(0) + 1;

    for seed in &eligible {
        let seed_need = seed.protection_need.max(seed.safety_need);
        if seed_need < 0.45 && seed.ambition < 0.45 {
            continue;
        }

        let mut cluster: Vec<&PrisonerGangProfile> = vec![*seed];
        let mut shared_bg_hits = 0usize;

        for other in &eligible {
            if other.id == seed.id {
                continue;
            }
            let rel = world_social.get(seed.id, other.id);
            let friendship = rel.friendship;
            let bond = rel.bond_score();
            let mutual_friendship = friendship >= 0.45 || bond >= 0.7;
            if !mutual_friendship {
                continue;
            }

            let bg_match = !seed.background.is_empty()
                && !other.background.is_empty()
                && seed.background == other.background;
            let protection_seek = other.protection_need >= 0.45 || other.safety_need >= 0.55;
            let social_pull = other.sociability >= 0.35 || rel.trust >= 0.35;

            // Drivers: friendship plus either shared background or protection-seeking pressure.
            if mutual_friendship && (bg_match || protection_seek) && social_pull {
                cluster.push(*other);
                if bg_match {
                    shared_bg_hits += 1;
                }
            }
        }

        if cluster.len() < 3 {
            continue;
        }

        // Ensure cluster cohesion (not just star around seed): require most pairs friendly.
        let mut friendly_pairs = 0usize;
        let mut total_pairs = 0usize;
        for i in 0..cluster.len() {
            for j in (i + 1)..cluster.len() {
                total_pairs += 1;
                let r = world_social.get(cluster[i].id, cluster[j].id);
                if r.friendship >= 0.35 || r.bond_score() >= 0.5 {
                    friendly_pairs += 1;
                }
            }
        }
        let cohesion = if total_pairs == 0 {
            0.0
        } else {
            friendly_pairs as f32 / total_pairs as f32
        };
        if cohesion < 0.45 {
            continue;
        }

        let avg_need = cluster
            .iter()
            .map(|p| p.protection_need.max(p.safety_need * 0.8))
            .sum::<f32>()
            / cluster.len() as f32;
        let avg_friend = cluster
            .iter()
            .skip(1)
            .map(|p| world_social.get(seed.id, p.id).friendship)
            .sum::<f32>()
            / (cluster.len() - 1).max(1) as f32;
        let shared_factor = shared_bg_hits as f32 / (cluster.len() - 1).max(1) as f32;

        let driver_score =
            avg_friend * 0.4 + shared_factor * 0.25 + avg_need * 0.25 + seed.ambition * 0.1;
        if driver_score < 0.4 {
            continue;
        }

        // Emergent chance so gangs do not form organically over time (not guaranteed every tick).
        let chance = (driver_score as f64 * 0.55).clamp(0.08, 0.9);
        if !rng.chance(chance) {
            continue;
        }

        let mut gang = Gang::with_formed_at(
            GangId(next_id),
            format!("Crew {}", next_id),
            seed.id,
            SimTime::EPOCH,
        );
        gang.culture = if shared_factor >= 0.5 {
            seed.background.clone()
        } else if avg_need > 0.65 {
            "protective".into()
        } else {
            "opportunistic".into()
        };
        gang.reputation = (0.35 + driver_score * 0.3).clamp(0.2, 0.9);
        gang.resources = (driver_score * 0.15).clamp(0.0, 0.4);
        for p in cluster.iter().skip(1) {
            gang.recruit(p.id);
        }
        return Some(gang);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relationships::RelationshipDelta;

    #[test]
    fn emergent_gang_formation_finds_cluster() {
        let prisoners = vec![
            PrisonerGangProfile {
                id: PrisonerId(1),
                gang_affiliation: None,
                background: "theft".into(),
                protection_need: 0.85,
                safety_need: 0.8,
                ambition: 0.6,
                sociability: 0.7,
            },
            PrisonerGangProfile {
                id: PrisonerId(2),
                gang_affiliation: None,
                background: "theft".into(),
                protection_need: 0.75,
                safety_need: 0.7,
                ambition: 0.4,
                sociability: 0.6,
            },
            PrisonerGangProfile {
                id: PrisonerId(3),
                gang_affiliation: None,
                background: "theft".into(),
                protection_need: 0.8,
                safety_need: 0.75,
                ambition: 0.5,
                sociability: 0.65,
            },
        ];
        let mut social = SocialGraph::default();
        for i in 0..prisoners.len() {
            for j in (i + 1)..prisoners.len() {
                social.record(
                    prisoners[i].id,
                    prisoners[j].id,
                    RelationshipDelta {
                        friendship: 0.8,
                        trust: 0.6,
                        loyalty: 0.5,
                        familiarity: 0.6,
                        ..Default::default()
                    },
                );
            }
        }
        let gangs: Vec<Gang> = Vec::new();
        let mut formed = None;
        for seed in 0..300u64 {
            let mut rng = RngStream::new(77, seed);
            formed = maybe_form_gang(&prisoners, &social, &gangs, &mut rng);
            if formed.is_some() {
                break;
            }
        }
        let gang = formed.expect("expected emergent gang formation");
        assert!(gang.members.len() >= 3);
        assert!(gang.active);
    }
}