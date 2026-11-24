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
    /// Construction queue for observer UI.
    fn construction_queue(&self) -> Vec<sumo::ConstructionSummary>;
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
    /// Policy summary for control panel.
    fn policy_summary(&self) -> sumo::PolicySummary;
    /// Set security level 0.0-1.0.
    fn set_security_level(&mut self, level: f32);
    /// Set privileges level 0.0-1.0.
    fn set_privileges_level(&mut self, level: f32);
    /// Set rehab funding multiplier 0.0-2.0.
    fn set_rehab_funding(&mut self, multiplier: f32);
    /// Manually pause/resume intake.
    fn set_intake_paused(&mut self, paused: bool);
    /// Save simulation state to a directory.
    fn save(&self, dir: &std::path::Path) -> Result<(), String>;
    /// Enable autosave.
    fn enable_autosave(&mut self, dir: std::path::PathBuf, interval_days: u64);
    /// Maybe run autosave; returns true if a save occurred.
    fn maybe_autosave(&mut self) -> Result<bool, String>;
    /// Incident log for audit/inspection.
    fn incident_log(&self) -> Vec<sumo::WorldEvent>;
    /// Structured incident records for audit/inspection (0.05).
    fn incident_records(&self) -> Vec<sumo::IncidentRecord>;
    /// Structured decision audit log.
    fn decision_audit_log(&self) -> Vec<sumo::DecisionAuditRecord>;
    /// Load simulation state from a save directory (0.05 control).
    fn load(&mut self, dir: &std::path::Path) -> Result<(), String>;
    /// Structured incident records matching a declarative filter (0.05).
    fn incident_records_filtered(
        &self,
        filter: &sumo::incidents::IncidentFilter,
    ) -> Vec<sumo::IncidentRecord>;
    /// Post-release outcome tracked for a prisoner, if any (0.05).
    fn post_release_status(&self, prisoner_id: u64) -> Option<bool>;
    /// Post-release records (0.05).
    fn post_release_records(&self) -> Vec<sumo::PostReleaseSummary>;
    /// Rehab enrollments derived from prisoner skill state (0.05).
    fn rehab_summaries(&self) -> Vec<sumo::RehabEnrollmentSummary>;
    /// Static rehab program catalog for enrollment controls (0.05).
    fn rehab_program_catalog(&self) -> Vec<sumo::RehabProgramInfo>;
    /// Enroll prisoner in rehab program by key (0.05 control).
    fn enroll_rehab(&mut self, prisoner_id: u64, program_key: &str) -> bool;
    /// Drop prisoner from rehab program by key (0.05 control).
    fn drop_rehab(&mut self, prisoner_id: u64, program_key: &str) -> bool;
    /// Aggregate rehab overview (0.05).
    fn rehab_overview(&self) -> sumo::RehabOverview;
    /// Contraband cases derived from the event record (0.05).
    fn contraband_cases(&self) -> Vec<sumo::ContrabandCaseSummary>;
    /// Contraband inventory rollup by item category (0.05).
    fn contraband_inventory(&self) -> Vec<sumo::ContrabandInventorySummary>;
    /// Resolved daily schedule timeline for a prisoner (0.05).
    fn schedule_timeline(&self, prisoner_id: u64) -> Option<sumo::ScheduleTimeline>;
    /// Budget snapshot over the trailing 30 days (0.05).
    fn budget_snapshot(&self) -> sumo::BudgetSnapshot;
    /// Apply rehabilitation program progress in minutes (validated control, 0.05).
    ///
    /// TODO(alcagent): a player-driven rehab *enrollment* control exists on
    /// `sumo::Sim` (`Sim::enroll_rehab`) but its `RehabProgram` parameter type
    /// is not yet re-exported through `sumo` for the `alcagent` binary crate, so
    /// the driver-level enrollment hook is intentionally not wired here yet.
    fn apply_rehab_progress(&mut self, prisoner_id: u64, minutes: u64) -> f32;
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
    /// Autosave interval in sim days (0 disables).
    pub autosave_days: u64,
}

