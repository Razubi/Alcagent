//! Capability system: named, typed abilities that prisoners may manifest
//! (Directive §3 — capability model).
//!
//! A [`Capability`] is the durable, serializable form stored on
//! `Prisoner::capabilities`. [`Capability::modifiers`] returns the
//! [`CapabilityDeltas`] the capability contributes to carrier traits; deltas
//! are applied and clamped by `Prisoner::apply_capability_modifiers`
//! (`alc_prisoner`). The design note is in
//! [`Documentation/Systems/capabilities.md`](../../../Documentation/Systems/capabilities.md).

use crate::Traits;
use serde::{Deserialize, Serialize};

/// Broad physical ability family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Physical { Strong, Fit, Fast, Durable }

/// Cellular abilities useful for escape, hideout and contraband.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cellular { CellEscape, Dig, HideContraband, HoldHideout, FakeId }

/// Practical abilities: trade, locks, repair, evasion, hideouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Practical { Trade, PickLocks, Repair, Smuggle, FitLocker, ScoreKey }

/// Social abilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Social { Persuade, Charm, Intimidate, Inform, Influence, Leadership }

/// Medical abilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Medical { FirstAid, Evaluation }

/// Family of ability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityKind {
    Physical(Physical),
    Cellular(Cellular),
    Practical(Practical),
    Social(Social),
    Medical(Medical),
}

impl CapabilityKind {
    /// Canonical serialized name of a kind (stable, `snake_case`).
    pub fn as_str(self) -> &'static str {
        match self {
            CapabilityKind::Physical(Physical::Strong) => "physical_strong",
            CapabilityKind::Physical(Physical::Fit) => "physical_fit",
            CapabilityKind::Physical(Physical::Fast) => "physical_fast",
            CapabilityKind::Physical(Physical::Durable) => "physical_durable",
            CapabilityKind::Cellular(Cellular::CellEscape) => "cell_escape",
            CapabilityKind::Cellular(Cellular::Dig) => "dig",
            CapabilityKind::Cellular(Cellular::HideContraband) => "hide_contraband",
            CapabilityKind::Cellular(Cellular::HoldHideout) => "hold_hideout",
            CapabilityKind::Cellular(Cellular::FakeId) => "fake_id",
            CapabilityKind::Practical(Practical::Trade) => "trade",
            CapabilityKind::Practical(Practical::PickLocks) => "pick_locks",
            CapabilityKind::Practical(Practical::Repair) => "repair",
            CapabilityKind::Practical(Practical::Smuggle) => "smuggle",
            CapabilityKind::Practical(Practical::FitLocker) => "fit_locker",
            CapabilityKind::Practical(Practical::ScoreKey) => "score_key",
            CapabilityKind::Social(Social::Persuade) => "persuade",
            CapabilityKind::Social(Social::Charm) => "charm",
            CapabilityKind::Social(Social::Intimidate) => "intimidate",
            CapabilityKind::Social(Social::Inform) => "inform",
            CapabilityKind::Social(Social::Influence) => "influence",
            CapabilityKind::Social(Social::Leadership) => "leadership",
            CapabilityKind::Medical(Medical::FirstAid) => "first_aid",
            CapabilityKind::Medical(Medical::Evaluation) => "evaluation",
        }
    }

    /// All capability kinds in the catalog (used by tests and tooling).
    pub const ALL: &'static [CapabilityKind] = &[
        CapabilityKind::Physical(Physical::Strong),
        CapabilityKind::Physical(Physical::Fit),
        CapabilityKind::Physical(Physical::Fast),
        CapabilityKind::Physical(Physical::Durable),
        CapabilityKind::Cellular(Cellular::CellEscape),
        CapabilityKind::Cellular(Cellular::Dig),
        CapabilityKind::Cellular(Cellular::HideContraband),
        CapabilityKind::Cellular(Cellular::HoldHideout),
        CapabilityKind::Cellular(Cellular::FakeId),
        CapabilityKind::Practical(Practical::Trade),
        CapabilityKind::Practical(Practical::PickLocks),
        CapabilityKind::Practical(Practical::Repair),
        CapabilityKind::Practical(Practical::Smuggle),
        CapabilityKind::Practical(Practical::FitLocker),
        CapabilityKind::Practical(Practical::ScoreKey),
        CapabilityKind::Social(Social::Persuade),
        CapabilityKind::Social(Social::Charm),
        CapabilityKind::Social(Social::Intimidate),
        CapabilityKind::Social(Social::Inform),
        CapabilityKind::Social(Social::Influence),
        CapabilityKind::Social(Social::Leadership),
        CapabilityKind::Medical(Medical::FirstAid),
        CapabilityKind::Medical(Medical::Evaluation),
    ];

    /// Whether the kind is a cellular ability (escape-flavored family).
    pub fn is_cellular(self) -> bool {
        matches!(self, CapabilityKind::Cellular(_))
    }
}

