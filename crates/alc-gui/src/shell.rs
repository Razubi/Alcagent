//! The egui application shell: a button-driven main menu, save browser,
//! settings screen and the simulation observer.
//!
//! The shell owns no simulation state directly. It only constructs or loads a
//! validated [`sumo::Sim`] and hands it to [`App`]; every in-world control still
//! flows through `App`'s command boundary. Screens are plain state transitions
//! so they can be tested without a window.

use crate::{
    GuiOptions, ThemeKind, ViewMode,
    app::App,
    settings::{Settings, UiDensity},
    theme,
};
use alc_core::{TimeSpeed, config::AiConfig};
use alc_persistence::{SaveProbe, list_saves, probe_save_dir};
use eframe::{App as EframeApp, egui};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Where a transient screen returns when it is cancelled or applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Opened from the front menu.
    MainMenu,
    /// Opened from the running observer.
    Simulation,
}

/// Top-level screen of the observer window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// The first screen shown at startup.
    MainMenu,
    /// Save browser; `origin` decides where Cancel/load-failure returns.
    LoadWorld(Origin),
    /// Observer preferences; `origin` decides where Apply/Cancel returns.
    Settings(Origin),
    /// The live eleven-view simulation observer.
    Simulation,
}

/// eframe app implementation for the whole window.
pub struct Shell {
    settings: Settings,
    settings_path: PathBuf,
    screen: Screen,
    sim: Option<App>,
    /// Seed input on the New World panel (not the Settings draft).
    seed_input: String,
    /// Seed input on the Settings screen; applied to `draft.seed` on Apply.
    settings_seed_input: String,
    load_root_input: String,
    saves: Vec<SaveProbe>,
    selected_save: Option<usize>,
    load_error: Option<String>,
    menu_error: Option<String>,
    settings_error: Option<String>,
    startup_error: Option<String>,
    draft: Settings,
    random_counter: u64,
    /// Monotonic component of manual save names (avoids second-resolution
    /// collisions when several saves happen within the same nanosecond tick).
    save_counter: u64,
    applied_theme: Option<(ThemeKind, UiDensity)>,
}

impl Shell {
    /// Create the shell. A startup `load` is attempted before the first frame;
    /// failure leaves the main menu visible with the reason on screen.
    #[must_use]
    pub fn new(options: GuiOptions) -> Self {
        let settings_path = options.settings_path.clone();
        let mut settings = Settings::load(&settings_path);
        // The CLI caller resolves unspecified flags from persisted settings
        // before constructing `GuiOptions`, so every value here is explicit.
        // In particular `autosave_days == 0` is a deliberate disable, not a
        // "keep the persisted preference" sentinel.
        settings.seed = options.seed;
        settings.save_dir = options.save_dir.to_string_lossy().into_owned();
        settings.ticks_per_second = options.ticks_per_second;
        settings.autosave_days = options.autosave_days;
        let seed_input = settings.seed.to_string();
        let settings_seed_input = settings.seed.to_string();
        let load_root_input = settings.save_dir.clone();
        let mut shell = Self {
            settings,
            settings_path,
            screen: Screen::MainMenu,
            sim: None,
            seed_input,
            settings_seed_input,
            load_root_input,
            saves: Vec::new(),
            selected_save: None,
            load_error: None,
            menu_error: None,
            settings_error: None,
            startup_error: None,
            draft: Settings::default(),
            random_counter: 0,
            save_counter: 0,
            applied_theme: None,
        };
        shell.draft = shell.settings.clone();
        shell.refresh_saves();

        if let Some(dir) = options.load.clone() {
            match Self::load_app(&dir, &shell.settings, &shell.settings_path) {
                Ok(app) => {
                    shell.sim = Some(app);
                    shell.screen = Screen::Simulation;
                }
                Err(err) => {
                    shell.startup_error = Some(format!(
                        "Startup load error: could not load {}: {err}",
                        dir.display()
                    ));
                    shell.screen = Screen::MainMenu;
                }
            }
        }
        shell
    }

