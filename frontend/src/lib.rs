//! Strom frontend library.
//!
//! This module exposes the frontend application for embedding in native mode.

#![warn(clippy::all, rust_2018_idioms)]
#![allow(dead_code)]

mod api;
mod app;
mod graph;
mod palette;
mod properties;
mod state;
mod ws;

// Re-export the app for use by the backend
pub use app::StromApp;

// Global tokio runtime for native builds
#[cfg(not(target_arch = "wasm32"))]
use once_cell::sync::Lazy;

#[cfg(not(target_arch = "wasm32"))]
static TOKIO_RUNTIME: Lazy<tokio::runtime::Runtime> =
    Lazy::new(|| tokio::runtime::Runtime::new().expect("Failed to create tokio runtime"));

// Re-export the native entry point (without tracing init - parent should handle that)
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native_gui() -> eframe::Result<()> {
    tracing::info!("Initializing Strom frontend (embedded mode)");

    // Initialize the global runtime (will be created on first access)
    let _runtime = &*TOKIO_RUNTIME;
    tracing::info!("Tokio runtime initialized for native GUI");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("Strom - GStreamer Flow Engine"),
        ..Default::default()
    };

    // Enter runtime context for the entire eframe run
    let _guard = TOKIO_RUNTIME.enter();

    eframe::run_native(
        "Strom",
        native_options,
        Box::new(|cc| {
            // Set dark theme
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(StromApp::new(cc)))
        }),
    )
}

// Native entry point with shutdown handler for Ctrl+C
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native_gui_with_shutdown(
    shutdown_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> eframe::Result<()> {
    tracing::info!("Initializing Strom frontend (embedded mode with shutdown handler)");

    // Initialize the global runtime (will be created on first access)
    let _runtime = &*TOKIO_RUNTIME;
    tracing::info!("Tokio runtime initialized for native GUI");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("Strom - GStreamer Flow Engine"),
        ..Default::default()
    };

    // Enter runtime context for the entire eframe run
    let _guard = TOKIO_RUNTIME.enter();

    eframe::run_native(
        "Strom",
        native_options,
        Box::new(move |cc| {
            // Set dark theme
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(StromApp::new_with_shutdown(cc, shutdown_flag)))
        }),
    )
}
