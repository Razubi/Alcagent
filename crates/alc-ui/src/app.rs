//! Interactive ratatui TUI (Directive §20, §21).
//!
//! This module owns the terminal, the frame loop and keyboard input. The
//! simulation itself is supplied through [`SimDriver`] so this crate never has
//! to depend on the engine crate (`sumo`) — that would be a dependency cycle,
//! since the engine already depends on `alc-ui` for observation snapshots.
//!
//! The binary crate (`alcagent`) adapts `sumo::Sim` to [`SimDriver`] locally.

use crate::views::{self, AppStatus};
use crate::{UiState, ViewMode};
use alc_core::TimeSpeed;
use sumo::{MapFrame, Observation, SimTime, Stats};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::Frame;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::{Duration, Instant};

/// The slice of the engine the TUI needs. Implement this in the binary crate
/// for `sumo::Sim` (or any other simulation backend).
///
/// This trait uses sumo's observation APIs (`Observation`, `MapFrame`, etc.)
/// instead of exposing the raw `World` directly. This maintains the engine's
/// authority over state access and enables features like PolicyMode::Council
/// and brain-inspector data.
pub trait SimDriver {
    /// Advance the simulation by one engine tick.
    fn tick(&mut self);
    /// Get a read-only observation snapshot for rendering.
    fn observation(&self) -> Observation;
    /// Get a point-in-time map frame of all pawn positions for the live map.
    fn map_frame(&self) -> MapFrame;
    /// Get metrics derived from the event log since `at`.
    fn stats_since(&self, at: SimTime) -> Stats;
    /// Get events since a point in time.
    fn events_since(&self, at: SimTime) -> Vec<sumo::WorldEvent>;
    /// Get all prisoners for the prisoner view.
    fn prisoners(&self) -> Vec<sumo::PrisonerSummary>;
    /// Get all gangs for the gang view.
    fn gangs(&self) -> Vec<sumo::GangSummary>;
    /// Get all guards for the staff view.
    fn guards(&self) -> Vec<sumo::GuardSummary>;
    /// Get the warden for the warden view.
    fn warden(&self) -> sumo::WardenSummary;
    /// Get the economy for the financial view.
    fn economy(&self) -> sumo::EconomySummary;
    /// Get social graph edges for the social network view.
    fn social_edges(&self) -> Vec<sumo::SocialEdge>;
    /// Get facilities for the map view.
    fn facilities(&self) -> Vec<sumo::FacilitySummary>;
    /// Current engine speed.
    fn speed(&self) -> TimeSpeed;
    /// Change engine speed (used when pausing/resuming).
    fn set_speed(&mut self, speed: TimeSpeed);
    /// Get the current policy mode.
    fn policy_mode(&self) -> sumo::PolicyMode;
    /// Set the policy mode (Council, Shadow, Deterministic).
    fn set_policy_mode(&mut self, mode: sumo::PolicyMode);
    /// Get the current simulation time.
    fn sim_time(&self) -> SimTime;
    /// Get the number of days simulated.
    fn days_simulated(&self) -> u64;
    /// Set lockdown state through the engine.
    fn set_lockdown(&mut self, lockdown: bool);
    /// Set policy through the engine.
    fn set_policy(&mut self, policy: sumo::Policy);
    /// Save simulation state to a directory.
    fn save(&self, dir: &std::path::Path) -> Result<(), String>;
    /// Enable autosave.
    fn enable_autosave(&mut self, dir: std::path::PathBuf, interval_days: u64);
    /// Maybe run autosave; returns true if a save occurred.
    fn maybe_autosave(&mut self) -> Result<bool, String>;
    /// Incident log for audit/inspection.
    fn incident_log(&self) -> Vec<sumo::WorldEvent>;
    /// Decision audit log as formatted entries.
    fn decision_log(&self) -> Vec<String>;
}

/// Startup options for the TUI.
#[derive(Debug, Clone)]
pub struct TuiOptions {
    /// Ticks issued per wall-clock second while running.
    pub ticks_per_second: u32,
    /// Start in the paused state.
    pub paused: bool,
    /// Directory used for manual saves ('S' key) and autosave.
    pub save_dir: std::path::PathBuf,
}

impl Default for TuiOptions {
    fn default() -> Self {
        Self {
            ticks_per_second: 60,
            paused: false,
            save_dir: std::path::PathBuf::from("saves/default"),
        }
    }
}

