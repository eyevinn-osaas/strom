//! Strom backend server.

use std::net::SocketAddr;
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

use strom_backend::{config::Config, create_app_with_state, state::AppState};
use strom_types::PipelineState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging - use RUST_LOG env var or default to info
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
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
    let state = AppState::with_json_storage(&config.flows_path, &config.blocks_path);
    state.load_from_storage().await?;

    // Discover and cache GStreamer elements
    state.discover_and_cache_elements().await?;

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

    // Start server - bind to 0.0.0.0 to be accessible from all interfaces (Docker, network, etc.)
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
