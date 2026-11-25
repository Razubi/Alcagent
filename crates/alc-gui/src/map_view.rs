//! Live spatial prison map built from `sumo::MapFrame`.
//!
//! Facilities are drawn as nodes at their engine-provided world coordinates;
//! prisoners and guards are pawns on those nodes (or interpolated along a nav
//! edge). The camera is a pure `(zoom, pan)` transform: panning and hovering
//! never rebuild the cached [`data::Snapshot`].

use crate::{data, panels, theme};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Stroke, StrokeKind, Vec2};
use std::collections::HashMap;

/// Camera and interaction state for the prison map.
#[derive(Debug, Clone)]
pub struct MapViewState {
    pub zoom: f32,
    /// Screen-space offset in canvas-local coordinates.
    pub pan: Vec2,
    pub hovered_pawn: Option<(bool, u64)>,
    pub selected_pawn: Option<(bool, u64)>,
    pub hovered_facility: Option<u64>,
    pub selected_facility: Option<u64>,
    pub show_edges: bool,
    pub show_incidents: bool,
    pub show_labels: bool,
    initialized: bool,
}

impl Default for MapViewState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
            hovered_pawn: None,
            selected_pawn: None,
            hovered_facility: None,
            selected_facility: None,
            show_edges: true,
            show_incidents: true,
            show_labels: true,
            initialized: false,
        }
    }
}

