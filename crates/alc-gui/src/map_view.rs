//! Institutional world map built from the shared `sumo::MapFrame`.
//!
//! Regions, roads and canonical world size come straight from the engine's
//! [`sumo::graphics::WorldMapLayout`]-derived frame; the GUI only converts
//! those toolkit-neutral values into egui painting. Facility footprints, pawn
//! dots and incident badges sit on top. The camera is the shared
//! [`sumo::MapCamera`]; every camera operation also has an explicit button, so
//! mouse drag/scroll is only a convenience and never the only control path.

use crate::{data, panels, settings, theme};
use eframe::egui::{self, Align2, FontId, Rect, Shape, Stroke, StrokeKind, Vec2};
use std::collections::HashMap;
use sumo::{MapCamera, Palette, ThemeKind, graphics};

/// Maximum number of names drawn in a facility roster. The rest are summarised
/// so a crowded cell block cannot grow the inspector without bound.
const MAX_ROSTER_ROWS: usize = 24;

fn roster_row_counts(prisoners: usize, guards: usize, cap: usize) -> (usize, usize, usize) {
    let shown_prisoners = prisoners.min(cap);
    let shown_guards = guards.min(cap.saturating_sub(shown_prisoners));
    let hidden = prisoners + guards - shown_prisoners - shown_guards;
    (shown_prisoners, shown_guards, hidden)
}

/// Camera, layer toggles and interaction state for the world map.
#[derive(Debug, Clone)]
pub struct MapViewState {
    /// Shared camera transform (zoom + canvas-local pan).
    pub camera: MapCamera,
    /// Last known canvas size, used by button zoom and centring.
    pub viewport: Vec2,
    pub hovered_pawn: Option<(bool, u64)>,
    pub selected_pawn: Option<(bool, u64)>,
    pub hovered_facility: Option<u64>,
    pub selected_facility: Option<u64>,
    pub show_zones: bool,
    pub show_edges: bool,
    pub show_facilities: bool,
    pub show_pawns: bool,
    pub show_incidents: bool,
    pub show_labels: bool,
    pub show_help_hints: bool,
    /// Visual theme, mirrored from observer settings for the shared palette.
    pub theme: ThemeKind,
    /// True after the first frame has fitted the shared map bounds.
    initialized: bool,
}

impl Default for MapViewState {
    fn default() -> Self {
        Self {
            camera: MapCamera::new(),
            viewport: Vec2::new(800.0, 600.0),
            hovered_pawn: None,
            selected_pawn: None,
            hovered_facility: None,
            selected_facility: None,
            show_zones: true,
            show_edges: true,
            show_facilities: true,
            show_pawns: true,
            show_incidents: true,
            show_labels: true,
            show_help_hints: true,
            theme: ThemeKind::Slate,
            initialized: false,
        }
    }
}

impl MapViewState {
    /// Build map defaults from persisted observer settings.
    #[must_use]
    pub fn from_settings(s: &settings::Settings) -> Self {
        let mut state = Self::default();
        state.apply_layer_defaults(s);
        state
    }

    /// Re-apply theme, layer and hint defaults without moving the camera.
    pub fn apply_layer_defaults(&mut self, s: &settings::Settings) {
        self.show_zones = s.map_show_zones;
        self.show_edges = s.map_show_edges;
        self.show_facilities = s.map_show_facilities;
        self.show_pawns = s.map_show_pawns;
        self.show_incidents = s.map_show_incidents;
        self.show_labels = s.map_show_labels;
        self.show_help_hints = s.show_help_hints;
        self.theme = s.theme;
    }

    /// Reset hover/selection and force a refit after a world swap.
    pub fn reset_interaction(&mut self) {
        self.hovered_pawn = None;
        self.selected_pawn = None;
        self.hovered_facility = None;
        self.selected_facility = None;
        self.initialized = false;
    }

    /// Palette for the current theme, shared with every other frontend.
    #[must_use]
    pub fn palette(&self) -> Palette {
        theme::palette(self.theme)
    }

