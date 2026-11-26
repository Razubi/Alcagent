//! Shared presentation primitives for the Alcagent frontends.
//!
//! This module is the graphics seam owned by the engine-facing crate: it holds
//! the canonical palette, view model, world-map layout, and camera math used by
//! every observer. Frontends may choose their own widgets, but they must not
//! invent their own colors, view ordering, or map geometry. Keeping those
//! decisions here gives both surfaces the same visual language and makes layout
//! changes testable without a window or a terminal.
//!
//! The module is deliberately toolkit-neutral: it does not depend on egui,
//! ratatui, or any widget library. Frontends convert [`Color`] and the simple
//! coordinate tuples into their native drawing types.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

use crate::map_frame::NavEdge;

/// Facility kinds offered by observer construction controls, in canonical order.
pub const CONSTRUCTION_KINDS: [alc_world::FacilityKind; 12] = [
    alc_world::FacilityKind::CellBlock,
    alc_world::FacilityKind::Dormitory,
    alc_world::FacilityKind::Medical,
    alc_world::FacilityKind::Education,
    alc_world::FacilityKind::Isolation,
    alc_world::FacilityKind::Visitor,
    alc_world::FacilityKind::Workshop,
    alc_world::FacilityKind::Kitchen,
    alc_world::FacilityKind::Recreation,
    alc_world::FacilityKind::Admin,
    alc_world::FacilityKind::Staff,
    alc_world::FacilityKind::Yard,
];

/// Stable single-character marker for a facility kind.
///
/// The TUI uses this as a map glyph; the GUI may use it in legends and labels.
#[must_use]
pub const fn facility_glyph(kind: alc_world::FacilityKind) -> char {
    use alc_world::FacilityKind;
    match kind {
        FacilityKind::CellBlock => 'C',
        FacilityKind::Dormitory => 'D',
        FacilityKind::Medical => 'M',
        FacilityKind::Kitchen => 'K',
        FacilityKind::Workshop => 'W',
        FacilityKind::Recreation => 'R',
        FacilityKind::Education => 'E',
        FacilityKind::Admin => 'A',
        FacilityKind::Isolation => 'I',
        FacilityKind::Visitor => 'V',
        FacilityKind::Staff => 'S',
        FacilityKind::Yard => 'Y',
    }
}

/// An RGBA color value shared by all frontends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel; 255 is fully opaque.
    pub a: u8,
}

impl Color {
    /// Create an opaque color.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Create a color with an explicit alpha channel.
    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// Canonical observation views, shared by every frontend.
///
/// The order is the navigation order used by menus, tab bars, and sidebars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    /// Institution-level dashboard.
    InstitutionOverview,
    /// Prisoner roster and profiles.
    Prisoner,
    /// Gang roster and politics.
    Gang,
    /// Guard and staff roster.
    Staff,
    /// Warden decisions and objectives.
    Warden,
    /// Budget and ledger.
    Financial,
    /// World event timeline.
    EventTimeline,
    /// Social role and relationship network.
    SocialNetwork,
    /// Spatial world map.
    Map,
    /// Structured incident reports.
    IncidentReports,
    /// Decision audit trail.
    DecisionAudit,
}

impl ViewMode {
    /// All views in canonical navigation order.
    #[must_use]
    pub const fn all() -> [Self; 11] {
        [
            Self::InstitutionOverview,
            Self::Prisoner,
            Self::Gang,
            Self::Staff,
            Self::Warden,
            Self::Financial,
            Self::EventTimeline,
            Self::SocialNetwork,
            Self::Map,
            Self::IncidentReports,
            Self::DecisionAudit,
        ]
    }

