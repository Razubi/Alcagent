//! Condition-driven daily event generators.
//!
//! The world tick supplies normalized pressure signals; this module decides
//! which explainable events those pressures justify. It is deliberately pure:
//! no subsystem state is mutated here, every event records its causal inputs,
//! and output is a deterministic function of `(seed, time, conditions)`.

use alc_core::SimTime;

use crate::{EventKind, WorldEvent};

/// Hard cap on condition-driven events emitted for one simulation day.
pub const MAX_EVENTS_PER_DAY: usize = 3;

/// Normalized world signals used to justify an event.
///
/// Every field is expected to be in `0.0..=1.0`; callers clamp before passing.
#[derive(Debug, Clone, Copy, Default)]
pub struct EventConditions {
    pub population: u32,
    pub hungry_fraction: f32,
    pub lonely_fraction: f32,
    pub family_contact_deficit: f32,
    pub guard_stress: f32,
    pub guard_shortage: f32,
    pub maintenance_debt: f32,
    pub medical_neglect: f32,
    pub tension: f32,
    pub crowding: f32,
    pub contraband_pressure: f32,
}

/// World change requested when a generated event is accepted by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailyEventEffect {
    SpreadRumor,
    ImproveFamilyContact,
    DamageGuardMorale,
    IncreaseMaintenanceDebt,
    IncreaseHunger,
    IncreaseMedicalNeed,
    RaiseTension,
    ConfiscateContraband,
}

#[derive(Debug, Clone)]
pub struct GeneratedEvent {
    pub event: WorldEvent,
    pub effect: DailyEventEffect,
}

#[derive(Debug, Clone)]
struct Candidate {
    order: usize,
    kind: EventKind,
    summary: String,
    causes: Vec<String>,
    actor: Option<u64>,
    confidence: f32,
    effect: DailyEventEffect,
    score: f32,
}

