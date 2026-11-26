//! Shared panels and small rendering helpers used by the observation views.

use crate::{
    ViewMode,
    app::{Command, ObserverState},
    data, theme,
};
use alc_core::TimeSpeed;
use eframe::egui;
use std::collections::HashMap;

/// All eleven observation views, in the engine's canonical tab order.
pub const ALL_VIEWS: [ViewMode; 11] = [
    ViewMode::InstitutionOverview,
    ViewMode::Prisoner,
    ViewMode::Gang,
    ViewMode::Staff,
    ViewMode::Warden,
    ViewMode::Financial,
    ViewMode::EventTimeline,
    ViewMode::SocialNetwork,
    ViewMode::Map,
    ViewMode::IncidentReports,
    ViewMode::DecisionAudit,
];

/// Visible playback modes in toolbar order. Event-driven playback is included
/// so the observer can advance only on meaningful engine events.
const PLAYBACK_BUTTONS: [(TimeSpeed, &str); 5] = [
    (TimeSpeed::Paused, "Pause"),
    (TimeSpeed::RealTime, "Real"),
    (TimeSpeed::FastForward, "FF"),
    (TimeSpeed::Extreme, "Extreme"),
    (TimeSpeed::EventDriven, "Event"),
];

/// Short tab label for an observation view (shared display model).
#[must_use]
pub fn view_label(view: ViewMode) -> &'static str {
    view.label()
}

/// Everything a screen needs. Rendering is read-only over the snapshot; any
/// user intent is appended to `commands` and applied later by the App.
pub struct RenderCtx<'a> {
    pub snapshot: &'a data::Snapshot,
    pub state: &'a mut ObserverState,
    pub commands: &'a mut Vec<Command>,
}

impl RenderCtx<'_> {
    /// Queue a validated control command.
    pub fn emit(&mut self, command: Command) {
        self.commands.push(command);
    }
}

/// Search widget shared by views that filter the global observer query.
///
/// Rendering the field before returning the lowercased query avoids the
/// one-frame lag that happens when the query is captured before `TextEdit`
/// mutates `ctx.state.search`.
pub fn search_field(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>, hint: &str) -> String {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut ctx.state.search)
                .desired_width(180.0)
                .hint_text(hint),
        );
        if !ctx.state.search.is_empty() && ui.small_button("Clear").clicked() {
            ctx.state.search.clear();
        }
    });
    ctx.state.search.to_lowercase()
}

/// A titled, lightly framed group. Keeps all views visually consistent.
pub fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.add(egui::Label::new(
            egui::RichText::new(title).color(theme::ACCENT).strong(),
        ));
        ui.separator();
        add(ui);
    });
}

/// Draw the compact navigation row: eleven view buttons plus visible playback
/// controls. Save/Load/Settings/Menu live in the shell top bar, so raw paths
/// never clutter this strip.
pub fn toolbar(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    egui::Panel::top("alc_gui_view_nav").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::Label::new(
                egui::RichText::new("Views:").small().color(theme::MUTED),
            ));
            for view in ViewMode::all() {
                let selected = ctx.state.view == view;
                let button = egui::Button::new(view_label(view))
                    .small()
                    .selected(selected);
                if ui.add(button).clicked() {
                    ctx.emit(Command::SetView(view));
                }
            }
        });

        ui.horizontal_wrapped(|ui| {
            let speed = ctx.snapshot.speed;
            ui.add(egui::Label::new(
                egui::RichText::new("Playback:").small().color(theme::MUTED),
            ));
            for (s, label) in PLAYBACK_BUTTONS {
                if ui
                    .add(egui::Button::new(label).small().selected(speed == s))
                    .clicked()
                {
                    ctx.emit(Command::SetSpeed(s));
                }
            }
            if ui.small_button("Step").clicked() {
                ctx.emit(Command::StepTick);
            }
            if ui.small_button("+1 Day").clicked() {
                ctx.emit(Command::OneDay);
            }
            ui.separator();
            ui.add(egui::Label::new(
                egui::RichText::new("Autosave:").small().color(theme::MUTED),
            ));
            let days = ctx.state.autosave_days;
            if ui.small_button("−").clicked() && days > 0 {
                ctx.emit(Command::SetAutosave(days - 1));
            }
            ui.label(format!("{days} d"));
            if ui.small_button("+").clicked() && days < 365 {
                ctx.emit(Command::SetAutosave(days + 1));
            }
        });
    });
}

/// Draw the full detail of a structured incident, including its cause,
/// consequences and response actions.
pub fn incident_detail(
    ui: &mut egui::Ui,
    record: &sumo::IncidentRecord,
    names: &HashMap<u64, String>,
) {
    for (label, value) in data::incident_detail_rows(record, names) {
        theme::kv(ui, &label, value);
    }
    theme::kv(
        ui,
        "Consequence",
        record
            .consequence
            .map(|id| format!("incident #{id}"))
            .unwrap_or_else(|| "none recorded".into()),
    );
    for link in &record.causal_chain {
        ui.add(egui::Label::new(
            egui::RichText::new(format!(
                "↳ {} #{}: {}",
                if link.is_incident {
                    "incident"
                } else {
                    "event"
                },
                link.source_id,
                link.description
            ))
            .color(theme::MUTED)
            .small(),
        ));
    }
    for action in &record.response_actions {
        ui.add(egui::Label::new(
            egui::RichText::new(format!(
                "• {:?} by {:?} at {} — {} ({})",
                action.action,
                action.responder,
                action.at,
                action.description,
                if action.success { "ok" } else { "failed" }
            ))
            .small(),
        ));
    }
}

/// Draw the cause/validation/outcome detail of a decision audit record.
pub fn decision_detail(ui: &mut egui::Ui, record: &sumo::DecisionAuditRecord) {
    for (label, value) in data::decision_detail_rows(record) {
        if label == "Validation" {
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(
                    egui::RichText::new(&label).color(theme::MUTED),
                ));
                ui.add(egui::Label::new(
                    egui::RichText::new(&value)
                        .color(theme::validation_colour(&value))
                        .strong(),
                ));
            });
        } else {
            theme::kv(ui, &label, value);
        }
    }
}

/// Draw a world event with its causal notes (the event-level audit trail).
pub fn event_detail(ui: &mut egui::Ui, event: &sumo::WorldEvent) {
    theme::kv(ui, "ID / time", format!("#{} · {}", event.id, event.at));
    theme::kv(ui, "Kind", format!("{:?}", event.kind));
    ui.label(&event.summary);
    if !event.actors.is_empty() {
        theme::kv(ui, "Actors", format!("{:?}", event.actors));
    }
    if event.causes.is_empty() {
        ui.add(egui::Label::new(
            egui::RichText::new("No causal notes recorded").color(theme::MUTED),
        ));
    } else {
        for cause in &event.causes {
            ui.add(egui::Label::new(
                egui::RichText::new(format!("↳ {cause}")).small(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_controls_expose_event_driven_mode() {
        assert_eq!(PLAYBACK_BUTTONS.len(), 5);
        assert!(
            PLAYBACK_BUTTONS
                .iter()
                .any(|(speed, label)| *speed == TimeSpeed::EventDriven && *label == "Event")
        );
        let labels: Vec<_> = PLAYBACK_BUTTONS.iter().map(|(_, l)| *l).collect();
        let mut unique = labels.clone();
        unique.dedup();
        assert_eq!(unique, labels);
    }
}
