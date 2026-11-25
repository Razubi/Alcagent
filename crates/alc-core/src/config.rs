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
            seed: 0xA1C_0A_6E_17_0,
            starting_population: 24,
            save_interval_days: 7,
            autosave_dir: "saves".into(),
            ai: AiConfig::default(),
        }
    }
}

/// AI orchestration knobs (Directive section 19).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub model: String,
    pub decision_interval_mins: u64,
    pub max_tokens_per_agent_per_day: u32,
    pub cache_enabled: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            model: "local-deterministic".into(),
            decision_interval_mins: 60,
            max_tokens_per_agent_per_day: 4_000,
            cache_enabled: true,
        }
    }
}
