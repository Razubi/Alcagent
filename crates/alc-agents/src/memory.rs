//! Layered agent memory (Directive §18; design §5).
//!
//! Four layers are modelled: a sensory buffer, short-term memory, long-term
//! memory, and institutional memory. Memory is selective — importance scoring
//! decides promotion, decay/consolidation keep every store bounded, and
//! retrieval is ranked and budget-aware.
//!
//! Two APIs are provided:
//!
//! * [`Memory`] — the original per-agent store, extended with the fourth
//!   institutional layer plus scoring, decay, consolidation and recall helpers.
//! * [`LayeredMemory`] — the full design model with [`MemoryRecord`]s,
//!   configurable capacities/thresholds, and the [`MemoryStore`] trait.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Epistemic records (information model compatibility type)
// ---------------------------------------------------------------------------

/// Epistemic categories (Directive §3 information model).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EpistemicKind {
    DirectKnowledge,
    Rumor,
    Suspicion,
    Belief,
    ConfirmedFact,
}

impl EpistemicKind {
    /// Baseline importance conferred by the category alone.
    pub fn base_importance(self) -> f32 {
        match self {
            EpistemicKind::DirectKnowledge => 0.55,
            EpistemicKind::Rumor => 0.25,
            EpistemicKind::Suspicion => 0.30,
            EpistemicKind::Belief => 0.40,
            EpistemicKind::ConfirmedFact => 0.85,
        }
    }

    /// Per-second confidence decay. Confirmed facts barely decay; rumors fade
    /// fastest (design §4.3).
    pub fn decay_rate(self) -> f32 {
        match self {
            EpistemicKind::DirectKnowledge => 1.0e-7,
            EpistemicKind::Rumor => 6.0e-7,
            EpistemicKind::Suspicion => 3.0e-7,
            EpistemicKind::Belief => 1.0e-7,
            EpistemicKind::ConfirmedFact => 1.0e-9,
        }
    }
}

/// A piece of information an agent holds, with epistemic category and
/// provenance. Kept byte-compatible with the original record shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpistemicRecord {
    pub kind: EpistemicKind,
    pub about: u64, // entity id the record concerns
    pub content: String,
    pub confidence: f32, // 0..1
    pub salience: f32,
    pub source: Option<u64>, // who told the agent
    pub created_at_secs: u64,
}

impl EpistemicRecord {
    pub fn new(kind: EpistemicKind, about: u64, content: impl Into<String>) -> Self {
        Self {
            kind,
            about,
            content: content.into(),
            confidence: 0.5,
            salience: 0.5,
            source: None,
            created_at_secs: 0,
        }
    }

    pub fn with_time(mut self, created_at_secs: u64) -> Self {
        self.created_at_secs = created_at_secs;
        self
    }

    /// Importance estimate used for promotion, eviction and forgetting.
    ///
    /// `0.45 * category base + 0.35 * confidence + 0.20 * salience`, clamped.
    pub fn importance(&self) -> f32 {
        let base = self.kind.base_importance();
        let confidence = self.confidence.clamp(0.0, 1.0);
        let salience = self.salience.clamp(0.0, 1.0);
        (0.45 * base + 0.35 * confidence + 0.20 * salience).clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// Layers, kinds, and records
// ---------------------------------------------------------------------------

/// The four memory layers (design §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryLayer {
    Sensory,
    ShortTerm,
    LongTerm,
    Institutional,
}

impl MemoryLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryLayer::Sensory => "sensory",
            MemoryLayer::ShortTerm => "short_term",
            MemoryLayer::LongTerm => "long_term",
            MemoryLayer::Institutional => "institutional",
        }
    }
}

/// Content kind of a memory record (design §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryKind {
    Episodic,
    Semantic,
    Procedural,
    Social,
    Emotional,
}

impl MemoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryKind::Episodic => "episodic",
            MemoryKind::Semantic => "semantic",
            MemoryKind::Procedural => "procedural",
            MemoryKind::Social => "social",
            MemoryKind::Emotional => "emotional",
        }
    }
}

/// Full memory record used by [`LayeredMemory`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: u64,
    pub layer: MemoryLayer,
    pub kind: MemoryKind,
    pub summary: String,
    pub about: u64,
    pub tags: Vec<String>,
    /// 0..1 importance score (design §5.3).
    pub importance: f32,
    /// -1..1 emotional valence.
    pub emotional_valence: f32,
    /// 0..1 emotional arousal.
    pub emotional_intensity: f32,
    pub created_at_secs: u64,
    pub last_accessed_secs: u64,
    pub access_count: u32,
    pub rehearsal_count: u32,
    pub decay_rate: f32,
    pub participants: Vec<u64>,
    /// True once promoted into long-term memory.
    pub consolidated: bool,
}

