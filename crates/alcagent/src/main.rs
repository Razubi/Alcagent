use alc_core::TimeSpeed;
use alc_ui::app::{SimDriver, TuiOptions};
use clap::{Parser, Subcommand};

const DEFAULT_SEED: u64 = 0xA1C_0A_6E_17_0;
const DEFAULT_SPEED: u32 = 60;

#[derive(Parser)]
#[command(
    name = "alcagent",
    version,
    about = "Autonomous Living Prison Simulation",
    long_about = "Autonomous Living Prison Simulation\n\nWith no arguments (or `observe`) an interactive ratatui observer opens.\n\nTUI controls:\n  q / Q / Esc / Ctrl-C   quit\n  Space                  pause / resume\n  + / =                  speed up\n  - / _                  speed down\n  1-9                    switch observation view\n  Up/Down/Left/Right or k/j/h/l   move the selection cursor\n  S                      manual save to --save-dir\n  Ctrl-L                 quick load from --save-dir\n  x / X                  toggle lockdown\n  N/E/K                  policy band (Normal/Enhanced/Lockdown)\n  L/U                    lockdown enable/lift\n  [/]                    security level down/up\n  ,/.                    privileges down/up\n  ;/'                    rehab funding down/up\n  B                      toggle intake pause\n  P                      cycle policy mode (Deterministic/Council/Shadow)\n\nUse `run <days>` for a headless soak run, or `--no-tui` for a short report."
)]
struct Cli {
    /// World-generation seed (same seed reproduces the same institution).
    #[arg(long, global = true, default_value_t = DEFAULT_SEED)]
    seed: u64,

    /// Observer speed in simulation ticks per wall-clock second (min 1).
    #[arg(long, global = true, default_value_t = DEFAULT_SPEED)]
    speed: u32,

    /// Disable the TUI: `observe` falls back to a short headless report.
    /// Useful for CI and non-interactive shells.
    #[arg(long, global = true)]
    no_tui: bool,

    /// Load simulation state from a save directory before running.
    #[arg(long, global = true, help = "Load simulation state from a save directory before running")]
    load: Option<std::path::PathBuf>,

    /// Directory for manual/auto saves.
    #[arg(long, global = true, default_value = "saves/default", help = "Directory for manual/auto saves")]
    save_dir: std::path::PathBuf,

