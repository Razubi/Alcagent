//! GUI rendering backend, enabled with `--features gui`.
//! Provides the egui/eframe application types.
//! Panel rendering logic is deferred to later phases.

/// The main GUI application window type.
/// Compiled only when the `gui` feature is active.
pub struct SumoApp;

/// Launch the egui/eframe application.
/// Compiled only when the `gui` feature is active.
pub fn launch_gui(_seed: u64) -> Result<(), String> {
    Ok(())
}
