//! The eleven observation views. Each `show_*` function renders a read-only
//! snapshot and only queues [`Command`]s through [`panels::RenderCtx`].

use crate::{
    ViewMode,
    app::Command,
    data, map_view,
    panels::{self, RenderCtx},
    theme,
};
use eframe::egui::{self, Color32, FontId};
use std::collections::{HashMap, HashSet};

/// Dispatch a frame to the active observation view.
pub fn show(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    match ctx.state.view {
        ViewMode::InstitutionOverview => overview(ui, ctx),
        ViewMode::Prisoner => prisoner(ui, ctx),
        ViewMode::Gang => gang(ui, ctx),
        ViewMode::Staff => staff(ui, ctx),
        ViewMode::Warden => warden(ui, ctx),
        ViewMode::Financial => financial(ui, ctx),
        ViewMode::EventTimeline => timeline(ui, ctx),
        ViewMode::SocialNetwork => social_network(ui, ctx),
        ViewMode::Map => map_view::show(ui, ctx),
        ViewMode::IncidentReports => incidents(ui, ctx),
        ViewMode::DecisionAudit => audit(ui, ctx),
    }
}

fn columns<'a>(
    ui: &mut egui::Ui,
    ctx: &mut RenderCtx<'a>,
    left: impl FnOnce(&mut egui::Ui, &mut RenderCtx<'a>),
    right: impl FnOnce(&mut egui::Ui, &mut RenderCtx<'a>),
) {
    ui.columns(2, |cols| {
        left(&mut cols[0], ctx);
        right(&mut cols[1], ctx);
    });
}

fn names(snap: &data::Snapshot) -> HashMap<u64, String> {
    snap.prisoners
        .iter()
        .map(|p| (p.id.raw(), p.name.clone()))
        .collect()
}

const POLICY_STEP: f32 = 0.05;
const REHAB_FUNDING_STEP: f32 = 0.1;
const SEVERITY_STEP: f32 = 0.05;
const POST_RELEASE_SUCCESS_RISK: f32 = 0.10;
const POST_RELEASE_FOLLOW_UP_RISK: f32 = 0.80;

/// Next value for a discrete +/- control, clamped to an inclusive range.
fn stepped_value(value: f32, delta: f32, min: f32, max: f32) -> f32 {
    (value + delta).clamp(min, max)
}

/// Visible minus/plus buttons with a fixed-precision readout. No sliders, no
/// drag fields and no hidden keybindings: every change is an explicit click.
fn step_buttons(
    ui: &mut egui::Ui,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    suffix: &str,
    mut emit: impl FnMut(f32),
) {
    let down = stepped_value(value, -step, min, max);
    if ui.small_button("−").clicked() && down != value {
        emit(down);
    }
    ui.label(format!("{value:.2}{suffix}"));
    let up = stepped_value(value, step, min, max);
    if ui.small_button("+").clicked() && up != value {
        emit(up);
    }
}

/// Stable display label for a typed parole status (no debug-string round trip).
fn parole_status_label(status: sumo::ParoleStatus) -> &'static str {
    use sumo::ParoleStatus as P;
    match status {
        P::None => "none",
        P::Scheduled => "scheduled",
        P::InHearing => "in hearing",
        P::Granted => "granted",
        P::Denied => "denied",
    }
}

/// Compact label for a contraband case row in the prisoner detail panel.
fn contraband_case_label(case: &sumo::ContrabandCaseSummary) -> String {
    format!(
        "#{} {} · sev {:.0}% · {}",
        case.case_id,
        case.item.as_deref().unwrap_or("Unknown"),
        case.severity * 100.0,
        if case.resolved { "resolved" } else { "open" }
    )
}

/// Coarse, observer-initiated follow-up outcome routed through the validated
/// `RecordPostRelease` command. The button choice supplies the risk/plan pair;
/// the observer does not have free-form simulation-state writes here.
fn post_release_record_command(row: &sumo::PostReleaseSummary, successful: bool) -> Command {
    let (risk, plan) = if successful {
        (POST_RELEASE_SUCCESS_RISK, true)
    } else {
        (POST_RELEASE_FOLLOW_UP_RISK, false)
    };
    Command::RecordPostRelease {
        prisoner: row.prisoner_id,
        release_type: row.release_type.clone(),
        risk,
        plan,
    }
}

// ---------------------------------------------------------------------------
// 1 — Institution overview
// ---------------------------------------------------------------------------

fn overview(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    let obs = &snap.observation;
    ui.heading("Institution Overview");
    columns(
        ui,
        ctx,
        |ui, _ctx| {
            panels::section(ui, "Vital signs", |ui| {
                theme::kv(ui, "Simulation time", obs.at.to_string());
                theme::kv(ui, "Days simulated", obs.days_simulated.to_string());
                theme::kv(
                    ui,
                    "Population",
                    format!("{} / {} capacity", obs.population, obs.capacity),
                );
                theme::kv(
                    ui,
                    "Guards / Facilities",
                    format!("{} / {}", obs.guards, obs.facilities),
                );
                theme::kv(
                    ui,
                    "Active gangs",
                    format!("{} ({} members)", obs.gangs, obs.gang_members),
                );
                theme::kv(ui, "Warden", &obs.warden_name);
                theme::kv(
                    ui,
                    "Philosophy",
                    data::short_truncate(&obs.warden_philosophy, 52),
                );
                theme::kv(ui, "Policy band", format!("{:?}", snap.policy.policy));
                theme::metric_bar(ui, "Security", snap.policy.security_level, false);
                theme::metric_bar(ui, "Privileges", snap.policy.privileges_level, true);
                theme::metric_bar(ui, "Economy health", snap.budget.health, true);
                theme::kv(
                    ui,
                    "Lockdown / Intake",
                    format!("{} / {}", snap.policy.lockdown, snap.policy.intake_paused),
                );
                theme::kv(
                    ui,
                    "Invariant violations",
                    snap.violations.len().to_string(),
                );
            });
            panels::section(ui, "Activity since start", |ui| {
                for (label, value) in data::stats_dashboard_rows(&snap.stats) {
                    theme::kv(ui, label, value);
                }
            });
        },
        |ui, ctx| {
            panels::section(ui, "Recent events", |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("overview_events")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for event in snap.events.iter().rev().take(40) {
                            ui.add(egui::Label::new(
                                egui::RichText::new(format!(
                                    "[{}] {:?} — {}",
                                    event.at,
                                    event.kind,
                                    data::short_truncate(&event.summary, 64)
                                ))
                                .small(),
                            ));
                        }
                    });
            });
            panels::section(ui, "Post-release tracking", |ui| {
                ui.add(egui::Label::new(
                    egui::RichText::new(
                        "Recorded reintegration outcomes. Buttons update a tracked case through the validated command path.",
                    )
                    .small()
                    .color(theme::MUTED),
                ));
                if snap.post_release.is_empty() {
                    ui.label("No post-release outcomes recorded.");
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("post_release_tracking")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        egui::Grid::new("post_release_grid")
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label("Released");
                                ui.label("Type");
                                ui.label("Risk / plan");
                                ui.label("Outcome");
                                ui.label("Record");
                                ui.end_row();
                                for row in &snap.post_release {
                                    let name = row
                                        .prisoner_name
                                        .clone()
                                        .unwrap_or_else(|| format!("#{}", row.prisoner_id.raw()));
                                    ui.label(format!(
                                        "{} · {}",
                                        data::short_truncate(&name, 16),
                                        row.released_at.map_or_else(
                                            || "time unknown".to_string(),
                                            |at| at.to_string()
                                        )
                                    ));
                                    ui.label(&row.release_type);
                                    ui.label(format!(
                                        "{} / {}",
                                        row.recidivism_risk
                                            .map_or_else(|| "—".to_string(), |r| format!("{r:.2}")),
                                        row.has_reintegration_plan.map_or_else(
                                            || "unknown".to_string(),
                                            |p| if p {
                                                "yes".to_string()
                                            } else {
                                                "no".to_string()
                                            },
                                        )
                                    ));
                                    ui.label(if row.follow_up_required {
                                        "follow-up"
                                    } else {
                                        "successful"
                                    });
                                    ui.horizontal(|ui| {
                                        if ui.small_button("Successful").clicked() {
                                            ctx.emit(post_release_record_command(row, true));
                                        }
                                        if ui.small_button("Follow-up").clicked() {
                                            ctx.emit(post_release_record_command(row, false));
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });
            });
            panels::section(ui, "Leaderboard (reputation)", |ui| {
                let mut board: Vec<(String, f32)> = snap
                    .prisoners
                    .iter()
                    .map(|p| (p.name.clone(), p.reputation))
                    .collect();
                board.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                for (rank, name, reputation) in data::leaderboard_table(&board, 8) {
                    theme::kv(ui, &format!("#{rank} {name}"), format!("{reputation:.2}"));
                }
            });
            panels::section(ui, "Facilities", |ui| {
                egui::Grid::new("overview_facilities")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("ID");
                        ui.label("Kind");
                        ui.label("Wing");
                        ui.label("Cap");
                        ui.label("Used");
                        ui.label("Cond");
                        ui.end_row();
                        for f in &snap.facilities {
                            ui.label(f.id.to_string());
                            ui.label(f.kind.display());
                            ui.label(f.wing.to_string());
                            ui.label(f.capacity.to_string());
                            ui.add(egui::ProgressBar::new(f.utilisation()).desired_width(70.0));
                            ui.add(
                                egui::ProgressBar::new(f.condition)
                                    .desired_width(70.0)
                                    .fill(theme::good_colour(f.condition)),
                            );
                            ui.end_row();
                        }
                    });
            });
        },
    );
}

