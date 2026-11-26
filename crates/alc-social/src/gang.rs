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

/// Clamp a heuristic score without allowing NaN to poison a decision.
fn clamp01(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
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
    pub commitment: f32, // 0..1
    pub influence: f32,  // 0..1 within gang
    pub grievances: f32, // 0..1
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
                if let Some(rec) = self.hierarchy.iter_mut().find(|r| r.prisoner == new_leader) {
                    rec.rank = Rank::Leader;
                    rec.influence = rec.influence.max(0.9);
                }
            }
        }
        if self.members.is_empty() {
            self.active = false;
        }
    }

    /// Split a coherent mutinous subset into a new gang.
    ///
    /// The caller allocates `new_id` so the world can keep identifiers unique
    /// without this module guessing at allocator state.
    pub fn fracture(&mut self, splinter: &[PrisonerId], new_id: GangId) -> Option<Gang> {
        if new_id == self.id {
            return None;
        }
        let mut split: Vec<PrisonerId> = splinter
            .iter()
            .copied()
            .filter(|p| self.members.contains(p))
            .collect();
        split.sort_unstable_by_key(|p| p.0);
        split.dedup();
        if split.is_empty() || split.len() >= self.members.len() {
            return None;
        }
        let founder = split[0];
        let mut new_gang = Gang::with_formed_at(
            new_id,
            format!("{} splinter", self.name),
            founder,
            self.formed_at,
        );
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

    /// Absorb another gang, moving its members, resources, goals and relations.
    ///
    /// Returns the ids actually moved into `self`. `other` is left inactive and
    /// empty so no duplicated organization survives the merge.
    pub fn merge_from(&mut self, other: &mut Gang) -> Vec<PrisonerId> {
        if self.id == other.id {
            return Vec::new();
        }
        let self_count = self.members.len();
        let other_count = other.members.len();
        let formed_at = self.formed_at;

        let mut moved: Vec<PrisonerId> = other
            .members
            .iter()
            .copied()
            .filter(|p| !self.members.contains(p))
            .collect();
        moved.sort_unstable_by_key(|p| p.0);
        moved.dedup();

        for &p in &moved {
            self.members.push(p);
            let record = other
                .hierarchy
                .iter()
                .find(|r| r.prisoner == p)
                .cloned()
                .unwrap_or_else(|| MemberRecord::new(p, formed_at, Rank::Member));
            self.hierarchy.push(record);
        }

        self.resources += other.resources.max(0.0);
        other.resources = 0.0;

        if self_count + other_count > 0 {
            let total = (self_count + other_count) as f32;
            self.reputation = clamp01(
                (self.reputation * self_count as f32 + other.reputation * other_count as f32)
                    / total,
            );
        }

        for goal in &other.goals {
            if !self.goals.contains(goal) {
                self.goals.push(goal.clone());
            }
        }
        self.goals.sort();
        for note in &other.internal_disputes {
            self.record_dispute(note.clone());
        }

        for &id in &other.rivalries {
            if id != self.id && id != other.id && !self.rivalries.contains(&id) {
                self.rivalries.push(id);
            }
        }
        for &id in &other.alliances {
            if id != self.id && id != other.id && !self.alliances.contains(&id) {
                self.alliances.push(id);
            }
        }
        self.rivalries.retain(|g| *g != self.id && *g != other.id);
        self.alliances.retain(|g| *g != self.id && *g != other.id);

        other.members.clear();
        other.hierarchy.clear();
        other.internal_disputes.clear();
        other.rivalries.clear();
        other.alliances.clear();
        other.goals.clear();
        other.leader = None;
        other.active = false;
        moved
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
            self.hierarchy
                .push(MemberRecord::new(new_leader, self.formed_at, Rank::Leader));
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

/// Kinds of durable gang-history entries (Directive §18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GangHistoryKind {
    Formed,
    Recruit,
    Loss,
    LeadershipChange,
    Split,
    Merge,
    Alliance,
    Rivalry,
    Dispute,
    MajorConflict,
    Disbanded,
}

impl GangHistoryKind {
    /// Short stable label suitable for history feeds and filters.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Formed => "formed",
            Self::Recruit => "recruit",
            Self::Loss => "loss",
            Self::LeadershipChange => "leader",
            Self::Split => "split",
            Self::Merge => "merge",
            Self::Alliance => "alliance",
            Self::Rivalry => "rivalry",
            Self::Dispute => "dispute",
            Self::MajorConflict => "conflict",
            Self::Disbanded => "disbanded",
        }
    }
}

