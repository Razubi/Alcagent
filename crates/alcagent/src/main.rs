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
    long_about = "Autonomous Living Prison Simulation\n\nWith no arguments (or `observe`) an interactive ratatui observer opens.\n\nTUI controls:\n  q / Q / Esc / Ctrl-C   quit\n  Space                  pause / resume\n  + / =                  speed up\n  - / _                  speed down\n  1-9                    switch observation view\n  Up/Down/Left/Right or k/j/h/l   move the selection cursor\n\nUse `run <days>` for a headless soak run, or `--no-tui` for a short report."
)]
struct Cli {
    /// World-generation seed (same seed reproduces the same institution).
    #[arg(long, global = true, default_value_t = DEFAULT_SEED)]
    seed: u64,

    /// Observer speed in simulation ticks per wall-clock second.
    #[arg(long, global = true, default_value_t = DEFAULT_SPEED)]
    speed: u32,

    /// Disable the TUI: `observe` falls back to a short headless report.
    /// Useful for CI and non-interactive shells.
    #[arg(long, global = true)]
    no_tui: bool,

    /// Load simulation state from a save directory before running.
    #[arg(long, global = true)]
    load: Option<std::path::PathBuf>,

    /// Directory for manual/auto saves.
    #[arg(long, global = true, default_value = "saves/default")]
    save_dir: std::path::PathBuf,

    /// Autosave interval in simulated days (0 disables).
    #[arg(long, global = true, default_value_t = 7)]
    autosave_days: u64,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Launch the interactive TUI observer (the default when no args are given).
    Observe,
    /// Run headless for N days at extreme speed, printing a summary.
    Run { days: u64 },
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
    fn speed(&self) -> TimeSpeed { self.0.speed() }
    fn set_speed(&mut self, speed: TimeSpeed) { self.0.set_speed(speed); }
    fn policy_mode(&self) -> sumo::PolicyMode { self.0.policy_mode() }
    fn set_policy_mode(&mut self, mode: sumo::PolicyMode) { self.0.set_policy_mode(mode); }
    fn sim_time(&self) -> sumo::SimTime { self.0.world.time }
    fn days_simulated(&self) -> u64 { self.0.world.days_simulated }
    fn set_lockdown(&mut self, lockdown: bool) { self.0.set_lockdown(lockdown); }
    fn set_policy(&mut self, policy: sumo::Policy) { self.0.set_policy(policy); }
    fn save(&self, dir: &std::path::Path) -> Result<(), String> { self.0.save(dir).map_err(|e| e.to_string()) }
    fn enable_autosave(&mut self, dir: std::path::PathBuf, interval_days: u64) { self.0.enable_autosave(dir, interval_days); }
    fn maybe_autosave(&mut self) -> Result<bool, String> { self.0.maybe_autosave().map_err(|e| e.to_string()) }
    fn incident_log(&self) -> Vec<sumo::WorldEvent> { self.0.incident_log() }
    fn decision_log(&self) -> Vec<String> {
        self.0.decision_log().iter().map(|d| format!("{:?}", d)).collect()
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
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
        Ok(sumo::Sim::load(dir)?)
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
        driver,
        TuiOptions {
            ticks_per_second: speed,
            paused: false,
            save_dir: save_dir.to_path_buf(),
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
    let _ = sim.save(save_dir);
    let elapsed = start.elapsed();

    let world = &sim.world;
    println!("=== Alcagent headless run ===");
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
