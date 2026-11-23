//! Persistence roundtrip tests (§23).

use alc_core::TimeSpeed;
use sumo::Sim;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sumo-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn save_load_roundtrip() {
    let dir = temp_dir("save");
    let mut sim = Sim::new(42);
    sim.set_speed(TimeSpeed::FastForward);
    sim.run_until(sim.world().time.add_days(3));
    sim.save(&dir).expect("save");
    let loaded = Sim::load(&dir).expect("load");
    assert_eq!(loaded.world().time, sim.world().time);
    assert_eq!(loaded.world().population(), sim.world().population());
    assert_eq!(loaded.world().days_simulated, sim.world().days_simulated);
    assert_eq!(loaded.world().economy.cash.0, sim.world().economy.cash.0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn autosave_creates_save_file() {
    let dir = temp_dir("autosave");
    let mut sim = Sim::new(43);
    sim.enable_autosave(&dir, 1);
    sim.set_speed(TimeSpeed::FastForward);
    sim.run_until(sim.world().time.add_days(3));
    assert!(std::path::Path::new(&dir).join("world.bin").exists());
    std::fs::remove_dir_all(&dir).ok();
}