/// Generate at most [`MAX_EVENTS_PER_DAY`] events from world conditions.
///
/// `actors` is an optional pool used only to attach a plausible actor id; it is
/// never mutated. The same inputs always produce the same event order, ids are
/// left at zero for `EventBus::emit` to assign.
pub fn generate_daily_events(
    seed: u64,
    at: SimTime,
    conditions: &EventConditions,
    actors: &[u64],
) -> Vec<GeneratedEvent> {
    let day = at.as_days();
    let mut candidates = Vec::with_capacity(8);
    let mut order = 0usize;

    push_candidate(
        &mut candidates,
        &mut order,
        seed,
        day,
        actors,
        "rumor",
        conditions,
        conditions.tension + conditions.lonely_fraction * 0.35,
        0.55,
        1.5,
        EventKind::Rumor,
        DailyEventEffect::SpreadRumor,
        0.55,
        |c| format!("A rumor spreads: tension {:.2}, isolation {:.2}", c.tension, c.lonely_fraction),
        |c| vec![
            format!("tension {:.2} exceeded 0.55 threshold", c.tension),
            format!("lonely_fraction {:.2} amplified the rumor", c.lonely_fraction),
        ],
    );

    push_candidate(
        &mut candidates,
        &mut order,
        seed,
        day,
        actors,
        "family",
        conditions,
        conditions.family_contact_deficit,
        0.35,
        1.4,
        EventKind::FamilyCorrespondence,
        DailyEventEffect::ImproveFamilyContact,
        1.0,
        |c| format!("Family correspondence arrives for an inmate with family-contact deficit {:.2}", c.family_contact_deficit),
        |c| vec![format!("family_contact_deficit {:.2} exceeded 0.35 threshold", c.family_contact_deficit)],
    );

    push_candidate(
        &mut candidates,
        &mut order,
        seed,
        day,
        actors,
        "misconduct",
        conditions,
        conditions.guard_stress.max(conditions.guard_shortage),
        0.50,
        1.5,
        EventKind::StaffMisconduct,
        DailyEventEffect::DamageGuardMorale,
        0.8,
        |c| format!("A guard crosses a policy line under stress {:.2} and staffing deficit {:.2}", c.guard_stress, c.guard_shortage),
        |c| vec![
            format!("guard_stress {:.2} contributed", c.guard_stress),
            format!("guard_shortage {:.2} reduced oversight", c.guard_shortage),
        ],
    );

    push_candidate(
        &mut candidates,
        &mut order,
        seed,
        day,
        actors,
        "equipment",
        conditions,
        conditions.maintenance_debt,
        0.40,
        1.5,
        EventKind::EquipmentFailure,
        DailyEventEffect::IncreaseMaintenanceDebt,
        0.9,
        |c| format!("Equipment fails after maintenance debt reaches {:.2}", c.maintenance_debt),
        |c| vec![format!("maintenance_debt {:.2} exceeded 0.40 threshold", c.maintenance_debt)],
    );

    push_candidate(
        &mut candidates,
        &mut order,
        seed,
        day,
        actors,
        "food",
        conditions,
        conditions.hungry_fraction,
        0.45,
        1.6,
        EventKind::FoodShortage,
        DailyEventEffect::IncreaseHunger,
        0.95,
        |c| format!("Food service falls short: {:.0}% of prisoners report hunger", c.hungry_fraction * 100.0),
        |c| vec![format!("hungry_fraction {:.2} exceeded 0.45 threshold", c.hungry_fraction)],
    );

    push_candidate(
        &mut candidates,
        &mut order,
        seed,
        day,
        actors,
        "medical",
        conditions,
        conditions.medical_neglect,
        0.55,
        1.6,
        EventKind::MedicalEmergency,
        DailyEventEffect::IncreaseMedicalNeed,
        0.95,
        |c| format!("A treatable condition becomes an emergency under medical neglect {:.2}", c.medical_neglect),
        |c| vec![format!("medical_neglect {:.2} exceeded 0.55 threshold", c.medical_neglect)],
    );

    push_candidate(
        &mut candidates,
        &mut order,
        seed,
        day,
        actors,
        "protest",
        conditions,
        (conditions.tension + conditions.crowding) * 0.5,
        0.45,
        1.5,
        EventKind::Protests,
        DailyEventEffect::RaiseTension,
        0.85,
        |c| format!("A protest forms: tension {:.2}, crowding {:.2}", c.tension, c.crowding),
        |c| vec![
            format!("tension {:.2} contributed", c.tension),
            format!("crowding {:.2} contributed", c.crowding),
        ],
    );

    push_candidate(
        &mut candidates,
        &mut order,
        seed,
        day,
        actors,
        "contraband",
        conditions,
        conditions.contraband_pressure,
        0.45,
        1.5,
        EventKind::ContrabandFound,
        DailyEventEffect::ConfiscateContraband,
        0.9,
        |c| format!("Contraband is found during a search (pressure {:.2})", c.contraband_pressure),
        |c| vec![format!("contraband_pressure {:.2} exceeded 0.45 threshold", c.contraband_pressure)],
    );

    // Strongest justified pressures win; the fixed order breaks exact ties.
    candidates.sort_by(|a, b| {
        b.score.total_cmp(&a.score).then_with(|| a.order.cmp(&b.order))
    });

    candidates
        .into_iter()
        .take(MAX_EVENTS_PER_DAY)
        .map(|c| {
            let mut event = WorldEvent::new(0, at, c.kind, c.summary)
                .with_confidence(c.confidence);
            if let Some(actor) = c.actor {
                event = event.with_actor(actor);
            }
            event = event.with_cause("condition-driven");
            for cause in c.causes {
                event = event.with_cause(cause);
            }
            GeneratedEvent { event, effect: c.effect }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn push_candidate<S, C>(
    candidates: &mut Vec<Candidate>,
    order: &mut usize,
    seed: u64,
    day: u64,
    actors: &[u64],
    salt_name: &str,
    conditions: &EventConditions,
    value: f32,
    threshold: f32,
    scale: f32,
    kind: EventKind,
    effect: DailyEventEffect,
    confidence: f32,
    summary: S,
    causes: C,
) where
    S: FnOnce(&EventConditions) -> String,
    C: FnOnce(&EventConditions) -> Vec<String>,
{
    let sal = salt(salt_name);
    let chance = activation(value, threshold, scale);
    if chance <= 0.0 || unit_noise(seed, day, sal) >= chance {
        return;
    }

    let actor = pick_actor(seed, day, sal, actors);
    // A small deterministic jitter breaks score ties without changing which
    // conditions are capable of firing.
    let score = chance * (0.85 + 0.30 * unit_noise(seed, day, sal ^ 0xA5A5_5A5A));
    candidates.push(Candidate {
        order: *order,
        kind,
        summary: summary(conditions),
        causes: causes(conditions),
        actor,
        confidence,
        effect,
        score,
    });
    *order += 1;
}

fn activation(value: f32, threshold: f32, scale: f32) -> f32 {
    ((value - threshold).max(0.0) * scale).clamp(0.0, 0.95)
}

fn salt(name: &str) -> u64 {
    // FNV-1a: stable across builds and save formats.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn unit_noise(seed: u64, day: u64, salt: u64) -> f32 {
    let mut x = seed
        ^ day.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ salt.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    ((x >> 40) as f32) / ((1u64 << 24) as f32)
}

fn pick_actor(seed: u64, day: u64, salt: u64, actors: &[u64]) -> Option<u64> {
    if actors.is_empty() {
        return None;
    }
    let index = (unit_noise(seed, day, salt ^ 0x1234_5678) * actors.len() as f32) as usize;
    actors.get(index % actors.len()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extreme() -> EventConditions {
        EventConditions {
            population: 200,
            hungry_fraction: 1.0,
            lonely_fraction: 1.0,
            family_contact_deficit: 1.0,
            guard_stress: 1.0,
            guard_shortage: 1.0,
            maintenance_debt: 1.0,
            medical_neglect: 1.0,
            tension: 1.0,
            crowding: 1.0,
            contraband_pressure: 1.0,
        }
    }

    fn signature(seed: u64, at: SimTime, c: &EventConditions) -> Vec<(EventKind, String, Vec<String>)> {
        generate_daily_events(seed, at, c, &[7, 11, 19])
            .into_iter()
            .map(|g| (g.event.kind, g.event.summary, g.event.causes))
            .collect()
    }

    #[test]
    fn generation_is_deterministic_and_bounded() {
        let at = SimTime::from_days(12);
        let first = signature(0x0_0050, at, &extreme());
        let second = signature(0x0_0050, at, &extreme());
        assert_eq!(first, second);
        assert!(!first.is_empty());
        assert!(first.len() <= MAX_EVENTS_PER_DAY);
    }

    #[test]
    fn calm_conditions_generate_nothing() {
        let generated = generate_daily_events(1, SimTime::from_days(3), &EventConditions::default(), &[]);
        assert!(generated.is_empty());
    }

    #[test]
    fn generated_events_carry_causes_and_effects() {
        let generated = generate_daily_events(9, SimTime::from_days(4), &extreme(), &[42]);
        for g in generated {
            assert!(!g.event.causes.is_empty());
            assert!(g.event.causes.iter().all(|cause| !cause.is_empty()));
            assert!(g.event.confidence > 0.0 && g.event.confidence <= 1.0);
            assert!(matches!(g.event.actors.as_slice(), [42]));
            // Distinct effect variants must stay meaningful to the engine.
            let _ = match g.effect {
                DailyEventEffect::SpreadRumor
                | DailyEventEffect::ImproveFamilyContact
                | DailyEventEffect::DamageGuardMorale
                | DailyEventEffect::IncreaseMaintenanceDebt
                | DailyEventEffect::IncreaseHunger
                | DailyEventEffect::IncreaseMedicalNeed
                | DailyEventEffect::RaiseTension
                | DailyEventEffect::ConfiscateContraband => (),
            };
        }
    }

    #[test]
    fn seed_changes_the_selected_events() {
        let at = SimTime::from_days(31);
        let c = EventConditions {
            tension: 0.9,
            lonely_fraction: 0.8,
            family_contact_deficit: 0.8,
            guard_stress: 0.9,
            maintenance_debt: 0.8,
            medical_neglect: 0.9,
            contraband_pressure: 0.9,
            ..EventConditions::default()
        };
        assert_ne!(signature(1, at, &c), signature(2, at, &c));
    }
}
