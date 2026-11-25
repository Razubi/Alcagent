//! Chaos soak, determinism and long-run persistence regressions (Directive §24 Phase 4).
//!
//! These tests deliberately run the engine far beyond the short smoke tests:
//! two simulated years (730 days) at [`TimeSpeed::Extreme`] from several seeds,
//! replay determinism checks, and a save/load continuation after a full year.
//!
//! Every assertion describes *healthy* behaviour. If one fails it points at a
//! real defect in a library crate (the engine's own invariant checker, needs
//! engine, economy, or event bus) rather than at a flaky test: all runs are
//! keyed off a fixed seed and the RNG streams are time/id derived (§21/§22).

use alc_core::TimeSpeed;
use sumo::Sim;

/// Capacity of the world event bus (see `alc_world::World::new`).
const MAX_EVENT_BUS_LEN: usize = 50_000;

/// Seeds used by the two-year soak. The first is the historical regression
/// seed used by the other integration tests; the others broaden coverage.
const SOAK_SEEDS: [u64; 3] = [0xA1C_0A_6E_17_0, 1, 2026];

/// Assert the engine's own invariant checker is clean.
fn assert_engine_invariants(sim: &Sim, label: &str) {
    let errs = sim.check_invariants();
    assert!(
        errs.is_empty(),
        "{label}: engine invariants violated: {errs:?}"
    );
}

/// Assert the extra physical-state invariants the engine checker does not
/// currently cover (NaN needs, finite budget, bus cap).
fn assert_physical_state(sim: &Sim, label: &str) {
    let w = sim.world();

    let pop = w.population();
    let cap = w.capacity();
    assert!(
        pop <= cap,
        "{label}: population {pop} exceeds capacity {cap}"
    );

    // Every stored need pressure is a finite number in [0, 1]. A NaN here
    // poisons utility comparisons and guard tension for the rest of the run.
    for p in &w.prisoners {
        let needs = [
            p.needs.hunger,
            p.needs.thirst,
            p.needs.sleep,
            p.needs.fatigue,
            p.needs.hygiene,
            p.needs.medical_need,
            p.needs.exercise,
            p.needs.temperature_comfort,
            p.needs.bathroom,
            p.needs.safety,
            p.needs.companionship,
            p.needs.respect,
            p.needs.freedom,
            p.needs.autonomy,
            p.needs.purpose,
            p.needs.stimulation,
            p.needs.stability,
            p.needs.privacy,
            p.needs.belonging,
            p.needs.friendship,
            p.needs.family_contact,
            p.needs.group_acceptance,
            p.needs.status,
            p.needs.protection,
            p.needs.recognition,
        ];
        for (i, value) in needs.iter().enumerate() {
            assert!(
                value.is_finite(),
                "{label}: prisoner {} need[{i}] is non-finite ({value})",
                p.id.raw()
            );
            assert!(
                (0.0..=1.0).contains(value),
                "{label}: prisoner {} need[{i}] out of range ({value})",
                p.id.raw()
            );
        }
        assert!(
            p.needs.total_pressure().is_finite(),
            "{label}: prisoner {} total_pressure is non-finite",
            p.id.raw()
        );
    }

    // The budget is an i64 ledger; both its major-unit view and the derived
    // health ratio must stay finite over multi-year runs.
    assert!(
        w.economy.cash.as_major().is_finite(),
        "{label}: economy budget is non-finite"
    );
    assert!(
        w.economy.health().is_finite(),
        "{label}: economy health is non-finite"
    );

    // Bounded history must never silently grow past its configured cap.
    assert!(
        w.events.len() <= MAX_EVENT_BUS_LEN,
        "{label}: event bus {} exceeds {MAX_EVENT_BUS_LEN}",
        w.events.len()
    );
}

/// Both invariant surfaces at once.
fn assert_healthy(sim: &Sim, label: &str) {
    assert_engine_invariants(sim, label);
    assert_physical_state(sim, label);
}

/// Run a fresh world of `seed` for `days` days at Extreme speed.
fn run(seed: u64, days: u64) -> Sim {
    let mut sim = Sim::new(seed);
    sim.set_speed(TimeSpeed::Extreme);
    sim.run_until(sim.world().time.add_days(days));
    sim
}

/// Fingerprint used by the determinism regressions. Event count and final
/// population are the task's required signals; the trace hash adds a stronger
/// check that the event *sequence* is reproducible too.
fn fingerprint(seed: u64, days: u64) -> (u32, usize, u64) {
    let sim = run(seed, days);
    (
        sim.world().population(),
        sim.events().len(),
        sim.trace_hash(),
    )
}

