//! WHIP API types shared between backend and frontend.

use serde::Deserialize;

/// Default minimum video bitrate hint (kbps) for WHIP ingest.
/// Sent to the browser via x-google-min-bitrate in the SDP answer.
pub const DEFAULT_MIN_VIDEO_BITRATE_KBPS: u32 = 1000;

/// Default start video bitrate hint (kbps) for WHIP ingest.
/// Sent to the browser via x-google-start-bitrate in the SDP answer.
pub const DEFAULT_START_VIDEO_BITRATE_KBPS: u32 = 2000;

/// Default maximum video bitrate hint (kbps) for WHIP ingest.
/// Sent to the browser via x-google-max-bitrate in the SDP answer.
pub const DEFAULT_MAX_VIDEO_BITRATE_KBPS: u32 = 4000;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// A client-side log entry sent from the WHIP ingest page.
#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ClientLogEntry {
    pub msg: String,
    pub level: Option<String>,
}
