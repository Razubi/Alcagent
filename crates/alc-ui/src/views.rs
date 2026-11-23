//! Read-only renderers for the nine observation modes (Directive §20).
//!
//! Every function in this module works against observation/summaries from the
//! engine and a [`UiState`] cursor. No function mutates simulation state: the
//! observer is strictly read-only (Directive §19).

use crate::{UiState, ViewMode};
use alc_prisoner::Classification;
use alc_world::FacilityKind;
use ratatui::prelude::*;
use ratatui::widgets::*;
use std::collections::HashMap;
use sumo::{
    EconomySummary, FacilitySummary, GangSummary, GuardSummary, Observation, PrisonerSummary,
    SocialEdge, WardenSummary, WorldEvent,
};

/// Bundled data the views need for rendering.
pub struct ViewData {
    pub observation: Observation,
    pub prisoners: Vec<PrisonerSummary>,
    pub gangs: Vec<GangSummary>,
    pub guards: Vec<GuardSummary>,
    pub warden: WardenSummary,
    pub economy: EconomySummary,
    pub social_edges: Vec<SocialEdge>,
    pub facilities: Vec<FacilitySummary>,
    pub events: Vec<WorldEvent>,
    pub incident_log: Vec<WorldEvent>,
    pub decision_log: Vec<String>,
}

/// Extra context the status bar needs that is not part of the world snapshot.
#[derive(Debug, Clone)]
pub struct AppStatus {
    pub paused: bool,
    pub ticks_per_second: u32,
    pub status_msg: Option<String>,
}

const ALL_MODES: [ViewMode; 11] = [
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

/// Status-bar help text. Kept as constants so tests can assert that the hints
/// stay in sync with the keys handled in `app::App::handle_key`.
pub const HELP_WIDE: &str =
    " │ ↑↓/←→ select · 1-9,0,- view · SPACE pause · +/- speed · s save · q quit";
pub const HELP_COMPACT: &str = " │ ↑↓ select · 1-9,0,- · SPC · +/- · s · q";

/// Map a `1`-`9`, `0`, `-` key press to its observation mode.
pub fn mode_from_index(index: usize) -> Option<ViewMode> {
    ALL_MODES.get(index).copied()
}

fn mode_name(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::InstitutionOverview => "Institution Overview",
        ViewMode::Prisoner => "Prisoner",
        ViewMode::Gang => "Gang",
        ViewMode::Staff => "Staff",
        ViewMode::Warden => "Warden",
        ViewMode::Financial => "Financial",
        ViewMode::EventTimeline => "Event Timeline",
        ViewMode::SocialNetwork => "Social Network",
        ViewMode::Map => "Prison Map",
        ViewMode::IncidentReports => "Incident Reports",
        ViewMode::DecisionAudit => "Decision Audit",
    }
}

// ---------------------------------------------------------------------------
// Small styling helpers
// ---------------------------------------------------------------------------

fn title_block(title: impl Into<String>) -> Block<'static> {
    let title = title.into();
    Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
}

fn plain(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::raw(text.into()))
}

fn kv(label: impl AsRef<str>, value: impl Into<String>) -> Line<'static> {
    let label = label.as_ref();
    Line::from(vec![
        Span::styled(
            format!("{label:<22}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(value.into()),
    ])
}

/// Colour for a *deficit* pressure: high is bad (red).
fn urgency_color(v: f32) -> Color {
    if v >= 0.75 {
        Color::Red
    } else if v >= 0.5 {
        Color::Yellow
    } else {
        Color::Green
    }
}

/// Colour for a *capacity* metric: high is good (green).
fn good_color(v: f32) -> Color {
    if v <= 0.25 {
        Color::Red
    } else if v <= 0.5 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn bar_line(label: &str, value: f32, width: usize) -> Line<'static> {
    let v = value.clamp(0.0, 1.0);
    let filled = (v * width as f32).round() as usize;
    let mut s = format!("{label:<22}");
    s.push_str(&"█".repeat(filled));
    s.push_str(&"░".repeat(width.saturating_sub(filled)));
    s.push_str(&format!(" {v:>4.2}"));
    Line::from(Span::styled(s, Style::default().fg(urgency_color(v))))
}

fn good_bar(label: &str, value: f32, width: usize) -> Line<'static> {
    let v = value.clamp(0.0, 1.0);
    let filled = (v * width as f32).round() as usize;
    let mut s = format!("{label:<22}");
    s.push_str(&"█".repeat(filled));
    s.push_str(&"░".repeat(width.saturating_sub(filled)));
    s.push_str(&format!(" {v:>4.2}"));
    Line::from(Span::styled(s, Style::default().fg(good_color(v))))
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub fn class_short(c: Classification) -> &'static str {
    match c {
        Classification::Minimum => "MIN",
        Classification::Low => "LOW",
        Classification::Medium => "MED",
        Classification::High => "HIGH",
        Classification::Maximum => "MAX",
    }
}

pub fn name_map(prisoners: &[PrisonerSummary]) -> HashMap<u64, String> {
    prisoners.iter().map(|p| (p.id.0, p.name.clone())).collect()
}

fn event_kind_short(kind: &alc_events::EventKind) -> String {
    format!("{kind:?}")
}