// ---------------------------------------------------------------------------
// 2 — Prisoner profiles
// ---------------------------------------------------------------------------

/// UI state for one parole control: enabled flag plus an explanatory tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParoleButtonState {
    enabled: bool,
    tip: &'static str,
}

/// Pure gating for the four parole buttons, newest state first:
/// `[schedule, grant, deny, release]`.
fn parole_button_states(
    due: bool,
    status: Option<sumo::ParoleStatus>,
    has_conditions: bool,
    released: bool,
) -> [ParoleButtonState; 4] {
    use sumo::ParoleStatus as P;
    let schedule = if released {
        ParoleButtonState {
            enabled: false,
            tip: "Prisoner has already been released",
        }
    } else if due && status.is_none() {
        ParoleButtonState {
            enabled: true,
            tip: "Schedule a hearing for this eligible prisoner",
        }
    } else {
        ParoleButtonState {
            enabled: false,
            tip: if status.is_some() {
                "A parole record already exists for this prisoner"
            } else {
                "Prisoner is not currently eligible for parole review"
            },
        }
    };
    // The engine opens a hearing automatically when a grant/deny is recorded
    // for an eligible prisoner with no prior record, so `due` alone is enough.
    let decision_open = !released
        && (matches!(status, Some(P::Scheduled | P::InHearing)) || (due && status.is_none()));
    let decision_tip = if released {
        "Prisoner has already been released"
    } else if matches!(status, Some(P::Granted | P::Denied)) {
        "A terminal parole decision has already been recorded"
    } else if decision_open {
        "Record a decision for this eligible prisoner"
    } else {
        "Prisoner is not currently eligible for a parole decision"
    };
    let grant = ParoleButtonState {
        enabled: decision_open,
        tip: decision_tip,
    };
    let deny = grant;
    let release = if released {
        ParoleButtonState {
            enabled: false,
            tip: "Parole release has already been executed",
        }
    } else if status == Some(P::Granted) && has_conditions {
        ParoleButtonState {
            enabled: true,
            tip: "Execute release with the recorded parole conditions",
        }
    } else {
        ParoleButtonState {
            enabled: false,
            tip: if status == Some(P::Granted) {
                "Granted parole has no recorded conditions yet"
            } else {
                "Parole must be granted before release"
            },
        }
    };
    [schedule, grant, deny, release]
}

