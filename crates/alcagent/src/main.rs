use alc_core::TimeSpeed;
use alc_ui::app::{SimDriver, TuiOptions};
use clap::parser::ValueSource;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

const DEFAULT_SEED: u64 = 0xA1C_0A_6E_17_0;
const DEFAULT_SPEED: u32 = 60;
const MAX_OBSERVER_SPEED: u32 = 4_000;

fn valid_observer_speed(speed: u32) -> bool {
    (1..=MAX_OBSERVER_SPEED).contains(&speed)
}

/// True when the user supplied the argument on the command line rather than
/// relying on its default. Global args can land on either the root or the
/// selected subcommand, and clap ids are the field names (underscored).
fn explicitly_set(matches: &clap::ArgMatches, name: &str) -> bool {
    let from_cli = |m: &clap::ArgMatches| m.value_source(name) == Some(ValueSource::CommandLine);
    from_cli(matches) || matches.subcommand().is_some_and(|(_, sub)| from_cli(sub))
}

#[derive(Parser)]
#[command(
    name = "alcagent",
    version,
    about = "Autonomous Living Prison Simulation",
    long_about = "Autonomous Living Prison Simulation\n\nWith no arguments (or `observe`) the graphical observer opens at the main menu.\nThe GUI is button/menu-driven; controls are visible on screen and there are no\nhidden keyboard shortcuts.\n\nUse --tui for the ratatui debug observer. Its key map is available from the in-app ? help overlay.\nUse `run <days>` for a headless soak run. Use --load to open a save at startup,\n--seed to choose a generated world, and --save-dir/--autosave-days to configure saves.\n\nFrontends share sumo's display models and validated controls; the world is always owned by sumo::Sim."
)]
struct Cli {
    /// World-generation seed (same seed reproduces the same institution).
    #[arg(long, global = true, default_value_t = DEFAULT_SEED)]
    seed: u64,

    /// Observer speed in simulation ticks per wall-clock second (1..=4000).
    #[arg(long, global = true, default_value_t = DEFAULT_SPEED)]
    speed: u32,

    /// Launch the ratatui debug observer instead of the graphical observer.
    #[arg(long, global = true)]
    tui: bool,

    /// Legacy alias: skip the observer and run a short headless report.
    #[arg(long, global = true)]
    no_tui: bool,

    /// Load simulation state from a save directory before running.
    #[arg(
        long,
        global = true,
        help = "Load simulation state from a save directory before running"
    )]
    load: Option<std::path::PathBuf>,

    /// Directory for manual/auto saves.
    #[arg(
        long,
        global = true,
        default_value = "saves/default",
        help = "Directory for manual/auto saves"
    )]
    save_dir: std::path::PathBuf,

    /// Autosave interval in simulated days (0 disables).
    #[arg(
        long,
        global = true,
        default_value_t = 7,
        help = "Autosave interval in simulated days (0 disables)"
    )]
    autosave_days: u64,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Launch the graphical observer (the default when no args are given).
    Observe,
    /// Run headless for N days at extreme speed, printing a summary.
    Run {
        /// Number of simulated days to run.
        #[arg(default_value_t = 30)]
        days: u64,
    },
    /// Print design docs.
    Docs,
}

/// Adapts the engine (`sumo::Sim`) to the terminal observer's `SimDriver` trait.
/// The graphical observer applies validated controls directly; this adapter is
/// the ratatui debug path and preserves the runtime `AiConfig` across loads.
struct EngineDriver(sumo::Sim, alc_core::config::AiConfig);