impl MapViewState {
    /// Fit the camera to the facilities in `frame` within `size`.
    pub fn fit(&mut self, frame: &sumo::MapFrame, size: Vec2) {
        if frame.facility_positions.is_empty() {
            return;
        }
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for &(x, y) in frame.facility_positions.values() {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        let w = (max_x - min_x).max(200.0);
        let h = (max_y - min_y).max(160.0);
        self.zoom = ((size.x - 40.0) / w)
            .min((size.y - 40.0) / h)
            .clamp(0.25, 4.0);
        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        self.pan = Vec2::new(size.x * 0.5 - cx * self.zoom, size.y * 0.5 - cy * self.zoom);
        self.initialized = true;
    }
}

/// Draw the map canvas and its inspector side panel.
pub fn show(ui: &mut egui::Ui, ctx: &mut panels::RenderCtx<'_>) {
    let snap = ctx.snapshot;
    let observer = &mut *ctx.state;

    if !observer.map.initialized {
        let size = Vec2::new(
            ui.available_width().max(400.0) - 300.0,
            ui.available_height().max(240.0),
        );
        observer.map.fit(&snap.map, size);
    }

    ui.horizontal(|ui| {
        let side_width = 300.0_f32;
        let canvas_width = (ui.available_width() - side_width - 8.0).max(260.0);
        let height = ui.available_height().max(240.0);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(canvas_width, height),
            egui::Sense::click_and_drag(),
        );

        {
            let state = &mut observer.map;
            if response.dragged() {
                state.pan += response.drag_delta();
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    let anchor = response.hover_pos().unwrap_or(rect.center()) - rect.min.to_vec2();
                    let factor = if scroll > 0.0 { 1.12 } else { 1.0 / 1.12 };
                    let (zoom, pan) =
                        data::zoom_at(anchor, state.zoom, state.pan, factor, 0.25, 5.0);
                    state.zoom = zoom;
                    state.pan = pan;
                }
            }

            let local_mouse = response.hover_pos().map(|p| p - rect.min.to_vec2());
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

            let painter = ui.painter().with_clip_rect(rect);
            draw_canvas(&painter, rect, snap, state);
        }

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
    mouse: Option<egui::Pos2>,
) -> HitTest {
    let Some(mouse) = mouse else {
        return HitTest::default();
    };
    let mut out = HitTest::default();
    let mut best = f32::MAX;
    for pawn in snap.map.prisoners.iter().chain(snap.map.guards.iter()) {
        let world = data::pawn_world_pos(pawn, &snap.map.facility_positions);
        let screen = local(world, state, rect);
        let d = screen.distance(mouse);
        if d < (8.0 * state.zoom.clamp(0.7, 2.0) + 4.0) && d < best {
            best = d;
            out.pawn = Some((pawn.is_prisoner, pawn.id));
            out.facility = None;
        }
    }
    if out.pawn.is_none() {
        let box_size = facility_size(state.zoom);
        for f in &snap.facilities {
            let center = local((f.x, f.y), state, rect);
            let r = Rect::from_center_size(center, box_size);
            if r.contains(mouse) {
                out.facility = Some(f.id);
                break;
            }
        }
    }
    out
}

fn local(world: (f32, f32), state: &MapViewState, rect: Rect) -> egui::Pos2 {
    rect.min + data::world_to_screen(world, state.zoom, state.pan).to_vec2()
}

fn facility_size(zoom: f32) -> Vec2 {
    let s = zoom.clamp(0.55, 1.8);
    Vec2::new(118.0 * s, 58.0 * s)
}

fn draw_canvas(painter: &egui::Painter, rect: Rect, snap: &data::Snapshot, state: &MapViewState) {
    painter.rect_filled(rect, 4.0, theme::MAP_BG);
    let positions = &snap.map.facility_positions;

    // Navigation edges and movement hints (drawn beneath the nodes).
    if state.show_edges {
        for edge in &snap.map.edges {
            let Some(&from) = positions.get(&edge.from) else {
                continue;
            };
            let Some(&to) = positions.get(&edge.to) else {
                continue;
            };
            let colour = if edge.restricted {
                Color32::from_rgba_unmultiplied(255, 140, 90, 110)
            } else if edge.sightline {
                Color32::from_rgba_unmultiplied(120, 200, 255, 130)
            } else {
                Color32::from_rgba_unmultiplied(150, 150, 170, 90)
            };
            painter.line_segment(
                [local(from, state, rect), local(to, state, rect)],
                Stroke::new(1.5, colour),
            );
        }
        for pawn in snap.map.prisoners.iter().chain(snap.map.guards.iter()) {
            if let (Some(_progress), Some(target)) = (pawn.edge_progress, pawn.target_node) {
                let Some(&from) = positions.get(&pawn.node_id) else {
                    continue;
                };
                let Some(&to) = positions.get(&target) else {
                    continue;
                };
                painter.line_segment(
                    [local(from, state, rect), local(to, state, rect)],
                    Stroke::new(3.0, Color32::from_rgba_unmultiplied(255, 255, 255, 45)),
                );
            }
        }
    }

    // Incident overlays: one badge per facility that has a recorded incident.
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

    // Facility nodes with capacity / utilisation and rosters.
    for f in &snap.facilities {
        let center = local((f.x, f.y), state, rect);
        let box_size = facility_size(state.zoom);
        let r = Rect::from_center_size(center, box_size);
        let selected =
            state.selected_facility == Some(f.id) || state.hovered_facility == Some(f.id);
        let fill = if f.locked {
            Color32::from_rgba_unmultiplied(90, 40, 45, 235)
        } else {
            Color32::from_rgba_unmultiplied(38, 52, 58, 235)
        };
        painter.rect_filled(r, 5.0, fill);
        let stroke_colour = if f.locked {
            theme::LOCKED
        } else if selected {
            theme::ACCENT
        } else {
            theme::FACILITY
        };
        painter.rect_stroke(
            r,
            5.0,
            Stroke::new(if selected { 2.5 } else { 1.5 }, stroke_colour),
            StrokeKind::Middle,
        );

        if state.show_labels {
            let s = state.zoom.clamp(0.7, 1.4);
            let title = format!("{} #{}", f.kind.display(), f.id);
            painter.text(
                r.min + Vec2::new(6.0 * s, 4.0 * s),
                Align2::LEFT_TOP,
                title,
                FontId::proportional(11.0 * s),
                Color32::WHITE,
            );
            painter.text(
                r.min + Vec2::new(6.0 * s, 19.0 * s),
                Align2::LEFT_TOP,
                format!(
                    "W{} · {} prisoners · {} guards",
                    f.wing,
                    f.prisoners.len(),
                    f.guards.len()
                ),
                FontId::proportional(9.0 * s),
                theme::MUTED,
            );
            let bar = Rect::from_min_max(
                r.min + Vec2::new(6.0 * s, 36.0 * s),
                r.min + Vec2::new(box_size.x - 6.0 * s, 43.0 * s),
            );
            painter.rect_filled(bar, 2.0, Color32::from_gray(60));
            let util = f.utilisation();
            let filled_rect = Rect::from_min_max(
                bar.min,
                egui::pos2(bar.min.x + bar.width() * util, bar.max.y),
            );
            if util > 0.0 {
                painter.rect_filled(filled_rect, 2.0, theme::severity_colour(util));
            }
        }

        if let Some(&(count, severity)) = incident_counts.get(&f.id) {
            let badge = r.max - Vec2::new(6.0, 6.0);
            painter.circle_filled(badge, 9.0, theme::severity_colour(severity));
            painter.text(
                badge,
                Align2::CENTER_CENTER,
                count.to_string(),
                FontId::proportional(9.0),
                Color32::BLACK,
            );
        }
    }

    // Pawns (prisoners then guards) with selection halos.
    for pawn in snap.map.prisoners.iter().chain(snap.map.guards.iter()) {
        let world = data::pawn_world_pos(pawn, &positions);
        let p = local(world, state, rect);
        if !rect.contains(p) {
            continue;
        }
        let radius = 4.5 * state.zoom.clamp(0.7, 2.0);
        let colour = if pawn.is_prisoner {
            theme::PRISONER
        } else {
            theme::GUARD
        };
        let selected = state.selected_pawn == Some((pawn.is_prisoner, pawn.id));
        let hovered = state.hovered_pawn == Some((pawn.is_prisoner, pawn.id));
        if selected || hovered {
            painter.circle_stroke(p, radius + 3.0, Stroke::new(2.0, theme::ACCENT));
        }
        painter.circle_filled(p, radius, colour);
        if state.show_labels && state.zoom >= 1.0 {
            painter.text(
                p + Vec2::new(radius + 3.0, 0.0),
                Align2::LEFT_CENTER,
                &pawn.name,
                FontId::proportional(9.0),
                if pawn.is_prisoner {
                    theme::PRISONER
                } else {
                    theme::GUARD
                },
            );
        }
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
        egui::RichText::new("Map Inspector")
            .strong()
            .color(theme::ACCENT),
    ));
    ui.horizontal(|ui| {
        if ui.button("Fit").clicked() {
            state.fit(&snap.map, rect.size());
        }
        if ui.button("Reset").clicked() {
            *state = MapViewState::default();
            state.fit(&snap.map, rect.size());
        }
        ui.add(egui::Label::new(
            egui::RichText::new(format!("{:.2}x", state.zoom)).color(theme::MUTED),
        ));
    });
    ui.checkbox(&mut state.show_edges, "Nav edges / movement");
    ui.checkbox(&mut state.show_incidents, "Incident overlays");
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
                .color(theme::ACCENT),
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
                .color(theme::FACILITY),
        ));
        theme::kv(ui, "Capacity", f.capacity.to_string());
        theme::kv(ui, "Utilisation", data::pct(f.utilisation()));
        theme::kv(ui, "Condition", data::pct(f.condition));
        theme::kv(ui, "Staff required", f.staff_required.to_string());
        theme::kv(ui, "Locked", if f.locked { "YES" } else { "no" });
        panels::section(ui, "Cell / wing roster", |ui| {
            for id in &f.prisoners {
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
            for id in &f.guards {
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
            if f.prisoners.is_empty() && f.guards.is_empty() {
                ui.add(egui::Label::new(
                    egui::RichText::new("No pawns at this node").color(theme::MUTED),
                ));
            }
        });
    } else {
        ui.add(egui::Label::new(
            egui::RichText::new("Hover a facility for its roster").color(theme::MUTED),
        ));
    }

    ui.separator();
    panels::section(ui, "Legend", |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(
                egui::RichText::new("●").color(theme::PRISONER),
            ));
            ui.label("Prisoner");
            ui.add(egui::Label::new(
                egui::RichText::new("◆").color(theme::GUARD),
            ));
            ui.label("Guard");
        });
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(
                egui::RichText::new("■").color(theme::FACILITY),
            ));
            ui.label("Facility");
            ui.add(egui::Label::new(
                egui::RichText::new("■").color(theme::LOCKED),
            ));
            ui.label("Locked");
        });
    });

    if state.show_incidents {
        ui.separator();
        panels::section(ui, "Incidents on map", |ui| {
            let recent: Vec<_> = snap.incidents.iter().rev().take(8).collect();
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
