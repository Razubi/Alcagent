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
    pub selected_decision: Option<String>,
    pub selected_construction: Option<u64>,
    pub selected_rehab: Option<u64>,
    pub selected_contraband: Option<u64>,
    /// Free-text search query over the active list (name / id substring).
    pub search: String,
    /// Declarative incident query (0.05); [`UiState::filter`] is the accessor.
    pub incident_filter: sumo::incidents::IncidentFilter,
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
            incident_filter: sumo::incidents::IncidentFilter::all(),
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
}

/// Render plan is documented here; actual drawing lives behind a trait so
/// headless tests can run without a terminal.
pub trait Renderer {
    fn render(&mut self, state: &UiState, frame: &mut dyn std::any::Any);
}