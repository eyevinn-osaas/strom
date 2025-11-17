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
/// This endpoint triggers lazy loading of element properties.
/// Properties are introspected on-demand and cached for future requests.
#[utoipa::path(
    get,
    path = "/api/elements/{name}",
    tag = "elements",
    params(
        ("name" = String, Path, description = "Element type name (e.g., 'videotestsrc')")
    ),
    responses(
        (status = 200, description = "Element information with properties", body = ElementInfoResponse),
        (status = 404, description = "Element not found", body = ErrorResponse)
    )
)]
pub async fn get_element_info(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ElementInfoResponse>, (StatusCode, Json<ErrorResponse>)> {
    info!("Getting element info with properties for: {}", name);

    // Use lazy loading to get element info with properties
    match state.get_element_info_with_properties(&name).await {
        Some(element) => {
            info!(
                "Returned element '{}' with {} properties",
                name,
                element.properties.len()
            );
            Ok(Json(ElementInfoResponse { element }))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new(format!("Element '{}' not found", name))),
        )),
    }
}

/// Get pad properties for a specific element type.
/// This endpoint introspects pad properties on-demand for better safety.
#[utoipa::path(
    get,
    path = "/api/elements/{name}/pads",
    tag = "elements",
    params(
        ("name" = String, Path, description = "Element type name (e.g., 'audiomixer')")
    ),
    responses(
        (status = 200, description = "Element information with pad properties", body = ElementInfoResponse),
        (status = 404, description = "Element not found", body = ErrorResponse)
    )
)]
pub async fn get_element_pad_properties(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ElementInfoResponse>, (StatusCode, Json<ErrorResponse>)> {
    info!("Getting pad properties for element: {}", name);

    // Introspect pad properties on-demand
    match state.get_element_pad_properties(&name).await {
        Some(element) => {
            let total_props: usize = element
                .src_pads
                .iter()
                .map(|p| p.properties.len())
                .sum::<usize>()
                + element
                    .sink_pads
                    .iter()
                    .map(|p| p.properties.len())
                    .sum::<usize>();
            info!(
                "Returned element '{}' with {} pad properties",
                name, total_props
            );
            Ok(Json(ElementInfoResponse { element }))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new(format!("Element '{}' not found", name))),
        )),
    }
}