    /// Fit the canonical map bounds carried by the engine frame.
    pub fn fit(&mut self, frame: &sumo::MapFrame, size: Vec2) {
        self.viewport = size;
        self.camera = MapCamera::fit(frame.bounds(), (size.x.max(1.0), size.y.max(1.0)), 20.0);
        self.initialized = true;
    }

    /// Zoom around the canvas centre (button control).
    pub fn zoom_button(&mut self, factor: f32) {
        let anchor = (self.viewport.x * 0.5, self.viewport.y * 0.5);
        self.camera = self.camera.zoom_at(anchor, factor, 0.2, 6.0);
    }

    /// Move the camera by a fixed screen-space step (button control).
    pub fn pan_button(&mut self, dx: f32, dy: f32) {
        self.camera.pan.0 += dx;
        self.camera.pan.1 += dy;
    }

    /// Centre the selected pawn or facility (button control).
    pub fn center_on(&mut self, world: (f32, f32)) {
        let zoom = self.camera.zoom;
        self.camera.pan = (
            self.viewport.x * 0.5 - world.0 * zoom,
            self.viewport.y * 0.5 - world.1 * zoom,
        );
    }
}

/// Draw the map canvas and its inspector side panel.
pub fn show(ui: &mut egui::Ui, ctx: &mut panels::RenderCtx<'_>) {
    let snap = ctx.snapshot;
    let observer = &mut *ctx.state;

    let side_width = 300.0_f32;
    let canvas_width = (ui.available_width() - side_width - 8.0).max(260.0);
    let height = ui.available_height().max(240.0);
    if !observer.map.initialized {
        observer.map.fit(&snap.map, Vec2::new(canvas_width, height));
    }
    observer.map.viewport = Vec2::new(canvas_width, height);

    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(canvas_width, height),
            egui::Sense::click_and_drag(),
        );

        {
            let state = &mut observer.map;
            // Optional convenience: drag pans, scroll zooms around the cursor.
            // Every camera move is also reachable from the buttons below.
            if response.dragged() {
                state.pan_button(response.drag_delta().x, response.drag_delta().y);
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    let anchor = response.hover_pos().unwrap_or(rect.center()) - rect.min;
                    let factor = if scroll > 0.0 { 1.12 } else { 1.0 / 1.12 };
                    state.camera = state.camera.zoom_at((anchor.x, anchor.y), factor, 0.2, 6.0);
                }
            }

            let local_mouse = response.hover_pos().map(|p| p - rect.min);
            let hits = hit_test(snap, state, rect, local_mouse);
            state.hovered_pawn = hits.pawn;
            state.hovered_facility = hits.facility;
            if response.clicked() {
                if let Some(pawn) = hits.pawn {
                    state.selected_pawn = Some(pawn);
                } else if let Some(facility) = hits.facility {
                    state.selected_facility = Some(facility);
                    state.selected_pawn = None;
                    observer.selected_facility = Some(facility);
                } else {
                    state.selected_pawn = None;
                    state.selected_facility = None;
                    observer.selected_facility = None;
                }
            }
        }

        let painter = ui.painter().with_clip_rect(rect);
        draw_canvas(&painter, rect, snap, &observer.map);

        ui.add_space(8.0);
        ui.allocate_ui_with_layout(
            Vec2::new(side_width, height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| draw_inspector(ui, observer, snap, rect),
        );
    });
}

#[derive(Default)]
struct HitTest {
    pawn: Option<(bool, u64)>,
    facility: Option<u64>,
}

fn hit_test(
    snap: &data::Snapshot,
    state: &MapViewState,
    rect: Rect,
    mouse: Option<Vec2>,
) -> HitTest {
    let Some(mouse) = mouse else {
        return HitTest::default();
    };
    let mut out = HitTest::default();
    let mut best = f32::MAX;
    if state.show_pawns {
        for pawn in snap.map.prisoners.iter().chain(snap.map.guards.iter()) {
            let world = data::pawn_world_pos(pawn, &snap.map.facility_positions);
            let screen = local(world, state, rect);
            let d = screen.distance(rect.min + mouse);
            if d < (8.0 * state.camera.zoom.clamp(0.7, 2.0) + 4.0) && d < best {
                best = d;
                out.pawn = Some((pawn.is_prisoner, pawn.id));
                out.facility = None;
            }
        }
    }
    if out.pawn.is_none() && state.show_facilities {
        let box_size = facility_size(state.camera.zoom);
        for f in &snap.facilities {
            let center = local((f.x, f.y), state, rect);
            let r = Rect::from_center_size(center, box_size);
            if r.contains(rect.min + mouse) {
                out.facility = Some(f.id);
                break;
            }
        }
    }
    out
}

