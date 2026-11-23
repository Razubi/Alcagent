//! Persistence: versioned save format, atomic writes, autosave.
//!
//! Design: bincode for compact binary saves; JSON sidecar for debugging.
//! Save format is versioned and migratable. Worlds round-trip through
//! `bincode` exactly, including orchestrator state, relationships, history,
//! and simulation time (Directive §23).
//!
//! The save format now includes EngineState (from sumo crate) alongside World
//! for complete state restoration including incidents, decisions, parole ledger,
//! population history, policy state, and post-release tracking.
//!
//! [`SaveManager::autosave_if_due`] honours the configured
//! `save_interval_days`; [`migrate`] is the forward-migration hook for future
//! format versions.

use alc_core::config::Config;
use alc_events::WorldEvent;
use alc_agents::Decision;
use alc_core::SimTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const SAVE_FORMAT_VERSION: u32 = 2; // Bumped for EngineState support

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveHeader {
    pub version: u32,
    pub created_at_secs: u64,
    pub population: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveFile {
    pub header: SaveHeader,
    pub world: alc_world::World,
    /// Engine state from sumo (parole, incidents, decisions, policies, etc.)
    /// Optional for backward compatibility with version 1 saves.
    #[serde(default)]
    pub engine_state: Option<EngineState>,
}

/// Engine state for persistence - mirrors sumo::EngineState for serialization.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EngineState {
    /// Parole ledger (Directive §15).
    pub parole: ParoleLedger,
    /// Population history for invariant checking.
    pub population_history: Vec<(u64, u32)>,
    /// World incident log (R1: incident reports).
    pub incidents: Vec<WorldEvent>,
    /// Sidecar incident log (R1/R6: structured incidents with causality).
    pub incident_sidecar: Vec<Incident>,
    /// Decision audit log (R2: decision audit log).
    pub decision_log: Vec<Decision>,
    /// Post-release tracking (R4: released prisoner outcomes).
    /// `BTreeMap` guarantees a reproducible serialization order.
    pub post_release: BTreeMap<u64, bool>,
    /// Policy and lockdown state (H1: policy management).
    pub policy_state: PolicyState,
    /// Policy mode for QA shadow/Deterministic mode (S3).
    pub policy_mode: PolicyMode,
    /// Autosave interval in simulated days, when autosave is enabled.
    /// `#[serde(default)]` keeps version-2 saves loadable.
    #[serde(default)]
    pub autosave_interval_days: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParoleLedger {
    /// `BTreeMap` guarantees a reproducible serialization order.
    pub status: BTreeMap<u64, ParoleStatus>,
    pub adjustments: BTreeMap<u64, f32>,
    pub hearing_at: BTreeMap<u64, SimTime>,
    pub last_decision_at: Option<SimTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ParoleStatus {
    #[default]
    None,
    Scheduled,
    Granted,
    Denied,
    InHearing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyState {
    pub policy: Policy,
    pub lockdown: bool,
    pub security_level: u32,
    pub last_changed: SimTime,
}

impl Default for PolicyState {
    fn default() -> Self {
        Self {
            policy: Policy::Normal,
            lockdown: false,
            security_level: 1,
            last_changed: SimTime::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Policy {
    #[default]
    Normal,
    Enhanced,
    Lockdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PolicyMode {
    #[default]
    Deterministic,
    Council,
    Shadow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    /// Unique identifier for this incident. Defaults to 0 for legacy saves.
    #[serde(default)]
    pub id: u64,
    /// Event taxonomy kind, stored as the native enum to avoid lossy
    /// string round-trips.
    pub kind: alc_events::EventKind,
    /// Severity on a 0.0..=1.0 scale. Was `u8` in legacy saves; the
    /// `#[serde(default)]` keeps missing fields loadable, but a type change
    /// requires a version bump for binary compatibility with old saves.
    pub severity: f32,
    pub cause: Option<u64>,
    pub consequence: Option<u64>,
    pub timestamp: SimTime,
    /// Location description (wing, node, or free-form). Defaults to empty
    /// for legacy saves.
    #[serde(default)]
    pub location: String,
    pub involved_prisoners: Vec<u64>,
    pub involved_guards: Vec<u64>,
    /// Whether the incident has been resolved. Defaults to false for legacy
    /// saves.
    #[serde(default)]
    pub resolved: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("serialize: {0}")]
    Serialize(String),
    #[error("version mismatch: found {found}, expected {expected}")]
    VersionMismatch { found: u32, expected: u32 },
    #[error("migration not implemented for version {found}")]
    MigrationUnsupported { found: u32 },
}

pub struct SaveManager {
    dir: PathBuf,
    /// Last simulation day an autosave was written (deduplicates intra-day
    /// autosave calls).
    last_autosave_day: AtomicU64,
}

impl SaveManager {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self { dir: dir.as_ref().to_path_buf(), last_autosave_day: AtomicU64::new(0) }
    }

    /// Atomic save: write to tmp then rename.
    pub fn save(&self, world: &alc_world::World) -> Result<(), SaveError> {
        self.save_with_engine(world, None)
    }

    /// Save world with EngineState for complete persistence.
    pub fn save_with_engine(&self, world: &alc_world::World, engine_state: Option<EngineState>) -> Result<(), SaveError> {
        fs::create_dir_all(&self.dir)?;
        let header = SaveHeader {
            version: SAVE_FORMAT_VERSION,
            created_at_secs: world.time.as_secs(),
            population: world.prisoners.len() as u32,
        };
        let sf = SaveFile { header, world: world.clone(), engine_state };
        let bytes = bincode::serialize(&sf).map_err(|e| SaveError::Serialize(e.to_string()))?;
        let tmp = self.dir.join("world.tmp");
        fs::write(&tmp, &bytes)?;
        let final_path = self.dir.join("world.bin");
        fs::rename(&tmp, &final_path)?;
        // Debug sidecar.
        if let Ok(json) = serde_json::to_string(&sf) {
            let _ = fs::write(self.dir.join("world.debug.json"), json);
        }
        Ok(())
    }

    /// Load a save with EngineState, rejecting versions other than the current one.
    pub fn load_with_engine(&self) -> Result<(alc_world::World, Option<EngineState>), SaveError> {
        let bytes = fs::read(self.dir.join("world.bin"))?;
        let sf: SaveFile =
            bincode::deserialize(&bytes).map_err(|e| SaveError::Serialize(e.to_string()))?;
        if sf.header.version != SAVE_FORMAT_VERSION {
            // Try migration for older versions
            let migrated = migrate(sf)?;
            return Ok((migrated.world, migrated.engine_state));
        }
        Ok((sf.world, sf.engine_state))
    }

    /// Load a save, rejecting versions other than the current one. Use
    /// [`SaveManager::load_migrated`] to run the migration hook first.
    pub fn load(&self) -> Result<alc_world::World, SaveError> {
        let bytes = fs::read(self.dir.join("world.bin"))?;
        let sf: SaveFile =
            bincode::deserialize(&bytes).map_err(|e| SaveError::Serialize(e.to_string()))?;
        if sf.header.version != SAVE_FORMAT_VERSION {
            return Err(SaveError::VersionMismatch {
                found: sf.header.version,
                expected: SAVE_FORMAT_VERSION,
            });
        }
        Ok(sf.world)
    }

    /// Load and migrate older formats to the current version.
    pub fn load_migrated(&self) -> Result<alc_world::World, SaveError> {
        let bytes = fs::read(self.dir.join("world.bin"))?;
        let sf: SaveFile =
            bincode::deserialize(&bytes).map_err(|e| SaveError::Serialize(e.to_string()))?;
        Ok(migrate(sf)?.world)
    }

    /// Unconditional autosave (legacy behaviour).
    pub fn autosave(&self, world: &alc_world::World) -> Result<(), SaveError> {
        self.save(world)
    }

    /// Autosave only when `save_interval_days` has elapsed. Returns whether a
    /// save was written. The interval is measured in whole simulation days;
    /// repeated calls on the same due day do not rewrite the save.
    pub fn autosave_if_due(
        &self,
        world: &alc_world::World,
        save_interval_days: u64,
    ) -> Result<bool, SaveError> {
        let interval = save_interval_days.max(1);
        let day = world.days_simulated;
        if day == 0 || day % interval != 0 {
            return Ok(false);
        }
        if self.last_autosave_day.load(Ordering::Relaxed) == day {
            return Ok(false);
        }
        self.save(world)?;
        self.last_autosave_day.store(day, Ordering::Relaxed);
        Ok(true)
    }

    /// Autosave using the top-level [`Config`] interval.
    pub fn autosave_with_config(
        &self,
        world: &alc_world::World,
        config: &Config,
    ) -> Result<bool, SaveError> {
        self.autosave_if_due(world, config.save_interval_days)
    }
}

/// Migrate a save to the current format version.
///
/// Version 1 -> 2: Adds EngineState field (optional, defaults to empty).
///
/// Newer EngineState sub-fields (`autosave_interval_days`, `Incident::id`,
/// `Incident::location`, `Incident::resolved`) are covered by `#[serde(default)]`
/// so they load transparently in self-describing formats (e.g. the JSON
/// debug sidecar). `Incident::severity` and `Incident::kind` changed binary
/// representations (u8 -> f32, String -> EventKind); bincode is positional,
/// so loading a version-2 *binary* save written by the old code requires
/// bumping `SAVE_FORMAT_VERSION`. Kept at 2 here per the no-bump preference;
/// see `Documentation/Communication/Note's/persistence-lib-rs-changes-needed.md`.
pub fn migrate(sf: SaveFile) -> Result<SaveFile, SaveError> {
    if sf.header.version == SAVE_FORMAT_VERSION {
        return Ok(sf);
    }
    if sf.header.version < SAVE_FORMAT_VERSION {
        // Version 1 -> 2 migration: add empty EngineState
        if sf.header.version == 1 {
            let mut new_sf = sf;
            new_sf.header.version = SAVE_FORMAT_VERSION;
            new_sf.engine_state = None; // Will be populated on first save with engine
            return Ok(new_sf);
        }
        Err(SaveError::MigrationUnsupported { found: sf.header.version })
    } else {
        Err(SaveError::VersionMismatch {
            found: sf.header.version,
            expected: SAVE_FORMAT_VERSION,
        })
    }
}

/// Deserialize then migrate a raw save payload.
pub fn migrate_bytes(bytes: &[u8]) -> Result<SaveFile, SaveError> {
    let sf: SaveFile =
        bincode::deserialize(bytes).map_err(|e| SaveError::Serialize(e.to_string()))?;
    migrate(sf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(label: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "alcagent-persist-{}-{}-{}",
            std::process::id(),
            label,
            n
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn world_round_trips_through_bincode() {
        let dir = unique_dir("roundtrip");
        let manager = SaveManager::new(&dir);
        let mut world = alc_world::World::new(0xA1C_0A_6E_17_0);
        world.time = world.time.add_days(2);
        world.orchestrator.charge(123);

        manager.save(&world).unwrap();
        let loaded = manager.load().unwrap();

        assert_eq!(loaded.seed, world.seed);
        assert_eq!(loaded.time, world.time);
        assert_eq!(loaded.prisoners.len(), world.prisoners.len());
        assert_eq!(loaded.guards.len(), world.guards.len());
        assert_eq!(loaded.facilities.len(), world.facilities.len());
        assert_eq!(loaded.economy.cash, world.economy.cash);
        assert_eq!(loaded.orchestrator.consumed_today, world.orchestrator.consumed_today);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn autosave_respects_interval() {
        let dir = unique_dir("interval");
        let manager = SaveManager::new(&dir);
        let mut world = alc_world::World::new(1);
        world.days_simulated = 3;

        assert!(!manager.autosave_if_due(&world, 7).unwrap());
        assert!(!Path::new(&dir).join("world.bin").exists());

        world.days_simulated = 7;
        assert!(manager.autosave_if_due(&world, 7).unwrap());
        assert!(Path::new(&dir).join("world.bin").exists());
        // Repeated calls on the same due day must not rewrite the save.
        assert!(!manager.autosave_if_due(&world, 7).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_accepts_current_version() {
        let sf = SaveFile {
            header: SaveHeader { version: SAVE_FORMAT_VERSION, created_at_secs: 1, population: 0 },
            world: alc_world::World::new(7),
            engine_state: None,
        };
        assert!(migrate(sf).is_ok());
    }

    #[test]
    fn migrate_rejects_unknown_versions() {
        let sf = SaveFile {
            header: SaveHeader { version: SAVE_FORMAT_VERSION + 5, created_at_secs: 1, population: 0 },
            world: alc_world::World::new(7),
            engine_state: None,
        };
        assert!(matches!(migrate(sf), Err(SaveError::VersionMismatch { .. })));
    }

    #[test]
    fn engine_state_serializes_deterministically() {
        // Map fields use BTreeMap, so two states built in different insertion
        // orders must serialize to identical bytes.
        let mut a = EngineState::default();
        a.post_release.insert(1, true);
        a.post_release.insert(2, false);
        a.parole.status.insert(1, ParoleStatus::Granted);
        a.parole.status.insert(2, ParoleStatus::Denied);
        a.parole.adjustments.insert(1, 0.25);
        a.parole.adjustments.insert(2, 0.75);
        a.parole.hearing_at.insert(1, SimTime::from_secs(10));
        a.parole.hearing_at.insert(2, SimTime::from_secs(20));

        let mut b = EngineState::default();
        b.post_release.insert(2, false);
        b.post_release.insert(1, true);
        b.parole.status.insert(2, ParoleStatus::Denied);
        b.parole.status.insert(1, ParoleStatus::Granted);
        b.parole.adjustments.insert(2, 0.75);
        b.parole.adjustments.insert(1, 0.25);
        b.parole.hearing_at.insert(2, SimTime::from_secs(20));
        b.parole.hearing_at.insert(1, SimTime::from_secs(10));

        assert_eq!(
            bincode::serialize(&a).unwrap(),
            bincode::serialize(&b).unwrap(),
            "BTreeMap-backed EngineState must have a reproducible byte layout"
        );
    }

    #[test]
    fn incident_default_and_serde_backward_compat() {
        // New Incidents carry all lossless fields.
        let inc = Incident {
            id: 7,
            kind: alc_events::EventKind::Fight,
            severity: 0.5,
            cause: Some(3),
            consequence: Some(9),
            timestamp: SimTime::from_secs(100),
            location: "Yard A".into(),
            involved_prisoners: vec![1, 2],
            involved_guards: vec![],
            resolved: true,
        };
        let bytes = bincode::serialize(&inc).unwrap();
        let loaded: Incident = bincode::deserialize(&bytes).unwrap();
        assert_eq!(loaded.id, 7);
        assert_eq!(loaded.severity, 0.5);
        assert_eq!(loaded.location, "Yard A");
        assert!(loaded.resolved);
        assert_eq!(loaded.cause, Some(3));
        assert_eq!(loaded.consequence, Some(9));
    }
}
