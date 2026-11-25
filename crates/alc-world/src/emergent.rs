//! Condition-driven emergent events (Spec 007).
//!
//! This module keeps the daily pressure scan and its bounded effects separate
//! from the main tick loop. All iteration follows `Vec` order so identical
//! worlds produce identical events without depending on `HashMap` ordering.

use alc_core::PrisonerId;
use alc_events::{
    DailyEventEffect, EventConditions, GeneratedEvent, MAX_EVENTS_PER_DAY, generate_daily_events,
};
use alc_prisoner::NeedTag;

use crate::World;

impl World {
    /// Reduce live world state to normalized pressure signals, generate at
    /// most three explainable events, then apply their bounded consequences.
    pub(crate) fn emergent_events_step(&mut self) {
        let population = self.population();
        if population == 0 {
            return;
        }

        let mut hunger_sum = 0.0f32;
        let mut lonely_sum = 0.0f32;
        let mut family_sum = 0.0f32;
        let mut medical_sum = 0.0f32;
        let mut insecurity_sum = 0.0f32;
        let mut contraband_sum = 0.0f32;

        for p in &self.prisoners {
            let hunger = p.needs.hunger.clamp(0.0, 1.0);
            hunger_sum += hunger;
            lonely_sum += (p.needs.companionship * 0.6 + p.needs.belonging * 0.4).clamp(0.0, 1.0);
            family_sum += p.needs.family_contact.clamp(0.0, 1.0);
            medical_sum += p.needs.medical_need.clamp(0.0, 1.0);
            // Insecurity, disrespect and hunger are the explainable roots of
            // collective tension; safety need is high when prisoners feel unsafe.
            insecurity_sum +=
                (p.needs.safety * 0.55 + p.needs.respect * 0.30 + hunger * 0.15).clamp(0.0, 1.0);

            // Look up by prisoner id rather than iterating the contraband
            // HashMap, whose order must never leak into the world digest.
            if let Some(st) = self.contraband_state.get(&p.id) {
                let flag_pressure = if st.flags_confiscated { 0.4 } else { 0.0 };
                contraband_sum += (flag_pressure
                    + st.flagged_days as f32 * 0.02
                    + st.discovered_count as f32 * 0.03)
                    .clamp(0.0, 1.0);
            }
        }

        let mut capacity = 0u32;
        let mut staff_required = 0u32;
        let mut facility_count = 0u32;
        let mut maintenance_sum = 0.0f32;
        for f in &self.facilities {
            capacity = capacity.saturating_add(f.capacity);
            staff_required = staff_required.saturating_add(f.staff_required);
            maintenance_sum += (1.0 - f.condition.clamp(0.0, 1.0)).clamp(0.0, 1.0);
            facility_count += 1;
        }

        let mut guard_stress_sum = 0.0f32;
        for g in &self.guards {
            guard_stress_sum += g.stress.clamp(0.0, 1.0);
        }
        let guard_stress = if self.guards.is_empty() {
            1.0
        } else {
            (guard_stress_sum / self.guards.len() as f32).clamp(0.0, 1.0)
        };
        let staff_coverage = if staff_required == 0 {
            1.0
        } else {
            (self.guards.len() as f32 / staff_required as f32).min(1.0)
        };
        let guard_shortage = (1.0 - staff_coverage).clamp(0.0, 1.0);
        let maintenance_debt = if facility_count == 0 {
            1.0
        } else {
            (maintenance_sum / facility_count as f32).clamp(0.0, 1.0)
        };
        let crowding = if capacity == 0 {
            1.0
        } else {
            (population.saturating_sub(capacity) as f32 / capacity as f32).clamp(0.0, 1.0)
        };

        let conditions = EventConditions {
            population,
            hungry_fraction: (hunger_sum / population as f32).clamp(0.0, 1.0),
            lonely_fraction: (lonely_sum / population as f32).clamp(0.0, 1.0),
            family_contact_deficit: (family_sum / population as f32).clamp(0.0, 1.0),
            guard_stress,
            guard_shortage,
            maintenance_debt,
            medical_neglect: (medical_sum / population as f32).clamp(0.0, 1.0),
            tension: (insecurity_sum / population as f32).clamp(0.0, 1.0),
            crowding,
            contraband_pressure: (contraband_sum / population as f32).clamp(0.0, 1.0),
        };

        // One current prisoner is enough to attach a plausible actor without
        // allocating a roster. Actor choice is deterministic by day.
        let actor_index = self.days_simulated as usize % self.prisoners.len();
        let actor = self.prisoners.get(actor_index).map(|p| p.id.raw());
        let actors = actor.as_slice();

        let generated = generate_daily_events(self.seed, self.time, &conditions, actors);
        debug_assert!(generated.len() <= MAX_EVENTS_PER_DAY);
        for generated_event in generated {
            self.apply_daily_event_effect(&generated_event);
            self.events.emit(generated_event.event);
        }
    }

