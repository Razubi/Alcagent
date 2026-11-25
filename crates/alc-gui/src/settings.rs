//! Lightweight persistence for observer preferences (last-used view, speed and
//! save path). This is intentionally tiny: the simulation world itself is
//! persisted exclusively through `sumo::Sim::save` / `Sim::load`.

use alc_core::TimeSpeed;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Observer preferences persisted between launches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Last observation view that was open.
    pub view: alc_ui::ViewMode,
    /// Last playback speed.
    pub speed: TimeSpeed,
    /// Directory used by the Save / Load controls (relative paths allowed).
    pub save_dir: String,
    /// Autosave interval in simulated days (0 disables autosave).
    pub autosave_days: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            view: alc_ui::ViewMode::InstitutionOverview,
            speed: TimeSpeed::Paused,
            save_dir: "saves/gui".to_string(),
            autosave_days: 0,
        }
    }
}

impl Settings {
    /// Load settings from `path`, falling back to [`Settings::default`] on any
    /// error (a missing or corrupt preference file must never block startup).
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .unwrap_or_default()
    }

    /// Persist settings to `path`; failures are ignored (the observer remains
    /// fully functional without write access to its preference file).
    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let s = Settings {
            view: alc_ui::ViewMode::Map,
            speed: TimeSpeed::FastForward,
            save_dir: "tmp/save".to_string(),
            autosave_days: 7,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.view, alc_ui::ViewMode::Map);
        assert_eq!(back.speed, TimeSpeed::FastForward);
        assert_eq!(back.autosave_days, 7);
    }
}