/// A capability a prisoner holds: canonical name + kind (persisted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub kind: CapabilityKind,
}

impl Capability {
    /// Build a capability from a kind with the canonical name.
    pub fn new(kind: CapabilityKind) -> Self {
        Self { name: kind.as_str().into(), kind }
    }

    /// Signed trait deltas the capability contributes to its carrier.
    ///
    /// Today the table is static per kind; the `traits` argument is the
    /// carrier's current traits and is kept for conditional, future deltas.
    pub fn modifiers(&self, _traits: &Traits) -> CapabilityDeltas {
        CapabilityDeltas::for_kind(self.kind)
    }
}

/// Signed trait deltas a capability contributes. Deltas are applied and
/// clamped by the carrier (see `Prisoner::apply_capability_modifiers`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CapabilityDeltas {
    pub aggression: f32,
    pub risk_tolerance: f32,
    pub sociability: f32,
    pub manipulation: f32,
    pub ambition: f32,
    pub loyalty: f32,
    pub empathy: f32,
    pub discipline: f32,
    pub intelligence: f32,
    pub emotional_stability: f32,
    pub impulsiveness: f32,
    pub risk: f32,
}

impl CapabilityDeltas {
    /// The per-kind delta table. `Intimidate`'s "+respect" flavor maps onto
    /// `risk_tolerance`: the relationship-level Respect axis lives on the
    /// engine side (`Needs`/`RelationshipDelta`).
    fn for_kind(kind: CapabilityKind) -> Self {
        use CapabilityDeltas as D;
        let d = match kind {
            CapabilityKind::Physical(Physical::Strong) => D { aggression: 0.15, risk_tolerance: 0.10, ..D::default() },
            CapabilityKind::Physical(Physical::Fit) => D { discipline: 0.10, emotional_stability: 0.05, ..D::default() },
            CapabilityKind::Physical(Physical::Fast) => D { risk_tolerance: 0.10, sociability: 0.05, ..D::default() },
            CapabilityKind::Physical(Physical::Durable) => D {
                discipline: 0.20,
                emotional_stability: 0.05,
                loyalty: 0.05,
                ..D::default()
            },
            CapabilityKind::Cellular(Cellular::CellEscape) => D {
                risk_tolerance: 0.30,
                aggression: 0.05,
                ..D::default()
            },
            CapabilityKind::Cellular(Cellular::Dig) => D { risk_tolerance: 0.25, aggression: 0.05, ..D::default() },
            CapabilityKind::Cellular(Cellular::HideContraband) => D {
                risk_tolerance: 0.15,
                discipline: -0.05,
                ..D::default()
            },
            CapabilityKind::Cellular(Cellular::HoldHideout) => D {
                loyalty: 0.10,
                risk_tolerance: 0.10,
                ambition: 0.05,
                ..D::default()
            },
            CapabilityKind::Cellular(Cellular::FakeId) => D {
                manipulation: 0.15,
                risk_tolerance: 0.10,
                ..D::default()
            },
            CapabilityKind::Practical(Practical::Trade) => D {
                sociability: 0.10,
                manipulation: 0.05,
                risk_tolerance: 0.02,
                ..D::default()
            },
            CapabilityKind::Practical(Practical::PickLocks) => D {
                risk_tolerance: 0.20,
                discipline: 0.05,
                ..D::default()
            },
            CapabilityKind::Practical(Practical::Repair) => D {
                discipline: 0.15,
                emotional_stability: 0.05,
                risk_tolerance: 0.05,
                ..D::default()
            },
            CapabilityKind::Practical(Practical::Smuggle) => D {
                risk_tolerance: 0.15,
                discipline: -0.05,
                manipulation: 0.05,
                ..D::default()
            },
            CapabilityKind::Practical(Practical::FitLocker) => D {
                discipline: 0.20,
                risk_tolerance: 0.05,
                ..D::default()
            },
            CapabilityKind::Practical(Practical::ScoreKey) => D {
                risk_tolerance: 0.20,
                discipline: 0.05,
                ..D::default()
            },
            CapabilityKind::Social(Social::Persuade) => D {
                sociability: 0.20,
                empathy: 0.10,
                ..D::default()
            },
            CapabilityKind::Social(Social::Charm) => D {
                sociability: 0.25,
                manipulation: 0.10,
                loyalty: 0.05,
                ..D::default()
            },
            CapabilityKind::Social(Social::Intimidate) => D {
                aggression: 0.20,
                risk_tolerance: 0.15,
                ..D::default()
            },
            CapabilityKind::Social(Social::Inform) => D {
                risk_tolerance: 0.10,
                sociability: 0.05,
                ..D::default()
            },
            CapabilityKind::Social(Social::Influence) => D {
                sociability: 0.10,
                manipulation: 0.15,
                ambition: 0.05,
                ..D::default()
            },
            CapabilityKind::Social(Social::Leadership) => D {
                ambition: 0.20,
                loyalty: 0.10,
                ..D::default()
            },
            CapabilityKind::Medical(Medical::FirstAid) => D {
                empathy: 0.20,
                discipline: 0.10,
                emotional_stability: 0.05,
                ..D::default()
            },
            CapabilityKind::Medical(Medical::Evaluation) => D {
                intelligence: 0.15,
                discipline: 0.10,
                risk_tolerance: 0.02,
                ..D::default()
            },
        };
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(
        aggression: f32, risk_tolerance: f32, sociability: f32, manipulation: f32, ambition: f32,
        loyalty: f32, empathy: f32, discipline: f32, intelligence: f32,
        emotional_stability: f32, impulsiveness: f32, risk: f32,
    ) -> CapabilityDeltas {
        CapabilityDeltas {
            aggression, risk_tolerance, sociability, manipulation, ambition, loyalty, empathy,
            discipline, intelligence, emotional_stability, impulsiveness, risk,
        }
    }

    #[test]
    fn capability_modifiers_table() {
        // Physical
        let cap = Capability::new(CapabilityKind::Physical(Physical::Strong));
        assert_eq!(cap.name, "physical_strong");
        assert_eq!(cap.modifiers(&Traits::default()), d(0.15, 0.10, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Physical(Physical::Fit));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.10, 0.0, 0.05, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Physical(Physical::Fast));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.10, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Physical(Physical::Durable));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.0, 0.0, 0.0, 0.0, 0.05, 0.0, 0.20, 0.0, 0.05, 0.0, 0.0));

        // Cellular
        let cap = Capability::new(CapabilityKind::Cellular(Cellular::CellEscape));
        assert_eq!(cap.name, "cell_escape");
        assert_eq!(cap.modifiers(&Traits::default()), d(0.05, 0.30, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Cellular(Cellular::Dig));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.05, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Cellular(Cellular::HideContraband));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.15, 0.0, 0.0, 0.0, 0.0, 0.0, -0.05, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Cellular(Cellular::HoldHideout));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.10, 0.0, 0.0, 0.05, 0.10, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Cellular(Cellular::FakeId));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.10, 0.0, 0.15, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        // Practical
        let cap = Capability::new(CapabilityKind::Practical(Practical::Trade));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.02, 0.10, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Practical(Practical::PickLocks));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.20, 0.0, 0.0, 0.0, 0.0, 0.0, 0.05, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Practical(Practical::Repair));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.15, 0.0, 0.05, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Practical(Practical::Smuggle));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.15, 0.0, 0.05, 0.0, 0.0, 0.0, -0.05, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Practical(Practical::FitLocker));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.20, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Practical(Practical::ScoreKey));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.20, 0.0, 0.0, 0.0, 0.0, 0.0, 0.05, 0.0, 0.0, 0.0, 0.0));

        // Social
        let cap = Capability::new(CapabilityKind::Social(Social::Persuade));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.0, 0.20, 0.10, 0.0, 0.0, 0.10, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Social(Social::Charm));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.0, 0.25, 0.10, 0.0, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Social(Social::Intimidate));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.20, 0.15, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Social(Social::Inform));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.10, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Social(Social::Influence));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.0, 0.10, 0.15, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Social(Social::Leadership));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.0, 0.0, 0.0, 0.20, 0.10, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));

        // Medical
        let cap = Capability::new(CapabilityKind::Medical(Medical::FirstAid));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.20, 0.10, 0.0, 0.05, 0.0, 0.0));

        let cap = Capability::new(CapabilityKind::Medical(Medical::Evaluation));
        assert_eq!(cap.modifiers(&Traits::default()), d(0.0, 0.02, 0.0, 0.0, 0.0, 0.0, 0.0, 0.10, 0.15, 0.0, 0.0, 0.0));
    }

    #[test]
    fn capability_catalog_is_complete_and_deterministic() {
        let all = CapabilityKind::ALL;
        assert_eq!(all.len(), 23);
        let mut names: Vec<&str> = all.iter().map(|k| Capability::new(*k).name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, sorted_unique(names.clone()));
    }

    fn sorted_unique(mut v: Vec<&str>) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        v.retain(|s| seen.insert(*s));
        v.sort_unstable();
        v
    }

    #[test]
    fn negative_deltas_are_clamped_at_apply() {
        // The clamp lives at apply time (Prisoner); here we verify the raw
        // table keeps the negative value so carriers clamp it themselves.
        let d = Capability::new(CapabilityKind::Cellular(Cellular::HideContraband)).modifiers(&Traits::default());
        assert_eq!(d.discipline, -0.05);
    }
}
