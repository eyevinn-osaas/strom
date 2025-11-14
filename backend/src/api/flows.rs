//! Flow API handlers.

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::process::Command;
use strom_types::{
    api::{CreateFlowRequest, ErrorResponse, FlowListResponse, FlowResponse},
    Flow, FlowId,
};
use tempfile::NamedTempFile;
use tracing::{error, info};
use utoipa;

use crate::state::AppState;

/// List all flows.
#[utoipa::path(
    get,
    path = "/api/flows",
    tag = "flows",
    responses(
        (status = 200, description = "List all flows", body = FlowListResponse)
    )
)]
pub async fn list_flows(State(state): State<AppState>) -> Json<FlowListResponse> {
    let flows = state.get_flows().await;
    Json(FlowListResponse { flows })
}

/// Get a specific flow by ID.
#[utoipa::path(
    get,
    path = "/api/flows/{id}",
    tag = "flows",
    params(
        ("id" = String, Path, description = "Flow ID (UUID)")
    ),
    responses(
        (status = 200, description = "Flow found", body = FlowResponse),
        (status = 404, description = "Flow not found", body = ErrorResponse)
    )
)]
pub async fn get_flow(
    State(state): State<AppState>,
    Path(id): Path<FlowId>,
) -> Result<Json<FlowResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.get_flow(&id).await {
        Some(flow) => Ok(Json(FlowResponse { flow })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Flow not found")),
        )),
    }
}

/// Create a new flow.
#[utoipa::path(
    post,
    path = "/api/flows",
    tag = "flows",
    request_body = CreateFlowRequest,
    responses(
        (status = 201, description = "Flow created", body = FlowResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn create_flow(
    State(state): State<AppState>,
    Json(req): Json<CreateFlowRequest>,
) -> Result<(StatusCode, Json<FlowResponse>), (StatusCode, Json<ErrorResponse>)> {
    info!("Received create flow request: name='{}'", req.name);

    let flow = Flow::new(req.name);

    info!("Creating flow: {} ({})", flow.name, flow.id);

    if let Err(e) = state.upsert_flow(flow.clone()).await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::with_details(
                "Failed to save flow",
                e.to_string(),
            )),
        ));
    }

    Ok((StatusCode::CREATED, Json(FlowResponse { flow })))
}

/// Update an existing flow.
#[utoipa::path(
    post,
    path = "/api/flows/{id}",
    tag = "flows",
    params(
        ("id" = String, Path, description = "Flow ID (UUID)")
    ),
    request_body = Flow,
    responses(
        (status = 200, description = "Flow updated", body = FlowResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 404, description = "Flow not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_flow(
    State(state): State<AppState>,
    Path(id): Path<FlowId>,
    Json(flow): Json<Flow>,
) -> Result<Json<FlowResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Ensure the ID in the path matches the flow
    if id != flow.id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Flow ID mismatch")),
        ));
    }

    // Check if flow exists
    if state.get_flow(&id).await.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Flow not found")),
        ));
    }

    info!("Updating flow: {} ({})", flow.name, flow.id);

    if let Err(e) = state.upsert_flow(flow.clone()).await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::with_details(
                "Failed to save flow",
                e.to_string(),
            )),
        ));
    }

    Ok(Json(FlowResponse { flow }))
}

/// Delete a flow.
#[utoipa::path(
    delete,
    path = "/api/flows/{id}",
    tag = "flows",
    params(
        ("id" = String, Path, description = "Flow ID (UUID)")
    ),
    responses(
        (status = 204, description = "Flow deleted"),
        (status = 404, description = "Flow not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn delete_flow(
    State(state): State<AppState>,
    Path(id): Path<FlowId>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    match state.delete_flow(&id).await {
        Ok(true) => {
            info!("Deleted flow: {}", id);
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Flow not found")),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::with_details(
                "Failed to delete flow",
                e.to_string(),
            )),
        )),
    }
}

/// Start a flow (pipeline).
#[utoipa::path(
    post,
    path = "/api/flows/{id}/start",
    tag = "flows",
    params(
        ("id" = String, Path, description = "Flow ID (UUID)")
    ),
    responses(
        (status = 200, description = "Flow started", body = FlowResponse),
        (status = 404, description = "Flow not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn start_flow(
    State(state): State<AppState>,
    Path(id): Path<FlowId>,
) -> Result<Json<FlowResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Start the pipeline
    if let Err(e) = state.start_flow(&id).await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::with_details(
                "Failed to start flow",
                e.to_string(),
            )),
        ));
    }

    // Return updated flow with state
    match state.get_flow(&id).await {
        Some(flow) => Ok(Json(FlowResponse { flow })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Flow not found")),
        )),
    }
}

