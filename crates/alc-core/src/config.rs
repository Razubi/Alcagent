use serde::{Deserialize, Serialize};

/// Top-level runtime configuration (loaded from file at startup).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub seed: u64,
    pub starting_population: u32,
    pub save_interval_days: u64,
    pub autosave_dir: String,
    pub ai: AiConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            seed: 0x00A1_C0A6_E170,
            starting_population: 24,
            save_interval_days: 7,
            autosave_dir: "saves".into(),
            ai: AiConfig::default(),
        }
    }
}

/// AI orchestration knobs (Directive section 19).
///
/// The original four fields are kept in place; every later field has a serde
/// default so configuration files written before spec 011 stay loadable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub model: String,
    pub decision_interval_mins: u64,
    pub max_tokens_per_agent_per_day: u32,
    pub cache_enabled: bool,

    /// Provider id. Unknown providers degrade to deterministic decisions.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Global token ceiling per simulated day.
    #[serde(default = "default_max_tokens_per_day")]
    pub max_tokens_per_day: u64,
    /// Sustained request refill rate (requests per simulated second).
    #[serde(default = "default_rate_limit_per_sec")]
    pub rate_limit_per_sec: u32,
    /// Initial/burst request allowance.
    #[serde(default = "default_rate_burst")]
    pub rate_burst: u32,
    /// Extra attempts after a provider failure.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Cache lifetime in simulated seconds (never wall-clock time).
    #[serde(default = "default_cache_ttl_secs")]
    pub cache_ttl_secs: u64,
    /// Maximum decisions accepted in one batch.
    #[serde(default = "default_max_batch_size")]
    pub max_batch_size: usize,
    /// Maximum age of a pending AI decision in simulated seconds.
    #[serde(default = "default_decision_timeout_secs")]
    pub decision_timeout_secs: u64,
    /// Hard cap on cached provider responses.
    #[serde(default = "default_max_cache_entries")]
    pub max_cache_entries: usize,
    /// Hard cap on memory records injected into one provider context.
    #[serde(default = "default_max_context_memories")]
    pub max_context_memories: usize,
    /// Maximum simulation operations per tick. Excess work is deferred to the
    /// next tick so a large population cannot stall playback (Spec 003).
    #[serde(default = "default_max_ops_per_tick")]
    pub max_ops_per_tick: usize,
}

fn default_provider() -> String {
    "local-deterministic".to_string()
}
const fn default_max_tokens_per_day() -> u64 {
    256_000
}
const fn default_rate_limit_per_sec() -> u32 {
    1
}
const fn default_rate_burst() -> u32 {
    200
}
const fn default_max_retries() -> u32 {
    2
}
const fn default_cache_ttl_secs() -> u64 {
    3_600
}
const fn default_max_batch_size() -> usize {
    16
}
const fn default_decision_timeout_secs() -> u64 {
    120
}
const fn default_max_cache_entries() -> usize {
    256
}
const fn default_max_context_memories() -> usize {
    8
}
const fn default_max_ops_per_tick() -> usize {
    10_000
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            model: "local-deterministic".into(),
            decision_interval_mins: 60,
            max_tokens_per_agent_per_day: 4_000,
            cache_enabled: true,
            provider: default_provider(),
            max_tokens_per_day: default_max_tokens_per_day(),
            rate_limit_per_sec: default_rate_limit_per_sec(),
            rate_burst: default_rate_burst(),
            max_retries: default_max_retries(),
            cache_ttl_secs: default_cache_ttl_secs(),
            max_batch_size: default_max_batch_size(),
            decision_timeout_secs: default_decision_timeout_secs(),
            max_cache_entries: default_max_cache_entries(),
            max_context_memories: default_max_context_memories(),
            max_ops_per_tick: default_max_ops_per_tick(),
        }
    }
}
