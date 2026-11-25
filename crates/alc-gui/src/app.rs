//! The egui application shell.
//!
//! [`App`] owns the simulation, all observer UI state, and a queue of
//! [`Command`]s. Rendering never touches `sumo::Sim`; handlers only enqueue
//! commands that are applied here through validated engine control methods.

use crate::{data, map_view, panels, screens, settings, ViewMode};
use alc_core::{PrisonerId, TimeSpeed};
use alc_warden::WardenAction;
use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

/// A user intent produced by a view. Applied by [`App::apply`] through
/// validated `sumo::Sim` controls — never as arbitrary world mutation.
#[derive(Debug, Clone)]
pub enum Command {
    SetView(ViewMode),
    SetSpeed(TimeSpeed),
    StepTick,
    OneDay,
    Save(String),
    Load(String),
    SetAutosave(u64),
    SetPolicyBand(sumo::Policy),
    SetLockdown(bool),
    SetSecurity(f32),
    SetPrivileges(f32),
    SetRehabFunding(f32),
    SetIntakePaused(bool),
    SetPolicyMode(sumo::PolicyMode),
    WardenAction(WardenAction),
    EnqueueConstruction {
        kind: alc_world::FacilityKind,
        wing: u32,
        capacity: u32,
    },
    AdvanceConstruction(u64, u32),
    CancelConstruction(u64),
    FailConstruction(u64),
    EnrollRehab {
        prisoner: PrisonerId,
        key: String,
    },
    DropRehab {
        prisoner: PrisonerId,
        key: String,
    },
    ResolveContraband(u64),
    ResolveIncident(u64),
    StartParole(PrisonerId),
    GrantParole(PrisonerId),
    DenyParole(PrisonerId, String),
    RecordPostRelease {
        prisoner: PrisonerId,
        release_type: String,
        risk: f32,
        plan: bool,
    },
}

/// Observer UI state (selection, search, filters and map camera).
///
/// This is deliberately separate from simulation state so it can change every
/// frame without invalidating the cached [`data::Snapshot`].
#[derive(Debug)]
pub struct ObserverState {
    pub view: ViewMode,
    pub search: String,
    pub selected_prisoner: Option<PrisonerId>,
    pub selected_gang: Option<u64>,
    pub selected_guard: Option<u64>,
    pub selected_incident: Option<u64>,
    pub selected_contraband: Option<u64>,
    pub selected_decision: Option<String>,
    pub selected_facility: Option<u64>,
    pub construction_kind_index: usize,
    pub rehab_program_index: usize,
    pub build_wing: u32,
    pub build_capacity: u32,
    pub save_dir: String,
    pub save_input: String,
    pub load_input: String,
    pub autosave_days: u64,
    pub status: Option<String>,
    pub incident_min_severity: f32,
    pub incident_open_only: bool,
    pub decision_validation_filter: Option<String>,
    pub event_kind_filter: String,
    pub map: map_view::MapViewState,
}

impl ObserverState {
    fn from_settings(s: &settings::Settings) -> Self {
        Self {
            view: s.view,
            search: String::new(),
            selected_prisoner: None,
            selected_gang: None,
            selected_guard: None,
            selected_incident: None,
            selected_contraband: None,
            selected_decision: None,
            selected_facility: None,
            construction_kind_index: 0,
            rehab_program_index: 0,
            build_wing: 1,
            build_capacity: 20,
            save_dir: s.save_dir.clone(),
            save_input: s.save_dir.clone(),
            load_input: s.save_dir.clone(),
            autosave_days: s.autosave_days,
            status: None,
            incident_min_severity: 0.0,
            incident_open_only: false,
            decision_validation_filter: None,
            event_kind_filter: String::new(),
            map: map_view::MapViewState::default(),
        }
    }

    /// Persist the observer-facing subset into [`settings::Settings`].
    fn to_settings(&self, speed: TimeSpeed) -> settings::Settings {
        settings::Settings {
            view: self.view,
            speed,
            save_dir: self.save_dir.clone(),
            autosave_days: self.autosave_days,
        }
    }
}