fn local(world: (f32, f32), state: &MapViewState, rect: Rect) -> egui::Pos2 {
    let (x, y) = state.camera.project(world);
    rect.min + Vec2::new(x, y)
}

fn facility_size(zoom: f32) -> Vec2 {
    let s = zoom.clamp(0.55, 1.8);
    Vec2::new(118.0 * s, 58.0 * s)
}

fn draw_canvas(painter: &egui::Painter, rect: Rect, snap: &data::Snapshot, state: &MapViewState) {
    let p = state.palette();
    let c = |color: sumo::Color| theme::from_shared(color);
    // Terrain is the base layer; the water feature and shared zones sit above.
    painter.rect_filled(rect, 4.0, c(p.terrain));

    // Subtle survey grid over the terrain makes the world-space scale legible.
    let step = 100.0 * state.camera.zoom;
    if step > 12.0 {
        let origin = state.camera.project((0.0, 0.0));
        let mut x = origin.0.rem_euclid(step);
        while x <= rect.width() {
            let color = c(sumo::Color::rgba(p.road.r, p.road.g, p.road.b, 24));
            painter.line_segment(
                [
                    rect.min + Vec2::new(x, 0.0),
                    rect.min + Vec2::new(x, rect.height()),
                ],
                Stroke::new(0.5, color),
            );
            x += step;
        }
        let mut y = origin.1.rem_euclid(step);
        while y <= rect.height() {
            let color = c(sumo::Color::rgba(p.road.r, p.road.g, p.road.b, 24));
            painter.line_segment(
                [
                    rect.min + Vec2::new(0.0, y),
                    rect.min + Vec2::new(rect.width(), y),
                ],
                Stroke::new(0.5, color),
            );
            y += step;
        }
    }

    // Water feature: the east-edge canal derived from the frame's canonical
    // bounds, never from a second geometry definition in the frontend.
    let canal = graphics::canal_bounds(snap.map.bounds());
    let water = Rect::from_min_max(
        local((canal.min_x, canal.min_y), state, rect),
        local((canal.max_x, canal.max_y), state, rect),
    );
    painter.rect_filled(
        water,
        0.0,
        c(sumo::Color::rgba(p.water.r, p.water.g, p.water.b, 170)),
    );

    // Canonical regions from the shared frame (zones are no longer rebuilt in
    // the GUI). Draw them beneath every other layer.
    if state.show_zones {
        for zone in &snap.map.zones {
            let min = local((zone.x, zone.y), state, rect);
            let size = Vec2::new(
                zone.width * state.camera.zoom,
                zone.height * state.camera.zoom,
            );
            let r = Rect::from_min_size(min, size);
            painter.rect_filled(r, 6.0, c(p.zone));
            painter.rect_stroke(
                r,
                6.0,
                Stroke::new(
                    1.0,
                    c(sumo::Color::rgba(p.muted.r, p.muted.g, p.muted.b, 110)),
                ),
                StrokeKind::Middle,
            );
            if state.show_labels {
                painter.text(
                    r.min + Vec2::new(8.0, 5.0),
                    Align2::LEFT_TOP,
                    zone.kind.label(),
                    FontId::proportional(9.0),
                    c(sumo::Color::rgba(p.muted.r, p.muted.g, p.muted.b, 220)),
                );
            }
        }
    }

    // Shared schematic road network.
    if state.show_edges {
        for edge in &snap.map.edges {
            let Some(&from) = snap.map.facility_positions.get(&edge.from) else {
                continue;
            };
            let Some(&to) = snap.map.facility_positions.get(&edge.to) else {
                continue;
            };
            let base = if edge.restricted { p.danger } else { p.road };
            let colour = c(sumo::Color::rgba(base.r, base.g, base.b, 150));
            painter.line_segment(
                [local(from, state, rect), local(to, state, rect)],
                Stroke::new(3.0, colour),
            );
        }
    }

    // Incident badges per facility with a recorded incident.
    let mut incident_counts: HashMap<u64, (u32, f32)> = HashMap::new();
    if state.show_incidents {
        for inc in &snap.incidents {
            if let Some(node) = data::incident_node(&inc.location) {
                let slot = incident_counts.entry(node).or_insert((0, 0.0));
                slot.0 += 1;
                slot.1 = slot.1.max(inc.severity);
            }
        }
    }

    // Facility footprints, shared glyphs and capacity bars.
    if state.show_facilities {
        for f in &snap.facilities {
            let center = local((f.x, f.y), state, rect);
            let box_size = facility_size(state.camera.zoom);
            let r = Rect::from_center_size(center, box_size);
            let selected =
                state.selected_facility == Some(f.id) || state.hovered_facility == Some(f.id);
            let fill = if f.locked { p.danger } else { p.panel };
            painter.rect_filled(r, 5.0, c(sumo::Color::rgba(fill.r, fill.g, fill.b, 235)));
            let stroke_colour = if f.locked {
                p.locked
            } else if selected {
                p.accent
            } else {
                p.facility
            };
            painter.rect_stroke(
                r,
                5.0,
                Stroke::new(if selected { 2.5 } else { 1.5 }, c(stroke_colour)),
                StrokeKind::Middle,
            );

            let s = state.camera.zoom.clamp(0.7, 1.4);
            painter.text(
                r.min + Vec2::new(6.0 * s, 2.0 * s),
                Align2::LEFT_TOP,
                sumo::graphics::facility_glyph(f.kind).to_string(),
                FontId::proportional(13.0 * s),
                c(if f.locked { p.locked } else { p.facility }),
            );
            if state.show_labels {
                let title = format!("{} #{}", f.kind.display(), f.id);
                painter.text(
                    r.min + Vec2::new(24.0 * s, 3.0 * s),
                    Align2::LEFT_TOP,
                    title,
                    FontId::proportional(10.0 * s),
                    c(p.text),
                );
                painter.text(
                    r.min + Vec2::new(6.0 * s, 19.0 * s),
                    Align2::LEFT_TOP,
                    format!("W{} · {}p / {}g", f.wing, f.prisoners.len(), f.guards.len()),
                    FontId::proportional(9.0 * s),
                    c(p.muted),
                );
                let bar = Rect::from_min_max(
                    r.min + Vec2::new(6.0 * s, 36.0 * s),
                    r.min + Vec2::new(box_size.x - 6.0 * s, 43.0 * s),
                );
                painter.rect_filled(
                    bar,
                    2.0,
                    c(sumo::Color::rgba(p.muted.r, p.muted.g, p.muted.b, 90)),
                );
                let util = f.utilisation();
                if util > 0.0 {
                    let filled_rect = Rect::from_min_max(
                        bar.min,
                        egui::pos2(bar.min.x + bar.width() * util, bar.max.y),
                    );
                    painter.rect_filled(filled_rect, 2.0, c(p.severity(util)));
                }
            }

            if let Some(&(count, severity)) = incident_counts.get(&f.id) {
                let badge = r.max - Vec2::new(6.0, 6.0);
                painter.circle_filled(badge, 9.0, c(p.severity(severity)));
                painter.text(
                    badge,
                    Align2::CENTER_CENTER,
                    count.to_string(),
                    FontId::proportional(9.0),
                    c(p.background),
                );
            }
        }
    }

    // Pawns (prisoners then guards) with selection halos.
    if state.show_pawns {
        for pawn in snap.map.prisoners.iter().chain(snap.map.guards.iter()) {
            let world = data::pawn_world_pos(pawn, &snap.map.facility_positions);
            let point = local(world, state, rect);
            if !rect.contains(point) {
                continue;
            }
            let radius = 4.5 * state.camera.zoom.clamp(0.7, 2.0);
            let colour = if pawn.is_prisoner {
                p.prisoner
            } else {
                p.guard
            };
            let selected = state.selected_pawn == Some((pawn.is_prisoner, pawn.id));
            let hovered = state.hovered_pawn == Some((pawn.is_prisoner, pawn.id));
            if selected || hovered {
                painter.circle_stroke(point, radius + 3.0, Stroke::new(2.0, c(p.accent)));
            }
            painter.circle_filled(point, radius, c(colour));
            if state.show_labels && state.camera.zoom >= 1.2 {
                painter.text(
                    point + Vec2::new(radius + 3.0, 0.0),
                    Align2::LEFT_CENTER,
                    &pawn.name,
                    FontId::proportional(9.0),
                    c(colour),
                );
            }
        }
    }

    draw_map_furniture(painter, rect, state, &p);
}

