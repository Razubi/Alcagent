//! Institutional policy state shared by the world, guards and warden.
//!
//! Policies live in `alc-core` so every layer (world simulation, guard duty
//! loop, warden decisions) can read the same authoritative posture without a
//! dependency cycle. `alc-world` and `alc-guard` re-export this type.

use serde::{Deserialize, Serialize};

/// Institutional policies (subset, extensible).
#[derive(Debug, Clone, Serialize)]
pub struct Policies {
    pub lockdown: bool,
    pub privileges_level: f32,
    pub rehab_funding_multiplier: f32,
    pub security_level: f32,
}

impl<'de> Deserialize<'de> for Policies {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PoliciesDe {
            lockdown: bool,
            privileges_level: f32,
            rehab_funding_multiplier: f32,
            security_level: f32,
        }

        let raw = PoliciesDe::deserialize(deserializer)?;
        let mut policies = Policies {
            lockdown: raw.lockdown,
            privileges_level: raw.privileges_level,
            rehab_funding_multiplier: raw.rehab_funding_multiplier,
            security_level: raw.security_level,
        };
        policies.normalize();
        Ok(policies)
    }
}

impl Default for Policies {
    fn default() -> Self {
        Self {
            lockdown: false,
            privileges_level: 0.5,
            rehab_funding_multiplier: 1.0,
            security_level: 0.6,
        }
    }
}

impl Policies {
    /// Clamp all continuous knobs back into their valid ranges.
    pub fn normalize(&mut self) {
        self.privileges_level = self.privileges_level.clamp(0.0, 1.0);
        self.rehab_funding_multiplier = self.rehab_funding_multiplier.clamp(0.0, 3.0);
        self.security_level = self.security_level.clamp(0.0, 1.0);
    }
}