impl MemoryRecord {
    pub fn new(
        layer: MemoryLayer,
        kind: MemoryKind,
        summary: impl Into<String>,
        about: u64,
    ) -> Self {
        Self {
            id: 0,
            layer,
            kind,
            summary: summary.into(),
            about,
            tags: Vec::new(),
            importance: 0.5,
            emotional_valence: 0.0,
            emotional_intensity: 0.0,
            created_at_secs: 0,
            last_accessed_secs: 0,
            access_count: 0,
            rehearsal_count: 0,
            decay_rate: 1.0e-7,
            participants: Vec::new(),
            consolidated: layer == MemoryLayer::LongTerm,
        }
    }

    pub fn age_secs(&self, now_secs: u64) -> u64 {
        now_secs.saturating_sub(self.created_at_secs)
    }

    /// Register a retrieval, strengthening the record against decay.
    pub fn touch(&mut self, now_secs: u64) {
        self.access_count = self.access_count.saturating_add(1);
        self.last_accessed_secs = now_secs;
        self.rehearsal_count = self.rehearsal_count.saturating_add(1);
    }

    /// Effective importance after decay and rehearsal (design §5.6):
    /// `importance * 0.5^(age / half_life) * (1 + rehearsal_bonus)`.
    pub fn effective_importance(&self, now_secs: u64, half_life_secs: u64) -> f32 {
        let age = self.age_secs(now_secs).max(1) as f32;
        let half_life = half_life_secs.max(1) as f32;
        let rehearsal = (1.0 + self.rehearsal_count as f32 * 0.05).min(2.0);
        (self.importance * 0.5f32.powf(age / half_life) * rehearsal).clamp(0.0, 1.0)
    }

    /// Retrieval ranking score (design §5.5). Goal relevance is approximated
    /// by importance because this crate has no goal store.
    pub fn retrieval_score(&self, query: &MemoryQuery, now_secs: u64, half_life_secs: u64) -> f32 {
        let semantic = if query.tags.is_empty() {
            1.0
        } else {
            let matches = query
                .tags
                .iter()
                .filter(|t| self.tags.iter().any(|rt| rt.eq_ignore_ascii_case(t)))
                .count();
            matches as f32 / query.tags.len() as f32
        };
        let recency = if now_secs <= self.created_at_secs {
            1.0
        } else {
            (-((now_secs - self.created_at_secs) as f32) / half_life_secs.max(1) as f32).exp()
        };
        let importance = self.importance.clamp(0.0, 1.0);
        let emotional = self.emotional_intensity.clamp(0.0, 1.0);
        let goal = importance;
        (0.35 * semantic + 0.25 * recency + 0.20 * importance + 0.10 * emotional + 0.10 * goal)
            .clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// Importance scoring (design §5.3)
// ---------------------------------------------------------------------------

/// Configurable importance weights. Defaults follow design §5.3.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ImportanceWeights {
    pub goal_relevance: f32,
    pub emotional_intensity: f32,
    pub social_significance: f32,
    pub consequence_magnitude: f32,
    pub novelty: f32,
    pub self_relevance: f32,
    pub repetition_bonus: f32,
}

impl Default for ImportanceWeights {
    fn default() -> Self {
        Self {
            goal_relevance: 0.30,
            emotional_intensity: 0.20,
            social_significance: 0.15,
            consequence_magnitude: 0.15,
            novelty: 0.10,
            self_relevance: 0.05,
            repetition_bonus: 0.05,
        }
    }
}

impl ImportanceWeights {
    /// Rescale weights so they sum to 1 (zero-sum weights are left alone).
    pub fn normalized(&self) -> Self {
        let sum = self.goal_relevance
            + self.emotional_intensity
            + self.social_significance
            + self.consequence_magnitude
            + self.novelty
            + self.self_relevance
            + self.repetition_bonus;
        if sum <= 0.0 {
            return *self;
        }
        Self {
            goal_relevance: self.goal_relevance / sum,
            emotional_intensity: self.emotional_intensity / sum,
            social_significance: self.social_significance / sum,
            consequence_magnitude: self.consequence_magnitude / sum,
            novelty: self.novelty / sum,
            self_relevance: self.self_relevance / sum,
            repetition_bonus: self.repetition_bonus / sum,
        }
    }
}

/// The seven normalised importance components (all 0..1).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ImportanceInput {
    pub goal_relevance: f32,
    pub emotional_intensity: f32,
    pub social_significance: f32,
    pub consequence_magnitude: f32,
    pub novelty: f32,
    pub self_relevance: f32,
    pub repetition_bonus: f32,
}