/// Stop a flow (pipeline).
#[utoipa::path(
    post,
    path = "/api/flows/{id}/stop",
    tag = "flows",
    params(
        ("id" = String, Path, description = "Flow ID (UUID)")
    ),
    responses(
        (status = 200, description = "Flow stopped", body = FlowResponse),
        (status = 404, description = "Flow not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn stop_flow(
    State(state): State<AppState>,
    Path(id): Path<FlowId>,
) -> Result<Json<FlowResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Stop the pipeline
    if let Err(e) = state.stop_flow(&id).await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::with_details(
                "Failed to stop flow",
                e.to_string(),
            )),
        ));
    }

    // Return updated flow with state
    match state.get_flow(&id).await {
        Some(flow) => Ok(Json(FlowResponse { flow })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Flow not found")),
        )),
    }
}

/// Generate a debug DOT/SVG graph for a flow's pipeline.
///
/// This endpoint generates a GraphViz DOT graph of the GStreamer pipeline
/// and converts it to SVG format. The SVG is returned directly and can be
/// viewed in a browser.
pub async fn debug_graph(
    State(state): State<AppState>,
    Path(id): Path<FlowId>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    info!("Generating debug graph for flow: {}", id);

    // Generate DOT graph from the pipeline
    let dot_content = state.generate_debug_graph(&id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new(
                "Flow not found or not running. Start the flow first.",
            )),
        )
    })?;

    // Create temporary DOT file
    let mut dot_file = NamedTempFile::new().map_err(|e| {
        error!("Failed to create temporary DOT file: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::with_details(
                "Failed to create temporary file",
                e.to_string(),
            )),
        )
    })?;

    use std::io::Write;
    dot_file.write_all(dot_content.as_bytes()).map_err(|e| {
        error!("Failed to write DOT content: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::with_details(
                "Failed to write DOT file",
                e.to_string(),
            )),
        )
    })?;

    // Convert DOT to SVG using the 'dot' command
    let svg_output = Command::new("dot")
        .arg("-Tsvg")
        .arg(dot_file.path())
        .output()
        .map_err(|e| {
            error!("Failed to execute 'dot' command: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::with_details(
                    "Failed to convert to SVG. Ensure Graphviz is installed.",
                    e.to_string(),
                )),
            )
        })?;

    if !svg_output.status.success() {
        let stderr = String::from_utf8_lossy(&svg_output.stderr);
        error!("dot command failed: {}", stderr);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::with_details(
                "SVG conversion failed",
                stderr.to_string(),
            )),
        ));
    }

    info!("Successfully generated SVG debug graph for flow: {}", id);

    // Return SVG as response
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/svg+xml")],
        svg_output.stdout,
    )
        .into_response())
}

/// Generate SDP for a specific block in a flow.
///
/// Returns the SDP (Session Description Protocol) data for AES67 output blocks.
/// This SDP can be used by receivers to connect to the audio stream.
#[utoipa::path(
    get,
    path = "/api/flows/{flow_id}/blocks/{block_id}/sdp",
    tag = "flows",
    params(
        ("flow_id" = String, Path, description = "Flow ID (UUID)"),
        ("block_id" = String, Path, description = "Block instance ID")
    ),
    responses(
        (status = 200, description = "SDP generated successfully", content_type = "application/sdp"),
        (status = 404, description = "Flow or block not found", body = ErrorResponse),
        (status = 400, description = "Block type does not support SDP", body = ErrorResponse)
    )
)]
pub async fn get_block_sdp(
    State(state): State<AppState>,
    Path((flow_id, block_id)): Path<(FlowId, String)>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    info!("Generating SDP for block {} in flow {}", block_id, flow_id);

    // Get the flow
    let flow = state.get_flow(&flow_id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Flow not found")),
        )
    })?;

    // Find the block instance
    let block = flow
        .blocks
        .iter()
        .find(|b| b.id == block_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Block not found in flow")),
            )
        })?;

    // Check if this is an AES67 output block
    if block.block_definition_id != "builtin.aes67_output" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "SDP generation is only supported for AES67 output blocks",
            )),
        ));
    }

    // Generate SDP
    let sdp = crate::blocks::sdp::generate_aes67_output_sdp(block, &flow.name);

    info!("Successfully generated SDP for block {}", block_id);

    // Return SDP as plain text response
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/sdp")],
        sdp,
    )
        .into_response())
}
