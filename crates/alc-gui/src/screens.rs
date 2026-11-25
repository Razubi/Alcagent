//! The eleven observation views. Each `show_*` function renders a read-only
//! snapshot and only queues [`Command`]s through [`panels::RenderCtx`].

use crate::{
    CONSTRUCTION_KINDS, ViewMode,
    app::Command,
    data, map_view,
    panels::{self, RenderCtx},
    theme,
};
use eframe::egui::{self, Color32, FontId};
use std::collections::HashMap;

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
        |ui, _ctx| {
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

fn prisoner(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    let q = ctx.state.search.to_lowercase();
    columns(
        ui,
        ctx,
        |ui, ctx| {
            ui.add(egui::TextEdit::singleline(&mut ctx.state.search).hint_text("Search prisoners"));
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
                    theme::kv(ui, "Parole", snap.selected_parole.clone().unwrap_or_else(|| "none".into()));
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
                    if ui.button("Schedule hearing").clicked() {
                        ctx.emit(Command::StartParole(id));
                    }
                    if ui.button("Grant").clicked() {
                        ctx.emit(Command::GrantParole(id));
                    }
                    if ui.button("Deny").clicked() {
                        ctx.emit(Command::DenyParole(id, "institutional conduct".to_string()));
                    }
                    if ui.button("Release on parole").clicked() {
                        ctx.emit(Command::ReleaseParole(id));
                    }
                });
                panels::section(ui, "Contraband cases", |ui| {
                    let cases: Vec<_> = snap.contraband.iter().filter(|c| c.holder == Some(id)).collect();
                    if cases.is_empty() {
                        ui.label("No cases recorded.");
                    }
                    for case in cases {
                        theme::kv(ui, &format!("#{} {}", case.case_id, case.item.as_deref().unwrap_or("Unknown")), format!("sev {:.2} · {}", case.severity, if case.resolved { "resolved" } else { "open" }));
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
            let q = ctx.state.search.to_lowercase();
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
                    theme::kv(
                        ui,
                        "Leader",
                        g.leader.map_or_else(
                            || "none".into(),
                            |l| {
                                name_map
                                    .get(&l.raw())
                                    .cloned()
                                    .unwrap_or_else(|| format!("prisoner #{}", l.raw()))
                            },
                        ),
                    );
                    theme::kv(ui, "Members", g.members.len().to_string());
                    theme::kv(ui, "Culture", &g.culture);
                    theme::metric_bar(ui, "Reputation", g.reputation, true);
                    theme::metric_bar(ui, "Resources", g.resources, false);
                    theme::kv(ui, "Formed", g.formed_at.to_string());
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
                    panels::section(ui, "Relations & goals", |ui| {
                        theme::kv(
                            ui,
                            "Rivalries",
                            format!(
                                "{:?}",
                                g.rivalries.iter().map(|(id, _)| *id).collect::<Vec<_>>()
                            ),
                        );
                        theme::kv(
                            ui,
                            "Alliances",
                            format!(
                                "{:?}",
                                g.alliances.iter().map(|(id, _)| *id).collect::<Vec<_>>()
                            ),
                        );
                        theme::kv(
                            ui,
                            "Disputes",
                            if g.internal_disputes.is_empty() {
                                "none".into()
                            } else {
                                g.internal_disputes.join("; ")
                            },
                        );
                        theme::kv(ui, "Goals", g.goals.join(", "));
                    });
                });
        },
    );
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
            let q = ctx.state.search.to_lowercase();
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
                    theme::kv(
                        ui,
                        "Report credibility",
                        format!("{:.2}", (g.competence + g.integrity) * 0.5),
                    );
                    theme::metric_bar(ui, "Competence", g.competence, true);
                    theme::metric_bar(ui, "Experience", g.experience, true);
                    theme::metric_bar(ui, "Morale", g.morale, true);
                    theme::metric_bar(ui, "Fatigue", g.fatigue, false);
                    theme::metric_bar(ui, "Stress", g.stress, false);
                    theme::metric_bar(ui, "Loyalty", g.loyalty, true);
                    theme::metric_bar(ui, "Integrity", g.integrity, true);
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
                let mut security = snap.policy.security_level;
                if ui
                    .add(egui::Slider::new(&mut security, 0.0..=1.0).text("Security"))
                    .changed()
                {
                    ctx.emit(Command::SetSecurity(security));
                }
                let mut privileges = snap.policy.privileges_level;
                if ui
                    .add(egui::Slider::new(&mut privileges, 0.0..=1.0).text("Privileges"))
                    .changed()
                {
                    ctx.emit(Command::SetPrivileges(privileges));
                }
                let mut funding = snap.policy.rehab_funding_multiplier;
                if ui
                    .add(egui::Slider::new(&mut funding, 0.0..=3.0).text("Rehab funding"))
                    .changed()
                {
                    ctx.emit(Command::SetRehabFunding(funding));
                }
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
                egui::ComboBox::from_label("Facility")
                    .selected_text(CONSTRUCTION_KINDS[ctx.state.construction_kind_index].display())
                    .show_ui(ui, |ui| {
                        for (i, kind) in CONSTRUCTION_KINDS.iter().enumerate() {
                            ui.selectable_value(
                                &mut ctx.state.construction_kind_index,
                                i,
                                kind.display(),
                            );
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("Wing");
                    ui.add(egui::DragValue::new(&mut ctx.state.build_wing).range(0..=99));
                    ui.label("Capacity");
                    ui.add(egui::DragValue::new(&mut ctx.state.build_capacity).range(1..=500));
                });
                if ui.button("Queue construction").clicked() {
                    ctx.emit(Command::EnqueueConstruction {
                        kind: CONSTRUCTION_KINDS[ctx.state.construction_kind_index],
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
    if n > 0 {
        let radius = (rect.width().min(rect.height()) * 0.5 - 40.0).max(30.0);
        let center = rect.center();
        for (i, p) in snap.prisoners.iter().enumerate() {
            let angle = (i as f32 / n as f32) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            positions.insert(
                p.id.raw(),
                center + egui::vec2(angle.cos() * radius, angle.sin() * radius),
            );
        }
    }
    for edge in &snap.social_edges {
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
        painter.text(
            pos + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            &p.name,
            FontId::proportional(9.0),
            Color32::WHITE,
        );
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
    let q = ctx.state.search.to_lowercase();
    ui.horizontal(|ui| {
        ui.label("Social edges");
        ui.label(format!(
            "{} edges · {} prisoners",
            snap.social_edges.len(),
            n
        ));
    });
    egui::ScrollArea::vertical()
        .id_salt("social_edges")
        .max_height(180.0)
        .show(ui, |ui| {
            for edge in snap.social_edges.iter().take(200) {
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
                ui.add(egui::Label::new(egui::RichText::new(label).small()));
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

fn incidents(ui: &mut egui::Ui, ctx: &mut RenderCtx<'_>) {
    let snap = ctx.snapshot;
    ui.horizontal(|ui| {
        ui.label("Minimum severity");
        ui.add(egui::Slider::new(
            &mut ctx.state.incident_min_severity,
            0.0..=1.0,
        ));
        ui.checkbox(&mut ctx.state.incident_open_only, "Open only");
    });
    let rows = data::incident_report_rows(&snap.events);
    let summary: Vec<String> = rows
        .iter()
        .map(|r| format!("{}: {}", r.kind.label(), r.count))
        .collect();
    theme::kv(
        ui,
        "Buckets",
        if summary.is_empty() {
            "none".into()
        } else {
            summary.join(" · ")
        },
    );
    let name_map = names(snap);
    columns(
        ui,
        ctx,
        |ui, ctx| {
            egui::ScrollArea::vertical()
                .id_salt("incident_list")
                .show(ui, |ui| {
                    for record in snap.incidents.iter().rev() {
                        if record.severity < ctx.state.incident_min_severity {
                            continue;
                        }
                        if ctx.state.incident_open_only && record.resolved {
                            continue;
                        }
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
                    }
                });
        },
        |ui, ctx| {
            let selected = ctx
                .state
                .selected_incident
                .and_then(|id| snap.incidents.iter().find(|r| r.id == id))
                .or_else(|| {
                    snap.incidents.iter().rev().find(|r| {
                        r.severity >= ctx.state.incident_min_severity
                            && (!ctx.state.incident_open_only || !r.resolved)
                    })
                });
            if let Some(record) = selected {
                ui.heading("Incident detail");
                panels::incident_detail(ui, record, &name_map);
                if !record.resolved && ui.button("Resolve incident").clicked() {
                    ctx.emit(Command::ResolveIncident(record.id));
                }
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
