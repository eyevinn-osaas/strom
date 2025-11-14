//! Element discovery API handlers.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use strom_types::api::{ElementInfoResponse, ElementListResponse, ErrorResponse};
use tracing::info;
use utoipa;

use crate::state::AppState;

/// List all available GStreamer elements.
#[utoipa::path(
    get,
    path = "/api/elements",
    tag = "elements",
    responses(
        (status = 200, description = "List of available GStreamer elements", body = ElementListResponse)
    )
)]
pub async fn list_elements(
    State(state): State<AppState>,
) -> Result<Json<ElementListResponse>, (StatusCode, Json<ErrorResponse>)> {
    info!("Discovering GStreamer elements");

    let elements = state.discover_elements().await;

    info!("Discovered {} elements", elements.len());

    Ok(Json(ElementListResponse { elements }))
}

/// Get detailed information about a specific element type.
#[utoipa::path(
    get,
    path = "/api/elements/{name}",
    tag = "elements",
    params(
        ("name" = String, Path, description = "Element type name (e.g., 'videotestsrc')")
    ),
    responses(
        (status = 200, description = "Element information", body = ElementInfoResponse),
        (status = 404, description = "Element not found", body = ErrorResponse)
    )
)]
pub async fn get_element_info(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ElementInfoResponse>, (StatusCode, Json<ErrorResponse>)> {
    info!("Getting element info for: {}", name);

    match state.get_element_info(&name).await {
        Some(element) => Ok(Json(ElementInfoResponse { element })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new(format!("Element '{}' not found", name))),
        )),
    }
}
