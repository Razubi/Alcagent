//! Observation UI (Directive §20). Ratatui TUI observer with multiple modes.
//!
//! The crate is split in three focused pieces:
//!
//! - [`UiState`] / [`ViewMode`] — the (persistable) observer state.
//! - [`views`] — pure render functions over a read-only [`alc_world::World`].
//! - [`app`] — terminal setup, the simulation loop and keyboard handling.

pub mod app;
pub mod views;

use serde::{Deserialize, Serialize};

/// Observation modes required by the directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    InstitutionOverview,
    Prisoner,
    Gang,
    Staff,
    Warden,
    Financial,
    EventTimeline,
    SocialNetwork,
    Map,
    IncidentReports,
    DecisionAudit,
}

/// Construction projects offered by the observer `c`/`C` controls.
///
/// Kept in one place so the keybind handler and Warden view never drift apart.
pub const CONSTRUCTION_KINDS: [alc_world::FacilityKind; 7] = [
    alc_world::FacilityKind::CellBlock,
    alc_world::FacilityKind::Dormitory,
    alc_world::FacilityKind::Medical,
    alc_world::FacilityKind::Education,
    alc_world::FacilityKind::Isolation,
    alc_world::FacilityKind::Visitor,
    alc_world::FacilityKind::Workshop,
];

/// Observer-side selection/filter state (0.05 control wiring).
///
/// The fields added here back the TUI control layer (`app::App`) without
/// touching the render functions in [`crate::views`]: search queries, entity
/// drill-down selections and the declarative incident query all live here.
pub struct UiState {
    pub mode: ViewMode,
    pub selected_prisoner: Option<u64>,
    pub selected_gang: Option<u64>,
    pub selected_guard: Option<u64>,
    pub selected_incident: Option<u64>,
    /// Stable key of the decision-audit row under the cursor, produced by
    /// [`decision_key`]. Content-derived so trimming older audit entries or
    /// changing a filter cannot make the key point at a different decision.
    pub selected_decision: Option<String>,
    pub selected_construction: Option<u64>,
    pub selected_rehab: Option<u64>,
    pub selected_contraband: Option<u64>,
    /// Free-text search query over the active list (name / id substring).
    pub search: String,
    /// Bookmarked/pinned entity ids (persist across mode changes).
    pub bookmarks: Vec<u64>,
    /// Recent observer-facing notifications (bounded).
    pub notifications: Vec<String>,
    /// Pause the simulation automatically on critical incidents.
    pub pause_on_critical: bool,
    /// Show the notification feed, pinned entities and observer flags.
    pub show_notifications: bool,
    /// Selected entry in [`CONSTRUCTION_KINDS`] for the Warden build controls.
    pub construction_kind_index: usize,
    /// Whether free-text search input is active.
    pub search_active: bool,
    /// Declarative incident query (0.05); [`UiState::filter`] is the accessor.
    pub incident_filter: sumo::incidents::IncidentFilter,
    /// Optional decision validation filter: "accepted", "repaired", "rejected", or None.
    pub decision_validation_filter: Option<String>,
    /// Show the help overlay with all keybindings.
    pub show_help: bool,
    /// Scroll offset for the (modal) help overlay; kept separate from the
    /// shared entity cursor so help navigation never moves entity selection.
    pub help_scroll: u16,
    /// Shared entity cursor (`scroll`): index into the active view's list.
    pub scroll: u16,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            mode: ViewMode::InstitutionOverview,
            selected_prisoner: None,
            selected_gang: None,
            selected_guard: None,
            selected_incident: None,
            selected_decision: None,
            selected_construction: None,
            selected_rehab: None,
            selected_contraband: None,
            search: String::new(),
            search_active: false,
            bookmarks: Vec::new(),
            notifications: Vec::new(),
            pause_on_critical: false,
            show_notifications: false,
            construction_kind_index: 0,
            incident_filter: sumo::incidents::IncidentFilter::all(),
            decision_validation_filter: None,
            show_help: false,
            help_scroll: 0,
            scroll: 0,
        }
    }

    /// Current search query (empty matches everything).
    pub fn query(&self) -> &str {
        &self.search
    }

    /// Set (or replace) the free-text search query.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.search = query.into();
    }

    /// Clear the search query.
    pub fn clear_query(&mut self) {
        self.search.clear();
    }

    /// Current declarative incident filter (all incidents by default).
    pub fn filter(&self) -> &sumo::incidents::IncidentFilter {
        &self.incident_filter
    }

    /// Replace the declarative incident filter.
    pub fn set_filter(&mut self, filter: sumo::incidents::IncidentFilter) {
        self.incident_filter = filter;
    }

    /// Cycle incident minimum severity filter: all -> high(0.40) -> critical(0.75) -> all.
    pub fn cycle_incident_min_severity(&mut self) {
        let next = match self.incident_filter.min_severity {
            None => Some(0.40),
            Some(s) if s < 0.74 => Some(0.75),
            _ => None,
        };
        self.incident_filter.min_severity = next;
    }

    /// Cycle incident resolved filter: all -> open only -> resolved only -> all.
    pub fn cycle_incident_resolved(&mut self) {
        self.incident_filter.resolved = match self.incident_filter.resolved {
            None => Some(false),
            Some(false) => Some(true),
            Some(true) => None,
        };
    }

    /// Cycle decision validation filter: all -> accepted -> repaired -> rejected -> all.
    pub fn cycle_decision_validation_filter(&mut self) {
        self.decision_validation_filter = match self.decision_validation_filter.as_deref() {
            None => Some("accepted".to_string()),
            Some("accepted") => Some("repaired".to_string()),
            Some("repaired") => Some("rejected".to_string()),
            _ => None,
        };
    }
}

/// Render plan is documented here; actual drawing lives behind a trait so
/// headless tests can run without a terminal.
pub trait Renderer {
    fn render(&mut self, state: &UiState, frame: &mut dyn std::any::Any);
}

/// Stable identity for an audit row, shared by the app and the detail renderer.
///
/// Every free-form field is length-prefixed, so no value can forge a field
/// boundary and two distinct decisions cannot collide (including two identical
/// actions by the same actor at different times).
pub(crate) fn decision_key(record: &sumo::DecisionAuditRecord) -> String {
    use std::fmt::Write;

    fn push(key: &mut String, value: &str) {
        let _ = write!(key, "{}:{}", value.len(), value);
    }

    // `SimTime::to_string` is minute-resolution; `secs` distinguishes audit
    // rows written within the same displayed minute.
    let time = record.time.secs.to_string();
    let outcome = format!("{:?}", record.outcome);
    let mut key = String::with_capacity(128);
    push(&mut key, &time);
    push(&mut key, &outcome);
    for field in [
        &record.actor,
        &record.source,
        &record.action,
        &record.summary,
        &record.validation,
        &record.reason,
        &record.rationale,
        &record.repair_reason,
    ] {
        push(&mut key, field);
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_key_is_stable_and_disjoint() {
        let mut first = sumo::DecisionAuditRecord::default();
        first.actor = "guard:7|prisoner:9".to_string();
        first.action = "fight".to_string();
        let mut second = first.clone();
        second.source = "agent".to_string();
        let key = decision_key(&first);
        assert_eq!(key, decision_key(&first));
        assert_ne!(key, decision_key(&second));
        second.source.clear();
        second.validation = "agent".to_string();
        assert_ne!(key, decision_key(&second), "field boundaries must not be forgeable");
        let mut later = first.clone();
        later.time.secs += 1;
        assert_ne!(key, decision_key(&later), "time alone must disambiguate rows");
    }
}