//! Shared colours and small rendering helpers for the egui observer.
//!
//! Keeping every colour decision in one place means the eleven observation
//! views stay visually consistent and the data layer can ask for a colour
//! (e.g. [`incident_colour`]) without depending on widget construction.

use crate::data::IncidentKind;
use crate::settings::UiDensity;
use egui::Color32;
use sumo::{Palette, ThemeKind};

/// Highlight colour used for the active view tab and selected rows.
pub const ACCENT: Color32 = Color32::from_rgb(91, 200, 255);
/// Muted colour for secondary labels and captions (shared Slate palette).
pub const MUTED: Color32 = Color32::from_rgb(150, 163, 180);
/// Background of the map canvas (shared Slate palette).
pub const MAP_BG: Color32 = Color32::from_rgb(14, 18, 24);

/// Colour used for a pawn that is a prisoner on the live map.
pub const PRISONER: Color32 = Color32::from_rgb(255, 176, 92);
/// Colour used for a pawn that is a guard on the live map.
pub const GUARD: Color32 = Color32::from_rgb(120, 200, 255);
/// Colour used for a facility node on the live map.
pub const FACILITY: Color32 = Color32::from_rgb(120, 230, 170);
/// Colour used for a locked facility node.
pub const LOCKED: Color32 = Color32::from_rgb(255, 96, 96);

/// Convert a toolkit-neutral shared colour to egui's `Color32`.
#[must_use]
pub const fn from_shared(color: sumo::Color) -> Color32 {
    Color32::from_rgba_unmultiplied_const(color.r, color.g, color.b, color.a)
}

/// Resolve the canonical palette for a theme.
#[must_use]
pub fn palette(theme: ThemeKind) -> Palette {
    Palette::for_theme(theme)
}

/// Apply the selected theme and UI density to the egui context.
///
/// This is called by the shell when the preference changes; individual views
/// never mutate globals. The map additionally uses [`palette`] directly so its
/// terrain, zones, roads and pawns follow the same shared colour language.
pub fn apply_visuals(ctx: &egui::Context, theme: ThemeKind, density: UiDensity) {
    let p = palette(theme);
    let light = theme == ThemeKind::Paper;
    ctx.set_theme(if light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    });
    let mut visuals = if light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.panel_fill = from_shared(p.panel);
    visuals.window_fill = from_shared(p.panel);
    visuals.extreme_bg_color = from_shared(p.background);
    visuals.faint_bg_color = from_shared(p.panel);
    visuals.hyperlink_color = from_shared(p.accent);
    visuals.selection.stroke.color = from_shared(p.accent);
    visuals.widgets.noninteractive.bg_fill = from_shared(p.panel);
    visuals.widgets.active.bg_fill = from_shared(p.zone);
    visuals.widgets.hovered.bg_fill = from_shared(p.zone);
    ctx.set_visuals(visuals);

    let (item_spacing, button_padding) = match density {
        UiDensity::Compact => (egui::vec2(4.0, 3.0), egui::vec2(5.0, 3.0)),
        UiDensity::Comfortable => (egui::vec2(6.0, 4.0), egui::vec2(7.0, 4.0)),
    };
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = item_spacing;
        style.spacing.button_padding = button_padding;
    });
}

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
    let value = v.to_ascii_lowercase();
    if value.starts_with("accepted") {
        good_colour(1.0)
    } else if value.starts_with("rejected") {
        Color32::from_rgb(255, 90, 90)
    } else if value.starts_with("repaired") || value.starts_with("proposed") {
        Color32::from_rgb(255, 195, 90)
    } else {
        MUTED
    }
}

/// Draw a labelled 0..1 bar. `good` selects the colour orientation.
pub fn metric_bar(ui: &mut egui::Ui, label: &str, value: f32, good: bool) {
    ui.horizontal(|ui| {
        ui.label(label);
        let v = value.clamp(0.0, 1.0);
        let colour = if good {
            good_colour(v)
        } else {
            urgency_colour(v)
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_accent_colors_match_the_shared_slate_palette() {
        let p = palette(ThemeKind::Slate);
        assert_eq!(ACCENT, from_shared(p.accent));
        assert_eq!(MUTED, from_shared(p.muted));
        assert_eq!(PRISONER, from_shared(p.prisoner));
        assert_eq!(GUARD, from_shared(p.guard));
        assert_eq!(FACILITY, from_shared(p.facility));
        assert_eq!(LOCKED, from_shared(p.locked));
        assert_eq!(MAP_BG, from_shared(p.background));
    }

    #[test]
    fn shared_color_conversion_preserves_alpha() {
        let c = sumo::Color::rgba(10, 20, 30, 90);
        assert_eq!(
            from_shared(c),
            Color32::from_rgba_unmultiplied(10, 20, 30, 90)
        );
    }
}
