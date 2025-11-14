//! Strom backend library.
//!
//! This module exposes the application builder for use in tests.

use axum::{routing::get, routing::post, Router};
use tower_http::cors::{Any, CorsLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub mod api;
pub mod assets;
pub mod config;
pub mod events;
pub mod gst;
pub mod openapi;
pub mod state;
pub mod storage;

use state::AppState;

/// Create the Axum application router.
///
/// This function is used both by the main server binary and by integration tests.
pub async fn create_app() -> Router {
    create_app_with_state(AppState::default()).await
}

/// Create the Axum application router with a given state.
pub async fn create_app_with_state(state: AppState) -> Router {
    // Initialize GStreamer (idempotent)
    gstreamer::init().expect("Failed to initialize GStreamer");

    // Build API router
    let api_router = Router::new()
        .route("/flows", get(api::flows::list_flows))
        .route("/flows", post(api::flows::create_flow))
        .route("/flows/{id}", get(api::flows::get_flow))
        .route("/flows/{id}", post(api::flows::update_flow))
        .route(
            "/flows/{id}",
            axum::routing::delete(api::flows::delete_flow),
        )
        .route("/flows/{id}/start", post(api::flows::start_flow))
        .route("/flows/{id}/stop", post(api::flows::stop_flow))
        .route("/flows/{id}/debug-graph", get(api::flows::debug_graph))
        .route("/elements", get(api::elements::list_elements))
        .route("/elements/{name}", get(api::elements::get_element_info))
        .route("/events", get(api::sse::events_stream));

    // Build main router with Swagger UI
    Router::new()
        .route("/health", get(health))
        .merge(
            SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi::ApiDoc::openapi()),
        )
        .nest("/api", api_router)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
        // Serve embedded frontend for all other routes
        .fallback(assets::serve_static)
}

/// Health check endpoint.
async fn health() -> &'static str {
    "OK"
}