    /// Short label used by compact navigation controls.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::InstitutionOverview => "Overview",
            Self::Prisoner => "Prisoners",
            Self::Gang => "Gangs",
            Self::Staff => "Staff",
            Self::Warden => "Warden",
            Self::Financial => "Financial",
            Self::EventTimeline => "Timeline",
            Self::SocialNetwork => "Network",
            Self::Map => "Map",
            Self::IncidentReports => "Incidents",
            Self::DecisionAudit => "Audit",
        }
    }

    /// Longer label used by headings and menus.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::InstitutionOverview => "Institution Overview",
            Self::Prisoner => "Prisoners",
            Self::Gang => "Gangs",
            Self::Staff => "Staff",
            Self::Warden => "Warden",
            Self::Financial => "Financial",
            Self::EventTimeline => "Event Timeline",
            Self::SocialNetwork => "Social Network",
            Self::Map => "World Map",
            Self::IncidentReports => "Incident Reports",
            Self::DecisionAudit => "Decision Audit",
        }
    }
}

impl Default for ViewMode {
    fn default() -> Self {
        Self::InstitutionOverview
    }
}

/// Visual theme selected in the settings screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeKind {
    /// Dark blue-gray institutional theme.
    #[default]
    Slate,
    /// Higher-contrast near-black theme.
    Midnight,
    /// Light paper theme for bright rooms.
    Paper,
}

impl ThemeKind {
    /// Stable label for the settings screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Slate => "Slate",
            Self::Midnight => "Midnight",
            Self::Paper => "Paper",
        }
    }

    /// All selectable themes.
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::Slate, Self::Midnight, Self::Paper]
    }
}

/// Canonical colors for every frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Main canvas background.
    pub background: Color,
    /// Panel and card background.
    pub panel: Color,
    /// Primary text color.
    pub text: Color,
    /// Secondary text color.
    pub muted: Color,
    /// Selection and action color.
    pub accent: Color,
    /// Positive state color.
    pub good: Color,
    /// Caution state color.
    pub warning: Color,
    /// Danger state color.
    pub danger: Color,
    /// Prisoner pawn color.
    pub prisoner: Color,
    /// Guard pawn color.
    pub guard: Color,
    /// Active facility color.
    pub facility: Color,
    /// Locked facility color.
    pub locked: Color,
    /// Road / navigation edge color.
    pub road: Color,
    /// Map terrain color.
    pub terrain: Color,
    /// Map water color.
    pub water: Color,
    /// Map zone fill color.
    pub zone: Color,
}

impl Palette {
    /// Resolve a concrete palette for `theme`.
    #[must_use]
    pub fn for_theme(theme: ThemeKind) -> Self {
        match theme {
            ThemeKind::Slate => Self {
                background: Color::rgb(14, 18, 24),
                panel: Color::rgb(24, 31, 40),
                text: Color::rgb(235, 241, 248),
                muted: Color::rgb(150, 163, 180),
                accent: Color::rgb(91, 200, 255),
                good: Color::rgb(120, 220, 150),
                warning: Color::rgb(255, 190, 80),
                danger: Color::rgb(255, 96, 96),
                prisoner: Color::rgb(255, 176, 92),
                guard: Color::rgb(120, 200, 255),
                facility: Color::rgb(120, 230, 170),
                locked: Color::rgb(255, 96, 96),
                road: Color::rgb(130, 150, 170),
                terrain: Color::rgb(25, 34, 39),
                water: Color::rgb(28, 58, 78),
                zone: Color::rgba(66, 88, 104, 90),
            },
            ThemeKind::Midnight => Self {
                background: Color::rgb(8, 10, 14),
                panel: Color::rgb(17, 21, 28),
                text: Color::rgb(240, 244, 250),
                muted: Color::rgb(132, 145, 163),
                accent: Color::rgb(104, 190, 255),
                good: Color::rgb(110, 220, 150),
                warning: Color::rgb(255, 184, 72),
                danger: Color::rgb(255, 84, 84),
                prisoner: Color::rgb(255, 168, 80),
                guard: Color::rgb(104, 190, 255),
                facility: Color::rgb(112, 224, 168),
                locked: Color::rgb(255, 84, 84),
                road: Color::rgb(118, 138, 158),
                terrain: Color::rgb(16, 24, 29),
                water: Color::rgb(20, 48, 68),
                zone: Color::rgba(52, 76, 94, 80),
            },
            ThemeKind::Paper => Self {
                background: Color::rgb(238, 240, 242),
                panel: Color::rgb(250, 250, 248),
                text: Color::rgb(28, 34, 40),
                muted: Color::rgb(96, 106, 116),
                accent: Color::rgb(24, 112, 168),
                good: Color::rgb(32, 132, 88),
                warning: Color::rgb(176, 112, 24),
                danger: Color::rgb(188, 52, 52),
                prisoner: Color::rgb(190, 104, 32),
                guard: Color::rgb(32, 104, 168),
                facility: Color::rgb(40, 140, 104),
                locked: Color::rgb(188, 52, 52),
                road: Color::rgb(124, 134, 142),
                terrain: Color::rgb(222, 226, 224),
                water: Color::rgb(188, 214, 228),
                zone: Color::rgba(150, 170, 180, 60),
            },
        }
    }

