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
//!
//! ## Version compatibility
//!
//! Version 3 added the decision audit log, autosave interval, policy/intake
//! flags and the world construction queue. Version 4 added structured incident
//! fields and contraband resolution timestamps. 0.05 state structs carry
//! container-level `#[serde(default)]` where safe, and loading is version-aware:
//! the format version is peeked from the bincode header before deserialization
//! so older/newer/corrupt saves produce an actionable error. Known legacy
//! bincode layouts (v1, v3, v4) are migrated explicitly; other older
//! serde-default compatible layouts are stamped forward when decodable.
//! Version 5 appends parole decision conditions/reasons/release times.

use alc_agents::Decision;
use alc_core::SimTime;
use alc_core::config::Config;
use alc_events::WorldEvent;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const SAVE_FORMAT_VERSION: u32 = 5; // 0.05: persisted parole conditions/decisions/releases

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
///
/// Container-level `#[serde(default)]` is intentional: pre-0.05 saves do not
/// contain `decision_audit` or `autosave_interval_days`, and future versions
/// may append further fields. Missing fields get a sane empty value instead of
/// making an otherwise loadable save fail.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
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
    /// Structured decision audit records for observer UI (0.05).
    #[serde(default)]
    pub decision_audit: Vec<DecisionAuditRecord>,
    /// Contraband case IDs resolved by observer/control (0.05).
    #[serde(default)]
    pub resolved_contraband_cases: std::collections::BTreeSet<u64>,
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
    /// Last autosave day (sim days). `#[serde(default)]` for older saves.
    #[serde(default)]
    pub last_autosave_day: Option<u64>,
    /// Contraband case IDs resolved by observer/control with their sim times.
    /// Engine-only state in older saves; persisted since format v4.
    #[serde(default)]
    pub resolved_contraband_at: BTreeMap<u64, SimTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ParoleLedger {
    /// `BTreeMap` guarantees a reproducible serialization order.
    pub status: BTreeMap<u64, ParoleStatus>,
    pub adjustments: BTreeMap<u64, f32>,
    pub hearing_at: BTreeMap<u64, SimTime>,
    pub last_decision_at: Option<SimTime>,
    /// Grant conditions (spec 006). `#[serde(default)]` keeps older saves loadable.
    pub conditions: BTreeMap<u64, ParoleConditions>,
    /// Persisted decision reasons (spec 006).
    pub decision_reason: BTreeMap<u64, String>,
    /// Time at which a granted prisoner left custody (spec 006).
    pub released_at: BTreeMap<u64, SimTime>,
}

/// Persisted mirror of `sumo::ParoleConditions` (spec 006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ParoleConditions {
    pub reporting_per_month: u8,
    pub supervision_months: u16,
    pub curfew: bool,
    pub drug_testing: bool,
    pub treatment_required: bool,
    pub no_contact_order: bool,
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
#[serde(default)]
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
#[serde(default)]
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
    /// Full causal chain leading to this incident (sumo::incidents::CausalLink).
    #[serde(default)]
    pub causal_chain: Vec<PersistedCausalLink>,
    /// Witnesses to this incident (prisoner ids).
    #[serde(default)]
    pub witnesses: Vec<u64>,
    /// Disposition after investigation/response.
    #[serde(default)]
    pub disposition: PersistedIncidentDisposition,
    /// Response actions taken in reaction to this incident.
    #[serde(default)]
    pub response_actions: Vec<PersistedResponseAction>,
    /// Whether the incident has been resolved. Defaults to false for legacy
    /// saves.
    #[serde(default)]
    pub resolved: bool,
}

/// Persistence mirror of `sumo::incidents::CausalLink`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PersistedCausalLink {
    pub source_id: u64,
    pub is_incident: bool,
    pub description: String,
}

/// Persistence mirror of `sumo::incidents::IncidentDisposition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PersistedIncidentDisposition {
    #[default]
    None,
    Investigated,
    Disciplinary,
    Medical,
    Resolved,
}

/// Persistence mirror of `sumo::incidents::ResponseAction`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PersistedResponseAction {
    pub action: String,
    pub responder: u64,
    pub responder_kind: String,
    pub at: SimTime,
    pub description: String,
    pub success: bool,
}

impl Default for Incident {
    fn default() -> Self {
        Self {
            id: 0,
            kind: alc_events::EventKind::Argument,
            severity: 0.0,
            cause: None,
            consequence: None,
            timestamp: SimTime::default(),
            location: String::new(),
            involved_prisoners: Vec::new(),
            involved_guards: Vec::new(),
            causal_chain: Vec::new(),
            witnesses: Vec::new(),
            disposition: PersistedIncidentDisposition::None,
            response_actions: Vec::new(),
            resolved: false,
        }
    }
}