fn recent_event_lines(events: &[WorldEvent], n: usize) -> Vec<Line<'static>> {
    events
        .iter()
        .rev()
        .take(n)
        .map(|e| {
            Line::from(vec![
                Span::styled(
                    format!("[{}] ", e.at),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<16}", event_kind_short(&e.kind)),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(truncate(&e.summary, 72)),
            ])
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Top-level dispatch: tab bar + view + status bar
// ---------------------------------------------------------------------------

/// Draw a complete frame: mode tabs, the active view, and the status bar.
pub fn draw(frame: &mut Frame, data: &ViewData, ui: &UiState, status: &AppStatus) {
    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(frame.area());

    render_tabbar(frame, chunks[0], ui.mode);

    let body = chunks[1];
    match ui.mode {
        ViewMode::InstitutionOverview => render_overview(frame, body, data),
        ViewMode::Prisoner => render_prisoner(frame, body, data, ui),
        ViewMode::Gang => render_gang(frame, body, data, ui),
        ViewMode::Staff => render_staff(frame, body, data, ui),
        ViewMode::Warden => render_warden(frame, body, data, ui),
        ViewMode::Financial => render_financial(frame, body, data, ui),
        ViewMode::EventTimeline => render_timeline(frame, body, data, ui),
        ViewMode::SocialNetwork => render_network(frame, body, data, ui),
        ViewMode::Map => render_map(frame, body, data, ui),
        ViewMode::IncidentReports => render_incidents(frame, body, data, ui),
        ViewMode::DecisionAudit => render_decisions(frame, body, data, ui),
    }

    render_status(frame, chunks[2], data, ui, status);
}

fn render_tabbar(frame: &mut Frame, area: Rect, active: ViewMode) {
    // `area` is two rows tall; row 0 holds the tabs, row 1 holds a persistent
    // control hint. Keeping the hint out of the status bar means it is never
    // clipped by the population / finance readouts on narrow terminals.
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    let tabs_area = rows[0];
    let short = [
        "1 Ovw", "2 Pris", "3 Gang", "4 Staff", "5 Ward", "6 Fin", "7 Evts", "8 Net", "9 Map", "0 Inc", "A Aud",
    ];
    let compact = tabs_area.width < 140;
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, mode) in ALL_MODES.iter().enumerate() {
        let is_active = *mode == active;
        let style = if is_active {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let key = match i { 9 => "0".to_string(), 10 => "A".to_string(), _ => format!("{}", i + 1) };
        let label = if compact {
            short[i].to_string()
        } else {
            format!(" {} {} ", key, mode_name(*mode))
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
    }
    let widget = Paragraph::new(Line::from(spans));
    frame.render_widget(widget, tabs_area);

    let hint = if tabs_area.width < 110 {
        HELP_COMPACT
    } else {
        HELP_WIDE
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::DarkGray),
        ))),
        rows[1],
    );
}

fn render_status(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState, status: &AppStatus) {
    let obs = &data.observation;
    let pop = obs.population;
    let cap = obs.capacity;
    let cash = obs.cash_major;
    let health = obs.economy_health;

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(
        format!(" {} ", mode_name(ui.mode)),
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw(" │ "));
    spans.push(Span::styled(
        obs.at.to_string(),
        Style::default().fg(Color::White),
    ));
    spans.push(Span::raw(format!(
        " │ Pop {pop}/{cap} │ Guards {} │ Gangs {} │ ${cash:.2} │ Health {:.0}%",
        obs.guards,
        obs.gangs,
        health * 100.0
    )));
    let run_style = if status.paused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };
    spans.push(Span::raw(" │ "));
    spans.push(Span::styled(
        if status.paused { " PAUSED " } else { " RUNNING " },
        run_style,
    ));
    spans.push(Span::styled(
        format!(" {} tps ", status.ticks_per_second),
        Style::default().fg(Color::Magenta),
    ));

    if let Some(msg) = &status.status_msg {
        spans.push(Span::raw(" │ "));
        spans.push(Span::styled(truncate(msg, 40), Style::default().fg(Color::Cyan)));
    }
    let widget = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Reset).fg(Color::White));
    frame.render_widget(widget, area);
}

// ---------------------------------------------------------------------------
// 1 — Institution overview
// ---------------------------------------------------------------------------

fn render_overview(frame: &mut Frame, area: Rect, data: &ViewData) {
    let obs = &data.observation;
    let chunks = Layout::vertical([
        Constraint::Length(12),
        Constraint::Min(5),
        Constraint::Length(10),
    ])
    .split(area);

    let summary = vec![
        kv("Simulation time", obs.at.to_string()),
        kv("Days simulated", obs.days_simulated.to_string()),
        kv("Seed", "n/a".to_string()),
        kv(
            "Population",
            format!("{} / {} capacity", obs.population, obs.capacity),
        ),
        kv(
            "Guards / Facilities",
            format!("{} / {}", obs.guards, obs.facilities),
        ),
        kv("Gangs", obs.gangs.to_string()),
        kv("Events logged", data.events.len().to_string()),
        bar_line("Security level", obs.security_level, 18),
        bar_line("Privileges", 0.5, 18),
        bar_line("Economy health", obs.economy_health as f32, 18),
        kv(
            "Lockdown",
            if obs.lockdown {
                "ACTIVE — institution on lockdown"
            } else {
                "none"
            },
        ),
    ];
    frame.render_widget(
        Paragraph::new(summary).block(title_block(" Institution Overview ")),
        chunks[0],
    );

    let mid = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[1]);
    let events = recent_event_lines(&data.events, 10);
    frame.render_widget(
        Paragraph::new(events).block(title_block(" Recent Events ")),
        mid[0],
    );

    let w = &data.warden;
    let warden_lines = vec![
        kv("Warden", w.name.clone()),
        kv("Philosophy", truncate(&w.philosophy, 46)),
        good_bar("Administrative focus", w.attention, 16),
        kv("Reports inbox", format!("{} guard reports", w.inbox.len())),
        kv("Pending actions", w.pending_actions.len().to_string()),
        kv("Last decision", w.last_decision_at.to_string()),
        kv(
            "Treasury",
            format!("${:.2}", data.economy.cash_major),
        ),
    ];
    frame.render_widget(
        Paragraph::new(warden_lines).block(title_block(" Warden & Institution ")),
        mid[1],
    );

    let rows: Vec<Row<'static>> = data
        .facilities
        .iter()
        .map(|f| {
            Row::new(vec![
                Cell::from(f.id.to_string()),
                Cell::from(f.kind.display().to_string()),
                Cell::from(format!("W{}", f.wing)),
                Cell::from(f.capacity.to_string()),
                Cell::from(format!("{:.0}%", f.condition * 100.0)),
                Cell::from(f.staff_required.to_string()),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(18),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec!["ID", "Facility", "Wing", "Cap", "Cond", "Staff"])
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    )
    .block(title_block(" Facilities "));
    frame.render_widget(table, chunks[2]);
}

// ---------------------------------------------------------------------------
// 2 — Prisoner profiles
// ---------------------------------------------------------------------------

fn render_prisoner(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks =
        Layout::horizontal([Constraint::Length(30), Constraint::Min(30)]).split(area);

    let idx = ui.scroll as usize;
    let items: Vec<ListItem<'static>> = data
        .prisoners
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let style = if i == idx {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{:>3} ", p.id.0)),
                Span::raw(format!("{:<21}", truncate(&p.name, 20))),
                Span::styled(class_short(p.classification), Style::default().fg(Color::DarkGray)),
            ]))
            .style(style)
        })
        .collect();
    let list = List::new(items).block(title_block(" Prisoners "));
    frame.render_widget(list, chunks[0]);

    if let Some(p) = data.prisoners.get(idx) {
        render_prisoner_detail(frame, chunks[1], p);
    } else {
        frame.render_widget(
            Paragraph::new("No prisoners in custody.").block(title_block(" Prisoner Profile ")),
            chunks[1],
        );
    }
}