    /// Color for a 0..1 severity value: good → warning → danger.
    #[must_use]
    pub fn severity(self, value: f32) -> Color {
        let value = value.clamp(0.0, 1.0);
        if value >= 0.75 {
            self.danger
        } else if value >= 0.4 {
            self.warning
        } else {
            self.good
        }
    }

    /// Color for a 0..1 metric where higher is better.
    #[must_use]
    pub fn quality(self, value: f32) -> Color {
        self.severity(1.0 - value.clamp(0.0, 1.0))
    }

    /// Color for a validation result string.
    #[must_use]
    pub fn validation(self, value: &str) -> Color {
        let value = value.to_ascii_lowercase();
        if value.starts_with("accepted") {
            self.good
        } else if value.starts_with("rejected") {
            self.danger
        } else if value.starts_with("repaired") || value.starts_with("proposed") {
            self.warning
        } else {
            self.muted
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::for_theme(ThemeKind::default())
    }
}

/// Logical region of the institutional world map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MapZoneKind {
    /// Gatehouse, administration, and visitation.
    Administration,
    /// Housing units.
    Housing,
    /// Medical, education, kitchen, and workshops.
    Services,
    /// Yards and recreation.
    Recreation,
    /// Staff quarters and support.
    Support,
}

impl MapZoneKind {
    /// Display name of the region.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Administration => "ENTRY & ADMINISTRATION",
            Self::Housing => "HOUSING",
            Self::Services => "SERVICES",
            Self::Recreation => "RECREATION",
            Self::Support => "STAFF SUPPORT",
        }
    }

    /// Stable sort rank for deterministic layout.
    const fn rank(self) -> u8 {
        match self {
            Self::Administration => 0,
            Self::Housing => 1,
            Self::Services => 2,
            Self::Recreation => 3,
            Self::Support => 4,
        }
    }
}

/// A rectangular region drawn on the world map.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MapZone {
    /// Region classification.
    pub kind: MapZoneKind,
    /// Left edge in world coordinates.
    pub x: f32,
    /// Top edge in world coordinates.
    pub y: f32,
    /// Region width.
    pub width: f32,
    /// Region height.
    pub height: f32,
}

/// A rectangular world-space area used by camera fitting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapBounds {
    /// Minimum x coordinate.
    pub min_x: f32,
    /// Minimum y coordinate.
    pub min_y: f32,
    /// Maximum x coordinate.
    pub max_x: f32,
    /// Maximum y coordinate.
    pub max_y: f32,
}

impl MapBounds {
    /// Bounds covering the canonical world map.
    #[must_use]
    pub fn from_layout(layout: &WorldMapLayout) -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            max_x: layout.width,
            max_y: layout.height,
        }
    }

    /// Width and height of the bounds.
    #[must_use]
    pub fn size(self) -> (f32, f32) {
        (self.max_x - self.min_x, self.max_y - self.min_y)
    }

    /// Center point of the bounds.
    #[must_use]
    pub fn center(self) -> (f32, f32) {
        (
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
        )
    }
}

