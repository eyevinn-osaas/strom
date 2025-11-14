//! Strom backend server.

use std::net::SocketAddr;
use tracing::{error, info, Level};
use tracing_subscriber::fmt;

use strom_backend::{config::Config, create_app_with_state, state::AppState};
use strom_types::PipelineState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .compact()
        .init();

    info!("Starting Strom backend server...");

    // Initialize GStreamer
    gstreamer::init()?;
    info!("GStreamer initialized");

    // Load configuration
    let config = Config::from_env();
    info!("Configuration loaded");

    // Create application with persistent storage
    let state = AppState::with_json_storage(&config.flows_path);
    state.load_from_storage().await?;

    // Restart flows that were running before shutdown
    info!("Restarting flows that were running...");
    let flows = state.get_flows().await;
    for flow in flows {
        if let Some(PipelineState::Playing) = flow.state {
            info!("Auto-restarting flow: {} ({})", flow.name, flow.id);
            match state.start_flow(&flow.id).await {
                Ok(_) => info!("Successfully restarted flow: {}", flow.name),
                Err(e) => error!("Failed to restart flow {}: {}", flow.name, e),
            }
        }
    }

    let app = create_app_with_state(state).await;

    // Start server
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