/// Screen-space legend, north arrow and scale bar. These never pan or zoom.
fn draw_map_furniture(painter: &egui::Painter, rect: Rect, state: &MapViewState, p: &Palette) {
    let c = |color: sumo::Color| theme::from_shared(color);
    let text = c(p.text);
    let muted = c(p.muted);
    let panel = Rect::from_min_max(
        rect.max - Vec2::new(210.0, 126.0),
        rect.max - Vec2::new(8.0, 8.0),
    );
    painter.rect_filled(
        panel,
        4.0,
        c(sumo::Color::rgba(p.panel.r, p.panel.g, p.panel.b, 220)),
    );
    painter.rect_stroke(panel, 4.0, Stroke::new(1.0, muted), StrokeKind::Middle);
    let left = panel.min + Vec2::new(8.0, 6.0);
    painter.text(
        left,
        Align2::LEFT_TOP,
        "LEGEND",
        FontId::proportional(10.0),
        text,
    );
    let rows = [
        (p.prisoner, "Prisoner"),
        (p.guard, "Guard"),
        (p.facility, "Facility"),
        (p.locked, "Locked / restricted"),
    ];
    for (i, (colour, label)) in rows.into_iter().enumerate() {
        let y = left.y + 18.0 + i as f32 * 13.0;
        painter.circle_filled(left + Vec2::new(4.0, y - left.y + 4.0), 3.0, c(colour));
        painter.text(
            left + Vec2::new(14.0, y - left.y),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(9.0),
            text,
        );
    }

    // Compass rose.
    let north = panel.min + Vec2::new(174.0, 32.0);
    painter.text(
        north,
        Align2::CENTER_BOTTOM,
        "N",
        FontId::proportional(10.0),
        text,
    );
    painter.line_segment(
        [north + Vec2::new(0.0, 3.0), north + Vec2::new(0.0, 22.0)],
        Stroke::new(1.5, text),
    );
    painter.add(Shape::convex_polygon(
        vec![
            north + Vec2::new(0.0, -2.0),
            north + Vec2::new(-5.0, 7.0),
            north + Vec2::new(5.0, 7.0),
        ],
        text,
        Stroke::NONE,
    ));

    // Scale bar: 100 canonical world units, laid out in screen space.
    let bar_start = panel.min + Vec2::new(8.0, 101.0);
    let bar_len = 100.0 * state.camera.zoom;
    painter.line_segment(
        [bar_start, bar_start + Vec2::new(bar_len, 0.0)],
        Stroke::new(2.0, text),
    );
    painter.text(
        bar_start + Vec2::new(0.0, 3.0),
        Align2::LEFT_TOP,
        "100 m",
        FontId::proportional(9.0),
        muted,
    );

    if state.show_help_hints {
        painter.rect_filled(
            Rect::from_min_max(
                rect.min + Vec2::new(6.0, 6.0),
                rect.min + Vec2::new(330.0, 24.0),
            ),
            3.0,
            c(sumo::Color::rgba(p.panel.r, p.panel.g, p.panel.b, 220)),
        );
        painter.text(
            rect.min + Vec2::new(12.0, 9.0),
            Align2::LEFT_TOP,
            "Drag to pan · scroll to zoom · buttons control all camera moves",
            FontId::proportional(9.0),
            muted,
        );
    }
}