    /// Autosave interval in simulated days (0 disables).
    #[arg(long, global = true, default_value_t = 7, help = "Autosave interval in simulated days (0 disables)")]
    autosave_days: u64,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Launch the interactive TUI observer (the default when no args are given).
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

/// Adapts the engine (`sumo::Sim`) to the UI's `SimDriver` trait. Implemented
/// here — not in `alc-ui` — because `sumo` already depends on `alc-ui` and a
/// direct `alc-ui -> sumo` dependency would be a cycle.
struct EngineDriver(sumo::Sim);

impl SimDriver for EngineDriver {
    fn tick(&mut self) { self.0.tick(); }
    fn observation(&self) -> sumo::Observation { self.0.observation() }
    fn map_frame(&self) -> sumo::MapFrame { self.0.map_frame() }
    fn stats_since(&self, at: sumo::SimTime) -> sumo::Stats { self.0.stats_since(at) }
    fn events_since(&self, at: sumo::SimTime) -> Vec<sumo::WorldEvent> {
        self.0.events_since(at)
    }
    fn prisoners(&self) -> Vec<sumo::PrisonerSummary> { self.0.prisoners() }
    fn gangs(&self) -> Vec<sumo::GangSummary> { self.0.gangs() }
    fn guards(&self) -> Vec<sumo::GuardSummary> { self.0.guards() }
    fn warden(&self) -> sumo::WardenSummary { self.0.warden() }
    fn economy(&self) -> sumo::EconomySummary { self.0.economy() }
    fn social_edges(&self) -> Vec<sumo::SocialEdge> { self.0.social_edges() }
    fn facilities(&self) -> Vec<sumo::FacilitySummary> { self.0.facilities() }
    fn construction_queue(&self) -> Vec<sumo::ConstructionSummary> { self.0.construction_queue() }
    fn speed(&self) -> TimeSpeed { self.0.speed() }
    fn set_speed(&mut self, speed: TimeSpeed) { self.0.set_speed(speed); }
    fn policy_mode(&self) -> sumo::PolicyMode { self.0.policy_mode() }
    fn set_policy_mode(&mut self, mode: sumo::PolicyMode) { self.0.set_policy_mode(mode); }
    fn sim_time(&self) -> sumo::SimTime { self.0.world.time }
    fn days_simulated(&self) -> u64 { self.0.world.days_simulated }
    fn set_lockdown(&mut self, lockdown: bool) { self.0.set_lockdown(lockdown); }
    fn set_policy(&mut self, policy: sumo::Policy) { self.0.set_policy(policy); }
    fn policy_summary(&self) -> sumo::PolicySummary { self.0.policy_summary() }
    fn set_security_level(&mut self, level: f32) { self.0.set_security_level(level); }
    fn set_privileges_level(&mut self, level: f32) { self.0.set_privileges_level(level); }
    fn set_rehab_funding(&mut self, multiplier: f32) { self.0.set_rehab_funding(multiplier); }
    fn set_intake_paused(&mut self, paused: bool) { self.0.set_intake_paused(paused); }
    fn save(&self, dir: &std::path::Path) -> Result<(), String> { self.0.save(dir).map_err(|e| e.to_string()) }
    fn enable_autosave(&mut self, dir: std::path::PathBuf, interval_days: u64) { self.0.enable_autosave(dir, interval_days); }
    fn maybe_autosave(&mut self) -> Result<bool, String> { self.0.maybe_autosave().map_err(|e| e.to_string()) }
    fn incident_log(&self) -> Vec<sumo::WorldEvent> { self.0.incident_log() }
    fn incident_records(&self) -> Vec<sumo::IncidentRecord> { self.0.incident_records() }
    fn incident_records_filtered(&self, filter: &sumo::incidents::IncidentFilter) -> Vec<sumo::IncidentRecord> { self.0.incident_records_filtered(filter) }
    fn decision_audit_log(&self) -> Vec<sumo::DecisionAuditRecord> { self.0.decision_audit_log().to_vec() }
    fn load(&mut self, dir: &std::path::Path) -> Result<(), String> {
        let loaded = sumo::Sim::load(dir).map_err(|e| e.to_string())?;
        self.0 = loaded;
        Ok(())
    }
    fn post_release_status(&self, prisoner_id: u64) -> Option<bool> { self.0.post_release_status(alc_core::PrisonerId(prisoner_id)) }
    fn post_release_records(&self) -> Vec<sumo::PostReleaseSummary> { self.0.post_release_records() }
    fn rehab_summaries(&self) -> Vec<sumo::RehabEnrollmentSummary> { self.0.rehab_summaries() }
    fn rehab_program_catalog(&self) -> Vec<sumo::RehabProgramInfo> { self.0.rehab_program_catalog() }
    fn enroll_rehab(&mut self, prisoner_id: u64, program_key: &str) -> bool { self.0.enroll_rehab_by_key(alc_core::PrisonerId(prisoner_id), program_key) }
    fn drop_rehab(&mut self, prisoner_id: u64, program_key: &str) -> bool { self.0.drop_rehab_by_key(alc_core::PrisonerId(prisoner_id), program_key) }
    fn rehab_overview(&self) -> sumo::RehabOverview { self.0.rehab_overview() }
    fn contraband_cases(&self) -> Vec<sumo::ContrabandCaseSummary> { self.0.contraband_summaries() }
    fn resolve_contraband_case(&mut self, case_id: u64) -> bool { self.0.resolve_contraband_case(case_id) }
    fn resolve_incident(&mut self, incident_id: u64) -> bool { self.0.resolve_incident(incident_id) }
    fn enqueue_construction(&mut self, kind: alc_world::FacilityKind, wing: u32, capacity: u32) -> u64 { self.0.enqueue_construction(kind, wing, capacity) }
    fn cancel_construction(&mut self, project_id: u64, reason: &str) -> bool { self.0.cancel_construction(project_id, reason) }
    fn fail_construction(&mut self, project_id: u64, reason: &str) -> bool { self.0.fail_construction(project_id, reason) }
    fn advance_construction(&mut self, project_id: u64, days: u32) -> bool { self.0.advance_construction(project_id, days) }
    fn contraband_inventory(&self) -> Vec<sumo::ContrabandInventorySummary> { self.0.contraband_inventory() }
    fn schedule_timeline(&self, prisoner_id: u64) -> Option<sumo::ScheduleTimeline> { self.0.schedule_timeline(alc_core::PrisonerId(prisoner_id)) }
    fn budget_snapshot(&self) -> sumo::BudgetSnapshot { self.0.budget_snapshot() }
    fn apply_rehab_progress(&mut self, prisoner_id: u64, minutes: u64) -> f32 { self.0.apply_rehab_progress(alc_core::PrisonerId(prisoner_id), minutes) }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    if cli.speed == 0 {
        return Err(anyhow::anyhow!("invalid --speed {}: must be at least 1", cli.speed));
    }
    match cli.cmd {
        None | Some(Cmd::Observe) => {
            if cli.no_tui {
                run_headless(30, cli.seed, cli.load.as_deref(), &cli.save_dir, cli.autosave_days)?;
            } else {
                launch_tui(cli.seed, cli.speed, cli.load.as_deref(), &cli.save_dir, cli.autosave_days)?;
            }
        }
        Some(Cmd::Run { days }) => run_headless(days, cli.seed, cli.load.as_deref(), &cli.save_dir, cli.autosave_days)?,
        Some(Cmd::Docs) => {
            println!("See agents.md (directive) and Documentation/ (index, design, systems, reports)");
        }
    }
    Ok(())
}

fn load_or_new(seed: u64, load: Option<&std::path::Path>) -> anyhow::Result<sumo::Sim> {
    if let Some(dir) = load {
        sumo::Sim::load(dir).map_err(|e| {
            anyhow::anyhow!("failed to load save from {}: {e}", dir.display())
        })
    } else {
        Ok(sumo::Sim::new(seed))
    }
}

fn launch_tui(seed: u64, speed: u32, load: Option<&std::path::Path>, save_dir: &std::path::Path, autosave_days: u64) -> anyhow::Result<()> {
    let mut sim = load_or_new(seed, load)?;
    if autosave_days > 0 {
        sim.enable_autosave(save_dir.to_path_buf(), autosave_days);
    }
    let driver = EngineDriver(sim);
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

fn run_headless(days: u64, seed: u64, load: Option<&std::path::Path>, save_dir: &std::path::Path, autosave_days: u64) -> anyhow::Result<()> {
    let mut sim = load_or_new(seed, load)?;
    sim.set_speed(TimeSpeed::Extreme);
    if autosave_days > 0 {
        sim.enable_autosave(save_dir.to_path_buf(), autosave_days);
    }

    let start = std::time::Instant::now();
    let target_day = sim.world.days_simulated + days;
    while sim.world.days_simulated < target_day {
        sim.tick();
        let _ = sim.maybe_autosave();
    }
    if let Err(e) = sim.save(save_dir) {
        return Err(anyhow::anyhow!("failed to save simulation to {}: {e}", save_dir.display()));
    }
    let elapsed = start.elapsed();

    let world = &sim.world;
    println!("=== Alcagent headless run ===");
    println!("Loaded from:   {}", load.map(|d| d.display().to_string()).unwrap_or_else(|| format!("new world (seed {:#x})", world.seed)));
    println!("Seed:            {:#x}", world.seed);
    println!("Sim time:        {}", world.time);
    println!("Days simulated:  {}", world.days_simulated);
    println!("Population:      {}", world.population());
    println!("Capacity:        {}", world.capacity());
    println!("Guards:          {}", world.guards.len());
    println!("Facilities:      {}", world.facilities.len());
    println!("Cash (major):    {:.2}", world.economy.cash.as_major());
    println!("Events logged:   {}", world.events.len());
    println!("Save dir:        {}", save_dir.display());
    println!("Real elapsed:    {:.2?}", elapsed);
    println!(
        "Ticks/sec:       {:.0}",
        world.days_simulated as f64 / elapsed.as_secs_f64().max(0.001)
    );
    println!();
    println!("Recent events (last 10):");
    let all: Vec<&alc_events::WorldEvent> = world.events.recent().collect();
    for e in all.iter().rev().take(10) {
        println!("  [{}] {} — {}", e.at, format!("{:?}", e.kind), e.summary);
    }
    Ok(())
}