/// Structured decision audit record for observer UI (0.05).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DecisionAuditRecord {
    pub time: SimTime,
    pub actor: String,
    pub source: String,
    pub action: String,
    pub summary: String,
    pub validation: String,
    pub reason: String,
    /// Detailed rationale for the decision.
    #[serde(default)]
    pub rationale: String,
    /// If repaired/clamped, the reason for the repair.
    #[serde(default)]
    pub repair_reason: String,
    /// Outcome state (Accepted, Repaired, Rejected, Proposed).
    #[serde(default)]
    pub outcome: alc_agents::decision::DecisionOutcomeState,
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("serialize: {0}")]
    Serialize(String),
    /// A save could not be decoded. Carries the version peeked from the
    /// bincode header (when available) so callers can tell a corrupt save
    /// from an incompatible format version.
    #[error("deserialize save (version {version:?}): {detail}")]
    Deserialize {
        version: Option<u32>,
        detail: String,
    },
    #[error("version mismatch: found {found}, expected {expected}")]
    VersionMismatch { found: u32, expected: u32 },
    #[error("migration not implemented for version {found} -> {expected}: {reason}")]
    MigrationUnsupported {
        found: u32,
        expected: u32,
        reason: String,
    },
}

pub struct SaveManager {
    dir: PathBuf,
    /// Last simulation day an autosave was written (deduplicates intra-day
    /// autosave calls).
    last_autosave_day: AtomicU64,
}

impl SaveManager {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            last_autosave_day: AtomicU64::new(0),
        }
    }

    /// Atomic save: write to tmp then rename.
    pub fn save(&self, world: &alc_world::World) -> Result<(), SaveError> {
        self.save_with_engine(world, None)
    }

    /// Save world with EngineState for complete persistence.
    ///
    /// Durability contract: serialize to memory, write `world.tmp`, `sync_all`
    /// the temp file (data + metadata) and only then rename it over
    /// `world.bin`. A crash before the rename therefore leaves the previous
    /// complete save intact.
    pub fn save_with_engine(
        &self,
        world: &alc_world::World,
        engine_state: Option<EngineState>,
    ) -> Result<(), SaveError> {
        fs::create_dir_all(&self.dir)?;
        let header = SaveHeader {
            version: SAVE_FORMAT_VERSION,
            created_at_secs: world.time.as_secs(),
            population: world.prisoners.len() as u32,
        };
        let sf = SaveFile {
            header,
            world: world.clone(),
            engine_state,
        };
        let bytes = bincode::serialize(&sf).map_err(|e| SaveError::Serialize(e.to_string()))?;
        let tmp = self.dir.join("world.tmp");
        {
            use std::io::Write;
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        let final_path = self.dir.join("world.bin");
        fs::rename(&tmp, &final_path)?;
        // Debug sidecar.
        if let Ok(json) = serde_json::to_string(&sf) {
            let _ = fs::write(self.dir.join("world.debug.json"), json);
        }
        Ok(())
    }

    /// Load a save with EngineState, migrating older versions when possible.
    ///
    /// Unlike [`SaveManager::load`], this entry point runs the forward
    /// migration hook so a v1/v2 save can still be opened by a v3 build.
    pub fn load_with_engine(&self) -> Result<(alc_world::World, Option<EngineState>), SaveError> {
        let bytes = fs::read(self.dir.join("world.bin"))?;
        let sf = migrate_bytes(&bytes)?;
        Ok((sf.world, sf.engine_state))
    }

    /// Load a save, rejecting versions other than the current one. Use
    /// [`SaveManager::load_migrated`] to run the migration hook first.
    pub fn load(&self) -> Result<alc_world::World, SaveError> {
        let bytes = fs::read(self.dir.join("world.bin"))?;
        let sf = deserialize_save(&bytes)?;
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
        Ok(migrate_bytes(&bytes)?.world)
    }

    /// Unconditional autosave (legacy behaviour).
    ///
    /// Prefer [`Self::save_with_engine`] so engine state is preserved.
    #[deprecated(note = "Use save_with_engine to avoid engine-less saves")]
    pub fn autosave(&self, world: &alc_world::World) -> Result<(), SaveError> {
        self.save(world)
    }

    /// Autosave only when `save_interval_days` has elapsed. Returns whether a
    /// save was written. The interval is measured in whole simulation days;
    /// repeated calls on the same due day do not rewrite the save.
    ///
    /// Prefer [`Self::autosave_with_engine_if_due`] so engine state is preserved.
    #[deprecated(note = "Use autosave_with_engine_if_due to avoid engine-less saves")]
    pub fn autosave_if_due(
        &self,
        world: &alc_world::World,
        save_interval_days: u64,
    ) -> Result<bool, SaveError> {
        self.autosave_with_engine_if_due(world, None, save_interval_days)
    }

    /// Autosave with engine state when the configured interval has elapsed.
    pub fn autosave_with_engine_if_due(
        &self,
        world: &alc_world::World,
        engine_state: Option<EngineState>,
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
        self.save_with_engine(world, engine_state)?;
        self.last_autosave_day.store(day, Ordering::Relaxed);
        Ok(true)
    }

    /// Autosave using the top-level [`Config`] interval.
    ///
    /// Prefer [`Self::autosave_with_engine_if_due`] so engine state is preserved.
    #[deprecated(note = "Use autosave_with_engine_if_due to avoid engine-less saves")]
    pub fn autosave_with_config(
        &self,
        world: &alc_world::World,
        config: &Config,
    ) -> Result<bool, SaveError> {
        self.autosave_with_engine_if_due(world, None, config.save_interval_days)
    }
}