    /// Apply one generator-approved effect with bounded, world-owned mutations.
    /// Effects never add or remove agents, facilities, gangs or social dyads.
    fn apply_daily_event_effect(&mut self, generated: &GeneratedEvent) {
        let actor = generated.event.actors.first().copied().map(PrisonerId);
        match generated.effect {
            DailyEventEffect::SpreadRumor => {
                for p in &mut self.prisoners {
                    p.needs.impulse(NeedTag::Safety, 0.02);
                }
            }
            DailyEventEffect::ImproveFamilyContact => {
                if let Some(id) = actor {
                    if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == id) {
                        p.needs.relieve(NeedTag::FamilyContact, 0.08);
                    }
                }
            }
            DailyEventEffect::DamageGuardMorale => {
                for g in &mut self.guards {
                    g.morale = (g.morale - 0.04).clamp(0.0, 1.0);
                    g.stress = (g.stress + 0.03).clamp(0.0, 1.0);
                }
            }
            DailyEventEffect::IncreaseMaintenanceDebt => {
                for f in &mut self.facilities {
                    f.condition = (f.condition - 0.02).clamp(0.0, 1.0);
                }
            }
            DailyEventEffect::IncreaseHunger => {
                for p in &mut self.prisoners {
                    p.needs.impulse(NeedTag::Hunger, 0.06);
                }
            }
            DailyEventEffect::IncreaseMedicalNeed => {
                if let Some(id) = actor {
                    if let Some(p) = self.prisoners.iter_mut().find(|p| p.id == id) {
                        p.needs.impulse(NeedTag::Medical, 0.12);
                    }
                }
            }
            DailyEventEffect::RaiseTension => {
                for p in &mut self.prisoners {
                    p.needs.impulse(NeedTag::Safety, 0.03);
                    p.needs.impulse(NeedTag::Respect, 0.02);
                }
            }
            DailyEventEffect::ConfiscateContraband => {
                if let Some(id) = actor {
                    let st = self.contraband_state.entry(id).or_default();
                    st.flags_confiscated = true;
                    st.flagged_days = st.flagged_days.saturating_add(1).min(30);
                    st.discovered_count = st.discovered_count.saturating_add(1).min(1_000);
                    st.inventory.clear();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PrisonerContrabandState;
    use alc_core::{PrisonerId, SimTime};
    use alc_events::{EventKind, WorldEvent};
    use alc_social::contraband::ContrabandItem;

    fn generated(effect: DailyEventEffect, actor: PrisonerId) -> GeneratedEvent {
        GeneratedEvent {
            event: WorldEvent::new(0, SimTime::EPOCH, EventKind::Rumor, "condition-driven test event")
                .with_actor(actor.raw())
                .with_cause("condition-driven"),
            effect,
        }
    }

    #[test]
    fn daily_event_effects_alter_world_state() {
        let mut w = World::new(0x0_007);
        let id = w.prisoners[0].id;
        {
            let p = w.prisoners.iter_mut().find(|p| p.id == id).expect("prisoner");
            p.needs.hunger = 0.2;
            p.needs.safety = 0.2;
            p.needs.respect = 0.2;
            p.needs.family_contact = 0.8;
            p.needs.medical_need = 0.1;
        }
        w.facilities[0].condition = 1.0;
        w.guards[0].morale = 0.8;
        w.guards[0].stress = 0.2;
        w.contraband_state.entry(id).or_default().inventory.insert(ContrabandItem::Weapons, 2);

        w.apply_daily_event_effect(&generated(DailyEventEffect::SpreadRumor, id));
        assert!(w.prisoners[0].needs.safety > 0.2);

        w.apply_daily_event_effect(&generated(DailyEventEffect::ImproveFamilyContact, id));
        assert!(w.prisoners[0].needs.family_contact < 0.8);

        w.apply_daily_event_effect(&generated(DailyEventEffect::DamageGuardMorale, id));
        assert!(w.guards[0].morale < 0.8);
        assert!(w.guards[0].stress > 0.2);

        w.apply_daily_event_effect(&generated(DailyEventEffect::IncreaseMaintenanceDebt, id));
        assert!(w.facilities[0].condition < 1.0);

        w.apply_daily_event_effect(&generated(DailyEventEffect::IncreaseHunger, id));
        assert!(w.prisoners[0].needs.hunger > 0.2);

        w.apply_daily_event_effect(&generated(DailyEventEffect::IncreaseMedicalNeed, id));
        assert!(w.prisoners[0].needs.medical_need > 0.1);

        w.apply_daily_event_effect(&generated(DailyEventEffect::RaiseTension, id));
        assert!(w.prisoners[0].needs.respect > 0.2);

        let before = w.contraband_state.get(&id).unwrap().discovered_count;
        w.apply_daily_event_effect(&generated(DailyEventEffect::ConfiscateContraband, id));
        let st: &PrisonerContrabandState = w.contraband_state.get(&id).expect("contraband state");
        assert!(st.flags_confiscated);
        assert!(st.inventory.is_empty());
        assert_eq!(st.discovered_count, before + 1);
        assert!(st.flagged_days >= 1);
    }
}