fn prisoner(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    columns(
        ui,
        ctx,
        |ui, ctx| {
            let q = panels::search_field(ui, ctx, "Search prisoners");
            let list: Vec<_> = snap
                .prisoners
                .iter()
                .filter(|p| {
                    q.is_empty()
                        || p.name.to_lowercase().contains(&q)
                        || p.id.raw().to_string().contains(&q)
                })
                .collect();
            egui::ScrollArea::vertical()
                .id_salt("prisoner_list")
                .show(ui, |ui| {
                    for p in &list {
                        let selected = ctx.state.selected_prisoner == Some(p.id);
                        if ui
                            .selectable_label(
                                selected,
                                format!(
                                    "{} · {} [{}]",
                                    p.id.raw(),
                                    data::short_truncate(&p.name, 18),
                                    data::short_class(p.classification)
                                ),
                            )
                            .clicked()
                        {
                            ctx.state.selected_prisoner = Some(p.id);
                        }
                    }
                });
        },
        |ui, ctx| {
            let Some(id) = ctx.state.selected_prisoner else {
                ui.label("Select a prisoner to inspect their profile, needs, rehab and schedule.");
                return;
            };
            let Some(p) = snap.prisoners.iter().find(|p| p.id == id) else {
                ui.label("Selected prisoner is no longer in custody.");
                return;
            };
            egui::ScrollArea::vertical().id_salt("prisoner_detail").show(ui, |ui| {
                ui.heading(data::short_truncate(&p.name, 32));
                panels::section(ui, "Profile", |ui| {
                    theme::kv(ui, "ID / Age", format!("{} / {}", p.id.raw(), p.age));
                    theme::kv(ui, "Class", format!("{:?}", p.classification));
                    theme::kv(ui, "Sentence", format!("{} served / {} total ({} remaining)", p.sentence_served_days, p.sentence_days, p.sentence_days.saturating_sub(p.sentence_served_days)));
                    theme::kv(ui, "Gang", p.gang_affiliation.map_or_else(|| "unaffiliated".into(), |g| format!("#{g}")));
                    theme::kv(ui, "Reputation", format!("{:.2}", p.reputation));
                    theme::kv(
                        ui,
                        "Parole",
                        snap.selected_parole_status
                            .map(parole_status_label)
                            .unwrap_or("none")
                            .to_string(),
                    );
                    theme::metric_bar(ui, "Health", p.condition.health, true);
                    theme::metric_bar(ui, "Mental health", p.condition.mental_health, true);
                    theme::metric_bar(ui, "Hope", p.condition.hope, true);
                    if let Some(agent) = &snap.selected_agent {
                        theme::kv(ui, "Intention", agent.current_intention.clone().unwrap_or_else(|| "none".into()));
                        theme::kv(ui, "Social drive", format!("{:.2}", agent.social_drive));
                    }
                });
                panels::section(ui, "Personality", |ui| {
                    let t = &p.traits;
                    theme::metric_bar(ui, "Intelligence", t.intelligence, true);
                    theme::metric_bar(ui, "Stability", t.emotional_stability, true);
                    theme::metric_bar(ui, "Impulsiveness", t.impulsiveness, false);
                    theme::metric_bar(ui, "Aggression", t.aggression, false);
                    theme::metric_bar(ui, "Sociability", t.sociability, true);
                    theme::metric_bar(ui, "Loyalty", t.loyalty, true);
                    theme::metric_bar(ui, "Manipulation", t.manipulation, false);
                    theme::metric_bar(ui, "Risk tolerance", t.risk_tolerance, false);
                    theme::metric_bar(ui, "Discipline", t.discipline, true);
                    theme::metric_bar(ui, "Empathy", t.empathy, true);
                    theme::metric_bar(ui, "Ambition", t.ambition, true);
                });
                panels::section(ui, "Needs (deficit pressure)", |ui| {
                    for (label, value) in data::need_rows(&p.needs) {
                        theme::metric_bar(ui, label, value, false);
                    }
                });
                panels::section(ui, "Goals", |ui| {
                    if p.goals.is_empty() {
                        ui.label("No active goals.");
                    }
                    for goal in &p.goals {
                        theme::kv(ui, &format!("{:?}", goal.kind), format!("pro {:.2} urg {:.2} · {}", goal.progress, goal.urgency, data::short_truncate(&goal.description, 54)));
                    }
                });
                panels::section(ui, "Daily schedule", |ui| {
                    theme::kv(ui, "Wing", p.schedule.wing.to_string());
                    theme::kv(ui, "Work", p.schedule.work_assignment.clone().unwrap_or_else(|| "none".into()));
                    theme::kv(ui, "Education", p.schedule.education.to_string());
                    if let Some(timeline) = &snap.selected_schedule {
                        for block in &timeline.blocks {
                            let marker = if block.current { ">" } else { " " };
                            let status = if block.cancelled {
                                format!("CANCELLED ({})", block.reason.as_deref().unwrap_or("unknown"))
                            } else {
                                block.activity.clone()
                            };
                            ui.add(egui::Label::new(egui::RichText::new(format!(
                                "{marker} {:02}:{:02}-{:02}:{:02} {} — {}",
                                block.start_minute / 60, block.start_minute % 60,
                                block.end_minute / 60, block.end_minute % 60,
                                block.label, status
                            )).small()));
                        }
                    }
                });
                panels::section(ui, "Rehabilitation", |ui| {
                    if snap.rehab_catalog.is_empty() {
                        ui.label("No programs catalogued.");
                    } else {
                        let idx = ctx.state.rehab_program_index.min(snap.rehab_catalog.len() - 1);
                        egui::ComboBox::from_label("Program")
                            .selected_text(&snap.rehab_catalog[idx].name)
                            .show_ui(ui, |ui| {
                                for (i, program) in snap.rehab_catalog.iter().enumerate() {
                                    ui.selectable_value(&mut ctx.state.rehab_program_index, i, &program.name);
                                }
                            });
                        let key = snap.rehab_catalog[idx].key.clone();
                        let enrolled = snap
                            .prisoners
                            .iter()
                            .find(|p| p.id == id)
                            .is_some_and(|p| p.skills.contains_key(&key));
                        if !enrolled && ui.button("Enroll").clicked() {
                            ctx.emit(Command::EnrollRehab { prisoner: id, key: key.clone() });
                        }
                        if enrolled && ui.button("Drop").clicked() {
                            ctx.emit(Command::DropRehab { prisoner: id, key });
                        }
                    }
                    for row in snap.rehab_rows.iter().filter(|r| r.prisoner_id == id) {
                        theme::kv(ui, &row.program, format!("{:.0}%", row.progress * 100.0));
                    }
                });
                panels::section(ui, "Parole", |ui| {
                    if snap.parole_due.contains(&id) {
                        ui.label("Eligible for parole review.");
                    }
                    if let Some(reason) = snap.selected_parole_reason.as_deref() {
                        theme::kv(ui, "Decision reason", reason.to_string());
                    }
                    if let Some(c) = snap.selected_parole_conditions.as_ref() {
                        ui.label(format!(
                            "Conditions: report {}/mo · {}mo supervision · curfew={} drug={} treatment={} no-contact={}",
                            c.reporting_per_month, c.supervision_months, c.curfew, c.drug_testing, c.treatment_required, c.no_contact_order
                        ));
                    }
                    if snap.selected_parole_released {
                        ui.label("Released from custody on parole.");
                    }
                    let due = snap.parole_due.contains(&id);
                    let status = snap.selected_parole_status;
                    let [schedule, grant, deny, release] = parole_button_states(
                        due,
                        status,
                        snap.selected_parole_conditions.is_some(),
                        snap.selected_parole_released,
                    );
                    if ui
                        .add_enabled(schedule.enabled, egui::Button::new("Schedule hearing"))
                        .on_hover_text(schedule.tip)
                        .clicked()
                    {
                        ctx.emit(Command::StartParole(id));
                    }
                    if ui
                        .add_enabled(grant.enabled, egui::Button::new("Grant"))
                        .on_hover_text(grant.tip)
                        .clicked()
                    {
                        ctx.emit(Command::GrantParole(id));
                    }
                    if ui
                        .add_enabled(deny.enabled, egui::Button::new("Deny"))
                        .on_hover_text(deny.tip)
                        .clicked()
                    {
                        ctx.emit(Command::DenyParole(id, "institutional conduct".to_string()));
                    }
                    if ui
                        .add_enabled(release.enabled, egui::Button::new("Release on parole"))
                        .on_hover_text(release.tip)
                        .clicked()
                    {
                        ctx.emit(Command::ReleaseParole(id));
                    }
                });
                panels::section(ui, "Contraband cases", |ui| {
                    let cases: Vec<_> = snap.contraband.iter().filter(|c| c.holder == Some(id)).collect();
                    if cases.is_empty() {
                        ui.label("No cases recorded.");
                        return;
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("prisoner_contraband_cases")
                        .max_height(140.0)
                        .show(ui, |ui| {
                            for case in &cases {
                                let selected = ctx.state.selected_contraband == Some(case.case_id);
                                if ui
                                    .selectable_label(selected, contraband_case_label(case))
                                    .clicked()
                                {
                                    ctx.state.selected_contraband = Some(case.case_id);
                                }
                            }
                        });
                    if let Some(case_id) = ctx.state.selected_contraband {
                        if let Some(case) = cases.iter().find(|c| c.case_id == case_id) {
                            ui.add(egui::Label::new(
                                egui::RichText::new(data::short_truncate(&case.summary, 120))
                                    .small()
                                    .color(theme::MUTED),
                            ));
                            if case.resolved {
                                ui.label("Case is already resolved.");
                            } else if ui.button("Resolve case").clicked() {
                                ctx.emit(Command::ResolveContraband(case.case_id));
                            }
                        }
                    }
                });
                panels::section(ui, "Memory", |ui| {
                    ui.label(data::short_truncate(&p.memory_summary, 400));
                });
            });
        },
    );
}

// ---------------------------------------------------------------------------
// 3 — Gangs
// ---------------------------------------------------------------------------

fn gang(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    if snap.gangs.is_empty() {
        ui.heading("Gangs");
        ui.label("No gangs have formed yet. Gangs emerge from protection needs, friendships, shared backgrounds and common enemies — advance time and watch the Social Network view.");
        return;
    }
    let name_map = names(snap);
    columns(
        ui,
        ctx,
        |ui, ctx| {
            let q = panels::search_field(ui, ctx, "Search gangs");
            egui::ScrollArea::vertical()
                .id_salt("gang_list")
                .show(ui, |ui| {
                    for g in &snap.gangs {
                        if !q.is_empty()
                            && !g.name.to_lowercase().contains(&q)
                            && !g.id.to_string().contains(&q)
                        {
                            continue;
                        }
                        let selected = ctx.state.selected_gang == Some(g.id);
                        if ui
                            .selectable_label(
                                selected,
                                format!(
                                    "#{} {} · {} members{}",
                                    g.id,
                                    data::short_truncate(&g.name, 16),
                                    g.members.len(),
                                    if g.active { "" } else { " (inactive)" }
                                ),
                            )
                            .clicked()
                        {
                            ctx.state.selected_gang = Some(g.id);
                        }
                    }
                });
        },
        |ui, ctx| {
            let Some(id) = ctx.state.selected_gang else {
                ui.label("Select a gang to inspect its hierarchy and relations.");
                return;
            };
            let Some(g) = snap.gangs.iter().find(|g| g.id == id) else {
                return;
            };
            egui::ScrollArea::vertical()
                .id_salt("gang_detail")
                .show(ui, |ui| {
                    ui.heading(&g.name);
                    for (key, value) in gang_detail_rows(g, &name_map) {
                        theme::kv(ui, &key, value);
                    }
                    panels::section(ui, "Hierarchy", |ui| {
                        egui::Grid::new("gang_hierarchy")
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label("Member");
                                ui.label("Rank");
                                ui.label("Commit");
                                ui.label("Influence");
                                ui.end_row();
                                for m in &g.hierarchy {
                                    ui.label(
                                        name_map
                                            .get(&m.prisoner.raw())
                                            .cloned()
                                            .unwrap_or_else(|| format!("#{}", m.prisoner.raw())),
                                    );
                                    ui.label(format!("{:?}", m.rank));
                                    ui.label(format!("{:.2}", m.commitment));
                                    ui.label(format!("{:.2}", m.influence));
                                    ui.end_row();
                                }
                            });
                    });
                });
        },
    );
}