    /// Current screen (testable state transition surface).
    #[must_use]
    pub fn screen(&self) -> Screen {
        self.screen
    }

    fn load_app(
        dir: &std::path::Path,
        settings: &Settings,
        settings_path: &std::path::Path,
    ) -> Result<App, String> {
        let sim = sumo::Sim::load_with_config(dir, &AiConfig::default())
            .map_err(|e| format!("load failed: {e}"))?;
        let mut app = App::configured(sim, settings.clone(), settings_path);
        app.state.current_save = Some(dir.to_path_buf());
        app.state.save_input = dir.to_string_lossy().into_owned();
        app.state.load_input = dir.to_string_lossy().into_owned();
        app.state.status = Some(format!("Loaded world from {}", dir.display()));
        Ok(app)
    }

    fn refresh_saves(&mut self) {
        self.saves = list_saves(&self.load_root_input);
        self.selected_save = if self.saves.is_empty() { None } else { Some(0) };
    }

    fn parse_seed(input: &str) -> Option<u64> {
        let value = input.trim();
        if let Some(hex) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            u64::from_str_radix(hex, 16).ok()
        } else {
            value.parse::<u64>().ok()
        }
    }

    fn random_seed(&mut self) -> u64 {
        self.random_counter = self.random_counter.wrapping_add(1);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        nanos
            ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ self.random_counter.wrapping_mul(0xD1B5_4A32_D192_ED03)
    }

    fn start_new_world(&mut self) -> bool {
        let Some(seed) = Self::parse_seed(&self.seed_input) else {
            self.menu_error = Some("Seed must be a decimal number or 0x-prefixed hex".to_string());
            return false;
        };
        self.settings.seed = seed;
        self.menu_error = None;
        let sim = sumo::Sim::new(seed);
        self.sim = Some(App::configured(
            sim,
            self.settings.clone(),
            &self.settings_path,
        ));
        self.startup_error = None;
        self.screen = Screen::Simulation;
        true
    }

    fn open_load(&mut self, origin: Origin) {
        self.load_root_input = self.settings.save_dir.clone();
        self.load_error = None;
        self.refresh_saves();
        self.screen = Screen::LoadWorld(origin);
    }

    fn open_settings(&mut self, origin: Origin) {
        self.draft = self.settings.clone();
        self.settings_seed_input = self.settings.seed.to_string();
        self.settings_error = None;
        self.screen = Screen::Settings(origin);
    }

    /// Cancel Settings, discard every draft edit (including seed and tick
    /// rate), and return to the screen that opened it.
    pub fn cancel_settings(&mut self) {
        if let Screen::Settings(origin) = self.screen {
            self.draft = self.settings.clone();
            self.settings_seed_input = self.settings.seed.to_string();
            self.settings_error = None;
            self.screen = match origin {
                Origin::MainMenu => Screen::MainMenu,
                Origin::Simulation => Screen::Simulation,
            };
        }
    }

    /// Parse the Settings seed draft, then persist and commit the whole draft
    /// atomically. An invalid seed leaves the draft open rather than silently
    /// applying stale values.
    pub fn apply_settings(&mut self) {
        let origin = match self.screen {
            Screen::Settings(origin) => origin,
            _ => return,
        };
        let Some(seed) = Self::parse_seed(&self.settings_seed_input) else {
            self.settings_error =
                Some("Seed must be a decimal number or 0x-prefixed hex".to_string());
            return;
        };
        self.draft.seed = seed;
        self.draft.ticks_per_second = self.draft.ticks_per_second.max(1);
        match self.draft.save_atomic(&self.settings_path) {
            Ok(()) => {
                self.settings = self.draft.clone();
                self.seed_input = self.settings.seed.to_string();
                self.load_root_input = self.settings.save_dir.clone();
                if let Some(sim) = self.sim.as_mut() {
                    // Refreshes speed, autosave and the absolute tick rate.
                    sim.apply_settings(&self.settings);
                }
                self.applied_theme = None;
                self.settings_error = None;
                self.screen = match origin {
                    Origin::MainMenu => Screen::MainMenu,
                    Origin::Simulation => Screen::Simulation,
                };
            }
            Err(err) => {
                self.settings_error = Some(format!("Settings were not saved: {err}"));
            }
        }
    }

    /// Load the currently highlighted save, swapping the world only on success.
    pub fn load_selected(&mut self) -> Result<(), String> {
        let Some(index) = self.selected_save else {
            return Err("Select a save first".to_string());
        };
        let Some(probe) = self.saves.get(index).cloned() else {
            return Err("Selected save is no longer available".to_string());
        };
        let app = Self::load_app(&probe.dir, &self.settings, &self.settings_path)?;
        self.sim = Some(app);
        self.screen = match self.screen {
            Screen::LoadWorld(Origin::Simulation) => Screen::Simulation,
            _ => Screen::Simulation,
        };
        self.load_error = None;
        self.startup_error = None;
        Ok(())
    }

    /// Pick a child directory of the save root that does not exist yet.
    /// Nanoseconds plus a monotonic counter avoid same-second and even
    /// same-nanosecond collisions; the loop handles pre-existing directories.
    fn fresh_save_dir(&mut self) -> Result<PathBuf, String> {
        let seed = self
            .sim
            .as_ref()
            .map_or(self.settings.seed, |app| app.sim.world.seed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = self.settings.save_root();
        for attempt in 0..4096u64 {
            self.save_counter = self.save_counter.wrapping_add(1);
            let name = format!(
                "world-{nanos:016x}-{attempt:04x}-{:08x}-{seed:x}",
                self.save_counter
            );
            let candidate = root.join(name);
            if !candidate.exists() {
                return Ok(candidate);
            }
        }
        Err("Could not find a free save directory".to_string())
    }

    /// Save the active world into a fresh child directory of the save root.
    /// The chosen path is reported in the observer status on success.
    pub fn save_current(&mut self) -> Result<PathBuf, String> {
        let dir = self.fresh_save_dir()?;
        let result = {
            let Some(sim) = self.sim.as_mut() else {
                return Err("No world is open".to_string());
            };
            sim.save_to(&dir)
        };
        self.refresh_saves();
        if let Ok(path) = &result {
            if let Some(app) = self.sim.as_mut() {
                app.state.status = Some(format!("Saved world to {}", path.display()));
            }
        }
        result
    }

    fn main_menu_ui(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(64.0);
                ui.add(egui::Label::new(
                    egui::RichText::new("ALCAGENT")
                        .size(42.0)
                        .strong()
                        .color(theme::from_shared(
                            theme::palette(self.settings.theme).accent,
                        )),
                ));
                ui.add(egui::Label::new(
                    egui::RichText::new("A living prison institution — watch it evolve")
                        .size(16.0)
                        .color(theme::MUTED),
                ));
                ui.add_space(36.0);
            });

            if let Some(err) = &self.startup_error {
                ui.vertical_centered(|ui| {
                    ui.add(egui::Label::new(
                        egui::RichText::new(err)
                            .color(theme::from_shared(
                                theme::palette(self.settings.theme).danger,
                            ))
                            .strong(),
                    ));
                });
                ui.add_space(12.0);
            }
            if let Some(err) = &self.menu_error {
                ui.vertical_centered(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(err).color(
                        theme::from_shared(theme::palette(self.settings.theme).warning),
                    )));
                });
                ui.add_space(12.0);
            }

            ui.vertical_centered(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.seed_input)
                        .desired_width(280.0)
                        .hint_text("world seed (decimal or 0x hex)"),
                );
                ui.horizontal(|ui| {
                    if ui.button("Use Default").clicked() {
                        self.seed_input = self.settings.seed.to_string();
                        self.menu_error = None;
                    }
                    if ui.button("Random").clicked() {
                        self.seed_input = format!("{:#x}", self.random_seed());
                        self.menu_error = None;
                    }
                    if ui.button("Start").clicked() {
                        self.start_new_world();
                    }
                });
                ui.add_space(20.0);
                if ui.button("Load Existing Save").clicked() {
                    self.open_load(Origin::MainMenu);
                }
                if ui.button("Settings").clicked() {
                    self.open_settings(Origin::MainMenu);
                }
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                ui.add_space(20.0);
                ui.add(egui::Label::new(
                    egui::RichText::new("Button-driven controls · no hidden shortcuts")
                        .small()
                        .color(theme::MUTED),
                ));
            });
        });
    }

    fn load_world_ui(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Load Existing Save");
            if let Some(err) = &self.load_error {
                ui.add(egui::Label::new(egui::RichText::new(err).strong().color(
                    theme::from_shared(theme::palette(self.settings.theme).danger),
                )));
            }
            ui.horizontal(|ui| {
                ui.label("Save root:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.load_root_input)
                        .desired_width(360.0)
                        .hint_text("directory containing save folders"),
                );
                if ui.button("Refresh").clicked() {
                    self.refresh_saves();
                }
                if ui.button("Up One Directory").clicked() {
                    let path = PathBuf::from(&self.load_root_input);
                    self.load_root_input = path.parent().map_or_else(
                        || self.load_root_input.clone(),
                        |p| p.to_string_lossy().into_owned(),
                    );
                    self.refresh_saves();
                }
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .max_height(460.0)
                .show(ui, |ui| {
                    if self.saves.is_empty() {
                        ui.label("No saves found under this directory.");
                    }
                    for (index, probe) in self.saves.iter().enumerate() {
                        let selected = self.selected_save == Some(index);
                        let row = format!(
                            "{}  ·  {}  ·  {:.1} KiB",
                            probe.name(),
                            probe.summary(),
                            probe.len as f64 / 1024.0
                        );
                        if ui.selectable_label(selected, row).clicked() {
                            self.selected_save = Some(index);
                        }
                    }
                });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Select").clicked() {
                    // Row clicks already select; this button makes the action
                    // explicit and re-probes the highlighted save so version,
                    // population, sim-time and size metadata are fresh.
                    if self.selected_save.is_none() && !self.saves.is_empty() {
                        self.selected_save = Some(0);
                    }
                    if let Some(index) = self.selected_save {
                        if let Some(probe) = self.saves.get(index).cloned() {
                            match probe_save_dir(&probe.dir) {
                                Ok(fresh) => {
                                    self.saves[index] = fresh;
                                    self.load_error = None;
                                }
                                Err(err) => {
                                    self.load_error = Some(format!(
                                        "Cannot probe {}: {err}",
                                        probe.dir.display()
                                    ));
                                }
                            }
                        }
                    }
                }
                if ui.button("Load").clicked() {
                    if let Err(err) = self.load_selected() {
                        self.load_error = Some(err);
                    }
                }
                if ui.button("Cancel").clicked() {
                    self.screen = match self.screen {
                        Screen::LoadWorld(Origin::Simulation) => Screen::Simulation,
                        _ => Screen::MainMenu,
                    };
                }
            });
        });
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Observer Settings");
            if let Some(err) = &self.settings_error {
                ui.add(egui::Label::new(egui::RichText::new(err).color(
                    theme::from_shared(theme::palette(self.settings.theme).danger),
                )));
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                settings_section(ui, "Startup & views", |ui| {
                    egui::ComboBox::from_label("Default view")
                        .selected_text(self.draft.view.title())
                        .show_ui(ui, |ui| {
                            for view in ViewMode::all() {
                                ui.selectable_value(&mut self.draft.view, view, view.title());
                            }
                        });
                    egui::ComboBox::from_label("Playback speed")
                        .selected_text(format!("{:?}", self.draft.speed))
                        .show_ui(ui, |ui| {
                            for speed in [
                                TimeSpeed::Paused,
                                TimeSpeed::RealTime,
                                TimeSpeed::FastForward,
                                TimeSpeed::Extreme,
                                TimeSpeed::EventDriven,
                            ] {
                                ui.selectable_value(
                                    &mut self.draft.speed,
                                    speed,
                                    format!("{speed:?}"),
                                );
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("Default seed:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings_seed_input)
                                .desired_width(220.0),
                        );
                    });
                });

                settings_section(ui, "Saves", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Save root:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.draft.save_dir)
                                .desired_width(320.0),
                        );
                    });
                    autosave_buttons(ui, &mut self.draft.autosave_days);
                });

                settings_section(ui, "Playback rate", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("RealTime ticks / second:");
                        if ui.small_button("−").clicked() {
                            self.draft.ticks_per_second =
                                self.draft.ticks_per_second.saturating_sub(5).max(1);
                        }
                        ui.label(self.draft.ticks_per_second.to_string());
                        if ui.small_button("+").clicked() {
                            self.draft.ticks_per_second =
                                self.draft.ticks_per_second.saturating_add(5).min(4000);
                        }
                    });
                });

                settings_section(ui, "Theme", |ui| {
                    ui.horizontal(|ui| {
                        for theme_kind in ThemeKind::all() {
                            if ui
                                .add(
                                    egui::Button::new(theme_kind.label())
                                        .selected(self.draft.theme == theme_kind),
                                )
                                .clicked()
                            {
                                self.draft.theme = theme_kind;
                            }
                        }
                    });
                });

                settings_section(ui, "World map layers", |ui| {
                    ui.checkbox(&mut self.draft.map_show_zones, "Regions");
                    ui.checkbox(&mut self.draft.map_show_edges, "Roads / nav edges");
                    ui.checkbox(&mut self.draft.map_show_facilities, "Facilities");
                    ui.checkbox(&mut self.draft.map_show_pawns, "Pawns");
                    ui.checkbox(&mut self.draft.map_show_incidents, "Incidents");
                    ui.checkbox(&mut self.draft.map_show_labels, "Labels");
                });

                settings_section(ui, "Interface", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Density:");
                        for density in UiDensity::all() {
                            if ui
                                .add(
                                    egui::Button::new(density.label())
                                        .selected(self.draft.ui_density == density),
                                )
                                .clicked()
                            {
                                self.draft.ui_density = density;
                            }
                        }
                    });
                    ui.checkbox(&mut self.draft.show_help_hints, "Show on-screen help hints");
                });
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Reset to Defaults").clicked() {
                    self.draft = Settings::default();
                    self.settings_seed_input = self.draft.seed.to_string();
                }
                if ui.button("Apply").clicked() {
                    self.apply_settings();
                }
                if ui.button("Cancel").clicked() {
                    self.cancel_settings();
                }
            });
        });
    }

    fn simulation_ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let status = self.sim.as_ref().map_or_else(
            || "No world loaded".to_string(),
            |app| {
                app.state.status.clone().unwrap_or_else(|| {
                    format!(
                        "Day {} · Pop {} · ${:.2}",
                        app.sim.world().days_simulated,
                        app.sim.world().population(),
                        app.sim.economy().cash_major
                    )
                })
            },
        );

        egui::Panel::top("alc_gui_shell_bar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("New").clicked() {
                    self.start_new_world();
                }
                if ui.button("Save").clicked() {
                    if let Err(err) = self.save_current() {
                        if let Some(app) = self.sim.as_mut() {
                            app.state.status = Some(err);
                        }
                    }
                }
                if ui.button("Load").clicked() {
                    self.open_load(Origin::Simulation);
                }
                if ui.button("Settings").clicked() {
                    self.open_settings(Origin::Simulation);
                }
                if ui.button("Menu").clicked() {
                    self.screen = Screen::MainMenu;
                }
                ui.separator();
                ui.add(egui::Label::new(
                    egui::RichText::new(status).small().color(theme::MUTED),
                ));
            });
        });

        if let Some(app) = self.sim.as_mut() {
            EframeApp::ui(app, ui, frame);
        } else {
            self.screen = Screen::MainMenu;
        }
    }
}