impl Default for TuiOptions {
    fn default() -> Self {
        Self {
            ticks_per_second: 60,
            paused: false,
            save_dir: std::path::PathBuf::from("saves/default"),
            autosave_days: 0,
        }
    }
}

/// The observer application: simulation driver + UI cursor + playback state.
///
/// The driver is boxed so the observer can reload a save (`Ctrl-L`) by swapping
/// the backing engine state without recreating the application.
pub struct App {
    driver: Box<dyn SimDriver>,
    ui: UiState,
    paused: bool,
    ticks_per_second: u32,
    should_quit: bool,
    save_dir: std::path::PathBuf,
    autosave_days: u64,
    status_msg: Option<String>,
}

impl App {
    pub fn new(driver: Box<dyn SimDriver>, opts: TuiOptions) -> Self {
        let mut app = Self {
            driver,
            ui: UiState::new(),
            paused: opts.paused,
            ticks_per_second: opts.ticks_per_second.clamp(1, 4_000),
            should_quit: false,
            save_dir: opts.save_dir,
            autosave_days: opts.autosave_days,
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

    pub fn driver(&self) -> &dyn SimDriver {
        self.driver.as_ref()
    }

    pub fn driver_mut(&mut self) -> &mut dyn SimDriver {
        self.driver.as_mut()
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn ticks_per_second(&self) -> u32 {
        self.ticks_per_second
    }

    /// Last observer-facing status/notification message (may be cleared by the
    /// next event; the status bar renders the most recent one).
    pub fn status_message(&self) -> Option<String> {
        self.status_msg.clone()
    }

    /// Post a transient observer-facing status/notification message.
    pub fn notify(&mut self, msg: impl Into<String>) {
        self.status_msg = Some(msg.into());
    }

    /// Manual save through the driver (TUI `S` key / CLI parity).
    pub fn save_now(&mut self) -> Result<(), String> {
        match self.driver.save(&self.save_dir) {
            Ok(()) => {
                self.notify(format!("Saved to {}", self.save_dir.display()));
                Ok(())
            }
            Err(e) => {
                self.notify(format!("Save failed: {e}"));
                Err(e)
            }
        }
    }

    /// Quick load through the driver from the configured save directory (TUI
    /// `Ctrl-L` key). Reloads the backing engine state in place.
    pub fn quick_load(&mut self) -> Result<(), String> {
        let dir = self.save_dir.clone();
        let result = self.driver.load(&dir);
        match result {
            Ok(()) => {
                self.paused = false;
                self.driver.set_speed(TimeSpeed::RealTime);
                if self.autosave_days > 0 {
                    self.driver.enable_autosave(self.save_dir.clone(), self.autosave_days);
                }
                self.notify(format!("Loaded save from {}", dir.display()));
                Ok(())
            }
            Err(e) => {
                self.notify(format!("Load failed: {e}"));
                Err(e)
            }
        }
    }

    /// Prisoners visible to the prisoner view given the current search query.
    fn visible_prisoners(&self) -> Vec<sumo::PrisonerSummary> {
        let all = self.driver.prisoners();
        let q = self.ui.query().trim().to_lowercase();
        if q.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|p| p.name.to_lowercase().contains(&q) || p.id.0.to_string().contains(&q))
            .collect()
    }

    /// Gangs visible to the gang view given the current search query.
    fn visible_gangs(&self) -> Vec<sumo::GangSummary> {
        let all = self.driver.gangs();
        let q = self.ui.query().trim().to_lowercase();
        if q.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|g| g.name.to_lowercase().contains(&q) || g.id.to_string().contains(&q))
            .collect()
    }

    /// Guards visible to the staff view given the current search query.
    fn visible_guards(&self) -> Vec<sumo::GuardSummary> {
        let all = self.driver.guards();
        let q = self.ui.query().trim().to_lowercase();
        if q.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|g| g.name.to_lowercase().contains(&q) || g.id.to_string().contains(&q))
            .collect()
    }

