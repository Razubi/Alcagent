//! `alc-gui` — the interactive egui observer for the Alcagent prison
//! simulation (Directive §20).
//!
//! This crate is the egui counterpart of the ratatui TUI in [`alc-ui`]: the
//! same nine observation screens ([`alc_ui::ViewMode`]) plus the read-only
//! data extraction helpers ([`alc_ui::views::truncate`],
//! [`alc_ui::views::class_short`], [`alc_ui::views::name_map`]), and it adds
//! live beta panels that expose engine state the TUI predates: the event
//! timeline feed, incident reports, policy management, expansion tools, the
//! guard assignment roster and the daily schedule.
//!
//! ## Architecture
//!
//! - [`App`] — the [`eframe::App`] implementation: owns [`sumo::Sim`], the
//!   active screen mode, the live event feed and queued user actions.
//! - [`data`] — pure, testable extraction over read-only engine snapshots
//!   ([`sumo::Observation`], [`sumo::AgentObservation`], [`sumo::Stats`],
//!   [`sumo::Sim::world`]) into row-shaped structs used by the panels.
//! - [`screens`] — the nine observation screens, one `show` function per
//!   mode.
//! - [`panels`] — beta panels (live feed, incident reports, policy
//!   management, expansion tools, guard assignment, daily schedule).
//! - [`theme`] — colouring and layout helpers over egui primitives.
//! - [`settings`] — persistence of the last-used screen.
//!
//! Authority rules (Directive §2, §A22): the GUI is an observer with limited
//! write access. It can only tick the engine (including the explicit
//! "+1 day" command), apply warden policy actions through
//! [`sumo::Sim::apply_warden_action`], and expand via
//! [`alc_warden::WardenAction::Expand`]. Every panel is strictly read-only
//! over [`sumo::Sim::world`].

pub mod app;
pub mod data;
pub mod panels;
pub mod screens;
pub mod settings;
pub mod theme;

pub use app::App;
pub use alc_ui::ViewMode;