/// Text rows used by the gang detail screen. Kept pure so tests can verify
/// that pressure, relationship scores and durable history are exposed without
/// needing a live egui context.
fn gang_detail_rows(
    g: &sumo::GangSummary,
    name_map: &HashMap<u64, String>,
) -> Vec<(String, String)> {
    let leader = g.leader.map_or_else(
        || "none".to_string(),
        |id| {
            name_map
                .get(&id.raw())
                .cloned()
                .unwrap_or_else(|| format!("prisoner #{}", id.raw()))
        },
    );
    let rivalries = if g.rivalries.is_empty() {
        "none".to_string()
    } else {
        g.rivalries
            .iter()
            .map(|(id, score)| format!("#{id}:{score:.2}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let alliances = if g.alliances.is_empty() {
        "none".to_string()
    } else {
        g.alliances
            .iter()
            .map(|(id, score)| format!("#{id}:{score:.2}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let scores = if g.relations.is_empty() {
        "none".to_string()
    } else {
        g.relations
            .iter()
            .map(|r| format!("#{} a{:.2}/h{:.2}", r.gang, r.affinity, r.hostility))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let history = if g.history.is_empty() {
        "none".to_string()
    } else {
        g.history
            .iter()
            .rev()
            .take(3)
            .map(|entry| {
                format!(
                    "{}: {} ({})",
                    entry.kind.label(),
                    data::short_truncate(&entry.summary, 48),
                    entry.at
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };

    vec![
        ("Leader".into(), leader),
        ("Members".into(), g.members.len().to_string()),
        ("Culture".into(), g.culture.clone()),
        ("Reputation".into(), format!("{:.2}", g.reputation)),
        ("Resources".into(), format!("{:.2}", g.resources)),
        ("Formed".into(), g.formed_at.to_string()),
        ("Active".into(), g.active.to_string()),
        ("Pressure".into(), format!("{:.2}", g.pressure.total())),
        ("Rivalries".into(), rivalries),
        ("Alliances".into(), alliances),
        ("Scores".into(), scores),
        (
            "Disputes".into(),
            if g.internal_disputes.is_empty() {
                "none".into()
            } else {
                g.internal_disputes.join("; ")
            },
        ),
        ("Goals".into(), g.goals.join(", ")),
        ("History".into(), history),
    ]
}

// ---------------------------------------------------------------------------
// 4 — Staff
// ---------------------------------------------------------------------------

fn staff(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    columns(
        ui,
        ctx,
        |ui, ctx| {
            let q = panels::search_field(ui, ctx, "Filter guards");
            egui::ScrollArea::vertical()
                .id_salt("staff_list")
                .show(ui, |ui| {
                    for g in &snap.guards {
                        if !q.is_empty()
                            && !g.name.to_lowercase().contains(&q)
                            && !g.id.to_string().contains(&q)
                        {
                            continue;
                        }
                        let selected = ctx.state.selected_guard == Some(g.id);
                        if ui
                            .selectable_label(
                                selected,
                                format!(
                                    "#{} {} · {:?} · {:?}",
                                    g.id,
                                    data::short_truncate(&g.name, 14),
                                    g.rank,
                                    g.post
                                ),
                            )
                            .clicked()
                        {
                            ctx.state.selected_guard = Some(g.id);
                        }
                    }
                });
        },
        |ui, ctx| {
            let Some(id) = ctx.state.selected_guard else {
                ui.label("Select a guard to inspect their post, condition and duty log.");
                return;
            };
            let Some(g) = snap.guards.iter().find(|g| g.id == id) else {
                return;
            };
            egui::ScrollArea::vertical()
                .id_salt("staff_detail")
                .show(ui, |ui| {
                    ui.heading(&g.name);
                    theme::kv(ui, "Rank / Post", format!("{:?} / {:?}", g.rank, g.post));
                    theme::kv(
                        ui,
                        "Authority / Shift",
                        format!("{:?} / {}", g.authority, g.shift),
                    );
                    theme::kv(ui, "Current task", &g.current_task);
                    theme::kv(ui, "Report credibility", format!("{:.2}", g.credibility));
                    theme::kv(
                        ui,
                        "Duty record",
                        format!(
                            "{} opinions · {} mistakes · {} violations",
                            g.tracked_opinions, g.mistakes, g.violations
                        ),
                    );
                    if g.off_duty {
                        theme::kv(
                            ui,
                            "Status",
                            format!("off-duty: {}", g.off_duty_activity.label()),
                        );
                    } else {
                        theme::kv(ui, "Status", "on duty");
                    }
                    theme::kv(
                        ui,
                        "Performance / bonds",
                        format!("{:.2} / {}", g.performance, g.bonds.len()),
                    );
                    theme::metric_bar(ui, "Competence", g.competence, true);
                    theme::metric_bar(ui, "Experience", g.experience, true);
                    theme::metric_bar(ui, "Morale", g.morale, true);
                    theme::metric_bar(ui, "Fatigue", g.fatigue, false);
                    theme::metric_bar(ui, "Stress", g.stress, false);
                    theme::metric_bar(ui, "Loyalty", g.loyalty, true);
                    theme::metric_bar(ui, "Integrity", g.integrity, true);
                    panels::section(ui, "Needs (higher = more unmet)", |ui| {
                        theme::metric_bar(ui, "Rest", g.needs.rest, false);
                        theme::metric_bar(ui, "Social", g.needs.social, false);
                        theme::metric_bar(ui, "Safety", g.needs.safety, false);
                        theme::metric_bar(ui, "Purpose", g.needs.purpose, false);
                        theme::metric_bar(ui, "Hygiene", g.needs.hygiene, false);
                        theme::metric_bar(ui, "Stimulation", g.needs.stimulation, false);
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Promote").clicked() {
                            ctx.emit(Command::WardenAction(
                                alc_warden::WardenAction::PromoteStaff { guard: g.id },
                            ));
                        }
                        if ui.button("Dismiss").clicked() {
                            ctx.emit(Command::WardenAction(
                                alc_warden::WardenAction::DismissGuard { guard: g.id },
                            ));
                        }
                    });
                    panels::section(ui, "Recent duty log", |ui| {
                        for entry in g.history.iter().rev().take(12) {
                            ui.add(egui::Label::new(egui::RichText::new(entry).small()));
                        }
                        if g.history.is_empty() {
                            ui.label("No notable duty events yet.");
                        }
                    });
                });
        },
    );
}

// ---------------------------------------------------------------------------
// 5 — Warden (policy, construction and staff controls)
// ---------------------------------------------------------------------------

fn warden(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    columns(
        ui,
        ctx,
        |ui, ctx| {
            let w = &snap.warden;
            ui.heading(&w.name);
            theme::kv(ui, "Philosophy", &w.philosophy);
            theme::metric_bar(ui, "Attention", w.attention, true);
            theme::kv(ui, "Last decision", w.last_decision_at.to_string());
            panels::section(ui, "AI runtime", |ui| {
                let ai = &snap.ai_runtime;
                theme::kv(
                    ui,
                    "Provider / model",
                    format!("{} / {}", ai.provider, ai.model),
                );
                theme::kv(
                    ui,
                    "Tokens today",
                    format!("{} / {}", ai.tokens_consumed_today, ai.max_tokens_per_day),
                );
                theme::kv(
                    ui,
                    "Cache",
                    format!(
                        "{} entries · {} hits · {} misses",
                        ai.cache_entries, ai.cache_hits, ai.cache_misses
                    ),
                );
                theme::kv(
                    ui,
                    "Control",
                    format!(
                        "{} retries · {} rate-limited · {} failures · batch {}",
                        ai.retries, ai.rate_limited, ai.provider_failures, ai.max_batch_size
                    ),
                );
            });
            panels::section(ui, "Leadership traits", |ui| {
                let t = &w.traits;
                for (label, value) in [
                    ("Strictness", t.strictness),
                    ("Compassion", t.compassion),
                    ("Political ambition", t.political_ambition),
                    ("Financial discipline", t.financial_discipline),
                    ("Risk tolerance", t.risk_tolerance),
                    ("Reform orientation", t.reform_orientation),
                    ("Authoritarian", t.authoritarian_tendencies),
                    ("Staff loyalty", t.staff_loyalty),
                    ("Corruption risk", t.corruption_susceptibility),
                    ("Reputation focus", t.reputation_concerns),
                    ("Competence", t.competence),
                ] {
                    theme::metric_bar(
                        ui,
                        label,
                        value,
                        !matches!(
                            label,
                            "Strictness"
                                | "Political ambition"
                                | "Risk tolerance"
                                | "Authoritarian"
                                | "Corruption risk"
                                | "Reputation focus"
                        ),
                    );
                }
            });
            panels::section(ui, "Pending actions", |ui| {
                if w.pending_actions.is_empty() {
                    ui.label("No pending actions.");
                }
                for action in w.pending_actions.iter().take(12) {
                    ui.label(format!("• {} — {action:?}", action.label()));
                }
            });
            panels::section(ui, "Decision history", |ui| {
                if w.recent_decisions.is_empty() {
                    ui.label("No warden decisions recorded yet.");
                }
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for decision in w.recent_decisions.iter().take(20) {
                            ui.label(format!(
                                "#{} · {} · {} · {} · {}",
                                decision.id,
                                decision.time,
                                decision.source.label(),
                                decision.action.label(),
                                if decision.applied {
                                    "applied"
                                } else {
                                    "deferred"
                                }
                            ));
                            ui.label(format!("   {}", decision.rationale));
                        }
                    });
            });
            panels::section(ui, "Reports inbox", |ui| {
                if w.inbox.is_empty() {
                    ui.label("No credible guard reports delivered.");
                }
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for report in w.inbox.iter().take(50) {
                            ui.label(format!(
                                "{} · {:?} · sev {} · cred {:.2} · {}",
                                report.at,
                                report.post,
                                report.severity,
                                report.credibility,
                                report.summary
                            ));
                        }
                    });
            });
            panels::section(ui, "Quick warden actions", |ui| {
                if ui.button("Hire 1 guard").clicked() {
                    ctx.emit(Command::WardenAction(
                        alc_warden::WardenAction::HireGuards { count: 1 },
                    ));
                }
                if ui.button("Fire 1 guard").clicked() {
                    ctx.emit(Command::WardenAction(
                        alc_warden::WardenAction::FireGuards { count: 1 },
                    ));
                }
                if ui.button("Request funding").clicked() {
                    ctx.emit(Command::WardenAction(
                        alc_warden::WardenAction::RequestFunding { amount_major: 100 },
                    ));
                }
                if ui.button("Reduce expenses").clicked() {
                    ctx.emit(Command::WardenAction(
                        alc_warden::WardenAction::ReduceExpenses,
                    ));
                }
                if ui.button("Negotiate with representatives").clicked() {
                    ctx.emit(Command::WardenAction(
                        alc_warden::WardenAction::NegotiateWithRepresentatives,
                    ));
                }
            });
        },
        |ui, ctx| {
            panels::section(ui, "Policy controls", |ui| {
                ui.horizontal(|ui| {
                    for policy in [
                        sumo::Policy::Normal,
                        sumo::Policy::Enhanced,
                        sumo::Policy::Lockdown,
                    ] {
                        if ui
                            .add(
                                egui::Button::new(format!("{policy:?}"))
                                    .selected(snap.policy.policy == policy),
                            )
                            .clicked()
                        {
                            ctx.emit(Command::SetPolicyBand(policy));
                        }
                    }
                });
                if ui
                    .add(
                        egui::Button::new(format!("Lockdown: {}", snap.policy.lockdown))
                            .selected(snap.policy.lockdown),
                    )
                    .clicked()
                {
                    ctx.emit(Command::SetLockdown(!snap.policy.lockdown));
                }
                if ui
                    .add(
                        egui::Button::new(format!(
                            "Intake: {}",
                            if snap.policy.intake_paused {
                                "paused"
                            } else {
                                "open"
                            }
                        ))
                        .selected(snap.policy.intake_paused),
                    )
                    .clicked()
                {
                    ctx.emit(Command::SetIntakePaused(!snap.policy.intake_paused));
                }
                ui.horizontal(|ui| {
                    ui.label("Security");
                    step_buttons(
                        ui,
                        snap.policy.security_level,
                        0.0,
                        1.0,
                        POLICY_STEP,
                        "",
                        |v| {
                            ctx.emit(Command::SetSecurity(v));
                        },
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Privileges");
                    step_buttons(
                        ui,
                        snap.policy.privileges_level,
                        0.0,
                        1.0,
                        POLICY_STEP,
                        "",
                        |v| ctx.emit(Command::SetPrivileges(v)),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Rehab funding");
                    step_buttons(
                        ui,
                        snap.policy.rehab_funding_multiplier,
                        0.0,
                        3.0,
                        REHAB_FUNDING_STEP,
                        "x",
                        |v| ctx.emit(Command::SetRehabFunding(v)),
                    );
                });
                ui.horizontal(|ui| {
                    for mode in [
                        sumo::PolicyMode::Deterministic,
                        sumo::PolicyMode::Council,
                        sumo::PolicyMode::Shadow,
                    ] {
                        if ui.button(format!("{mode:?}")).clicked() {
                            ctx.emit(Command::SetPolicyMode(mode));
                        }
                    }
                });
            });
            panels::section(ui, "Construction", |ui| {
                let kind_index = ctx
                    .state
                    .construction_kind_index
                    .min(crate::CONSTRUCTION_KINDS.len() - 1);
                egui::ComboBox::from_label("Facility")
                    .selected_text(crate::CONSTRUCTION_KINDS[kind_index].display())
                    .show_ui(ui, |ui| {
                        for (i, kind) in crate::CONSTRUCTION_KINDS.iter().enumerate() {
                            ui.selectable_value(
                                &mut ctx.state.construction_kind_index,
                                i,
                                kind.display(),
                            );
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("Wing");
                    if ui.small_button("-").clicked() {
                        ctx.state.build_wing = ctx.state.build_wing.saturating_sub(1);
                    }
                    ui.label(format!("{}", ctx.state.build_wing));
                    if ui.small_button("+").clicked() {
                        ctx.state.build_wing = (ctx.state.build_wing + 1).min(99);
                    }
                    ui.add_space(12.0);
                    ui.label("Capacity");
                    if ui.small_button("-").clicked() {
                        ctx.state.build_capacity =
                            ctx.state.build_capacity.max(1).saturating_sub(1).max(1);
                    }
                    ui.label(format!("{}", ctx.state.build_capacity));
                    if ui.small_button("+").clicked() {
                        ctx.state.build_capacity = (ctx.state.build_capacity + 1).min(500);
                    }
                });
                if ui.button("Queue construction").clicked() {
                    ctx.emit(Command::EnqueueConstruction {
                        kind: crate::CONSTRUCTION_KINDS[kind_index],
                        wing: ctx.state.build_wing,
                        capacity: ctx.state.build_capacity,
                    });
                }
                egui::ScrollArea::vertical()
                    .id_salt("construction_queue")
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for p in &snap.construction {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "#{} {} W{} {}/{}d ({:?})",
                                    p.id,
                                    p.kind.display(),
                                    p.wing,
                                    p.progress_days,
                                    p.total_days,
                                    p.status
                                ));
                                if ui.button("+1d").clicked() {
                                    ctx.emit(Command::AdvanceConstruction(p.id, 1));
                                }
                                if ui.small_button("Approve").clicked() {
                                    ctx.emit(Command::ApproveConstruction(p.id));
                                }
                                if ui.small_button("Cancel").clicked() {
                                    ctx.emit(Command::CancelConstruction(p.id));
                                }
                                if ui.small_button("Fail").clicked() {
                                    ctx.emit(Command::FailConstruction(p.id));
                                }
                            });
                        }
                        if snap.construction.is_empty() {
                            ui.label("No projects queued.");
                        }
                    });
            });
        },
    );
}