impl std::fmt::Display for GangHistoryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// How a dissatisfied member acts on a grievance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisputeAction {
    Refuse,
    Covert,
    Negotiate,
    ChallengeLeader,
}

/// A detected member-vs-leadership grievance (pure observation; no state change).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InternalDispute {
    pub prisoner: PrisonerId,
    pub action: DisputeAction,
    pub grievance_delta: f32, // 0..1, proposed addition to MemberRecord::grievances
    pub reason: String,
}

/// Per-prisoner inputs for gang-politics heuristics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GangPoliticsProfile {
    pub prisoner: PrisonerId,
    pub loyalty: f32,       // 0..1
    pub autonomy: f32,      // 0..1
    pub discipline: f32,    // 0..1
    pub leadership: f32,    // 0..1
    pub ambition: f32,      // 0..1
    pub goal_conflict: f32, // 0..1
}

/// Aggregate institutional pressure exerted by the gang graph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct GangPressure {
    pub rivalry: f32,    // 0..1
    pub disputes: f32,   // 0..1
    pub contraband: f32, // 0..1
}

impl GangPressure {
    /// Weighted, bounded composite pressure in `0..=1`.
    #[must_use]
    pub fn total(&self) -> f32 {
        clamp01(self.rivalry * 0.4 + self.disputes * 0.3 + self.contraband * 0.3)
    }
}

/// Stable world-history cause marker for events sourced from gang politics.
#[must_use]
pub fn gang_event_cause(gang: GangId) -> String {
    format!("gang:{}", gang.0)
}

fn goal_affinity(left: &[String], right: &[String]) -> f32 {
    let left: Vec<String> = left
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let right: Vec<String> = right
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if left.is_empty() && right.is_empty() {
        return 0.0;
    }
    let hits = left
        .iter()
        .filter(|goal| right.iter().any(|other| other == *goal))
        .count();
    clamp01(hits as f32 / left.len().max(right.len()).max(1) as f32)
}

fn culture_affinity(left: &str, right: &str) -> f32 {
    let left = left.trim().to_ascii_lowercase();
    let right = right.trim().to_ascii_lowercase();
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left == right {
        return 1.0;
    }
    let left_tokens: Vec<&str> = left.split_whitespace().collect();
    let right_tokens: Vec<&str> = right.split_whitespace().collect();
    let hits = left_tokens
        .iter()
        .filter(|token| right_tokens.iter().any(|other| other == *token))
        .count();
    clamp01(hits as f32 / left_tokens.len().max(right_tokens.len()).max(1) as f32)
}

