use serde::{Deserialize, Serialize};

/// Golden-ratio step used by SplitMix64. Keeping a persistent state and
/// emitting a full 64-bit output per advance preserves deterministic replay
/// and gives clean statistical properties without reseeding a generator on
/// every call.
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// Finalizer mixing for SplitMix64 output. This is the standard, well-tested
/// bit-mixing function; it takes a 64-bit value and returns a 64-bit result.
fn splitmix64(z: u64) -> u64 {
    let z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Per-agent deterministic RNG stream.
///
/// Each agent owns one stream; the stream is advanced only by that agent's
/// own rolls, so replay of a single agent is reproducible regardless of
/// scheduling order. The internal state is persistent across `next_u64`
/// calls and is serializable for save/restore fidelity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RngStream {
    stream: u64,
    state: u128,
}

impl RngStream {
    /// Deterministically derive a stream from a root seed and an agent id.
    pub fn new(root_seed: u64, id: u64) -> Self {
        let seed = root_seed ^ id.rotate_left(32);
        Self {
            stream: id,
            state: splitmix64(seed) as u128,
        }
    }

    /// Derive an independent child stream from `label`. The child does not
    /// share mutable state with the parent, so forking is safe regardless of
    /// how far the parent has advanced.
    pub fn fork(&self, label: u64) -> Self {
        let seed = (self.state as u64) ^ label ^ GAMMA;
        Self {
            stream: label,
            state: splitmix64(seed) as u128,
        }
    }

    /// Advance the persistent state and emit one 64-bit output. No generator
    /// is reseeded; the sequence is a pure function of the current `state`.
    pub fn next_u64(&mut self) -> u64 {
        let prev = self.state as u64;
        let next_state = (prev as u128).wrapping_add(GAMMA as u128);
        self.state = next_state;
        splitmix64(prev.wrapping_add(GAMMA))
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