fn render_prisoner_detail(frame: &mut Frame, area: Rect, p: &PrisonerSummary) {
    let chunks = Layout::vertical([
        Constraint::Length(10),
        Constraint::Min(10),
        Constraint::Min(8),
    ])
    .split(area);

    let gang = p
        .gang_affiliation
        .map(|g| format!("Gang #{g}"))
        .unwrap_or_else(|| "unaffiliated".to_string());
    let header = vec![
        kv("Name", p.name.clone()),
        kv(
            "ID / Age",
            format!("{} / {} years", p.id.0, p.age),
        ),
        kv("Classification", format!("{:?}", p.classification)),
        kv(
            "Sentence",
            format!(
                "{} served / {} total — {} remaining",
                p.sentence_served_days,
                p.sentence_days,
                p.sentence_days.saturating_sub(p.sentence_served_days)
            ),
        ),
        kv("Gang", gang),
        kv("Reputation", format!("{:.2}", p.reputation)),
        good_bar("Health", p.condition.health, 14),
        good_bar("Mental health", p.condition.mental_health, 14),
        good_bar("Hope", p.condition.hope, 14),
        bar_line("Injury / Illness", (p.condition.injury + p.condition.illness) / 2.0, 14),
    ];
    frame.render_widget(
        Paragraph::new(header).block(title_block(format!(" {} — Profile ", p.name))),
        chunks[0],
    );

    let columns = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);
    let t = &p.traits;
    let traits = vec![
        good_bar("Intelligence", t.intelligence, 12),
        good_bar("Stability", t.emotional_stability, 12),
        bar_line("Impulsiveness", t.impulsiveness, 12),
        bar_line("Aggression", t.aggression, 12),
        good_bar("Sociability", t.sociability, 12),
        good_bar("Loyalty", t.loyalty, 12),
        bar_line("Manipulation", t.manipulation, 12),
        bar_line("Risk tolerance", t.risk_tolerance, 12),
        good_bar("Discipline", t.discipline, 12),
        good_bar("Empathy", t.empathy, 12),
        good_bar("Ambition", t.ambition, 12),
        good_bar("Leadership", t.leadership, 12),
    ];
    frame.render_widget(
        Paragraph::new(traits).block(title_block(" Personality Traits ")),
        columns[0],
    );

    let needs = need_lines(&p.needs);
    frame.render_widget(
        Paragraph::new(needs).block(title_block(" Needs (deficit pressure) ")),
        columns[1],
    );

    let lower =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[2]);

    let mut goals: Vec<Line<'static>> = Vec::new();
    for g in &p.goals {
        goals.push(Line::from(vec![
            Span::styled(
                format!("{:?}", g.kind),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(format!(
                " pro {:.2} urg {:.2} — {}",
                g.progress, g.urgency, truncate(&g.description, 40)
            )),
        ]));
    }
    if goals.is_empty() {
        goals.push(plain("No active goals."));
    }
    goals.push(plain("--- Schedule ---"));
    goals.push(kv("Wing", p.schedule.wing.to_string()));
    goals.push(kv(
        "Work",
        p.schedule
            .work_assignment
            .clone()
            .unwrap_or_else(|| "none".into()),
    ));
    goals.push(kv("Education", p.schedule.education.to_string()));
    goals.push(kv(
        "Discipline",
        if p.schedule.disciplinary_status {
            "sanctioned"
        } else {
            "clean"
        },
    ));
    frame.render_widget(
        Paragraph::new(goals).block(title_block(" Goals & Schedule ")),
        lower[0],
    );

    let mut tail: Vec<Line<'static>> = Vec::new();
    tail.push(plain("--- Skills ---"));
    let mut skills: Vec<_> = p.skills.iter().collect();
    skills.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (k, v) in skills.iter().take(5) {
        tail.push(kv(k.as_str(), format!("{v:.2}")));
    }
    if p.skills.is_empty() {
        tail.push(plain("  (none recorded)"));
    }
    tail.push(plain("--- Memory ---"));
    let memory = if p.memory_summary.is_empty() {
        "  (no significant memories yet)".to_string()
    } else {
        truncate(&p.memory_summary, 400)
    };
    for part in memory.lines() {
        tail.push(plain(part.to_string()));
    }
    frame.render_widget(
        Paragraph::new(tail).block(title_block(" Skills & Memory ")),
        lower[1],
    );
}

fn need_lines(n: &alc_prisoner::Needs) -> Vec<Line<'static>> {
    vec![
        bar_line("Hunger", n.hunger, 10),
        bar_line("Thirst", n.thirst, 10),
        bar_line("Sleep", n.sleep, 10),
        bar_line("Fatigue", n.fatigue, 10),
        bar_line("Hygiene", n.hygiene, 10),
        bar_line("Medical", n.medical_need, 10),
        bar_line("Exercise", n.exercise, 10),
        bar_line("Temperature", n.temperature_comfort, 10),
        bar_line("Bathroom", n.bathroom, 10),
        bar_line("Safety", n.safety, 10),
        bar_line("Companionship", n.companionship, 10),
        bar_line("Respect", n.respect, 10),
        bar_line("Freedom", n.freedom, 10),
        bar_line("Autonomy", n.autonomy, 10),
        bar_line("Purpose", n.purpose, 10),
        bar_line("Stimulation", n.stimulation, 10),
        bar_line("Stability", n.stability, 10),
        bar_line("Privacy", n.privacy, 10),
        bar_line("Belonging", n.belonging, 10),
        bar_line("Friendship", n.friendship, 10),
        bar_line("Family contact", n.family_contact, 10),
        bar_line("Acceptance", n.group_acceptance, 10),
        bar_line("Status", n.status, 10),
        bar_line("Protection", n.protection, 10),
        bar_line("Recognition", n.recognition, 10),
    ]
}