/// Deterministic `(alliance_affinity, rivalry_hostility)` in `0..=1`.
///
/// Derived only from goals, culture, declared relations and observed dyads in
/// `graph`; no map iteration order is involved.
#[must_use]
pub fn gang_relation_scores(a: &Gang, b: &Gang, graph: &SocialGraph) -> (f32, f32) {
    if a.id == b.id {
        return (1.0, 0.0);
    }
    let goals = goal_affinity(&a.goals, &b.goals);
    let culture = culture_affinity(&a.culture, &b.culture);

    let mut affiliative = 0.0f32;
    let mut hostile = 0.0f32;
    let mut pairs = 0usize;
    for &member_a in &a.members {
        for &member_b in &b.members {
            if member_a == member_b {
                continue;
            }
            let rel = graph.get(member_a, member_b);
            affiliative += clamp01(
                (rel.friendship + rel.trust * 0.5 + rel.loyalty * 0.4 + rel.gratitude * 0.2
                    - rel.hostility
                    - rel.rivalry
                    - rel.resentment * 0.5
                    + 1.0)
                    * 0.5,
            );
            hostile +=
                clamp01(rel.hostility + rel.rivalry * 0.8 + rel.fear * 0.3 + rel.resentment * 0.3);
            pairs += 1;
        }
    }
    let (member_aff, member_host) = if pairs == 0 {
        (0.5, 0.0)
    } else {
        (
            clamp01(affiliative / pairs as f32),
            clamp01(hostile / pairs as f32),
        )
    };

    let alliance_bonus = if a.alliances.contains(&b.id) || b.alliances.contains(&a.id) {
        0.15
    } else {
        0.0
    };
    let rivalry_bonus = if a.rivalries.contains(&b.id) || b.rivalries.contains(&a.id) {
        0.2
    } else {
        0.0
    };

    let affinity = clamp01(
        goals * 0.25 + culture * 0.15 + member_aff * 0.6 + alliance_bonus - rivalry_bonus * 0.5,
    );
    let hostility =
        clamp01(member_host * 0.7 + rivalry_bonus + (1.0 - goals) * 0.1 - affinity * 0.2);
    (affinity, hostility)
}

/// Detect disgruntled non-leader members and their likely response to a directive.
///
/// Pure: reads the gang, graph and caller-supplied profiles only. Results are
/// sorted by prisoner id so the world can apply them in a stable order.
#[must_use]
pub fn detect_internal_disputes(
    gang: &Gang,
    graph: &SocialGraph,
    profiles: &[GangPoliticsProfile],
    directive: &GroupDirective,
) -> Vec<InternalDispute> {
    let Some(leader) = gang.leader else {
        return Vec::new();
    };
    let mut out = Vec::new();

    for &prisoner in &gang.members {
        if prisoner == leader {
            continue;
        }
        let Some(profile) = profiles.iter().find(|p| p.prisoner == prisoner) else {
            continue;
        };
        let loyalty = clamp01(profile.loyalty);
        let autonomy = clamp01(profile.autonomy);
        let discipline = clamp01(profile.discipline);
        let leadership = clamp01(profile.leadership);
        let ambition = clamp01(profile.ambition);
        let goal_conflict = clamp01(profile.goal_conflict);

        let rel = graph.get(prisoner, leader);
        let rel_pos = clamp01(
            (rel.friendship + rel.trust * 0.6 + rel.loyalty * 0.4 + rel.gratitude * 0.2
                - rel.hostility
                - rel.resentment
                - rel.rivalry
                + 1.0)
                * 0.5,
        );
        let grievances = gang
            .hierarchy
            .iter()
            .find(|r| r.prisoner == prisoner)
            .map_or(0.0, |r| clamp01(r.grievances));

        let pressure = clamp01(directive.pressure);
        let risk = clamp01(directive.personal_risk);
        let gain = clamp01(directive.expected_gain);
        let score = clamp01(
            goal_conflict * 0.32
                + (1.0 - loyalty) * 0.22
                + (1.0 - discipline) * 0.08
                + (1.0 - rel_pos) * 0.16
                + grievances * 0.12
                + pressure * 0.08
                + risk * 0.06
                - gain * 0.08,
        );
        if score < 0.38 {
            continue;
        }

        let ctx = MemberDecisionContext {
            loyalty,
            fear_of_gang: clamp01(rel.fear * 0.7 + pressure * 0.3),
            autonomy,
            empathy: 0.5,
            discipline,
            relationship_to_leader: rel.bond_score().clamp(-1.0, 1.0),
            goal_conflict,
            moral_discomfort: clamp01(score * 0.7),
            retaliation_fear: clamp01(pressure * 0.5 + rel.fear * 0.5),
            deception_opportunity: clamp01((1.0 - discipline) * 0.6 + autonomy * 0.4),
            negotiation_leverage: clamp01(leadership * 0.5 + rel.respect * 0.3 + rel_pos * 0.2),
        };
        let resolution = resolve_leader_member_conflict(directive, &ctx);

        let challenge = clamp01(
            leadership * 0.3
                + ambition * 0.28
                + (1.0 - loyalty) * 0.18
                + goal_conflict * 0.16
                + (1.0 - rel_pos) * 0.12
                + grievances * 0.1
                - discipline * 0.16,
        );
        let action = if challenge >= 0.58 {
            DisputeAction::ChallengeLeader
        } else {
            match resolution.decision {
                MemberDecision::Refuse => DisputeAction::Refuse,
                MemberDecision::Covert => DisputeAction::Covert,
                MemberDecision::Negotiate => DisputeAction::Negotiate,
                MemberDecision::Comply => {
                    if discipline > 0.6 {
                        DisputeAction::Negotiate
                    } else {
                        DisputeAction::Refuse
                    }
                }
            }
        };

        let reason = if goal_conflict >= 0.55 {
            "goal conflict"
        } else if rel_pos < 0.25 {
            "leader distrust"
        } else if loyalty < 0.35 {
            "low loyalty"
        } else if autonomy > 0.65 && discipline < 0.45 {
            "autonomy pressure"
        } else {
            "unresolved grievance"
        };

        out.push(InternalDispute {
            prisoner,
            action,
            grievance_delta: (score * 0.25 + 0.05).clamp(0.05, 0.3),
            reason: reason.to_owned(),
        });
    }

    out.sort_by_key(|d| d.prisoner.0);
    out
}

