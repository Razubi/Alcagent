//! Expansion planning: building catalog and attention-budget mechanics.

use serde::{Deserialize, Serialize};

/// Staffing requirement for a building or project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StaffRequirement {
    pub role: String,
    pub count: u32,
}

impl StaffRequirement {
    pub fn new(role: impl Into<String>, count: u32) -> Self {
        Self {
            role: role.into(),
            count,
        }
    }
}

/// Specification for a buildable/expandable facility type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildingSpec {
    pub key: &'static str,
    pub name: &'static str,
    /// Construction/commissioning cost in minor currency units (cents).
    pub cost_minor: i64,
    /// Build/commission time in days.
    pub build_days: u32,
    /// Capacity delta when operational (beds/seats/slots depending on building).
    pub capacity: u32,
    /// Staffing required to operate effectively.
    pub staff_required: Vec<StaffRequirement>,
    /// Prerequisite building keys or capability tags.
    pub prerequisites: Vec<&'static str>,
}

impl BuildingSpec {
    pub const fn simple(
        key: &'static str,
        name: &'static str,
        cost_minor: i64,
        build_days: u32,
        capacity: u32,
    ) -> Self {
        // Note: const fn with Vec not possible; use helper in catalog builder.
        Self {
            key,
            name,
            cost_minor,
            build_days,
            capacity,
            staff_required: Vec::new(),
            prerequisites: Vec::new(),
        }
    }

    /// Catalog of all twelve directive building types (Directive §12).
    ///
    /// This is the canonical catalog; [`building_catalog`] is kept as a
    /// free-function alias for existing callers.
    pub fn catalog() -> Vec<BuildingSpec> {
        building_catalog()
    }
}

/// Catalog of all directive building types.
pub fn building_catalog() -> Vec<BuildingSpec> {
    vec![
        BuildingSpec {
            key: "cell_block",
            name: "Cell Block",
            cost_minor: 48_000_000, // $480k
            build_days: 120,
            capacity: 48,
            staff_required: vec![StaffRequirement::new("Officer", 4), StaffRequirement::new("Sergeant", 1)],
            prerequisites: vec!["power_headroom", "perimeter_integrity"],
        },
        BuildingSpec {
            key: "dormitory",
            name: "Dormitory",
            cost_minor: 26_000_000,
            build_days: 75,
            capacity: 60,
            staff_required: vec![StaffRequirement::new("Officer", 3)],
            prerequisites: vec!["power_headroom"],
        },
        BuildingSpec {
            key: "medical_facility",
            name: "Medical Facility",
            cost_minor: 52_000_000,
            build_days: 130,
            capacity: 12,
            staff_required: vec![
                StaffRequirement::new("Medical Officer", 2),
                StaffRequirement::new("Senior Officer", 1),
            ],
            prerequisites: vec!["administrative_building", "utilities"],
        },
        BuildingSpec {
            key: "kitchen",
            name: "Kitchen",
            cost_minor: 24_000_000,
            build_days: 60,
            capacity: 120,
            staff_required: vec![StaffRequirement::new("Cook", 3), StaffRequirement::new("Officer", 1)],
            prerequisites: vec!["water", "power_headroom"],
        },
        BuildingSpec {
            key: "workshop",
            name: "Workshop",
            cost_minor: 31_000_000,
            build_days: 80,
            capacity: 30,
            staff_required: vec![StaffRequirement::new("Instructor", 1), StaffRequirement::new("Officer", 2)],
            prerequisites: vec!["kitchen", "power_headroom"],
        },
        BuildingSpec {
            key: "recreation_area",
            name: "Recreation Area",
            cost_minor: 18_000_000,
            build_days: 45,
            capacity: 80,
            staff_required: vec![StaffRequirement::new("Officer", 1)],
            prerequisites: vec!["yard_or_space"],
        },
        BuildingSpec {
            key: "education_building",
            name: "Education Building",
            cost_minor: 39_000_000,
            build_days: 100,
            capacity: 24,
            staff_required: vec![StaffRequirement::new("Instructor", 2), StaffRequirement::new("Officer", 1)],
            prerequisites: vec!["administrative_building"],
        },
        BuildingSpec {
            key: "administrative_building",
            name: "Administrative Building",
            cost_minor: 44_000_000,
            build_days: 110,
            capacity: 0,
            staff_required: vec![StaffRequirement::new("Administrator", 2), StaffRequirement::new("Officer", 1)],
            prerequisites: vec!["power_headroom"],
        },
        BuildingSpec {
            key: "isolation_facility",
            name: "Isolation Facility",
            cost_minor: 50_000_000,
            build_days: 110,
            capacity: 12,
            staff_required: vec![StaffRequirement::new("Officer", 3), StaffRequirement::new("Sergeant", 1)],
            prerequisites: vec!["cell_block", "perimeter_integrity"],
        },
        BuildingSpec {
            key: "visitor_facilities",
            name: "Visitor Facilities",
            cost_minor: 21_000_000,
            build_days: 55,
            capacity: 20,
            staff_required: vec![StaffRequirement::new("Officer", 2)],
            prerequisites: vec!["reception", "screening"],
        },
        BuildingSpec {
            key: "staff_facilities",
            name: "Staff Facilities",
            cost_minor: 28_000_000,
            build_days: 70,
            capacity: 0,
            staff_required: vec![StaffRequirement::new("Administrator", 1)],
            prerequisites: vec!["utilities"],
        },
        BuildingSpec {
            key: "yard_expansion",
            name: "Yard Expansion",
            cost_minor: 15_000_000,
            build_days: 40,
            capacity: 100,
            staff_required: vec![StaffRequirement::new("Officer", 1)],
            prerequisites: vec!["perimeter_integrity"],
        },
    ]
}