#[test]
fn chaos_soak_730_days_three_seeds() {
    // Collect every seed's result before asserting so one failure does not
    // hide the other two. The per-seed line is visible with --nocapture.
    let mut engine_violations: Vec<(u64, Vec<String>)> = Vec::new();

    for seed in SOAK_SEEDS {
        let sim = run(seed, 730);
        let label = format!("2-year soak (seed {seed})");

        assert!(
            sim.days_simulated() >= 729,
            "{label}: only {} days simulated",
            sim.days_simulated()
        );
        assert_physical_state(&sim, &label);

        let errs = sim.check_invariants();
        if !errs.is_empty() {
            engine_violations.push((seed, errs.clone()));
        }
        eprintln!(
            "SOAK seed={seed} days={} pop={}/{} cash_cents={} security={:.2} lockdown={} events={} invariants={}",
            sim.days_simulated(),
            sim.world().population(),
            sim.world().capacity(),
            sim.world().economy.cash.0,
            sim.world().policies.security_level,
            sim.world().policies.lockdown,
            sim.events().len(),
            if errs.is_empty() {
                "clean".to_string()
            } else {
                format!("{errs:?}")
            },
        );
    }

    assert!(
        engine_violations.is_empty(),
        "engine invariants violated after 2 years: {engine_violations:?}"
    );
}

#[test]
fn same_seed_reproduces_event_count_population_and_trace() {
    let a = fingerprint(0xA1C_0A_6E_17_0, 10);
    let b = fingerprint(0xA1C_0A_6E_17_0, 10);
    assert_eq!(
        a.0, b.0,
        "same seed changed final population: {} vs {}",
        a.0, b.0
    );
    assert_eq!(
        a.1, b.1,
        "same seed changed event count: {} vs {}",
        a.1, b.1
    );
    assert_eq!(a.2, b.2, "same seed changed trace hash: {} vs {}", a.2, b.2);
}

#[test]
fn different_seeds_produce_different_event_counts() {
    // At a 10-day horizon the bounded bus is not yet saturated, so the count
    // is a meaningful signal rather than a constant 50_000.
    let a = fingerprint(1, 10);
    let b = fingerprint(2, 10);
    assert_ne!(
        a.1, b.1,
        "distinct seeds produced an identical event count ({}) — RNG looks degenerate",
        a.1
    );
    assert_ne!(a.2, b.2, "distinct seeds produced an identical trace hash");
}

#[test]
fn save_load_after_year_continues_invariant_clean_and_deterministic() {
    let dir = std::env::temp_dir().join(format!("sumo-chaos-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let mut original = run(0xA1C_0A_6E_17_0, 365);
    original.save(&dir).expect("save 365-day world");
    let mut loaded = Sim::load(&dir).expect("load 365-day world");

    // Sim::load resets the engine-side speed to Paused; restore the clock so
    // both copies actually advance during the continuation window.
    loaded.set_speed(TimeSpeed::Extreme);

    original.run_until(original.world().time.add_days(30));
    loaded.run_until(loaded.world().time.add_days(30));

    assert_healthy(&original, "original after 365+30 days");
    assert_healthy(&loaded, "loaded after 365+30 days");

    // A loaded world must not merely avoid crashing: it must replay the same
    // deterministic trajectory (RNG is seeded from world time/ids, §21).
    assert_eq!(
        loaded.world().time,
        original.world().time,
        "clock diverged after load"
    );
    assert_eq!(
        loaded.days_simulated(),
        original.days_simulated(),
        "day counter diverged after load"
    );
    assert_eq!(
        loaded.world().population(),
        original.world().population(),
        "population diverged after load"
    );
    assert_eq!(
        loaded.world().economy.cash.0,
        original.world().economy.cash.0,
        "budget diverged after load"
    );
    assert_eq!(
        loaded.events().len(),
        original.events().len(),
        "event count diverged after load"
    );
    assert_eq!(
        loaded.trace_hash(),
        original.trace_hash(),
        "event trace diverged after load"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The world should keep producing a *variety* of events, not just guard
/// reports. A healthy year shows social, conflict, institutional and lifecycle
/// activity alongside security response (Directive §17).
#[test]
fn event_kinds_are_diverse_after_a_year() {
    let sim = run(0xA1C_0A_6E_17_0, 365);

    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for ev in sim.events().recent() {
        *counts.entry(format!("{:?}", ev.kind)).or_insert(0) += 1;
    }
    println!("event distribution after 365 days: {counts:?}");

    assert!(
        counts.len() >= 10,
        "too few distinct event kinds: {counts:?}"
    );
    for kind in [
        "RelationshipShift",
        "Rumor",
        "Argument",
        "ParoleHearing",
        "PolicyChange",
        "SecurityResponse",
    ] {
        assert!(
            counts.contains_key(kind),
            "no {kind} events in a full year: {counts:?}"
        );
    }
    // Guard reports must not swamp all other activity combined.
    let security = counts.get("SecurityResponse").copied().unwrap_or(0)
        + counts.get("GuardReport").copied().unwrap_or(0);
    let total: usize = counts.values().sum();
    assert!(
        security * 2 < total,
        "guard events dominate: {security}/{total}: {counts:?}"
    );
}
