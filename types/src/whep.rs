//! WHEP API types shared between backend and frontend.

use serde::Serialize;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Response structure for a WHEP stream.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct WhepStreamInfo {
    /// Unique identifier for the WHEP endpoint
    pub endpoint_id: String,
    /// Stream mode (e.g., "video", "audio", "video+audio")
    pub mode: String,
    /// Whether the stream includes audio
    pub has_audio: bool,
    /// Whether the stream includes video
    pub has_video: bool,
}

/// Response structure for the streams list endpoint.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct WhepStreamsResponse {
    /// List of active WHEP streams
    pub streams: Vec<WhepStreamInfo>,
}

/// Response structure for ICE servers endpoint.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct IceServersResponse {
    /// List of ICE server configurations (STUN/TURN)
    pub ice_servers: Vec<IceServer>,
    /// ICE transport policy ("all" or "relay")
    pub ice_transport_policy: String,
}

/// ICE server configuration for WebRTC.
/// For TURN servers, username and credential are extracted from the URL.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct IceServer {
    /// ICE server URL (e.g., "stun:stun.l.google.com:19302")
    pub urls: String,
    /// Username for TURN server authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Credential for TURN server authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}