/// The eframe observer application.
pub struct App {
    pub sim: sumo::Sim,
    pub state: ObserverState,
    snapshot: Option<data::Snapshot>,
    cache_revision: u64,
    cache_selection: Option<PrisonerId>,
    revision: u64,
    commands: Vec<Command>,
    last_logic: Option<Instant>,
    tick_debt: f32,
    settings_path: PathBuf,
}

impl App {
    /// Construct an observer around a freshly booted simulation.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self::with_sim(sumo::Sim::new(seed))
    }

    /// Construct an observer around an existing simulation (used by tests and
    /// by the load path).
    #[must_use]
    pub fn with_sim(mut sim: sumo::Sim) -> Self {
        let settings_path = PathBuf::from("saves/alc-gui/settings.json");
        let settings = settings::Settings::load(&settings_path);
        sim.set_speed(settings.speed);
        let state = ObserverState::from_settings(&settings);
        let mut app = Self {
            sim,
            state,
            snapshot: None,
            cache_revision: u64::MAX,
            cache_selection: None,
            revision: 0,
            commands: Vec::new(),
            last_logic: None,
            tick_debt: 0.0,
            settings_path,
        };
        let speed = app.state.view; // view restored from settings; speed below
        let _ = speed;
        app
    }

    /// Number of ticks to run per real second at each playback speed.
    fn ticks_per_second(speed: TimeSpeed) -> f32 {
        match speed {
            TimeSpeed::Paused => 0.0,
            TimeSpeed::RealTime | TimeSpeed::EventDriven => 60.0,
            TimeSpeed::FastForward => 120.0,
            TimeSpeed::Extreme => 240.0,
        }
    }

    /// Apply a queued command. This is the single mutation boundary for the UI.
    fn apply(&mut self, command: Command) {
        use Command::*;
        let mut persist = false;
        match command {
            SetSpeed(speed) => {
                self.sim.set_speed(speed);
                self.state.status = Some(format!("Playback: {speed:?}"));
                persist = true;
            }
            StepTick => {
                let prev = self.sim.speed();
                if prev == TimeSpeed::Paused {
                    self.sim.set_speed(TimeSpeed::RealTime);
                    self.sim.tick();
                    self.sim.set_speed(prev);
                } else {
                    self.sim.tick();
                }
                self.state.status = Some("Stepped one tick".to_string());
            }
            OneDay => {
                let prev = self.sim.speed();
                self.sim.set_speed(TimeSpeed::Extreme);
                self.sim.run_days(1);
                self.sim.set_speed(prev);
                self.state.status = Some("Advanced one simulated day".to_string());
            }
            Save(dir) => {
                match self.sim.save(&dir) {
                    Ok(()) => self.state.status = Some(format!("Saved world to {dir}")),
                    Err(e) => self.state.status = Some(format!("Save failed: {e}")),
                }
                self.state.save_dir = dir.clone();
                self.state.save_input = dir.clone();
                self.state.load_input = dir;
                if self.state.autosave_days > 0 {
                    let dir = self.state.save_dir.clone();
                    self.sim.enable_autosave(dir, self.state.autosave_days);
                } else {
                    self.sim.disable_autosave();
                }
                persist = true;
            }
            Load(dir) => {
                match sumo::Sim::load(&dir) {
                    Ok(sim) => {
                        let speed = self.sim.speed();
                        self.sim = sim;
                        self.sim.set_speed(speed);
                        self.state.status = Some(format!("Loaded world from {dir}"));
                    }
                    Err(e) => self.state.status = Some(format!("Load failed: {e}")),
                }
                persist = true;
            }
            SetAutosave(days) => {
                self.state.autosave_days = days;
                if days == 0 {
                    self.sim.disable_autosave();
                    self.state.status = Some("Autosave disabled".to_string());
                } else {
                    let dir = self.state.save_dir.clone();
                    self.sim.enable_autosave(dir, days);
                    self.state.status = Some(format!("Autosave every {days} day(s)"));
                }
                persist = true;
            }
            SetPolicyBand(policy) => {
                self.sim.set_policy(policy);
                self.state.status = Some(format!("Policy band: {policy:?}"));
            }
            SetLockdown(on) => {
                self.sim.set_lockdown(on);
                self.state.status = Some(format!("Lockdown: {}", if on { "ON" } else { "off" }));
            }
            SetSecurity(v) => {
                self.sim.set_security_level(v);
                self.state.status = Some(format!("Security level {v:.2}"));
            }
            SetPrivileges(v) => {
                self.sim.set_privileges_level(v);
                self.state.status = Some(format!("Privileges {v:.2}"));
            }
            SetRehabFunding(v) => {
                self.sim.set_rehab_funding(v);
                self.state.status = Some(format!("Rehab funding {v:.2}x"));
            }
            SetIntakePaused(paused) => {
                self.sim.set_intake_paused(paused);
                self.state.status = Some(format!("Intake {}", if paused { "paused" } else { "open" }));
            }
            SetPolicyMode(mode) => {
                self.sim.set_policy_mode(mode);
                self.state.status = Some(format!("Policy mode: {mode:?}"));
            }
            WardenAction(action) => {
                let label = action.label();
                self.sim.apply_warden_action(action);
                self.state.status = Some(format!("Warden action applied: {label}"));
            }
            EnqueueConstruction { kind, wing, capacity } => {
                let id = self.sim.enqueue_construction(kind, wing, capacity);
                self.state.status = Some(format!("Queued {} construction #{id}", kind.display()));
            }
            AdvanceConstruction(id, days) => {
                let ok = self.sim.advance_construction(id, days);
                self.state.status = Some(format!(
                    "Construction #{id} advance {days}d: {}",
                    if ok { "accepted" } else { "rejected" }
                ));
            }
            CancelConstruction(id) => {
                let ok = self.sim.cancel_construction(id, "observer control");
                self.state.status = Some(format!(
                    "Construction #{id} cancel: {}",
                    if ok { "accepted" } else { "rejected" }
                ));
            }
            FailConstruction(id) => {
                let ok = self.sim.fail_construction(id, "observer control");
                self.state.status = Some(format!(
                    "Construction #{id} fail: {}",
                    if ok { "accepted" } else { "rejected" }
                ));
            }
            EnrollRehab { prisoner, key } => {
                let ok = self.sim.enroll_rehab_by_key(prisoner, key.as_str());
                self.state.status = Some(format!(
                    "Enroll {key} for prisoner {}: {}",
                    prisoner.raw(),
                    if ok { "accepted" } else { "rejected" }
                ));
            }
            DropRehab { prisoner, key } => {
                let ok = self.sim.drop_rehab_by_key(prisoner, key.as_str());
                self.state.status = Some(format!(
                    "Drop {key} for prisoner {}: {}",
                    prisoner.raw(),
                    if ok { "accepted" } else { "rejected" }
                ));
            }
            ResolveContraband(id) => {
                let ok = self.sim.resolve_contraband_case(id);
                self.state.status = Some(format!(
                    "Resolve contraband #{id}: {}",
                    if ok { "accepted" } else { "rejected" }
                ));
            }
            ResolveIncident(id) => {
                let ok = self.sim.resolve_incident(id);
                self.state.status = Some(format!(
                    "Resolve incident #{id}: {}",
                    if ok { "accepted" } else { "rejected" }
                ));
            }
            StartParole(id) => {
                let ok = self.sim.start_parole_hearing(id).is_some();
                self.state.status = Some(format!(
                    "Parole hearing for prisoner {}: {}",
                    id.raw(),
                    if ok { "scheduled" } else { "rejected" }
                ));
            }
            GrantParole(id) => {
                let ok = self.sim.grant_parole(id).is_some();
                self.state.status = Some(format!(
                    "Parole for prisoner {}: {}",
                    id.raw(),
                    if ok { "granted" } else { "rejected" }
                ));
            }
            DenyParole(id, reason) => {
                let ok = self.sim.deny_parole(id, reason).is_some();
                self.state.status = Some(format!(
                    "Parole for prisoner {}: {}",
                    id.raw(),
                    if ok { "denied" } else { "rejected" }
                ));
            }
            RecordPostRelease { prisoner, release_type, risk, plan } => {
                let _ = self.sim.record_post_release(prisoner, &release_type, risk, plan);
                self.state.status = Some(format!(
                    "Post-release recorded for prisoner {} ({release_type})",
                    prisoner.raw()
                ));
            }
        }
        self.revision += 1;
        if persist {
            self.state.to_settings(self.sim.speed()).save(&self.settings_path);
        }
    }

    /// Return the cached snapshot, rebuilding only when the engine revision or
    /// the selected prisoner changed.
    fn snapshot(&mut self) -> &data::Snapshot {
        if self.snapshot.is_none()
            || self.cache_revision != self.revision
            || self.cache_selection != self.state.selected_prisoner
        {
            self.snapshot = Some(data::capture(&self.sim, self.state.selected_prisoner));
            self.cache_revision = self.revision;
            self.cache_selection = self.state.selected_prisoner;
        }
        self.snapshot.as_ref().expect("snapshot just captured")
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = self
            .last_logic
            .map(|t| now.duration_since(t).as_secs_f32().min(0.25))
            .unwrap_or(0.0);
        self.last_logic = Some(now);

        let speed = self.sim.speed();
        let rate = Self::ticks_per_second(speed);
        if rate > 0.0 {
            self.tick_debt = (self.tick_debt + dt * rate).min(512.0);
            let n = self.tick_debt.floor() as usize;
            self.tick_debt -= n as f32;
            for _ in 0..n {
                self.sim.tick();
            }
            if n > 0 {
                self.revision += 1;
            }
            ctx.request_repaint();
        } else {
            self.tick_debt = 0.0;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let snapshot = self.snapshot();
        let mut ctx = panels::RenderCtx {
            snapshot,
            state: &mut self.state,
            commands: &mut self.commands,
        };
        panels::toolbar(ui, &mut ctx);
        egui::CentralPanel::default().show(ui, |ui| screens::show(ui, &mut ctx));

        let commands = std::mem::take(&mut self.commands);
        for command in commands {
            self.apply(command);
        }
    }

    fn on_exit(&mut self) {
        self.state.to_settings(self.sim.speed()).save(&self.settings_path);
    }
}

