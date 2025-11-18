//! Native GUI module - launches the frontend in native mode.
//!
//! Only available when the "gui" feature is enabled.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[cfg(feature = "gui")]
pub fn launch_gui() -> eframe::Result<()> {
    tracing::info!("Launching native GUI...");
    strom_frontend::run_native_gui()
}

#[cfg(feature = "gui")]
pub fn launch_gui_with_shutdown(shutdown_flag: Arc<AtomicBool>) -> eframe::Result<()> {
    tracing::info!("Launching native GUI with shutdown handler...");
    strom_frontend::run_native_gui_with_shutdown(shutdown_flag)
}

#[cfg(not(feature = "gui"))]
pub fn launch_gui() -> Result<(), String> {
    Err("GUI feature not enabled. Rebuild with --features gui".to_string())
}

#[cfg(not(feature = "gui"))]
pub fn launch_gui_with_shutdown(_shutdown_flag: Arc<AtomicBool>) -> Result<(), String> {
    Err("GUI feature not enabled. Rebuild with --features gui".to_string())
}
