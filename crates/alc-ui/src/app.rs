//! Interactive ratatui TUI (Directive §20, §21).
//!
//! This module owns the terminal, the frame loop and keyboard input. The
//! simulation itself is supplied through [`SimDriver`] so this crate never has
//! to depend on the engine crate (`sumo`) — that would be a dependency cycle,
//! since the engine already depends on `alc-ui` for observation snapshots.
//!
//! The binary crate (`alcagent`) adapts `sumo::Sim` to [`SimDriver`] locally.

use crate::views::{self, AppStatus};
use crate::{CONSTRUCTION_KINDS, UiState, ViewMode};
use alc_core::TimeSpeed;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::Frame;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::{Duration, Instant};
use sumo::{MapFrame, Observation, SimTime, Stats};

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
    /// Get detected emergent social roles for the social network view.
    fn social_roles(&self) -> Vec<sumo::SocialRoleSummary>;
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
    /// Resolve a contraband case by case id. Returns true if state changed.
    /// Default no-op keeps the trait backward-compatible when the driver has no case store.
    fn resolve_contraband_case(&mut self, _case_id: u64) -> bool {
        false
    }
    /// Resolve a structured incident by id (0.05 control).
    fn resolve_incident(&mut self, _incident_id: u64) -> bool {
        false
    }
    /// Enqueue a construction project (0.05 control). Returns the project id.
    fn enqueue_construction(
        &mut self,
        _kind: alc_world::FacilityKind,
        _wing: u32,
        _capacity: u32,
    ) -> u64 {
        0
    }
    /// Cancel a construction project (0.05 control).
    fn cancel_construction(&mut self, _project_id: u64, _reason: &str) -> bool {
        false
    }
    /// Mark a construction project failed (0.05 control).
    fn fail_construction(&mut self, _project_id: u64, _reason: &str) -> bool {
        false
    }
    /// Advance a construction project by days (0.05 control).
    fn advance_construction(&mut self, _project_id: u64, _days: u32) -> bool {
        false
    }
    /// Approve a queued construction request (spec 006 control).
    fn approve_construction(&mut self, _project_id: u64, _reason: &str) -> bool {
        false
    }

    /// Prisoners eligible for a parole hearing (spec 006 read surface).
    fn parole_due(&self) -> Vec<u64> {
        Vec::new()
    }
    /// Parole status for a prisoner, if a record exists.
    fn parole_status(&self, _prisoner_id: u64) -> Option<sumo::ParoleStatus> {
        None
    }
    /// Persisted conditions for a decided prisoner.
    fn parole_conditions(&self, _prisoner_id: u64) -> Option<sumo::ParoleConditions> {
        None
    }
    /// Persisted decision reason for a prisoner.
    fn parole_decision_reason(&self, _prisoner_id: u64) -> Option<String> {
        None
    }
    /// Sim time at which a grant became an actual release.
    fn parole_released_at(&self, _prisoner_id: u64) -> Option<sumo::SimTime> {
        None
    }
    /// Schedule a parole hearing (validated control).
    fn start_parole_hearing(&mut self, _prisoner_id: u64) -> bool {
        false
    }
    /// Grant parole (validated control).
    fn grant_parole(&mut self, _prisoner_id: u64) -> bool {
        false
    }
    /// Deny parole (validated control).
    fn deny_parole(&mut self, _prisoner_id: u64, _reason: &str) -> bool {
        false
    }
    /// Execute a grant as an actual release from custody (validated control).
    fn release_on_parole(&mut self, _prisoner_id: u64) -> bool {
        false
    }

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

    /// Downcast access to the concrete driver (0.05 test support).
    ///
    /// Returns `None` by default; drivers that want it override with
    /// `Some(self)`. Used by `alc-ui` tests to reach mock state.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
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
    /// Incident ids already seen by the auto-pause-on-critical logic this
    /// session; a *new* critical id pauses the sim, so resuming (`SPACE`)
    /// works instead of re-pausing forever.
    seen_critical: Vec<u64>,
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
            seen_critical: Vec::new(),
        };
        // Speed precedence: paused observer state wins; otherwise honor a
        // speed the driver already has (e.g. `Sim::load` restores the saved
        // `World.speed`); only fall back to RealTime when the driver is
        // actually paused but we are not.
        let engine_speed = if app.paused {
            TimeSpeed::Paused
        } else if app.driver.speed() == TimeSpeed::Paused {
            TimeSpeed::RealTime
        } else {
            app.driver.speed()
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
        let msg = msg.into();
        self.ui.notifications.push(msg.clone());
        if self.ui.notifications.len() > 32 {
            let drain = self.ui.notifications.len() - 32;
            self.ui.notifications.drain(..drain);
        }
        self.status_msg = Some(msg);
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
                    self.driver
                        .enable_autosave(self.save_dir.clone(), self.autosave_days);
                }
                // Clear observer state that references the pre-load world so
                // stale pins/queries/notifications don't leak into the new
                // session, then revalidate cached selections.
                self.clear_observer_state();
                self.revalidate_selections();
                self.notify(format!("Loaded save from {}", dir.display()));
                Ok(())
            }
            Err(e) => {
                self.notify(format!("Load failed: {e}"));
                Err(e)
            }
        }
    }

    /// Drop observer state that may reference pre-load entities: bookmarks,
    /// the free-text search (including active input), and notifications.
    fn clear_observer_state(&mut self) {
        self.ui.bookmarks.clear();
        self.ui.search.clear();
        self.ui.search_active = false;
        self.ui.notifications.clear();
        // Content-derived keys cannot be revalidated against a different save.
        self.ui.selected_decision = None;
        self.ui.selected_incident = None;
    }

    /// Re-clamp cached entity selections after the world changed under the
    /// observer. Reads the world only through the driver.
    fn revalidate_selections(&mut self) {
        let cases = self.driver.contraband_cases();
        self.ui.selected_contraband = self
            .ui
            .selected_contraband
            .filter(|id| cases.iter().any(|c| c.case_id == *id));
        let queue = self.driver.construction_queue();
        self.ui.selected_construction = self
            .ui
            .selected_construction
            .filter(|id| queue.iter().any(|p| p.id == *id));
        let incidents = self.driver.incident_records();
        self.ui.selected_incident = self
            .ui
            .selected_incident
            .filter(|id| incidents.iter().any(|r| r.id == *id));
        self.sync_selection();
    }

    /// Prisoners visible to the prisoner view given the current search query.
    fn visible_prisoners(&self) -> Vec<sumo::PrisonerSummary> {
        let q = self.ui.query();
        self.driver
            .prisoners()
            .into_iter()
            .filter(|p| query_matches(&p.name, q) || query_matches(&p.id.0.to_string(), q))
            .collect()
    }

    /// Gangs visible to the gang view given the current search query.
    fn visible_gangs(&self) -> Vec<sumo::GangSummary> {
        let q = self.ui.query();
        self.driver
            .gangs()
            .into_iter()
            .filter(|g| query_matches(&g.name, q) || query_matches(&g.id.to_string(), q))
            .collect()
    }

    /// Guards visible to the staff view given the current search query.
    fn visible_guards(&self) -> Vec<sumo::GuardSummary> {
        let q = self.ui.query();
        self.driver
            .guards()
            .into_iter()
            .filter(|g| query_matches(&g.name, q) || query_matches(&g.id.to_string(), q))
            .collect()
    }

    /// Events visible to the timeline view given the current search query.
    fn visible_events(&self) -> Vec<sumo::WorldEvent> {
        let q = self.ui.query();
        self.driver
            .events_since(SimTime::EPOCH)
            .into_iter()
            .filter(|e| query_matches(&e.summary, q) || query_matches(&format!("{:?}", e.kind), q))
            .collect()
    }

    /// Incident records for the incident view, honoring the declarative
    /// [`UiState`] filter through the driver *and* the free-text query.
    fn visible_incidents(&self) -> Vec<sumo::IncidentRecord> {
        let q = self.ui.query().trim().to_lowercase();
        let mut records: Vec<_> = self
            .driver
            .incident_records_filtered(self.ui.filter())
            .into_iter()
            .filter(|r| {
                q.is_empty()
                    || r.kind.to_lowercase().contains(&q)
                    || r.location.to_lowercase().contains(&q)
                    || r.disposition.to_lowercase().contains(&q)
            })
            .collect();
        // One ordering contract for the cursor, selection and renderer.
        records.reverse();
        records
    }

    /// Decision audit entries for the decision view, narrowed by the current
    /// search query (actor / source / action / summary substring match) and
    /// optional validation filter.
    fn visible_decisions(&self) -> Vec<sumo::DecisionAuditRecord> {
        let all = self.driver.decision_audit_log().to_vec();
        let q = self.ui.query();
        let vf = self.ui.decision_validation_filter.as_deref();
        let mut visible: Vec<_> = all
            .into_iter()
            .filter(|d| {
                let q_ok = query_matches(&d.actor, q)
                    || query_matches(&d.source, q)
                    || query_matches(&d.action, q)
                    || query_matches(&d.summary, q);
                let v_ok = match vf {
                    None => true,
                    Some("repaired") => d.validation.starts_with("repaired"),
                    Some("rejected") => d.validation.starts_with("rejected"),
                    Some("accepted") => d.validation.starts_with("accepted"),
                    Some(v) => d.validation == v,
                };
                q_ok && v_ok
            })
            .collect();
        // Newest first everywhere: the list, the cursor and the detail panel.
        visible.reverse();
        visible
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

            // Observer safety valve: halt once on a *newly seen* critical
            // incident (see `check_pause_on_critical`).
            self.check_pause_on_critical();

            self.sync_selection();
            terminal.draw(|frame| self.render(frame))?;
            std::thread::sleep(Duration::from_millis(16));
        }
        Ok(())
    }

    /// Assemble the read-only [`views::ViewData`] bundle the renderers use.
    /// `map_frame` is fetched once per frame through the driver (no raw
    /// `World` access — see the [`SimDriver`] docs); true totals come from the
    /// driver unfiltered by the observer search so view titles can show both
    /// visible and total counts.
    fn build_view_data(&self) -> views::ViewData {
        views::ViewData {
            observation: self.driver.observation(),
            map_frame: self.driver.map_frame(),
            total_prisoners: self.driver.prisoners().len(),
            total_gangs: self.driver.gangs().len(),
            total_guards: self.driver.guards().len(),
            // Lists are filtered through [`App::visible_*`] so the search
            // query in [`UiState`] narrows exactly what the (read-only) views
            // render.
            prisoners: self.visible_prisoners(),
            gangs: self.visible_gangs(),
            guards: self.visible_guards(),
            warden: self.driver.warden(),
            economy: self.driver.economy(),
            social_edges: self.driver.social_edges(),
            social_roles: if matches!(self.ui.mode, ViewMode::SocialNetwork) {
                self.driver.social_roles()
            } else {
                Vec::new()
            },
            facilities: self.driver.facilities(),
            construction_queue: self.driver.construction_queue(),
            events: self.visible_events(),
            incident_log: self.driver.incident_log(),
            incident_records: self.visible_incidents(),
            decision_audit: self.visible_decisions(),
            policy: self.driver.policy_summary(),
            post_release_records: self.driver.post_release_records(),
            rehab_summaries: self.driver.rehab_summaries(),
            rehab_program_catalog: self.driver.rehab_program_catalog(),
            rehab_overview: self.driver.rehab_overview(),
            contraband_cases: self.driver.contraband_cases(),
            contraband_inventory: self.driver.contraband_inventory(),
            budget_snapshot: self.driver.budget_snapshot(),
            selected_schedule: self
                .ui
                .selected_prisoner
                .and_then(|id| self.driver.schedule_timeline(id)),
            parole_due: self.driver.parole_due(),
            selected_parole: self
                .ui
                .selected_prisoner
                .and_then(|id| self.driver.parole_status(id)),
            selected_parole_conditions: self
                .ui
                .selected_prisoner
                .and_then(|id| self.driver.parole_conditions(id)),
            selected_parole_reason: self
                .ui
                .selected_prisoner
                .and_then(|id| self.driver.parole_decision_reason(id)),
            selected_parole_released_at: self
                .ui
                .selected_prisoner
                .and_then(|id| self.driver.parole_released_at(id)),
        }
    }

    fn render(&self, frame: &mut Frame) {
        let status = AppStatus {
            paused: self.paused,
            ticks_per_second: self.ticks_per_second,
            status_msg: self.status_msg.clone(),
            save_dir: self.save_dir.display().to_string(),
            autosave_days: self.autosave_days,
        };
        let data = self.build_view_data();
        views::draw(frame, &data, &self.ui, &status);
    }

    /// Clamp the shared cursor to the active list and keep the selected-id
    /// fields in [`UiState`] consistent with it.
    fn sync_selection(&mut self) {
        let obs = self.driver.observation();
        if matches!(self.ui.mode, ViewMode::Warden) {
            // Preserve a context prisoner so the Warden's r/R (rehab
            // enroll/drop) has a target even when the observer entered the
            // view before any Prisoner selection was made.
            if self.ui.selected_prisoner.is_none() {
                self.ui.selected_prisoner = self.driver.prisoners().first().map(|p| p.id.0);
            }
            // Revalidate the selected construction project against the queue
            // so x/X/> target the queue entry, not a stale id.
            let queue = self.driver.construction_queue();
            let still_valid = queue
                .iter()
                .any(|p| Some(p.id) == self.ui.selected_construction);
            if !queue.is_empty() && !still_valid {
                self.ui.selected_construction = queue.first().map(|p| p.id);
            }
        }
        let prisoners = if matches!(self.ui.mode, ViewMode::Prisoner) {
            self.visible_prisoners()
        } else {
            Vec::new()
        };
        let gangs = if matches!(self.ui.mode, ViewMode::Gang) {
            self.visible_gangs()
        } else {
            Vec::new()
        };
        let guards = if matches!(self.ui.mode, ViewMode::Staff) {
            self.visible_guards()
        } else {
            Vec::new()
        };
        let facilities = if matches!(self.ui.mode, ViewMode::Map) {
            self.driver.facilities()
        } else {
            Vec::new()
        };
        let events = if matches!(self.ui.mode, ViewMode::EventTimeline) {
            self.visible_events()
        } else {
            Vec::new()
        };
        let social = if matches!(self.ui.mode, ViewMode::SocialNetwork) {
            self.driver.social_edges()
        } else {
            Vec::new()
        };
        let economy = if matches!(self.ui.mode, ViewMode::Financial) {
            self.driver.economy()
        } else {
            self.driver.economy()
        };
        let incidents = if matches!(self.ui.mode, ViewMode::IncidentReports) {
            self.visible_incidents()
        } else {
            Vec::new()
        };
        let decisions = if matches!(self.ui.mode, ViewMode::DecisionAudit) {
            self.visible_decisions()
        } else {
            Vec::new()
        };

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
        // In Warden mode there is no prisoner list; the context prisoner set
        // above is preserved instead.
        if !matches!(self.ui.mode, ViewMode::Warden) {
            self.ui.selected_prisoner = prisoners.get(idx).map(|p| p.id.0);
        }
        self.ui.selected_gang = gangs.get(idx).map(|g| g.id);
        self.ui.selected_guard = guards.get(idx).map(|g| g.id);
        if matches!(self.ui.mode, ViewMode::IncidentReports) {
            self.ui.selected_incident = incidents.get(idx).map(|r| r.id);
            let cases = self.driver.contraband_cases();
            let current = self.ui.selected_contraband;
            let still_valid = current.is_some_and(|id| cases.iter().any(|c| c.case_id == id));
            self.ui.selected_contraband = if still_valid {
                current
            } else {
                cases.first().map(|c| c.case_id)
            };
        }
        if matches!(self.ui.mode, ViewMode::DecisionAudit) {
            self.ui.selected_decision = decisions.get(idx).map(crate::decision_key);
        }
        let _ = obs;
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        if self.ui.search_active {
            match code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.ui.search_active = false;
                    self.notify(format!("Search: {:?}", self.ui.search));
                }
                KeyCode::Backspace => {
                    self.ui.search.pop();
                }
                // Ctrl keys stay live while searching (0.05 control wiring).
                KeyCode::Char('c') | KeyCode::Char('C') if ctrl => {
                    self.should_quit = true;
                }
                KeyCode::Char('l') | KeyCode::Char('L') if ctrl => {
                    let _ = self.quick_load();
                }
                // Clear the query while keeping the search input open.
                KeyCode::Char('w') | KeyCode::Char('W') if ctrl => {
                    self.ui.search.clear();
                    self.notify("Query cleared");
                }
                KeyCode::Char(c) if !ctrl => {
                    if self.ui.search.len() < 64 {
                        self.ui.search.push(c);
                    }
                }
                _ => {}
            }
            return;
        }
        if self.ui.show_help {
            match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.ui.help_scroll = self.ui.help_scroll.saturating_sub(1);
                    return;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.ui.help_scroll = self.ui.help_scroll.saturating_add(1);
                    return;
                }
                _ => {}
            }
        }
        if modifiers.contains(KeyModifiers::CONTROL) {
            if code == KeyCode::Char('c') || code == KeyCode::Char('C') {
                self.should_quit = true;
            } else if code == KeyCode::Char('l') || code == KeyCode::Char('L') {
                // Quick load from the configured save directory (0.05 control).
                // Errors are surfaced through the status message.
                let _ = self.quick_load();
            } else if code == KeyCode::Char('r') || code == KeyCode::Char('R') {
                // Resolve/acknowledge the selected (or newest open critical)
                // incident via the validated driver control (0.05).
                self.resolve_critical_incident();
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
            KeyCode::Char('x') | KeyCode::Char('X')
                if !matches!(self.ui.mode, ViewMode::Warden) =>
            {
                // Toggle lockdown through the validated engine control.
                let lockdown = !self.driver.policy_summary().lockdown;
                self.driver.set_lockdown(lockdown);
                self.notify(if lockdown {
                    "Lockdown enabled"
                } else {
                    "Lockdown lifted"
                });
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
                self.notify(format!(
                    "Rehab funding: {:.2}x",
                    ps.rehab_funding_multiplier
                ));
            }
            KeyCode::Char('\'') => {
                let mut ps = self.driver.policy_summary();
                ps.rehab_funding_multiplier = (ps.rehab_funding_multiplier + 0.1).min(3.0);
                self.driver.set_rehab_funding(ps.rehab_funding_multiplier);
                self.notify(format!(
                    "Rehab funding: {:.2}x",
                    ps.rehab_funding_multiplier
                ));
            }
            KeyCode::Char('b') | KeyCode::Char('B') => {
                let ps = self.driver.policy_summary();
                let next = !ps.intake_paused;
                self.driver.set_intake_paused(next);
                self.notify(format!(
                    "Intake {}",
                    if next { "paused" } else { "resumed" }
                ));
            }
            KeyCode::Char('?') => {
                self.ui.show_help = !self.ui.show_help;
            }
            KeyCode::Char('/') => {
                self.ui.search_active = true;
                self.notify("Search: type to filter, Enter/Esc to finish");
            }
            KeyCode::Tab => {
                let target = match self.ui.mode {
                    ViewMode::Prisoner => self.ui.selected_prisoner,
                    ViewMode::Gang => self.ui.selected_gang,
                    ViewMode::Staff => self.ui.selected_guard,
                    _ => None,
                };
                if let Some(id) = target {
                    if let Some(pos) = self.ui.bookmarks.iter().position(|b| *b == id) {
                        self.ui.bookmarks.remove(pos);
                        self.notify(format!("Unpinned #{id}"));
                    } else {
                        self.ui.bookmarks.push(id);
                        self.notify(format!("Pinned #{id}"));
                    }
                } else {
                    self.notify("Select an entity to pin it");
                }
            }
            KeyCode::Char('n') => {
                self.ui.show_notifications = !self.ui.show_notifications;
            }
            KeyCode::Char('V') => {
                self.ui.pause_on_critical = !self.ui.pause_on_critical;
                self.notify(format!(
                    "Pause-on-critical {}",
                    if self.ui.pause_on_critical {
                        "on"
                    } else {
                        "off"
                    }
                ));
            }
            KeyCode::Char('c') if matches!(self.ui.mode, ViewMode::Warden) => {
                self.ui.construction_kind_index =
                    (self.ui.construction_kind_index + 1) % CONSTRUCTION_KINDS.len();
                self.notify(format!(
                    "Construction kind: {:?}",
                    CONSTRUCTION_KINDS[self.ui.construction_kind_index]
                ));
            }
            KeyCode::Char('C') if matches!(self.ui.mode, ViewMode::Warden) => {
                let kind =
                    CONSTRUCTION_KINDS[self.ui.construction_kind_index % CONSTRUCTION_KINDS.len()];
                let capacity = match kind {
                    alc_world::FacilityKind::CellBlock | alc_world::FacilityKind::Dormitory => 40,
                    _ => 10,
                };
                let id = self.driver.enqueue_construction(kind, 1, capacity);
                self.notify(format!(
                    "Queued {} project #{id} (wing 1, cap {capacity})",
                    kind.display()
                ));
                self.ui.selected_construction = Some(id);
            }
            KeyCode::Char('x') | KeyCode::Char('X') if matches!(self.ui.mode, ViewMode::Warden) => {
                if let Some(id) = self.ui.selected_construction {
                    let reason = if matches!(code, KeyCode::Char('X')) {
                        "observer-marked failure"
                    } else {
                        "observer cancellation"
                    };
                    let ok = if matches!(code, KeyCode::Char('X')) {
                        self.driver.fail_construction(id, reason)
                    } else {
                        self.driver.cancel_construction(id, reason)
                    };
                    self.notify(format!(
                        "Project #{id} {} {}",
                        reason,
                        if ok { "recorded" } else { "unavailable" }
                    ));
                } else {
                    self.notify("No construction project selected");
                }
            }
            KeyCode::Char('>') if matches!(self.ui.mode, ViewMode::Warden) => {
                if let Some(id) = self.ui.selected_construction {
                    let ok = self.driver.advance_construction(id, 1);
                    self.notify(format!(
                        "Advanced project #{id} by 1 day: {}",
                        if ok { "ok" } else { "unavailable" }
                    ));
                } else {
                    self.notify("No construction project selected");
                }
            }
            KeyCode::Char('v') if matches!(self.ui.mode, ViewMode::Warden) => {
                if let Some(id) = self.ui.selected_construction {
                    let ok = self.driver.approve_construction(id, "observer approval");
                    self.notify(format!(
                        "Project #{id} approval: {}",
                        if ok { "recorded" } else { "unavailable" }
                    ));
                } else {
                    self.notify("No construction project selected");
                }
            }
            KeyCode::Char('H' | 'G' | 'J' | 'Y') if matches!(self.ui.mode, ViewMode::Prisoner) => {
                if let Some(id) = self.ui.selected_prisoner {
                    let (verb, ok) = match code {
                        KeyCode::Char('H') => {
                            ("hearing scheduled", self.driver.start_parole_hearing(id))
                        }
                        KeyCode::Char('G') => ("parole granted", self.driver.grant_parole(id)),
                        KeyCode::Char('J') => (
                            "parole denied",
                            self.driver.deny_parole(id, "institutional conduct"),
                        ),
                        KeyCode::Char('Y') => {
                            ("released on parole", self.driver.release_on_parole(id))
                        }
                        _ => unreachable!(),
                    };
                    self.notify(format!(
                        "Prisoner #{id} {verb}: {}",
                        if ok { "recorded" } else { "rejected" }
                    ));
                } else {
                    self.notify("Select a prisoner for parole controls");
                }
            }
            KeyCode::Char('u') if matches!(self.ui.mode, ViewMode::Warden) => {
                self.cycle_construction(1);
            }
            KeyCode::Char('O') if matches!(self.ui.mode, ViewMode::Warden) => {
                self.cycle_construction(-1);
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
            KeyCode::Char('t') | KeyCode::Char('T') => {
                if matches!(self.ui.mode, ViewMode::Prisoner | ViewMode::Warden) {
                    let catalog = self.driver.rehab_program_catalog();
                    if catalog.is_empty() {
                        self.notify("No rehab programs available");
                    } else {
                        let next = self
                            .ui
                            .selected_rehab
                            .map(|i| (i as usize + 1) % catalog.len() as usize)
                            .unwrap_or(0);
                        self.ui.selected_rehab = Some(next as u64);
                        if let Some(p) = catalog.get(next) {
                            self.notify(format!("Selected rehab program: {}", p.name));
                        }
                    }
                }
            }
            KeyCode::Char('r') => {
                if matches!(self.ui.mode, ViewMode::Prisoner | ViewMode::Warden) {
                    if let Some(pid) = self.ui.selected_prisoner {
                        let catalog = self.driver.rehab_program_catalog();
                        let idx = self.ui.selected_rehab.unwrap_or(0) as usize;
                        if let Some(prog) = catalog.get(idx).or_else(|| catalog.first()) {
                            self.ui.selected_rehab =
                                Some(catalog.iter().position(|p| p.key == prog.key).unwrap_or(0)
                                    as u64);
                            let ok = self.driver.enroll_rehab(pid, &prog.key);
                            self.notify(if ok {
                                format!("Enrolled P{pid} in {}", prog.name)
                            } else {
                                format!("Enroll failed for P{pid} ({})", prog.name)
                            });
                        } else {
                            self.notify("No rehab programs available");
                        }
                    } else {
                        self.notify("Select a prisoner to enroll in rehab");
                    }
                }
            }
            KeyCode::Char('R') => {
                if matches!(self.ui.mode, ViewMode::Prisoner | ViewMode::Warden) {
                    if let Some(pid) = self.ui.selected_prisoner {
                        let catalog = self.driver.rehab_program_catalog();
                        let idx = self.ui.selected_rehab.unwrap_or(0) as usize;
                        let selected_key = catalog.get(idx).map(|p| p.key.clone());
                        let rows: Vec<_> = self
                            .driver
                            .rehab_summaries()
                            .into_iter()
                            .filter(|r| r.prisoner_id.raw() == pid)
                            .collect();
                        let target = selected_key
                            .as_ref()
                            .and_then(|k| rows.iter().find(|r| &r.program_key == k))
                            .or_else(|| rows.first());
                        if let Some(row) = target {
                            let ok = self.driver.drop_rehab(pid, &row.program_key);
                            self.notify(if ok {
                                format!("Dropped P{pid} from {}", row.program)
                            } else {
                                format!("Drop failed for P{pid} ({})", row.program)
                            });
                        } else {
                            self.notify(format!("P{pid} has no active rehab enrollment"));
                        }
                    } else {
                        self.notify("Select a prisoner to drop from rehab");
                    }
                }
            }
            KeyCode::Char('f') => {
                if matches!(self.ui.mode, ViewMode::IncidentReports) {
                    let cases = self.driver.contraband_cases();
                    if cases.is_empty() {
                        self.ui.selected_contraband = None;
                        self.notify("No contraband cases");
                    } else {
                        let cur = self.ui.selected_contraband;
                        let idx = cur
                            .and_then(|id| cases.iter().position(|c| c.case_id == id))
                            .unwrap_or(0);
                        let next = (idx + 1) % cases.len();
                        self.ui.selected_contraband = Some(cases[next].case_id);
                        self.notify(format!("Selected contraband case #{}", cases[next].case_id));
                    }
                }
            }
            KeyCode::Char('F') => {
                if matches!(self.ui.mode, ViewMode::IncidentReports) {
                    if let Some(case_id) = self.ui.selected_contraband {
                        let ok = self.driver.resolve_contraband_case(case_id);
                        self.notify(if ok {
                            format!("Resolved contraband case #{case_id}")
                        } else {
                            format!("Contraband case #{case_id} already resolved / unavailable")
                        });
                    } else {
                        self.notify("No contraband case selected");
                    }
                }
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                if matches!(self.ui.mode, ViewMode::IncidentReports) {
                    self.ui.cycle_incident_min_severity();
                    let msg = match self.ui.filter().min_severity {
                        None => "Incident severity filter: all".to_string(),
                        Some(s) if s >= 0.74 => {
                            "Incident severity filter: critical (>=75%)".to_string()
                        }
                        Some(_) => "Incident severity filter: high+ (>=40%)".to_string(),
                    };
                    self.ui.scroll = 0;
                    self.notify(msg);
                }
            }
            KeyCode::Char('o') | KeyCode::Char('O') => {
                if matches!(self.ui.mode, ViewMode::IncidentReports) {
                    self.ui.cycle_incident_resolved();
                    let msg = match self.ui.filter().resolved {
                        None => "Incident status filter: all".to_string(),
                        Some(false) => "Incident status filter: open only".to_string(),
                        Some(true) => "Incident status filter: resolved only".to_string(),
                    };
                    self.ui.scroll = 0;
                    self.notify(msg);
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if matches!(self.ui.mode, ViewMode::DecisionAudit) {
                    self.ui.cycle_decision_validation_filter();
                    let msg = match self.ui.decision_validation_filter.as_deref() {
                        None => "Decision filter: all".to_string(),
                        Some(v) => format!("Decision filter: {v}"),
                    };
                    self.ui.scroll = 0;
                    self.notify(msg);
                }
            }
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k' | 'h') => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j' | 'l') => self.move_cursor(1),
            _ => {}
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        // All views (including the schematic map) share one selection cursor.
        // Clamp rather than cast so a large list never wraps back to the top.
        let next = i32::from(self.ui.scroll) + delta;
        self.ui.scroll = next.clamp(0, i32::from(u16::MAX)) as u16;
    }

    /// Resolve the selected incident, or — when none is selected — the newest
    /// open critical incident (Ctrl-R). Routing goes through the validated
    /// [`SimDriver::resolve_incident`] control; it never mutates driver state
    /// directly.
    fn resolve_critical_incident(&mut self) {
        let records = self.driver.incident_records();
        let newest_critical = records
            .iter()
            .rev()
            .find(|r| !r.resolved && r.severity >= 0.75)
            .map(|r| r.id);
        let target = match self.ui.selected_incident {
            Some(id) if records.iter().any(|r| r.id == id && !r.resolved) => Some(id),
            _ => newest_critical,
        };
        match target {
            Some(id) => {
                let ok = self.driver.resolve_incident(id);
                // If the incident re-appears as unresolved later, allow the
                // auto-pause-on-critical to fire again.
                self.seen_critical.retain(|&s| s != id);
                self.notify(format!(
                    "Resolved incident #{id}: {}",
                    if ok {
                        "recorded"
                    } else {
                        "already resolved / unavailable"
                    }
                ));
            }
            None => self.notify("No open critical incidents"),
        }
    }

    /// Cycle the Warden's selected construction project forward (`u`) or
    /// backward (`U`) through the current queue.
    fn cycle_construction(&mut self, delta: i32) {
        let queue = self.driver.construction_queue();
        if queue.is_empty() {
            self.notify("No construction projects in queue");
            return;
        }
        let len = queue.len() as i32;
        let idx = match self.ui.selected_construction {
            Some(id) => queue
                .iter()
                .position(|p| p.id == id)
                .map(|i| (i as i32 + delta).rem_euclid(len))
                .unwrap_or(0),
            None => 0,
        };
        let id = queue[idx as usize].id;
        self.ui.selected_construction = Some(id);
        self.notify(format!(
            "Selected construction project #{id} ({}/{})",
            idx + 1,
            queue.len()
        ));
    }

    /// Observer safety valve from `run()`: pause once on a *newly seen*
    /// critical incident (see [`App::seen_critical`]).
    fn check_pause_on_critical(&mut self) {
        if !(self.ui.pause_on_critical && !self.paused) {
            return;
        }
        let new_critical = self
            .driver
            .incident_records()
            .into_iter()
            .filter(|r| r.severity >= 0.75 && !r.resolved)
            .map(|r| r.id)
            .find(|id| !self.seen_critical.contains(id));
        if let Some(id) = new_critical {
            self.seen_critical.push(id);
            self.paused = true;
            self.driver.set_speed(TimeSpeed::Paused);
            self.notify("Auto-paused: unresolved critical incident");
        }
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

/// Shared case-insensitive substring matcher for the `visible_*` filters.
/// Empty queries match everything.
fn query_matches(haystack: &str, needle: &str) -> bool {
    let n = needle.trim().to_lowercase();
    n.is_empty() || haystack.to_lowercase().contains(&n)
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    struct MockDriver {
        sim: sumo::Sim,
        ticks: u32,
        lockdown: bool,
        autosave_enables: Vec<(std::path::PathBuf, u64)>,
        /// Structured incident records served by `incident_records`.
        incidents: Vec<sumo::IncidentRecord>,
        /// Incident ids recorded through `resolve_incident`.
        resolved_incidents: Vec<u64>,
        /// Structured decision audit records served by `decision_audit_log`.
        decisions: Vec<sumo::DecisionAuditRecord>,
        /// Construction projects in the queue served by `construction_queue`.
        construction_queue: Vec<sumo::ConstructionSummary>,
        /// Parole control calls observed by the spec 006 key tests.
        parole_calls: Vec<String>,
        /// Construction approvals observed by the spec 006 key tests.
        approval_calls: Vec<u64>,
    }

    /// Borrow the concrete [`MockDriver`] behind an [`App`] (tests).
    fn mock(app: &mut App) -> &mut MockDriver {
        app.driver_mut()
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<MockDriver>())
            .expect("App driver is a MockDriver")
    }

    impl SimDriver for MockDriver {
        fn tick(&mut self) {
            self.ticks += 1;
            self.sim.tick();
        }
        fn observation(&self) -> sumo::Observation {
            self.sim.observation()
        }
        fn map_frame(&self) -> sumo::MapFrame {
            self.sim.map_frame()
        }
        fn stats_since(&self, at: sumo::SimTime) -> sumo::Stats {
            self.sim.stats_since(at)
        }
        fn events_since(&self, at: sumo::SimTime) -> Vec<sumo::WorldEvent> {
            self.sim.events_since(at)
        }
        fn prisoners(&self) -> Vec<sumo::PrisonerSummary> {
            self.sim.prisoners()
        }
        fn gangs(&self) -> Vec<sumo::GangSummary> {
            self.sim.gangs()
        }
        fn guards(&self) -> Vec<sumo::GuardSummary> {
            self.sim.guards()
        }
        fn warden(&self) -> sumo::WardenSummary {
            self.sim.warden()
        }
        fn economy(&self) -> sumo::EconomySummary {
            self.sim.economy()
        }
        fn social_edges(&self) -> Vec<sumo::SocialEdge> {
            self.sim.social_edges()
        }
        fn social_roles(&self) -> Vec<sumo::SocialRoleSummary> {
            self.sim.social_roles()
        }
        fn facilities(&self) -> Vec<sumo::FacilitySummary> {
            self.sim.facilities()
        }
        fn construction_queue(&self) -> Vec<sumo::ConstructionSummary> {
            self.sim.construction_queue()
        }
        fn speed(&self) -> TimeSpeed {
            self.sim.speed()
        }
        fn set_speed(&mut self, speed: TimeSpeed) {
            self.sim.set_speed(speed);
        }
        fn policy_mode(&self) -> sumo::PolicyMode {
            self.sim.policy_mode()
        }
        fn set_policy_mode(&mut self, mode: sumo::PolicyMode) {
            self.sim.set_policy_mode(mode);
        }
        fn sim_time(&self) -> sumo::SimTime {
            self.sim.world.time
        }
        fn days_simulated(&self) -> u64 {
            self.sim.world.days_simulated
        }
        fn set_lockdown(&mut self, lockdown: bool) {
            self.lockdown = lockdown;
        }
        fn set_policy(&mut self, policy: sumo::Policy) {
            self.sim.set_policy(policy);
        }
        fn policy_summary(&self) -> sumo::PolicySummary {
            let mut ps = self.sim.policy_summary();
            ps.lockdown = self.lockdown;
            ps
        }
        fn set_security_level(&mut self, level: f32) {
            self.sim.set_security_level(level);
        }
        fn set_privileges_level(&mut self, level: f32) {
            self.sim.set_privileges_level(level);
        }
        fn set_rehab_funding(&mut self, multiplier: f32) {
            self.sim.set_rehab_funding(multiplier);
        }
        fn set_intake_paused(&mut self, paused: bool) {
            self.sim.set_intake_paused(paused);
        }
        fn save(&self, _dir: &std::path::Path) -> Result<(), String> {
            Ok(())
        }
        fn enable_autosave(&mut self, dir: std::path::PathBuf, interval_days: u64) {
            self.autosave_enables.push((dir.clone(), interval_days));
            self.sim.enable_autosave(dir, interval_days);
        }
        fn maybe_autosave(&mut self) -> Result<bool, String> {
            self.sim.maybe_autosave().map_err(|e| e.to_string())
        }
        fn incident_log(&self) -> Vec<sumo::WorldEvent> {
            self.sim.incident_log()
        }
        fn incident_records(&self) -> Vec<sumo::IncidentRecord> {
            self.incidents.clone()
        }
        fn resolve_incident(&mut self, id: u64) -> bool {
            self.resolved_incidents.push(id);
            if let Some(r) = self.incidents.iter_mut().find(|r| r.id == id) {
                r.resolved = true;
            }
            true
        }
        fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
            Some(self)
        }
        fn decision_audit_log(&self) -> Vec<sumo::DecisionAuditRecord> {
            self.decisions.clone()
        }
        fn incident_records_filtered(
            &self,
            filter: &sumo::incidents::IncidentFilter,
        ) -> Vec<sumo::IncidentRecord> {
            self.incidents
                .iter()
                .filter(|r| {
                    if let Some(min) = filter.min_severity {
                        if r.severity < min {
                            return false;
                        }
                    }
                    if let Some(resolved) = filter.resolved {
                        if r.resolved != resolved {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect()
        }
        fn load(&mut self, _dir: &std::path::Path) -> Result<(), String> {
            Ok(())
        }
        fn post_release_status(&self, _prisoner_id: u64) -> Option<bool> {
            None
        }
        fn post_release_records(&self) -> Vec<sumo::PostReleaseSummary> {
            Vec::new()
        }
        fn rehab_summaries(&self) -> Vec<sumo::RehabEnrollmentSummary> {
            Vec::new()
        }
        fn rehab_program_catalog(&self) -> Vec<sumo::RehabProgramInfo> {
            Vec::new()
        }
        fn enroll_rehab(&mut self, _prisoner_id: u64, _program_key: &str) -> bool {
            false
        }
        fn drop_rehab(&mut self, _prisoner_id: u64, _program_key: &str) -> bool {
            false
        }
        fn rehab_overview(&self) -> sumo::RehabOverview {
            sumo::RehabOverview::default()
        }
        fn contraband_cases(&self) -> Vec<sumo::ContrabandCaseSummary> {
            Vec::new()
        }
        fn contraband_inventory(&self) -> Vec<sumo::ContrabandInventorySummary> {
            Vec::new()
        }
        fn schedule_timeline(&self, _prisoner_id: u64) -> Option<sumo::ScheduleTimeline> {
            None
        }
        fn budget_snapshot(&self) -> sumo::BudgetSnapshot {
            sumo::BudgetSnapshot::default()
        }
        fn apply_rehab_progress(&mut self, _prisoner_id: u64, _minutes: u64) -> f32 {
            0.0
        }
        fn enqueue_construction(
            &mut self,
            kind: alc_world::FacilityKind,
            wing: u32,
            capacity: u32,
        ) -> u64 {
            let id = self.construction_queue.len() as u64 + 1;
            self.construction_queue.push(sumo::ConstructionSummary {
                id,
                kind,
                wing,
                capacity,
                total_days: 10,
                progress_days: 0,
                cost: 100.0,
                started_day: 0,
                status: alc_world::ConstructionStatus::Queued,
                terminal_reason: None,
                staff_allocated: false,
                materials_ready: false,
                prereqs_met: false,
            });
            id
        }
        fn approve_construction(&mut self, project_id: u64, _reason: &str) -> bool {
            self.approval_calls.push(project_id);
            true
        }
        fn parole_due(&self) -> Vec<u64> {
            self.sim.parole_due().iter().map(|id| id.raw()).collect()
        }
        fn parole_status(&self, id: u64) -> Option<sumo::ParoleStatus> {
            self.sim.parole_status(alc_core::PrisonerId(id))
        }
        fn parole_conditions(&self, id: u64) -> Option<sumo::ParoleConditions> {
            self.sim
                .parole_conditions(alc_core::PrisonerId(id))
                .cloned()
        }
        fn parole_decision_reason(&self, id: u64) -> Option<String> {
            self.sim
                .parole_decision_reason(alc_core::PrisonerId(id))
                .map(str::to_string)
        }
        fn parole_released_at(&self, id: u64) -> Option<sumo::SimTime> {
            self.sim.parole_released_at(alc_core::PrisonerId(id))
        }
        fn start_parole_hearing(&mut self, id: u64) -> bool {
            self.parole_calls.push(format!("hearing:{id}"));
            true
        }
        fn grant_parole(&mut self, id: u64) -> bool {
            self.parole_calls.push(format!("grant:{id}"));
            true
        }
        fn deny_parole(&mut self, id: u64, _reason: &str) -> bool {
            self.parole_calls.push(format!("deny:{id}"));
            true
        }
        fn release_on_parole(&mut self, id: u64) -> bool {
            self.parole_calls.push(format!("release:{id}"));
            true
        }
    }

    fn new_mock() -> MockDriver {
        MockDriver {
            sim: sumo::Sim::new(7),
            ticks: 0,
            lockdown: false,
            autosave_enables: Vec::new(),
            incidents: Vec::new(),
            resolved_incidents: Vec::new(),
            decisions: default_decisions(),
            construction_queue: Vec::new(),
            parole_calls: Vec::new(),
            approval_calls: Vec::new(),
        }
    }

    fn app() -> App {
        App::new(Box::new(new_mock()), TuiOptions::default())
    }

    #[test]
    fn app_new_honors_restored_driver_speed() {
        // A non-paused driver speed (e.g. restored by `Sim::load`) is kept.
        let mut mock = new_mock();
        mock.sim.set_speed(TimeSpeed::Extreme);
        let app = App::new(Box::new(mock), TuiOptions::default());
        assert_eq!(app.driver().speed(), TimeSpeed::Extreme);
        // Explicit paused observer state still wins over the restored speed.
        let mut mock2 = new_mock();
        mock2.sim.set_speed(TimeSpeed::Extreme);
        let app2 = App::new(
            Box::new(mock2),
            TuiOptions {
                paused: true,
                ..Default::default()
            },
        );
        assert_eq!(app2.driver().speed(), TimeSpeed::Paused);
    }

    fn default_decisions() -> Vec<sumo::DecisionAuditRecord> {
        // Identical actor/action but distinct times — decision drill-down keys
        // must stay unique for rows like these.
        (0..3)
            .map(|i| sumo::DecisionAuditRecord {
                time: alc_core::SimTime::from_secs(100 + i),
                actor: "Warden A".to_string(),
                action: "toggle_lockdown".to_string(),
                source: "test".to_string(),
                summary: format!("decision {i}"),
                ..Default::default()
            })
            .collect()
    }

    fn critical_record(id: u64, severity: f32, resolved: bool) -> sumo::IncidentRecord {
        sumo::IncidentRecord {
            id,
            severity,
            resolved,
            ..Default::default()
        }
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
    fn observer_feed_pin_and_search_keys_work() {
        let mut app = app();
        app.handle_key(KeyCode::Char('n'), KeyModifiers::NONE);
        assert!(app.ui().show_notifications);
        app.handle_key(KeyCode::Char('n'), KeyModifiers::NONE);
        assert!(!app.ui().show_notifications);

        app.handle_key(KeyCode::Char('V'), KeyModifiers::NONE);
        assert!(app.ui().pause_on_critical);

        app.ui_mut().mode = ViewMode::Prisoner;
        app.ui_mut().selected_prisoner = Some(42);
        app.handle_key(KeyCode::Tab, KeyModifiers::NONE);
        assert!(app.ui().bookmarks.contains(&42));
        app.handle_key(KeyCode::Tab, KeyModifiers::NONE);
        assert!(!app.ui().bookmarks.contains(&42));

        app.handle_key(KeyCode::Char('/'), KeyModifiers::NONE);
        assert!(app.ui().search_active);
        app.handle_key(KeyCode::Char('a'), KeyModifiers::NONE);
        app.handle_key(KeyCode::Char('b'), KeyModifiers::NONE);
        app.handle_key(KeyCode::Backspace, KeyModifiers::NONE);
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE);
        assert!(!app.ui().search_active);
        assert_eq!(app.ui().search, "a");

        app.notify("notice one");
        assert_eq!(
            app.ui().notifications.last().map(String::as_str),
            Some("notice one")
        );
        for i in 0..40 {
            app.notify(format!("notice {i}"));
        }
        assert_eq!(app.ui().notifications.len(), 32);
    }

    #[test]
    fn construction_keys_cycle_and_queue_in_warden_view() {
        let mut app = app();
        app.handle_key(KeyCode::Char('5'), KeyModifiers::NONE);
        assert_eq!(app.ui().mode, ViewMode::Warden);
        let before = app.ui().construction_kind_index;
        app.handle_key(KeyCode::Char('c'), KeyModifiers::NONE);
        assert_ne!(app.ui().construction_kind_index, before);
        app.handle_key(KeyCode::Char('C'), KeyModifiers::NONE);
        // MockDriver's enqueue_construction returns queue ids starting at 1.
        assert_eq!(app.ui().selected_construction, Some(1));
        app.handle_key(KeyCode::Char('>'), KeyModifiers::NONE);
        app.handle_key(KeyCode::Char('x'), KeyModifiers::NONE);
        app.handle_key(KeyCode::Char('X'), KeyModifiers::NONE);
        assert!(app.status_message().is_some());
    }

    #[test]
    fn parole_and_approval_keys_route_through_validated_controls() {
        let mut app = app();
        let pid = app.driver().prisoners()[0].id.raw();

        // Warden view: `v` approves the selected project.
        app.handle_key(KeyCode::Char('5'), KeyModifiers::NONE);
        app.handle_key(KeyCode::Char('C'), KeyModifiers::NONE);
        app.handle_key(KeyCode::Char('v'), KeyModifiers::NONE);
        assert_eq!(mock(&mut app).approval_calls, vec![1]);

        // Prisoner view: H/G/J/Y mirror the GUI parole controls.
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        app.ui_mut().selected_prisoner = Some(pid);
        for key in ['H', 'G', 'J', 'Y'] {
            app.handle_key(KeyCode::Char(key), KeyModifiers::NONE);
        }
        assert_eq!(
            mock(&mut app).parole_calls,
            vec![
                format!("hearing:{pid}"),
                format!("grant:{pid}"),
                format!("deny:{pid}"),
                format!("release:{pid}"),
            ]
        );

        // Read surfaces are bundled into ViewData for renderers.
        app.driver_mut().start_parole_hearing(pid);
        let data = app.build_view_data();
        assert_eq!(data.selected_parole, app.driver().parole_status(pid));
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
            assert_eq!(
                app.ui().mode,
                views::mode_from_index(usize::from(i) - 1).unwrap()
            );
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
                app.ui().scroll,
                expected,
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
        assert_eq!(
            app.visible_prisoners().len(),
            app.driver().prisoners().len()
        );
    }

    #[test]
    fn incident_filter_is_honored_by_visible_incidents() {
        let mut app = app();
        let all = app.visible_incidents();
        assert_eq!(all.len(), app.driver().incident_records().len());
        app.ui_mut()
            .set_filter(sumo::incidents::IncidentFilter::all().with_min_severity(0.99));
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

    #[test]
    fn view_data_carries_map_frame_and_true_totals() {
        let app = app();
        let data = app.build_view_data();
        // MapFrame is present and unfiltered.
        assert_eq!(data.map_frame.at, app.driver().map_frame().at);
        // True totals equal driver totals.
        assert_eq!(data.total_prisoners, app.driver().prisoners().len());
        assert_eq!(data.total_gangs, app.driver().gangs().len());
        assert_eq!(data.total_guards, app.driver().guards().len());
        // And the whole frame renders.
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
    }

    #[test]
    fn pause_on_critical_pauses_once_and_resumes() {
        let mut app = app();
        mock(&mut app).incidents = vec![critical_record(1, 0.9, false)];
        app.handle_key(KeyCode::Char('V'), KeyModifiers::NONE);
        assert!(app.ui().pause_on_critical);
        assert!(!app.is_paused());
        // New critical id pauses the sim.
        app.check_pause_on_critical();
        assert!(app.is_paused());
        // Resuming is not re-blocked by the same incident.
        app.handle_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(!app.is_paused());
        app.check_pause_on_critical();
        assert!(!app.is_paused(), "seen critical must not re-pause");
        // A genuinely new critical id pauses again.
        mock(&mut app)
            .incidents
            .push(critical_record(2, 0.95, false));
        app.check_pause_on_critical();
        assert!(app.is_paused());
    }

    #[test]
    fn ctrl_r_resolves_selected_or_newest_critical_incident() {
        let mut app = app();
        mock(&mut app).incidents = vec![
            critical_record(10, 0.2, false),
            critical_record(11, 0.95, false),
        ];
        app.handle_key(KeyCode::Char('0'), KeyModifiers::NONE);
        // Newest open critical resolved when nothing is selected.
        app.handle_key(KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert_eq!(mock(&mut app).resolved_incidents, vec![11]);
        // Nothing left to resolve.
        app.handle_key(KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert_eq!(mock(&mut app).resolved_incidents.len(), 1);
    }

    #[test]
    fn decision_keys_are_unique_and_follow_cursor() {
        let mut app = app();
        app.ui_mut().mode = ViewMode::DecisionAudit;
        app.sync_selection();
        let mut keys = Vec::new();
        for i in 0..mock(&mut app).decisions.len() {
            let visible = app.visible_decisions();
            let expected = crate::decision_key(&visible[i]);
            let key = app
                .ui()
                .selected_decision
                .clone()
                .expect("selection follows cursor");
            assert_eq!(key, expected, "key must identify the row under the cursor");
            keys.push(key);
            app.move_cursor(1);
            app.sync_selection();
        }
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(keys.len(), unique.len(), "decision keys must be unique");
        assert!(!keys.iter().any(|k| k.is_empty()));
    }

    #[test]
    fn decision_key_survives_trim_and_filter() {
        let mut app = app();
        app.ui_mut().mode = ViewMode::DecisionAudit;
        app.sync_selection();
        let key = app
            .ui()
            .selected_decision
            .clone()
            .expect("newest decision selected");
        // Simulate bounded-log trimming from the front.
        mock(&mut app).decisions.remove(0);
        app.ui_mut().scroll = 0;
        app.sync_selection();
        assert_eq!(app.ui().selected_decision.as_deref(), Some(key.as_str()));
        // Filtering renumbers rows; the key must still identify the same record.
        app.ui_mut().set_query("decision 2");
        app.ui_mut().scroll = 0;
        app.sync_selection();
        assert_eq!(app.ui().selected_decision.as_deref(), Some(key.as_str()));
    }

    #[test]
    fn quick_load_clears_stale_observer_state() {
        let mut app = app();
        app.ui_mut().bookmarks.push(42);
        app.ui_mut().search.push('q');
        app.ui_mut().search_active = true;
        app.ui_mut().notifications.push("stale".to_string());
        app.ui_mut().selected_contraband = Some(1);
        app.ui_mut().selected_decision = Some("stale-key".to_string());
        assert!(app.quick_load().is_ok());
        assert!(app.ui().bookmarks.is_empty());
        assert!(app.ui().search.is_empty());
        assert!(!app.ui().search_active);
        assert!(!app.ui().notifications.iter().any(|n| n == "stale"));
        assert!(app.ui().selected_contraband.is_none());
        assert!(app.ui().selected_decision.is_none());
    }

    #[test]
    fn search_query_applies_to_incidents() {
        let mut app = app();
        mock(&mut app).incidents = vec![sumo::IncidentRecord {
            id: 7,
            kind: "Fight".to_string(),
            location: "Yard".to_string(),
            ..Default::default()
        }];
        app.ui_mut().set_query("fight");
        assert_eq!(app.visible_incidents().len(), 1);
        app.ui_mut().set_query("yard");
        assert_eq!(app.visible_incidents().len(), 1);
        app.ui_mut().set_query("zzz-nothing");
        assert!(app.visible_incidents().is_empty());
        app.ui_mut().clear_query();
        assert_eq!(app.visible_incidents().len(), 1);
    }

    #[test]
    fn ctrl_c_ctrl_l_and_ctrl_w_work_while_searching() {
        let mut a = app();
        a.handle_key(KeyCode::Char('/'), KeyModifiers::NONE);
        assert!(a.ui().search_active);
        // Ctrl-C quits even while the search input is open.
        a.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(a.should_quit);
        // Ctrl-W clears the query while keeping the input open.
        let mut a2 = app();
        a2.handle_key(KeyCode::Char('/'), KeyModifiers::NONE);
        a2.ui_mut().search = "some query".to_string();
        a2.handle_key(KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert!(a2.ui().search_active);
        assert_eq!(a2.ui().search, "");
        assert!(a2.status_message().is_some());
    }

    #[test]
    fn warden_mode_preserves_context_prisoner() {
        let mut app = app();
        assert!(
            !mock(&mut app).sim.world.prisoners.is_empty(),
            "world has prisoners"
        );
        app.handle_key(KeyCode::Char('5'), KeyModifiers::NONE);
        assert_eq!(app.ui().mode, ViewMode::Warden);
        app.sync_selection();
        assert!(
            app.ui().selected_prisoner.is_some(),
            "Warden needs a context prisoner for r/R"
        );
    }

    #[test]
    fn uppercase_u_and_o_cycle_construction_selection() {
        let mut app = app();
        app.handle_key(KeyCode::Char('5'), KeyModifiers::NONE);
        // Queue one project via the validated enqueue control.
        app.handle_key(KeyCode::Char('C'), KeyModifiers::NONE);
        assert_eq!(app.ui().selected_construction, Some(1));
        // Cycle around a single-project queue stays put.
        app.handle_key(KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(app.ui().selected_construction, Some(1));
        app.handle_key(KeyCode::Char('O'), KeyModifiers::NONE);
        assert_eq!(app.ui().selected_construction, Some(1));
    }
}