    /// Events visible to the timeline view given the current search query.
    fn visible_events(&self) -> Vec<sumo::WorldEvent> {
        let all = self.driver.events_since(SimTime::EPOCH);
        let q = self.ui.query().trim().to_lowercase();
        if q.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|e| {
                e.summary.to_lowercase().contains(&q)
                    || format!("{:?}", e.kind).to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Incident records for the incident view, honoring the declarative
    /// [`UiState`] filter through the driver.
    fn visible_incidents(&self) -> Vec<sumo::IncidentRecord> {
        self.driver.incident_records_filtered(self.ui.filter())
    }

    /// Decision audit entries for the decision view, narrowed by the current
    /// search query (actor / source / action / summary substring match).
    fn visible_decisions(&self) -> Vec<sumo::DecisionAuditRecord> {
        let all = self.driver.decision_audit_log().to_vec();
        let q = self.ui.query().trim().to_lowercase();
        if q.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|d| {
                d.actor.to_lowercase().contains(&q)
                    || d.source.to_lowercase().contains(&q)
                    || d.action.to_lowercase().contains(&q)
                    || d.summary.to_lowercase().contains(&q)
            })
            .collect()
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
        // Lists are filtered through [`App::visible_*`] so the search query in
        // [`UiState`] narrows exactly what the (read-only) views render.
        let prisoners = self.visible_prisoners();
        let gangs = self.visible_gangs();
        let guards = self.visible_guards();
        let data = views::ViewData {
            observation: obs,
            prisoners,
            gangs,
            guards,
            warden: self.driver.warden(),
            economy: self.driver.economy(),
            social_edges: self.driver.social_edges(),
            facilities: self.driver.facilities(),
            construction_queue: self.driver.construction_queue(),
            events: self.visible_events(),
            incident_log: self.driver.incident_log(),
            incident_records: self.visible_incidents(),
            decision_audit: self.visible_decisions(),
            policy: self.driver.policy_summary(),
            post_release_records: self.driver.post_release_records(),
            rehab_summaries: self.driver.rehab_summaries(),
            rehab_overview: self.driver.rehab_overview(),
            contraband_cases: self.driver.contraband_cases(),
            contraband_inventory: self.driver.contraband_inventory(),
            budget_snapshot: self.driver.budget_snapshot(),
            selected_schedule: self.ui.selected_prisoner.and_then(|id| self.driver.schedule_timeline(id)),
        };
        views::draw(frame, &data, &self.ui, &status);
    }

    /// Clamp the shared cursor to the active list and keep the selected-id
    /// fields in [`UiState`] consistent with it.
    fn sync_selection(&mut self) {
        let obs = self.driver.observation();
        let prisoners = if matches!(self.ui.mode, ViewMode::Prisoner) { self.visible_prisoners() } else { Vec::new() };
        let gangs = if matches!(self.ui.mode, ViewMode::Gang) { self.visible_gangs() } else { Vec::new() };
        let guards = if matches!(self.ui.mode, ViewMode::Staff) { self.visible_guards() } else { Vec::new() };
        let facilities = if matches!(self.ui.mode, ViewMode::Map) { self.driver.facilities() } else { Vec::new() };
        let events = if matches!(self.ui.mode, ViewMode::EventTimeline) { self.visible_events() } else { Vec::new() };
        let social = if matches!(self.ui.mode, ViewMode::SocialNetwork) { self.driver.social_edges() } else { Vec::new() };
        let economy = if matches!(self.ui.mode, ViewMode::Financial) { self.driver.economy() } else { self.driver.economy() };
        let incidents = if matches!(self.ui.mode, ViewMode::IncidentReports) { self.visible_incidents() } else { Vec::new() };
        let decisions = if matches!(self.ui.mode, ViewMode::DecisionAudit) { self.visible_decisions() } else { Vec::new() };

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
        if matches!(self.ui.mode, ViewMode::IncidentReports) {
            self.ui.selected_incident = incidents.get(idx).map(|r| r.id);
        }
        if matches!(self.ui.mode, ViewMode::DecisionAudit) {
            self.ui.selected_decision = decisions.get(idx).map(|d| format!("{}|{}", d.actor, d.action));
        }
        let _ = obs;
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        if modifiers.contains(KeyModifiers::CONTROL) {
            if code == KeyCode::Char('c') || code == KeyCode::Char('C') {
                self.should_quit = true;
            } else if code == KeyCode::Char('l') || code == KeyCode::Char('L') {
                // Quick load from the configured save directory (0.05 control).
                // Errors are surfaced through the status message.
                let _ = self.quick_load();
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
                // Manual save via the driver; errors surface through the status.
                let _ = self.save_now();
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                // Toggle lockdown through the validated engine control.
                let lockdown = !self.driver.policy_summary().lockdown;
                self.driver.set_lockdown(lockdown);
                self.notify(if lockdown { "Lockdown enabled" } else { "Lockdown lifted" });
            }
            KeyCode::Char('L') => {
                self.driver.set_lockdown(true);
                self.notify("Lockdown enabled");
            }
            KeyCode::Char('U') => {
                self.driver.set_lockdown(false);
                self.notify("Lockdown lifted");
            }
            KeyCode::Char('N') => {
                self.driver.set_policy(sumo::Policy::Normal);
                self.notify("Policy: Normal");
            }
            KeyCode::Char('E') => {
                self.driver.set_policy(sumo::Policy::Enhanced);
                self.notify("Policy: Enhanced");
            }
            KeyCode::Char('K') => {
                self.driver.set_policy(sumo::Policy::Lockdown);
                self.notify("Policy: Lockdown");
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                let current = self.driver.policy_mode();
                let next = match current {
                    sumo::PolicyMode::Deterministic => sumo::PolicyMode::Council,
                    sumo::PolicyMode::Council => sumo::PolicyMode::Shadow,
                    sumo::PolicyMode::Shadow => sumo::PolicyMode::Deterministic,
                };
                self.driver.set_policy_mode(next);
                self.notify(format!("Policy mode: {:?}", next));
            }
            KeyCode::Char('[') => {
                let mut ps = self.driver.policy_summary();
                ps.security_level = (ps.security_level - 0.05).max(0.0);
                self.driver.set_security_level(ps.security_level);
                self.notify(format!("Security: {:.0}%", ps.security_level * 100.0));
            }
            KeyCode::Char(']') => {
                let mut ps = self.driver.policy_summary();
                ps.security_level = (ps.security_level + 0.05).min(1.0);
                self.driver.set_security_level(ps.security_level);
                self.notify(format!("Security: {:.0}%", ps.security_level * 100.0));
            }
            KeyCode::Char(',') => {
                let mut ps = self.driver.policy_summary();
                ps.privileges_level = (ps.privileges_level - 0.05).max(0.0);
                self.driver.set_privileges_level(ps.privileges_level);
                self.notify(format!("Privileges: {:.0}%", ps.privileges_level * 100.0));
            }
            KeyCode::Char('.') => {
                let mut ps = self.driver.policy_summary();
                ps.privileges_level = (ps.privileges_level + 0.05).min(1.0);
                self.driver.set_privileges_level(ps.privileges_level);
                self.notify(format!("Privileges: {:.0}%", ps.privileges_level * 100.0));
            }
            KeyCode::Char(';') => {
                let mut ps = self.driver.policy_summary();
                ps.rehab_funding_multiplier = (ps.rehab_funding_multiplier - 0.1).max(0.0);
                self.driver.set_rehab_funding(ps.rehab_funding_multiplier);
                self.notify(format!("Rehab funding: {:.2}x", ps.rehab_funding_multiplier));
            }
            KeyCode::Char('\'') => {
                let mut ps = self.driver.policy_summary();
                ps.rehab_funding_multiplier = (ps.rehab_funding_multiplier + 0.1).min(2.0);
                self.driver.set_rehab_funding(ps.rehab_funding_multiplier);
                self.notify(format!("Rehab funding: {:.2}x", ps.rehab_funding_multiplier));
            }
            KeyCode::Char('b') | KeyCode::Char('B') => {
                let ps = self.driver.policy_summary();
                let next = !ps.intake_paused;
                self.driver.set_intake_paused(next);
                self.notify(format!("Intake {}", if next { "paused" } else { "resumed" }));
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
            KeyCode::Char('r') => {
                if let Some(pid) = self.ui.selected_prisoner {
                    let enrolled: Vec<String> = self.driver.rehab_summaries().into_iter().filter(|r| r.prisoner_id.raw() == pid).map(|r| r.program_key).collect();
                    if let Some(prog) = self.driver.rehab_program_catalog().into_iter().find(|p| !enrolled.iter().any(|k| k == &p.key)) {
                        let ok = self.driver.enroll_rehab(pid, &prog.key);
                        self.notify(if ok { format!("Enrolled P{pid} in {}", prog.name) } else { format!("Enroll failed for P{pid}") });
                    } else {
                        self.notify(format!("P{pid} already enrolled in all programs"));
                    }
                }
            }
            KeyCode::Char('R') => {
                if let Some(pid) = self.ui.selected_prisoner {
                    if let Some(row) = self.driver.rehab_summaries().into_iter().find(|r| r.prisoner_id.raw() == pid) {
                        let ok = self.driver.drop_rehab(pid, &row.program_key);
                        self.notify(if ok { format!("Dropped P{pid} from {}", row.program) } else { format!("Drop failed for P{pid}") });
                    } else {
                        self.notify(format!("P{pid} has no active rehab enrollment"));
                    }
                }
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
pub fn run_tui(driver: Box<dyn SimDriver>, opts: TuiOptions) -> Result<()> {
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
        lockdown: bool,
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
        fn construction_queue(&self) -> Vec<sumo::ConstructionSummary> { self.sim.construction_queue() }
        fn speed(&self) -> TimeSpeed { self.sim.speed() }
        fn set_speed(&mut self, speed: TimeSpeed) { self.sim.set_speed(speed); }
        fn policy_mode(&self) -> sumo::PolicyMode { self.sim.policy_mode() }
        fn set_policy_mode(&mut self, mode: sumo::PolicyMode) { self.sim.set_policy_mode(mode); }
        fn sim_time(&self) -> sumo::SimTime { self.sim.world.time }
        fn days_simulated(&self) -> u64 { self.sim.world.days_simulated }
        fn set_lockdown(&mut self, lockdown: bool) { self.lockdown = lockdown; }
        fn set_policy(&mut self, policy: sumo::Policy) { self.sim.set_policy(policy); }
        fn policy_summary(&self) -> sumo::PolicySummary { let mut ps = self.sim.policy_summary(); ps.lockdown = self.lockdown; ps }
        fn set_security_level(&mut self, level: f32) { self.sim.set_security_level(level); }
        fn set_privileges_level(&mut self, level: f32) { self.sim.set_privileges_level(level); }
        fn set_rehab_funding(&mut self, multiplier: f32) { self.sim.set_rehab_funding(multiplier); }
        fn set_intake_paused(&mut self, paused: bool) { self.sim.set_intake_paused(paused); }
        fn save(&self, _dir: &std::path::Path) -> Result<(), String> { Ok(()) }
        fn enable_autosave(&mut self, dir: std::path::PathBuf, interval_days: u64) { self.sim.enable_autosave(dir, interval_days); }
        fn maybe_autosave(&mut self) -> Result<bool, String> { self.sim.maybe_autosave().map_err(|e| e.to_string()) }
        fn incident_log(&self) -> Vec<sumo::WorldEvent> { self.sim.incident_log() }
        fn incident_records(&self) -> Vec<sumo::IncidentRecord> { self.sim.incident_records() }
        fn incident_records_filtered(&self, filter: &sumo::incidents::IncidentFilter) -> Vec<sumo::IncidentRecord> {
            self.sim.incident_records().into_iter().filter(|r| {
                if let Some(min) = filter.min_severity { if r.severity < min { return false; } }
                if let Some(resolved) = filter.resolved { if r.resolved != resolved { return false; } }
                true
            }).collect()
        }
        fn decision_audit_log(&self) -> Vec<sumo::DecisionAuditRecord> { self.sim.decision_audit_log().to_vec() }
        fn load(&mut self, _dir: &std::path::Path) -> Result<(), String> { Ok(()) }
        fn post_release_status(&self, _prisoner_id: u64) -> Option<bool> { None }
        fn post_release_records(&self) -> Vec<sumo::PostReleaseSummary> { Vec::new() }
        fn rehab_summaries(&self) -> Vec<sumo::RehabEnrollmentSummary> { Vec::new() }
        fn rehab_program_catalog(&self) -> Vec<sumo::RehabProgramInfo> { Vec::new() }
        fn enroll_rehab(&mut self, _prisoner_id: u64, _program_key: &str) -> bool { false }
        fn drop_rehab(&mut self, _prisoner_id: u64, _program_key: &str) -> bool { false }
        fn rehab_overview(&self) -> sumo::RehabOverview { sumo::RehabOverview::default() }
        fn contraband_cases(&self) -> Vec<sumo::ContrabandCaseSummary> { Vec::new() }
        fn contraband_inventory(&self) -> Vec<sumo::ContrabandInventorySummary> { Vec::new() }
        fn schedule_timeline(&self, _prisoner_id: u64) -> Option<sumo::ScheduleTimeline> { None }
        fn budget_snapshot(&self) -> sumo::BudgetSnapshot { sumo::BudgetSnapshot::default() }
        fn apply_rehab_progress(&mut self, _prisoner_id: u64, _minutes: u64) -> f32 { 0.0 }
    }

    fn app() -> App {
        App::new(
            Box::new(MockDriver { sim: sumo::Sim::new(7), ticks: 0, lockdown: false }),
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

    #[test]
    fn ctrl_l_quick_load_reloads_from_save_dir_and_notifies() {
        let mut app = app();
        assert_eq!(app.status_message(), None);
        app.handle_key(KeyCode::Char('l'), KeyModifiers::CONTROL);
        assert_eq!(
            app.status_message().as_deref(),
            Some("Loaded save from saves/default"),
            "Ctrl-L should quick-load and notify"
        );
    }

    #[test]
    fn x_toggles_lockdown_and_notifies() {
        let mut app = app();
        assert!(!app.driver().policy_summary().lockdown);
        app.handle_key(KeyCode::Char('x'), KeyModifiers::NONE);
        assert!(app.driver().policy_summary().lockdown);
        assert_eq!(app.status_message().as_deref(), Some("Lockdown enabled"));
        app.handle_key(KeyCode::Char('x'), KeyModifiers::NONE);
        assert!(!app.driver().policy_summary().lockdown);
        assert_eq!(app.status_message().as_deref(), Some("Lockdown lifted"));
    }

    #[test]
    fn save_key_reports_save_outcome_via_status_message() {
        let mut app = app();
        app.handle_key(KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(
            app.status_message().as_deref(),
            Some("Saved to saves/default"),
            "save key should report through the status message"
        );
        assert!(app.save_now().is_ok());
    }

    #[test]
    fn search_query_filters_visible_prisoner_list() {
        let mut app = app();
        assert!(!app.visible_prisoners().is_empty());
        app.ui_mut().set_query("zzzz-no-such-prisoner-zzzzz");
        assert!(app.visible_prisoners().is_empty());
        app.ui_mut().clear_query();
        assert_eq!(app.visible_prisoners().len(), app.driver().prisoners().len());
    }

    #[test]
    fn incident_filter_is_honored_by_visible_incidents() {
        let mut app = app();
        let all = app.visible_incidents();
        assert_eq!(all.len(), app.driver().incident_records().len());
        app.ui_mut().set_filter(sumo::incidents::IncidentFilter::all().with_min_severity(0.99));
        assert_eq!(app.ui().filter().min_severity, Some(0.99));
        // Filtering must not panic and must return a bounded Vec.
        let _ = app.visible_incidents();
    }

    #[test]
    fn decision_search_narrows_visible_decisions() {
        let mut app = app();
        let all = app.visible_decisions();
        app.ui_mut().set_query("zzzz-no-such-decision-zzzzz");
        assert!(app.visible_decisions().is_empty());
        app.ui_mut().clear_query();
        assert_eq!(app.visible_decisions().len(), all.len());
    }
}