impl SimDriver for EngineDriver {
    fn tick(&mut self) {
        self.0.tick();
    }
    fn observation(&self) -> sumo::Observation {
        self.0.observation()
    }
    fn map_frame(&self) -> sumo::MapFrame {
        self.0.map_frame()
    }
    fn stats_since(&self, at: sumo::SimTime) -> sumo::Stats {
        self.0.stats_since(at)
    }
    fn events_since(&self, at: sumo::SimTime) -> Vec<sumo::WorldEvent> {
        self.0.events_since(at)
    }
    fn recent_events(&self, n: usize) -> Vec<sumo::WorldEvent> {
        self.0.events().last(n)
    }
    fn prisoners(&self) -> Vec<sumo::PrisonerSummary> {
        self.0.prisoners()
    }
    fn gangs(&self) -> Vec<sumo::GangSummary> {
        self.0.gangs()
    }
    fn guards(&self) -> Vec<sumo::GuardSummary> {
        self.0.guards()
    }
    fn warden(&self) -> sumo::WardenSummary {
        self.0.warden()
    }
    fn ai_runtime_snapshot(&self) -> sumo::AiRuntimeSnapshot {
        self.0.ai_runtime_snapshot()
    }
    fn economy(&self) -> sumo::EconomySummary {
        self.0.economy()
    }
    fn social_edges(&self) -> Vec<sumo::SocialEdge> {
        self.0.social_edges()
    }
    fn social_roles(&self) -> Vec<sumo::SocialRoleSummary> {
        self.0.social_roles()
    }
    fn facilities(&self) -> Vec<sumo::FacilitySummary> {
        self.0.facilities()
    }
    fn construction_queue(&self) -> Vec<sumo::ConstructionSummary> {
        self.0.construction_queue()
    }
    fn speed(&self) -> TimeSpeed {
        self.0.speed()
    }
    fn set_speed(&mut self, speed: TimeSpeed) {
        self.0.set_speed(speed);
    }
    fn policy_mode(&self) -> sumo::PolicyMode {
        self.0.policy_mode()
    }
    fn set_policy_mode(&mut self, mode: sumo::PolicyMode) {
        self.0.set_policy_mode(mode);
    }
    fn sim_time(&self) -> sumo::SimTime {
        self.0.world.time
    }
    fn days_simulated(&self) -> u64 {
        self.0.world.days_simulated
    }
    fn set_lockdown(&mut self, lockdown: bool) {
        self.0.set_lockdown(lockdown);
    }
    fn set_policy(&mut self, policy: sumo::Policy) {
        self.0.set_policy(policy);
    }
    fn policy_summary(&self) -> sumo::PolicySummary {
        self.0.policy_summary()
    }
    fn set_security_level(&mut self, level: f32) {
        self.0.set_security_level(level);
    }
    fn set_privileges_level(&mut self, level: f32) {
        self.0.set_privileges_level(level);
    }
    fn set_rehab_funding(&mut self, multiplier: f32) {
        self.0.set_rehab_funding(multiplier);
    }
    fn set_intake_paused(&mut self, paused: bool) {
        self.0.set_intake_paused(paused);
    }
    fn save(&self, dir: &std::path::Path) -> Result<(), String> {
        self.0.save(dir).map_err(|e| e.to_string())
    }
    fn enable_autosave(&mut self, dir: std::path::PathBuf, interval_days: u64) {
        self.0.enable_autosave(dir, interval_days);
    }
    fn maybe_autosave(&mut self) -> Result<bool, String> {
        self.0.maybe_autosave().map_err(|e| e.to_string())
    }
    fn incident_log(&self) -> Vec<sumo::WorldEvent> {
        self.0.incident_log()
    }
    fn incident_records(&self) -> Vec<sumo::IncidentRecord> {
        self.0.incident_records()
    }
    fn incident_records_filtered(
        &self,
        filter: &sumo::incidents::IncidentFilter,
    ) -> Vec<sumo::IncidentRecord> {
        self.0.incident_records_filtered(filter)
    }
    fn decision_audit_log(&self) -> Vec<sumo::DecisionAuditRecord> {
        self.0.decision_audit_log().to_vec()
    }
    fn load(&mut self, dir: &std::path::Path) -> Result<(), String> {
        let loaded = sumo::Sim::load_with_config(dir, &self.1).map_err(|e| e.to_string())?;
        self.0 = loaded;
        Ok(())
    }
    fn post_release_status(&self, prisoner_id: u64) -> Option<bool> {
        self.0
            .post_release_status(alc_core::PrisonerId(prisoner_id))
    }
    fn post_release_records(&self) -> Vec<sumo::PostReleaseSummary> {
        self.0.post_release_records()
    }
    fn rehab_summaries(&self) -> Vec<sumo::RehabEnrollmentSummary> {
        self.0.rehab_summaries()
    }
    fn rehab_program_catalog(&self) -> Vec<sumo::RehabProgramInfo> {
        self.0.rehab_program_catalog()
    }
    fn enroll_rehab(&mut self, prisoner_id: u64, program_key: &str) -> bool {
        self.0
            .enroll_rehab_by_key(alc_core::PrisonerId(prisoner_id), program_key)
    }
    fn drop_rehab(&mut self, prisoner_id: u64, program_key: &str) -> bool {
        self.0
            .drop_rehab_by_key(alc_core::PrisonerId(prisoner_id), program_key)
    }
    fn rehab_overview(&self) -> sumo::RehabOverview {
        self.0.rehab_overview()
    }
    fn contraband_cases(&self) -> Vec<sumo::ContrabandCaseSummary> {
        self.0.contraband_summaries()
    }
    fn resolve_contraband_case(&mut self, case_id: u64) -> bool {
        self.0.resolve_contraband_case(case_id)
    }
    fn resolve_incident(&mut self, incident_id: u64) -> bool {
        self.0.resolve_incident(incident_id)
    }
    fn enqueue_construction(
        &mut self,
        kind: alc_world::FacilityKind,
        wing: u32,
        capacity: u32,
    ) -> u64 {
        self.0.enqueue_construction(kind, wing, capacity)
    }
    fn cancel_construction(&mut self, project_id: u64, reason: &str) -> bool {
        self.0.cancel_construction(project_id, reason)
    }
    fn fail_construction(&mut self, project_id: u64, reason: &str) -> bool {
        self.0.fail_construction(project_id, reason)
    }
    fn advance_construction(&mut self, project_id: u64, days: u32) -> bool {
        self.0.advance_construction(project_id, days)
    }
    fn approve_construction(&mut self, project_id: u64, reason: &str) -> bool {
        self.0.approve_construction(project_id, reason)
    }
    fn parole_due(&self) -> Vec<u64> {
        self.0.parole_due().iter().map(|id| id.raw()).collect()
    }
    fn parole_status(&self, prisoner_id: u64) -> Option<sumo::ParoleStatus> {
        self.0.parole_status(alc_core::PrisonerId(prisoner_id))
    }
    fn parole_conditions(&self, prisoner_id: u64) -> Option<sumo::ParoleConditions> {
        self.0
            .parole_conditions(alc_core::PrisonerId(prisoner_id))
            .cloned()
    }
    fn parole_decision_reason(&self, prisoner_id: u64) -> Option<String> {
        self.0
            .parole_decision_reason(alc_core::PrisonerId(prisoner_id))
            .map(str::to_string)
    }
    fn parole_released_at(&self, prisoner_id: u64) -> Option<sumo::SimTime> {
        self.0.parole_released_at(alc_core::PrisonerId(prisoner_id))
    }
    fn start_parole_hearing(&mut self, prisoner_id: u64) -> bool {
        self.0
            .start_parole_hearing(alc_core::PrisonerId(prisoner_id))
            .is_some()
    }
    fn grant_parole(&mut self, prisoner_id: u64) -> bool {
        self.0
            .grant_parole(alc_core::PrisonerId(prisoner_id))
            .is_some()
    }
    fn deny_parole(&mut self, prisoner_id: u64, reason: &str) -> bool {
        self.0
            .deny_parole(alc_core::PrisonerId(prisoner_id), reason)
            .is_some()
    }
    fn release_on_parole(&mut self, prisoner_id: u64) -> bool {
        self.0.release_on_parole(alc_core::PrisonerId(prisoner_id))
    }
    fn contraband_inventory(&self) -> Vec<sumo::ContrabandInventorySummary> {
        self.0.contraband_inventory()
    }
    fn schedule_timeline(&self, prisoner_id: u64) -> Option<sumo::ScheduleTimeline> {
        self.0.schedule_timeline(alc_core::PrisonerId(prisoner_id))
    }
    fn budget_snapshot(&self) -> sumo::BudgetSnapshot {
        self.0.budget_snapshot()
    }
    fn apply_rehab_progress(&mut self, prisoner_id: u64, minutes: u64) -> f32 {
        self.0
            .apply_rehab_progress(alc_core::PrisonerId(prisoner_id), minutes)
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches)?;
    if !valid_observer_speed(cli.speed) {
        return Err(anyhow::anyhow!(
            "invalid --speed {}: must be in 1..={}",
            cli.speed,
            MAX_OBSERVER_SPEED
        ));
    }
    if cli.tui && cli.no_tui {
        return Err(anyhow::anyhow!("--tui and --no-tui are mutually exclusive"));
    }
    if matches!(cli.cmd, Some(Cmd::Run { .. }) | Some(Cmd::Docs)) && (cli.tui || cli.no_tui) {
        return Err(anyhow::anyhow!(
            "--tui and --no-tui apply only to the observer; use them without `run` or `docs`"
        ));
    }
    match cli.cmd {
        None | Some(Cmd::Observe) => {
            if cli.no_tui {
                run_headless(
                    30,
                    cli.seed,
                    cli.load.as_deref(),
                    &cli.save_dir,
                    cli.autosave_days,
                )?;
            } else if cli.tui {
                launch_tui(
                    cli.seed,
                    cli.speed,
                    cli.load.as_deref(),
                    &cli.save_dir,
                    cli.autosave_days,
                )?;
            } else {
                // Unspecified launch flags inherit the observer preferences so
                // Settings choices survive a restart; explicit flags win.
                let settings_path = std::path::PathBuf::from("saves/alc-gui/settings.json");
                let persisted = alc_gui::Settings::load(&settings_path);
                let seed = if explicitly_set(&matches, "seed") {
                    cli.seed
                } else {
                    persisted.seed
                };
                let save_dir = if explicitly_set(&matches, "save_dir") {
                    cli.save_dir.clone()
                } else {
                    persisted.save_root()
                };
                let autosave_days = if explicitly_set(&matches, "autosave_days") {
                    cli.autosave_days
                } else {
                    persisted.autosave_days
                };
                let speed = if explicitly_set(&matches, "speed") {
                    cli.speed
                } else {
                    persisted.ticks_per_second
                };
                launch_gui(
                    seed,
                    speed,
                    cli.load.clone(),
                    &save_dir,
                    autosave_days,
                    &settings_path,
                )?;
            }
        }
        Some(Cmd::Run { days }) => run_headless(
            days,
            cli.seed,
            cli.load.as_deref(),
            &cli.save_dir,
            cli.autosave_days,
        )?,
        Some(Cmd::Docs) => {
            println!("See agents.md (directive) and docs/ (see docs/README.md)");
        }
    }
    Ok(())
}