/// Water strip along the east edge of the canonical world map.
#[must_use]
pub const fn canal_bounds(bounds: MapBounds) -> MapBounds {
    MapBounds {
        min_x: bounds.max_x - 90.0,
        min_y: bounds.min_y,
        max_x: bounds.max_x,
        max_y: bounds.max_y,
    }
}

/// Deterministic institutional world-map geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldMapLayout {
    /// Canonical world width.
    pub width: f32,
    /// Canonical world height.
    pub height: f32,
    /// Regions drawn beneath facilities.
    pub zones: Vec<MapZone>,
    /// Facility id → center coordinate.
    pub positions: HashMap<u64, (f32, f32)>,
    /// Schematic road network connecting facilities.
    pub edges: Vec<NavEdge>,
}

impl WorldMapLayout {
    /// Build the canonical layout for `facilities`.
    ///
    /// Positions are grouped by facility purpose and sorted by stable kind/id
    /// rank, so the same facility set always produces the same map regardless
    /// of insertion order.
    #[must_use]
    pub fn new(facilities: &[(u64, alc_world::FacilityKind)]) -> Self {
        let zones = vec![
            MapZone {
                kind: MapZoneKind::Administration,
                x: 60.0,
                y: 40.0,
                width: 880.0,
                height: 110.0,
            },
            MapZone {
                kind: MapZoneKind::Housing,
                x: 60.0,
                y: 180.0,
                width: 380.0,
                height: 400.0,
            },
            MapZone {
                kind: MapZoneKind::Services,
                x: 470.0,
                y: 180.0,
                width: 470.0,
                height: 280.0,
            },
            MapZone {
                kind: MapZoneKind::Recreation,
                x: 470.0,
                y: 490.0,
                width: 280.0,
                height: 170.0,
            },
            MapZone {
                kind: MapZoneKind::Support,
                x: 780.0,
                y: 490.0,
                width: 160.0,
                height: 170.0,
            },
        ];

        let mut sorted = facilities.to_vec();
        sorted.sort_by_key(|(id, kind)| (zone_for_kind(*kind).rank(), kind_rank(*kind), *id));

        let mut positions = HashMap::new();
        let mut grouped: HashMap<MapZoneKind, Vec<u64>> = HashMap::new();
        let mut counts: HashMap<MapZoneKind, usize> = HashMap::new();
        for (id, kind) in &sorted {
            let zone = zone_for_kind(*kind);
            let idx = counts.entry(zone).or_default();
            let cols = zone_columns(zone);
            let col = (*idx % cols) as f32;
            let row = (*idx / cols) as f32;
            let zone_rect = zones
                .iter()
                .find(|z| z.kind == zone)
                .expect("every zone kind has a rect");
            let step_x = (zone_rect.width - 140.0) / (cols.saturating_sub(1).max(1)) as f32;
            let step_y = if zone == MapZoneKind::Administration {
                0.0
            } else {
                110.0
            };
            positions.insert(
                *id,
                (
                    zone_rect.x + 70.0 + col * step_x,
                    zone_rect.y + 58.0 + row * step_y,
                ),
            );
            grouped.entry(zone).or_default().push(*id);
            *idx += 1;
        }

        let edges = build_edges(&grouped, &sorted);
        Self {
            width: 1000.0,
            height: 700.0,
            zones,
            positions,
            edges,
        }
    }
}

fn zone_for_kind(kind: alc_world::FacilityKind) -> MapZoneKind {
    use alc_world::FacilityKind;
    match kind {
        FacilityKind::Admin | FacilityKind::Visitor => MapZoneKind::Administration,
        FacilityKind::CellBlock | FacilityKind::Dormitory | FacilityKind::Isolation => {
            MapZoneKind::Housing
        }
        FacilityKind::Medical
        | FacilityKind::Kitchen
        | FacilityKind::Workshop
        | FacilityKind::Education => MapZoneKind::Services,
        FacilityKind::Yard | FacilityKind::Recreation => MapZoneKind::Recreation,
        FacilityKind::Staff => MapZoneKind::Support,
    }
}