/// Launch the native observer window with a fresh simulation from `seed`.
///
/// # Errors
/// Returns `eframe` errors if a graphics context or window cannot be created.
pub fn launch(seed: u64) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Alcagent — Living Prison Observer",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(seed)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_applied_through_validated_controls() {
        let mut app = App::new(0xC0FFEE);
        let pop0 = app.sim.world().population();
        app.apply(Command::StepTick);
        app.apply(Command::OneDay);
        assert!(app.sim.world().days_simulated >= 1);
        assert_eq!(app.sim.world().population(), pop0); // paused tick is a no-op

        app.apply(Command::SetLockdown(true));
        assert!(app.sim.world().policies.lockdown);
        app.apply(Command::SetLockdown(false));
        assert!(!app.sim.world().policies.lockdown);

        app.apply(Command::SetSecurity(0.93));
        assert!((app.sim.world().policies.security_level - 0.93).abs() < 1e-6);
        app.apply(Command::SetPolicyBand(sumo::Policy::Enhanced));
        assert!((app.sim.world().policies.security_level - 0.8).abs() < 1e-6);

        app.apply(Command::WardenAction(WardenAction::HireGuards { count: 1 }));
        assert!(app.state.status.is_some());
        assert!(app.sim.check_invariants().is_empty());
    }

    #[test]
    fn tick_rates_are_bounded_and_ordered() {
        assert_eq!(App::ticks_per_second(TimeSpeed::Paused), 0.0);
        assert!(App::ticks_per_second(TimeSpeed::RealTime) < App::ticks_per_second(TimeSpeed::FastForward));
        assert!(App::ticks_per_second(TimeSpeed::FastForward) < App::ticks_per_second(TimeSpeed::Extreme));
    }
}
