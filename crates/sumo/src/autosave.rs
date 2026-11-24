//! Scheduled autosaves (Directive §23).

use std::path::PathBuf;
use serde::{Serialize, Deserialize};

/// Engine autosave scheduler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutosaveScheduler {
    /// Directory to save into.
    pub dir: PathBuf,
    /// Autosave every this many simulated days (0 = disabled).
    pub interval_days: u64,
    last_save_day: u64,
}

impl AutosaveScheduler {
    /// Create a new autosave scheduler.
    ///
    /// The world will be saved to `dir` every `interval_days` simulated days.
    /// Set `interval_days` to `0` to disable autosave.
    pub fn new(dir: impl Into<PathBuf>, interval_days: u64) -> Self {
        Self { dir: dir.into(), interval_days, last_save_day: 0 }
    }
    /// Restore last save day from persistence.
    pub fn with_last_save_day(mut self, day: u64) -> Self {
        self.last_save_day = day;
        self
    }
    /// Last simulated day an autosave was written.
    pub fn last_save_day(&self) -> u64 { self.last_save_day }
    /// Check if an autosave is due.
    ///
    /// Returns `true` if the interval has elapsed since the last save.
    pub fn due(&self, days_simulated: u64) -> bool {
        self.interval_days > 0 && days_simulated.saturating_sub(self.last_save_day) >= self.interval_days
    }
    /// Mark the current save as having been written at `days_simulated`.
    pub fn mark_saved(&mut self, days_simulated: u64) {
        self.last_save_day = days_simulated;
    }
}