//! The egui application shell.
//!
//! [`App`] owns the simulation, all observer UI state, and a queue of
//! [`Command`]s. Rendering never touches `sumo::Sim`; handlers only enqueue
//! commands that are applied here through validated engine control methods.

use crate::{ViewMode, data, map_view, panels, screens, settings};
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
    ApproveConstruction(u64),
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
    ReleaseParole(PrisonerId),
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
    pub selected_event: Option<u64>,
    pub selected_contraband: Option<u64>,
    /// Stable content key of the selected audit row (survives log trimming).
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
            selected_event: None,
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

    /// Drop observer state that may point into the world that was just replaced
    /// by a load. Camera position and view preference are kept; all selected
    /// ids, free-text queries and content-derived keys are cleared because they
    /// cannot be meaningfully revalidated against a different save.
    fn clear_stale_for_load(&mut self) {
        self.search.clear();
        self.selected_prisoner = None;
        self.selected_gang = None;
        self.selected_guard = None;
        self.selected_incident = None;
        self.selected_event = None;
        self.selected_contraband = None;
        self.selected_decision = None;
        self.selected_facility = None;
        self.construction_kind_index = 0;
        self.rehab_program_index = 0;
        self.status = None;
        self.incident_min_severity = 0.0;
        self.incident_open_only = false;
        self.decision_validation_filter = None;
        self.event_kind_filter.clear();
        self.map.hovered_pawn = None;
        self.map.selected_pawn = None;
        self.map.hovered_facility = None;
        self.map.selected_facility = None;
    }
}

/// Coarse, read-only fingerprint of the control surfaces a warden action can
/// touch. It is captured immediately before/after a validated action so the UI
/// can report a no-op instead of claiming every `WardenAction` was applied.
#[derive(Debug, Clone, PartialEq)]
struct WardenControlFingerprint {
    last_event_id: u64,
    last_event_summary: String,
    guards: usize,
    cash_major: f64,
    ledger_len: usize,
    security: f32,
    privileges: f32,
    rehab_funding: f32,
    lockdown: bool,
    construction: usize,
    facility_condition_sum: f32,
    attention: f32,
}

impl WardenControlFingerprint {
    fn capture(sim: &sumo::Sim) -> Self {
        let world = sim.world();
        let policy = sim.policy_summary();
        let economy = sim.economy();
        let last_event = sim.events().recent().last();
        Self {
            last_event_id: last_event.map_or(0, |e| e.id),
            last_event_summary: last_event.map_or_else(String::new, |e| e.summary.clone()),
            guards: world.guards.len(),
            cash_major: economy.cash_major,
            ledger_len: economy.ledger.len(),
            security: policy.security_level,
            privileges: policy.privileges_level,
            rehab_funding: policy.rehab_funding_multiplier,
            lockdown: policy.lockdown,
            construction: world.construction_queue.len(),
            facility_condition_sum: world.facilities.iter().map(|f| f.condition).sum(),
            attention: world.warden.attention,
        }
    }
}

/// Observable result of a validated warden control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WardenActionOutcome {
    Applied,
    Deferred,
    NoOp,
    Rejected,
}