// ---------------------------------------------------------------------------
// 6 — Financial
// ---------------------------------------------------------------------------

fn financial(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    let b = &snap.budget;
    ui.heading("Financial Overview");
    columns(
        ui,
        ctx,
        |ui, _ctx| {
            panels::section(ui, "Treasury", |ui| {
                theme::kv(ui, "Cash on hand", format!("${:.2}", b.cash_major));
                theme::kv(ui, "Health", format!("{:.0}%", b.health * 100.0));
                theme::kv(
                    ui,
                    "Funding / prisoner / day",
                    format!("${:.2}", b.funding_per_prisoner_per_day),
                );
                theme::kv(
                    ui,
                    "Base daily grant",
                    format!("${:.2}", b.base_daily_grant),
                );
                theme::kv(
                    ui,
                    "Population / capacity",
                    format!("{} / {}", b.population, b.capacity),
                );
                theme::kv(ui, "Crowding threshold", b.crowding_threshold.to_string());
                theme::kv(ui, "Runway", format!("{:.0} days", b.runway_days));
                theme::kv(
                    ui,
                    "Warden affordance",
                    format!(
                        "tactical={} strategic={} incidents={}",
                        snap.warden.control.can_decide_tactical,
                        snap.warden.control.can_decide_strategic,
                        snap.warden.control.security_incidents_this_week
                    ),
                );
                theme::kv(
                    ui,
                    "Projected revenue / day",
                    format!("${:.2}", b.projected_daily_revenue),
                );
                theme::kv(
                    ui,
                    "Projected expense / day",
                    format!("${:.2}", b.projected_daily_expense),
                );
                theme::kv(
                    ui,
                    "Trailing daily expense",
                    format!("${:.2}", b.trailing_daily_expense_avg),
                );
            });
            panels::section(ui, "Warnings", |ui| {
                if b.warnings.is_empty() {
                    ui.label("No financial warnings.");
                }
                for warning in &b.warnings {
                    ui.add(egui::Label::new(
                        egui::RichText::new(format!("⚠ {warning}"))
                            .color(theme::severity_colour(0.8)),
                    ));
                }
            });
            panels::section(ui, "Economy summary", |ui| {
                theme::kv(ui, "Cash", format!("${:.2}", snap.economy.cash_major));
                theme::kv(ui, "Health", format!("{:.3}", snap.economy.health));
                theme::kv(ui, "Ledger entries", snap.economy.ledger.len().to_string());
            });
        },
        |ui, _ctx| {
            panels::section(ui, "Budget by account (30d)", |ui| {
                egui::Grid::new("budget_lines")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Account");
                        ui.label("In");
                        ui.label("Out");
                        ui.label("Net");
                        ui.end_row();
                        for line in &b.lines {
                            ui.label(&line.account);
                            ui.label(format!("{:.2}", line.inflow_major));
                            ui.label(format!("{:.2}", line.outflow_major));
                            ui.add(egui::Label::new(
                                egui::RichText::new(format!("{:.2}", line.net_major)).color(
                                    if line.net_major >= 0.0 {
                                        theme::good_colour(1.0)
                                    } else {
                                        theme::severity_colour(1.0)
                                    },
                                ),
                            ));
                            ui.end_row();
                        }
                    });
            });
            panels::section(ui, "Recent ledger", |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("ledger")
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for entry in snap.economy.ledger.iter().rev().take(30) {
                            ui.add(egui::Label::new(
                                egui::RichText::new(format!(
                                    "t={} {:?} {:+.2} — {}",
                                    entry.at_secs,
                                    entry.account,
                                    entry.amount.as_major(),
                                    data::short_truncate(&entry.note, 46)
                                ))
                                .small(),
                            ));
                        }
                    });
            });
        },
    );
}

