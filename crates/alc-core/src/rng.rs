use rand::{RngCore, SeedableRng};
use rand_pcg::Pcg64;
use serde::{Deserialize, Serialize};

/// Per-agent deterministic RNG stream.
///
/// Each agent owns one stream; the stream is advanced only by that agent's
/// own rolls, so replay of a single agent is reproducible regardless of
/// scheduling order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RngStream {
    stream: u64,
    state: u128,
}

impl RngStream {
    /// Deterministically derive a stream from a root seed and an agent id.
    pub fn new(root_seed: u64, id: u64) -> Self {
        let mut hasher = root_seed ^ id.rotate_left(32);
        hasher = hasher.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        Self { stream: id, state: hasher as u128 }
    }

    pub fn fork(&self, label: u64) -> Self {
        let mut child = self.state ^ (label as u128);
        child = child.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        Self { stream: label, state: child }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut rng = Pcg64::seed_from_u64(self.state as u64 ^ (self.stream << 1));
        let v = rng.next_u64();
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        v
    }

    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next_f64() * (hi - lo)
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.next_f64() < p.clamp(0.0, 1.0)
    }
}