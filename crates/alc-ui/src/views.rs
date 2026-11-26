//! Read-only renderers for the observation modes (Directive §20).
//!
//! Every function in this module works against observation/summaries from the
//! engine and a [`UiState`] cursor. No function mutates simulation state: the
//! observer is strictly read-only (Directive §19). View order, colors and map
//! geometry come from `sumo::graphics`; only terminal glyphs and compact
//! captions are defined locally.

use crate::{CONSTRUCTION_KINDS, UiState, ViewMode};
use alc_prisoner::Classification;
use ratatui::prelude::*;
use ratatui::widgets::*;
use std::collections::HashMap;
use sumo::graphics::{
    Color as SharedColor, MapBounds, MapCamera, Palette, facility_glyph as shared_facility_glyph,
};
use sumo::{
    EconomySummary, FacilitySummary, GangSummary, GuardSummary, MapFrame, Observation,
    PrisonerSummary, SocialEdge, SocialRoleSummary, WardenSummary, WorldEvent,
};

/// Bundled data the views need for rendering.
pub struct ViewData {
    pub observation: Observation,
    /// Live map snapshot (pawns/facilities/edges) for the spatial map view.
    pub map_frame: MapFrame,
    /// True world totals (unfiltered by the observer search); used by titles
    /// that report both visible and total counts.
    pub total_prisoners: usize,
    pub total_gangs: usize,
    pub total_guards: usize,
    pub prisoners: Vec<PrisonerSummary>,
    pub gangs: Vec<GangSummary>,
    pub guards: Vec<GuardSummary>,
    pub warden: WardenSummary,
    /// Runtime-only AI orchestration policy and telemetry for observer surfaces.
    pub ai_runtime: sumo::AiRuntimeSnapshot,
    pub economy: EconomySummary,
    pub social_edges: Vec<SocialEdge>,
    /// Detected roles, ordered by prisoner id then role label.
    pub social_roles: Vec<SocialRoleSummary>,
    pub facilities: Vec<FacilitySummary>,
    pub construction_queue: Vec<sumo::ConstructionSummary>,
    /// Full filtered event history for the timeline view.
    pub events: Vec<WorldEvent>,
    /// Capped tail for capped previews (loaded through `recent_events`).
    pub recent_events: Vec<WorldEvent>,
    pub incident_log: Vec<WorldEvent>,
    /// Incident rows visible in the list/summary (search + filter applied).
    pub incident_records: Vec<sumo::IncidentRecord>,
    /// Unfiltered prisoner names for detail panels, even when lists are searched.
    pub all_prisoners: Vec<PrisonerSummary>,
    /// Unfiltered incident records for causal links/history in detail panels.
    pub all_incident_records: Vec<sumo::IncidentRecord>,
    pub decision_audit: Vec<sumo::DecisionAuditRecord>,
    pub policy: sumo::PolicySummary,
    pub post_release_records: Vec<sumo::PostReleaseSummary>,
    pub rehab_summaries: Vec<sumo::RehabEnrollmentSummary>,
    pub rehab_program_catalog: Vec<sumo::RehabProgramInfo>,
    pub rehab_overview: sumo::RehabOverview,
    pub contraband_cases: Vec<sumo::ContrabandCaseSummary>,
    pub contraband_inventory: Vec<sumo::ContrabandInventorySummary>,
    pub budget_snapshot: sumo::BudgetSnapshot,
    pub selected_schedule: Option<sumo::ScheduleTimeline>,
    /// Prisoners currently eligible for parole (spec 006 control surface).
    pub parole_due: Vec<u64>,
    /// Parole status of the selected prisoner.
    pub selected_parole: Option<sumo::ParoleStatus>,
    /// Conditions attached to the selected prisoner's grant.
    pub selected_parole_conditions: Option<sumo::ParoleConditions>,
    /// Persisted decision reason for the selected prisoner.
    pub selected_parole_reason: Option<String>,
    /// Time at which the selected prisoner was released on parole.
    pub selected_parole_released_at: Option<sumo::SimTime>,
}

/// Extra context the status bar needs that is not part of the world snapshot.
#[derive(Debug, Clone)]
pub struct AppStatus {
    pub paused: bool,
    pub ticks_per_second: u32,
    pub status_msg: Option<String>,
    pub save_dir: String,
    pub autosave_days: u64,
    /// Global open-critical count from the driver, independent of filters.
    pub open_critical: usize,
}

/// Number of events shown in the overview preview; the timeline keeps the
/// full filtered history, but this preview only needs a bounded tail.
pub const OVERVIEW_EVENT_LIMIT: usize = 10;

/// Full status-bar help text. Measured at render time; tests assert it stays
/// in sync with the keys handled in `app::App::handle_key`.
pub const HELP_WIDE: &str = " ←↑↓→ select · 1-9,0,A views · SPACE pause · +/- speed · S save · Ctrl-L load · / search · Ctrl-W clear query · b/B intake · Tab pin · n notes · V auto-pause · N/E/K policy bands · p policy mode · L lock · U unlock (non-Warden) · x/X lockdown (non-Warden) · Ctrl-R resolve critical · Warden: c kind C queue u/U project v approve x/X cancel/fail > advance · H/G/J/Y parole (Prisoner) · t/r/R rehab · f/F contraband · m/o filters · d/D decision filter · ? help (Esc/? close) · q quit";
/// Compact status-bar help used when the terminal is too narrow for
/// [`HELP_WIDE`] (measured by [`Line::width`]).
pub const HELP_COMPACT: &str = " ↑↓ 1-9,0,A SPC +/- S Ctrl-L / b Tab n V c/C u/U x/X N/E/K p ? q";

/// TUI-only key captions for the tab bar, aligned with [`ViewMode::all`].
const MODE_KEYS: [&str; 11] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "A"];

/// Map a `1`-`9`, `0`, `A` key press to its observation mode. The order is the
/// shared canonical order from [`ViewMode::all`].
pub fn mode_from_index(index: usize) -> Option<ViewMode> {
    ViewMode::all().get(index).copied()
}

/// Compact tab caption for an 80-column terminal. These are TUI-only hints;
/// full labels come from [`ViewMode::label`] / [`ViewMode::title`], and the
/// canonical catalog itself lives in `sumo::graphics`.
fn compact_tab_label(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::InstitutionOverview => "Ovw",
        ViewMode::Financial => "Fin",
        ViewMode::EventTimeline => "Evts",
        ViewMode::SocialNetwork => "Net",
        ViewMode::IncidentReports => "Inc",
        ViewMode::DecisionAudit => "Aud",
        ViewMode::Prisoner => "Pris",
        ViewMode::Gang => "Gang",
        ViewMode::Warden => "Ward",
        _ => mode.label(),
    }
}

// ---------------------------------------------------------------------------
// Small styling helpers
// ---------------------------------------------------------------------------

/// Shared palette for terminal rendering. `sumo` owns the color decisions;
/// this backend only maps them onto ratatui's native color type.
fn palette() -> Palette {
    Palette::default()
}

/// Convert a shared RGBA color to ratatui. Terminal cells have no alpha
/// channel, so the alpha component is intentionally dropped.
fn to_tui_color(color: SharedColor) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

/// Shared good/danger color for a binary state (surplus/deficit, active/...).
fn state_color(ok: bool) -> Color {
    to_tui_color(if ok { palette().good } else { palette().danger })
}

fn title_block(title: impl Into<String>) -> Block<'static> {
    let title = title.into();
    Block::default().borders(Borders::ALL).title(Span::styled(
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
        Span::styled(format!("{label:<22}"), Style::default().fg(Color::DarkGray)),
        Span::raw(value.into()),
    ])
}

/// Colour for a *deficit* pressure: high is bad (shared severity scale).
fn urgency_color(v: f32) -> Color {
    to_tui_color(palette().severity(v))
}

/// Colour for a *capacity* metric: high is good (shared quality scale).
fn good_color(v: f32) -> Color {
    to_tui_color(palette().quality(v))
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
                Span::styled(format!("[{}] ", e.at), Style::default().fg(Color::DarkGray)),
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

    if ui.show_notifications {
        render_observer_overlay(frame, frame.area(), ui);
    }
    if ui.show_help {
        render_help_overlay(frame, frame.area(), ui);
    }
}

