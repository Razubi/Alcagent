//! Stability tests for the state digest (bug B10).
//!
//! The digest must be computed with a portable, deterministic FNV-1a hasher
//! (never `std::collections::hash_map::DefaultHasher`) so that identical
//! logical states always produce identical digests. These tests exercise the
//! public digest API through `sumo::state_digest` and `Sim`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use sumo::state_digest::{compute_full_state_digest, compute_state_digest, compute_world_digest};
use sumo::{PolicyMode, Sim};

mod unique {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub fn temp_dir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "sumo-state-digest-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
}

#[test]
fn digest_is_identical_for_two_identical_sims() {
    let mut a = Sim::new(0x5A17_B10C);
    let mut b = Sim::new(0x5A17_B10C);

    for _ in 0..8 {
        a.tick();
        b.tick();
    }

    assert_eq!(compute_state_digest(&a.engine_state), compute_state_digest(&b.engine_state));
    assert_eq!(compute_world_digest(&a.world), compute_world_digest(&b.world));
    assert_eq!(compute_full_state_digest(&a), compute_full_state_digest(&b));
}

#[test]
fn digest_changes_when_a_single_state_field_changes() {
    let mut a = Sim::new(0xC0DE_1234);
    let b = Sim::new(0xC0DE_1234);

    assert_eq!(compute_full_state_digest(&a), compute_full_state_digest(&b));

    // Change exactly one engine field via the public API.
    let pid = a.world().prisoners.first().expect("bootstrap prisoners").id;
    a.set_post_release(pid, true);

    assert_ne!(
        compute_full_state_digest(&a),
        compute_full_state_digest(&b),
        "changing one engine state field must change the digest"
    );

    // A single policy-mode change must also be reflected.
    let before = compute_full_state_digest(&a);
    a.set_policy_mode(PolicyMode::Shadow);
    assert_ne!(compute_full_state_digest(&a), before);
}

#[test]
fn digest_matches_after_save_and_load() {
    let dir: PathBuf = unique::temp_dir("roundtrip");
    let sim = Sim::new(0xB10C_5A17);

    let before_engine = compute_state_digest(&sim.engine_state);
    let before_world = compute_world_digest(&sim.world);
    let before_full = compute_full_state_digest(&sim);

    sim.save(&dir).expect("save must succeed");
    let loaded = Sim::load(&dir).expect("load must succeed");

    assert_eq!(compute_state_digest(&loaded.engine_state), before_engine);
    assert_eq!(compute_world_digest(&loaded.world), before_world);
    assert_eq!(compute_full_state_digest(&loaded), before_full);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn source_contains_no_default_hasher() {
    // Guard against regressions: the stable digest must never fall back to
    // `DefaultHasher`, which is explicitly non-portable.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/state_digest.rs");
    let source = std::fs::read_to_string(&path).expect("read state_digest.rs");
    assert!(
        !source.contains("DefaultHasher"),
        "state_digest.rs must not reference DefaultHasher"
    );
}