/// Peek the save-format version from the head of a bincode payload.
///
/// `SaveFile.header.version` is the first field of the first struct, encoded
/// by bincode 1.x as a fixed-width little-endian `u32` (4 bytes). Reading it
/// separately lets us report a version-aware error even when the rest of the
/// payload uses an incompatible layout.
fn peek_save_version(bytes: &[u8]) -> Option<u32> {
    let head = bytes.get(..4)?;
    Some(u32::from_le_bytes([head[0], head[1], head[2], head[3]]))
}

/// Deserialize a save, attaching version context to any failure.
fn deserialize_save(bytes: &[u8]) -> Result<SaveFile, SaveError> {
    let version = peek_save_version(bytes);
    bincode::deserialize(bytes).map_err(|e| {
        let hint = match version {
            Some(v) if v < SAVE_FORMAT_VERSION => format!(
                "save format v{v} is older than the supported v{SAVE_FORMAT_VERSION}; \
                 load via `load_migrated`/`load_with_engine` (or resave on a current build)"
            ),
            Some(v) if v > SAVE_FORMAT_VERSION => format!(
                "save format v{v} was written by a newer build (this build supports v{SAVE_FORMAT_VERSION})"
            ),
            Some(v) => format!("save format v{v} is corrupt"),
            None => "payload is too short to contain the 4-byte version header".to_string(),
        };
        SaveError::Deserialize { version, detail: format!("{hint}; bincode: {e}") }
    })
}

/// v1 layout: the `engine_state` field did not exist yet. Only used as a
/// fallback when the current layout cannot decode a v1 binary save.
#[derive(Debug, Serialize, Deserialize)]
struct SaveFileV1 {
    header: SaveHeader,
    world: alc_world::World,
}

/// Parole ledger layout through v4: before decision conditions, reasons and
/// release times were persisted. Used by the v3/v4 migration paths.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct ParoleLedgerLegacy {
    pub status: BTreeMap<u64, ParoleStatus>,
    pub adjustments: BTreeMap<u64, f32>,
    pub hearing_at: BTreeMap<u64, SimTime>,
    pub last_decision_at: Option<SimTime>,
}

impl From<ParoleLedgerLegacy> for ParoleLedger {
    fn from(legacy: ParoleLedgerLegacy) -> Self {
        Self {
            status: legacy.status,
            adjustments: legacy.adjustments,
            hearing_at: legacy.hearing_at,
            last_decision_at: legacy.last_decision_at,
            conditions: BTreeMap::new(),
            decision_reason: BTreeMap::new(),
            released_at: BTreeMap::new(),
        }
    }
}

/// v3 engine layout: predates `resolved_contraband_at`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct EngineStateV3 {
    pub parole: ParoleLedgerLegacy,
    pub population_history: Vec<(u64, u32)>,
    pub incidents: Vec<WorldEvent>,
    pub incident_sidecar: Vec<Incident>,
    pub decision_log: Vec<Decision>,
    pub decision_audit: Vec<DecisionAuditRecord>,
    pub resolved_contraband_cases: std::collections::BTreeSet<u64>,
    pub post_release: BTreeMap<u64, bool>,
    pub policy_state: PolicyState,
    pub policy_mode: PolicyMode,
    pub autosave_interval_days: Option<u64>,
    pub last_autosave_day: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SaveFileV3 {
    header: SaveHeader,
    world: alc_world::World,
    #[serde(default)]
    engine_state: Option<EngineStateV3>,
}