// ---------------------------------------------------------------------------
// 3 — Gangs
// ---------------------------------------------------------------------------

fn render_gang(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks =
        Layout::horizontal([Constraint::Length(30), Constraint::Min(30)]).split(area);
    let idx = ui.scroll as usize;

    if data.gangs.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                plain("No gangs have formed yet."),
                plain(""),
                plain("Gangs emerge from trust, protection needs, shared backgrounds"),
                plain("and common enemies — watch the Social Network and Prisoner views."),
                plain(""),
                plain("Advance time with SPACE / +/- and keep observing."),
            ])
            .block(title_block(" Gangs ")),
            area,
        );
        return;
    }

    let names = name_map(&data.prisoners);
    let items: Vec<ListItem<'static>> = data
        .gangs
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let style = if i == idx {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{:>3} ", g.id)),
                Span::raw(format!("{:<18}", truncate(&g.name, 17))),
                Span::styled(
                    format!("{:>2}m", g.members.len()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    if g.active { " ACTIVE" } else { " INACTIVE" },
                    Style::default().fg(if g.active { Color::Green } else { Color::Red }),
                ),
            ]))
            .style(style)
        })
        .collect();
    frame.render_widget(
        List::new(items).block(title_block(" Gangs ")),
        chunks[0],
    );

    let Some(g) = data.gangs.get(idx) else {
        frame.render_widget(
            Paragraph::new("Select a gang with ↑/↓.").block(title_block(" Gang Detail ")),
            chunks[1],
        );
        return;
    };

    let header = vec![
        kv("Name", g.name.clone()),
        kv("ID", g.id.to_string()),
        kv(
            "Leader",
            g.leader
                .and_then(|id| names.get(&id.0).cloned())
                .unwrap_or_else(|| "none".into()),
        ),
        kv("Members", g.members.len().to_string()),
        good_bar("Reputation", g.reputation, 16),
        bar_line("Resources", g.resources, 16),
        kv("Culture", g.culture.clone()),
        kv("Formed", g.formed_at.to_string()),
        kv("Active", g.active.to_string()),
    ];
    let detail_chunks =
        Layout::vertical([Constraint::Length(10), Constraint::Min(6), Constraint::Length(6)])
            .split(chunks[1]);
    frame.render_widget(
        Paragraph::new(header).block(title_block(format!(" {} ", g.name))),
        detail_chunks[0],
    );

    let hierarchy_rows: Vec<Row<'static>> = g
        .hierarchy
        .iter()
        .map(|m| {
            Row::new(vec![
                Cell::from(
                    names
                        .get(&m.prisoner.0)
                        .cloned()
                        .unwrap_or_else(|| format!("Prisoner #{}", m.prisoner.0)),
                ),
                Cell::from(format!("{:?}", m.rank)),
                Cell::from(format!("{:.2}", m.commitment)),
                Cell::from(format!("{:.2}", m.influence)),
                Cell::from(format!("{:.2}", m.grievances)),
            ])
        })
        .collect();
    let hierarchy = Table::new(
        hierarchy_rows,
        [
            Constraint::Length(22),
            Constraint::Length(12),
            Constraint::Length(11),
            Constraint::Length(11),
            Constraint::Length(11),
        ],
    )
    .header(
        Row::new(vec!["Member", "Rank", "Commit", "Influence", "Grievance"])
            .style(Style::default().fg(Color::Yellow)),
    )
    .block(title_block(" Hierarchy "));
    frame.render_widget(hierarchy, detail_chunks[1]);

    let tail = vec![
        kv(
            "Rivalries",
            format!("{:?}", g.rivalries.iter().map(|r| r.0).collect::<Vec<_>>()),
        ),
        kv(
            "Alliances",
            format!("{:?}", g.alliances.iter().map(|r| r.0).collect::<Vec<_>>()),
        ),
        kv(
            "Disputes",
            if g.internal_disputes.is_empty() {
                "none".into()
            } else {
                truncate(&g.internal_disputes.join("; "), 60)
            },
        ),
        kv("Goals", truncate(&g.goals.join(", "), 60)),
    ];
    frame.render_widget(
        Paragraph::new(tail).block(title_block(" Relations & Goals ")),
        detail_chunks[2],
    );
}

// ---------------------------------------------------------------------------
// 4 — Staff / guards
// ---------------------------------------------------------------------------

fn render_staff(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks =
        Layout::horizontal([Constraint::Length(30), Constraint::Min(30)]).split(area);
    let idx = ui.scroll as usize;

    let items: Vec<ListItem<'static>> = data
        .guards
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let style = if i == idx {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{:>3} ", g.id)),
                Span::raw(format!("{:<16}", truncate(&g.name, 15))),
                Span::styled(
                    format!("{:?}", g.rank),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(format!(" {:?}", g.post)),
            ]))
            .style(style)
        })
        .collect();
    frame.render_widget(
        List::new(items).block(title_block(" Staff Roster ")),
        chunks[0],
    );

    let Some(g) = data.guards.get(idx) else {
        frame.render_widget(
            Paragraph::new("No guards on duty.").block(title_block(" Guard Detail ")),
            chunks[1],
        );
        return;
    };

    let detail = Layout::vertical([
        Constraint::Length(9),
        Constraint::Min(8),
        Constraint::Length(7),
    ])
    .split(chunks[1]);
    let header = vec![
        kv("Name", g.name.clone()),
        kv("Rank", format!("{:?}", g.rank)),
        kv("Post", format!("{:?}", g.post)),
        kv("Authority", format!("{:?}", g.authority)),
        kv("Shift", g.shift.to_string()),
        kv("Current task", g.current_task.clone()),
        kv("Report credibility", format!("{:.2}", (g.competence + g.integrity) / 2.0)),
        kv("On-duty history", format!("{} entries", g.history.len())),
    ];
    frame.render_widget(
        Paragraph::new(header).block(title_block(format!(" {} ", g.name))),
        detail[0],
    );

    let bars = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(detail[1]);
    frame.render_widget(
        Paragraph::new(vec![
            good_bar("Competence", g.competence, 14),
            good_bar("Experience", g.experience, 14),
            good_bar("Morale", g.morale, 14),
            bar_line("Fatigue", g.fatigue, 14),
        ])
        .block(title_block(" Duty Condition ")),
        bars[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            bar_line("Stress", g.stress, 14),
            good_bar("Loyalty", g.loyalty, 14),
            good_bar("Integrity", g.integrity, 14),
            good_bar("Personality (lenient)", g.personality, 14),
        ])
        .block(title_block(" Disposition ")),
        bars[1],
    );

    let history: Vec<Line<'static>> = g
        .history
        .iter()
        .rev()
        .take(8)
        .map(|h| plain(h.clone()))
        .collect();
    frame.render_widget(
        Paragraph::new(if history.is_empty() {
            vec![plain("No notable duty events yet.")]
        } else {
            history
        })
        .block(title_block(" Recent Duty Log ")),
        detail[2],
    );
}

