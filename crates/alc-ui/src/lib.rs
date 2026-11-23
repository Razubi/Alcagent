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

pub struct UiState {
    pub mode: ViewMode,
    pub selected_prisoner: Option<u64>,
    pub selected_gang: Option<u64>,
    pub selected_guard: Option<u64>,
    pub scroll: u16,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            mode: ViewMode::InstitutionOverview,
            selected_prisoner: None,
            selected_gang: None,
            selected_guard: None,
            scroll: 0,
        }
    }
}

/// Render plan is documented here; actual drawing lives behind a trait so
/// headless tests can run without a terminal.
pub trait Renderer {
    fn render(&mut self, state: &UiState, frame: &mut dyn std::any::Any);
}