/// Pick a coherent mutinous subset suitable for `Gang::fracture`.
#[must_use]
pub fn split_candidates(gang: &Gang, disputes: &[InternalDispute]) -> Option<Vec<PrisonerId>> {
    let mut candidates: Vec<PrisonerId> = disputes
        .iter()
        .filter(|d| {
            d.action != DisputeAction::Negotiate
                && d.grievance_delta >= 0.1
                && gang.members.contains(&d.prisoner)
                && gang.leader != Some(d.prisoner)
        })
        .map(|d| d.prisoner)
        .collect();
    candidates.sort_unstable_by_key(|p| p.0);
    candidates.dedup();
    if candidates.len() < 2 || candidates.len() >= gang.members.len() {
        return None;
    }

    // A splinter needs a shared grievance, not unrelated grudges.
    let mut reasons: Vec<&str> = disputes
        .iter()
        .filter(|d| candidates.contains(&d.prisoner))
        .map(|d| d.reason.as_str())
        .collect();
    reasons.sort_unstable();
    reasons.dedup();
    if reasons.len() > 2 {
        return None;
    }
    Some(candidates)
}

/// Choose the strongest leadership challenger among a gang's members.
#[must_use]
pub fn leadership_challenger(
    gang: &Gang,
    profiles: &[GangPoliticsProfile],
    rng: &mut RngStream,
) -> Option<PrisonerId> {
    let mut candidates: Vec<&GangPoliticsProfile> = profiles
        .iter()
        .filter(|p| gang.members.contains(&p.prisoner) && gang.leader != Some(p.prisoner))
        .collect();
    candidates.sort_by_key(|p| p.prisoner.0);

    let mut best: Option<(PrisonerId, f32)> = None;
    for profile in candidates {
        let grievances = gang
            .hierarchy
            .iter()
            .find(|r| r.prisoner == profile.prisoner)
            .map_or(0.0, |r| clamp01(r.grievances));
        let score = clamp01(
            clamp01(profile.leadership) * 0.3
                + clamp01(profile.ambition) * 0.28
                + (1.0 - clamp01(profile.loyalty)) * 0.18
                + grievances * 0.16
                - clamp01(profile.discipline) * 0.08
                + (rng.next_f64() as f32) * 0.08,
        );
        let better = match best {
            None => true,
            Some((current, previous)) => match score.total_cmp(&previous) {
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Equal => profile.prisoner.0 < current.0,
                std::cmp::Ordering::Less => false,
            },
        };
        if better {
            best = Some((profile.prisoner, score));
        }
    }
    best.map(|(prisoner, _)| prisoner)
}