// ---------------------------------------------------------------------------
// 5 — Warden
// ---------------------------------------------------------------------------

fn render_warden(frame: &mut Frame, area: Rect, data: &ViewData, _ui: &UiState) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Min(30)]).split(area);
    let w = &data.warden;

    let t = &w.traits;
    let traits = vec![
        bar_line("Strictness", t.strictness, 14),
        good_bar("Compassion", t.compassion, 14),
        bar_line("Political ambition", t.political_ambition, 14),
        good_bar("Financial discipline", t.financial_discipline, 14),
        bar_line("Risk tolerance", t.risk_tolerance, 14),
        good_bar("Reform orientation", t.reform_orientation, 14),
        bar_line("Authoritarian", t.authoritarian_tendencies, 14),
        good_bar("Staff loyalty", t.staff_loyalty, 14),
        bar_line("Corruption risk", t.corruption_susceptibility, 14),
        bar_line("Reputation focus", t.reputation_concerns, 14),
        good_bar("Competence", t.competence, 14),
    ];
    let left = Layout::vertical([Constraint::Length(6), Constraint::Min(6)]).split(chunks[0]);
    frame.render_widget(
        Paragraph::new(vec![
            kv("Warden", w.name.clone()),
            kv("Philosophy", truncate(&w.philosophy, 48)),
            good_bar("Attention", w.attention, 14),
            kv("Reports received", w.inbox.len().to_string()),
        ])
        .block(title_block(" Warden Profile ")),
        left[0],
    );
    frame.render_widget(
        Paragraph::new(traits).block(title_block(" Leadership Traits ")),
        left[1],
    );

    let actions: Vec<Line<'static>> = if w.pending_actions.is_empty() {
        vec![plain("No pending actions.")]
    } else {
        w.pending_actions.iter().take(20).map(|a| plain(format!("{:?}", a))).collect()
    };
    frame.render_widget(
        Paragraph::new(actions).block(title_block(" Pending Actions ")),
        chunks[1],
    );
}

// ---------------------------------------------------------------------------
// 6 — Finances
// ---------------------------------------------------------------------------

fn render_financial(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks =
        Layout::vertical([Constraint::Length(7), Constraint::Min(4)]).split(area);
    let e = &data.economy;

    let summary = vec![
        kv("Treasury", format!("${:.2}", e.cash_major)),
        kv("Financial health", format!("{:.0}%  (30-day runway heuristic)", e.health * 100.0)),
        kv(
            "Funding / prisoner / day",
            format!("${:.2}", e.funding_per_prisoner_per_day),
        ),
        kv(
            "Crowding threshold",
            format!("{} prisoners", e.crowding_threshold),
        ),
        kv("Ledger entries", e.ledger.len().to_string()),
    ];
    frame.render_widget(
        Paragraph::new(summary).block(title_block(" Prison Economy ")),
        chunks[0],
    );

    let per_page = (chunks[1].height.saturating_sub(3) as usize).max(1);
    let offset = ui.scroll as usize;
    let rows: Vec<Row<'static>> = e
        .ledger
        .iter()
        .rev()
        .skip(offset)
        .take(per_page)
        .map(|l| {
            let day = SimDay(l.at_secs / 86_400);
            let amount_style = if l.amount.0 >= 0 {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            };
            Row::new(vec![
                Cell::from(format!("D{}", day.0)),
                Cell::from(format!("{:?}", l.account)),
                Cell::from(Span::styled(
                    format!("{:>12.2}", l.amount.as_major()),
                    amount_style,
                )),
                Cell::from(truncate(&l.note, 60)),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Length(22),
            Constraint::Length(14),
            Constraint::Min(20),
        ],
    )
    .header(
        Row::new(vec!["Day", "Account", "Amount", "Note"])
            .style(Style::default().fg(Color::Yellow)),
    )
    .block(title_block(format!(
        " Ledger — from entry {} (↑/↓ to scroll) ",
        u32::from(ui.scroll) + 1
    )));
    frame.render_widget(table, chunks[1]);
}

/// Tiny wrapper so the ledger day can be formatted without importing SimTime.
struct SimDay(u64);

// ---------------------------------------------------------------------------
// 7 — Event timeline
// ---------------------------------------------------------------------------

fn render_timeline(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks =
        Layout::vertical([Constraint::Min(4), Constraint::Length(7)]).split(area);
    let mut events: Vec<_> = data.events.iter().collect();
    events.reverse(); // newest first
    let idx = (ui.scroll as usize).min(events.len().saturating_sub(1));

    let items: Vec<ListItem<'static>> = events
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let marker = if i == idx { "»" } else { " " };
            let style = if i == idx {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} "), Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("[{}] ", e.at),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<17}", event_kind_short(&e.kind)),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(truncate(&e.summary, 88)),
            ]))
            .style(style)
        })
        .collect();
    let list = List::new(items).block(title_block(format!(
        " Event Timeline — {} events (newest first) ",
        events.len()
    )));
    frame.render_widget(list, chunks[0]);

    if let Some(e) = events.get(idx) {
        let detail = vec![
            kv("Event", e.summary.clone()),
            kv("Kind", event_kind_short(&e.kind)),
            kv("At", e.at.to_string()),
            kv("ID", e.id.to_string()),
            kv(
                "Confidence",
                format!("{:.0}%", e.confidence * 100.0),
            ),
            kv(
                "Causes",
                if e.causes.is_empty() {
                    "not recorded".into()
                } else {
                    truncate(&e.causes.join(" → "), 90)
                },
            ),
        ];
        frame.render_widget(
            Paragraph::new(detail).block(title_block(" Event Detail ")),
            chunks[1],
        );
    } else {
        frame.render_widget(
            Paragraph::new("No events recorded yet.").block(title_block(" Event Detail ")),
            chunks[1],
        );
    }
}

