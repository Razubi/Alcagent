//! `alc-gui` — the interactive egui/eframe observer for the Alcagent prison
//! simulation (Directive §20).
//!
//! This crate is the graphical counterpart of the ratatui TUI in `alc-ui`. It
//! exposes the same eleven observation views ([`ViewMode`]) and adds a live,
//! spatial map built from [`sumo::MapFrame`].
//!
//! ## Module responsibilities
//!
//! - [`app`] — the [`eframe::App`] implementation. Owns the [`sumo::Sim`], the
//!   playback clock, view/selection state, and the queue of validated control
//!   commands. All mutations enter through `sumo::Sim` control methods.
//! - [`data`] — pure, testable extraction over read-only engine snapshots into
//!   owned DTOs ([`data::Snapshot`]). Also holds the map projection math.
//! - [`map_view`] — the spatial prison map: facilities, pawns, nav edges,
//!   incident overlays, pan/zoom, hover/click inspection and rosters.
//! - [`screens`] — one `show` function per observation view; rendering only.
//! - [`panels`] — shared panels (playback toolbar, controls, detail views).
//! - [`theme`] — all colour and small widget helpers.
//! - [`settings`] — persistence of observer preferences (never world state).
//!
//! ## Authority
//!
//! The observer never mutates arbitrary world state. UI handlers enqueue a
//! [`app::Command`], which the App applies through validated `sumo::Sim`
//! methods (`tick`, `set_speed`, `apply_warden_action`, policy setters,
//! construction/rehab/contraband/parole controls). Rendering receives only
//! read-only snapshots from [`data::capture`].

pub mod app;
pub mod data;
pub mod map_view;
pub mod panels;
pub mod screens;
pub mod settings;
pub mod theme;

use serde::{Deserialize, Serialize};

/// The eleven observation modes, mirroring `alc_ui::ViewMode` so the egui and
/// ratatui observers stay interchangeable without a compile-time dependency on
/// the TUI crate.
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

/// Construction projects offered by the observer's build controls (mirrors
/// `alc_ui::CONSTRUCTION_KINDS`).
pub const CONSTRUCTION_KINDS: [alc_world::FacilityKind; 12] = [
    alc_world::FacilityKind::CellBlock,
    alc_world::FacilityKind::Dormitory,
    alc_world::FacilityKind::Medical,
    alc_world::FacilityKind::Education,
    alc_world::FacilityKind::Isolation,
    alc_world::FacilityKind::Visitor,
    alc_world::FacilityKind::Workshop,
    // Keep the original order stable; append the remaining facility kinds so
    // existing observer selections/cycle expectations do not shift.
    alc_world::FacilityKind::Kitchen,
    alc_world::FacilityKind::Recreation,
    alc_world::FacilityKind::Admin,
    alc_world::FacilityKind::Staff,
    alc_world::FacilityKind::Yard,
];

pub use app::{App, launch};