/// The observer application: simulation driver + UI cursor + playback state.
pub struct App<D: SimDriver> {
    driver: D,
    ui: UiState,
    paused: bool,
    ticks_per_second: u32,
    should_quit: bool,
    save_dir: std::path::PathBuf,
    status_msg: Option<String>,
}

impl<D: SimDriver> App<D> {
    pub fn new(driver: D, opts: TuiOptions) -> Self {
        let mut app = Self {
            driver,
            ui: UiState::new(),
            paused: opts.paused,
            ticks_per_second: opts.ticks_per_second.clamp(1, 4_000),
            should_quit: false,
            save_dir: opts.save_dir,
            status_msg: None,
        };
        // The engine starts paused by default; mirror our playback state.
        let engine_speed = if app.paused {
            TimeSpeed::Paused
        } else {
            TimeSpeed::RealTime
        };
        app.driver.set_speed(engine_speed);
        app
    }

    pub fn ui(&self) -> &UiState {
        &self.ui
    }

    pub fn ui_mut(&mut self) -> &mut UiState {
        &mut self.ui
    }

    pub fn driver(&self) -> &D {
        &self.driver
    }

    pub fn driver_mut(&mut self) -> &mut D {
        &mut self.driver
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn ticks_per_second(&self) -> u32 {
        self.ticks_per_second
    }

    /// Run the frame loop until the observer quits.
    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        let mut last = Instant::now();
        let mut accumulator = Duration::ZERO;

        loop {
            // Drain all pending input without blocking the render loop.
            while event::poll(Duration::from_millis(0))? {
                if let Event::Key(key) = event::read()? {
                    // `Repeat` lets held arrow keys scroll continuously.
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                        self.handle_key(key.code, key.modifiers);
                    }
                }
            }
            if self.should_quit {
                break;
            }

            let now = Instant::now();
            accumulator += now.duration_since(last);
            last = now;

            if self.paused {
                accumulator = Duration::ZERO;
            } else {
                let interval =
                    Duration::from_secs_f64(1.0 / f64::from(self.ticks_per_second.max(1)));
                let mut budget = 0_u32;
                while accumulator >= interval && budget < 5_000 {
                    self.driver.tick();
                    let _ = self.driver.maybe_autosave();
                    accumulator -= interval;
                    budget += 1;
                }
                // Don't accumulate an unbounded debt if the machine stalls.
                if accumulator > interval.saturating_mul(10) {
                    accumulator = Duration::ZERO;
                }
            }

            self.sync_selection();
            terminal.draw(|frame| self.render(frame))?;
            std::thread::sleep(Duration::from_millis(16));
        }
        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let status = AppStatus {
            paused: self.paused,
            ticks_per_second: self.ticks_per_second,
            status_msg: self.status_msg.clone(),
        };
        let obs = self.driver.observation();
        let data = views::ViewData {
            observation: obs,
            prisoners: self.driver.prisoners(),
            gangs: self.driver.gangs(),
            guards: self.driver.guards(),
            warden: self.driver.warden(),
            economy: self.driver.economy(),
            social_edges: self.driver.social_edges(),
            facilities: self.driver.facilities(),
            events: self.driver.events_since(SimTime::EPOCH),
            incident_log: self.driver.incident_log(),
            decision_log: self.driver.decision_log(),
        };
        views::draw(frame, &data, &self.ui, &status);
    }

    /// Clamp the shared cursor to the active list and keep the selected-id
    /// fields in [`UiState`] consistent with it.
    fn sync_selection(&mut self) {
        let obs = self.driver.observation();
        let prisoners = if matches!(self.ui.mode, ViewMode::Prisoner) { self.driver.prisoners() } else { Vec::new() };
        let gangs = if matches!(self.ui.mode, ViewMode::Gang) { self.driver.gangs() } else { Vec::new() };
        let guards = if matches!(self.ui.mode, ViewMode::Staff) { self.driver.guards() } else { Vec::new() };
        let facilities = if matches!(self.ui.mode, ViewMode::Map) { self.driver.facilities() } else { Vec::new() };
        let events = if matches!(self.ui.mode, ViewMode::EventTimeline) { self.driver.events_since(SimTime::EPOCH) } else { Vec::new() };
        let social = if matches!(self.ui.mode, ViewMode::SocialNetwork) { self.driver.social_edges() } else { Vec::new() };
        let economy = if matches!(self.ui.mode, ViewMode::Financial) { self.driver.economy() } else { self.driver.economy() };
        let incidents = if matches!(self.ui.mode, ViewMode::IncidentReports) { self.driver.incident_log() } else { Vec::new() };
        let decisions = if matches!(self.ui.mode, ViewMode::DecisionAudit) { self.driver.decision_log() } else { Vec::new() };

        let count = match self.ui.mode {
            ViewMode::Prisoner => prisoners.len(),
            ViewMode::Gang => gangs.len(),
            ViewMode::Staff => guards.len(),
            ViewMode::EventTimeline => events.len(),
            ViewMode::Financial => economy.ledger.len(),
            ViewMode::SocialNetwork => social.len(),
            ViewMode::Map => facilities.len(),
            ViewMode::IncidentReports => incidents.len(),
            ViewMode::DecisionAudit => decisions.len(),
            ViewMode::InstitutionOverview | ViewMode::Warden => 0,
        };
        let max = count.saturating_sub(1).min(u16::MAX as usize) as u16;
        if count == 0 {
            self.ui.scroll = 0;
        } else if self.ui.scroll > max {
            self.ui.scroll = max;
        }
        let idx = self.ui.scroll as usize;
        self.ui.selected_prisoner = prisoners.get(idx).map(|p| p.id.0);
        self.ui.selected_gang = gangs.get(idx).map(|g| g.id);
        self.ui.selected_guard = guards.get(idx).map(|g| g.id);
        let _ = obs;
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        if modifiers.contains(KeyModifiers::CONTROL) {
            if code == KeyCode::Char('c') || code == KeyCode::Char('C') {
                self.should_quit = true;
            }
            return;
        }

        match code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char(' ') => self.toggle_pause(),
            KeyCode::Char('+') | KeyCode::Char('=') => self.speed_up(),
            KeyCode::Char('-') | KeyCode::Char('_') => self.speed_down(),
            KeyCode::Char('s') | KeyCode::Char('S') => {
                match self.driver.save(&self.save_dir) {
                    Ok(()) => self.status_msg = Some(format!("Saved to {}", self.save_dir.display())),
                    Err(e) => self.status_msg = Some(format!("Save failed: {e}")),
                }
            }
            KeyCode::Char('L') => {
                self.driver.set_lockdown(true);
                self.status_msg = Some("Lockdown enabled".into());
            }
            KeyCode::Char('U') => {
                self.driver.set_lockdown(false);
                self.status_msg = Some("Lockdown lifted".into());
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                // Quick policy toggle: Normal -> Enhanced -> Lockdown -> Normal
                let current = self.driver.policy_mode();
                let next = match current {
                    sumo::PolicyMode::Deterministic => sumo::PolicyMode::Council,
                    sumo::PolicyMode::Council => sumo::PolicyMode::Shadow,
                    sumo::PolicyMode::Shadow => sumo::PolicyMode::Deterministic,
                };
                self.driver.set_policy_mode(next);
                self.status_msg = Some(format!("Policy mode: {:?}", next));
            }
            KeyCode::Char(c @ '1'..='9') => {
                if let Some(mode) = views::mode_from_index((c as u8 - b'1') as usize) {
                    self.ui.mode = mode;
                    self.ui.scroll = 0;
                }
            }
            KeyCode::Char('0') | KeyCode::Char('i') | KeyCode::Char('I') => {
                self.ui.mode = ViewMode::IncidentReports;
                self.ui.scroll = 0;
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.ui.mode = ViewMode::DecisionAudit;
                self.ui.scroll = 0;
            }
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k' | 'h') => {
                self.move_cursor(-1)
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j' | 'l') => {
                self.move_cursor(1)
            }
            _ => {}
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        // All views (including the schematic map) share one selection cursor.
        // Clamp rather than cast so a large list never wraps back to the top.
        let next = i32::from(self.ui.scroll) + delta;
        self.ui.scroll = next.clamp(0, i32::from(u16::MAX)) as u16;
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        let speed = if self.paused {
            TimeSpeed::Paused
        } else {
            TimeSpeed::RealTime
        };
        self.driver.set_speed(speed);
    }

    fn speed_up(&mut self) {
        self.ticks_per_second = self.ticks_per_second.saturating_mul(2).min(4_000);
    }

    fn speed_down(&mut self) {
        self.ticks_per_second = (self.ticks_per_second / 2).max(1);
    }
}

