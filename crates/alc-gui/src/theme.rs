//! Shared colours and small rendering helpers for the egui observer.
//!
//! Keeping every colour decision in one place means the eleven observation
//! views stay visually consistent and the data layer can ask for a colour
//! (e.g. [`incident_colour`]) without depending on widget construction.

use crate::data::IncidentKind;
use egui::Color32;

/// Highlight colour used for the active view tab and selected rows.
pub const ACCENT: Color32 = Color32::from_rgb(91, 200, 255);
/// Muted colour for secondary labels and captions.
pub const MUTED: Color32 = Color32::from_rgb(150, 160, 175);
/// Background of the map canvas.
pub const MAP_BG: Color32 = Color32::from_rgb(22, 27, 34);

/// Colour used for a pawn that is a prisoner on the live map.
pub const PRISONER: Color32 = Color32::from_rgb(255, 176, 92);
/// Colour used for a pawn that is a guard on the live map.
pub const GUARD: Color32 = Color32::from_rgb(120, 200, 255);
/// Colour used for a facility node on the live map.
pub const FACILITY: Color32 = Color32::from_rgb(120, 230, 170);
/// Colour used for a locked facility node.
pub const LOCKED: Color32 = Color32::from_rgb(255, 96, 96);

/// Colour for each incident bucket (shared by the data layer and panels).
#[must_use]
pub fn incident_colour(kind: IncidentKind) -> Color32 {
    match kind {
        IncidentKind::Fight => Color32::from_rgb(255, 92, 92),
        IncidentKind::Argument => Color32::from_rgb(255, 178, 92),
        IncidentKind::Threat => Color32::from_rgb(255, 120, 160),
        IncidentKind::Escape => Color32::from_rgb(220, 92, 255),
        IncidentKind::Contraband => Color32::from_rgb(255, 220, 92),
        IncidentKind::Misconduct => Color32::from_rgb(200, 160, 255),
        IncidentKind::GangClash => Color32::from_rgb(255, 72, 72),
        IncidentKind::Security => Color32::from_rgb(120, 180, 255),
        IncidentKind::Other => MUTED,
    }
}

/// Colour for a 0..1 severity value (green → yellow → red).
#[must_use]
pub fn severity_colour(v: f32) -> Color32 {
    let v = v.clamp(0.0, 1.0);
    if v >= 0.75 {
        Color32::from_rgb(255, 80, 80)
    } else if v >= 0.4 {
        Color32::from_rgb(255, 190, 80)
    } else {
        Color32::from_rgb(130, 220, 140)
    }
}

/// Colour for a "higher is better" 0..1 metric.
#[must_use]
pub fn good_colour(v: f32) -> Color32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.25 {
        Color32::from_rgb(255, 90, 90)
    } else if v <= 0.5 {
        Color32::from_rgb(255, 190, 80)
    } else {
        Color32::from_rgb(130, 220, 140)
    }
}

/// Colour for a "higher is worse" 0..1 pressure metric.
#[must_use]
pub fn urgency_colour(v: f32) -> Color32 {
    good_colour(1.0 - v)
}

/// Colour for a decision validation outcome string.
#[must_use]
pub fn validation_colour(v: &str) -> Color32 {
    match v.to_ascii_lowercase().as_str() {
        "accepted" => good_colour(1.0),
        "repaired" | "proposed" => Color32::from_rgb(255, 195, 90),
        "rejected" => Color32::from_rgb(255, 90, 90),
        _ => MUTED,
    }
}

/// Draw a labelled 0..1 bar. `good` selects the colour orientation.
pub fn metric_bar(ui: &mut egui::Ui, label: &str, value: f32, good: bool) {
    ui.horizontal(|ui| {
        ui.label(label);
        let v = value.clamp(0.0, 1.0);
        let colour = if good { good_colour(v) } else { urgency_colour(v) };
        let bar = egui::ProgressBar::new(v)
            .desired_width(110.0)
            .fill(colour)
            .text(format!("{v:.2}"));
        ui.add(bar);
    });
}

/// Render a compact "key: value" line, with the key muted.
pub fn kv(ui: &mut egui::Ui, key: &str, value: impl Into<egui::WidgetText>) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(egui::RichText::new(key).color(MUTED)));
        ui.label(value);
    });
}