/// Bounded, deterministic pressure from rivalries, disputes and hostile ties.
#[must_use]
pub fn gang_pressure(gangs: &[Gang], graph: &SocialGraph) -> GangPressure {
    let mut rival_pairs: std::collections::BTreeSet<(u64, u64)> = std::collections::BTreeSet::new();
    for gang in gangs.iter().filter(|g| g.active) {
        for &other_id in &gang.rivalries {
            let Some(other) = gangs
                .iter()
                .find(|g| g.active && g.id == other_id && g.id != gang.id)
            else {
                continue;
            };
            let pair = if gang.id.0 <= other.id.0 {
                (gang.id.0, other.id.0)
            } else {
                (other.id.0, gang.id.0)
            };
            rival_pairs.insert(pair);
        }
    }
    let max_pairs = gangs.len().saturating_mul(gangs.len().saturating_sub(1)) / 2;
    let rivalry = clamp01(rival_pairs.len() as f32 / max_pairs.max(1) as f32);

    let dispute_count: usize = gangs.iter().map(|g| g.internal_disputes.len()).sum();
    let disputes = clamp01(dispute_count as f32 / 12.0);

    let members: std::collections::BTreeSet<u64> = gangs
        .iter()
        .flat_map(|g| g.members.iter().map(|p| p.0))
        .collect();
    let mut contraband_sum = 0.0f32;
    let mut edges = 0usize;
    for ((a, b), rel) in graph.iter() {
        if !members.contains(&a) && !members.contains(&b) {
            continue;
        }
        edges += 1;
        contraband_sum += clamp01(rel.rivalry * 0.45 + rel.hostility * 0.35 + rel.fear * 0.2)
            * clamp01(rel.familiarity);
    }
    let contraband = if edges == 0 {
        0.0
    } else {
        clamp01(contraband_sum / edges as f32)
    };

    GangPressure {
        rivalry,
        disputes,
        contraband,
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
    let free: Vec<&GangFormationCandidate> =
        candidates.iter().filter(|c| !c.already_in_gang).collect();
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
            let enemy_match =
                seed.common_enemy.is_some() && seed.common_enemy == other.common_enemy;
            let driver = friend * 0.35
                + other.protection_need * 0.25
                + if bg_match { 0.2 } else { 0.0 }
                + if enemy_match { 0.2 } else { 0.0 };
            if driver > 0.45
                || (friend > 0.8 && (bg_match || enemy_match || other.protection_need > 0.6))
            {
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
            seed.background_tag
                .clone()
                .unwrap_or_else(|| "kinship".into())
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
    let mut gang = Gang::with_formed_at(
        next_gang_id,
        format!("Crew {}", next_gang_id.0),
        founder,
        now,
    );
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
    let comply = (ctx.loyalty * 0.35
        + leader_rel * 0.2
        + directive.pressure * 0.2
        + directive.expected_gain * 0.15
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
            generate_gang_name(rng, &seed.background),
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

/// Generate a deterministic gang name from RNG and culture/background.
pub fn generate_gang_name(rng: &mut RngStream, culture: &str) -> String {
    const NAMES: [&str; 24] = [
        "Iron Saints", "Red Hand", "Black Wolves", "Night Crew", "Stone Syndicate",
        "Glass Serpents", "Blue Collar", "Cold Front", "Dead Reckoning", "Last Resort",
        "Quiet Riot", "Iron Gate", "Broken Halo", "Shadow Market", "Steel Union",
        "Lost Boys", "Dark Harbor", "Second Chance", "Iron Fist", "Paper Tigers",
        "Silent Accord", "Rust Kings", "White Noise", "Final Word",
    ];
    let name = NAMES[(rng.next_u64() as usize) % NAMES.len()];
    if culture.is_empty() || culture == "new" {
        name.to_string()
    } else {
        format!("{} {}", name, culture)
    }
}