impl ImportanceInput {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Weighted importance score in `0..1`.
pub fn score_importance(input: &ImportanceInput, weights: &ImportanceWeights) -> f32 {
    let w = weights.normalized();
    let raw = w.goal_relevance * input.goal_relevance.clamp(0.0, 1.0)
        + w.emotional_intensity * input.emotional_intensity.clamp(0.0, 1.0)
        + w.social_significance * input.social_significance.clamp(0.0, 1.0)
        + w.consequence_magnitude * input.consequence_magnitude.clamp(0.0, 1.0)
        + w.novelty * input.novelty.clamp(0.0, 1.0)
        + w.self_relevance * input.self_relevance.clamp(0.0, 1.0)
        + w.repetition_bonus * input.repetition_bonus.clamp(0.0, 1.0);
    raw.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Configuration and queries
// ---------------------------------------------------------------------------

/// Per-agent memory configuration (design §5.8).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub short_term_slots: usize,
    pub long_term_max_records: usize,
    pub sensory_slots: usize,
    pub long_term_threshold: f32,
    pub short_term_threshold: f32,
    pub forget_threshold: f32,
    pub decay_half_life_secs: u64,
    pub retrieval_top_k: usize,
    /// Cap on the institutional layer. Defaulted for save compatibility
    /// (older saves lack the field and deserialize to the default).
    #[serde(default = "default_institutional_max_records")]
    pub institutional_max_records: usize,
}

const fn default_institutional_max_records() -> usize {
    200
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            short_term_slots: 9,
            long_term_max_records: 500,
            sensory_slots: 16,
            long_term_threshold: 0.55,
            short_term_threshold: 0.20,
            forget_threshold: 0.05,
            decay_half_life_secs: 7 * 86_400,
            retrieval_top_k: 8,
            institutional_max_records: default_institutional_max_records(),
        }
    }
}

/// Query used by ranked retrieval.
#[derive(Debug, Clone, Default)]
pub struct MemoryQuery {
    pub about: Option<u64>,
    pub tags: Vec<String>,
    pub since_secs: Option<u64>,
    pub min_importance: f32,
    /// Maximum records returned; `0` means unlimited.
    pub top_k: usize,
    /// Current simulation time for recency scoring.
    pub now_secs: u64,
}

impl MemoryQuery {
    pub fn new(about: u64) -> Self {
        Self {
            about: Some(about),
            top_k: 8,
            ..Default::default()
        }
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    pub fn with_min_importance(mut self, min: f32) -> Self {
        self.min_importance = min;
        self
    }
}

/// A memory record paired with its retrieval score.
#[derive(Debug, Clone)]
pub struct RankedMemory<'a> {
    pub record: &'a MemoryRecord,
    pub score: f32,
}

/// Ranked [`EpistemicRecord`] result used by [`Memory::recall`].
#[derive(Debug, Clone)]
pub struct RankedEpistemic<'a> {
    pub record: &'a EpistemicRecord,
    pub score: f32,
}

// ---------------------------------------------------------------------------
// Full layered store
// ---------------------------------------------------------------------------

/// Four-layer memory store with configurable capacities and thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayeredMemory {
    pub sensory: Vec<MemoryRecord>,
    pub short_term: Vec<MemoryRecord>,
    pub long_term: Vec<MemoryRecord>,
    pub institutional: Vec<MemoryRecord>,
    pub config: MemoryConfig,
    #[serde(default)]
    next_id: u64,
}

impl Default for LayeredMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl LayeredMemory {
    pub fn new() -> Self {
        Self::with_config(MemoryConfig::default())
    }

    pub fn with_config(config: MemoryConfig) -> Self {
        Self {
            sensory: Vec::new(),
            short_term: Vec::new(),
            long_term: Vec::new(),
            institutional: Vec::new(),
            config,
            next_id: 1,
        }
    }

    /// Store a record in its declared layer and return its assigned id.
    pub fn store(&mut self, mut record: MemoryRecord) -> u64 {
        let id = self.next_id;
        record.id = id;
        self.next_id = self.next_id.saturating_add(1);
        match record.layer {
            MemoryLayer::Sensory => self.sensory.push(record),
            MemoryLayer::ShortTerm => self.short_term.push(record),
            MemoryLayer::LongTerm => {
                record.consolidated = true;
                self.long_term.push(record);
            }
            MemoryLayer::Institutional => self.institutional.push(record),
        }
        id
    }