/// Convenience entry point: set up the terminal, run the app, and always
/// restore the terminal to its previous state (even on error).
pub fn run_tui<D: SimDriver>(driver: D, opts: TuiOptions) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut app = App::new(driver, opts);
    let result = app.run(&mut terminal);

    // Best-effort cleanup — never leave the user's terminal in raw mode.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockDriver {
        sim: sumo::Sim,
        ticks: u32,
    }

    impl SimDriver for MockDriver {
        fn tick(&mut self) { self.ticks += 1; self.sim.tick(); }
        fn observation(&self) -> sumo::Observation { self.sim.observation() }
        fn map_frame(&self) -> sumo::MapFrame { self.sim.map_frame() }
        fn stats_since(&self, at: sumo::SimTime) -> sumo::Stats { self.sim.stats_since(at) }
        fn events_since(&self, at: sumo::SimTime) -> Vec<sumo::WorldEvent> { self.sim.events_since(at) }
        fn prisoners(&self) -> Vec<sumo::PrisonerSummary> { self.sim.prisoners() }
        fn gangs(&self) -> Vec<sumo::GangSummary> { self.sim.gangs() }
        fn guards(&self) -> Vec<sumo::GuardSummary> { self.sim.guards() }
        fn warden(&self) -> sumo::WardenSummary { self.sim.warden() }
        fn economy(&self) -> sumo::EconomySummary { self.sim.economy() }
        fn social_edges(&self) -> Vec<sumo::SocialEdge> { self.sim.social_edges() }
        fn facilities(&self) -> Vec<sumo::FacilitySummary> { self.sim.facilities() }
        fn speed(&self) -> TimeSpeed { self.sim.speed() }
        fn set_speed(&mut self, speed: TimeSpeed) { self.sim.set_speed(speed); }
        fn policy_mode(&self) -> sumo::PolicyMode { self.sim.policy_mode() }
        fn set_policy_mode(&mut self, mode: sumo::PolicyMode) { self.sim.set_policy_mode(mode); }
        fn sim_time(&self) -> sumo::SimTime { self.sim.world.time }
        fn days_simulated(&self) -> u64 { self.sim.world.days_simulated }
        fn set_lockdown(&mut self, lockdown: bool) { self.sim.set_lockdown(lockdown); }
        fn set_policy(&mut self, policy: sumo::Policy) { self.sim.set_policy(policy); }
        fn save(&self, dir: &std::path::Path) -> Result<(), String> { self.sim.save(dir).map_err(|e| e.to_string()) }
        fn enable_autosave(&mut self, dir: std::path::PathBuf, interval_days: u64) { self.sim.enable_autosave(dir, interval_days); }
        fn maybe_autosave(&mut self) -> Result<bool, String> { self.sim.maybe_autosave().map_err(|e| e.to_string()) }
        fn incident_log(&self) -> Vec<sumo::WorldEvent> { self.sim.incident_log() }
        fn decision_log(&self) -> Vec<String> { self.sim.decision_log().iter().map(|d| format!("{:?}", d)).collect() }
    }

    fn app() -> App<MockDriver> {
        App::new(
            MockDriver { sim: sumo::Sim::new(7), ticks: 0 },
            TuiOptions::default(),
        )
    }

    #[test]
    fn number_keys_switch_modes_and_reset_cursor() {
        let mut app = app();
        app.ui_mut().scroll = 9;
        app.handle_key(KeyCode::Char('7'), KeyModifiers::NONE);
        assert_eq!(app.ui().mode, ViewMode::EventTimeline);
        assert_eq!(app.ui().scroll, 0);
        app.handle_key(KeyCode::Char('9'), KeyModifiers::NONE);
        assert_eq!(app.ui().mode, ViewMode::Map);
    }

    #[test]
    fn space_toggles_pause_and_engine_speed() {
        let mut app = app();
        assert!(!app.is_paused());
        assert_eq!(app.driver().speed(), TimeSpeed::RealTime);
        app.handle_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(app.is_paused());
        assert_eq!(app.driver().speed(), TimeSpeed::Paused);
        app.handle_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(!app.is_paused());
        assert_eq!(app.driver().speed(), TimeSpeed::RealTime);
    }

    #[test]
    fn speed_and_cursor_keys_are_bounded() {
        let mut app = app();
        app.handle_key(KeyCode::Char('-'), KeyModifiers::NONE);
        assert_eq!(app.ticks_per_second(), 30);
        for _ in 0..30 {
            app.handle_key(KeyCode::Char('-'), KeyModifiers::NONE);
        }
        assert_eq!(app.ticks_per_second(), 1);
        app.handle_key(KeyCode::Char('+'), KeyModifiers::NONE);
        assert_eq!(app.ticks_per_second(), 2);

        app.handle_key(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.ui().scroll, 0);
        app.handle_key(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.ui().scroll, 1);

        app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.should_quit);
    }

    #[test]
    fn selection_sync_tracks_world_lists() {
        let mut app = app();
        app.ui_mut().mode = ViewMode::Prisoner;
        app.ui_mut().scroll = 3;
        app.sync_selection();
        assert!(app.ui().selected_prisoner.is_some());
        app.ui_mut().scroll = u16::MAX;
        app.sync_selection();
        assert_eq!(
            app.ui().selected_prisoner,
            app.driver().prisoners().last().map(|p| p.id.0)
        );
    }

    #[test]
    fn all_quit_keys_work_and_other_ctrl_keys_are_ignored() {
        for code in [KeyCode::Char('q'), KeyCode::Char('Q'), KeyCode::Esc] {
            let mut quit_app = app();
            quit_app.handle_key(code, KeyModifiers::NONE);
            assert!(quit_app.should_quit, "{code:?} should quit");
        }
        let mut ctrl_c_app = app();
        ctrl_c_app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(ctrl_c_app.should_quit, "Ctrl-C should quit");

        let mut other_app = app();
        other_app.handle_key(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert!(!other_app.should_quit, "unrelated Ctrl key must not quit");
        other_app.handle_key(KeyCode::Char('z'), KeyModifiers::NONE);
        assert!(!other_app.should_quit, "unbound key must not quit");
    }

    #[test]
    fn every_view_has_a_key() {
        let mut app = app();
        for i in 1..=9u8 {
            let key = KeyCode::Char(char::from(b'0' + i));
            app.handle_key(key, KeyModifiers::NONE);
            assert_eq!(app.ui().mode, views::mode_from_index(usize::from(i) - 1).unwrap());
        }
        app.handle_key(KeyCode::Char('0'), KeyModifiers::NONE);
        assert_eq!(app.ui().mode, ViewMode::IncidentReports);
        app.handle_key(KeyCode::Char('i'), KeyModifiers::NONE);
        assert_eq!(app.ui().mode, ViewMode::IncidentReports);
        app.handle_key(KeyCode::Char('A'), KeyModifiers::NONE);
        assert_eq!(app.ui().mode, ViewMode::DecisionAudit);
    }

    #[test]
    fn alias_keys_adjust_speed_and_cursor() {
        let mut app = app();
        let base = app.ticks_per_second();
        app.handle_key(KeyCode::Char('='), KeyModifiers::NONE);
        assert_eq!(app.ticks_per_second(), base * 2);
        app.handle_key(KeyCode::Char('_'), KeyModifiers::NONE);
        assert_eq!(app.ticks_per_second(), base);

        for (key, expected) in [
            (KeyCode::Char('h'), 0),
            (KeyCode::Char('k'), 0),
            (KeyCode::Char('l'), 2),
            (KeyCode::Char('j'), 2),
            (KeyCode::Left, 0),
            (KeyCode::Up, 0),
            (KeyCode::Right, 2),
            (KeyCode::Down, 2),
        ] {
            app.ui_mut().scroll = 1;
            app.handle_key(key, KeyModifiers::NONE);
            assert_eq!(
                app.ui().scroll, expected,
                "key {key:?} did not move the cursor as documented"
            );
        }
    }

    #[test]
    fn cursor_does_not_wrap_past_u16_max() {
        let mut app = app();
        app.ui_mut().scroll = u16::MAX;
        app.handle_key(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.ui().scroll, u16::MAX);
        app.handle_key(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.ui().scroll, u16::MAX - 1);
    }
}