impl eframe::App for Shell {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.screen == Screen::Simulation {
            if let Some(app) = self.sim.as_mut() {
                EframeApp::logic(app, ctx, frame);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let visual_key = (self.settings.theme, self.settings.ui_density);
        if self.applied_theme != Some(visual_key) {
            theme::apply_visuals(ui.ctx(), visual_key.0, visual_key.1);
            self.applied_theme = Some(visual_key);
        }
        match self.screen {
            Screen::MainMenu => self.main_menu_ui(ui),
            Screen::LoadWorld(_) => self.load_world_ui(ui),
            Screen::Settings(_) => self.settings_ui(ui),
            Screen::Simulation => self.simulation_ui(ui, frame),
        }
    }
}

/// A titled group used by the settings screen.
fn settings_section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.add(egui::Label::new(
        egui::RichText::new(title).strong().color(theme::ACCENT),
    ));
    ui.separator();
    add(ui);
    ui.add_space(6.0);
}

/// Autosave interval with visible +/- buttons (no DragValue).
fn autosave_buttons(ui: &mut egui::Ui, days: &mut u64) {
    ui.horizontal(|ui| {
        ui.label("Autosave interval (days):");
        if ui.small_button("−").clicked() {
            *days = days.saturating_sub(1);
        }
        ui.label(if *days == 0 {
            "off".to_string()
        } else {
            days.to_string()
        });
        if ui.small_button("+").clicked() {
            *days = (*days + 1).min(365);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_options(dir: &std::path::Path) -> GuiOptions {
        GuiOptions {
            load: None,
            settings_path: dir.join("settings.json"),
            save_dir: dir.join("saves"),
            ..GuiOptions::default()
        }
    }

    #[test]
    fn startup_without_load_shows_main_menu() {
        let dir = std::env::temp_dir().join(format!("alc-gui-shell-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let shell = Shell::new(test_options(&dir));
        assert_eq!(shell.screen(), Screen::MainMenu);
        assert!(shell.sim.is_none());
        assert!(shell.startup_error.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn launch_seed_and_paths_drive_menu_defaults() {
        let dir = std::env::temp_dir().join(format!("alc-gui-shell-opts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let options = GuiOptions {
            seed: 0x1234,
            save_dir: dir.join("custom-saves"),
            ticks_per_second: 240,
            autosave_days: 3,
            ..test_options(&dir)
        };
        let shell = Shell::new(options.clone());
        assert_eq!(shell.seed_input, "4660");
        assert_eq!(shell.settings_seed_input, "4660");
        assert_eq!(shell.settings.seed, 0x1234);
        assert_eq!(shell.load_root_input, options.save_dir.to_string_lossy());
        assert_eq!(shell.settings.ticks_per_second, 240);
        assert_eq!(shell.settings.autosave_days, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_startup_load_stays_on_main_menu_with_a_visible_error() {
        let dir = std::env::temp_dir().join(format!("alc-gui-shell-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut options = test_options(&dir);
        options.load = Some(dir.join("missing-save"));
        let shell = Shell::new(options);
        assert_eq!(shell.screen(), Screen::MainMenu);
        assert!(
            shell
                .startup_error
                .as_deref()
                .is_some_and(|e| e.contains("Startup load error"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn successful_startup_load_enters_simulation() {
        let dir = std::env::temp_dir().join(format!("alc-gui-shell-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let save = dir.join("saves").join("seed-1");
        sumo::Sim::new(11).save(&save).unwrap();
        let mut options = test_options(&dir);
        options.load = Some(save.clone());
        let shell = Shell::new(options);
        assert_eq!(shell.screen(), Screen::Simulation);
        let sim = shell.sim.as_ref().unwrap();
        assert_eq!(sim.sim.world().seed, 11);
        assert!(
            sim.state
                .status
                .as_deref()
                .is_some_and(|s| s.starts_with("Loaded world"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_and_load_screens_return_to_their_origin() {
        let dir = std::env::temp_dir().join(format!("alc-gui-shell-flow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = Shell::new(test_options(&dir));

        shell.open_settings(Origin::MainMenu);
        assert_eq!(shell.screen(), Screen::Settings(Origin::MainMenu));
        shell.cancel_settings();
        assert_eq!(shell.screen(), Screen::MainMenu);

        shell.open_load(Origin::MainMenu);
        assert_eq!(shell.screen(), Screen::LoadWorld(Origin::MainMenu));
        shell.screen = Screen::LoadWorld(Origin::Simulation);
        assert!(shell.load_selected().is_err());
        shell.screen = Screen::LoadWorld(Origin::Simulation);

        assert!(shell.start_new_world());
        assert_eq!(shell.screen(), Screen::Simulation);
        shell.open_settings(Origin::Simulation);
        shell.cancel_settings();
        assert_eq!(shell.screen(), Screen::Simulation);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_browser_lists_saved_directories_with_metadata() {
        let dir = std::env::temp_dir().join(format!("alc-gui-shell-saves-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let save = dir.join("saves").join("seed-12");
        sumo::Sim::new(12).save(&save).unwrap();
        let mut options = test_options(&dir);
        options.load = None;
        let mut shell = Shell::new(options);
        shell.refresh_saves();
        assert_eq!(shell.saves.len(), 1);
        let probe = &shell.saves[0];
        assert_eq!(probe.name(), "seed-12");
        assert!(probe.population.is_some_and(|pop| pop > 0));
        assert!(probe.created_at_secs.is_some());
        assert!(probe.len > 0);
        assert_eq!(probe.status, alc_persistence::SaveStatus::Current);
        shell.selected_save = Some(0);
        shell.screen = Screen::LoadWorld(Origin::MainMenu);
        shell.load_selected().unwrap();
        assert_eq!(shell.screen(), Screen::Simulation);
        assert_eq!(shell.sim.as_ref().unwrap().sim.world().seed, 12);

        let root = dir.join("saves");
        let root_text = root.to_string_lossy();
        assert_eq!(shell.settings.save_dir, root_text);
        assert_eq!(shell.load_root_input, root_text);
        let app = shell.sim.as_ref().unwrap();
        assert_eq!(app.state.save_dir, root_text);
        assert_eq!(app.state.current_save.as_deref(), Some(save.as_path()));
        shell.open_load(Origin::Simulation);
        assert_eq!(shell.saves.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seed_parsing_accepts_decimal_and_hex() {
        assert_eq!(Shell::parse_seed("42"), Some(42));
        assert_eq!(Shell::parse_seed("0xA1CA"), Some(0xA1CA));
        assert_eq!(Shell::parse_seed(" 7 "), Some(7));
        assert_eq!(Shell::parse_seed("nope"), None);
    }

    #[test]
    fn launch_options_are_applied_even_when_disabling_a_persisted_autosave() {
        let dir = std::env::temp_dir().join(format!(
            "alc-gui-shell-authoritative-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Settings {
            autosave_days: 7,
            ticks_per_second: 25,
            save_dir: "stale/place".into(),
            seed: 9,
            ..Settings::default()
        }
        .save_atomic(&dir.join("settings.json"))
        .unwrap();

        let options = GuiOptions {
            seed: 77,
            save_dir: dir.join("saves"),
            ticks_per_second: 120,
            autosave_days: 0,
            ..test_options(&dir)
        };
        let mut shell = Shell::new(options);
        assert_eq!(shell.settings.seed, 77);
        assert_eq!(shell.settings.ticks_per_second, 120);
        assert_eq!(shell.settings.autosave_days, 0);
        assert_eq!(shell.settings.save_dir, dir.join("saves").to_string_lossy());

        assert!(shell.start_new_world());
        assert_eq!(shell.sim.as_ref().unwrap().state.autosave_days, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_cancel_discards_every_draft_edit() {
        let dir = std::env::temp_dir().join(format!("alc-gui-shell-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = Shell::new(test_options(&dir));
        let committed = shell.settings.clone();

        shell.open_settings(Origin::MainMenu);
        shell.settings_seed_input = "999".into();
        shell.draft.ticks_per_second = 900;
        shell.draft.autosave_days = 12;
        shell.cancel_settings();

        assert_eq!(shell.settings, committed);
        assert_eq!(shell.draft, committed);
        assert_eq!(shell.settings_seed_input, committed.seed.to_string());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_apply_persists_seed_and_tick_rate_and_retunes_running_app() {
        let dir = std::env::temp_dir().join(format!("alc-gui-shell-apply-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = Shell::new(test_options(&dir));
        assert!(shell.start_new_world());

        shell.open_settings(Origin::Simulation);
        shell.settings_seed_input = "0x2A".into();
        shell.draft.ticks_per_second = 180;
        shell.draft.autosave_days = 4;
        shell.apply_settings();

        assert_eq!(shell.screen(), Screen::Simulation);
        assert_eq!(shell.settings.seed, 42);
        assert_eq!(shell.settings.ticks_per_second, 180);
        assert_eq!(shell.seed_input, "42");
        let persisted = Settings::load(&dir.join("settings.json"));
        assert_eq!(persisted.seed, 42);
        assert_eq!(persisted.ticks_per_second, 180);
        assert_eq!(shell.sim.as_ref().unwrap().state.autosave_days, 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_settings_seed_does_not_commit_the_draft() {
        let dir =
            std::env::temp_dir().join(format!("alc-gui-shell-badseed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = Shell::new(test_options(&dir));
        let committed = shell.settings.clone();
        shell.open_settings(Origin::MainMenu);
        shell.settings_seed_input = "nope".into();
        shell.draft.ticks_per_second = 999;

        shell.apply_settings();

        assert_eq!(shell.screen(), Screen::Settings(Origin::MainMenu));
        assert_eq!(shell.settings, committed);
        assert!(shell.settings_error.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn startup_error_clears_on_new_world_and_on_successful_load() {
        let dir =
            std::env::temp_dir().join(format!("alc-gui-shell-clearerr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut options = test_options(&dir);
        options.load = Some(dir.join("missing-save"));
        let mut shell = Shell::new(options);
        assert!(shell.startup_error.is_some());

        shell.seed_input = "5".into();
        assert!(shell.start_new_world());
        assert!(shell.startup_error.is_none());

        let save = dir.join("saves").join("seed-5");
        shell.sim.as_mut().unwrap().sim.save(&save).unwrap();
        shell.refresh_saves();
        shell.selected_save = Some(0);
        shell.startup_error = Some("stale failure".into());
        shell.load_selected().unwrap();
        assert!(shell.startup_error.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rapid_manual_saves_use_distinct_directories_and_report_the_path() {
        let dir =
            std::env::temp_dir().join(format!("alc-gui-shell-saveunique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = Shell::new(test_options(&dir));
        assert!(shell.start_new_world());

        let first = shell.save_current().unwrap();
        let second = shell.save_current().unwrap();

        assert_ne!(first, second);
        assert!(first.exists() && second.exists());
        let status = shell
            .sim
            .as_ref()
            .unwrap()
            .state
            .status
            .as_deref()
            .unwrap_or_default();
        assert!(status.contains(&second.display().to_string()), "{status}");

        // Saving a world must not turn its generated leaf into the save root,
        // otherwise the next launch's browser would list no worlds.
        let root = dir.join("saves");
        let root_text = root.to_string_lossy();
        assert_eq!(shell.settings.save_dir, root_text);
        assert_eq!(shell.load_root_input, root_text);
        let app = shell.sim.as_ref().unwrap();
        assert_eq!(app.state.save_dir, root_text);
        assert_eq!(app.state.current_save.as_deref(), Some(second.as_path()));
        assert_eq!(
            Settings::load(&dir.join("settings.json")).save_dir,
            root_text
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