/// Build the scrollable help body as titled sections. Split into its own
/// function so render code and tests share the facts being displayed.
fn help_body() -> Vec<Line<'static>> {
    let title = |s: &str| {
        Line::from(Span::styled(
            format!("— {s} "),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let entry = |s: &str| Line::from(format!("  {s}"));
    vec![
        Line::from(Span::styled(
            "=== KEYBINDINGS ===",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        title("Navigation"),
        entry(
            "1-9,0,A Switch views (2=Prisoner 3=Gang 4=Staff 5=Warden 6=Financial 7=Timeline 8=Network 9=Map 0=Incidents A=Decisions)",
        ),
        entry("↑↓,h,l,j,k Scroll lists (h/l also scroll the help overlay)"),
        entry("q,Q,Esc,Ctrl-C Quit outside help"),
        Line::from(""),
        title("Simulation"),
        entry("SPACE Pause/resume · +/- Speed · S Save · Ctrl-L Quick load"),
        entry("b/B Pause/resume intake"),
        Line::from(""),
        title("Observer"),
        entry(
            "/ Search · Ctrl-W Clear query · Tab Pin entity · n Notes feed · V Auto-pause on critical",
        ),
        entry("Ctrl-R Resolve selected/newest open critical incident (also while searching)"),
        entry("Ctrl-C Quit · Ctrl-L Quick load (also works while searching)"),
        Line::from(""),
        title("Policy"),
        entry("N/E/K Policy bands (Normal / Enhanced / Lockdown)"),
        entry("p Policy mode (Council / Shadow / Deterministic)"),
        entry("L Lock · U Unlock (outside Warden view)"),
        entry("x/X Toggle lockdown (outside Warden view)"),
        entry("[ ] , . Security/privileges sliders · ; ' Rehab funding"),
        Line::from(""),
        title("Construction (Warden view)"),
        entry(
            "c Cycle build kind · C Queue project · u/U Select project · v Approve · x Cancel · X Fail · > Advance",
        ),
        Line::from(""),
        title("Parole (Prisoner view)"),
        entry("H Schedule hearing · G Grant · J Deny · Y Release on granted parole"),
        Line::from(""),
        title("Rehab (Prisoner/Warden views)"),
        entry("t Cycle program · r Enroll selected · R Drop from program"),
        Line::from(""),
        title("Incidents (Incident view)"),
        entry("f Next case · F Resolve selected · m/M Severity filter · o/O Resolved filter"),
        Line::from(""),
        title("Decisions (Decision view)"),
        entry("d/D Decision validation filter"),
        Line::from(""),
        Line::from(Span::styled(
            "↑/↓ (or h/j/k/l) scrolls the help overlay; Esc or ? closes it.",
            Style::default().fg(Color::DarkGray),
        )),
    ]
}

fn render_help_overlay(frame: &mut Frame, area: Rect, ui: &UiState) {
    let body = help_body();
    let visible = area.height.saturating_sub(3).max(1) as usize;
    let max_scroll = body.len().saturating_sub(visible).min(u16::MAX as usize) as u16;
    let scroll = ui.help_scroll.min(max_scroll);
    let lines: Vec<Line> = body.into_iter().skip(scroll as usize).collect();
    let block = Block::default()
        .title(" Help (↑/↓ scrolls) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
    let area = centered_rect(80, 80, area);
    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_observer_overlay(frame: &mut Frame, area: Rect, ui: &UiState) {
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            "=== OBSERVER FEED ===",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!(
            "  Search: {}",
            if ui.search.is_empty() {
                "none".to_string()
            } else {
                ui.search.clone()
            }
        )),
        Line::from(format!(
            "  Auto-pause on critical: {}  ·  Pins: {}",
            if ui.pause_on_critical { "ON" } else { "off" },
            ui.bookmarks.len()
        )),
        Line::from(format!(
            "  Bookmarks: {}",
            if ui.bookmarks.is_empty() {
                "none".to_string()
            } else {
                ui.bookmarks
                    .iter()
                    .take(12)
                    .map(|id| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Notifications (newest first)",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    if ui.notifications.is_empty() {
        lines.push(Line::from("  No notifications yet."));
    } else {
        for msg in ui.notifications.iter().rev().take(10) {
            lines.push(Line::from(format!("  • {msg}")));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press n to close. Tab pins the selected entity.",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(" Observer Feed ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let area = centered_rect(70, 70, area);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

/// Shrink a span vector so its measured total fits `width`. Measured with
/// [`Span::width`] so unicode labels count correctly; trailing separators are
/// dropped first, then the last label is truncated if needed.
fn fit_spans(mut spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    while spans.iter().map(Span::width).sum::<usize>() > width && spans.len() > 1 {
        spans.pop();
    }
    // Room left in `width` after every span except the last.
    let rest: usize = spans
        .iter()
        .take(spans.len().saturating_sub(1))
        .map(Span::width)
        .sum();
    let room = width.saturating_sub(rest);
    if let Some(last) = spans.last_mut() {
        if last.width() > room {
            *last = Span::styled(truncate(&last.content, room), last.style);
        }
    }
    spans
}

fn render_tabbar(frame: &mut Frame, area: Rect, active: ViewMode) {
    // `area` is two rows tall; row 0 holds the tabs, row 1 holds a persistent
    // control hint. Keeping the hint out of the status bar means it is never
    // clipped by the population / finance readouts on narrow terminals.
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    let tabs_area = rows[0];
    let width = tabs_area.width as usize;
    let build = |compact: bool| -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, mode) in ViewMode::all().iter().enumerate() {
            let is_active = *mode == active;
            let style = if is_active {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let key = MODE_KEYS.get(i).copied().unwrap_or("?");
            let label = if compact {
                format!("{} {}", key, compact_tab_label(*mode))
            } else {
                format!(" {} {} ", key, mode.title())
            };
            spans.push(Span::styled(label, style));
            spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
        }
        spans
    };
    // Prefer the full labels when they measure in; fall back to short ones.
    let spans = fit_spans(build(false), width);
    frame.render_widget(Paragraph::new(Line::from(spans)), tabs_area);

    // Choose the status-bar hint by its *measured* width so the row always
    // fits the terminal, not by a hard-coded threshold.
    let hint_line = Line::from(Span::styled(
        HELP_WIDE,
        Style::default().fg(Color::DarkGray),
    ));
    let hint_text = if hint_line.width() <= width {
        HELP_WIDE
    } else {
        HELP_COMPACT
    };
    let hint_span = Span::styled(
        truncate(hint_text, width),
        Style::default().fg(Color::DarkGray),
    );
    frame.render_widget(Paragraph::new(Line::from(vec![hint_span])), rows[1]);
}

fn render_status(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState, status: &AppStatus) {
    let obs = &data.observation;
    let pop = obs.population;
    let cap = obs.capacity;
    let cash = obs.cash_major;
    let health = obs.economy_health;

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(
        format!(" {} ", ui.mode.title()),
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
            .bg(to_tui_color(palette().danger))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Black)
            .bg(to_tui_color(palette().good))
            .add_modifier(Modifier::BOLD)
    };
    spans.push(Span::raw(" │ "));
    spans.push(Span::styled(
        if status.paused {
            " PAUSED "
        } else {
            " RUNNING "
        },
        run_style,
    ));
    spans.push(Span::styled(
        format!(" {} tps ", status.ticks_per_second),
        Style::default().fg(Color::Magenta),
    ));
    if obs.lockdown {
        spans.push(Span::styled(
            " LOCKDOWN ",
            Style::default()
                .fg(Color::Black)
                .bg(to_tui_color(palette().danger))
                .add_modifier(Modifier::BOLD),
        ));
    }

    let autosave = if status.autosave_days == 0 {
        "off".to_string()
    } else {
        format!("{}d", status.autosave_days)
    };
    spans.push(Span::raw(" │ "));
    spans.push(Span::styled(
        format!("Save:{} Auto:{}", truncate(&status.save_dir, 16), autosave),
        Style::default().fg(Color::DarkGray),
    ));
    spans.push(Span::raw(" │ "));
    spans.push(Span::styled(
        format!(
            "Pins:{} CritPause:{}",
            ui.bookmarks.len(),
            if ui.pause_on_critical { "on" } else { "off" }
        ),
        Style::default().fg(Color::DarkGray),
    ));
    if let Some(msg) = &status.status_msg {
        spans.push(Span::raw(" │ "));
        spans.push(Span::styled(
            truncate(msg, 40),
            Style::default().fg(Color::Cyan),
        ));
    }
    if status.open_critical > 0 {
        spans.push(Span::raw(" │ "));
        spans.push(Span::styled(
            format!("CRITICAL:{} open", status.open_critical),
            Style::default()
                .fg(Color::Black)
                .bg(to_tui_color(palette().danger))
                .add_modifier(Modifier::BOLD),
        ));
    }
    let widget =
        Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Reset).fg(Color::White));
    frame.render_widget(widget, area);
}

// ---------------------------------------------------------------------------
// 1 — Institution overview
// ---------------------------------------------------------------------------

fn render_overview(frame: &mut Frame, area: Rect, data: &ViewData) {
    let obs = &data.observation;
    let chunks = Layout::vertical([
        Constraint::Length(13),
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
        kv(
            "Rehab",
            format!(
                "{} enroll / {} prisoners / {:.0}% avg",
                data.rehab_overview.enrollments,
                data.rehab_overview.enrolled_prisoners,
                data.rehab_overview.average_progress * 100.0
            ),
        ),
        kv(
            "Contraband/Post-release",
            format!(
                "{} cases / {} records",
                data.contraband_cases.len(),
                data.post_release_records.len()
            ),
        ),
        bar_line("Security level", data.policy.security_level, 18),
        bar_line("Privileges", data.policy.privileges_level, 18),
        bar_line("Economy health", obs.economy_health as f32, 18),
        kv(
            "Rehab funding",
            format!("{:.2}x", data.policy.rehab_funding_multiplier),
        ),
        kv(
            "Lockdown / Intake",
            format!(
                "{} / {}",
                if data.policy.lockdown {
                    "ACTIVE"
                } else {
                    "none"
                },
                if data.policy.intake_paused {
                    "paused"
                } else {
                    "open"
                }
            ),
        ),
    ];
    frame.render_widget(
        Paragraph::new(summary).block(title_block(" Institution Overview ")),
        chunks[0],
    );

    let mid = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[1]);
    let events = recent_event_lines(&data.recent_events, OVERVIEW_EVENT_LIMIT);
    frame.render_widget(
        Paragraph::new(events).block(title_block(" Recent Events ")),
        mid[0],
    );

    let w = &data.warden;
    let mut warden_lines = vec![
        kv("Warden", w.name.clone()),
        kv("Philosophy", truncate(&w.philosophy, 46)),
        good_bar("Administrative focus", w.attention, 16),
        kv("Reports inbox", format!("{} guard reports", w.inbox.len())),
        kv("Pending actions", w.pending_actions.len().to_string()),
        kv("Last decision", w.last_decision_at.to_string()),
        kv("Treasury", format!("${:.2}", data.economy.cash_major)),
    ];
    warden_lines.push(plain("--- Recent critical events ---"));
    let mut critical_events: Vec<&sumo::IncidentRecord> = data
        .incident_records
        .iter()
        .filter(|r| r.severity >= 0.4)
        .collect();
    critical_events.sort_by(|a, b| b.at.cmp(&a.at));
    if critical_events.is_empty() {
        warden_lines.push(plain("No critical incidents recorded."));
    } else {
        for r in critical_events.iter().take(5) {
            let status = if r.resolved { "RES" } else { "OPEN" };
            warden_lines.push(plain(format!(
                "[{}] {} sev {:.0}% {} ({})",
                r.at,
                truncate(&r.kind, 12),
                r.severity * 100.0,
                truncate(&r.location, 20),
                status
            )));
        }
    }
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
        Row::new(vec!["ID", "Facility", "Wing", "Cap", "Cond", "Staff"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(title_block(" Facilities "));
    frame.render_widget(table, chunks[2]);
}

// ---------------------------------------------------------------------------
// 2 — Prisoner profiles
// ---------------------------------------------------------------------------

fn render_prisoner(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks = Layout::horizontal([Constraint::Length(30), Constraint::Min(30)]).split(area);

    // `data.prisoners` is already filtered by the observer search (see
    // `App::visible_prisoners`) — do not filter again here.
    let filtered: &[PrisonerSummary] = &data.prisoners;
    let idx = (ui.scroll as usize).min(filtered.len().saturating_sub(1));
    let items: Vec<ListItem<'static>> = filtered
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
                Span::styled(
                    class_short(p.classification),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
            .style(style)
        })
        .collect();
    let title = if ui.search.is_empty() {
        format!(
            " Prisoners ({} of {}) ",
            filtered.len(),
            data.total_prisoners
        )
    } else {
        format!(
            " Prisoners ({} of {}) q='{}' ",
            filtered.len(),
            data.total_prisoners,
            truncate(&ui.search, 12)
        )
    };
    let list = List::new(items).block(title_block(title));
    frame.render_widget(list, chunks[0]);

    if let Some(p) = filtered.get(idx) {
        render_prisoner_detail(frame, chunks[1], p, data, ui);
    } else {
        frame.render_widget(
            Paragraph::new(if data.prisoners.is_empty() {
                "No prisoners in custody."
            } else {
                "No prisoners match search."
            })
            .block(title_block(" Prisoner Profile ")),
            chunks[1],
        );
    }
}

fn render_prisoner_detail(
    frame: &mut Frame,
    area: Rect,
    p: &PrisonerSummary,
    data: &ViewData,
    ui: &UiState,
) {
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
        kv("ID / Age", format!("{} / {} years", p.id.0, p.age)),
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
        bar_line(
            "Injury / Illness",
            (p.condition.injury + p.condition.illness) / 2.0,
            14,
        ),
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
    frame.render_widget(
        Paragraph::new(need_lines(&p.needs)).block(title_block(" Needs (deficit pressure) ")),
        columns[1],
    );

    let lower = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);

    let mut goals: Vec<Line<'static>> = Vec::new();
    for g in &p.goals {
        goals.push(Line::from(vec![
            Span::styled(format!("{:?}", g.kind), Style::default().fg(Color::Yellow)),
            Span::raw(format!(
                " pro {:.2} urg {:.2} — {}",
                g.progress,
                g.urgency,
                truncate(&g.description, 40)
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

    if let Some(tl) = data
        .selected_schedule
        .as_ref()
        .filter(|t| t.prisoner_id == p.id)
    {
        goals.push(plain(format!(
            "--- Daily Timeline day {} {:02}:{:02} att {:.0}% ---",
            tl.day,
            tl.current_minute / 60,
            tl.current_minute % 60,
            tl.attendance_probability * 100.0
        )));
        for b in &tl.blocks {
            let marker = if b.current { ">" } else { " " };
            let status = if b.cancelled {
                format!(
                    "CANCELLED{}",
                    b.reason
                        .as_deref()
                        .map(|r| format!(" ({r})"))
                        .unwrap_or_default()
                )
            } else {
                b.activity.clone()
            };
            goals.push(plain(format!(
                "{} {:02}:{:02}-{:02}:{:02} {} — {}",
                marker,
                b.start_minute / 60,
                b.start_minute % 60,
                b.end_minute / 60,
                b.end_minute % 60,
                b.label,
                status
            )));
        }
    } else {
        goals.push(plain("--- Daily Timeline (inferred) ---"));
        let work = p
            .schedule
            .work_assignment
            .clone()
            .unwrap_or_else(|| "idle".into());
        let disc = if p.schedule.disciplinary_status {
            " (restricted)"
        } else {
            ""
        };
        goals.push(plain(format!("06:00 Wake & count{disc}")));
        goals.push(plain("07:00 Breakfast"));
        if p.schedule.education {
            goals.push(plain(format!("08:00-12:00 Education / {work}")));
        } else {
            goals.push(plain(format!("08:00-12:00 Work: {work}")));
        }
        goals.push(plain("12:00 Lunch"));
        goals.push(plain("13:00-15:00 Recreation / yard"));
        if p.schedule.education {
            goals.push(plain("15:00 Programs & counseling"));
        }
        goals.push(plain("17:00 Dinner"));
        goals.push(plain("18:00 Free time / social"));
        goals.push(plain("21:00 Lockup"));
    }
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

    tail.push(plain("--- Rehab (0.05) t:cycle r:enroll R:drop ---"));
    if data.rehab_program_catalog.is_empty() {
        tail.push(plain("  no rehab catalog available"));
    } else {
        let sel_idx = ui.selected_rehab.unwrap_or(0) as usize % data.rehab_program_catalog.len();
        let on_hold = p.schedule.disciplinary_status;
        tail.push(plain(format!(
            "  eligibility: {} · funding {:.2}x · {} program(s)",
            if on_hold {
                "on hold (disciplinary)"
            } else {
                "open"
            },
            data.policy.rehab_funding_multiplier,
            data.rehab_program_catalog.len()
        )));
        for (i, prog) in data.rehab_program_catalog.iter().enumerate() {
            let marker = if i == sel_idx { ">" } else { " " };
            let enrolled = data
                .rehab_summaries
                .iter()
                .any(|r| r.prisoner_id == p.id && r.program_key == prog.key);
            let rollup = data
                .rehab_overview
                .programs
                .iter()
                .find(|r| r.key == prog.key);
            let seats = rollup.map(|r| r.enrolled).unwrap_or(0);
            tail.push(plain(format!(
                "{marker}{} diff {:.0}% seats {} {}{}",
                prog.name,
                prog.difficulty * 100.0,
                seats,
                if enrolled { "[enrolled]" } else { "" },
                if on_hold && !enrolled { " [HOLD]" } else { "" }
            )));
        }
    }
    let mut rehab_rows: Vec<_> = data
        .rehab_summaries
        .iter()
        .filter(|r| r.prisoner_id == p.id)
        .collect();
    rehab_rows.sort_by(|a, b| a.program.cmp(&b.program));
    if rehab_rows.is_empty() {
        tail.push(plain("  active: none"));
    }
    for r in rehab_rows.iter().take(4) {
        tail.push(kv(
            &r.program,
            format!(
                "{:.0}%{}",
                r.progress * 100.0,
                if r.disciplinary { " disc" } else { "" }
            ),
        ));
    }

    tail.push(plain("--- Contraband (0.05) ---"));
    let mut cases: Vec<_> = data
        .contraband_cases
        .iter()
        .filter(|c| c.holder == Some(p.id))
        .collect();
    cases.sort_by(|a, b| b.at.cmp(&a.at));
    if cases.is_empty() {
        tail.push(plain("  no cases"));
    }
    for c in cases.iter().take(4) {
        tail.push(plain(format!(
            "d{} {} sev {:.1}{}",
            c.at.as_days(),
            c.item.as_deref().unwrap_or("Unknown"),
            c.severity,
            if c.resolved { " resolved" } else { "" }
        )));
    }

    tail.push(plain(
        "--- Parole (0.05) H:hearing G:grant J:deny Y:release ---",
    ));
    let due = data.parole_due.contains(&p.id.raw());
    tail.push(kv(
        "Eligibility",
        if due { "due for review" } else { "not due" }.to_string(),
    ));
    tail.push(kv(
        "Status",
        data.selected_parole
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|| "none".into()),
    ));
    if let Some(reason) = data.selected_parole_reason.as_deref() {
        tail.push(kv("Decision", truncate(reason, 48)));
    }
    if let Some(c) = data.selected_parole_conditions.as_ref() {
        tail.push(plain(format!(
            "  Conditions: report {}/mo · {}mo supervision · curfew={} drug={} treat={} nocontact={}",
            c.reporting_per_month, c.supervision_months, c.curfew, c.drug_testing, c.treatment_required, c.no_contact_order
        )));
    }
    if let Some(at) = data.selected_parole_released_at {
        tail.push(kv("Released", format!("day {}", at.as_days())));
    }

    if let Some(pr) = data
        .post_release_records
        .iter()
        .find(|r| r.prisoner_id == p.id)
    {
        tail.push(plain("--- Post-release ---"));
        tail.push(kv(
            "Outcome",
            if pr.successful_reintegration {
                "successful"
            } else {
                "follow-up"
            }
            .to_string(),
        ));
        if let Some(risk) = pr.recidivism_risk {
            tail.push(kv("Recidivism", format!("{:.0}%", risk * 100.0)));
        }
    }

    tail.push(plain("--- Memory ---"));
    let memory = if p.memory_summary.is_empty() {
        "  (no significant memories yet)".to_string()
    } else {
        truncate(&p.memory_summary, 300)
    };
    for part in memory.lines() {
        tail.push(plain(part.to_string()));
    }
    frame.render_widget(
        Paragraph::new(tail).block(title_block(" Skills, Rehab & Memory ")),
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
    let chunks = Layout::horizontal([Constraint::Length(30), Constraint::Min(30)]).split(area);

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

    // `data.gangs` is already filtered by the observer search.
    let filtered: &[GangSummary] = &data.gangs;
    let idx = (ui.scroll as usize).min(filtered.len().saturating_sub(1));
    let names = name_map(&data.all_prisoners);
    let items: Vec<ListItem<'static>> = filtered
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
                    Style::default().fg(state_color(g.active)),
                ),
            ]))
            .style(style)
        })
        .collect();
    let gtitle = if ui.search.is_empty() {
        format!(" Gangs ({} of {}) ", filtered.len(), data.total_gangs)
    } else {
        format!(
            " Gangs ({} of {}) q='{}' ",
            filtered.len(),
            data.total_gangs,
            truncate(&ui.search, 10)
        )
    };
    frame.render_widget(List::new(items).block(title_block(gtitle)), chunks[0]);

    let Some(g) = filtered.get(idx) else {
        frame.render_widget(
            Paragraph::new(if ui.search.is_empty() {
                "Select a gang with ↑/↓."
            } else {
                "No gangs match search."
            })
            .block(title_block(" Gang Detail ")),
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
        bar_line("Pressure", g.pressure.total(), 16),
    ];
    let detail_chunks = Layout::vertical([
        Constraint::Length(12),
        Constraint::Min(5),
        Constraint::Length(7),
        Constraint::Min(4),
    ])
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

    let relation_scores = if g.relations.is_empty() {
        "none".to_string()
    } else {
        truncate(
            &g.relations
                .iter()
                .map(|r| format!("#{} a{:.2}/h{:.2}", r.gang, r.affinity, r.hostility))
                .collect::<Vec<_>>()
                .join("; "),
            60,
        )
    };
    let tail = vec![
        kv(
            "Rivals",
            format!(
                "{:?}",
                g.rivalries
                    .iter()
                    .map(|(id, score)| format!("#{id}:{score:.2}"))
                    .collect::<Vec<_>>()
            ),
        ),
        kv(
            "Allies",
            format!(
                "{:?}",
                g.alliances
                    .iter()
                    .map(|(id, score)| format!("#{id}:{score:.2}"))
                    .collect::<Vec<_>>()
            ),
        ),
        kv("Scores", relation_scores),
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

    let history: Vec<Line<'static>> = if g.history.is_empty() {
        vec![plain("No recorded gang transitions yet.")]
    } else {
        g.history
            .iter()
            .rev()
            .take(4)
            .map(|entry| {
                plain(format!(
                    "{} {}",
                    entry.kind.label(),
                    truncate(&entry.summary, 60)
                ))
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(history).block(title_block(" Gang History ")),
        detail_chunks[3],
    );
}

// ---------------------------------------------------------------------------
// 4 — Staff / guards
// ---------------------------------------------------------------------------

fn render_staff(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks = Layout::horizontal([Constraint::Length(30), Constraint::Min(30)]).split(area);
    // `data.guards` is already filtered by the observer search.
    let filtered: &[GuardSummary] = &data.guards;
    let idx = (ui.scroll as usize).min(filtered.len().saturating_sub(1));

    let items: Vec<ListItem<'static>> = filtered
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
    let stitle = if ui.search.is_empty() {
        format!(
            " Staff Roster ({} of {}) ",
            filtered.len(),
            data.total_guards
        )
    } else {
        format!(
            " Staff ({} of {}) q='{}' ",
            filtered.len(),
            data.total_guards,
            truncate(&ui.search, 10)
        )
    };
    frame.render_widget(List::new(items).block(title_block(stitle)), chunks[0]);

    let Some(g) = filtered.get(idx) else {
        frame.render_widget(
            Paragraph::new(if data.guards.is_empty() {
                "No guards on duty."
            } else {
                "No staff match search."
            })
            .block(title_block(" Guard Detail ")),
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
        kv("Report credibility", format!("{:.2}", g.credibility)),
        kv(
            "Duty record",
            format!(
                "{} opinions · {} mistakes · {} violations · {} log entries",
                g.tracked_opinions,
                g.mistakes,
                g.violations,
                g.history.len()
            ),
        ),
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

fn render_warden(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    let w = &data.warden;
    let p = &data.policy;

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
    let left = Layout::vertical([Constraint::Length(7), Constraint::Min(6)]).split(chunks[0]);
    let ai = &data.ai_runtime;
    frame.render_widget(
        Paragraph::new(vec![
            kv("Warden", w.name.clone()),
            kv("Philosophy", truncate(&w.philosophy, 48)),
            good_bar("Attention", w.attention, 14),
            kv(
                "Reports / decisions",
                format!("{} / {}", w.inbox.len(), w.recent_decisions.len()),
            ),
            kv(
                "AI runtime",
                format!(
                    "{} / {} · tok {}/{} · cache {}/{} · retry {} · limited {}",
                    ai.provider,
                    ai.model,
                    ai.tokens_consumed_today,
                    ai.max_tokens_per_day,
                    ai.cache_hits,
                    ai.cache_entries,
                    ai.retries,
                    ai.rate_limited
                ),
            ),
        ])
        .block(title_block(" Warden Profile ")),
        left[0],
    );
    frame.render_widget(
        Paragraph::new(traits).block(title_block(" Leadership Traits ")),
        left[1],
    );

    let right = Layout::vertical([
        Constraint::Length(7),
        Constraint::Length(10),
        Constraint::Min(8),
        Constraint::Length(8),
    ])
    .split(chunks[1]);

    let mut action_lines: Vec<Line<'static>> = w
        .pending_actions
        .iter()
        .take(3)
        .map(|a| plain(format!("Pending: {}", a.label())))
        .collect();
    if action_lines.is_empty() {
        action_lines.push(plain("No pending actions."));
    }
    if let Some(decision) = w.recent_decisions.first() {
        action_lines.push(plain(format!(
            "Decision #{}: {}",
            decision.id,
            decision.action.label()
        )));
        action_lines.push(plain(format!(
            "  {} · {}",
            decision.source.label(),
            truncate(&decision.rationale, 70)
        )));
    }
    frame.render_widget(
        Paragraph::new(action_lines).block(title_block(" Actions & Rationale ")),
        right[0],
    );

    let policy_lines = vec![
        kv("Policy band", format!("{:?}", p.policy)),
        kv(
            "Lockdown",
            if p.lockdown {
                "ENABLED".to_string()
            } else {
                "off".to_string()
            },
        ),
        good_bar("Security", p.security_level, 14),
        good_bar("Privileges", p.privileges_level, 14),
        kv(
            "Rehab funding",
            format!("{:.2}x", p.rehab_funding_multiplier),
        ),
        kv(
            "Intake",
            if p.intake_paused {
                "PAUSED".to_string()
            } else {
                "open".to_string()
            },
        ),
        kv(
            "Budget (30d)",
            format!(
                "net ${:.0}/d runway {:.0}d",
                data.budget_snapshot.projected_daily_revenue
                    - data.budget_snapshot.projected_daily_expense,
                data.budget_snapshot.runway_days
            ),
        ),
    ];
    frame.render_widget(
        Paragraph::new(policy_lines).block(title_block(" Policy Controls ")),
        right[1],
    );

    // Construction display (0.05)
    let sel_c = ui.selected_construction;
    let cq_rows: Vec<Row<'static>> = data
        .construction_queue
        .iter()
        .map(|pjt| {
            let selected = sel_c == Some(pjt.id);
            let style = if selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let status = format!("{:?}", pjt.status);
            let reason = pjt
                .terminal_reason
                .clone()
                .unwrap_or_else(|| "-".to_string());
            Row::new(vec![
                Cell::from(if selected {
                    format!("»{}", pjt.id)
                } else {
                    pjt.id.to_string()
                }),
                Cell::from(pjt.kind.display().to_string()),
                Cell::from(format!("W{}", pjt.wing)),
                Cell::from(pjt.capacity.to_string()),
                Cell::from(format!("{}/{}", pjt.progress_days, pjt.total_days)),
                Cell::from(status),
                Cell::from(truncate(&reason, 16)),
            ])
            .style(style)
        })
        .collect();
    let cq_table = Table::new(
        cq_rows,
        [
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Min(8),
        ],
    )
    .header(
        Row::new(vec!["ID", "Kind", "Wg", "Cap", "Prog", "Status", "Reason"])
            .style(Style::default().fg(Color::Yellow)),
    )
    .block(title_block(format!(
        " Construction Queue ({}) ",
        data.construction_queue.len()
    )));
    frame.render_widget(cq_table, right[2]);

    let mut detail_lines = if let Some(sel_id) = sel_c {
        if let Some(pjt) = data.construction_queue.iter().find(|p| p.id == sel_id) {
            let prereq_ok = pjt.staff_allocated && pjt.materials_ready && pjt.prereqs_met;
            vec![
                kv(
                    "Selected",
                    format!("#{} {} W{}", pjt.id, pjt.kind.display(), pjt.wing),
                ),
                kv(
                    "Progress",
                    format!("{}/{} days", pjt.progress_days, pjt.total_days),
                ),
                kv("Cost", format!("${:.0}", pjt.cost)),
                kv("Status", format!("{:?}", pjt.status)),
                kv(
                    "Terminal reason",
                    pjt.terminal_reason.clone().unwrap_or_else(|| "none".into()),
                ),
                kv(
                    "Prereqs",
                    format!(
                        "staff={} mats={} fac={} ok={}",
                        pjt.staff_allocated, pjt.materials_ready, pjt.prereqs_met, prereq_ok
                    ),
                ),
            ]
        } else {
            vec![plain(format!(
                "Selected construction #{} not in queue.",
                sel_id
            ))]
        }
    } else if let Some(pjt) = data.construction_queue.first() {
        vec![
            plain("No selected_construction; showing first queue item."),
            kv("ID/Kind", format!("#{} {}", pjt.id, pjt.kind.display())),
            kv("Cost", format!("${:.0}", pjt.cost)),
            kv(
                "Prereqs",
                format!(
                    "staff={} mats={} fac={}",
                    pjt.staff_allocated, pjt.materials_ready, pjt.prereqs_met
                ),
            ),
        ]
    } else {
        vec![plain("No construction projects in queue.")]
    };
    let sel_kind = ui.construction_kind_index % CONSTRUCTION_KINDS.len();
    let catalog = CONSTRUCTION_KINDS
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            format!(
                "{}{}",
                if i == sel_kind { ">" } else { " " },
                kind.display()
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    detail_lines.push(plain(format!("Catalog: {catalog}")));
    detail_lines.push(plain(
        "c cycle · C queue · u/U select · v approve · x cancel · X fail · > advance",
    ));
    frame.render_widget(
        Paragraph::new(detail_lines).block(title_block(" Construction Detail ")),
        right[3],
    );
}

// ---------------------------------------------------------------------------
// 6 — Finances
// ---------------------------------------------------------------------------

fn render_financial(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    // Content-sized panels: summary, drivers/warnings, then the paginated
    // ledger. `Min` (instead of `Length`) lets the panels grow to fit their
    // lines on tall terminals and shrink on short ones.
    let chunks = Layout::vertical([
        Constraint::Min(6),
        Constraint::Min(9),
        Constraint::Min(8),
        Constraint::Min(8),
    ])
    .split(area);
    let e = &data.economy;

    // Budget breakdown by account.
    let mut accounts: Vec<(alc_economy::Account, i64)> = Vec::new();
    for l in &e.ledger {
        if let Some(entry) = accounts.iter_mut().find(|(a, _)| *a == l.account) {
            entry.1 += l.amount.0;
        } else {
            accounts.push((l.account, l.amount.0));
        }
    }
    accounts.sort_by(|a, b| b.1.abs().cmp(&a.1.abs()));

    let revenue_total: i64 = accounts
        .iter()
        .filter(|(a, _)| is_revenue_account(*a))
        .map(|(_, v)| *v)
        .sum();
    let expense_total: i64 = accounts
        .iter()
        .filter(|(a, _)| !is_revenue_account(*a))
        .map(|(_, v)| *v)
        .sum();

    // Recent daily net (last 7 days) plus 30-day variance and forecast.
    let max_day = e
        .ledger
        .iter()
        .map(|l| l.at_secs / 86_400)
        .max()
        .unwrap_or(0);
    let mut daily_lines: Vec<Line<'static>> = Vec::new();
    let mut net_7: i64 = 0;
    for i in 0..7u64 {
        let day = max_day.saturating_sub(6 - i);
        let net: i64 = e
            .ledger
            .iter()
            .filter(|l| l.at_secs / 86_400 == day)
            .map(|l| l.amount.0)
            .sum();
        net_7 += net;
        daily_lines.push(Line::from(Span::styled(
            format!("D{} {:+.2}", day, net as f64 / 100.0),
            Style::default().fg(state_color(net >= 0)),
        )));
    }
    let day_30_start = max_day.saturating_sub(29);
    let net_30: i64 = e
        .ledger
        .iter()
        .filter(|l| l.at_secs / 86_400 >= day_30_start)
        .map(|l| l.amount.0)
        .sum();
    let avg_daily = net_30 as f64 / 30.0 / 100.0;
    let forecast_7 = e.cash_major + avg_daily * 7.0;
    let forecast_30 = e.cash_major + avg_daily * 30.0;

    // Cost groups for an at-a-glance expense breakdown.
    let mut cost_groups: Vec<(&'static str, i64)> = Vec::new();
    for (acct, net) in &accounts {
        if *net >= 0 {
            continue;
        }
        let group = match acct {
            alc_economy::Account::Salaries | alc_economy::Account::Admin => "Payroll/admin",
            alc_economy::Account::Maintenance
            | alc_economy::Account::Utilities
            | alc_economy::Account::Water
            | alc_economy::Account::Supplies
            | alc_economy::Account::Construction => "Facilities/ops",
            alc_economy::Account::Food | alc_economy::Account::Medical => "Care/food",
            alc_economy::Account::Rehabilitation | alc_economy::Account::Education => {
                "Rehab/education"
            }
            alc_economy::Account::SecurityEquipment => "Security",
            alc_economy::Account::Transportation => "Transport",
            _ => "Other",
        };
        if let Some(entry) = cost_groups.iter_mut().find(|(g, _)| *g == group) {
            entry.1 += *net;
        } else {
            cost_groups.push((group, *net));
        }
    }
    cost_groups.sort_by(|a, b| b.1.cmp(&a.1));

    let mut warnings: Vec<String> = Vec::new();
    if e.health < 0.3 {
        warnings.push("WARNING: financial health critical".to_string());
    } else if e.health < 0.5 {
        warnings.push("CAUTION: financial health low".to_string());
    }
    if e.cash_major < 0.0 {
        warnings.push("WARNING: negative treasury".to_string());
    }
    if avg_daily < 0.0 {
        warnings.push(format!("CAUTION: 30d net {:+.2}/day", avg_daily));
    }
    if e.funding_per_prisoner_per_day <= 0.0 {
        warnings.push("WARNING: no per-prisoner funding".to_string());
    }
    if warnings.is_empty() {
        warnings.push("No financial warnings.".to_string());
    }

    let summary = vec![
        kv("Treasury", format!("${:.2}", e.cash_major)),
        kv(
            "Funding / prisoner / day",
            format!("${:.2}", e.funding_per_prisoner_per_day),
        ),
        kv(
            "Crowding threshold",
            format!("{} prisoners", e.crowding_threshold),
        ),
        kv("Ledger entries", e.ledger.len().to_string()),
        kv(
            "Revenue (all-time)",
            format!("${:.2}", revenue_total as f64 / 100.0),
        ),
        kv(
            "Expenses (all-time)",
            format!("${:.2}", expense_total as f64 / 100.0),
        ),
        kv(
            "Net (all-time)",
            format!("${:.2}", (revenue_total + expense_total) as f64 / 100.0),
        ),
        kv(
            "Budget (30d proj/d)",
            format!(
                "inc ${:.0} exp ${:.0} net ${:.0}",
                data.budget_snapshot.projected_daily_revenue,
                data.budget_snapshot.projected_daily_expense,
                data.budget_snapshot.projected_daily_revenue
                    - data.budget_snapshot.projected_daily_expense
            ),
        ),
    ];
    frame.render_widget(
        Paragraph::new(summary).block(title_block(" Prison Economy & Budget ")),
        chunks[0],
    );

    // Trend / forecast / drivers / warnings panel — previously clipped by the
    // fixed `Length` above; now its own content-sized panel.
    let drivers = vec![
        kv(
            "Trend",
            format!(
                "7d {:+.2} · 30d {:+.2} · avg {:+.2}/d",
                net_7 as f64 / 100.0,
                net_30 as f64 / 100.0,
                avg_daily
            ),
        ),
        kv(
            "Forecast cash",
            format!("7d ${:.2} · 30d ${:.2}", forecast_7, forecast_30),
        ),
        kv(
            "Health drivers",
            format!(
                "cash {} · 30d avg {} · funding {}",
                if e.cash_major < 0.0 {
                    "negative"
                } else {
                    "positive"
                },
                if avg_daily < 0.0 {
                    "deficit"
                } else {
                    "surplus"
                },
                if e.funding_per_prisoner_per_day <= 0.0 {
                    "off"
                } else {
                    "on"
                }
            ),
        ),
        kv("Warnings", warnings.join(" · ")),
    ];
    frame.render_widget(
        Paragraph::new(drivers).block(title_block(" Trend & Drivers ")),
        chunks[1],
    );

    // Budget breakdown panel (accounts, cost groups, daily net).
    let mut breakdown_lines: Vec<Line<'static>> = Vec::new();
    breakdown_lines.push(Line::from(Span::styled(
        format!("{:<24} {:>12}", "Account", "Net"),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    for (acct, net) in accounts.iter().take(5) {
        let style = Style::default().fg(state_color(*net >= 0));
        breakdown_lines.push(Line::from(vec![
            Span::raw(format!("{:<24}", format!("{:?}", acct))),
            Span::styled(format!("{:>12.2}", *net as f64 / 100.0), style),
        ]));
    }
    if accounts.is_empty() {
        breakdown_lines.push(plain("No ledger entries yet."));
    }
    breakdown_lines.push(plain("--- Cost groups (all-time) ---"));
    if cost_groups.is_empty() {
        breakdown_lines.push(plain("  no expenses recorded"));
    } else {
        for (group, net) in cost_groups.iter().take(5) {
            breakdown_lines.push(kv(*group, format!("${:.2}", *net as f64 / -100.0)));
        }
    }
    breakdown_lines.push(plain(format!(
        "--- Daily net / forecast (30d avg {:+.2}/d) ---",
        avg_daily
    )));
    for l in daily_lines {
        breakdown_lines.push(l);
    }
    frame.render_widget(
        Paragraph::new(breakdown_lines).block(title_block(" Budget Breakdown ")),
        chunks[2],
    );

    let per_page = (chunks[3].height.saturating_sub(3) as usize).max(1);
    let offset = ui.scroll as usize;
    let rows: Vec<Row<'static>> = e
        .ledger
        .iter()
        .rev()
        .skip(offset)
        .take(per_page)
        .map(|l| {
            let day = SimDay(l.at_secs / 86_400);
            let amount_style = Style::default().fg(state_color(l.amount.0 >= 0));
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
    frame.render_widget(table, chunks[3]);
}

fn is_revenue_account(a: alc_economy::Account) -> bool {
    use alc_economy::Account;
    matches!(
        a,
        Account::PerCapitaFunding
            | Account::GovGrant
            | Account::Bonus
            | Account::ContractIncome
            | Account::RehabFunding
            | Account::SpecialProgram
    )
}

/// Tiny wrapper so the ledger day can be formatted without importing SimTime.
struct SimDay(u64);

// ---------------------------------------------------------------------------
// 7 — Event timeline
// ---------------------------------------------------------------------------

fn render_timeline(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks = Layout::vertical([Constraint::Min(4), Constraint::Length(7)]).split(area);
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
                Span::styled(format!("[{}] ", e.at), Style::default().fg(Color::DarkGray)),
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
            kv("Confidence", format!("{:.0}%", e.confidence * 100.0)),
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
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(4),
        Constraint::Length(8),
    ])
    .split(area);
    let names = name_map(&data.all_prisoners);

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
            let an = names
                .get(&e.prisoner_a.0)
                .cloned()
                .unwrap_or_else(|| format!("P{}", e.prisoner_a.0));
            let bn = names
                .get(&e.prisoner_b.0)
                .cloned()
                .unwrap_or_else(|| format!("P{}", e.prisoner_b.0));
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
            "Prisoner A",
            "Prisoner B",
            "Friend",
            "Trust",
            "Fear",
            "Hostile",
            "Rival",
            "Bond",
            "Threat",
        ])
        .style(Style::default().fg(Color::Yellow)),
    )
    .block(title_block(format!(
        " Relationship Edges — from dyad {} (↑/↓ to scroll) ",
        u32::from(ui.scroll) + 1
    )));
    frame.render_widget(table, chunks[1]);

    let role_q = ui.search.to_lowercase();
    let role_rows: Vec<Row<'static>> = data
        .social_roles
        .iter()
        .filter(|row| {
            if role_q.is_empty() {
                return true;
            }
            let name = names.get(&row.prisoner.0).map(String::as_str).unwrap_or("");
            name.to_lowercase().contains(&role_q)
                || row.role.to_lowercase().contains(&role_q)
                || row.prisoner.0.to_string().contains(&role_q)
        })
        .take(120)
        .map(|row| {
            let name = names
                .get(&row.prisoner.0)
                .cloned()
                .unwrap_or_else(|| format!("P{}", row.prisoner.0));
            Row::new(vec![
                Cell::from(truncate(&name, 18)),
                Cell::from(row.role.clone()),
                Cell::from(format!("{:.2}", row.strength.clamp(0.0, 1.0))),
            ])
        })
        .collect();
    let role_table = Table::new(
        role_rows,
        [
            Constraint::Length(18),
            Constraint::Length(18),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec!["Prisoner", "Role", "Strength"]).style(Style::default().fg(Color::Yellow)),
    )
    .block(title_block(" Emergent Social Roles "));
    frame.render_widget(role_table, chunks[2]);
}

// ---------------------------------------------------------------------------
// 9 — Facility map
// ---------------------------------------------------------------------------

/// Glyph for a facility id in the map grid. The character itself is owned by
/// `sumo::graphics`; only the ASCII byte conversion and the unknown-facility
/// fallback are terminal-local.
fn facility_glyph(id: u64, facilities: &[FacilitySummary]) -> u8 {
    facilities
        .iter()
        .find(|f| f.id == id)
        .map(|f| shared_facility_glyph(f.kind) as u8)
        .unwrap_or(b'F')
}

/// Canonical world-map bounds carried by the frame, not a facility-derived
/// crop. A unit span keeps pawns plottable before any facilities are known.
fn frame_bounds(frame: &MapFrame) -> MapBounds {
    let bounds = frame.bounds();
    if bounds.size().0 > 0.0 && bounds.size().1 > 0.0 {
        bounds
    } else {
        MapBounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 1.0,
            max_y: 1.0,
        }
    }
}

/// Camera for a terminal grid. `MapCamera::fit` clamps zoom for pixel canvases;
/// terminal cells need the exact fit computed from the shared bounds instead.
fn grid_camera(bounds: MapBounds, cols: usize, rows: usize) -> MapCamera {
    let (span_x, span_y) = bounds.size();
    let max_c = cols.saturating_sub(1).max(1) as f32;
    let max_r = rows.saturating_sub(1).max(1) as f32;
    let zoom = (max_c / span_x.max(1.0)).min(max_r / span_y.max(1.0));
    MapCamera {
        zoom,
        pan: (-bounds.min_x * zoom, -bounds.min_y * zoom),
    }
}

fn render_map(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let mf = &data.map_frame;
    let chunks = Layout::horizontal([Constraint::Min(1), Constraint::Length(30)]).split(area);
    let grid_area = chunks[0];
    let right = Layout::vertical([
        Constraint::Min(6),
        Constraint::Length(8),
        Constraint::Min(6),
    ])
    .split(chunks[1]);

    // --- Grid geometry from the shared WorldMapLayout positions carried by
    // MapFrame. MapBounds/MapCamera do the fitting and projection; rendering
    // is O(cells) with cols*rows ≈ O(facilities). ---
    let bounds = frame_bounds(mf);
    let cols = ((grid_area.width as usize).saturating_sub(2)).clamp(8, 40);
    let rows = ((grid_area.height as usize).saturating_sub(2)).clamp(8, 20);
    let camera = grid_camera(bounds, cols, rows);
    let last_c = (cols - 1) as isize;
    let last_r = (rows - 1) as isize;
    let cell_for = |x: f32, y: f32| -> (usize, usize) {
        let (px, py) = camera.project((x, y));
        (
            (px.round() as isize).clamp(0, last_c) as usize,
            (py.round() as isize).clamp(0, last_r) as usize,
        )
    };

    // --- Grid base: facility glyphs (O(facilities)). ---
    let mut grid = vec!['.'; cols * rows];
    // --- Nav edges first (O(edges)); facilities and pawns overwrite them, so
    // edges always render beneath pawns and only visible when unobstructed. ---
    for edge in &mf.edges {
        let Some(&from) = mf.facility_positions.get(&edge.from) else {
            continue;
        };
        let Some(&to) = mf.facility_positions.get(&edge.to) else {
            continue;
        };
        let glyph: char = if edge.restricted { 'x' } else { '·' };
        for (x, y) in [from, to] {
            let (c, r) = cell_for(x, y);
            grid[r * cols + c] = glyph;
        }
    }
    for (id, (x, y)) in &mf.facility_positions {
        let (c, r) = cell_for(*x, *y);
        grid[r * cols + c] = facility_glyph(*id, &data.facilities) as char;
    }

    // --- Pawns on top of facilities (guards first so prisoners overwrite),
    // recording per-node occupancy (O(pawns)). ---
    let mut pawns: Vec<&sumo::PawnPosition> = mf.prisoners.iter().chain(mf.guards.iter()).collect();
    pawns.sort_by_key(|p| p.is_prisoner);
    let mut occupancy: HashMap<u64, usize> = HashMap::new();
    for p in &pawns {
        *occupancy.entry(p.node_id).or_insert(0) += 1;
        let (c, r) = cell_for(p.x, p.y);
        grid[r * cols + c] = if p.is_prisoner { 'P' } else { 'G' };
    }

    // --- Incident overlays where available (O(incidents)). ---
    let open_incidents: Vec<&sumo::Incident> =
        mf.incidents.iter().filter(|i| !i.resolved).collect();
    for inc in &open_incidents {
        let plot_node = inc.location.trim().parse::<u64>().ok().or_else(|| {
            inc.involved_prisoners.iter().find_map(|pid| {
                mf.prisoners
                    .iter()
                    .find(|p| p.id == pid.raw())
                    .map(|p| p.node_id)
            })
        });
        if let Some(id) = plot_node {
            if let Some((x, y)) = mf.facility_positions.get(&id) {
                let (c, r) = cell_for(*x, *y);
                grid[r * cols + c] = '!';
            }
        }
    }

    // --- Locked nodes (e.g. full lockdown). ---
    for &id in &mf.locked_nodes {
        if let Some((x, y)) = mf.facility_positions.get(&id) {
            let (c, r) = cell_for(*x, *y);
            grid[r * cols + c] = 'L';
        }
    }

    // --- Draw one preallocated row String per grid row. ---
    let mut grid_lines: Vec<Line<'static>> = Vec::with_capacity(rows);
    for r in 0..rows {
        grid_lines.push(Line::from(
            grid[r * cols..(r + 1) * cols].iter().collect::<String>(),
        ));
    }
    frame.render_widget(
        Paragraph::new(grid_lines).block(title_block(" Spatial Map (MapFrame) ")),
        grid_area,
    );

    // --- Right panel: selected pawn detail, incidents, legend. ---
    let sel_pawn = pawns.get(ui.scroll as usize).copied();
    let mut detail: Vec<Line<'static>> = Vec::new();
    let mut sel_cell = None;
    if let Some(p) = sel_pawn {
        sel_cell = Some(cell_for(p.x, p.y));
        detail.push(kv("Pawn", format!("{} #{}", p.name, p.id)));
        detail.push(kv("Node", format!("W{}", p.node_id)));
        detail.push(kv("Activity", truncate(&p.activity, 40)));
        if let Some(intent) = &p.intention {
            detail.push(kv("Intention", truncate(intent, 30)));
        }
        if let Some(threat) = p.threat {
            detail.push(kv("Threat", format!("{threat:.2}")));
        }
        if let Some(mood) = p.mood {
            detail.push(kv("Mood", format!("{mood:.2}")));
        }
        if p.is_prisoner {
            detail.push(kv(
                "Gang",
                p.gang
                    .map(|g| format!("#{g}"))
                    .unwrap_or_else(|| "none".into()),
            ));
            if let Some(class) = &p.classification {
                detail.push(kv("Class", class.clone()));
            }
        } else if let Some(post) = &p.post {
            detail.push(kv("Post", post.clone()));
        }
    } else {
        detail.push(plain("No pawns on the map."));
    }
    // Selected facility/cell at the cursor.
    if let Some(cell) = sel_cell {
        let node = mf
            .facility_positions
            .iter()
            .find(|(_, (fx, fy))| cell_for(*fx, *fy) == cell)
            .map(|(id, _)| *id);
        if let Some(id) = node {
            if let Some(f) = data.facilities.iter().find(|f| f.id == id) {
                let occ = occupancy.get(&id).copied().unwrap_or(0);
                let util = if f.capacity == 0 {
                    0.0
                } else {
                    occ as f32 / f.capacity as f32 * 100.0
                };
                detail.push(plain("--- Facility ---"));
                detail.push(kv("Kind", f.kind.display().to_string()));
                detail.push(kv("Capacity", f.capacity.to_string()));
                detail.push(kv(
                    "Occupancy",
                    format!("{occ}/{} ({util:.0}% util)", f.capacity),
                ));
                detail.push(kv("Condition", format!("{:.0}%", f.condition * 100.0)));
            }
        }
    }
    detail.push(kv(
        "Overall occupancy",
        format!(
            "{}/{} ({:.0}% util)",
            data.observation.population,
            data.observation.capacity,
            if data.observation.capacity == 0 {
                0.0
            } else {
                data.observation.population as f32 / data.observation.capacity as f32 * 100.0
            }
        ),
    ));
    frame.render_widget(
        Paragraph::new(detail).block(title_block(" Pawn Detail ")),
        right[0],
    );

    // Incident overlays list.
    let mut inc_lines: Vec<Line<'static>> = vec![plain(format!(
        "Incident overlays: {} open",
        open_incidents.len()
    ))];
    for inc in open_incidents.iter().take(6) {
        inc_lines.push(Line::from(vec![
            Span::styled("  ! ", Style::default().fg(to_tui_color(palette().danger))),
            Span::raw(format!(
                "#{} {:?} @ {} sev {:.0}%",
                inc.id,
                inc.kind,
                truncate(
                    if inc.location.is_empty() {
                        "unknown"
                    } else {
                        &inc.location
                    },
                    20
                ),
                inc.severity * 100.0
            )),
        ]));
    }
    frame.render_widget(
        Paragraph::new(inc_lines).block(title_block(" Incident Overlays ")),
        right[1],
    );

    // Legend.
    let sight = mf.edges.iter().filter(|e| e.sightline).count();
    let restricted = mf.edges.iter().filter(|e| e.restricted).count();
    frame.render_widget(
        Paragraph::new(vec![
            plain("Legend:"),
            plain("  . empty  P prisoner  G guard  ! open incident  L locked"),
            plain("  C Cell Block · D Dorm · M Medical · K Kitchen · Y Yard · A Admin · E Education · V Visitor · I Isolation · S Staff · W Workshop · R Recreation"),
            plain(format!(
                "  Edges: {} total · {sight} sightline · {restricted} restricted",
                mf.edges.len()
            )),
        ])
        .block(title_block(" Legend ")),
        right[2],
    );
}

// ---------------------------------------------------------------------------
// 10 — Incident reports
// ---------------------------------------------------------------------------

fn render_incidents(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(4),
        Constraint::Length(12),
    ])
    .split(area);
    // The driver already supplies newest-first records (see App::visible_incidents).
    let records: Vec<_> = data.incident_records.iter().collect();
    let total = records.len();
    let idx = if total == 0 {
        0
    } else {
        (ui.scroll as usize).min(total - 1)
    };

    // Severity / status counts.
    let critical = records.iter().filter(|r| r.severity >= 0.75).count();
    let high = records
        .iter()
        .filter(|r| r.severity >= 0.4 && r.severity < 0.75)
        .count();
    let low = records.iter().filter(|r| r.severity < 0.4).count();
    let open = records.iter().filter(|r| !r.resolved).count();
    let resolved = records.iter().filter(|r| r.resolved).count();

    let names = name_map(&data.all_prisoners);
    let summary = vec![
        kv(
            "Incidents",
            format!("{} total · {} open · {} resolved", total, open, resolved),
        ),
        kv(
            "Severity",
            format!("critical {} · high {} · low {}", critical, high, low),
        ),
        kv(
            "Contraband (0.05)",
            format!(
                "{} cases · {} inventory kinds",
                data.contraband_cases.len(),
                data.contraband_inventory.len()
            ),
        ),
        if records.is_empty() {
            plain(format!(
                "No structured incidents yet. Incident log has {} events.",
                data.incident_log.len()
            ))
        } else {
            plain(format!(
                "Showing {} newest-first (page {} of {}). Incident log (raw events): {}.",
                total,
                idx + 1,
                total,
                data.incident_log.len()
            ))
        },
    ];
    frame.render_widget(
        Paragraph::new(summary).block(title_block(" Incident Summary & Filters ")),
        chunks[0],
    );

    // Pagination window around the selected cursor.
    let page_size = (chunks[1].height.saturating_sub(3) as usize).max(1);
    let page_start = if total == 0 {
        0
    } else {
        (idx / page_size) * page_size
    };
    let page_end = (page_start + page_size).min(total);

    let items: Vec<ListItem<'static>> = records[page_start..page_end]
        .iter()
        .enumerate()
        .map(|(j, r)| {
            let i = page_start + j;
            let marker = if i == idx { "»" } else { " " };
            let style = if i == idx {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let sev_color = to_tui_color(palette().severity(r.severity));
            let status = if r.resolved { "RES" } else { "OPEN" };
            let actor_count = r.involved_prisoners.len();
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} "), Style::default().fg(Color::Yellow)),
                Span::styled(format!("[{}] ", r.at), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:<14}", truncate(&r.kind, 14)),
                    Style::default().fg(to_tui_color(palette().danger)),
                ),
                Span::styled(
                    format!("sev {:.0}% ", r.severity * 100.0),
                    Style::default().fg(sev_color),
                ),
                Span::styled(
                    format!("{status:<4} "),
                    Style::default().fg(if r.resolved {
                        to_tui_color(palette().good)
                    } else {
                        Color::Magenta
                    }),
                ),
                Span::styled(
                    format!("actors:{} ", actor_count),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(truncate(&r.location, 30)),
            ]))
            .style(style)
        })
        .collect();
    frame.render_widget(
        List::new(items).block(title_block(format!(
            " Incident Reports — {}..{} of {} (newest first) ",
            page_start + 1,
            page_end,
            total
        ))),
        chunks[1],
    );

    // The detail follows the visible cursor; the cached id only confirms that
    // the cursor row is the one the observer selected.
    let cursor_record = records.get(idx).copied();
    let selected_record = match (ui.selected_incident, cursor_record) {
        (Some(id), Some(r)) if r.id == id => Some(r),
        (_, r) => r,
    };
    let mut detail = if let Some(r) = selected_record {
        let participants = if r.involved_prisoners.is_empty() {
            "none".to_string()
        } else {
            r.involved_prisoners
                .iter()
                .map(|id| names.get(id).cloned().unwrap_or_else(|| format!("P{}", id)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        // Causal chain: walk cause backwards, then forward through consequence.
        let mut chain: Vec<String> = Vec::new();
        let mut cur = r.cause;
        let mut seen = std::collections::HashSet::new();
        while let Some(cid) = cur {
            if !seen.insert(cid) {
                break;
            }
            if let Some(c) = data.all_incident_records.iter().find(|x| x.id == cid) {
                chain.push(format!(
                    "#{} {} sev {:.0}%",
                    c.id,
                    truncate(&c.kind, 14),
                    c.severity * 100.0
                ));
                cur = c.cause;
            } else {
                chain.push(format!("#{} (missing)", cid));
                break;
            }
            if chain.len() > 8 {
                break;
            }
        }
        chain.reverse();
        chain.push(format!("[#{} {} CURRENT]", r.id, truncate(&r.kind, 14)));
        let mut fwd = r.consequence;
        let mut seen_f = std::collections::HashSet::new();
        while let Some(cid) = fwd {
            if !seen_f.insert(cid) {
                break;
            }
            if let Some(c) = data.all_incident_records.iter().find(|x| x.id == cid) {
                chain.push(format!(
                    "#{} {} sev {:.0}%",
                    c.id,
                    truncate(&c.kind, 14),
                    c.severity * 100.0
                ));
                fwd = c.consequence;
            } else {
                chain.push(format!("#{} (missing)", cid));
                break;
            }
            if chain.len() > 16 {
                break;
            }
        }
        let outcome = if r.resolved {
            "resolved"
        } else {
            "open / unresolved"
        };
        let witnesses = if r.witnesses.is_empty() {
            "none".to_string()
        } else {
            r.witnesses
                .iter()
                .map(|id| names.get(id).cloned().unwrap_or_else(|| format!("P{id}")))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let cause = r
            .causal_chain
            .first()
            .map(|link| {
                format!(
                    "{} #{}: {}",
                    if link.is_incident {
                        "incident"
                    } else {
                        "event"
                    },
                    link.source_id,
                    truncate(&link.description, 40)
                )
            })
            .or_else(|| r.cause.map(|id| format!("incident #{id}")))
            .unwrap_or_else(|| "none recorded".to_string());
        let responses_ok = r.response_actions.iter().filter(|a| a.success).count();
        let responses_failed = r.response_actions.len() - responses_ok;
        let validation = if r.response_actions.is_empty() {
            "no staff response recorded".to_string()
        } else {
            format!(
                "{} response(s): {} ok / {} failed",
                r.response_actions.len(),
                responses_ok,
                responses_failed
            )
        };
        let mut d = vec![
            kv("ID / kind", format!("#{} {}", r.id, truncate(&r.kind, 24))),
            kv(
                "At / severity",
                format!("{} · {:.0}%", r.at, r.severity * 100.0),
            ),
            kv(
                "Location",
                if r.location.is_empty() {
                    "unknown".into()
                } else {
                    r.location.clone()
                },
            ),
            kv("Cause", cause),
            kv("Actors", participants),
            kv("Witnesses", witnesses),
            kv("Validation", validation),
            kv("Outcome", format!("{} · {}", outcome, r.disposition)),
        ];
        d.push(plain("--- Causal chain ---"));
        for link in chain {
            d.push(plain(format!("  {link}")));
        }
        if !r.causal_chain.is_empty() {
            d.push(plain("--- Causal links (structured) ---"));
            for link in r.causal_chain.iter().take(8) {
                d.push(plain(format!(
                    "  {} #{}: {}",
                    if link.is_incident {
                        "incident"
                    } else {
                        "event"
                    },
                    link.source_id,
                    truncate(&link.description, 60)
                )));
            }
        }
        d.push(plain("--- Response actions ---"));
        if r.response_actions.is_empty() {
            d.push(plain("  none recorded"));
        } else {
            for action in r.response_actions.iter().take(8) {
                d.push(plain(format!(
                    "  {:?} by {:?} at {} — {} ({})",
                    action.action,
                    action.responder,
                    action.at,
                    truncate(&action.description, 48),
                    if action.success { "ok" } else { "failed" }
                )));
            }
        }
        // Per-prisoner incident history filtering: if a prisoner selected, show their history; else show for participants.
        let history_ids: Vec<u64> = if let Some(pid) = ui.selected_prisoner {
            vec![pid]
        } else {
            r.involved_prisoners.clone()
        };
        for pid in history_ids {
            let pname = names
                .get(&pid)
                .cloned()
                .unwrap_or_else(|| format!("P{}", pid));
            let history: Vec<String> = data
                .all_incident_records
                .iter()
                .filter(|rr| rr.id != r.id && rr.involved_prisoners.contains(&pid))
                .rev()
                .take(5)
                .map(|rr| {
                    format!(
                        "#{} {} sev {:.0}% {}",
                        rr.id,
                        truncate(&rr.kind, 12),
                        rr.severity * 100.0,
                        if rr.resolved { "RES" } else { "OPEN" }
                    )
                })
                .collect();
            d.push(plain(format!("--- History for {} (P{}) ---", pname, pid)));
            if history.is_empty() {
                d.push(plain("  no prior incidents"));
            } else {
                for h in history {
                    d.push(plain(format!("  {h}")));
                }
            }
        }
        d
    } else {
        vec![plain("No incidents recorded.")]
    };
    let filter = ui.filter();
    let sev_filter = match filter.min_severity {
        None => "all".to_string(),
        Some(s) if s >= 0.74 => "critical+".to_string(),
        Some(_) => "high+".to_string(),
    };
    let res_filter = match filter.resolved {
        None => "all",
        Some(false) => "open",
        Some(true) => "resolved",
    };
    detail.push(plain(format!(
        "--- Contraband Cases (0.05) sev:{sev_filter} status:{res_filter} ---"
    )));
    detail.push(plain(
        "Keys: f next case · F resolve selected · m severity · o open/resolved",
    ));
    if data.contraband_cases.is_empty() {
        detail.push(plain("No contraband cases recorded."));
    } else {
        if let Some(sel) = ui.selected_contraband {
            if let Some(c) = data.contraband_cases.iter().find(|c| c.case_id == sel) {
                let holder = c
                    .holder
                    .map(|h| {
                        names
                            .get(&h.raw())
                            .cloned()
                            .unwrap_or_else(|| format!("P{}", h.raw()))
                    })
                    .unwrap_or_else(|| "unknown".into());
                detail.push(Line::from(vec![
                    Span::styled("Selected: ", Style::default().fg(Color::Cyan)),
                    Span::raw(format!(
                        "#{} {} holder {} ",
                        c.case_id,
                        c.item.as_deref().unwrap_or("Unknown"),
                        holder
                    )),
                    Span::styled(
                        if c.resolved { "RESOLVED" } else { "OPEN" },
                        Style::default()
                            .fg(state_color(c.resolved))
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
        }
        for c in data.contraband_cases.iter().rev().take(5) {
            let marker = if Some(c.case_id) == ui.selected_contraband {
                "»"
            } else {
                " "
            };
            let holder = c
                .holder
                .map(|h| {
                    names
                        .get(&h.raw())
                        .cloned()
                        .unwrap_or_else(|| format!("P{}", h.raw()))
                })
                .unwrap_or_else(|| "unknown".into());
            detail.push(plain(format!(
                "{marker}#{} d{} {} holder {} sev {:.0}%{}",
                c.case_id,
                c.at.as_days(),
                c.item.as_deref().unwrap_or("Unknown"),
                holder,
                c.severity * 100.0,
                if c.resolved { " RES" } else { " OPEN" }
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(detail).block(title_block(" Incident Detail & Outcome ")),
        chunks[2],
    );
}

// ---------------------------------------------------------------------------
// 11 — Decision audit
// ---------------------------------------------------------------------------

fn render_decisions(frame: &mut Frame, area: Rect, data: &ViewData, ui: &UiState) {
    let chunks = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(4),
        Constraint::Length(10),
    ])
    .split(area);
    // The driver already supplies newest-first records (see App::visible_decisions).
    let entries: Vec<_> = data.decision_audit.iter().collect();
    let idx = (ui.scroll as usize).min(entries.len().saturating_sub(1));

    let accepted = entries
        .iter()
        .filter(|d| d.validation.starts_with("accepted"))
        .count();
    let repaired = entries
        .iter()
        .filter(|d| d.validation.starts_with("repaired"))
        .count();
    let rejected = entries
        .iter()
        .filter(|d| d.validation.starts_with("rejected"))
        .count();
    let other = entries.len().saturating_sub(accepted + repaired + rejected);

    let df = ui.decision_validation_filter.as_deref().unwrap_or("all");
    let summary = vec![
        kv(
            "Decisions",
            format!(
                "{} total · accepted {} · repaired {} · rejected {} · other {}",
                entries.len(),
                accepted,
                repaired,
                rejected,
                other
            ),
        ),
        kv(
            "Filter",
            format!("validation={df} (d to cycle) · green=accepted yellow=repaired red=rejected"),
        ),
        plain("Validation/reason/source shown in detail. Entries newest-first."),
        if entries.is_empty() {
            plain("No decisions recorded. Pending data contract: decision_audit empty.")
        } else {
            plain(format!(
                "Distinct actors: {}.",
                entries
                    .iter()
                    .map(|d| d.actor.clone())
                    .collect::<std::collections::HashSet<_>>()
                    .len()
            ))
        },
        kv(
            "AI runtime",
            format!(
                "{} / {} · tok {}/{} · cache {}/{} · retry {} · limited {}",
                data.ai_runtime.provider,
                data.ai_runtime.model,
                data.ai_runtime.tokens_consumed_today,
                data.ai_runtime.max_tokens_per_day,
                data.ai_runtime.cache_hits,
                data.ai_runtime.cache_entries,
                data.ai_runtime.retries,
                data.ai_runtime.rate_limited
            ),
        ),
    ];
    frame.render_widget(
        Paragraph::new(summary).block(title_block(" Decision Audit Summary & Filters ")),
        chunks[0],
    );

    let items: Vec<ListItem<'static>> = entries
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let marker = if i == idx { "»" } else { " " };
            let style = if i == idx {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let vcolor = to_tui_color(palette().validation(&d.validation));
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} "), Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("[{}] ", d.time),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<12}", truncate(&d.actor, 12)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(format!("{:<10}", d.validation), Style::default().fg(vcolor)),
                Span::styled(
                    format!("{:<8}", truncate(&d.source, 8)),
                    Style::default().fg(Color::Magenta),
                ),
                Span::raw(truncate(&d.summary, 55)),
            ]))
            .style(style)
        })
        .collect();
    frame.render_widget(
        List::new(items).block(title_block(format!(
            " Decision Audit — {} entries (newest first) ",
            entries.len()
        ))),
        chunks[1],
    );

    // The unique content key identifies the row under the cursor. If the key
    // has gone stale after trimming/filtering, the visible cursor wins.
    let cursor_record = entries.get(idx).copied();
    let selected = match (&ui.selected_decision, cursor_record) {
        (Some(key), Some(d)) if crate::decision_key(d) == *key => Some(d),
        (_, d) => d,
    };
    let detail = if let Some(d) = selected {
        let outcome = format!("{:?}", d.outcome);
        let proposed = if d.action.is_empty() {
            d.summary.clone()
        } else {
            d.action.clone()
        };
        let provenance = if d.source.is_empty() {
            "unknown".to_string()
        } else {
            d.source.clone()
        };
        let cause = if d.reason.is_empty() {
            if d.rationale.is_empty() {
                "none recorded".to_string()
            } else {
                truncate(&d.rationale, 120)
            }
        } else {
            truncate(&d.reason, 120)
        };
        vec![
            kv("Time", d.time.to_string()),
            kv("Actors", format!("{} · {}", d.actor, provenance)),
            kv("Cause", cause),
            kv("Proposed action", truncate(&proposed, 80)),
            kv("Validation", d.validation.clone()),
            kv("Outcome", outcome),
            kv(
                "Rationale",
                if d.rationale.is_empty() {
                    "none recorded".into()
                } else {
                    truncate(&d.rationale, 120)
                },
            ),
            kv(
                "Repair reason",
                if d.repair_reason.is_empty() {
                    "none".into()
                } else {
                    truncate(&d.repair_reason, 120)
                },
            ),
            kv("Summary", truncate(&d.summary, 120)),
            plain("↑/↓ scroll list; detail follows the cursor."),
        ]
    } else {
        vec![plain(
            "No decisions recorded. Pending data contract: decision_audit empty.",
        )]
    };
    frame.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: false })
            .block(title_block(" Decision Detail ")),
        chunks[2],
    );
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
            save_dir: "saves/default".to_string(),
            autosave_days: 0,
            open_critical: 0,
        }
    }

    fn view_data_from_world(world: &alc_world::World) -> ViewData {
        let observation = sumo::Observation::capture(world);
        let prisoners: Vec<PrisonerSummary> = world
            .prisoners
            .iter()
            .map(|p| PrisonerSummary {
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
                bio: p.bio.clone(),
                crime: p.crime.clone(),
                crime_category: p.crime_category,
            })
            .collect();
        let pressure = world.gang_pressure();
        let gangs = world
            .gangs
            .iter()
            .map(|g| {
                let relation_for = |other: &alc_social::Gang| {
                    alc_social::gang_relation_scores(g, other, &world.social)
                };
                let mut rivalries: Vec<(u64, f32)> = g
                    .rivalries
                    .iter()
                    .filter_map(|id| {
                        world
                            .gangs
                            .iter()
                            .find(|other| other.id == *id)
                            .map(|other| (id.0, relation_for(other).1.max(0.25)))
                    })
                    .collect();
                rivalries.sort_by_key(|(id, _)| *id);
                let mut alliances: Vec<(u64, f32)> = g
                    .alliances
                    .iter()
                    .filter_map(|id| {
                        world
                            .gangs
                            .iter()
                            .find(|other| other.id == *id)
                            .map(|other| (id.0, relation_for(other).0.max(0.25)))
                    })
                    .collect();
                alliances.sort_by_key(|(id, _)| *id);
                let relations = world
                    .gangs
                    .iter()
                    .filter(|other| other.id != g.id)
                    .map(|other| {
                        let (affinity, hostility) = relation_for(other);
                        sumo::GangRelationSummary {
                            gang: other.id.0,
                            affinity,
                            hostility,
                        }
                    })
                    .collect();
                let history = world
                    .gang_history()
                    .iter()
                    .filter(|entry| entry.gang == g.id)
                    .cloned()
                    .collect();
                GangSummary {
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
                    rivalries,
                    alliances,
                    internal_disputes: g.internal_disputes.clone(),
                    goals: g.goals.clone(),
                    pressure,
                    history,
                    relations,
                }
            })
            .collect();
        let guards = world
            .guards
            .iter()
            .map(|g| GuardSummary {
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
                credibility: g.credibility(),
                mistakes: g.mistake_count(),
                violations: g.violation_count(),
                tracked_opinions: g.opinion_count(),
                needs: g.needs,
                off_duty: g.off_duty,
                off_duty_activity: g.off_duty_activity,
                performance: g.performance_score(),
                bonds: g.bonds.clone(),
                history: g.history.clone(),
            })
            .collect();
        let warden = WardenSummary {
            name: world.warden.name.clone(),
            philosophy: world.warden.philosophy.clone(),
            traits: world.warden.traits.clone(),
            attention: world.warden.attention,
            inbox: Vec::new(),
            pending_actions: world.warden.pending_actions.clone(),
            last_decision_at: world.warden.last_decision_at,
            control: world.warden.control_panel_summary(world.economy.health()),
            recent_decisions: world
                .warden
                .audit_log
                .recent(20)
                .into_iter()
                .cloned()
                .collect(),
        };
        let economy = EconomySummary {
            cash_major: world.economy.cash.as_major(),
            health: world.economy.health(),
            funding_per_prisoner_per_day: world.economy.funding_per_prisoner_per_day.as_major(),
            crowding_threshold: world.economy.crowding_threshold,
            ledger: world.economy.ledger.clone(),
        };
        let mut social_edges = Vec::new();
        for ((a, b), edge) in world.social.iter() {
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
        let facilities = world
            .facilities
            .iter()
            .map(|f| FacilitySummary {
                id: f.id.0,
                kind: f.kind,
                wing: 0,
                capacity: f.capacity,
                condition: f.condition,
                staff_required: 0,
            })
            .collect();
        // The overview only displays a bounded tail; use the capped EventBus
        // API instead of materialising the whole bounded history in tests.
        let mut events: Vec<WorldEvent> = world.events.last_reversed(OVERVIEW_EVENT_LIMIT);
        events.reverse();
        ViewData {
            observation,
            map_frame: MapFrame::capture(world),
            total_prisoners: world.prisoners.len(),
            total_gangs: world.gangs.len(),
            total_guards: world.guards.len(),
            prisoners: prisoners.clone(),
            gangs,
            guards,
            warden,
            ai_runtime: sumo::AiRuntimeSnapshot::default(),
            economy,
            social_edges,
            social_roles: Vec::new(),
            facilities,
            construction_queue: Vec::new(),
            events: events.clone(),
            recent_events: events,
            incident_log: Vec::new(),
            incident_records: Vec::new(),
            all_prisoners: prisoners,
            all_incident_records: Vec::new(),
            decision_audit: Vec::new(),
            policy: sumo::PolicySummary::default(),
            post_release_records: Vec::new(),
            rehab_summaries: Vec::new(),
            rehab_program_catalog: Vec::new(),
            rehab_overview: sumo::RehabOverview::default(),
            contraband_cases: Vec::new(),
            contraband_inventory: Vec::new(),
            budget_snapshot: sumo::BudgetSnapshot::default(),
            selected_schedule: None,
            parole_due: Vec::new(),
            selected_parole: None,
            selected_parole_conditions: None,
            selected_parole_reason: None,
            selected_parole_released_at: None,
        }
    }

    fn render_all(world: &alc_world::World, width: u16, height: u16, scroll: u16) {
        let data = view_data_from_world(world);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        for (i, mode) in ViewMode::all().iter().enumerate() {
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
        for (i, mode) in ViewMode::all().iter().enumerate() {
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
            let ui = UiState {
                mode,
                scroll: u16::MAX,
                ..UiState::new()
            };
            terminal
                .draw(|frame| draw(frame, &data, &ui, &status(false)))
                .unwrap();
        }
    }

    #[test]
    fn social_network_renders_detected_role_rows() {
        let world = alc_world::World::new(11);
        let mut data = view_data_from_world(&world);
        let prisoner = data.prisoners[0].id;
        data.social_roles = vec![SocialRoleSummary {
            prisoner,
            role: "Broker".to_string(),
            strength: 0.42,
        }];
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let ui = UiState {
            mode: ViewMode::SocialNetwork,
            ..UiState::new()
        };
        terminal
            .draw(|frame| draw(frame, &data, &ui, &status(false)))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                rendered.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(rendered.contains("Broker"));
    }

    #[test]
    fn gang_view_renders_pressure_scores_and_history() {
        use alc_core::{GangId, SimTime};
        use alc_social::{Gang as SocialGang, GangHistoryKind};
        use alc_world::GangHistoryEntry;

        let mut world = alc_world::World::new(21);
        let leader = world.prisoners[0].id;
        let member = world.prisoners[1].id;
        let gid = GangId(7);
        let mut gang = SocialGang::new(gid, "Iron Choir", leader);
        gang.recruit(member);
        gang.goals = vec!["control the yard".into()];
        world.gangs.push(gang);
        world.gang_history.push(GangHistoryEntry {
            id: 0,
            gang: gid,
            kind: GangHistoryKind::LeadershipChange,
            at: SimTime::from_secs(world.time.as_secs()),
            summary: "Iron Choir leadership changed".into(),
            actors: vec![member.raw()],
            causes: vec!["gang:7".into()],
        });
        world.next_gang_history_id = 1;

        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let ui = UiState {
            mode: ViewMode::Gang,
            ..UiState::new()
        };
        terminal
            .draw(|frame| draw(frame, &data, &ui, &status(false)))
            .unwrap();

        let buf = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                rendered.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(rendered.contains("Pressure"), "{rendered}");
        assert!(rendered.contains("Scores"), "{rendered}");
        assert!(rendered.contains("Gang History"), "{rendered}");
        assert!(rendered.contains("leadership"), "{rendered}");
    }

    #[test]
    fn mode_index_mapping_follows_shared_catalog() {
        let modes = ViewMode::all();
        for (i, mode) in modes.iter().enumerate() {
            assert_eq!(mode_from_index(i), Some(*mode));
            assert_eq!(mode.label(), ViewMode::all()[i].label());
            assert_eq!(mode.title(), ViewMode::all()[i].title());
        }
        assert_eq!(mode_from_index(modes.len()), None);
        assert_eq!(mode_from_index(usize::MAX), None);
    }

    #[test]
    fn wide_tabs_use_shared_titles() {
        let world = alc_world::World::new(5);
        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(240, 24)).unwrap();
        let ui = UiState {
            mode: ViewMode::Map,
            ..UiState::new()
        };
        terminal
            .draw(|frame| draw(frame, &data, &ui, &status(false)))
            .unwrap();
        let buf = terminal.backend().buffer();
        let tab_row: String = (0..buf.area.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect();
        for mode in ViewMode::all() {
            assert!(tab_row.contains(mode.title()), "missing {}", mode.title());
        }
    }

    #[test]
    fn shared_palette_converts_to_ratatui_colors() {
        let p = palette();
        assert_eq!(
            to_tui_color(SharedColor::rgba(12, 34, 56, 78)),
            Color::Rgb(12, 34, 56),
            "ratatui cells have no alpha channel"
        );
        assert_eq!(urgency_color(0.9), to_tui_color(p.danger));
        assert_eq!(urgency_color(0.5), to_tui_color(p.warning));
        assert_eq!(urgency_color(0.1), to_tui_color(p.good));
        assert_eq!(good_color(0.1), to_tui_color(p.danger));
        assert_eq!(good_color(0.9), to_tui_color(p.good));
        assert_eq!(to_tui_color(p.validation("accepted")), to_tui_color(p.good));
        assert_eq!(
            to_tui_color(p.validation("rejected")),
            to_tui_color(p.danger)
        );
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
    fn observer_feed_overlay_renders_notices_and_pins() {
        let world = alc_world::World::new(3);
        let data = view_data_from_world(&world);
        let mut ui = UiState::new();
        ui.show_notifications = true;
        ui.notifications.push("Saved to saves".to_string());
        ui.bookmarks.push(42);
        ui.pause_on_critical = true;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| draw(frame, &data, &ui, &status(false)))
            .unwrap();
        assert!(ui.show_notifications);
    }

    #[test]
    fn control_hints_are_visible_on_narrow_terminals() {
        let world = alc_world::World::new(3);
        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| draw(frame, &data, &UiState::new(), &status(false)))
            .unwrap();
        let buf = terminal.backend().buffer();
        let hint_row: String = (0..buf.area.width)
            .map(|x| buf[(x, 1)].symbol().to_string())
            .collect();
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
            let ui = UiState {
                mode: ViewMode::Financial,
                scroll,
                ..UiState::new()
            };
            terminal
                .draw(|frame| draw(frame, &data, &ui, &status(false)))
                .unwrap();
            let buf = terminal.backend().buffer();
            let mut rendered = String::new();
            for y in 0..buf.area.height {
                for x in 0..buf.area.width {
                    rendered.push_str(buf[(x, y)].symbol());
                }
            }
            assert!(
                rendered.contains("ENTRY_"),
                "ledger rows missing at cursor {scroll}"
            );
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

    #[test]
    fn fit_spans_truncates_to_measured_width() {
        let spans = vec![Span::raw("abcdefgh"), Span::raw("xyz")];
        let fitted = fit_spans(spans, 4);
        assert_eq!(fitted.iter().map(Span::width).sum::<usize>(), 4);
        let one = fit_spans(vec![Span::raw("abcdefgh")], 3);
        assert_eq!(one.iter().map(Span::width).sum::<usize>(), 3);
        assert_eq!(
            fit_spans(vec![Span::raw("")], 5)
                .iter()
                .map(Span::width)
                .sum::<usize>(),
            0
        );
    }

    #[test]
    fn map_view_renders_grid_with_pawns_and_legend() {
        let world = alc_world::World::new(7);
        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let ui = UiState {
            mode: ViewMode::Map,
            ..UiState::new()
        };
        terminal
            .draw(|frame| draw(frame, &data, &ui, &status(false)))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                rendered.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(rendered.contains("Spatial Map"));
        assert!(rendered.contains("Legend"));
        assert!(rendered.contains("Pawn Detail"));
    }

    #[test]
    fn map_view_renders_nav_edges_and_legend_counts() {
        let world = alc_world::World::new(7);
        assert!(
            world.facilities.len() >= 2,
            "expected bootstrapped facilities"
        );
        let data = view_data_from_world(&world);
        assert!(
            !data.map_frame.edges.is_empty(),
            "expected placeholder nav edges from MapFrame"
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let ui = UiState {
            mode: ViewMode::Map,
            ..UiState::new()
        };
        terminal
            .draw(|frame| draw(frame, &data, &ui, &status(false)))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                rendered.push_str(buf[(x, y)].symbol());
            }
        }
        // Nav edges are visible on the grid as '·' (sightline) / 'x' (restricted).
        assert!(
            rendered.contains('·'),
            "no sightline edge glyph on map grid"
        );
        assert!(rendered.contains("Edges:"), "legend missing edge counts");
    }

    #[test]
    fn financial_panels_show_trend_forecast_drivers_and_warnings() {
        let mut world = alc_world::World::new(13);
        world.economy.ledger.clear();
        for i in 0..30u32 {
            world.economy.ledger.push(alc_economy::LedgerEntry {
                at_secs: u64::from(i) * 86_400,
                account: alc_economy::Account::PerCapitaFunding,
                amount: alc_economy::Money::from_major(i64::from(i) + 1),
                note: format!("E{i}"),
            });
        }
        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(140, 45)).unwrap();
        let ui = UiState {
            mode: ViewMode::Financial,
            ..UiState::new()
        };
        terminal
            .draw(|frame| draw(frame, &data, &ui, &status(false)))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                rendered.push_str(buf[(x, y)].symbol());
            }
        }
        // Previously-clipped sections are present now.
        for needle in [
            "Trend",
            "Forecast cash",
            "Health drivers",
            "Warnings",
            "Daily net / forecast",
            "D29",
        ] {
            assert!(
                rendered.contains(needle),
                "financial panel missing {needle}"
            );
        }
    }

    fn render_text(data: &ViewData, ui: &UiState, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, data, ui, &status(false)))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                rendered.push_str(buf[(x, y)].symbol());
            }
        }
        rendered
    }

    #[test]
    fn incident_detail_shows_cause_actors_validation_and_outcome() {
        let world = alc_world::World::new(11);
        let mut data = view_data_from_world(&world);
        data.all_incident_records = vec![sumo::IncidentRecord {
            id: 11,
            kind: "Argument".to_string(),
            severity: 0.6,
            resolved: true,
            ..Default::default()
        }];
        data.incident_records = vec![sumo::IncidentRecord {
            id: 12,
            at: alc_core::SimTime { secs: 60 },
            kind: "Fight".to_string(),
            severity: 0.9,
            location: "Yard".to_string(),
            involved_prisoners: vec![7],
            cause: Some(11),
            consequence: None,
            causal_chain: vec![sumo::incidents::CausalLink::incident(11, "prior argument")],
            witnesses: vec![7],
            response_actions: vec![sumo::incidents::ResponseAction {
                action: sumo::incidents::ResponseActionKind::GuardIntervention,
                responder: sumo::incidents::Responder::Guard(3),
                at: alc_core::SimTime { secs: 61 },
                description: "separated".to_string(),
                success: true,
            }],
            disposition: "Disciplinary".to_string(),
            resolved: true,
        }];
        let ui = UiState {
            mode: ViewMode::IncidentReports,
            selected_incident: Some(12),
            ..UiState::new()
        };
        let rendered = render_text(&data, &ui, 160, 60);
        let actor_label = name_map(&data.all_prisoners)
            .get(&7)
            .cloned()
            .unwrap_or_else(|| "P7".to_string());
        for needle in [
            "Cause",
            "prior argument",
            "Argument",
            "Actors",
            actor_label.as_str(),
            "Validation",
            "1 ok / 0 failed",
            "Outcome",
            "Disciplinary",
        ] {
            assert!(
                rendered.contains(needle),
                "incident detail missing {needle}"
            );
        }
        assert!(
            !rendered.contains("(missing)"),
            "full incident records must resolve causal links"
        );
    }

    #[test]
    fn decision_detail_shows_cause_actors_validation_and_outcome() {
        let world = alc_world::World::new(12);
        let mut data = view_data_from_world(&world);
        data.decision_audit = vec![sumo::DecisionAuditRecord {
            time: alc_core::SimTime { secs: 90 },
            actor: "prisoner:7".to_string(),
            source: "agent".to_string(),
            action: "fight".to_string(),
            summary: "attacked peer".to_string(),
            validation: "rejected: guard nearby".to_string(),
            reason: "guard nearby".to_string(),
            rationale: "status".to_string(),
            repair_reason: "clamped to socialize".to_string(),
            ..Default::default()
        }];
        let ui = UiState {
            mode: ViewMode::DecisionAudit,
            ..UiState::new()
        };
        let rendered = render_text(&data, &ui, 160, 60);
        for needle in [
            "Cause",
            "guard nearby",
            "Actors",
            "prisoner:7",
            "Validation",
            "rejected: guard nearby",
            "Outcome",
            "Repair reason",
        ] {
            assert!(
                rendered.contains(needle),
                "decision detail missing {needle}"
            );
        }
    }

    #[test]
    fn decision_detail_follows_cursor_when_key_is_stale() {
        let world = alc_world::World::new(13);
        let mut data = view_data_from_world(&world);
        data.decision_audit = (0..2)
            .map(|i| sumo::DecisionAuditRecord {
                time: alc_core::SimTime { secs: 100 + i },
                actor: format!("prisoner:{i}"),
                action: "fight".to_string(),
                summary: format!("decision {i}"),
                reason: format!("DETAIL_TOKEN_{i}"),
                validation: "accepted".to_string(),
                ..Default::default()
            })
            .collect();
        let stale_key = crate::decision_key(&data.decision_audit[0]);
        let ui = UiState {
            mode: ViewMode::DecisionAudit,
            scroll: 1,
            selected_decision: Some(stale_key),
            ..UiState::new()
        };
        let rendered = render_text(&data, &ui, 160, 60);
        assert!(
            rendered.contains("DETAIL_TOKEN_1"),
            "detail must follow the cursor"
        );
        assert!(
            !rendered.contains("DETAIL_TOKEN_0"),
            "stale key must not override the cursor"
        );
    }

    #[test]
    fn help_overlay_scrolls_and_fits_at_80x24() {
        let world = alc_world::World::new(5);
        let data = view_data_from_world(&world);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut ui = UiState::new();
        ui.show_help = true;
        terminal
            .draw(|frame| draw(frame, &data, &ui, &status(false)))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                rendered.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(rendered.contains("KEYBINDINGS"));
        // Scroll to the tail; help must still render without overflow.
        ui.help_scroll = 3;
        terminal
            .draw(|frame| draw(frame, &data, &ui, &status(false)))
            .unwrap();
    }

    #[test]
    fn help_body_documents_current_keybindings() {
        let text = help_body()
            .into_iter()
            .flat_map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.to_string())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
            .join(" ");
        for needle in [
            "b/B Pause/resume intake",
            "u/U Select project",
            "L Lock · U Unlock",
            "Esc or ? closes it",
            "Ctrl-R Resolve",
        ] {
            assert!(text.contains(needle), "help body missing {needle}");
        }
        assert!(HELP_WIDE.contains("b/B"));
        assert!(HELP_WIDE.contains("u/U"));
        assert!(HELP_COMPACT.contains("b"));
    }

    #[test]
    fn status_bar_reports_global_critical_count() {
        let world = alc_world::World::new(3);
        let data = view_data_from_world(&world);
        let mut st = status(false);
        st.open_critical = 3;
        let mut terminal = Terminal::new(TestBackend::new(240, 1)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_status(frame, area, &data, &UiState::new(), &st);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let rendered: String = (0..buf.area.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect();
        assert!(rendered.contains("CRITICAL:3 open"), "{rendered}");
    }

    #[test]
    fn overview_event_preview_uses_capped_recent_events() {
        let world = alc_world::World::new(3);
        let mut data = view_data_from_world(&world);
        data.events = vec![alc_events::WorldEvent::new(
            1,
            sumo::SimTime::EPOCH,
            alc_events::EventKind::Rumor,
            "FULL-HISTORY-ONLY",
        )];
        data.recent_events = vec![alc_events::WorldEvent::new(
            2,
            sumo::SimTime::EPOCH,
            alc_events::EventKind::Investigation,
            "CAPPED-PREVIEW-EVENT",
        )];
        let ui = UiState {
            mode: ViewMode::InstitutionOverview,
            ..UiState::new()
        };
        let rendered = render_text(&data, &ui, 140, 40);
        assert!(
            rendered.contains("CAPPED-PREVIEW-EVENT"),
            "overview must render the capped tail"
        );
        assert!(
            !rendered.contains("FULL-HISTORY-ONLY"),
            "overview must not render the full history list"
        );
    }

    #[test]
    fn incident_detail_uses_unfiltered_sources_under_search() {
        let world = alc_world::World::new(31);
        let mut data = view_data_from_world(&world);
        let participant = data.all_prisoners[1].id.raw();
        let expected_name = data.all_prisoners[1].name.clone();
        let cause_id = 77;
        let cause = sumo::IncidentRecord {
            id: cause_id,
            kind: "Argument".to_string(),
            severity: 0.7,
            resolved: true,
            ..Default::default()
        };
        let incident = sumo::IncidentRecord {
            id: 12,
            kind: "Fight".to_string(),
            severity: 0.9,
            involved_prisoners: vec![participant],
            cause: Some(cause_id),
            causal_chain: vec![sumo::incidents::CausalLink::incident(
                cause_id,
                "prior dispute",
            )],
            disposition: "Disciplinary".to_string(),
            ..Default::default()
        };
        data.all_incident_records = vec![cause, incident.clone()];
        // Simulate a search that narrows both visible lists.
        data.prisoners.clear();
        data.incident_records = vec![incident];
        let ui = UiState {
            mode: ViewMode::IncidentReports,
            selected_incident: Some(12),
            ..UiState::new()
        };
        let rendered = render_text(&data, &ui, 160, 60);
        assert!(
            rendered.contains(&expected_name),
            "detail must use unfiltered prisoner names: {rendered}"
        );
        assert!(
            !rendered.contains(&format!("P{participant}")),
            "filtered name map must not leak placeholder ids"
        );
        assert!(rendered.contains("Argument"), "cause record missing");
        assert!(
            !rendered.contains("(missing)"),
            "full records must resolve causal links"
        );
    }
}