const fn zone_columns(zone: MapZoneKind) -> usize {
    match zone {
        MapZoneKind::Administration => 3,
        MapZoneKind::Housing => 2,
        MapZoneKind::Services => 3,
        MapZoneKind::Recreation => 2,
        MapZoneKind::Support => 1,
    }
}

const fn kind_rank(kind: alc_world::FacilityKind) -> u8 {
    use alc_world::FacilityKind;
    match kind {
        FacilityKind::Admin => 0,
        FacilityKind::Visitor => 1,
        FacilityKind::CellBlock => 2,
        FacilityKind::Dormitory => 3,
        FacilityKind::Isolation => 4,
        FacilityKind::Medical => 5,
        FacilityKind::Kitchen => 6,
        FacilityKind::Workshop => 7,
        FacilityKind::Education => 8,
        FacilityKind::Yard => 9,
        FacilityKind::Recreation => 10,
        FacilityKind::Staff => 11,
    }
}

fn build_edges(
    grouped: &HashMap<MapZoneKind, Vec<u64>>,
    sorted: &[(u64, alc_world::FacilityKind)],
) -> Vec<NavEdge> {
    let mut edges = Vec::new();
    let mut seen = BTreeSet::new();

    // Local roads: a loop inside each region. Isolation has restricted access.
    let mut zones: Vec<MapZoneKind> = grouped.keys().copied().collect();
    zones.sort_by_key(|zone| zone.rank());
    for zone in zones {
        let Some(ids) = grouped.get(&zone) else {
            continue;
        };
        if ids.len() == 1 {
            continue;
        }
        for (idx, from) in ids.iter().enumerate() {
            let to = ids[(idx + 1) % ids.len()];
            push_edge(
                &mut edges,
                &mut seen,
                *from,
                to,
                zone == MapZoneKind::Housing && *from >= 9000,
            );
        }
    }

    // Cross-region roads connect each region to the entry/admin anchor when it
    // exists, otherwise to the first facility in the earliest region.
    let anchor = grouped
        .get(&MapZoneKind::Administration)
        .and_then(|ids| ids.first().copied())
        .or_else(|| sorted.first().map(|(id, _)| *id));
    if let Some(anchor) = anchor {
        for zone in [
            MapZoneKind::Housing,
            MapZoneKind::Services,
            MapZoneKind::Recreation,
            MapZoneKind::Support,
        ] {
            if let Some(first) = grouped.get(&zone).and_then(|ids| ids.first()).copied() {
                push_edge(
                    &mut edges,
                    &mut seen,
                    anchor,
                    first,
                    zone == MapZoneKind::Support,
                );
                push_edge(
                    &mut edges,
                    &mut seen,
                    first,
                    anchor,
                    zone == MapZoneKind::Support,
                );
            }
        }
    }

    // Every facility must have at least one outgoing road even in sparse worlds.
    for (id, _) in sorted {
        if !edges.iter().any(|edge| edge.from == *id) {
            let target = sorted
                .iter()
                .find(|(other, _)| other != id)
                .map(|(other, _)| *other);
            if let Some(target) = target {
                push_edge(&mut edges, &mut seen, *id, target, false);
            }
        }
    }

    edges
}

fn push_edge(
    edges: &mut Vec<NavEdge>,
    seen: &mut BTreeSet<(u64, u64)>,
    from: u64,
    to: u64,
    restricted: bool,
) {
    if from == to || !seen.insert((from, to)) {
        return;
    }
    edges.push(NavEdge {
        from,
        to,
        sightline: !restricted,
        restricted,
    });
}

/// Camera transform for a map canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapCamera {
    /// Zoom factor.
    pub zoom: f32,
    /// Canvas-local pan offset as `(x, y)`.
    pub pan: (f32, f32),
}

