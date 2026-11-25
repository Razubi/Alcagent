//! Canonical provider context construction (Directive §3, §19).
//!
//! The context assembled here is the *only* input handed to a decision
//! provider. It is sorted, bounded and derived from simulation data, so the
//! same world state always produces the same hash. Epistemic records are kept
//! in five explicit buckets (direct knowledge, rumor, suspicion, belief and
//! confirmed fact); agents must not treat rumor as fact.

use crate::decision::{ActionCandidate, Role, ValidationContext};
use crate::memory::{EpistemicKind, EpistemicRecord, Memory, MemoryQuery};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// A bounded memory item included in a provider context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextMemory {
    pub kind: EpistemicKind,
    pub id: u64,
    pub about: u64,
    pub summary: String,
    pub confidence: f32,
    pub relevance: f32,
}

impl ContextMemory {
    pub fn from_ranked(record: &EpistemicRecord, relevance: f32) -> Self {
        Self {
            kind: record.kind,
            id: record.source.unwrap_or(record.about),
            about: record.about,
            summary: record.content.clone(),
            confidence: record.confidence.clamp(0.0, 1.0),
            relevance: relevance.clamp(0.0, 1.0),
        }
    }
}

/// Relevant memories separated by epistemic status.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EpistemicContext {
    #[serde(default)]
    pub direct_knowledge: Vec<ContextMemory>,
    #[serde(default)]
    pub rumor: Vec<ContextMemory>,
    #[serde(default)]
    pub suspicion: Vec<ContextMemory>,
    #[serde(default)]
    pub belief: Vec<ContextMemory>,
    #[serde(default)]
    pub confirmed_fact: Vec<ContextMemory>,
}

impl EpistemicContext {
    /// Rank records through [`Memory::recall`], then bucket them by category.
    ///
    /// `top_k` is a hard cap: each bucket is truncated again so one category
    /// cannot crowd out every other source of information.
    pub fn from_memory(
        memory: &Memory,
        about: Option<u64>,
        tags: &[String],
        top_k: usize,
        now_secs: u64,
    ) -> Self {
        let mut context = Self::default();
        if top_k == 0 {
            return context;
        }
        let query = MemoryQuery {
            about,
            tags: tags.to_vec(),
            top_k: top_k.saturating_mul(2),
            now_secs,
            ..MemoryQuery::default()
        };
        let per_bucket = top_k.max(1);
        for ranked in memory.recall(&query) {
            let item = ContextMemory::from_ranked(ranked.record, ranked.score);
            let bucket = match ranked.record.kind {
                EpistemicKind::DirectKnowledge => &mut context.direct_knowledge,
                EpistemicKind::Rumor => &mut context.rumor,
                EpistemicKind::Suspicion => &mut context.suspicion,
                EpistemicKind::Belief => &mut context.belief,
                EpistemicKind::ConfirmedFact => &mut context.confirmed_fact,
            };
            if bucket.len() < per_bucket {
                bucket.push(item);
            }
        }
        context.sort();
        context
    }