// ---------------------------------------------------------------------------
// 8 — Social network
// ---------------------------------------------------------------------------

fn render_network(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks =
        Layout::vertical([Constraint::Length(4), Constraint::Min(4)]).split(area);
    let names = name_map(&data.prisoners);

    let active_gangs = data.gangs.iter().filter(|g| g.active).count();
    frame.render_widget(
        Paragraph::new(vec![
            kv("Relationship dyads", data.social_edges.len().to_string()),
            kv("Gangs active", active_gangs.to_string()),
            kv("Prisoners tracked", data.prisoners.len().to_string()),
        ])
        .block(title_block(" Social Structure ")),
        chunks[0],
    );

    let mut dyads = data.social_edges.clone();
    dyads.sort_by(|a, b| {
        b.bond_score
            .abs()
            .partial_cmp(&a.bond_score.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let per_page = (chunks[1].height.saturating_sub(3) as usize).max(1);
    let offset = ui.scroll as usize;
    let rows: Vec<Row<'static>> = dyads
        .iter()
        .skip(offset)
        .take(per_page)
        .map(|e| {
            let an = names.get(&e.prisoner_a.0).cloned().unwrap_or_else(|| format!("P{}", e.prisoner_a.0));
            let bn = names.get(&e.prisoner_b.0).cloned().unwrap_or_else(|| format!("P{}", e.prisoner_b.0));
            Row::new(vec![
                Cell::from(truncate(&an, 18)),
                Cell::from(truncate(&bn, 18)),
                Cell::from(format!("{:.2}", e.friendship)),
                Cell::from(format!("{:.2}", e.trust)),
                Cell::from(format!("{:.2}", e.fear)),
                Cell::from(format!("{:.2}", e.hostility)),
                Cell::from(format!("{:.2}", e.rivalry)),
                Cell::from(format!("{:+.2}", e.bond_score)),
                Cell::from(format!("{:.2}", e.threat_score)),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(18),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec![
            "Prisoner A", "Prisoner B", "Friend", "Trust", "Fear", "Hostile", "Rival", "Bond", "Threat",
        ])
        .style(Style::default().fg(Color::Yellow)),
    )
    .block(title_block(format!(
        " Relationship Edges — from dyad {} (↑/↓ to scroll) ",
        u32::from(ui.scroll) + 1
    )));
    frame.render_widget(table, chunks[1]);
}

// ---------------------------------------------------------------------------
// 9 — Facility map
// ---------------------------------------------------------------------------

fn render_map(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Min(25)]).split(area);

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(plain("╔════════════════ PERIMETER ════════════════╗"));
    let towers = data
        .guards
        .iter()
        .filter(|g| g.post == alc_guard::PostType::Tower)
        .count();
    lines.push(plain(format!("║  Watch towers: {towers:<2}  Patrols active        ║")));
    lines.push(plain("╠════════════════ INSIDE THE WALLS ═════════╣"));

    let order = [
        FacilityKind::CellBlock,
        FacilityKind::Dormitory,
        FacilityKind::Yard,
        FacilityKind::Kitchen,
        FacilityKind::Medical,
        FacilityKind::Recreation,
        FacilityKind::Education,
        FacilityKind::Workshop,
        FacilityKind::Admin,
        FacilityKind::Isolation,
        FacilityKind::Visitor,
        FacilityKind::Staff,
    ];
    for kind in order {
        let matching: Vec<&FacilitySummary> = data
            .facilities
            .iter()
            .filter(|f| f.kind == kind)
            .collect();
        if matching.is_empty() {
            continue;
        }
        for f in matching {
            let cond = format!("{:.0}%", f.condition * 100.0);
            let label = format!(
                "║  W{} {:<13} cap {:>3}  cond {:>4}  ",
                f.wing,
                kind.display(),
                f.capacity,
                cond
            );
            let color = if f.condition < 0.4 {
                Color::Red
            } else if f.condition < 0.7 {
                Color::Yellow
            } else {
                Color::Green
            };
            lines.push(Line::from(Span::styled(
                format!("{label:47}║"),
                Style::default().fg(color),
            )));
        }
    }
    lines.push(plain("╚═══════════════════════════════════════════╝"));
    lines.push(plain(""));
    lines.push(plain("Legend: green = good condition · yellow = worn · red = critical"));
    lines.push(plain("↑/↓ moves the facility cursor; guards are listed at right."));

    frame.render_widget(
        Paragraph::new(lines).block(title_block(" Facility Map ")),
        chunks[0],
    );

    let right = Layout::vertical([Constraint::Percentage(50), Constraint::Min(4)])
        .split(chunks[1]);

    let mut posts: Vec<(alc_guard::PostType, usize)> = Vec::new();
    for g in &data.guards {
        if let Some(entry) = posts.iter_mut().find(|(p, _)| *p == g.post) {
            entry.1 += 1;
        } else {
            posts.push((g.post, 1));
        }
    }
    posts.sort_by_key(|(p, _)| format!("{p:?}"));
    let mut post_lines: Vec<Line<'static>> = Vec::new();
    for (post, count) in posts {
        post_lines.push(kv(format!("{post:?}"), format!("{count} posted")));
    }
    frame.render_widget(
        Paragraph::new(post_lines).block(title_block(" Guard Posts ")),
        right[0],
    );

    let cursor = ui.scroll as usize;
    let rows: Vec<Row<'static>> = data
        .facilities
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let style = if i == cursor {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(f.id.to_string()),
                Cell::from(f.kind.display().to_string()),
                Cell::from(format!("W{}", f.wing)),
                Cell::from(f.capacity.to_string()),
            ])
            .style(style)
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(16),
            Constraint::Length(6),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec!["ID", "Facility", "Wing", "Cap"])
            .style(Style::default().fg(Color::Yellow)),
    )
    .block(title_block(" Facilities (cursor) "));
    frame.render_widget(table, right[1]);
}