impl MapCamera {
    /// Create a unit camera.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            zoom: 1.0,
            pan: (0.0, 0.0),
        }
    }

    /// Fit `bounds` into `viewport` with `padding`, returning a new camera.
    #[must_use]
    pub fn fit(bounds: MapBounds, viewport: (f32, f32), padding: f32) -> Self {
        let (width, height) = bounds.size();
        let width = width.max(1.0);
        let height = height.max(1.0);
        let available_x = (viewport.0 - padding * 2.0).max(1.0);
        let available_y = (viewport.1 - padding * 2.0).max(1.0);
        let zoom = (available_x / width)
            .min(available_y / height)
            .clamp(0.2, 4.0);
        let (cx, cy) = bounds.center();
        Self {
            zoom,
            pan: (viewport.0 * 0.5 - cx * zoom, viewport.1 * 0.5 - cy * zoom),
        }
    }

    /// Project a world point into canvas-local coordinates.
    #[must_use]
    pub fn project(self, world: (f32, f32)) -> (f32, f32) {
        (
            world.0 * self.zoom + self.pan.0,
            world.1 * self.zoom + self.pan.1,
        )
    }

    /// Invert [`Self::project`].
    #[must_use]
    pub fn unproject(self, screen: (f32, f32)) -> (f32, f32) {
        (
            (screen.0 - self.pan.0) / self.zoom,
            (screen.1 - self.pan.1) / self.zoom,
        )
    }

    /// Zoom by `factor` while keeping the world point under `anchor` fixed.
    #[must_use]
    pub fn zoom_at(self, anchor: (f32, f32), factor: f32, min: f32, max: f32) -> Self {
        let world = self.unproject(anchor);
        let zoom = (self.zoom * factor).clamp(min, max);
        Self {
            zoom,
            pan: (anchor.0 - world.0 * zoom, anchor.1 - world.1 * zoom),
        }
    }
}

impl Default for MapCamera {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alc_world::FacilityKind;

    #[test]
    fn layout_is_stable_for_same_facility_set() {
        let input = vec![
            (3, FacilityKind::CellBlock),
            (7, FacilityKind::Medical),
            (2, FacilityKind::Yard),
        ];
        let a = WorldMapLayout::new(&input);
        let b = WorldMapLayout::new(&input);
        assert_eq!(a, b);
        assert_eq!(a.positions.len(), 3);
        assert!(a.edges.len() >= input.len());
    }

    #[test]
    fn layout_groups_facilities_into_regions() {
        let input = vec![
            (1, FacilityKind::CellBlock),
            (2, FacilityKind::Medical),
            (3, FacilityKind::Yard),
            (4, FacilityKind::Admin),
        ];
        let layout = WorldMapLayout::new(&input);
        let cell = layout.positions[&1];
        let medical = layout.positions[&2];
        let yard = layout.positions[&3];
        let admin = layout.positions[&4];
        assert!(admin.1 < cell.1);
        assert!(cell.0 < medical.0);
        assert!(yard.1 > medical.1);
    }

    #[test]
    fn camera_fit_and_zoom_keep_anchor_fixed() {
        let layout = WorldMapLayout::new(&[(1, FacilityKind::Yard)]);
        let camera = MapCamera::fit(MapBounds::from_layout(&layout), (800.0, 600.0), 20.0);
        let anchor = (300.0, 240.0);
        let world = camera.unproject(anchor);
        let zoomed = camera.zoom_at(anchor, 1.5, 0.2, 4.0);
        let projected = zoomed.project(world);
        assert!((projected.0 - anchor.0).abs() < 0.001);
        assert!((projected.1 - anchor.1).abs() < 0.001);
    }

    #[test]
    fn shared_view_labels_cover_eleven_views() {
        assert_eq!(ViewMode::all().len(), 11);
        assert_eq!(ViewMode::Map.title(), "World Map");
    }
}