    /// Build a context directly from already-ranked records.
    pub fn from_records(records: &[(EpistemicRecord, f32)], top_k: usize) -> Self {
        let mut memory = Memory::new();
        for (record, _) in records {
            memory.remember_scored(record.clone());
        }
        let mut context = Self::default();
        let mut ranked: Vec<(EpistemicRecord, f32)> = records.to_vec();
        ranked.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then_with(|| a.0.about.cmp(&b.0.about))
                .then_with(|| a.0.content.cmp(&b.0.content))
        });
        for (record, score) in ranked.into_iter().take(top_k.saturating_mul(2)) {
            let item = ContextMemory::from_ranked(&record, score);
            let bucket = match record.kind {
                EpistemicKind::DirectKnowledge => &mut context.direct_knowledge,
                EpistemicKind::Rumor => &mut context.rumor,
                EpistemicKind::Suspicion => &mut context.suspicion,
                EpistemicKind::Belief => &mut context.belief,
                EpistemicKind::ConfirmedFact => &mut context.confirmed_fact,
            };
            if bucket.len() < top_k.max(1) {
                bucket.push(item);
            }
        }
        context.sort();
        context
    }

    /// Canonicalize item order so hashes never depend on insertion order.
    pub fn sort(&mut self) {
        for bucket in [
            &mut self.direct_knowledge,
            &mut self.rumor,
            &mut self.suspicion,
            &mut self.belief,
            &mut self.confirmed_fact,
        ] {
            bucket.sort_by(|a, b| {
                b.relevance
                    .total_cmp(&a.relevance)
                    .then_with(|| a.id.cmp(&b.id))
                    .then_with(|| a.summary.cmp(&b.summary))
            });
        }
    }

    pub fn len(&self) -> usize {
        self.direct_knowledge.len()
            + self.rumor.len()
            + self.suspicion.len()
            + self.belief.len()
            + self.confirmed_fact.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A complete, bounded request for one provider decision.
#[derive(Debug, Clone)]
pub struct DecisionRequest {
    pub agent: u64,
    pub role: Role,
    pub now_secs: u64,
    pub candidates: Vec<ActionCandidate>,
    pub validation: ValidationContext,
    pub epistemic: EpistemicContext,
    /// Hash of the canonical provider context at request time (cache key).
    pub context_hash: u64,
    /// Hash of the validation slice used for pending-decision freshness.
    pub validation_hash: u64,
}

impl DecisionRequest {
    pub fn new(
        agent: u64,
        role: Role,
        now_secs: u64,
        candidates: Vec<ActionCandidate>,
        validation: ValidationContext,
        epistemic: EpistemicContext,
    ) -> Self {
        let validation_hash = hash_validation_context(&validation);
        let mut request = Self {
            agent,
            role,
            now_secs,
            candidates,
            validation,
            epistemic,
            context_hash: 0,
            validation_hash,
        };
        request.sort_candidates();
        request.context_hash = context_hash(&ProviderContext::from(&request));
        request
    }

    fn sort_candidates(&mut self) {
        self.candidates.sort_by(|a, b| {
            a.decision
                .action_name()
                .cmp(b.decision.action_name())
                .then_with(|| {
                    a.decision
                        .target_id()
                        .unwrap_or(u64::MAX)
                        .cmp(&b.decision.target_id().unwrap_or(u64::MAX))
                })
                .then_with(|| {
                    a.expected_utility
                        .total_cmp(&b.expected_utility)
                        .then_with(|| a.risk.total_cmp(&b.risk))
                })
        });
    }
}

/// Serializable render of the provider input. This is what is hashed and, in
/// a networked provider, what would be sent off-simulation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderContext {
    pub prompt: String,
    pub catalog: Vec<String>,
    pub candidate_count: usize,
    pub memory_count: usize,
}

impl From<&DecisionRequest> for ProviderContext {
    fn from(request: &DecisionRequest) -> Self {
        let mut text = String::new();
        let _ = writeln!(
            text,
            "agent={} role={:?} now={}",
            request.agent, request.role, request.now_secs
        );
        let _ = writeln!(
            text,
            "facility={} lockdown={} guards={} stamina_bits={:08x} gang={:?} leader={}",
            request.validation.current_facility,
            request.validation.lockdown,
            request.validation.guards_nearby,
            request.validation.stamina.to_bits(),
            request.validation.gang_id,
            request.validation.is_gang_leader
        );
        let mut visible = request.validation.visible_entities.clone();
        visible.sort_unstable();
        visible.dedup();
        let mut facilities = request.validation.known_facilities.clone();
        facilities.sort_unstable();
        facilities.dedup();
        let _ = writeln!(text, "visible={visible:?} facilities={facilities:?}");
        let _ = writeln!(
            text,
            "last={} contraband={}",
            request
                .validation
                .last_decision
                .as_ref()
                .map_or("none", |d| d.action_name()),
            request.validation.has_contraband
        );

        let mut candidates = request.candidates.clone();
        candidates.sort_by(|a, b| {
            a.decision
                .action_name()
                .cmp(b.decision.action_name())
                .then_with(|| {
                    a.decision
                        .target_id()
                        .unwrap_or(u64::MAX)
                        .cmp(&b.decision.target_id().unwrap_or(u64::MAX))
                })
                .then_with(|| a.expected_utility.total_cmp(&b.expected_utility))
        });
        let catalog: Vec<String> = candidates
            .iter()
            .map(|c| {
                format!(
                    "{}:{}:{:08x}",
                    c.decision.action_name(),
                    c.decision.target_id().unwrap_or(u64::MAX),
                    c.expected_utility.to_bits()
                )
            })
            .collect();
        let _ = writeln!(text, "candidates={}", catalog.join("|"));

        for (label, items) in [
            ("direct_knowledge", &request.epistemic.direct_knowledge),
            ("rumor", &request.epistemic.rumor),
            ("suspicion", &request.epistemic.suspicion),
            ("belief", &request.epistemic.belief),
            ("confirmed_fact", &request.epistemic.confirmed_fact),
        ] {
            let mut lines: Vec<String> = items
                .iter()
                .map(|m| {
                    format!(
                        "{}@{}:{}:{:08x}",
                        label,
                        m.id,
                        m.summary,
                        m.relevance.to_bits()
                    )
                })
                .collect();
            lines.sort();
            let _ = writeln!(text, "{label}={}", lines.join("|"));
        }
        Self {
            prompt: text,
            catalog,
            candidate_count: request.candidates.len(),
            memory_count: request.epistemic.len(),
        }
    }
}

