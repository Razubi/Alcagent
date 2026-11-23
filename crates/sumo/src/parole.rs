//! Parole ledger and related types (Directive §15).

use serde::{Serialize, Deserialize};
use std::collections::HashMap;

use alc_core::{PrisonerId, SimTime};

/// Parole status for a single prisoner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum ParoleStatus {
    /// No parole status set.
    None,
    /// Prisoner is scheduled for a parole hearing.
    Scheduled,
    /// Parole has been granted.
    Granted,
    /// Parole has been denied.
    Denied,
    /// Prisoner is currently in hearing.
    InHearing,
}

/// Parole ledger (Directive §15).
///
/// Tracks parole status, adjustments, hearing dates, and last decision dates
/// for prisoners.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParoleLedger {
    /// Prisoner parole status records.
    pub status: HashMap<PrisonerId, ParoleStatus>,
    /// Parole adjustment values.
    pub adjustments: HashMap<PrisonerId, f32>,
    /// Hearing dates for prisoners.
    pub hearing_at: HashMap<PrisonerId, SimTime>,
    /// Last decision date for each prisoner.
    pub last_decision_at: Option<SimTime>,
}

impl ParoleLedger {
    /// Get the current parole status for a prisoner, if any.
    pub fn status(&self, id: PrisonerId) -> Option<ParoleStatus> {
        self.status.get(&id).copied()
    }

    /// Whether the engine ledger has any record of this prisoner.
    pub fn has(&self, id: PrisonerId) -> bool {
        self.status.contains_key(&id)
    }

    /// Get the hearing date for a prisoner, if any.
    pub fn hearing_at(&self, id: PrisonerId) -> Option<SimTime> {
        self.hearing_at.get(&id).copied()
    }

    /// Get the last decision date for a prisoner, if any.
    pub fn last_decision_at(&self) -> Option<SimTime> {
        self.last_decision_at
    }

    /// Set the parole status for a prisoner.
    pub fn set(&mut self, id: PrisonerId, status: ParoleStatus, at: SimTime) {
        self.status.insert(id, status);
        self.last_decision_at = Some(at);
    }

    /// Set the adjustment value for a prisoner.
    pub fn set_adjustment(&mut self, id: PrisonerId, lean: f32) {
        self.adjustments.insert(id, lean);
    }
}