fn default_ai_config() -> alc_core::config::AiConfig {
    alc_core::config::AiConfig::default()
}

fn load_or_new(seed: u64, load: Option<&std::path::Path>) -> anyhow::Result<sumo::Sim> {
    load_or_new_with_config(seed, load, &default_ai_config())
}

fn load_or_new_with_config(
    seed: u64,
    load: Option<&std::path::Path>,
    ai: &alc_core::config::AiConfig,
) -> anyhow::Result<sumo::Sim> {
    // Startup AI settings come from the additive `AiConfig` defaults. Runtime
    // provider/cache/telemetry stay out of the v5 save format and are restored
    // through the config-driven constructors on both fresh and loaded worlds.
    if let Some(dir) = load {
        sumo::Sim::load_with_config(dir, ai)
            .map_err(|e| anyhow::anyhow!("failed to load save from {}: {e}", dir.display()))
    } else {
        Ok(sumo::Sim::new_with_config(seed, ai))
    }
}

fn launch_gui(
    seed: u64,
    speed: u32,
    load: Option<std::path::PathBuf>,
    save_dir: &std::path::Path,
    autosave_days: u64,
    settings_path: &std::path::Path,
) -> anyhow::Result<()> {
    println!("Alcagent graphical observer starting");
    let options = alc_gui::GuiOptions {
        seed,
        load,
        save_dir: save_dir.to_path_buf(),
        settings_path: settings_path.to_path_buf(),
        ticks_per_second: speed,
        autosave_days,
    };
    alc_gui::launch(options).map_err(|err| anyhow::anyhow!("graphical observer failed: {err}"))
}