// ---------------------------------------------------------------------------
// 7 — Event timeline
// ---------------------------------------------------------------------------

fn timeline(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    columns(
        ui,
        ctx,
        |ui, ctx| {
            ui.add(
                egui::TextEdit::singleline(&mut ctx.state.event_kind_filter)
                    .hint_text("Filter by kind or text"),
            );
            let filter = ctx.state.event_kind_filter.to_lowercase();
            egui::ScrollArea::vertical()
                .id_salt("timeline_list")
                .show(ui, |ui| {
                    for event in snap.events.iter().rev() {
                        let kind = format!("{:?}", event.kind);
                        if !filter.is_empty()
                            && !kind.to_lowercase().contains(&filter)
                            && !event.summary.to_lowercase().contains(&filter)
                        {
                            continue;
                        }
                        let selected = ctx.state.selected_event == Some(event.id);
                        if ui
                            .selectable_label(
                                selected,
                                format!(
                                    "[{}] {:<16} {}",
                                    event.at,
                                    data::short_truncate(&kind, 15),
                                    data::short_truncate(&event.summary, 52)
                                ),
                            )
                            .clicked()
                        {
                            ctx.state.selected_event = Some(event.id);
                        }
                    }
                });
        },
        |ui, ctx| {
            if let Some(id) = ctx.state.selected_event {
                if let Some(event) = snap.events.iter().find(|e| e.id == id) {
                    ui.heading("Event detail");
                    panels::event_detail(ui, event);
                } else {
                    ui.label("Event has aged out of the retained window.");
                }
            } else {
                ui.label("Select an event to inspect its kind, actors and causal notes.");
            }
        },
    );
}

// ---------------------------------------------------------------------------
// 8 — Social network
// ---------------------------------------------------------------------------

/// Deterministic grid placement: stable for a given prisoner order and canvas,
/// and readable when the ring would collapse into overlapping labels.
const SOCIAL_MAX_EDGES_DRAWN: usize = 400;
const SOCIAL_MAX_LABELS: usize = 24;
const SOCIAL_LIST_LIMIT: usize = 200;

fn social_cell(index: usize, count: usize, rect: egui::Rect) -> egui::Pos2 {
    if count == 0 {
        return rect.center();
    }
    let cols = (count as f32).sqrt().ceil() as usize;
    let rows = (count + cols - 1) / cols;
    let col = index % cols;
    let row = index / cols;
    let margin = 24.0;
    let width = (rect.width() - margin * 2.0).max(1.0);
    let height = (rect.height() - margin * 2.0).max(1.0);
    let x = if cols == 1 {
        0.5
    } else {
        col as f32 / (cols - 1) as f32
    };
    let y = if rows == 1 {
        0.5
    } else {
        row as f32 / (rows - 1) as f32
    };
    rect.min + egui::vec2(margin + x * width, margin + y * height)
}

/// Labels are capped and biased toward the selected prisoner and its direct
/// neighbours; with no selection a deterministic stride sample is shown.
fn social_label_ids(
    prisoners: &[sumo::PrisonerSummary],
    edges: &[sumo::SocialEdge],
    selected: Option<u64>,
    max: usize,
) -> HashSet<u64> {
    let mut out = HashSet::new();
    if max == 0 {
        return out;
    }
    if let Some(selected) = selected {
        out.insert(selected);
        for edge in edges {
            if out.len() >= max {
                break;
            }
            let a = edge.prisoner_a.raw();
            let b = edge.prisoner_b.raw();
            if a == selected {
                out.insert(b);
            } else if b == selected {
                out.insert(a);
            }
        }
    }
    if out.is_empty() {
        let stride = (prisoners.len() + max - 1) / max;
        for (i, prisoner) in prisoners.iter().enumerate() {
            if out.len() >= max {
                break;
            }
            if i % stride == 0 {
                out.insert(prisoner.id.raw());
            }
        }
    }
    out
}

