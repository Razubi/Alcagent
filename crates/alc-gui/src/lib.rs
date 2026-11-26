//! `alc-gui` — the interactive egui/eframe observer for the Alcagent prison
//! simulation (Directive §20).
//!
//! This crate is a library-only egui adapter for the `sumo` engine. The native
//! binary lives in `alcagent`; this crate exposes [`launch`] and the observer
//! [`App`]. The window opens at a button-driven main menu, and every world
//! mutation still passes through validated `sumo::Sim` controls.
//!
//! ## Module responsibilities
//!
//! - [`shell`] — the application shell: MainMenu / LoadWorld / Settings /
//!   Simulation screens, startup load and viewport quit.
//! - [`app`] — the simulation observer. Owns the [`sumo::Sim`], playback clock,
//!   view/selection state and the queue of validated control commands.
//! - [`data`] — pure, testable extraction over read-only engine snapshots into
//!   owned DTOs ([`data::Snapshot`]). Also holds map projection helpers.
//! - [`map_view`] — the institutional world map built from shared
//!   [`sumo::graphics`] geometry: regions, roads, footprints, pawns and badges.
//! - [`screens`] — one `show` function per observation view; rendering only.
//! - [`panels`] — shared panels (slim toolbar, controls, detail views).
//! - [`theme`] — shared-palette colour conversion and small widget helpers.
//! - [`settings`] — persistence of observer preferences (never world state).
//!
//! ## Authority
//!
//! The observer never mutates arbitrary world state. UI handlers enqueue a
//! [`app::Command`], which the App applies through validated `sumo::Sim`
//! methods (`tick`, `set_speed`, `apply_warden_action`, policy setters,
//! construction/rehab/contraband/parole controls). Rendering receives only
//! read-only snapshots from [`data::capture`].

pub mod app;
pub mod data;
pub mod map_view;
pub mod panels;
pub mod screens;
pub mod settings;
pub mod shell;
pub mod theme;

/// Default world-generation seed (the one offered by `alcagent`).
pub const DEFAULT_SEED: u64 = 0xA1C0_A6E1_70;

/// Shared display models re-exported so frontends do not duplicate them.
pub use sumo::{
    CONSTRUCTION_KINDS, Color, MapBounds, MapCamera, MapZone, MapZoneKind, Palette, ThemeKind,
    ViewMode, WorldMapLayout,
};

pub use app::App;
/// Observer preference types re-exported so the CLI can resolve unspecified
/// launch flags from persisted settings before constructing [`GuiOptions`].
pub use settings::{Settings, UiDensity};
pub use shell::Shell;

/// Everything the native observer needs to open a window.
///
/// World creation and loading are still performed by the shell: `load` is
/// attempted before the first frame and only swaps into the session on success.
#[derive(Debug, Clone)]
pub struct GuiOptions {
    /// Seed used by the New World panel and by a fresh session.
    pub seed: u64,
    /// Save directory to open directly at startup, when present.
    pub load: Option<std::path::PathBuf>,
    /// Root directory of the save browser and manual saves.
    pub save_dir: std::path::PathBuf,
    /// Path of the observer preference JSON (separate from world saves).
    pub settings_path: std::path::PathBuf,
    /// Absolute RealTime rate in simulation ticks per real second (1..=4000).
    /// The CLI resolves this from persisted [`Settings`] when the flag is omitted.
    pub ticks_per_second: u32,
    /// Autosave interval in simulated days (0 disables autosave).
    pub autosave_days: u64,
}

impl Default for GuiOptions {
    fn default() -> Self {
        Self {
            seed: DEFAULT_SEED,
            load: None,
            save_dir: std::path::PathBuf::from("saves/gui"),
            settings_path: std::path::PathBuf::from("saves/alc-gui/settings.json"),
            ticks_per_second: 60,
            autosave_days: 0,
        }
    }
}

/// Launch the native observer window. Startup is the button-driven main menu.
///
/// # Errors
/// Returns `eframe` errors if a graphics context or window cannot be created.
pub fn launch(options: GuiOptions) -> eframe::Result<()> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Alcagent — Living Prison Observer"),
        ..Default::default()
    };
    eframe::run_native(
        "Alcagent — Living Prison Observer",
        native,
        Box::new(move |_cc| Ok(Box::new(Shell::new(options)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_options_have_stable_defaults() {
        let o = GuiOptions::default();
        assert_eq!(o.seed, DEFAULT_SEED);
        assert!(o.load.is_none());
        assert_eq!(o.save_dir, std::path::PathBuf::from("saves/gui"));
        assert_eq!(
            o.settings_path,
            std::path::PathBuf::from("saves/alc-gui/settings.json")
        );
        assert_eq!(o.ticks_per_second, 60);
        assert_eq!(o.autosave_days, 0);
    }

    #[test]
    fn observer_settings_types_are_public_to_callers() {
        let settings = Settings::default();
        assert_eq!(settings.ticks_per_second, 60);
        assert_eq!(UiDensity::default(), UiDensity::Comfortable);
        let _ = settings.save_root();
    }

    #[test]
    fn construction_kinds_match_shared_engine_order() {
        assert_eq!(CONSTRUCTION_KINDS, sumo::CONSTRUCTION_KINDS);
        assert_eq!(CONSTRUCTION_KINDS.len(), 12);
    }

    #[test]
    fn shared_view_models_cover_eleven_observation_views() {
        assert_eq!(ViewMode::all().len(), 11);
        assert_eq!(ViewMode::Map.label(), "Map");
        assert_eq!(ViewMode::Map.title(), "World Map");
        assert_eq!(ViewMode::default(), ViewMode::InstitutionOverview);
    }
}
