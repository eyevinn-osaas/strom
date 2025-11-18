//! Native GUI module - launches the frontend in native mode.
//!
//! Only available when the "gui" feature is enabled.

#[cfg(feature = "gui")]
pub fn launch_gui() -> eframe::Result<()> {
    tracing::info!("Launching native GUI...");
    strom_frontend::run_native_gui()
}

#[cfg(not(feature = "gui"))]
pub fn launch_gui() -> Result<(), String> {
    Err("GUI feature not enabled. Rebuild with --features gui".to_string())
}