impl From<EngineStateV3> for EngineState {
    fn from(v3: EngineStateV3) -> Self {
        Self {
            parole: v3.parole.into(),
            population_history: v3.population_history,
            incidents: v3.incidents,
            incident_sidecar: v3.incident_sidecar,
            decision_log: v3.decision_log,
            decision_audit: v3.decision_audit,
            resolved_contraband_cases: v3.resolved_contraband_cases,
            post_release: v3.post_release,
            policy_state: v3.policy_state,
            policy_mode: v3.policy_mode,
            autosave_interval_days: v3.autosave_interval_days,
            last_autosave_day: v3.last_autosave_day,
            resolved_contraband_at: BTreeMap::new(),
        }
    }
}

/// v4 engine layout: identical to the current engine state except the parole
/// ledger, which predates conditions/reasons/release times.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct EngineStateV4 {
    pub parole: ParoleLedgerLegacy,
    pub population_history: Vec<(u64, u32)>,
    pub incidents: Vec<WorldEvent>,
    pub incident_sidecar: Vec<Incident>,
    pub decision_log: Vec<Decision>,
    pub decision_audit: Vec<DecisionAuditRecord>,
    pub resolved_contraband_cases: std::collections::BTreeSet<u64>,
    pub post_release: BTreeMap<u64, bool>,
    pub policy_state: PolicyState,
    pub policy_mode: PolicyMode,
    pub autosave_interval_days: Option<u64>,
    pub last_autosave_day: Option<u64>,
    pub resolved_contraband_at: BTreeMap<u64, SimTime>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SaveFileV4 {
    header: SaveHeader,
    world: alc_world::World,
    #[serde(default)]
    engine_state: Option<EngineStateV4>,
}

impl From<EngineStateV4> for EngineState {
    fn from(v4: EngineStateV4) -> Self {
        Self {
            parole: v4.parole.into(),
            population_history: v4.population_history,
            incidents: v4.incidents,
            incident_sidecar: v4.incident_sidecar,
            decision_log: v4.decision_log,
            decision_audit: v4.decision_audit,
            resolved_contraband_cases: v4.resolved_contraband_cases,
            post_release: v4.post_release,
            policy_state: v4.policy_state,
            policy_mode: v4.policy_mode,
            autosave_interval_days: v4.autosave_interval_days,
            last_autosave_day: v4.last_autosave_day,
            resolved_contraband_at: v4.resolved_contraband_at,
        }
    }
}

/// Migrate a save to the current format version.
///
/// - v1 -> v5: v1 predates `engine_state`; an empty engine state is installed.
/// - v2/v3/v4 -> v5: fields added since those versions are serde-defaulted in
///   self-describing formats; for bincode, explicit legacy layouts are used
///   where trailing-field additions break direct decoding.
/// - v0 or unknown older versions have no defined layout and are rejected
///   with a clear error instead of being silently misinterpreted.
/// - Newer versions are rejected (never downgraded) with a version mismatch.
pub fn migrate(sf: SaveFile) -> Result<SaveFile, SaveError> {
    let found = sf.header.version;
    if found == SAVE_FORMAT_VERSION {
        return Ok(sf);
    }
    if found == 0 || found > SAVE_FORMAT_VERSION {
        if found > SAVE_FORMAT_VERSION {
            return Err(SaveError::VersionMismatch {
                found,
                expected: SAVE_FORMAT_VERSION,
            });
        }
        return Err(SaveError::MigrationUnsupported {
            found,
            expected: SAVE_FORMAT_VERSION,
            reason: "version 0 is not a valid save format version; no legacy layout to read"
                .to_string(),
        });
    }

    let mut migrated = sf;
    migrated.header.version = SAVE_FORMAT_VERSION;
    if migrated.engine_state.is_none() {
        migrated.engine_state = Some(EngineState::default());
    }
    Ok(migrated)
}