fn social_network(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(
            (ui.available_width() - 340.0).max(260.0),
            (ui.available_height() - 120.0).max(220.0),
        ),
        egui::Sense::click(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 4.0, theme::MAP_BG);
    let n = snap.prisoners.len();
    let mut positions = HashMap::new();
    for (i, p) in snap.prisoners.iter().enumerate() {
        positions.insert(p.id.raw(), social_cell(i, n, rect));
    }
    for edge in snap.social_edges.iter().take(SOCIAL_MAX_EDGES_DRAWN) {
        let Some(&a) = positions.get(&edge.prisoner_a.raw()) else {
            continue;
        };
        let Some(&b) = positions.get(&edge.prisoner_b.raw()) else {
            continue;
        };
        let colour = if edge.threat_score > edge.bond_score {
            Color32::from_rgba_unmultiplied(255, 90, 90, 150)
        } else {
            Color32::from_rgba_unmultiplied(110, 220, 160, 130)
        };
        painter.line_segment(
            [a, b],
            egui::Stroke::new(1.0 + edge.bond_score * 2.5, colour),
        );
    }
    let labels = social_label_ids(
        &snap.prisoners,
        &snap.social_edges,
        ctx.state.selected_prisoner.map(|id| id.raw()),
        SOCIAL_MAX_LABELS,
    );
    for p in &snap.prisoners {
        let Some(&pos) = positions.get(&p.id.raw()) else {
            continue;
        };
        let selected = ctx.state.selected_prisoner == Some(p.id);
        painter.circle_filled(
            pos,
            if selected { 9.0 } else { 6.0 },
            if selected {
                theme::ACCENT
            } else {
                theme::PRISONER
            },
        );
        if labels.contains(&p.id.raw()) {
            painter.text(
                pos + egui::vec2(8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                data::short_truncate(&p.name, 12),
                FontId::proportional(9.0),
                if selected {
                    theme::ACCENT
                } else {
                    Color32::WHITE
                },
            );
        }
    }
    if response.clicked() {
        if let Some(pos) = response.hover_pos() {
            let mut best = (f32::MAX, None);
            for (id, p) in &positions {
                let d = (*p).distance(pos);
                if d < best.0 {
                    best = (d, Some(*id));
                }
            }
            if best.0 < 14.0 {
                if let Some(id) = best.1 {
                    ctx.state.selected_prisoner = snap
                        .prisoners
                        .iter()
                        .find(|p| p.id.raw() == id)
                        .map(|p| p.id);
                }
            }
        }
    }

    ui.separator();
    let name_map = names(snap);
    let q = panels::search_field(ui, ctx, "Filter edges and roles");
    let hidden_edges = snap
        .social_edges
        .len()
        .saturating_sub(SOCIAL_MAX_EDGES_DRAWN);
    ui.horizontal(|ui| {
        ui.label("Social edges");
        ui.label(format!(
            "{} edges · {} prisoners · drawing {} · {} labels",
            snap.social_edges.len(),
            n,
            snap.social_edges.len() - hidden_edges,
            labels.len()
        ));
    });
    if hidden_edges > 0 {
        ui.add(egui::Label::new(
            egui::RichText::new(format!(
                "+{hidden_edges} edges not drawn (layout cap {SOCIAL_MAX_EDGES_DRAWN})"
            ))
            .color(theme::MUTED)
            .small(),
        ));
    }
    egui::ScrollArea::vertical()
        .id_salt("social_edges")
        .max_height(180.0)
        .show(ui, |ui| {
            let mut matched = 0usize;
            for edge in &snap.social_edges {
                let a = name_map
                    .get(&edge.prisoner_a.raw())
                    .map_or_else(|| format!("#{}", edge.prisoner_a.raw()), Clone::clone);
                let b = name_map
                    .get(&edge.prisoner_b.raw())
                    .map_or_else(|| format!("#{}", edge.prisoner_b.raw()), Clone::clone);
                let label = format!(
                    "{} ↔ {} · bond {:.2} · threat {:.2} · trust {:.2}",
                    a, b, edge.bond_score, edge.threat_score, edge.trust
                );
                if !q.is_empty() && !label.to_lowercase().contains(&q) {
                    continue;
                }
                matched += 1;
                if matched <= SOCIAL_LIST_LIMIT {
                    ui.add(egui::Label::new(egui::RichText::new(label).small()));
                }
            }
            if matched > SOCIAL_LIST_LIMIT {
                ui.add(egui::Label::new(
                    egui::RichText::new(format!(
                        "+{} more edges ({} matching; list cap {SOCIAL_LIST_LIMIT})",
                        matched - SOCIAL_LIST_LIMIT,
                        matched
                    ))
                    .color(theme::MUTED)
                    .small(),
                ));
            }
        });

    panels::section(ui, "Emergent social roles", |ui| {
        ui.label(format!(
            "Detected: {} · first 120 shown",
            snap.social_roles.len()
        ));
        let mut shown = 0usize;
        egui::ScrollArea::vertical()
            .id_salt("social_roles")
            .max_height(120.0)
            .show(ui, |ui| {
                for row in &snap.social_roles {
                    let name = name_map
                        .get(&row.prisoner.raw())
                        .map_or_else(|| format!("#{}", row.prisoner.raw()), Clone::clone);
                    let label = format!(
                        "#{} {} · {} {:.0}%",
                        row.prisoner.raw(),
                        data::short_truncate(&name, 18),
                        row.role,
                        (row.strength * 100.0).clamp(0.0, 100.0)
                    );
                    if !q.is_empty() && !label.to_lowercase().contains(&q) {
                        continue;
                    }
                    ui.add(egui::Label::new(egui::RichText::new(label).small()));
                    shown += 1;
                    if shown >= 120 {
                        break;
                    }
                }
                if shown == 0 {
                    ui.label(if q.is_empty() {
                        "No social roles detected yet."
                    } else {
                        "No detected roles match this search."
                    });
                }
            });
    });
}

// ---------------------------------------------------------------------------
// 9 — Incident reports
// ---------------------------------------------------------------------------

/// Count structured incident records by their engine kind label. This keeps
/// the bucket summary in sync with the records the list/detail actually show
/// (the event window is capped and can disagree with the incident store).
fn incident_bucket_rows(records: &[sumo::IncidentRecord]) -> Vec<(String, u32)> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for record in records {
        *counts.entry(record.kind.clone()).or_insert(0) += 1;
    }
    let mut rows: Vec<(String, u32)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows
}

fn incidents(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    ui.horizontal(|ui| {
        ui.label("Minimum severity");
        step_buttons(
            ui,
            ctx.state.incident_min_severity,
            0.0,
            1.0,
            SEVERITY_STEP,
            "",
            |v| ctx.state.incident_min_severity = v,
        );
        ui.checkbox(&mut ctx.state.incident_open_only, "Open only");
    });
    let buckets = incident_bucket_rows(&snap.incidents);
    let summary: Vec<String> = buckets
        .iter()
        .map(|(kind, count)| format!("{kind}: {count}"))
        .collect();
    theme::kv(
        ui,
        "Buckets (structured incidents)",
        if summary.is_empty() {
            "none".into()
        } else {
            format!("{} total · {}", snap.incidents.len(), summary.join(" · "))
        },
    );
    let name_map = names(snap);
    let min_severity = ctx.state.incident_min_severity;
    let open_only = ctx.state.incident_open_only;
    let visible: Vec<&sumo::IncidentRecord> = snap
        .incidents
        .iter()
        .rev()
        .filter(|r| r.severity >= min_severity && (!open_only || !r.resolved))
        .collect();
    columns(
        ui,
        ctx,
        |ui, ctx| {
            egui::ScrollArea::vertical()
                .id_salt("incident_list")
                .show(ui, |ui| {
                    for record in &visible {
                        let selected = ctx.state.selected_incident == Some(record.id);
                        if ui
                            .selectable_label(
                                selected,
                                format!(
                                    "#{} {} sev {:.0}% {} · {}",
                                    record.id,
                                    data::short_truncate(&record.kind, 14),
                                    record.severity * 100.0,
                                    record.at,
                                    data::short_truncate(&record.location, 20)
                                ),
                            )
                            .clicked()
                        {
                            ctx.state.selected_incident = Some(record.id);
                        }
                    }
                    if snap.incidents.is_empty() {
                        ui.label("No structured incidents recorded yet.");
                    } else if visible.is_empty() {
                        ui.label("No incidents match the active filters.");
                    }
                });
        },
        |ui, ctx| {
            // Read selection after the list column ran so a click in this frame
            // is reflected in the detail column.
            let selected_id = ctx.state.selected_incident;
            let selected = selected_id.and_then(|id| visible.iter().copied().find(|r| r.id == id));
            // Auto-show the newest visible record only when nothing was pinned;
            // never let the detail panel fall back to a hidden selection.
            let fallback = if selected_id.is_none() {
                visible.first().copied()
            } else {
                None
            };
            if let Some(record) = selected.or(fallback) {
                ui.heading("Incident detail");
                panels::incident_detail(ui, record, &name_map);
                if !record.resolved && ui.button("Resolve incident").clicked() {
                    ctx.emit(Command::ResolveIncident(record.id));
                }
            } else if selected_id.is_some() {
                ui.label("Selected incident is hidden by the active filters.");
            } else {
                ui.label("Select an incident to see its cause, actors, validation and outcome.");
            }
        },
    );
}

// ---------------------------------------------------------------------------
// 10 — Decision audit
// ---------------------------------------------------------------------------

