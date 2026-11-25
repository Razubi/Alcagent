//! Parole ledger and related types (Directive §15).
//!
//! The ledger is the engine's auditable record of eligibility hearings,
//! decisions, release conditions and actual release. All deriving and
//! validation is pure and deterministic so identical worlds produce identical
//! records and identical save bytes.

use serde::{Deserialize, Serialize};
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

/// Conditions attached to a parole grant (§15 step 9).
///
/// Kept as plain scalars so the record is cheap, deterministic and trivially
/// persistable. [`ParoleConditions::normalize`] is the single validation gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParoleConditions {
    /// Required check-ins per month (0..=12; 0 means no reporting).
    pub reporting_per_month: u8,
    /// Supervision length in months (1..=60).
    pub supervision_months: u16,
    /// Curfew requirement.
    pub curfew: bool,
    /// Drug/alcohol testing requirement.
    pub drug_testing: bool,
    /// Mandatory treatment requirement.
    pub treatment_required: bool,
    /// No-contact order requirement.
    pub no_contact_order: bool,
}

impl Default for ParoleConditions {
    fn default() -> Self {
        Self {
            reporting_per_month: 1,
            supervision_months: 12,
            curfew: false,
            drug_testing: false,
            treatment_required: false,
            no_contact_order: false,
        }
    }
}

impl ParoleConditions {
    /// Clamp every field into its documented range.
    pub fn normalize(&mut self) {
        self.reporting_per_month = self.reporting_per_month.min(12);
        self.supervision_months = self.supervision_months.clamp(1, 60);
    }

    /// Derive conditions deterministically from a prisoner id and the two
    /// decision inputs the engine already computes (risk and rehabilitation).
    ///
    /// No clock, RNG or AI call is involved, so the same inputs always produce
    /// the same conditions — including after a save/load roundtrip.
    pub fn deterministic(id: PrisonerId, risk: f32, rehab: f32) -> Self {
        let r = risk.clamp(0.0, 1.0);
        let h = rehab.clamp(0.0, 1.0);
        let seed = id
            .raw()
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add((r * 1000.0) as u64)
            .rotate_left(13)
            .wrapping_add((h * 1000.0) as u64);
        let reporting = (1 + (seed % 6) as u8 + (r * 4.0) as u8).min(12);
        let supervision = 6 + (seed >> 8 % 24) as u16 + if r > 0.6 { 6 } else { 0 };
        Self {
            reporting_per_month: reporting,
            supervision_months: supervision.min(60).max(1),
            curfew: r > 0.4 || (seed & 0x1) != 0,
            drug_testing: r > 0.25 || (seed & 0x2) != 0,
            treatment_required: h < 0.5 || (seed & 0x4) != 0,
            no_contact_order: r > 0.6,
        }
    }

    /// True when this record differs from `other` after normalization.
    pub fn is_normalized(&self) -> bool {
        let mut copy = self.clone();
        copy.normalize();
        &copy == self
    }
}

/// Parole ledger (Directive §15).
///
/// Tracks parole status, warden adjustments, hearing dates, decisions,
/// conditions and release times for prisoners.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParoleLedger {
    /// Prisoner parole status records.
    pub status: HashMap<PrisonerId, ParoleStatus>,
    /// Parole adjustment values (warden recommendations).
    pub adjustments: HashMap<PrisonerId, f32>,
    /// Hearing dates for prisoners.
    pub hearing_at: HashMap<PrisonerId, SimTime>,
    /// Last decision date for each prisoner.
    pub last_decision_at: Option<SimTime>,
    /// Conditions attached to each decision/grant.
    pub conditions: HashMap<PrisonerId, ParoleConditions>,
    /// Human-readable decision reason, persisted with the decision.
    pub decision_reason: HashMap<PrisonerId, String>,
    /// Sim time at which a granted prisoner actually left custody.
    pub released_at: HashMap<PrisonerId, SimTime>,
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

    /// Get the last decision date for the ledger, if any.
    pub fn last_decision_at(&self) -> Option<SimTime> {
        self.last_decision_at
    }

    /// Conditions recorded for a prisoner, if any.
    pub fn conditions(&self, id: PrisonerId) -> Option<&ParoleConditions> {
        self.conditions.get(&id)
    }

    /// Decision reason recorded for a prisoner, if any.
    pub fn decision_reason(&self, id: PrisonerId) -> Option<&str> {
        self.decision_reason.get(&id).map(String::as_str)
    }

    /// Release time for a granted prisoner, if they have left custody.
    pub fn released_at(&self, id: PrisonerId) -> Option<SimTime> {
        self.released_at.get(&id).copied()
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

    /// Record a scheduled hearing time.
    pub fn set_hearing_at(&mut self, id: PrisonerId, at: SimTime) {
        self.hearing_at.insert(id, at);
    }

    /// Record normalized decision conditions.
    pub fn set_conditions(&mut self, id: PrisonerId, mut conditions: ParoleConditions) {
        conditions.normalize();
        self.conditions.insert(id, conditions);
    }

    /// Record the reason for a decision.
    pub fn set_decision_reason(&mut self, id: PrisonerId, reason: impl Into<String>) {
        self.decision_reason.insert(id, reason.into());
    }

    /// Record the time at which a grant became an actual release.
    pub fn set_released_at(&mut self, id: PrisonerId, at: SimTime) {
        self.released_at.insert(id, at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_are_deterministic_and_normalized() {
        let a = ParoleConditions::deterministic(PrisonerId(42), 0.8, 0.3);
        let b = ParoleConditions::deterministic(PrisonerId(42), 0.8, 0.3);
        assert_eq!(a, b);
        assert!(a.is_normalized());
        assert!(a.reporting_per_month >= 1 && a.reporting_per_month <= 12);
        assert!(a.supervision_months >= 1 && a.supervision_months <= 60);
        assert!(a.curfew, "high-risk grants carry a curfew");

        let mut raw = ParoleConditions {
            reporting_per_month: 99,
            supervision_months: 0,
            ..Default::default()
        };
        assert!(!raw.is_normalized());
        raw.normalize();
        assert!(raw.is_normalized());
        assert_eq!(raw.reporting_per_month, 12);
        assert_eq!(raw.supervision_months, 1);
    }
}