    /// Convenience constructor: importance decides short-term vs long-term.
    pub fn remember(&mut self, kind: MemoryKind, summary: impl Into<String>, about: u64) -> u64 {
        let mut record = MemoryRecord::new(MemoryLayer::ShortTerm, kind, summary, about);
        if record.importance >= self.config.long_term_threshold {
            record.layer = MemoryLayer::LongTerm;
        }
        self.store(record)
    }

    /// Relevant long-term then short-term records about an entity.
    pub fn recall_relevant(&self, about: u64) -> Vec<&MemoryRecord> {
        let mut out: Vec<&MemoryRecord> =
            self.long_term.iter().filter(|r| r.about == about).collect();
        out.extend(self.short_term.iter().filter(|r| r.about == about));
        out
    }

    /// Ranked retrieval across all layers (design §5.5).
    pub fn retrieve(&self, query: &MemoryQuery) -> Vec<RankedMemory<'_>> {
        let half_life = self.config.decay_half_life_secs;
        let mut ranked: Vec<RankedMemory<'_>> = self
            .sensory
            .iter()
            .chain(self.short_term.iter())
            .chain(self.long_term.iter())
            .chain(self.institutional.iter())
            .filter(|r| query.about.map_or(true, |about| r.about == about))
            .filter(|r| {
                query
                    .since_secs
                    .map_or(true, |since| r.created_at_secs >= since)
            })
            .filter(|r| r.importance >= query.min_importance)
            .filter(|r| {
                query.tags.is_empty()
                    || query
                        .tags
                        .iter()
                        .any(|t| r.tags.iter().any(|rt| rt.eq_ignore_ascii_case(t)))
            })
            .map(|r| RankedMemory {
                record: r,
                score: r.retrieval_score(query, query.now_secs, half_life),
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.record.id.cmp(&b.record.id))
        });
        if query.top_k > 0 {
            ranked.truncate(query.top_k);
        }
        ranked
    }

    /// Register a retrieval (strengthens the record).
    pub fn touch(&mut self, id: u64, now_secs: u64) {
        for layer in [
            &mut self.sensory,
            &mut self.short_term,
            &mut self.long_term,
            &mut self.institutional,
        ] {
            if let Some(record) = layer.iter_mut().find(|r| r.id == id) {
                record.touch(now_secs);
                return;
            }
        }
    }

    /// Consolidation pass (design §5.6): promote, merge, decay, forget, evict.
    pub fn consolidate(&mut self, now_secs: u64) {
        let cfg = self.config;

        // 1. Promote salient short-term records into long-term.
        let mut promoted = Vec::new();
        let mut retained = Vec::new();
        for mut record in self.short_term.drain(..) {
            if record.importance >= cfg.long_term_threshold || record.emotional_intensity >= 0.85 {
                record.layer = MemoryLayer::LongTerm;
                record.consolidated = true;
                promoted.push(record);
            } else {
                retained.push(record);
            }
        }
        self.short_term = retained;
        self.long_term.extend(promoted);

        // 2. Merge duplicates by (about, summary).
        merge_records(&mut self.sensory);
        merge_records(&mut self.short_term);
        merge_records(&mut self.long_term);

        // 3. Decay and forget.
        self.decay(now_secs);
        self.long_term.retain(|r| {
            r.effective_importance(now_secs, cfg.decay_half_life_secs) >= cfg.forget_threshold
        });
        self.short_term.retain(|r| {
            r.effective_importance(now_secs, cfg.decay_half_life_secs) >= cfg.forget_threshold
        });
        self.sensory.clear();

        // 4. Enforce capacity caps by evicting the lowest-value records.
        enforce_cap(
            &mut self.long_term,
            cfg.long_term_max_records,
            now_secs,
            cfg.decay_half_life_secs,
        );
        enforce_cap(
            &mut self.short_term,
            cfg.short_term_slots,
            now_secs,
            cfg.decay_half_life_secs,
        );
        // Institutional history Used to grow forever; every save got larger.
        enforce_cap(
            &mut self.institutional,
            cfg.institutional_max_records,
            now_secs,
            cfg.decay_half_life_secs,
        );
    }

    /// Apply decay in place.
    pub fn decay(&mut self, now_secs: u64) {
        let half_life = self.config.decay_half_life_secs;
        for record in self
            .sensory
            .iter_mut()
            .chain(self.short_term.iter_mut())
            .chain(self.long_term.iter_mut())
            .chain(self.institutional.iter_mut())
        {
            let age = record.age_secs(now_secs).max(1) as f32;
            let rehearsal = (1.0 + record.rehearsal_count as f32 * 0.05).min(2.0);
            record.importance =
                (record.importance * 0.5f32.powf(age / half_life.max(1) as f32) * rehearsal)
                    .clamp(0.0, 1.0);
        }
    }

    /// Remove every record matching the predicate, across all layers.
    pub fn forget<F: Fn(&MemoryRecord) -> bool>(&mut self, predicate: F) {
        self.sensory.retain(|r| !predicate(r));
        self.short_term.retain(|r| !predicate(r));
        self.long_term.retain(|r| !predicate(r));
        self.institutional.retain(|r| !predicate(r));
    }

    pub fn total_records(&self) -> usize {
        self.sensory.len() + self.short_term.len() + self.long_term.len() + self.institutional.len()
    }

    pub fn len(&self) -> usize {
        self.total_records()
    }

    pub fn is_empty(&self) -> bool {
        self.total_records() == 0
    }

    /// Bounded-memory invariant: no layer exceeds its configured capacity and
    /// the total stays under `max_records` (Directive §24).
    pub fn is_bounded(&self, max_records: usize) -> bool {
        self.sensory.len() <= self.config.sensory_slots
            && self.short_term.len() <= self.config.short_term_slots
            && self.long_term.len() <= self.config.long_term_max_records
            && self.institutional.len() <= self.config.institutional_max_records
            && self.total_records() <= max_records
    }
}

