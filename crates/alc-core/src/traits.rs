//! Personality + cognitive traits (Directive §3).
//!
//! Moved to `alc_core` from `alc_prisoner` so the capability system
//! ([`crate::Capability`]) can reference it from `alc_core` itself —
//! `alc_prisoner` re-exports it for compatibility.

use crate::RngStream;
use serde::{Deserialize, Serialize};

fn half() -> f32 { 0.5 }

/// All personality + cognitive traits (Directive §3). Each 0..1 except
/// `intelligence`, which reads as a raw percentile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Traits {
    pub intelligence: f32,
    pub emotional_stability: f32,
    pub impulsiveness: f32,
    pub aggression: f32,
    pub sociability: f32,
    pub loyalty: f32,
    pub manipulation: f32,
    pub risk_tolerance: f32,
    pub discipline: f32,
    pub empathy: f32,
    pub ambition: f32,
    pub leadership: f32,
    /// General risk propensity (`risk_tolerance` is the disposition-flavored
    /// one used by escape planning; `risk` is a carrier signal).
    #[serde(default = "half")]
    pub risk: f32,
}

impl Default for Traits {
    fn default() -> Self {
        Self {
            intelligence: 0.5,
            emotional_stability: 0.5,
            impulsiveness: 0.5,
            aggression: 0.5,
            sociability: 0.5,
            loyalty: 0.5,
            manipulation: 0.5,
            risk_tolerance: 0.5,
            discipline: 0.5,
            empathy: 0.5,
            ambition: 0.5,
            leadership: 0.5,
            risk: 0.5,
        }
    }
}

impl Traits {
    pub fn randomized(rng: &mut RngStream) -> Self {
        let r = |rng: &mut RngStream| rng.range(0.1, 0.95) as f32;
        Self {
            intelligence: r(rng),
            emotional_stability: r(rng),
            impulsiveness: r(rng),
            aggression: r(rng),
            sociability: r(rng),
            loyalty: r(rng),
            manipulation: r(rng),
            risk_tolerance: r(rng),
            discipline: r(rng),
            empathy: r(rng),
            ambition: r(rng),
            leadership: r(rng),
            risk: r(rng),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_traits_are_midpoint() {
        let t = Traits::default();
        assert_eq!(t.risk, 0.5);
        assert_eq!(t.aggression, 0.5);
    }

    #[test]
    fn randomized_traits_stay_in_range() {
        let mut rng = RngStream::new(1, 7);
        let t = Traits::randomized(&mut rng);
        assert!((0.1..=0.95).contains(&t.risk));
        assert!((0.1..=0.95).contains(&t.aggression));
    }
}