fn launch_tui(
    seed: u64,
    speed: u32,
    load: Option<&std::path::Path>,
    save_dir: &std::path::Path,
    autosave_days: u64,
) -> anyhow::Result<()> {
    let ai = default_ai_config();
    let mut sim = load_or_new_with_config(seed, load, &ai)?;
    if autosave_days > 0 {
        sim.enable_autosave(save_dir.to_path_buf(), autosave_days);
    }
    let driver = EngineDriver(sim, ai);
    alc_ui::app::run_tui(
        Box::new(driver),
        TuiOptions {
            ticks_per_second: speed,
            paused: false,
            save_dir: save_dir.to_path_buf(),
            autosave_days,
        },
    )
}

fn run_headless(
    days: u64,
    seed: u64,
    load: Option<&std::path::Path>,
    save_dir: &std::path::Path,
    autosave_days: u64,
) -> anyhow::Result<()> {
    let mut sim = load_or_new(seed, load)?;
    sim.set_speed(TimeSpeed::Extreme);
    if autosave_days > 0 {
        sim.enable_autosave(save_dir.to_path_buf(), autosave_days);
    }

    let start_day = sim.world.days_simulated;
    let start = std::time::Instant::now();
    let ticks = sim.run_days(days);
    let elapsed = start.elapsed();
    let progressed_days = sim.world.days_simulated.saturating_sub(start_day);
    if days > 0 && progressed_days == 0 {
        return Err(anyhow::anyhow!(
            "headless run made no progress after {ticks} ticks at day {}",
            sim.world.days_simulated
        ));
    }
    if let Some(err) = sim.last_autosave_error() {
        eprintln!("autosave warning: {err}");
    }
    sim.save(save_dir)
        .map_err(|e| anyhow::anyhow!("failed to save simulation to {}: {e}", save_dir.display()))?;

    println!("=== Alcagent headless run ===");
    println!(
        "Loaded from:   {}",
        load.map(|d| d.display().to_string())
            .unwrap_or_else(|| format!("new world (seed {:#x})", sim.world.seed))
    );
    println!("Save dir:        {}", save_dir.display());
    println!("Real elapsed:    {:.2?}", elapsed);
    println!("Ticks:           {ticks}");
    println!("Requested days:  {days}");
    println!("Progressed days: {progressed_days}");
    println!();
    print!("{}", sim.report());
    println!();
    println!("Recent events (last 10):");
    for e in sim.world.events.last_reversed(10) {
        println!("  [{}] {:?} — {}", e.at, e.kind, e.summary);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observer_speed_bounds_match_tui() {
        assert!(!valid_observer_speed(0));
        assert!(valid_observer_speed(1));
        assert!(valid_observer_speed(DEFAULT_SPEED));
        assert!(valid_observer_speed(MAX_OBSERVER_SPEED));
        assert!(!valid_observer_speed(MAX_OBSERVER_SPEED + 1));
    }
}