/// Merge duplicate records (same `about` and `summary`) inside one layer.
fn merge_records(records: &mut Vec<MemoryRecord>) {
    let mut write = 0;
    for read in 0..records.len() {
        let (about, summary) = (records[read].about, records[read].summary.clone());
        let mut merged = false;
        for idx in 0..write {
            if records[idx].about == about && records[idx].summary == summary {
                let source = records[read].clone();
                let target = &mut records[idx];
                target.importance = target.importance.max(source.importance);
                target.emotional_intensity =
                    target.emotional_intensity.max(source.emotional_intensity);
                target.rehearsal_count = target
                    .rehearsal_count
                    .saturating_add(1)
                    .max(source.rehearsal_count);
                target.last_accessed_secs =
                    target.last_accessed_secs.max(source.last_accessed_secs);
                merged = true;
                break;
            }
        }
        if !merged {
            records.swap(write, read);
            write += 1;
        }
    }
    records.truncate(write);
}

/// Evict lowest-value records until the layer fits `cap`.
fn enforce_cap(records: &mut Vec<MemoryRecord>, cap: usize, now_secs: u64, half_life: u64) {
    if records.len() <= cap {
        return;
    }
    // Sort *descending* by value so `truncate` drops the weakest memories.
    records.sort_by(|a, b| {
        b.effective_importance(now_secs, half_life)
            .total_cmp(&a.effective_importance(now_secs, half_life))
            .then(a.id.cmp(&b.id))
    });
    records.truncate(cap);
}

/// The [`MemoryStore`] trait from design §5.5.
pub trait MemoryStore {
    fn store(&mut self, record: MemoryRecord) -> u64;
    fn retrieve(&self, query: &MemoryQuery) -> Vec<RankedMemory<'_>>;
    fn consolidate(&mut self, now_secs: u64);
    fn decay(&mut self, now_secs: u64);
    fn forget<F: Fn(&MemoryRecord) -> bool>(&mut self, predicate: F);
}

impl MemoryStore for LayeredMemory {
    fn store(&mut self, record: MemoryRecord) -> u64 {
        LayeredMemory::store(self, record)
    }

    fn retrieve(&self, query: &MemoryQuery) -> Vec<RankedMemory<'_>> {
        LayeredMemory::retrieve(self, query)
    }

    fn consolidate(&mut self, now_secs: u64) {
        LayeredMemory::consolidate(self, now_secs);
    }

    fn decay(&mut self, now_secs: u64) {
        LayeredMemory::decay(self, now_secs);
    }

    fn forget<F: Fn(&MemoryRecord) -> bool>(&mut self, predicate: F) {
        LayeredMemory::forget(self, predicate);
    }
}

// ---------------------------------------------------------------------------
// Original per-agent memory, extended to four layers
// ---------------------------------------------------------------------------

/// Four-layer per-agent memory (Directive §18).
///
/// `sensory`, `short_term`, `long_term` and `institutional` mirror design
/// §5.1. The original `remember` / `recall_relevant` behaviour is preserved;
/// scoring, decay, consolidation and ranked recall are additive.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Memory {
    pub sensory: Vec<EpistemicRecord>,       // last tick
    pub short_term: Vec<EpistemicRecord>,    // recent interactions
    pub long_term: Vec<EpistemicRecord>,     // consolidated important events
    pub institutional: Vec<EpistemicRecord>, // shared, access-controlled history
}