/// Deserialize then migrate a raw save payload.
///
/// Always peeks the version first so that an undecodable save yields an
/// actionable error. Legacy binary layouts are attempted for known older
/// versions when the current layout cannot decode them.
pub fn migrate_bytes(bytes: &[u8]) -> Result<SaveFile, SaveError> {
    let version = peek_save_version(bytes).ok_or_else(|| SaveError::Deserialize {
        version: None,
        detail: format!(
            "save payload is too short: only {} byte(s); expected at least 4 for the v{SAVE_FORMAT_VERSION} version header",
            bytes.len()
        ),
    })?;

    let decoded = match deserialize_save(bytes) {
        Ok(sf) => sf,
        Err(err) => match version {
            1 => match bincode::deserialize::<SaveFileV1>(bytes) {
                Ok(legacy) => SaveFile {
                    header: legacy.header,
                    world: legacy.world,
                    engine_state: None,
                },
                Err(_) => return Err(err),
            },
            3 => match bincode::deserialize::<SaveFileV3>(bytes) {
                Ok(legacy) => SaveFile {
                    header: legacy.header,
                    world: legacy.world,
                    engine_state: legacy.engine_state.map(EngineState::from),
                },
                Err(_) => return Err(err),
            },
            4 => match bincode::deserialize::<SaveFileV4>(bytes) {
                Ok(legacy) => SaveFile {
                    header: legacy.header,
                    world: legacy.world,
                    engine_state: legacy.engine_state.map(EngineState::from),
                },
                Err(_) => return Err(err),
            },
            _ => return Err(err),
        },
    };
    migrate(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::TimeSpeed;

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
        world.speed = TimeSpeed::FastForward;
        world.orchestrator.charge(123);

        manager.save(&world).unwrap();
        let loaded = manager.load().unwrap();

        assert_eq!(loaded.seed, world.seed);
        assert_eq!(loaded.time, world.time);
        assert_eq!(
            loaded.speed, world.speed,
            "World.speed must survive the save"
        );
        assert_eq!(loaded.prisoners.len(), world.prisoners.len());
        assert_eq!(loaded.guards.len(), world.guards.len());
        assert_eq!(loaded.facilities.len(), world.facilities.len());
        assert_eq!(loaded.economy.cash, world.economy.cash);
        assert_eq!(
            loaded.orchestrator.consumed_today,
            world.orchestrator.consumed_today
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_replaces_stale_temp_file_via_atomic_rename() {
        // The write path is: create/truncate world.tmp, write bytes, fsync it,
        // then rename over world.bin. A stale temp file must never leak into a
        // loadable save.
        let dir = unique_dir("atomic-write");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("world.tmp"),
            b"stale bytes from an interrupted save",
        )
        .unwrap();

        let manager = SaveManager::new(&dir);
        let world = alc_world::World::new(0xA70_0D);
        manager.save(&world).unwrap();

        assert!(
            !dir.join("world.tmp").exists(),
            "the temp file must be consumed by the atomic rename"
        );
        assert_eq!(manager.load().unwrap().seed, world.seed);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn autosave_respects_interval() {
        let dir = unique_dir("interval");
        let manager = SaveManager::new(&dir);
        let mut world = alc_world::World::new(1);
        world.days_simulated = 3;

        assert!(
            !manager
                .autosave_with_engine_if_due(&world, None, 7)
                .unwrap()
        );
        assert!(!Path::new(&dir).join("world.bin").exists());

        world.days_simulated = 7;
        assert!(
            manager
                .autosave_with_engine_if_due(&world, None, 7)
                .unwrap()
        );
        assert!(Path::new(&dir).join("world.bin").exists());
        // Repeated calls on the same due day must not rewrite the save.
        assert!(
            !manager
                .autosave_with_engine_if_due(&world, None, 7)
                .unwrap()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_accepts_current_version() {
        let sf = SaveFile {
            header: SaveHeader {
                version: SAVE_FORMAT_VERSION,
                created_at_secs: 1,
                population: 0,
            },
            world: alc_world::World::new(7),
            engine_state: None,
        };
        assert!(migrate(sf).is_ok());
    }

    #[test]
    fn migrate_rejects_unknown_versions() {
        let sf = SaveFile {
            header: SaveHeader {
                version: SAVE_FORMAT_VERSION + 5,
                created_at_secs: 1,
                population: 0,
            },
            world: alc_world::World::new(7),
            engine_state: None,
        };
        assert!(matches!(
            migrate(sf),
            Err(SaveError::VersionMismatch { .. })
        ));
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
            causal_chain: Vec::new(),
            witnesses: Vec::new(),
            disposition: PersistedIncidentDisposition::default(),
            response_actions: Vec::new(),
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

    // --- 0.05 migration / default tests ---------------------------------

    #[test]
    fn engine_state_partial_json_uses_defaults() {
        // Self-describing formats (the debug sidecar, tools, tests) can omit
        // any 0.05 field and still load without an explicit migration step.
        let partial = r#"{
            "post_release": {"7": true}
        }"#;
        let state: EngineState = serde_json::from_str(partial).unwrap();
        assert_eq!(state.post_release.get(&7), Some(&true));
        assert!(state.decision_audit.is_empty());
        assert!(state.incident_sidecar.is_empty());
        assert!(state.autosave_interval_days.is_none());
        assert_eq!(state.policy_mode, PolicyMode::Deterministic);
        assert_eq!(state.policy_state.policy, Policy::Normal);
        assert!(!state.policy_state.lockdown);
    }

    #[test]
    fn incident_partial_json_uses_defaults() {
        let partial = r#"{"kind":"Fight","severity":0.75}"#;
        let inc: Incident = serde_json::from_str(partial).unwrap();
        assert_eq!(inc.kind, alc_events::EventKind::Fight);
        assert_eq!(inc.severity, 0.75);
        assert_eq!(inc.id, 0);
        assert_eq!(inc.location, "");
        assert!(!inc.resolved);
        assert!(inc.cause.is_none());
        assert!(inc.consequence.is_none());
        assert!(inc.involved_prisoners.is_empty());
        assert!(inc.involved_guards.is_empty());
    }

    #[test]
    fn engine_state_full_roundtrip_preserves_0_05_fields() {
        let dir = unique_dir("engine-roundtrip");
        let manager = SaveManager::new(&dir);
        let world = alc_world::World::new(0xC0FF_EE);

        let mut state = EngineState::default();
        state.decision_audit.push(DecisionAuditRecord {
            time: SimTime::from_secs(42),
            actor: "warden".into(),
            source: "deterministic".into(),
            action: "Expand".into(),
            summary: "queued a cell block".into(),
            validation: "accepted".into(),
            reason: "funded".into(),
            rationale: String::new(),
            repair_reason: String::new(),
            outcome: alc_agents::decision::DecisionOutcomeState::Accepted,
        });
        state.incident_sidecar.push(Incident {
            id: 9,
            kind: alc_events::EventKind::Fight,
            severity: 0.8,
            cause: Some(9),
            consequence: Some(9),
            timestamp: SimTime::from_secs(43),
            location: "Yard".into(),
            involved_prisoners: vec![1, 2],
            involved_guards: vec![],
            causal_chain: Vec::new(),
            witnesses: Vec::new(),
            disposition: PersistedIncidentDisposition::default(),
            response_actions: Vec::new(),
            resolved: false,
        });
        state.post_release.insert(3, true);
        state.autosave_interval_days = Some(7);
        state.policy_state.lockdown = true;
        state.policy_state.security_level = 5;
        state.policy_mode = PolicyMode::Council;

        manager
            .save_with_engine(&world, Some(state.clone()))
            .expect("save");
        let (loaded_world, loaded_state) = manager.load_with_engine().expect("load");

        assert_eq!(loaded_world.seed, world.seed);
        let loaded = loaded_state.expect("engine state");
        assert_eq!(loaded.decision_audit.len(), 1);
        assert_eq!(loaded.decision_audit[0].action, "Expand");
        assert_eq!(loaded.decision_audit[0].time, SimTime::from_secs(42));
        assert_eq!(loaded.incident_sidecar.len(), 1);
        assert_eq!(loaded.incident_sidecar[0].severity, 0.8);
        assert_eq!(loaded.post_release.get(&3), Some(&true));
        assert_eq!(loaded.autosave_interval_days, Some(7));
        assert!(loaded.policy_state.lockdown);
        assert_eq!(loaded.policy_state.security_level, 5);
        assert_eq!(loaded.policy_mode, PolicyMode::Council);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_v1_and_v2_stamp_current_version_and_fill_engine_state() {
        for version in [1_u32, 2_u32] {
            let sf = SaveFile {
                header: SaveHeader {
                    version,
                    created_at_secs: 1,
                    population: 0,
                },
                world: alc_world::World::new(7),
                engine_state: None,
            };
            let migrated = migrate(sf).expect("older version migrates");
            assert_eq!(migrated.header.version, SAVE_FORMAT_VERSION);
            let state = migrated.engine_state.expect("engine state installed");
            assert!(state.decision_audit.is_empty());
            assert!(state.post_release.is_empty());
            assert_eq!(state.policy_mode, PolicyMode::Deterministic);
        }
    }

    #[test]
    fn migrate_v3_binary_fixture_to_v4() {
        let dir = unique_dir("v3-fixture");
        fs::create_dir_all(&dir).unwrap();
        let world = alc_world::World::new(0xA1C_05);
        let mut v3_engine = EngineStateV3::default();
        v3_engine.autosave_interval_days = Some(3);
        v3_engine.last_autosave_day = Some(9);
        v3_engine.resolved_contraband_cases.insert(77);
        v3_engine.post_release.insert(5, true);

        let sf = SaveFileV3 {
            header: SaveHeader {
                version: 3,
                created_at_secs: 12,
                population: 1,
            },
            world: world.clone(),
            engine_state: Some(v3_engine),
        };
        let bytes = bincode::serialize(&sf).expect("serialize v3 fixture");
        fs::write(dir.join("world.bin"), &bytes).unwrap();

        let manager = SaveManager::new(&dir);
        let (loaded_world, loaded_engine) = manager.load_with_engine().expect("load migrated v3");
        assert_eq!(loaded_world.seed, world.seed);
        let engine = loaded_engine.expect("engine state");
        assert_eq!(engine.autosave_interval_days, Some(3));
        assert_eq!(engine.last_autosave_day, Some(9));
        assert!(engine.resolved_contraband_cases.contains(&77));
        assert_eq!(engine.post_release.get(&5), Some(&true));
        assert!(engine.resolved_contraband_at.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_rejects_version_zero_with_clear_reason() {
        let sf = SaveFile {
            header: SaveHeader {
                version: 0,
                created_at_secs: 1,
                population: 0,
            },
            world: alc_world::World::new(7),
            engine_state: None,
        };
        match migrate(sf) {
            Err(SaveError::MigrationUnsupported {
                found,
                expected,
                reason,
            }) => {
                assert_eq!(found, 0);
                assert_eq!(expected, SAVE_FORMAT_VERSION);
                assert!(reason.contains("not a valid save format version"));
            }
            other => panic!("expected MigrationUnsupported, got {other:?}"),
        }
    }

    #[test]
    fn load_and_migrate_older_binary_version() {
        let dir = unique_dir("migrate-bytes");
        let manager = SaveManager::new(&dir);
        let world = alc_world::World::new(0xBEEF);

        fs::create_dir_all(&dir).unwrap();
        // Serialize the current layout but tag it as v2. All v3 additions are
        // serde-defaulted, so this is a faithful stand-in for a v2 save whose
        // sub-fields did not yet exist.
        let legacy_tagged = SaveFile {
            header: SaveHeader {
                version: 2,
                created_at_secs: 1,
                population: 0,
            },
            world: world.clone(),
            engine_state: None,
        };
        let bytes = bincode::serialize(&legacy_tagged).unwrap();
        fs::write(dir.join("world.bin"), &bytes).unwrap();

        let migrated = manager.load_migrated().expect("v2 loads via migration");
        assert_eq!(migrated.seed, world.seed);
        assert_eq!(migrated.time, world.time);

        // load_with_engine uses the same migration hook.
        let (w, engine) = manager.load_with_engine().expect("v2 loads with engine");
        assert_eq!(w.seed, world.seed);
        assert!(engine.is_some());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_v1_payload_without_engine_state_loads() {
        let dir = unique_dir("v1-payload");
        let manager = SaveManager::new(&dir);
        let world = alc_world::World::new(0xA1CE);
        fs::create_dir_all(&dir).unwrap();

        let v1 = SaveFileV1 {
            header: SaveHeader {
                version: 1,
                created_at_secs: 1,
                population: 0,
            },
            world: world.clone(),
        };
        let bytes = bincode::serialize(&v1).unwrap();
        fs::write(dir.join("world.bin"), &bytes).unwrap();

        let migrated = manager.load_migrated().expect("v1 loads via migration");
        assert_eq!(migrated.seed, world.seed);
        let (_, engine) = manager.load_with_engine().expect("v1 loads with engine");
        assert!(engine.is_some());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn future_version_reports_clear_mismatch() {
        let dir = unique_dir("future-version");
        let manager = SaveManager::new(&dir);
        fs::create_dir_all(&dir).unwrap();
        let sf = SaveFile {
            header: SaveHeader {
                version: SAVE_FORMAT_VERSION + 3,
                created_at_secs: 1,
                population: 0,
            },
            world: alc_world::World::new(7),
            engine_state: None,
        };
        fs::write(dir.join("world.bin"), bincode::serialize(&sf).unwrap()).unwrap();

        let err = manager.load().expect_err("future save must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("version mismatch"), "unexpected error: {msg}");
        assert!(msg.contains(&(SAVE_FORMAT_VERSION + 3).to_string()));
        assert!(msg.contains(&SAVE_FORMAT_VERSION.to_string()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_payload_reports_version_context() {
        let bytes = [0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02];
        assert_eq!(peek_save_version(&bytes), Some(0xEF_BE_AD_DE));

        let err = migrate_bytes(&bytes).expect_err("garbage must fail");
        match &err {
            SaveError::Deserialize { version, detail } => {
                assert_eq!(*version, Some(0xEF_BE_AD_DE));
                assert!(
                    detail.contains("newer build"),
                    "unexpected detail: {detail}"
                );
            }
            other => panic!("expected Deserialize error, got {other:?}"),
        }

        let err = migrate_bytes(&[1, 2]).expect_err("short payload must fail");
        assert!(err.to_string().contains("too short"));
        assert!(matches!(err, SaveError::Deserialize { version: None, .. }));
    }

    #[test]
    fn world_lifecycle_state_round_trips_through_save_manager() {
        let dir = unique_dir("lifecycle-roundtrip");
        let manager = SaveManager::new(&dir);
        let mut world = alc_world::World::new(0xA1C_05);

        // Construction lifecycle
        let pid = world.enqueue_construction(alc_world::FacilityKind::Education, 1, 20);
        assert!(pid > 0);
        if let Some(p) = world.construction_queue.first_mut() {
            p.status = alc_world::ConstructionStatus::InProgress;
            p.progress_days = 2;
            p.staff_allocated = true;
            p.materials_ready = true;
            p.prereqs_met = true;
        }

        // Post-release / rehab / contraband records
        if let Some(prisoner) = world.prisoners.first().cloned() {
            let pid_u64 = prisoner.id;
            world
                .post_release_records
                .push(alc_world::PostReleaseRecord {
                    id: world.next_post_release_id,
                    prisoner_id: pid_u64.raw(),
                    name: prisoner.name.clone(),
                    reason: "transfer".to_string(),
                    day: 3,
                    sentence_info: alc_world::PostReleaseSentenceInfo {
                        sentence_days: prisoner.sentence_days,
                        sentence_served_days: prisoner.sentence_served_days,
                        classification: prisoner.classification,
                        escape_motivation_at_exit: 0.1,
                    },
                    outcome_seed: 42,
                    employment: true,
                    housing: false,
                    violation: false,
                    return_to_custody: false,
                });
            world.next_post_release_id += 1;
            world
                .rehab_records
                .entry(pid_u64)
                .or_default()
                .push(alc_world::RehabProgramRecord {
                    program: alc_social::RehabProgram::Education,
                    enrolled_day: 1,
                    progress_days: 2,
                    progress_sum: 0.2,
                    completed: false,
                    dropped: false,
                    attendance_days: 2,
                    last_attended_day: Some(2),
                    failure_reason: None,
                });
            world
                .contraband_state
                .entry(pid_u64)
                .or_default()
                .flags_confiscated = true;
            world.resolve_contraband_case(77, alc_core::SimTime::from_secs(3600));
        }

        manager.save(&world).unwrap();
        let loaded = manager.load().unwrap();

        assert_eq!(
            loaded.construction_queue.len(),
            world.construction_queue.len()
        );
        let lp = &loaded.construction_queue[0];
        assert_eq!(lp.id, pid);
        assert_eq!(lp.status, alc_world::ConstructionStatus::InProgress);
        assert_eq!(lp.progress_days, 2);
        assert!(lp.staff_allocated && lp.materials_ready && lp.prereqs_met);

        assert_eq!(
            loaded.post_release_records.len(),
            world.post_release_records.len()
        );
        assert_eq!(loaded.post_release_records[0].reason, "transfer");
        assert_eq!(loaded.next_post_release_id, world.next_post_release_id);

        let loaded_rehab: usize = loaded.rehab_records.values().map(|v| v.len()).sum();
        let world_rehab: usize = world.rehab_records.values().map(|v| v.len()).sum();
        assert_eq!(loaded_rehab, world_rehab);
        assert!(
            loaded
                .contraband_state
                .values()
                .any(|s| s.flags_confiscated)
        );
        assert_eq!(
            loaded.resolved_contraband_at.get(&77).copied(),
            Some(alc_core::SimTime::from_secs(3600))
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