/// Lookup a building spec by key.
pub fn find_building_spec(key: &str) -> Option<BuildingSpec> {
    building_catalog().into_iter().find(|s| s.key == key)
}

/// Administrative attention budget mechanics for the warden/institution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionBudget {
    /// Current available points.
    pub points: f32,
    /// Maximum storable points after refill.
    pub max_points: f32,
    /// Daily refill baseline before modifiers.
    pub base_refill: f32,
    /// Competence modifier contribution factor (0..1 scale input expected by caller).
    pub competence_gain: f32,
    /// Per-active-construction-site daily drain.
    pub per_site_drain: f32,
}

impl Default for AttentionBudget {
    fn default() -> Self {
        Self {
            points: 100.0,
            max_points: 120.0,
            base_refill: 100.0,
            competence_gain: 0.6,
            per_site_drain: 2.0,
        }
    }
}

impl AttentionBudget {
    pub fn new(points: f32) -> Self {
        let mut b = Self::default();
        b.points = points.clamp(0.0, b.max_points);
        b
    }

    /// Refill points for a new day, accounting for competence (0..1) and active construction sites.
    pub fn refill_daily(&mut self, competence_0_to_1: f32, active_construction_sites: u32) {
        let comp = competence_0_to_1.clamp(0.0, 1.0);
        let refill = self.base_refill * (0.7 + self.competence_gain * comp);
        let drain = self.per_site_drain * active_construction_sites as f32;
        self.points = (self.points + refill - drain).clamp(0.0, self.max_points);
    }

    pub fn can_afford(&self, cost: f32) -> bool {
        self.points + f32::EPSILON >= cost.max(0.0)
    }

    /// Spend attention if available; returns true on success.
    pub fn spend(&mut self, cost: f32) -> bool {
        let c = cost.max(0.0);
        if self.can_afford(c) {
            self.points = (self.points - c).max(0.0);
            true
        } else {
            false
        }
    }

    pub fn reserve(&mut self, minimum: f32) {
        if self.points < minimum {
            self.points = 0.0;
        }
    }

    /// Maximum number of concurrent decisions of `cost` points the budget can afford.
    pub fn max_concurrent_decisions(&self, cost: f32) -> u32 {
        let c = cost.max(f32::EPSILON);
        (self.points / c).floor().max(0.0) as u32
    }

    /// Suggested costs for common decision classes.
    pub fn tactical_cost() -> f32 { 2.0 }
    pub fn strategic_cost() -> f32 { 12.0 }
    pub fn crisis_cost() -> f32 { 20.0 }
    pub fn investigation_weekly_cost() -> f32 { 4.0 }
}
