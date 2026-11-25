//! Reusable bug-fixing infrastructure for the `sumo` engine.
//!
//! This file owns three pieces of hardening tooling ( roadmap Phase 4 / Phase 5 ):
//!
//! 1. [`repro_ticket`] — a compact, deterministic "bug ticket" string built
//!    from the world seed, a target day and the last N decision strings. It is
//!    designed to be pasted verbatim into an issue so a failure can be
//!    reproduced without attaching a full save.
//! 2. Property-style fuzz tests (plain loops, **no new dependencies**) that
//!    drive [`sumo::Sim::deliver_decision`] with seeded, pseudo-random but
//!    reproducible decision sequences for 200 ticks and assert that the world
//!    stays healthy (empty invariants, no panic, bounded event bus, population
//!    within capacity, finite needs).
//! 3. A replay-equality test proving a save -> load -> continue run produces
//!    the same [`sumo::Sim::trace_hash`] as an uninterrupted run.
//!
//! The fuzz driver uses [`alc_core::RngStream`] so the decision stream is
//! reproducible from a single seed. All targets are picked from the *live*
//! world (current prisoners, facilities and gangs), never from stale ids.

use alc_agents::{Decision, DecisionAction};
use alc_core::{PrisonerId, RngStream, TimeSpeed};
use sumo::Sim;

/// Capacity of the world event bus (`alc_world::World::new` uses 50_000).
const MAX_EVENT_BUS_LEN: usize = 50_000;

/// Number of decision strings retained in a repro ticket.
const TICKET_WINDOW: usize = 8;

/// Build a compact, deterministic bug-reproduction ticket.
///
/// The output has the shape `seed=<seed>;day=<day>;inputs=<comma-separated>`.
/// It is a pure function of its arguments: identical arguments always produce
/// an identical string, which is the property that makes it useful in issues
/// and regression tests.
pub fn repro_ticket(seed: u64, target_day: u64, last_inputs: &[String]) -> String {
    format!(
        "seed={seed};day={target_day};inputs={}",
        last_inputs.join(",")
    )
}

/// A short, stable label for a delivered decision, used in tickets.
fn decision_label(actor: PrisonerId, decision: &Decision) -> String {
    format!("{}:{}", actor.raw(), decision.action_name())
}

/// Keep the last `TICKET_WINDOW` labels in a rolling window.
fn remember(window: &mut Vec<String>, label: String) {
    if window.len() == TICKET_WINDOW {
        window.remove(0);
    }
    window.push(label);
}

/// Assert every invariant the bug-hardening suite cares about.
///
/// Panics with a [`repro_ticket`] on failure so the exact decision sequence
/// that broke the world is available in the test output.
fn assert_healthy(sim: &Sim, label: &str, seed: u64, ticket_inputs: &[String]) {
    let ticket = repro_ticket(seed, sim.days_simulated(), ticket_inputs);

    let violations = sim.check_invariants();
    assert!(
        violations.is_empty(),
        "{label}: engine invariants violated: {violations:?} | {ticket}"
    );

    let w = sim.world();
    let pop = w.population();
    let cap = w.capacity();
    assert!(
        pop <= cap,
        "{label}: population {pop} exceeds capacity {cap} | {ticket}"
    );

    assert!(
        w.events.len() <= MAX_EVENT_BUS_LEN,
        "{label}: event bus {} exceeds {MAX_EVENT_BUS_LEN} | {ticket}",
        w.events.len()
    );

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
                "{label}: prisoner {} need[{i}] is non-finite ({value}) | {ticket}",
                p.id.raw()
            );
        }
        assert!(
            p.needs.total_pressure().is_finite(),
            "{label}: prisoner {} total_pressure is non-finite | {ticket}",
            p.id.raw()
        );
    }
}

// ---------------------------------------------------------------------------
// 1. repro_ticket determinism
// ---------------------------------------------------------------------------

#[test]
fn repro_ticket_is_deterministic() {
    let inputs = vec![
        "3:socialize".to_string(),
        "7:fight".to_string(),
        "11:comply".to_string(),
    ];
    let a = repro_ticket(4242, 42, &inputs);
    let b = repro_ticket(4242, 42, &inputs);
    assert_eq!(a, b, "same arguments must produce an identical ticket");
    assert_eq!(a, "seed=4242;day=42;inputs=3:socialize,7:fight,11:comply");
}

#[test]
fn repro_ticket_changes_with_any_component() {
    let inputs = vec!["1:comply".to_string(), "2:refuse".to_string()];
    let base = repro_ticket(7, 3, &inputs);

    assert_ne!(
        base,
        repro_ticket(8, 3, &inputs),
        "seed must change the ticket"
    );
    assert_ne!(
        base,
        repro_ticket(7, 4, &inputs),
        "day must change the ticket"
    );

    let reordered = vec!["2:refuse".to_string(), "1:comply".to_string()];
    assert_ne!(
        base,
        repro_ticket(7, 3, &reordered),
        "input order must be preserved"
    );

    let extra = vec!["1:comply".to_string()];
    assert_ne!(
        base,
        repro_ticket(7, 3, &extra),
        "input count must change the ticket"
    );
}

#[test]
fn repro_ticket_handles_empty_inputs() {
    assert_eq!(repro_ticket(1, 0, &[]), "seed=1;day=0;inputs=");
}

// ---------------------------------------------------------------------------
// 2. Property-style fuzz of deliver_decision
// ---------------------------------------------------------------------------