/// Derive the observer-facing outcome without bypassing engine validation.
/// `apply_warden_action` always writes an accepted audit record, so audit
/// presence alone is not proof that state changed; compare the control surface
/// and honour explicit rejection/deferred markers.
fn warden_action_outcome(
    before: &WardenControlFingerprint,
    after: &WardenControlFingerprint,
    audit: Option<&sumo::DecisionAuditRecord>,
    action: &WardenAction,
) -> WardenActionOutcome {
    if let Some(record) = audit {
        let validation = record.validation.to_ascii_lowercase();
        let outcome = format!("{:?}", record.outcome).to_ascii_lowercase();
        if validation.starts_with("rejected") || outcome == "rejected" {
            return WardenActionOutcome::Rejected;
        }
    }
    if matches!(action, WardenAction::NoAction { .. }) {
        return WardenActionOutcome::NoOp;
    }
    if after.last_event_id > before.last_event_id
        && after
            .last_event_summary
            .to_ascii_lowercase()
            .contains("deferred")
    {
        return WardenActionOutcome::Deferred;
    }
    if before == after {
        // ReduceExpenses deliberately defers when finances are healthy or the
        // cooldown is active; surface that instead of a generic no-op.
        if matches!(action, WardenAction::ReduceExpenses) {
            WardenActionOutcome::Deferred
        } else {
            WardenActionOutcome::NoOp
        }
    } else {
        WardenActionOutcome::Applied
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
        let app = Self {
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
            SetView(view) => {
                self.state.view = view;
                persist = true;
            }
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
                        // A loaded save carries its own runtime autosave state;
                        // reapply the observer's current preference like the
                        // TUI does, then drop every selection/query that could
                        // point into the previous world.
                        self.state.save_dir = dir.clone();
                        self.state.save_input = dir.clone();
                        self.state.load_input = dir.clone();
                        if self.state.autosave_days > 0 {
                            let autosave_dir = self.state.save_dir.clone();
                            self.sim
                                .enable_autosave(autosave_dir, self.state.autosave_days);
                        } else {
                            self.sim.disable_autosave();
                        }
                        self.state.clear_stale_for_load();
                        self.state.status = Some(format!("Loaded world from {dir}"));
                        self.snapshot = None;
                        self.cache_revision = u64::MAX;
                        self.cache_selection = None;
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
                self.state.status =
                    Some(format!("Intake {}", if paused { "paused" } else { "open" }));
            }
            SetPolicyMode(mode) => {
                self.sim.set_policy_mode(mode);
                self.state.status = Some(format!("Policy mode: {mode:?}"));
            }
            WardenAction(action) => {
                let label = action.label();
                let before = WardenControlFingerprint::capture(&self.sim);
                // Keep the single validated mutation boundary; only the
                // observer-facing status is derived from the resulting state.
                self.sim.apply_warden_action(action.clone());
                let after = WardenControlFingerprint::capture(&self.sim);
                let outcome = warden_action_outcome(
                    &before,
                    &after,
                    self.sim.decision_audit_log().last(),
                    &action,
                );
                self.state.status = Some(match outcome {
                    WardenActionOutcome::Applied => format!("Warden action applied: {label}"),
                    WardenActionOutcome::Deferred => {
                        format!("Warden action deferred: {label} (engine held it back this cycle)")
                    }
                    WardenActionOutcome::NoOp => {
                        format!("Warden action had no observable effect: {label}")
                    }
                    WardenActionOutcome::Rejected => format!("Warden action rejected: {label}"),
                });
            }
            EnqueueConstruction {
                kind,
                wing,
                capacity,
            } => {
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
            ApproveConstruction(id) => {
                let ok = self.sim.approve_construction(id, "observer approval");
                self.state.status = Some(format!(
                    "Construction #{id} approval: {}",
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
            ReleaseParole(id) => {
                let ok = self.sim.release_on_parole(id);
                self.state.status = Some(format!(
                    "Release on parole for prisoner {}: {}",
                    id.raw(),
                    if ok { "granted" } else { "rejected" }
                ));
            }
            RecordPostRelease {
                prisoner,
                release_type,
                risk,
                plan,
            } => {
                let _ = self
                    .sim
                    .record_post_release(prisoner, &release_type, risk, plan);
                self.state.status = Some(format!(
                    "Post-release recorded for prisoner {} ({release_type})",
                    prisoner.raw()
                ));
            }
        }
        self.revision += 1;
        if persist {
            self.state
                .to_settings(self.sim.speed())
                .save(&self.settings_path);
        }
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
        // Ensure the cache is current, then borrow only the fields we need so
        // the immutable snapshot can coexist with mutable observer state.
        if self.snapshot.is_none()
            || self.cache_revision != self.revision
            || self.cache_selection != self.state.selected_prisoner
        {
            let selection = self.state.selected_prisoner;
            self.snapshot = Some(data::capture(&self.sim, selection));
            self.cache_revision = self.revision;
            self.cache_selection = selection;
        }
        let snapshot = self.snapshot.as_ref().expect("snapshot just captured");
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
    fn construction_and_parole_commands_use_validated_controls() {
        let mut app = App::new(0xC0FFEE_02);
        let pid = app.sim.world().prisoners[0].id;
        let total = app.sim.world().prisoners[0].sentence_days;
        if let Some(p) = app
            .sim
            .world_mut()
            .prisoners
            .iter_mut()
            .find(|p| p.id == pid)
        {
            p.sentence_served_days = (total * 7 / 10).max(1);
            p.schedule.disciplinary_status = false;
        }

        app.apply(Command::StartParole(pid));
        assert_eq!(
            app.sim.parole_status(pid),
            Some(sumo::ParoleStatus::Scheduled)
        );
        app.apply(Command::GrantParole(pid));
        assert_eq!(
            app.sim.parole_status(pid),
            Some(sumo::ParoleStatus::Granted)
        );
        assert!(app.sim.parole_conditions(pid).is_some());

        let audit_before = app.sim.decision_audit_log().len();
        app.apply(Command::ReleaseParole(pid));
        assert!(!app.sim.world().prisoners.iter().any(|p| p.id == pid));
        assert_eq!(app.sim.parole_released_at(pid), Some(app.sim.world().time));
        assert!(app.sim.decision_audit_log().len() > audit_before);

        let project = app
            .sim
            .enqueue_construction(alc_world::FacilityKind::Medical, 1, 10);
        app.apply(Command::ApproveConstruction(project));
        assert_eq!(
            app.sim
                .construction_queue()
                .into_iter()
                .find(|p| p.id == project)
                .unwrap()
                .status,
            alc_world::ConstructionStatus::Approved
        );
        let violations = app.sim.check_invariants();
        assert!(
            violations.is_empty(),
            "parole/construction commands left violations: {violations:?}"
        );
    }

    #[test]
    fn tick_rates_are_bounded_and_ordered() {
        assert_eq!(App::ticks_per_second(TimeSpeed::Paused), 0.0);
        assert!(
            App::ticks_per_second(TimeSpeed::RealTime)
                < App::ticks_per_second(TimeSpeed::FastForward)
        );
        assert!(
            App::ticks_per_second(TimeSpeed::FastForward)
                < App::ticks_per_second(TimeSpeed::Extreme)
        );
    }

    #[test]
    fn load_clear_drops_stale_selections_and_filters_but_keeps_preferences() {
        let mut state = ObserverState::from_settings(&settings::Settings::default());
        state.view = ViewMode::Map;
        state.search = "stale".into();
        state.selected_prisoner = Some(PrisonerId(7));
        state.selected_incident = Some(3);
        state.selected_decision = Some("old-key".into());
        state.event_kind_filter = "Fight".into();
        state.incident_min_severity = 0.8;
        state.incident_open_only = true;
        state.map.selected_facility = Some(2);
        state.map.hovered_pawn = Some((true, 9));

        state.clear_stale_for_load();

        assert_eq!(state.view, ViewMode::Map);
        assert!(state.search.is_empty());
        assert!(state.selected_prisoner.is_none());
        assert!(state.selected_incident.is_none());
        assert!(state.selected_decision.is_none());
        assert!(state.event_kind_filter.is_empty());
        assert_eq!(state.incident_min_severity, 0.0);
        assert!(!state.incident_open_only);
        assert!(state.map.selected_facility.is_none());
        assert!(state.map.hovered_pawn.is_none());
    }

    #[test]
    fn warden_actions_report_observed_outcome_not_universal_success() {
        let mut app = App::new(0xC0FFEE_10);
        app.apply(Command::WardenAction(WardenAction::NoAction {
            rationale: "hold".into(),
        }));
        let status = app.state.status.as_deref().unwrap_or_default();
        assert!(status.contains("no observable effect"), "{status}");
        assert!(!status.contains("applied"), "{status}");

        app.apply(Command::WardenAction(WardenAction::ReduceExpenses));
        let status = app.state.status.as_deref().unwrap_or_default();
        assert!(status.contains("deferred"), "{status}");

        app.apply(Command::WardenAction(WardenAction::HireGuards { count: 1 }));
        let status = app.state.status.as_deref().unwrap_or_default();
        assert!(status.contains("applied"), "{status}");

        app.apply(Command::WardenAction(WardenAction::RequestFunding {
            amount_major: 1_000,
        }));
        let status = app.state.status.as_deref().unwrap_or_default();
        assert!(status.contains("applied"), "{status}");
        app.apply(Command::WardenAction(WardenAction::RequestFunding {
            amount_major: 1_000,
        }));
        let status = app.state.status.as_deref().unwrap_or_default();
        assert!(status.contains("deferred"), "{status}");
        assert!(!status.contains("applied"), "{status}");
    }

    #[test]
    fn successful_load_invalidates_snapshot_and_observer_state() {
        let dir = std::env::temp_dir().join(format!("alc-gui-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut app = App::new(0xC0FFEE_11);
        app.state.search = "stale".into();
        app.state.selected_prisoner = Some(app.sim.world().prisoners[0].id);
        app.sim.save(&dir).expect("save test world");
        app.apply(Command::Load(dir.to_string_lossy().into_owned()));

        assert!(
            app.state
                .status
                .as_deref()
                .is_some_and(|s| s.starts_with("Loaded world"))
        );
        assert!(app.state.search.is_empty());
        assert!(app.state.selected_prisoner.is_none());
        assert!(app.snapshot.is_none());
        assert_eq!(app.cache_revision, u64::MAX);
        assert_eq!(app.cache_selection, None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