impl Memory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Original routing behaviour, preserved: confirmed facts go straight to
    /// long-term, everything else is held in short-term.
    pub fn remember(&mut self, rec: EpistemicRecord) {
        match rec.kind {
            EpistemicKind::ConfirmedFact => self.long_term.push(rec),
            _ => self.short_term.push(rec),
        }
    }

    /// Importance-aware routing: high-importance events are written directly
    /// to long-term memory (design §5.3).
    pub fn remember_scored(&mut self, rec: EpistemicRecord) {
        let importance = rec.importance();
        let threshold = MemoryConfig::default().long_term_threshold;
        if importance >= threshold {
            self.long_term.push(rec);
        } else {
            self.remember(rec);
        }
    }

    /// Record an institutional-memory item explicitly.
    pub fn record_institutional(&mut self, rec: EpistemicRecord) {
        self.institutional.push(rec);
    }

    /// Relevant long-term then short-term records about an entity (original
    /// signature preserved).
    pub fn recall_relevant(&self, about: u64) -> Vec<&EpistemicRecord> {
        let mut out: Vec<&EpistemicRecord> =
            self.long_term.iter().filter(|r| r.about == about).collect();
        out.extend(self.short_term.iter().filter(|r| r.about == about));
        out
    }

    /// Ranked recall across all four layers.
    pub fn recall(&self, query: &MemoryQuery) -> Vec<RankedEpistemic<'_>> {
        let mut ranked: Vec<RankedEpistemic<'_>> = self
            .long_term
            .iter()
            .chain(self.short_term.iter())
            .chain(self.sensory.iter())
            .chain(self.institutional.iter())
            .filter(|r| query.about.map_or(true, |about| r.about == about))
            .filter(|r| {
                query
                    .since_secs
                    .map_or(true, |since| r.created_at_secs >= since)
            })
            .filter(|r| r.importance() >= query.min_importance)
            .map(|r| {
                let recency = if query.now_secs <= r.created_at_secs {
                    1.0
                } else {
                    (-((query.now_secs - r.created_at_secs) as f32)
                        / MemoryConfig::default().decay_half_life_secs.max(1) as f32)
                        .exp()
                };
                let score = (0.5 * r.importance() + 0.5 * recency).clamp(0.0, 1.0);
                RankedEpistemic { record: r, score }
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if query.top_k > 0 {
            ranked.truncate(query.top_k);
        }
        ranked
    }

    /// Importance estimate for a single record.
    pub fn importance_of(rec: &EpistemicRecord) -> f32 {
        rec.importance()
    }

    /// Apply per-kind confidence/salience decay.
    pub fn decay(&mut self, now_secs: u64) {
        for rec in self
            .sensory
            .iter_mut()
            .chain(self.short_term.iter_mut())
            .chain(self.long_term.iter_mut())
            .chain(self.institutional.iter_mut())
        {
            let dt = now_secs.saturating_sub(rec.created_at_secs) as f32;
            let factor = (1.0 - rec.kind.decay_rate() * dt).max(0.0);
            rec.confidence = (rec.confidence * factor).clamp(0.0, 1.0);
            rec.salience = (rec.salience * factor).clamp(0.0, 1.0);
        }
    }

    /// Consolidation: promote, merge, decay, forget, enforce caps.
    pub fn consolidate(&mut self, now_secs: u64) {
        let cfg = MemoryConfig::default();

        let mut promoted = Vec::new();
        let mut retained = Vec::new();
        for rec in self.short_term.drain(..) {
            if rec.importance() >= cfg.long_term_threshold
                || rec.kind == EpistemicKind::ConfirmedFact
            {
                promoted.push(rec);
            } else {
                retained.push(rec);
            }
        }
        self.short_term = retained;
        self.long_term.extend(promoted);

        merge_epistemic(&mut self.short_term);
        merge_epistemic(&mut self.long_term);
        merge_epistemic(&mut self.sensory);

        self.decay(now_secs);

        let threshold = cfg.forget_threshold;
        self.short_term.retain(|r| r.importance() >= threshold);
        self.long_term.retain(|r| r.importance() >= threshold);

        enforce_epistemic_cap(&mut self.long_term, cfg.long_term_max_records, now_secs);
        enforce_epistemic_cap(&mut self.short_term, cfg.short_term_slots, now_secs);
        enforce_epistemic_cap(&mut self.sensory, cfg.sensory_slots, now_secs);
        enforce_epistemic_cap(
            &mut self.institutional,
            cfg.institutional_max_records,
            now_secs,
        );
    }

    /// Remove records matching the predicate from every layer.
    pub fn forget<F: Fn(&EpistemicRecord) -> bool>(&mut self, predicate: F) {
        self.sensory.retain(|r| !predicate(r));
        self.short_term.retain(|r| !predicate(r));
        self.long_term.retain(|r| !predicate(r));
        self.institutional.retain(|r| !predicate(r));
    }

    pub fn total_records(&self) -> usize {
        self.sensory.len() + self.short_term.len() + self.long_term.len() + self.institutional.len()
    }

    pub fn len(&self) -> usize {
        self.total_records()
    }

    pub fn is_empty(&self) -> bool {
        self.total_records() == 0
    }

    /// Bounded-memory invariant helper (used by `alc-testing`).
    pub fn is_bounded(&self, max_records: usize) -> bool {
        let cfg = MemoryConfig::default();
        self.sensory.len() <= cfg.sensory_slots
            && self.short_term.len() <= cfg.short_term_slots
            && self.long_term.len() <= cfg.long_term_max_records
            && self.institutional.len() <= cfg.institutional_max_records
            && self.total_records() <= max_records
    }
}