/// FNV-1a hash over the canonical provider context. Deterministic across
/// processes and never uses wall-clock state or unordered containers.
pub fn context_hash(context: &ProviderContext) -> u64 {
    hash_bytes(context.prompt.as_bytes())
}

/// Hash the validation slice used for pending-decision freshness checks.
pub fn hash_validation_context(context: &ValidationContext) -> u64 {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "{}:{}:{:08x}:{}:{}:{:?}:{:?}:{}:{}:{}:{:?}",
        context.current_facility,
        context.lockdown,
        context.stamina.to_bits(),
        context.tokens_remaining_today,
        context.guards_nearby,
        context.agent_id,
        role_code(context.role),
        context.in_gang,
        context.gang_id.unwrap_or(u64::MAX),
        context.is_gang_leader,
        context.now_secs
    );
    let mut visible = context.visible_entities.clone();
    visible.sort_unstable();
    visible.dedup();
    let mut facilities = context.known_facilities.clone();
    facilities.sort_unstable();
    facilities.dedup();
    let _ = writeln!(
        text,
        "{visible:?}|{facilities:?}|{}",
        context.has_contraband
    );
    if let Some(last) = &context.last_decision {
        let _ = writeln!(
            text,
            "last:{}:{}:{:?}",
            last.action_name(),
            last.target_id().unwrap_or(u64::MAX),
            last.outcome
        );
    }
    hash_bytes(text.as_bytes())
}

fn role_code(role: Role) -> u8 {
    match role {
        Role::Prisoner => 0,
        Role::Guard => 1,
        Role::Warden => 2,
        Role::Unknown => 3,
    }
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::{Decision, DecisionAction};
    use crate::memory::EpistemicRecord;

    #[test]
    fn hashes_are_stable_and_order_independent() {
        let mut ctx = ValidationContext {
            visible_entities: vec![9, 3],
            known_facilities: vec![2, 1],
            ..Default::default()
        };
        let a = hash_validation_context(&ctx);
        ctx.visible_entities.reverse();
        ctx.known_facilities.reverse();
        assert_eq!(a, hash_validation_context(&ctx));
        ctx.now_secs += 1;
        assert_ne!(a, hash_validation_context(&ctx));
    }

    #[test]
    fn context_separates_epistemic_buckets() {
        let mut memory = Memory::new();
        memory.remember(EpistemicRecord::new(
            EpistemicKind::Rumor,
            4,
            "heard a guard was bribed",
        ));
        memory.remember(EpistemicRecord::new(
            EpistemicKind::DirectKnowledge,
            4,
            "saw a fight",
        ));
        let context = EpistemicContext::from_memory(&memory, Some(4), &[], 8, 0);
        assert_eq!(context.rumor.len(), 1);
        assert_eq!(context.direct_knowledge.len(), 1);
        assert!(context.confirmed_fact.is_empty());
    }

    #[test]
    fn request_hash_changes_with_candidates() {
        let context = ValidationContext::default();
        let epistemic = EpistemicContext::default();
        let a = DecisionRequest::new(
            1,
            Role::Prisoner,
            0,
            vec![ActionCandidate::new(
                Decision::new(DecisionAction::Comply),
                0.1,
            )],
            context.clone(),
            epistemic.clone(),
        );
        let b = DecisionRequest::new(
            1,
            Role::Prisoner,
            0,
            vec![ActionCandidate::new(
                Decision::new(DecisionAction::Refuse),
                0.1,
            )],
            context,
            epistemic,
        );
        assert_ne!(a.context_hash, b.context_hash);
    }
}
