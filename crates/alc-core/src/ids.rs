use serde::{Deserialize, Serialize};

/// Identifier for a prisoner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrisonerId(pub u64);

/// Identifier for a guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GuardId(pub u64);

/// Identifier for the warden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WardenId(pub u64);

/// Identifier for a facility/building.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FacilityId(pub u64);

/// Identifier for a gang/organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GangId(pub u64);

/// Generic agent handle used by the orchestration layer when the
/// concrete agent kind is not known (e.g. during dispatch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub u64);

impl Default for PrisonerId {
    fn default() -> Self { Self(0) }
}

impl PrisonerId { pub fn raw(self) -> u64 { self.0 } }
impl GuardId { pub fn raw(self) -> u64 { self.0 } }
impl WardenId { pub fn raw(self) -> u64 { self.0 } }
impl FacilityId { pub fn raw(self) -> u64 { self.0 } }
impl GangId { pub fn raw(self) -> u64 { self.0 } }
impl AgentId { pub fn raw(self) -> u64 { self.0 } }