fn merge_epistemic(records: &mut Vec<EpistemicRecord>) {
    let mut write = 0;
    for read in 0..records.len() {
        let (about, content) = (records[read].about, records[read].content.clone());
        let mut merged = false;
        for idx in 0..write {
            if records[idx].about == about && records[idx].content == content {
                let source = records[read].clone();
                let target = &mut records[idx];
                target.confidence = target.confidence.max(source.confidence);
                target.salience = target.salience.max(source.salience);
                target.kind = if source.kind == EpistemicKind::ConfirmedFact {
                    EpistemicKind::ConfirmedFact
                } else {
                    target.kind
                };
                merged = true;
                break;
            }
        }
        if !merged {
            records.swap(write, read);
            write += 1;
        }
    }
    records.truncate(write);
}

fn enforce_epistemic_cap(records: &mut Vec<EpistemicRecord>, cap: usize, now_secs: u64) {
    if records.len() <= cap {
        return;
    }
    // Descending by value: `truncate` must discard the weakest records, not
    // the strongest (the previous ascending sort did the opposite).
    records.sort_by(|a, b| {
        let age_a = now_secs.saturating_sub(a.created_at_secs).max(1) as f32;
        let age_b = now_secs.saturating_sub(b.created_at_secs).max(1) as f32;
        let score_a = a.importance() * 0.5f32.powf(age_a / 604_800.0);
        let score_b = b.importance() * 0.5f32.powf(age_b / 604_800.0);
        score_b
            .total_cmp(&score_a)
            .then(b.created_at_secs.cmp(&a.created_at_secs))
    });
    records.truncate(cap);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_routes_confirmed_facts_to_long_term() {
        let mut memory = Memory::default();
        memory.remember(EpistemicRecord::new(
            EpistemicKind::DirectKnowledge,
            1,
            "saw a fight",
        ));
        memory.remember(EpistemicRecord::new(
            EpistemicKind::ConfirmedFact,
            1,
            "lockdown announced",
        ));
        assert_eq!(memory.short_term.len(), 1);
        assert_eq!(memory.long_term.len(), 1);
        assert_eq!(memory.recall_relevant(1).len(), 2);
    }

    #[test]
    fn importance_scoring_uses_weights() {
        let input = ImportanceInput {
            goal_relevance: 1.0,
            emotional_intensity: 1.0,
            social_significance: 0.5,
            consequence_magnitude: 0.5,
            novelty: 0.0,
            self_relevance: 1.0,
            repetition_bonus: 0.0,
        };
        let score = score_importance(&input, &ImportanceWeights::default());
        assert!((0.0..=1.0).contains(&score));
        assert!(score > 0.5);
    }

    #[test]
    fn consolidation_promotes_and_bounds() {
        let cfg = MemoryConfig {
            short_term_slots: 2,
            long_term_max_records: 3,
            ..Default::default()
        };
        let mut memory = LayeredMemory::with_config(cfg);
        for i in 0..10 {
            let mut record = MemoryRecord::new(
                MemoryLayer::ShortTerm,
                MemoryKind::Episodic,
                format!("event {i}"),
                i,
            );
            record.importance = if i < 5 { 0.9 } else { 0.1 };
            record.created_at_secs = 1_000;
            memory.store(record);
        }
        memory.consolidate(2_000);
        assert!(memory.short_term.len() <= cfg.short_term_slots);
        assert!(memory.long_term.len() <= cfg.long_term_max_records);
        assert!(memory.is_bounded(cfg.long_term_max_records + cfg.short_term_slots));
    }

    #[test]
    fn retrieval_ranks_by_importance_and_recency() {
        let mut memory = LayeredMemory::new();
        let mut old = MemoryRecord::new(MemoryLayer::LongTerm, MemoryKind::Episodic, "old", 5);
        old.importance = 0.9;
        old.created_at_secs = 0;
        memory.store(old);
        let mut fresh = MemoryRecord::new(MemoryLayer::ShortTerm, MemoryKind::Episodic, "fresh", 5);
        fresh.importance = 0.4;
        fresh.created_at_secs = 10_000;
        memory.store(fresh);

        let query = MemoryQuery {
            about: Some(5),
            top_k: 2,
            now_secs: 10_000,
            ..Default::default()
        };
        let ranked = memory.retrieve(&query);
        assert_eq!(ranked.len(), 2);
        assert!(ranked[0].score >= ranked[1].score);
    }

    #[test]
    fn institutional_memory_is_bounded_across_many_consolidations() {
        let cfg = MemoryConfig {
            institutional_max_records: 5,
            ..Default::default()
        };
        let mut layered = LayeredMemory::with_config(cfg);
        for day in 0..20u64 {
            for i in 0..10 {
                let mut r = MemoryRecord::new(
                    MemoryLayer::Institutional,
                    MemoryKind::Semantic,
                    format!("institution event {day}-{i}"),
                    i,
                );
                r.importance = 0.8;
                r.created_at_secs = day * 86_400;
                layered.store(r);
            }
            layered.consolidate(day * 86_400);
        }
        assert!(
            layered.institutional.len() <= cfg.institutional_max_records,
            "institutional layer grew to {}",
            layered.institutional.len()
        );
        assert!(layered.is_bounded(cfg.institutional_max_records + cfg.short_term_slots));

        let mut plain = Memory::new();
        for i in 0..500u64 {
            let mut r = EpistemicRecord::new(
                EpistemicKind::ConfirmedFact,
                i,
                format!("institution history {i}"),
            );
            r.confidence = 0.9;
            r.salience = 0.9;
            plain.record_institutional(r);
        }
        plain.consolidate(86_400);
        assert!(
            plain.institutional.len() <= MemoryConfig::default().institutional_max_records,
            "institutional memory grew to {}",
            plain.institutional.len()
        );
        assert!(plain.is_bounded(600));
    }

    #[test]
    fn capacity_eviction_keeps_the_most_important_memories() {
        let cfg = MemoryConfig {
            short_term_slots: 2,
            long_term_max_records: 2,
            institutional_max_records: 2,
            ..Default::default()
        };
        let mut layered = LayeredMemory::with_config(cfg);
        for (i, importance) in [0.1, 0.9, 0.5, 0.8].into_iter().enumerate() {
            let mut r = MemoryRecord::new(
                MemoryLayer::LongTerm,
                MemoryKind::Episodic,
                format!("event-{i}"),
                i as u64,
            );
            r.importance = importance;
            r.created_at_secs = 1_000;
            layered.store(r);
        }
        layered.consolidate(1_000);
        let kept: Vec<&str> = layered
            .long_term
            .iter()
            .map(|r| r.summary.as_str())
            .collect();
        assert_eq!(kept.len(), 2);
        assert!(
            kept.contains(&"event-1") && kept.contains(&"event-3"),
            "eviction discarded the strongest memories: {kept:?}"
        );
        assert!(
            !kept.contains(&"event-0"),
            "weakest memory survived: {kept:?}"
        );

        let mut plain = Memory::new();
        for i in 0..10u64 {
            let mut r = EpistemicRecord::new(
                EpistemicKind::DirectKnowledge,
                i,
                format!("observation-{i}"),
            );
            r.confidence = 0.01 * i as f32;
            r.salience = 0.01 * i as f32;
            r.created_at_secs = 1_000;
            plain.remember(r);
        }
        plain.consolidate(1_000);
        assert_eq!(
            plain.short_term.len(),
            MemoryConfig::default().short_term_slots
        );
        assert!(
            !plain
                .short_term
                .iter()
                .any(|r| r.content == "observation-0"),
            "weakest epistemic record survived eviction"
        );
    }

    #[test]
    fn memory_config_deserializes_without_institutional_cap() {
        // Old saves predate `institutional_max_records`; serde default keeps them loadable.
        let json = r#"{"short_term_slots":9,"long_term_max_records":500,"sensory_slots":16,"long_term_threshold":0.55,"short_term_threshold":0.2,"forget_threshold":0.05,"decay_half_life_secs":604800,"retrieval_top_k":8}"#;
        let cfg: MemoryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.institutional_max_records, 200);
    }
}