// ---------------------------------------------------------------------------
// 10 — Incident reports
// ---------------------------------------------------------------------------

fn render_incidents(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks = Layout::vertical([Constraint::Min(4), Constraint::Length(7)]).split(area);
    let mut events: Vec<_> = data.incident_log.iter().collect();
    events.reverse();
    let idx = (ui.scroll as usize).min(events.len().saturating_sub(1));

    let items: Vec<ListItem<'static>> = events.iter().enumerate().map(|(i, e)| {
        let marker = if i == idx { "»" } else { " " };
        let style = if i == idx {
            Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)
        } else { Style::default() };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(Color::Yellow)),
            Span::styled(format!("[{}] ", e.at), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<16}", event_kind_short(&e.kind)), Style::default().fg(Color::Red)),
            Span::raw(truncate(&e.summary, 88)),
        ])).style(style)
    }).collect();
    frame.render_widget(
        List::new(items).block(title_block(format!(" Incident Reports — {} (newest first) ", events.len()))),
        chunks[0],
    );

    if let Some(e) = events.get(idx) {
        let detail = vec![
            kv("Event", e.summary.clone()),
            kv("Kind", event_kind_short(&e.kind)),
            kv("At", e.at.to_string()),
            kv("ID", e.id.to_string()),
            kv("Confidence", format!("{:.0}%", e.confidence * 100.0)),
            kv("Causes", if e.causes.is_empty() { "none".into() } else { truncate(&e.causes.join(" → "), 90) }),
        ];
        frame.render_widget(Paragraph::new(detail).block(title_block(" Incident Detail ")), chunks[1]);
    } else {
        frame.render_widget(Paragraph::new("No incidents recorded.").block(title_block(" Incident Detail ")), chunks[1]);
    }
}

// ---------------------------------------------------------------------------
// 11 — Decision audit
// ---------------------------------------------------------------------------

fn render_decisions(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks = Layout::vertical([Constraint::Min(4), Constraint::Length(7)]).split(area);
    let mut entries: Vec<_> = data.decision_log.iter().collect();
    entries.reverse();
    let idx = (ui.scroll as usize).min(entries.len().saturating_sub(1));

    let items: Vec<ListItem<'static>> = entries.iter().enumerate().map(|(i, d)| {
        let marker = if i == idx { "»" } else { " " };
        let style = if i == idx {
            Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)
        } else { Style::default() };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(Color::Yellow)),
            Span::raw(truncate(d, 110)),
        ])).style(style)
    }).collect();
    frame.render_widget(
        List::new(items).block(title_block(format!(" Decision Audit — {} entries (newest first) ", entries.len()))),
        chunks[0],
    );

    let detail = if let Some(d) = entries.get(idx) {
        vec![plain(truncate(d, 500))]
    } else {
        vec![plain("No decisions recorded.")]
    };
    frame.render_widget(Paragraph::new(detail).block(title_block(" Decision Detail ")), chunks[1]);
}