fn draw_inspector(
    ui: &mut egui::Ui,
    observer: &mut crate::app::ObserverState,
    snap: &data::Snapshot,
    rect: Rect,
) {
    let selected_incident = &mut observer.selected_incident;
    let state = &mut observer.map;
    ui.add(egui::Label::new(
        egui::RichText::new("World Map")
            .strong()
            .color(theme::from_shared(state.palette().accent)),
    ));

    ui.horizontal(|ui| {
        if ui.button("Zoom In").clicked() {
            state.zoom_button(1.25);
        }
        if ui.button("Zoom Out").clicked() {
            state.zoom_button(0.8);
        }
        if ui.button("Fit").clicked() {
            state.fit(&snap.map, rect.size());
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Pan ◀").clicked() {
            state.pan_button(-120.0, 0.0);
        }
        if ui.button("Pan ▶").clicked() {
            state.pan_button(120.0, 0.0);
        }
        if ui.button("Pan ▲").clicked() {
            state.pan_button(0.0, -120.0);
        }
        if ui.button("Pan ▼").clicked() {
            state.pan_button(0.0, 120.0);
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Center Selection").clicked() {
            if let Some(world) = selected_world(snap, state) {
                state.center_on(world);
            } else {
                observer.status = Some("Select a pawn or facility to center it".to_string());
            }
        }
        if ui
            .button("Reset view")
            .on_hover_text("Clears selection and refits the camera; map layers are unchanged")
            .clicked()
        {
            state.reset_interaction();
            observer.selected_facility = None;
            state.fit(&snap.map, rect.size());
        }
    });
    ui.add(egui::Label::new(
        egui::RichText::new(format!("{:.2}x", state.camera.zoom)).color(theme::MUTED),
    ));

    ui.separator();
    ui.add(egui::Label::new(
        egui::RichText::new("Map layers")
            .strong()
            .color(theme::ACCENT),
    ));
    ui.checkbox(&mut state.show_zones, "Regions");
    ui.checkbox(&mut state.show_edges, "Roads / nav edges");
    ui.checkbox(&mut state.show_facilities, "Facilities");
    ui.checkbox(&mut state.show_pawns, "Pawns");
    ui.checkbox(&mut state.show_incidents, "Incidents");
    ui.checkbox(&mut state.show_labels, "Labels");

    ui.separator();
    let pawn = state
        .hovered_pawn
        .or(state.selected_pawn)
        .and_then(|(is_prisoner, id)| {
            let list = if is_prisoner {
                &snap.map.prisoners
            } else {
                &snap.map.guards
            };
            list.iter().find(|p| p.id == id)
        });
    if let Some(pawn) = pawn {
        ui.add(egui::Label::new(
            egui::RichText::new(&pawn.name)
                .strong()
                .color(theme::from_shared(state.palette().accent)),
        ));
        theme::kv(
            ui,
            "Role",
            if pawn.is_prisoner {
                "Prisoner"
            } else {
                "Guard"
            },
        );
        let facility = snap.facilities.iter().find(|f| f.id == pawn.node_id);
        theme::kv(
            ui,
            "Location",
            facility.map_or_else(|| format!("node {}", pawn.node_id), |f| f.label()),
        );
        theme::kv(ui, "Activity", &pawn.activity);
        if let Some(intention) = &pawn.intention {
            theme::kv(ui, "Intention", intention);
        }
        if let Some(class) = &pawn.classification {
            theme::kv(ui, "Class", class);
        }
        if let Some(gang) = pawn.gang {
            theme::kv(ui, "Gang", format!("#{gang}"));
        }
        if let Some(post) = &pawn.post {
            theme::kv(ui, "Post", post);
        }
        if let Some(mood) = pawn.mood {
            theme::kv(ui, "Mood", format!("{:.0}%", mood * 100.0));
        }
        if let Some(threat) = pawn.threat {
            theme::kv(ui, "Threat", format!("{:.0}%", threat * 100.0));
        }
        if let Some(target) = pawn.target_node {
            theme::kv(ui, "Heading", format!("node {target}"));
        }
    } else {
        ui.add(egui::Label::new(
            egui::RichText::new("Hover a pawn for details · click to pin").color(theme::MUTED),
        ));
    }

    ui.separator();
    let facility_id = state.hovered_facility.or(state.selected_facility);
    if let Some(f) = facility_id.and_then(|id| snap.facilities.iter().find(|f| f.id == id)) {
        ui.add(egui::Label::new(
            egui::RichText::new(f.label())
                .strong()
                .color(theme::from_shared(state.palette().facility)),
        ));
        theme::kv(ui, "Capacity", f.capacity.to_string());
        theme::kv(ui, "Utilisation", data::pct(f.utilisation()));
        theme::kv(ui, "Condition", data::pct(f.condition));
        theme::kv(ui, "Staff required", f.staff_required.to_string());
        theme::kv(ui, "Locked", if f.locked { "YES" } else { "no" });
        panels::section(ui, "Cell / wing roster", |ui| {
            let (shown_prisoners, shown_guards, hidden) =
                roster_row_counts(f.prisoners.len(), f.guards.len(), MAX_ROSTER_ROWS);
            egui::ScrollArea::vertical()
                .id_salt("facility_roster")
                .max_height(160.0)
                .show(ui, |ui| {
                    for id in f.prisoners.iter().take(shown_prisoners) {
                        let name = snap
                            .prisoners
                            .iter()
                            .find(|p| p.id.raw() == *id)
                            .map_or_else(|| format!("prisoner #{id}"), |p| p.name.clone());
                        ui.add(egui::Label::new(
                            egui::RichText::new(format!("● {name}"))
                                .color(theme::PRISONER)
                                .small(),
                        ));
                    }
                    for id in f.guards.iter().take(shown_guards) {
                        let name = snap
                            .guards
                            .iter()
                            .find(|g| g.id == *id)
                            .map_or_else(|| format!("guard #{id}"), |g| g.name.clone());
                        ui.add(egui::Label::new(
                            egui::RichText::new(format!("◆ {name}"))
                                .color(theme::GUARD)
                                .small(),
                        ));
                    }
                    if hidden > 0 {
                        ui.add(egui::Label::new(
                            egui::RichText::new(format!(
                                "+{hidden} more ({} total at this node)",
                                f.prisoners.len() + f.guards.len()
                            ))
                            .color(theme::MUTED)
                            .small(),
                        ));
                    }
                    if f.prisoners.is_empty() && f.guards.is_empty() {
                        ui.add(egui::Label::new(
                            egui::RichText::new("No pawns at this node").color(theme::MUTED),
                        ));
                    }
                });
        });
    } else {
        ui.add(egui::Label::new(
            egui::RichText::new("Hover a facility for its roster").color(theme::MUTED),
        ));
    }

    if state.show_incidents {
        ui.separator();
        panels::section(ui, "Incidents on map", |ui| {
            let recent: Vec<_> = snap.incidents.iter().rev().take(6).collect();
            if recent.is_empty() {
                ui.label("No structured incidents yet.");
            }
            for inc in recent {
                let label = format!(
                    "#{} {} sev {:.0}% · {}",
                    inc.id,
                    inc.kind,
                    inc.severity * 100.0,
                    data::short_truncate(&inc.location, 22)
                );
                if ui.add(egui::Button::new(label).small()).clicked() {
                    *selected_incident = Some(inc.id);
                }
            }
        });
    }
}

