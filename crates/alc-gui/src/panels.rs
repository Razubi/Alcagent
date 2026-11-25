//! Shared panels and small rendering helpers used by the observation views.

use crate::{
    app::{Command, ObserverState},
    data, theme, ViewMode,
};
use alc_core::TimeSpeed;
use eframe::egui;
use std::collections::HashMap;

/// All eleven observation views, in tab order.
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

/// Short tab label for an observation view.
#[must_use]
pub fn view_label(view: ViewMode) -> &'static str {
    match view {
        ViewMode::InstitutionOverview => "Overview",
        ViewMode::Prisoner => "Prisoners",
        ViewMode::Gang => "Gangs",
        ViewMode::Staff => "Staff",
        ViewMode::Warden => "Warden",
        ViewMode::Financial => "Financial",
        ViewMode::EventTimeline => "Timeline",
        ViewMode::SocialNetwork => "Network",
        ViewMode::Map => "Map",
        ViewMode::IncidentReports => "Incidents",
        ViewMode::DecisionAudit => "Audit",
    }
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

/// Draw the persistent top toolbar: status readout, view tabs, playback
/// controls and save/load path controls.
pub fn toolbar(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    egui::Panel::top("alc_gui_toolbar").show(ui, |ui| {
        let obs = &ctx.snapshot.observation;
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::Label::new(
                egui::RichText::new("ALCAGENT · LIVING PRISON").strong().color(theme::ACCENT),
            ));
            ui.separator();
            ui.label(format!(
                "{} · Day {} · Pop {}/{} · Guards {} · Gangs {}",
                obs.at, obs.days_simulated, obs.population, obs.capacity, obs.guards, obs.gangs
            ));
            ui.separator();
            ui.label(format!("${:.2} · Health {:.0}%", obs.cash_major, obs.economy_health * 100.0));
            if obs.lockdown {
                ui.add(egui::Label::new(egui::RichText::new("LOCKDOWN").color(theme::LOCKED).strong()));
            }
            if let Some(msg) = &ctx.state.status {
                ui.separator();
                ui.add(egui::Label::new(egui::RichText::new(msg).color(theme::MUTED)));
            }
        });

        ui.horizontal_wrapped(|ui| {
            for view in ALL_VIEWS {
                let selected = ctx.state.view == view;
                let button = egui::Button::new(view_label(view)).selected(selected);
                if ui.add(button).clicked() {
                    ctx.emit(Command::SetView(view));
                }
            }
        });

        ui.horizontal_wrapped(|ui| {
            let speed = ctx.snapshot.speed;
            ui.label("Playback:");
            for (s, label) in [
                (TimeSpeed::Paused, "Pause"),
                (TimeSpeed::RealTime, "Real"),
                (TimeSpeed::FastForward, "FF"),
                (TimeSpeed::Extreme, "Extreme"),
            ] {
                if ui.add(egui::Button::new(label).selected(speed == s)).clicked() {
                    ctx.emit(Command::SetSpeed(s));
                }
            }
            if ui.button("Step 1 tick").clicked() {
                ctx.emit(Command::StepTick);
            }
            if ui.button("+1 Day").clicked() {
                ctx.emit(Command::OneDay);
            }
            ui.separator();
            ui.label("Save/Load:");
            ui.add(
                egui::TextEdit::singleline(&mut ctx.state.save_input)
                    .desired_width(170.0)
                    .hint_text("save directory"),
            );
            if ui.button("Save").clicked() {
                ctx.emit(Command::Save(ctx.state.save_input.clone()));
            }
            if ui.button("Load").clicked() {
                ctx.emit(Command::Load(ctx.state.save_input.clone()));
            }
            ui.separator();
            ui.label("Autosave days:");
            let mut days = ctx.state.autosave_days;
            if ui.add(egui::DragValue::new(&mut days).range(0..=365)).changed() {
                ctx.emit(Command::SetAutosave(days));
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
                if link.is_incident { "incident" } else { "event" },
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
        ui.add(egui::Label::new(egui::RichText::new("No causal notes recorded").color(theme::MUTED)));
    } else {
        for cause in &event.causes {
            ui.add(egui::Label::new(egui::RichText::new(format!("↳ {cause}")).small()));
        }
    }
}
