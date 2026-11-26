//! Lightweight persistence for observer preferences (startup defaults, view,
//! theme, map layers and save root). This is intentionally separate from world
//! state: the simulation world itself is persisted exclusively through
//! `sumo::Sim::save` / `sumo::Sim::load`.
//!
//! Every field is `#[serde(default)]`-ed at the struct level, so preference
//! files written by earlier builds (which only had view/speed/save_dir and
//! autosave_days) keep deserializing without migration. `ticks_per_second`
//! additionally carries a field-level default so older files get the 60 Hz
//! RealTime baseline.

use alc_core::TimeSpeed;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use sumo::{ThemeKind, ViewMode};

/// Spacing preset for the observer widgets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiDensity {
    /// Tighter spacing for dense institutional dashboards.
    Compact,
    /// Roomy spacing, the default.
    #[default]
    Comfortable,
}

impl UiDensity {
    /// Stable label for the settings screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
        }
    }

    /// All selectable densities.
    #[must_use]
    pub const fn all() -> [Self; 2] {
        [Self::Compact, Self::Comfortable]
    }
}

/// Baseline RealTime rate used when a preference file omits `ticks_per_second`.
const fn default_ticks_per_second() -> u32 {
    60
}

/// Observer preferences persisted between launches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Observation view selected by default when a world opens.
    pub view: ViewMode,
    /// Playback speed selected by default when a world opens.
    pub speed: TimeSpeed,
    /// Absolute RealTime rate in simulation ticks per real second (1..=4000).
    ///
    /// This is the authoritative playback rate: the observer scales every
    /// non-paused speed relative to the 60 Hz baseline. Older preference files
    /// without this field deserialize to [`default_ticks_per_second`].
    #[serde(default = "default_ticks_per_second")]
    pub ticks_per_second: u32,
    /// Root directory used by the Save / Load controls and the save browser.
    pub save_dir: String,
    /// Autosave interval in simulated days (0 disables autosave).
    pub autosave_days: u64,
    /// Seed offered by the New World panel on startup.
    pub seed: u64,
    /// Visual theme applied to the whole observer.
    pub theme: ThemeKind,
    /// Draw the shared institutional map regions beneath facilities.
    pub map_show_zones: bool,
    /// Draw the shared schematic road/nav network.
    pub map_show_edges: bool,
    /// Draw facility footprints and glyphs.
    pub map_show_facilities: bool,
    /// Draw prisoner and guard pawns.
    pub map_show_pawns: bool,
    /// Draw incident badges on the map.
    pub map_show_incidents: bool,
    /// Draw map labels (region names, facility names and pawn names).
    pub map_show_labels: bool,
    /// Widget spacing preset.
    pub ui_density: UiDensity,
    /// Show on-screen usage hints (including optional mouse map controls).
    pub show_help_hints: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            view: ViewMode::InstitutionOverview,
            speed: TimeSpeed::Paused,
            ticks_per_second: default_ticks_per_second(),
            save_dir: "saves/gui".to_string(),
            autosave_days: 0,
            seed: crate::DEFAULT_SEED,
            theme: ThemeKind::Slate,
            map_show_zones: true,
            map_show_edges: true,
            map_show_facilities: true,
            map_show_pawns: true,
            map_show_incidents: true,
            map_show_labels: true,
            ui_density: UiDensity::Comfortable,
            show_help_hints: true,
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

    /// Save root as a path (relative paths are resolved by the caller).
    #[must_use]
    pub fn save_root(&self) -> PathBuf {
        if self.save_dir.trim().is_empty() {
            PathBuf::from("saves/gui")
        } else {
            PathBuf::from(&self.save_dir)
        }
    }

    /// Persist settings to `path` through a temporary file and a backup so a
    /// crash or a full disk cannot leave a half-written preference file behind.
    ///
    /// The existing file is moved to a `.bak` sibling (never deleted first),
    /// the freshly written temp file is renamed into place, and only then is
    /// the backup removed. If any step fails, the backup is restored. No new
    /// dependency is needed: plain `std::fs` is sufficient.
    ///
    /// Failures are reported to the caller but never panic; the observer stays
    /// functional without write access to its preference file.
    pub fn save_atomic(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension("json.tmp");
        let backup = path.with_extension("json.bak");

        // Clear leftovers from a previously interrupted save so the rename
        // steps below cannot fail because a stale temp/backup already exists.
        if tmp.exists() {
            std::fs::remove_file(&tmp)?;
        }
        if backup.exists() {
            std::fs::remove_file(&backup)?;
        }

        std::fs::write(&tmp, json)?;
        let had_original = path.exists();
        if had_original {
            if let Err(err) = std::fs::rename(path, &backup) {
                let _ = std::fs::remove_file(&tmp);
                return Err(err);
            }
        }

        match std::fs::rename(&tmp, path) {
            Ok(()) => {
                if had_original {
                    // New file is committed; the previous version is expendable.
                    let _ = std::fs::remove_file(&backup);
                }
                Ok(())
            }
            Err(err) => {
                if had_original && backup.exists() {
                    // Restore the previous preference file verbatim.
                    let _ = std::fs::remove_file(path);
                    let _ = std::fs::rename(&backup, path);
                }
                let _ = std::fs::remove_file(&tmp);
                Err(err)
            }
        }
    }

    /// Persist settings, ignoring failures (preferences are best-effort).
    pub fn save(&self, path: &Path) {
        let _ = self.save_atomic(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_match_the_shared_display_models() {
        let s = Settings::default();
        assert_eq!(s.view, ViewMode::InstitutionOverview);
        assert_eq!(s.speed, TimeSpeed::Paused);
        assert_eq!(s.ticks_per_second, 60);
        assert_eq!(s.save_dir, "saves/gui");
        assert_eq!(s.autosave_days, 0);
        assert_eq!(s.seed, crate::DEFAULT_SEED);
        assert_eq!(s.theme, ThemeKind::Slate);
        assert!(s.map_show_zones && s.map_show_edges && s.map_show_pawns);
        assert!(s.show_help_hints);
    }

    #[test]
    fn settings_round_trip() {
        let s = Settings {
            view: ViewMode::Map,
            speed: TimeSpeed::FastForward,
            ticks_per_second: 120,
            save_dir: "tmp/save".to_string(),
            autosave_days: 7,
            seed: 42,
            theme: ThemeKind::Midnight,
            map_show_zones: false,
            map_show_edges: false,
            map_show_facilities: true,
            map_show_pawns: false,
            map_show_incidents: true,
            map_show_labels: false,
            ui_density: UiDensity::Compact,
            show_help_hints: false,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn legacy_settings_json_deserializes_with_defaults() {
        let legacy =
            r#"{"view":"Map","speed":"FastForward","save_dir":"old/saves","autosave_days":3}"#;
        let s: Settings = serde_json::from_str(legacy).unwrap();
        assert_eq!(s.view, ViewMode::Map);
        assert_eq!(s.speed, TimeSpeed::FastForward);
        assert_eq!(s.ticks_per_second, 60);
        assert_eq!(s.save_dir, "old/saves");
        assert_eq!(s.autosave_days, 3);
        assert_eq!(s.theme, ThemeKind::Slate);
        assert_eq!(s.seed, crate::DEFAULT_SEED);
        assert!(s.map_show_zones);
        assert_eq!(s.ui_density, UiDensity::Comfortable);
    }

    #[test]
    fn explicit_ticks_per_second_in_legacy_json_is_preserved() {
        let json = r#"{"speed":"RealTime","ticks_per_second":4000}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.ticks_per_second, 4000);
        assert_eq!(s.speed, TimeSpeed::RealTime);
        assert_eq!(s.autosave_days, 0);
    }

    #[test]
    fn atomic_save_round_trips_and_overwrites() {
        let dir = std::env::temp_dir().join(format!("alc-gui-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let _ = std::fs::remove_dir_all(&dir);
        let first = Settings {
            save_dir: "saves/one".into(),
            theme: ThemeKind::Paper,
            ..Settings::default()
        };
        first.save_atomic(&path).unwrap();
        assert_eq!(Settings::load(&path), first);
        let second = Settings {
            save_dir: "saves/two".into(),
            theme: ThemeKind::Midnight,
            ..Settings::default()
        };
        second.save_atomic(&path).unwrap();
        assert_eq!(Settings::load(&path), second);
        assert!(!dir.join("settings.json.tmp").exists());
        assert!(!dir.join("settings.json.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
