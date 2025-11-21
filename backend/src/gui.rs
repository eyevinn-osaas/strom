//! Native GUI module - launches the frontend in native mode.
//!
//! Only available when the "gui" feature is enabled.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[cfg(feature = "gui")]
pub fn launch_gui(port: u16) -> eframe::Result<()> {
    tracing::info!("Launching native GUI connecting to port {}...", port);
    strom_frontend::run_native_gui(port)
}

#[cfg(feature = "gui")]
pub fn launch_gui_with_shutdown(port: u16, shutdown_flag: Arc<AtomicBool>) -> eframe::Result<()> {
    tracing::info!(
        "Launching native GUI with shutdown handler connecting to port {}...",
        port
    );
    strom_frontend::run_native_gui_with_shutdown(port, shutdown_flag, None)
}

/// Launch the native GUI with authentication token for auto-login.
#[cfg(feature = "gui")]
pub fn launch_gui_with_auth(
    port: u16,
    shutdown_flag: Arc<AtomicBool>,
    auth_token: String,
) -> eframe::Result<()> {
    tracing::info!(
        "Launching native GUI with auth token connecting to port {}...",
        port
    );
    strom_frontend::run_native_gui_with_shutdown(port, shutdown_flag, Some(auth_token))
}

#[cfg(not(feature = "gui"))]
pub fn launch_gui(_port: u16) -> Result<(), String> {
    Err("GUI feature not enabled. Rebuild with --features gui".to_string())
}

#[cfg(not(feature = "gui"))]
pub fn launch_gui_with_shutdown(_port: u16, _shutdown_flag: Arc<AtomicBool>) -> Result<(), String> {
    Err("GUI feature not enabled. Rebuild with --features gui".to_string())
}

#[cfg(not(feature = "gui"))]
pub fn launch_gui_with_auth(
    _port: u16,
    _shutdown_flag: Arc<AtomicBool>,
    _auth_token: String,
) -> Result<(), String> {
    Err("GUI feature not enabled. Rebuild with --features gui".to_string())
}