fn selected_world(snap: &data::Snapshot, state: &MapViewState) -> Option<(f32, f32)> {
    if let Some((is_prisoner, id)) = state.selected_pawn.or(state.hovered_pawn) {
        let list = if is_prisoner {
            &snap.map.prisoners
        } else {
            &snap.map.guards
        };
        if let Some(pawn) = list.iter().find(|p| p.id == id) {
            return Some(data::pawn_world_pos(pawn, &snap.map.facility_positions));
        }
    }
    state
        .selected_facility
        .or(state.hovered_facility)
        .and_then(|id| snap.facilities.iter().find(|f| f.id == id))
        .map(|f| (f.x, f.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sumo::ViewMode;

    #[test]
    fn rosters_are_capped_and_hidden_rows_counted() {
        assert_eq!(roster_row_counts(4, 3, 5), (4, 1, 2));
        assert_eq!(roster_row_counts(0, 0, MAX_ROSTER_ROWS), (0, 0, 0));
        let (p, g, hidden) = roster_row_counts(100, 5, MAX_ROSTER_ROWS);
        assert_eq!(p + g, MAX_ROSTER_ROWS);
        assert_eq!(hidden, 105 - MAX_ROSTER_ROWS);
    }

    #[test]
    fn camera_fit_uses_the_shared_frame_bounds() {
        let frame = sumo::Sim::new(0x5EED).map_frame();
        let mut state = MapViewState::default();
        state.fit(&frame, Vec2::new(800.0, 600.0));
        assert!(state.initialized);
        assert!(state.camera.zoom > 0.0);
        let bounds = frame.bounds();
        let (left, top) = state.camera.project((bounds.min_x, bounds.min_y));
        let (right, bottom) = state.camera.project((bounds.max_x, bounds.max_y));
        assert!((left - right).abs() < 1e-3 || right > left);
        assert!(bottom > top);
    }

    #[test]
    fn settings_apply_to_map_layers_and_theme() {
        let s = settings::Settings {
            view: ViewMode::Map,
            theme: ThemeKind::Paper,
            map_show_zones: false,
            map_show_edges: false,
            map_show_facilities: false,
            map_show_pawns: false,
            map_show_incidents: false,
            map_show_labels: false,
            show_help_hints: false,
            ..settings::Settings::default()
        };
        let state = MapViewState::from_settings(&s);
        assert!(!state.show_zones && !state.show_edges && !state.show_facilities);
        assert!(!state.show_pawns && !state.show_incidents && !state.show_labels);
        assert!(!state.show_help_hints);
        assert_eq!(state.theme, ThemeKind::Paper);
    }

    #[test]
    fn button_camera_controls_change_the_shared_camera() {
        let mut state = MapViewState::default();
        state.viewport = Vec2::new(800.0, 600.0);
        state.camera = MapCamera::fit(
            sumo::MapBounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 1000.0,
                max_y: 700.0,
            },
            (800.0, 600.0),
            20.0,
        );
        let zoom0 = state.camera.zoom;
        state.zoom_button(1.25);
        assert!(state.camera.zoom > zoom0);
        state.zoom_button(0.8);
        assert!((state.camera.zoom - zoom0).abs() < 1e-5);
        let pan0 = state.camera.pan;
        state.pan_button(-50.0, 30.0);
        assert_eq!(state.camera.pan, (pan0.0 - 50.0, pan0.1 + 30.0));
        state.center_on((500.0, 350.0));
        assert!((state.camera.project((500.0, 350.0)).0 - 400.0).abs() < 1e-5);
        state.reset_interaction();
        assert!(!state.initialized);
    }

    #[test]
    fn reset_interaction_preserves_layer_and_theme_settings() {
        let s = settings::Settings {
            view: ViewMode::Map,
            theme: ThemeKind::Paper,
            map_show_zones: false,
            map_show_labels: false,
            show_help_hints: false,
            ..settings::Settings::default()
        };
        let mut state = MapViewState::from_settings(&s);
        state.selected_pawn = Some((true, 4));
        state.hovered_facility = Some(9);
        state.initialized = true;
        let camera = state.camera;

        state.reset_interaction();

        assert_eq!(state.camera, camera);
        assert_eq!(state.selected_pawn, None);
        assert_eq!(state.hovered_facility, None);
        assert!(!state.initialized);
        assert!(!state.show_zones && !state.show_labels && !state.show_help_hints);
        assert_eq!(state.theme, ThemeKind::Paper);
    }

    #[test]
    fn canonical_bounds_and_canal_follow_frame_dimensions() {
        let mut frame = sumo::Sim::new(0xB0A7).map_frame();
        frame.width = 640.0;
        frame.height = 480.0;

        let bounds = frame.bounds();
        assert_eq!(
            (bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y),
            (0.0, 0.0, 640.0, 480.0)
        );

        let canal = graphics::canal_bounds(bounds);
        assert_eq!(
            (canal.min_x, canal.min_y, canal.max_x, canal.max_y),
            (550.0, 0.0, 640.0, 480.0)
        );
    }
}