// ---------------------------------------------------------------------------
// Tests — render every mode through a headless backend
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn status(paused: bool) -> AppStatus {
        AppStatus {
            paused,
            ticks_per_second: 30,
            status_msg: None,
        }
    }

    fn view_data_from_world(world: &alc_world::World) -> ViewData {
        let observation = sumo::Observation::capture(world);
        let prisoners = world.prisoners.iter().map(|p| PrisonerSummary {
            id: p.id,
            name: p.name.clone(),
            age: p.age,
            classification: p.classification,
            sentence_served_days: p.sentence_served_days,
            sentence_days: p.sentence_days,
            gang_affiliation: p.gang_affiliation,
            reputation: p.reputation,
            condition: p.condition.clone(),
            traits: p.traits.clone(),
            needs: p.needs.clone(),
            goals: p.goals.clone(),
            schedule: p.schedule.clone(),
            skills: p.skills.clone(),
            memory_summary: p.memory_summary.clone(),
        }).collect();
        let gangs = world.gangs.iter().map(|g| GangSummary {
            id: g.id.0,
            name: g.name.clone(),
            leader: g.leader,
            members: g.members.clone(),
            reputation: g.reputation,
            resources: g.resources,
            culture: g.culture.clone(),
            formed_at: g.formed_at,
            active: g.active,
            hierarchy: g.hierarchy.clone(),
            rivalries: g.rivalries.iter().map(|id| (id.0, 1.0)).collect(),
            alliances: g.alliances.iter().map(|id| (id.0, 1.0)).collect(),
            internal_disputes: g.internal_disputes.clone(),
            goals: g.goals.clone(),
        }).collect();
        let guards = world.guards.iter().map(|g| GuardSummary {
            id: g.id.0,
            name: g.name.clone(),
            rank: g.rank,
            post: g.post,
            authority: g.authority,
            shift: g.shift,
            current_task: g.current_task.clone(),
            competence: g.competence,
            experience: g.experience,
            morale: g.morale,
            fatigue: g.fatigue,
            stress: g.stress,
            loyalty: g.loyalty,
            integrity: g.integrity,
            personality: g.personality,
            history: g.history.clone(),
        }).collect();
        let warden = WardenSummary {
            name: world.warden.name.clone(),
            philosophy: world.warden.philosophy.clone(),
            traits: world.warden.traits.clone(),
            attention: world.warden.attention,
            inbox: Vec::new(),
            pending_actions: world.warden.pending_actions.clone(),
            last_decision_at: world.warden.last_decision_at,
        };
        let economy = EconomySummary {
            cash_major: world.economy.cash.as_major(),
            health: world.economy.health(),
            funding_per_prisoner_per_day: world.economy.funding_per_prisoner_per_day.as_major(),
            crowding_threshold: world.economy.crowding_threshold,
            ledger: world.economy.ledger.clone(),
        };
        let mut social_edges = Vec::new();
        for ((a,b), edge) in world.social.iter() {
            social_edges.push(SocialEdge {
                prisoner_a: alc_core::PrisonerId(a),
                prisoner_b: alc_core::PrisonerId(b),
                friendship: edge.friendship,
                trust: edge.trust,
                fear: edge.fear,
                hostility: edge.hostility,
                rivalry: edge.rivalry,
                bond_score: (edge.friendship + edge.trust) / 2.0,
                threat_score: (edge.hostility + edge.rivalry + edge.fear) / 3.0,
            });
        }
        let facilities = world.facilities.iter().map(|f| FacilitySummary {
            id: f.id.0,
            kind: f.kind,
            wing: 0,
            capacity: f.capacity,
            condition: f.condition,
            staff_required: 0,
        }).collect();
        let events: Vec<WorldEvent> = world.events.recent().cloned().collect();
        ViewData { observation, prisoners, gangs, guards, warden, economy, social_edges, facilities, events, incident_log: Vec::new(), decision_log: Vec::new() }
    }

    fn render_all(world: &alc_world::World, width: u16, height: u16, scroll: u16) {
        let data = view_data_from_world(world);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        for (i, mode) in ALL_MODES.iter().enumerate() {
            let ui = UiState {
                mode: *mode,
                scroll,
                ..UiState::new()
            };
            terminal
                .draw(|frame| draw(frame, &data, &ui, &status(i % 2 == 0)))
                .unwrap();
        }
    }

    #[test]
    fn every_view_renders_without_panic() {
        let world = alc_world::World::new(0xA1C_0A_6E_17);
        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(140, 45)).unwrap();
        for (i, mode) in ALL_MODES.iter().enumerate() {
            let ui = UiState {
                mode: *mode,
                scroll: i as u16,
                ..UiState::new()
            };
            terminal
                .draw(|frame| draw(frame, &data, &ui, &status(i % 2 == 0)))
                .unwrap();
        }
    }

    #[test]
    fn empty_world_renders_every_view() {
        let world = alc_world::World::new(1);
        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| draw(frame, &data, &UiState::new(), &status(true)))
            .unwrap();
    }

    #[test]
    fn sparse_world_renders_every_view() {
        let mut world = alc_world::World::new(42);
        world.events = alc_events::EventBus::new(50_000);
        world.social = alc_social::SocialGraph::default();
        world.gangs.clear();
        if let Some(p) = world.prisoners.first_mut() {
            p.gang_affiliation = Some(9_999);
        }
        for (w, h) in [(80u16, 24u16), (140, 45), (40, 12)] {
            render_all(&world, w, h, 0);
        }
    }

    #[test]
    fn huge_population_renders_every_view() {
        let mut world = alc_world::World::new(7);
        let template = world.prisoners[0].clone();
        for i in 0..2_000u64 {
            let mut p = template.clone();
            p.id = alc_core::PrisonerId(1_000 + i);
            p.name = format!("Inmate {i}");
            p.gang_affiliation = Some(i % 17);
            world.prisoners.push(p);
        }
        render_all(&world, 140, 45, 0);
        render_all(&world, 140, 45, u16::MAX);
    }

    #[test]
    fn max_cursor_does_not_overflow_paged_titles() {
        let world = alc_world::World::new(9);
        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        for mode in [ViewMode::Financial, ViewMode::SocialNetwork] {
            let ui = UiState { mode, scroll: u16::MAX, ..UiState::new() };
            terminal.draw(|frame| draw(frame, &data, &ui, &status(false))).unwrap();
        }
    }

    #[test]
    fn mode_index_mapping_is_total() {
        for i in 0..11 { assert!(mode_from_index(i).is_some()); }
        assert_eq!(mode_from_index(11), None);
        assert_eq!(mode_from_index(usize::MAX), None);
    }

    #[test]
    fn help_hints_cover_the_bound_keys() {
        for help in [HELP_WIDE, HELP_COMPACT] {
            assert!(help.contains("1-9"));
            assert!(help.contains("q"));
            assert!(help.contains("+/-"));
            assert!(help.contains("↑↓"));
        }
        assert!(HELP_WIDE.contains("SPACE"));
    }

    #[test]
    fn control_hints_are_visible_on_narrow_terminals() {
        let world = alc_world::World::new(3);
        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &data, &UiState::new(), &status(false))).unwrap();
        let buf = terminal.backend().buffer();
        let hint_row: String = (0..buf.area.width).map(|x| buf[(x, 1)].symbol().to_string()).collect();
        assert!(hint_row.contains("1-9"));
        assert!(hint_row.contains("q"));
        assert!(hint_row.contains("+/-"));
    }

    #[test]
    fn paged_views_show_data_at_mid_cursor() {
        let mut world = alc_world::World::new(11);
        world.economy.ledger.clear();
        for i in 0..500u32 {
            world.economy.ledger.push(alc_economy::LedgerEntry {
                at_secs: u64::from(i) * 86_400,
                account: alc_economy::Account::PerCapitaFunding,
                amount: alc_economy::Money::from_major(i64::from(i) + 1),
                note: format!("ENTRY_{i}"),
            });
        }
        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
        for scroll in [250u16, 499] {
            let ui = UiState { mode: ViewMode::Financial, scroll, ..UiState::new() };
            terminal.draw(|frame| draw(frame, &data, &ui, &status(false))).unwrap();
            let buf = terminal.backend().buffer();
            let mut rendered = String::new();
            for y in 0..buf.area.height {
                for x in 0..buf.area.width { rendered.push_str(buf[(x, y)].symbol()); }
            }
            assert!(rendered.contains("ENTRY_"), "ledger rows missing at cursor {scroll}");
        }
    }

    #[test]
    fn helpers_format_bounded_values() {
        assert_eq!(class_short(Classification::Maximum), "MAX");
        assert_eq!(truncate("abcdef", 3), "ab…");
        assert_eq!(truncate("abc", 10), "abc");
        let line = bar_line("Hunger", 2.0, 4);
        assert!(format!("{line:?}").contains('█'));
    }
}