/// Pick one decision to deliver, using only ids that exist in the live world.
///
/// Targets are chosen from current prisoners/facilities/gangs so decisions are
/// mostly well-formed; the validator still rejects some (visibility,
/// authority, repeated-decision safety, ...), which is exactly the mix a fuzz
/// driver wants. Untargeted actions are always available.
fn pick_decision(sim: &Sim, rng: &mut RngStream) -> (PrisonerId, Decision) {
    let w = sim.world();

    // Actor: any live prisoner.
    let actor_idx = (rng.next_u64() as usize).min(w.prisoners.len() - 1);
    let actor = w.prisoners[actor_idx].id;

    // Targets drawn from the live world (indices clamped to stay in range).
    let pick_prisoner = |rng: &mut RngStream| -> u64 {
        let i = (rng.next_u64() as usize).min(w.prisoners.len() - 1);
        w.prisoners[i].id.raw()
    };
    let pick_facility = |rng: &mut RngStream| -> u64 {
        let i = (rng.next_u64() as usize).min(w.facilities.len() - 1);
        w.facilities[i].id.0
    };

    let variant = rng.next_u64() % 13;
    let action = match variant {
        0 => DecisionAction::MoveTo {
            target_facility: pick_facility(rng),
        },
        1 => DecisionAction::Socialize {
            with: pick_prisoner(rng),
        },
        2 => DecisionAction::Fight {
            target: pick_prisoner(rng),
        },
        3 => DecisionAction::Trade {
            with: pick_prisoner(rng),
            item: "soap".to_string(),
        },
        4 => DecisionAction::Comply,
        5 => DecisionAction::Refuse,
        6 => DecisionAction::EscapeAttempt,
        7 => DecisionAction::RequestMedical,
        8 if !w.gangs.is_empty() => {
            let i = (rng.next_u64() as usize).min(w.gangs.len() - 1);
            DecisionAction::JoinGang {
                gang: w.gangs[i].id.raw(),
            }
        }
        9 => DecisionAction::LeaveGang,
        10 if !w.gangs.is_empty() => {
            let i = (rng.next_u64() as usize).min(w.gangs.len() - 1);
            DecisionAction::LeadGang {
                gang: w.gangs[i].id.raw(),
            }
        }
        11 => DecisionAction::SpreadRumor {
            about: pick_prisoner(rng),
            content: "yard rumor".to_string(),
        },
        _ => DecisionAction::None,
    };

    (actor, Decision::new(action))
}

/// Drive one seeded 200-tick decision fuzz run and return its final trace hash.
fn fuzz_run(seed: u64) -> u64 {
    let mut sim = Sim::new(seed);
    sim.set_speed(TimeSpeed::Extreme);

    let mut rng = RngStream::new(seed, u64::MAX);
    let mut ticket_inputs: Vec<String> = Vec::new();

    for tick in 0..200 {
        let (actor, decision) = pick_decision(&sim, &mut rng);
        let label = decision_label(actor, &decision);

        // No panic, no state explosion: deliver then advance the clock.
        let _outcome = sim.deliver_decision(actor, decision);
        remember(&mut ticket_inputs, label);
        sim.tick();

        assert_healthy(
            &sim,
            &format!("fuzz seed={seed} tick={tick}"),
            seed,
            &ticket_inputs,
        );
    }

    sim.trace_hash()
}

#[test]
fn fuzz_deliver_decision_keeps_world_healthy() {
    // A spread of seeds exercises different intake personalities and RNG
    // streams while staying fast (200 ticks each at Extreme speed).
    for seed in [0xA1C_0A_6E_17_0_u64, 1, 2026] {
        let hash = fuzz_run(seed);
        assert_ne!(hash, 0, "seed {seed} produced a zero trace hash");
    }
}

#[test]
fn fuzz_decision_stream_is_reproducible() {
    // Same seed -> same delivered sequence -> same trace fingerprint.
    let a = fuzz_run(99);
    let b = fuzz_run(99);
    assert_eq!(a, b, "seeded decision fuzz must be reproducible");
}

// ---------------------------------------------------------------------------
// 3. Save -> load -> continue replay equality
// ---------------------------------------------------------------------------

fn replay_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sumo-bug-hardening-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn save_load_continue_matches_uninterrupted_trace_hash() {
    const SEED: u64 = 0xA1C_0A_6E_17_0;
    let checkpoint_day = 3;
    let final_day = 6;

    // Uninterrupted run: seed -> final day in one go.
    let mut uninterrupted = Sim::new(SEED);
    uninterrupted.set_speed(TimeSpeed::Extreme);
    uninterrupted.run_until(uninterrupted.world().time.add_days(final_day));

    // Save/load run: seed -> checkpoint, persist, reload, continue to final.
    let dir = replay_dir("replay");
    let mut branch = Sim::new(SEED);
    branch.set_speed(TimeSpeed::Extreme);
    branch.run_until(branch.world().time.add_days(checkpoint_day));
    branch.save(&dir).expect("save at checkpoint day");

    let mut loaded = Sim::load(&dir).expect("load checkpoint save");
    // Sim::load resets speed to Paused; restore it so the continuation ticks.
    loaded.set_speed(TimeSpeed::Extreme);
    // Target is absolute from the world epoch: the loaded world's clock already
    // includes the checkpoint days, so adding `final_day` again would overshoot.
    let target = alc_core::SimTime::EPOCH.add_days(final_day);
    loaded.run_until(target);

    let uninterrupted_hash = uninterrupted.trace_hash();
    let replayed_hash = loaded.trace_hash();

    assert_eq!(
        loaded.world().time,
        uninterrupted.world().time,
        "clock diverged after save/load continuation"
    );
    assert_eq!(
        replayed_hash, uninterrupted_hash,
        "save -> load -> continue changed the trace hash ({replayed_hash} vs {uninterrupted_hash})"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
