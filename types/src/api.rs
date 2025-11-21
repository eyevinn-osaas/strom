//! API request and response types.

use crate::element::{ElementInfo, PropertyValue};
use crate::flow::{Flow, FlowId, FlowProperties};
use crate::state::PipelineState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// Request to update flow properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct UpdateFlowPropertiesRequest {
    pub properties: FlowProperties,
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
// Property API Types (for live updates)
// ============================================================================

/// Request to update a property on a running pipeline element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct UpdatePropertyRequest {
    /// The name of the property to update
    pub property_name: String,
    /// The new value for the property
    pub value: PropertyValue,
}

/// Response containing current property values from a running element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ElementPropertiesResponse {
    /// The element ID
    pub element_id: String,
    /// Current property values
    pub properties: HashMap<String, PropertyValue>,
}

/// Request to update a property on a pad in a running pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct UpdatePadPropertyRequest {
    /// The name of the property to update
    pub property_name: String,
    /// The new value for the property
    pub value: PropertyValue,
}

/// Response containing current property values from a pad.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PadPropertiesResponse {
    /// The element ID
    pub element_id: String,
    /// The pad name
    pub pad_name: String,
    /// Current property values
    pub properties: HashMap<String, PropertyValue>,
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
// WebRTC Stats Types
// ============================================================================

/// WebRTC statistics for a flow.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct WebRtcStats {
    /// Stats for each WebRTC connection (keyed by element name)
    pub connections: HashMap<String, WebRtcConnectionStats>,
}

/// Stats for a single WebRTC connection (webrtcbin element).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct WebRtcConnectionStats {
    /// Inbound RTP stream statistics
    pub inbound_rtp: Vec<RtpStreamStats>,
    /// Outbound RTP stream statistics
    pub outbound_rtp: Vec<RtpStreamStats>,
    /// ICE candidate pair statistics
    pub ice_candidates: Option<IceCandidateStats>,
    /// Raw stats as key-value pairs (for debugging/extensibility)
    pub raw: HashMap<String, String>,
}

/// RTP stream statistics (inbound or outbound).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct RtpStreamStats {
    /// Stream identifier
    pub ssrc: Option<u32>,
    /// Media type (audio or video)
    pub media_type: Option<String>,
    /// Codec being used
    pub codec: Option<String>,
    /// Total bytes sent/received
    pub bytes: Option<u64>,
    /// Total packets sent/received
    pub packets: Option<u64>,
    /// Packets lost (inbound only)
    pub packets_lost: Option<i64>,
    /// Jitter in seconds (inbound only)
    pub jitter: Option<f64>,
    /// Round-trip time in seconds
    pub round_trip_time: Option<f64>,
    /// Bitrate in bits per second (calculated)
    pub bitrate: Option<u64>,
}

/// ICE candidate statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct IceCandidateStats {
    /// Local candidate type (host, srflx, relay)
    pub local_candidate_type: Option<String>,
    /// Remote candidate type
    pub remote_candidate_type: Option<String>,
    /// Connection state
    pub state: Option<String>,
}

/// Response containing WebRTC statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct WebRtcStatsResponse {
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = Uuid))]
    pub flow_id: FlowId,
    pub stats: WebRtcStats,
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
