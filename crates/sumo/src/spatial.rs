//! Spatial indexing, level-of-detail simulation, and procedural city hooks
//! (Spec 009). Everything here is engine-owned: frontends only read DTOs.
//!
//! The prison map is a coarse grid of named facilities. Each facility is one
//! cell, which makes co-presence lookups O(1) per cell instead of O(n²) over
//! every prisoner pair. City districts reuse the same grid abstraction, so the
//! prison can later become one district of a larger procedural world.

use std::collections::{BTreeMap, BTreeSet};

/// Stable cell identifier shared by prison facilities and city blocks.
pub type CellId = u64;

/// Coarse spatial index mapping agents to cells.
#[derive(Debug, Clone, Default)]
pub struct SpatialGrid {
    /// Agents in each cell, keyed by cell id.
    cells: BTreeMap<CellId, BTreeSet<u64>>,
    /// Current cell of each indexed agent.
    locations: BTreeMap<u64, CellId>,
}

impl SpatialGrid {
    /// Insert or move an agent into a cell. Re-inserting into the same cell is
    /// a no-op; moving updates both indexes atomically.
    pub fn insert(&mut self, agent: u64, cell: CellId) {
        if let Some(old) = self.locations.get(&agent).copied() {
            if old == cell {
                return;
            }
            if let Some(occupants) = self.cells.get_mut(&old) {
                occupants.remove(&agent);
                if occupants.is_empty() {
                    self.cells.remove(&old);
                }
            }
        }
        self.locations.insert(agent, cell);
        self.cells.entry(cell).or_default().insert(agent);
    }

    /// Remove an agent from the grid.
    pub fn remove(&mut self, agent: u64) -> bool {
        let Some(cell) = self.locations.remove(&agent) else {
            return false;
        };
        if let Some(occupants) = self.cells.get_mut(&cell) {
            occupants.remove(&agent);
            if occupants.is_empty() {
                self.cells.remove(&cell);
            }
        }
        true
    }

    /// Occupants of a cell in ascending id order (deterministic).
    #[must_use]
    pub fn occupants(&self, cell: CellId) -> &BTreeSet<u64> {
        static EMPTY: BTreeSet<u64> = BTreeSet::new();
        self.cells.get(&cell).unwrap_or(&EMPTY)
    }

    /// Cell containing an agent, if present.
    #[must_use]
    pub fn cell_of(&self, agent: u64) -> Option<CellId> {
        self.locations.get(&agent).copied()
    }

    /// Enumerate co-present pairs in deterministic order. This replaces the
    /// previous nested scan over every prisoner and is O(n) in cell occupancy.
    #[must_use]
    pub fn co_present_pairs(&self) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        for occupants in self.cells.values() {
            let ids: Vec<u64> = occupants.iter().copied().collect();
            for i in 0..ids.len() {
                for j in (i + 1)..ids.len() {
                    out.push((ids[i], ids[j]));
                }
            }
        }
        out
    }

    /// Total agents indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.locations.len()
    }

    /// Whether no agents are indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locations.is_empty()
    }
}

/// Simulation fidelity level assigned to an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LodLevel {
    /// Full needs, decisions, encounters and social updates (prison interior).
    Full,
    /// Needs drift and coarse decisions only (prison exterior).
    Reduced,
    /// Needs drift only; no encounters or decisions (distant city).
    Minimal,
}

impl LodLevel {
    /// Short stable label for observers.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Reduced => "reduced",
            Self::Minimal => "minimal",
        }
    }

    /// Assign a fidelity level from distance to the prison centre in cells.
    #[must_use]
    pub fn for_distance(distance: u32) -> Self {
        match distance {
            0..=3 => Self::Full,
            4..=12 => Self::Reduced,
            _ => Self::Minimal,
        }
    }

    /// Whether this agent participates in co-presence encounters.
    #[must_use]
    pub fn has_encounters(self) -> bool {
        self == Self::Full
    }

    /// Whether this agent makes autonomous decisions.
    #[must_use]
    pub fn makes_decisions(self) -> bool {
        matches!(self, Self::Full | Self::Reduced)
    }
}

/// Kind of a procedurally generated city district.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DistrictKind {
    /// Housing and family life.
    Residential,
    /// Trade, services and entertainment.
    Commercial,
    /// Workshops, depots and factories.
    Industrial,
    /// Courts, administration and the prison itself.
    Government,
}

impl DistrictKind {
    /// Short stable label for observers.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Residential => "residential",
            Self::Commercial => "commercial",
            Self::Industrial => "industrial",
            Self::Government => "government",
        }
    }
}

/// One generated city district.
#[derive(Debug, Clone, PartialEq)]
pub struct District {
    /// Stable district cell id.
    pub id: CellId,
    /// Functional kind of the district.
    pub kind: DistrictKind,
    /// Map coordinates of the district centre.
    pub centre: (i32, i32),
    /// Cell ids of buildings placed in this district.
    pub buildings: Vec<CellId>,
    /// Seeded resident population (may be streamed lazily).
    pub population: u32,
}