fn audit(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    ui.horizontal(|ui| {
        ui.label("Validation");
        for filter in [
            None::<&str>,
            Some("accepted"),
            Some("repaired"),
            Some("rejected"),
        ] {
            let label = filter.unwrap_or("all");
            if ui
                .add(
                    egui::Button::new(label).selected(
                        ctx.state.decision_validation_filter == filter.map(str::to_string),
                    ),
                )
                .clicked()
            {
                ctx.state.decision_validation_filter = filter.map(str::to_string);
            }
        }
    });
    let ai = &snap.ai_runtime;
    theme::kv(
        ui,
        "AI runtime",
        format!(
            "{} / {} · tokens {}/{} · cache {}/{} · retries {} · rate-limited {} · batches {}",
            ai.provider,
            ai.model,
            ai.tokens_consumed_today,
            ai.max_tokens_per_day,
            ai.cache_hits,
            ai.cache_entries,
            ai.retries,
            ai.rate_limited,
            ai.batches
        ),
    );
    ui.separator();
    let filter = ctx.state.decision_validation_filter.clone();
    let visible: Vec<_> = snap
        .decision_audit
        .iter()
        .filter(|record| {
            filter
                .as_deref()
                .is_none_or(|f| data::validation_matches(&record.validation, f))
        })
        .collect();
    let selected_key = ctx.state.selected_decision.clone();
    columns(
        ui,
        ctx,
        |ui, ctx| {
            egui::ScrollArea::vertical()
                .id_salt("audit_list")
                .show(ui, |ui| {
                    for record in visible.iter().rev() {
                        let key = data::decision_key(record);
                        let selected = selected_key.as_deref() == Some(key.as_str());
                        if ui
                            .selectable_label(
                                selected,
                                format!(
                                    "{} {} · {} · {}",
                                    record.time,
                                    data::short_truncate(&record.actor, 16),
                                    data::short_truncate(&record.action, 16),
                                    record.validation
                                ),
                            )
                            .clicked()
                        {
                            ctx.state.selected_decision = Some(key);
                        }
                    }
                    if visible.is_empty() {
                        ui.label("No decisions audited yet.");
                    }
                });
        },
        |ui, ctx| {
            let selected = ctx
                .state
                .selected_decision
                .as_deref()
                .and_then(|key| {
                    visible
                        .iter()
                        .copied()
                        .find(|r| data::decision_key(r) == key)
                })
                .or_else(|| visible.last().copied());
            if let Some(record) = selected {
                ui.heading("Decision detail");
                panels::decision_detail(ui, record);
            } else {
                ui.label("Select an audit entry to see its cause, actors, validation and outcome.");
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::{GangId, PrisonerId, SimTime};
    use alc_social::GangPressure;
    use alc_world::GangHistoryEntry;
    use sumo::{GangRelationSummary, GangSummary};

    #[test]
    fn gang_detail_rows_expose_politics_history_and_relation_scores() {
        let mut names = HashMap::new();
        names.insert(1, "Rook".to_string());
        let summary = GangSummary {
            id: 7,
            name: "Iron Choir".into(),
            leader: Some(PrisonerId(1)),
            members: vec![PrisonerId(1), PrisonerId(2)],
            reputation: 0.62,
            resources: 0.31,
            culture: "mutual protection".into(),
            formed_at: SimTime::from_secs(0),
            active: true,
            hierarchy: Vec::new(),
            rivalries: vec![(9, 0.72)],
            alliances: vec![(11, 0.66)],
            internal_disputes: vec!["goal conflict".into()],
            goals: vec!["hold the yard".into()],
            pressure: GangPressure {
                rivalry: 0.5,
                disputes: 0.3,
                contraband: 0.4,
            },
            history: vec![GangHistoryEntry {
                id: 3,
                gang: GangId(7),
                kind: alc_social::GangHistoryKind::LeadershipChange,
                at: SimTime::from_secs(86_400),
                summary: "Iron Choir leadership changed".into(),
                actors: vec![2],
                causes: vec!["gang:7".into()],
            }],
            relations: vec![GangRelationSummary {
                gang: 9,
                affinity: 0.2,
                hostility: 0.7,
            }],
        };

        let rows = gang_detail_rows(&summary, &names);
        let value = |key: &str| {
            rows.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| panic!("missing row {key}"))
        };
        assert_eq!(value("Leader"), "Rook");
        assert_eq!(value("Rivalries"), "#9:0.72");
        assert_eq!(value("Alliances"), "#11:0.66");
        assert_eq!(value("Scores"), "#9 a0.20/h0.70");
        assert!(value("Pressure").parse::<f32>().unwrap() > 0.4);
        assert!(value("History").contains("leadership"));
    }

    #[test]
    fn construction_catalog_exposes_every_engine_facility_kind() {
        use alc_world::FacilityKind as F;
        use std::collections::HashSet;

        let offered: HashSet<_> = crate::CONSTRUCTION_KINDS.iter().copied().collect();
        let expected = [
            F::CellBlock,
            F::Dormitory,
            F::Medical,
            F::Education,
            F::Isolation,
            F::Visitor,
            F::Workshop,
            F::Kitchen,
            F::Recreation,
            F::Admin,
            F::Staff,
            F::Yard,
        ];
        assert_eq!(offered.len(), expected.len());
        for kind in expected {
            assert!(
                offered.contains(&kind),
                "{kind:?} missing from build controls"
            );
        }
    }

    #[test]
    fn social_grid_layout_is_deterministic_and_stays_in_canvas() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 200.0));
        assert_eq!(social_cell(2, 0, rect), rect.center());
        let first = social_cell(0, 5, rect);
        let last = social_cell(4, 5, rect);
        assert_eq!(first, social_cell(0, 5, rect));
        assert!(rect.contains(first) && rect.contains(last));
        assert!(first.x < last.x && first.y < last.y);
    }

    #[test]
    fn social_label_culling_prefers_selection_and_stays_bounded() {
        let sim = sumo::Sim::new(0xC0FFEE_20);
        let prisoners = sim.prisoners();
        let selected = prisoners[0].id.raw();
        let focused = social_label_ids(&prisoners, &[], Some(selected), 3);
        assert!(focused.contains(&selected));
        assert!(focused.len() <= 3);
        let sampled = social_label_ids(&prisoners, &[], None, 4);
        assert!(!sampled.is_empty());
        assert!(sampled.len() <= 4);
        assert_eq!(sampled, social_label_ids(&prisoners, &[], None, 4));
    }

    #[test]
    fn incident_buckets_count_structured_records_not_event_window() {
        let mut fight = sumo::IncidentRecord::default();
        fight.kind = "Fight".into();
        let fight2 = fight.clone();
        let mut argument = sumo::IncidentRecord::default();
        argument.kind = "Argument".into();
        let rows = incident_bucket_rows(&[fight, fight2, argument]);
        assert_eq!(rows[0], ("Fight".to_string(), 2));
        assert_eq!(rows[1], ("Argument".to_string(), 1));
        assert!(incident_bucket_rows(&[]).is_empty());
    }

    #[test]
    fn parole_controls_gate_on_status_conditions_and_release() {
        use sumo::ParoleStatus as P;
        assert_eq!(parole_status_label(P::Scheduled), "scheduled");
        assert_eq!(parole_status_label(P::InHearing), "in hearing");
        assert_eq!(parole_status_label(P::Granted), "granted");
        assert_eq!(parole_status_label(P::Denied), "denied");

        let due = parole_button_states(true, None, false, false);
        assert!(due[0].enabled && due[1].enabled && due[2].enabled);
        assert!(!due[3].enabled);

        let not_due = parole_button_states(false, None, false, false);
        assert!(not_due.iter().all(|state| !state.enabled));

        let scheduled = parole_button_states(false, Some(P::Scheduled), false, false);
        assert!(!scheduled[0].enabled);
        assert!(scheduled[1].enabled && scheduled[2].enabled);
        assert!(!scheduled[3].enabled);

        let granted = parole_button_states(false, Some(P::Granted), true, false);
        assert!(granted[3].enabled);
        assert!(!granted[1].enabled && !granted[2].enabled);

        let released = parole_button_states(true, Some(P::Granted), true, true);
        assert!(released.iter().all(|state| !state.enabled));
    }

    #[test]
    fn step_controls_clamp_to_their_inclusive_ranges() {
        let mid = stepped_value(0.5, POLICY_STEP, 0.0, 1.0);
        assert!((mid - 0.55).abs() < 1e-6);
        assert_eq!(stepped_value(0.0, -POLICY_STEP, 0.0, 1.0), 0.0);
        assert_eq!(stepped_value(3.0, REHAB_FUNDING_STEP, 0.0, 3.0), 3.0);
        assert_eq!(stepped_value(1.0, SEVERITY_STEP, 0.0, 1.0), 1.0);
    }

    #[test]
    fn contraband_case_label_shows_id_item_severity_and_state() {
        let mut case = sumo::ContrabandCaseSummary::default();
        case.case_id = 7;
        case.item = Some("Phones".into());
        case.severity = 0.4;
        let label = contraband_case_label(&case);
        assert!(label.contains("#7"));
        assert!(label.contains("Phones"));
        assert!(label.contains("40%"));
        assert!(label.contains("open"));
        case.resolved = true;
        assert!(contraband_case_label(&case).contains("resolved"));
    }

    #[test]
    fn post_release_buttons_emit_both_outcomes_through_commands() {
        let mut row = sumo::PostReleaseSummary::default();
        row.prisoner_id = PrisonerId(9);
        row.release_type = "parole".into();

        let success = post_release_record_command(&row, true);
        match success {
            Command::RecordPostRelease {
                prisoner,
                release_type,
                risk,
                plan,
            } => {
                assert_eq!(prisoner, PrisonerId(9));
                assert_eq!(release_type, "parole");
                assert!(risk < 0.5 && plan);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let follow_up = post_release_record_command(&row, false);
        match follow_up {
            Command::RecordPostRelease { risk, plan, .. } => {
                assert!(risk >= 0.5 && !plan);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
