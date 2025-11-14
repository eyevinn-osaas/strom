//! API request and response types.

use crate::element::ElementInfo;
use crate::flow::{Flow, FlowId};
use crate::state::PipelineState;
use serde::{Deserialize, Serialize};

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

// ============================================================================
// Flow API Types
// ============================================================================

/// Request to create a new flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct CreateFlowRequest {
    pub name: String,
}

/// Request to update an existing flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct UpdateFlowRequest {
    pub flow: Flow,
}

/// Response containing a single flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct FlowResponse {
    pub flow: Flow,
}

/// Response containing a list of flows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct FlowListResponse {
    pub flows: Vec<Flow>,
}

/// Response for flow state query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct FlowStateResponse {
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = Uuid))]
    pub id: FlowId,
    pub state: PipelineState,
}

// ============================================================================
// Element API Types
// ============================================================================

/// Response containing information about available elements.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ElementListResponse {
    pub elements: Vec<ElementInfo>,
}

/// Response containing detailed information about a specific element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ElementInfoResponse {
    pub element: ElementInfo,
}

// ============================================================================
// WebSocket Message Types
// ============================================================================

/// Messages sent from server to client via WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Pipeline state has changed
    StateChange {
        flow_id: FlowId,
        state: PipelineState,
    },
    /// An error occurred
    Error {
        flow_id: Option<FlowId>,
        message: String,
    },
    /// A warning message
    Warning {
        flow_id: Option<FlowId>,
        message: String,
    },
    /// Informational message
    Info { message: String },
}

/// Messages sent from client to server via WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Subscribe to updates for a specific flow
    Subscribe { flow_id: FlowId },
    /// Unsubscribe from updates for a specific flow
    Unsubscribe { flow_id: FlowId },
    /// Ping to keep connection alive
    Ping,
}

// ============================================================================
// Error Response
// ============================================================================

/// Standard error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ErrorResponse {
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            details: None,
        }
    }

    pub fn with_details(error: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            details: Some(details.into()),
        }
    }
}