/// Procedurally generated city layout.
#[derive(Debug, Clone, PartialEq)]
pub struct CityLayout {
    /// Generated districts in stable order.
    pub districts: Vec<District>,
    /// Road segments as pairs of cell ids.
    pub roads: Vec<(CellId, CellId)>,
    /// Allocator cursor for future cells.
    pub next_cell_id: CellId,
}

/// Pure, deterministic hook for generating the surrounding city.
///
/// Implementations must be functions of the seed only so saves and traces
/// remain reproducible.
pub trait CityGenerator {
    /// Generate a city layout around `prison_centre`.
    fn generate(&self, seed: u64, prison_centre: (i32, i32)) -> CityLayout;
}

/// Default concentric-grid city generator.
#[derive(Debug, Clone, Copy, Default)]
pub struct GridCityGenerator;

impl CityGenerator for GridCityGenerator {
    fn generate(&self, _seed: u64, prison_centre: (i32, i32)) -> CityLayout {
        let mut next = 1;
        let mut districts = Vec::new();
        for (offset, kind) in [
            (0, DistrictKind::Government),
            (6, DistrictKind::Commercial),
            (12, DistrictKind::Residential),
            (18, DistrictKind::Industrial),
        ] {
            let id = next;
            next += 1;
            let buildings = (0..4)
                .map(|_| {
                    let cell = next;
                    next += 1;
                    cell
                })
                .collect();
            districts.push(District {
                id,
                kind,
                centre: (
                    prison_centre.0.saturating_add(offset),
                    prison_centre.1.saturating_add(offset),
                ),
                buildings,
                population: match kind {
                    DistrictKind::Government => 40,
                    DistrictKind::Commercial => 160,
                    DistrictKind::Residential => 600,
                    DistrictKind::Industrial => 200,
                },
            });
        }
        let mut roads = Vec::new();
        for pair in districts.windows(2) {
            roads.push((pair[0].id, pair[1].id));
        }
        CityLayout {
            districts,
            roads,
            next_cell_id: next,
        }
    }
}

/// Per-tick profiling data exposed through [`crate::Sim::profiling`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProfilingSnapshot {
    /// Operations charged in the most recent tick.
    pub tick_ops_used: u32,
    /// Configured maximum operations per tick.
    pub tick_ops_budget: u32,
    /// Total items deferred by the tick budget guard.
    pub tick_deferrals: u64,
    /// Number of agents present in the spatial index.
    pub agents_indexed: usize,
    /// Agents simulated at full fidelity.
    pub agents_full: usize,
    /// Agents simulated at reduced fidelity.
    pub agents_reduced: usize,
    /// Agents simulated at minimal fidelity.
    pub agents_minimal: usize,
    /// Number of generated city districts.
    pub city_districts: usize,
    /// Number of generated city buildings.
    pub city_buildings: usize,
    /// Total seeded city population.
    pub city_population_seeded: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_pairs_are_deterministic_and_area_scoped() {
        let mut grid = SpatialGrid::default();
        grid.insert(1, 10);
        grid.insert(2, 10);
        grid.insert(3, 10);
        grid.insert(4, 11);
        assert_eq!(grid.occupants(10).len(), 3);
        assert_eq!(grid.co_present_pairs(), vec![(1, 2), (1, 3), (2, 3)]);
        grid.insert(3, 11);
        assert_eq!(grid.co_present_pairs(), vec![(1, 2), (3, 4)]);
        assert!(grid.remove(1));
        assert_eq!(grid.cell_of(1), None);
        assert_eq!(grid.co_present_pairs(), vec![(3, 4)]);
    }

    #[test]
    fn lod_levels_follow_distance() {
        assert_eq!(LodLevel::for_distance(0), LodLevel::Full);
        assert_eq!(LodLevel::for_distance(6), LodLevel::Reduced);
        assert_eq!(LodLevel::for_distance(30), LodLevel::Minimal);
        assert!(LodLevel::Full.has_encounters());
        assert!(!LodLevel::Minimal.makes_decisions());
    }

    #[test]
    fn city_generation_is_deterministic() {
        let a = GridCityGenerator.generate(42, (0, 0));
        let b = GridCityGenerator.generate(42, (0, 0));
        assert_eq!(a, b);
        assert_eq!(a.districts.len(), 4);
        assert_eq!(a.districts[0].kind, DistrictKind::Government);
        assert_eq!(a.roads.len(), 3);
        assert!(a.next_cell_id > a.districts.last().unwrap().buildings[3]);
        assert_eq!(
            a.districts.iter().map(|d| d.population).sum::<u32>(),
            1_000
        );
    }
}
