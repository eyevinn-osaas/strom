//! Discovery API types shared between backend and frontend.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

// ============================================================================
// Stream Discovery Types
// ============================================================================

/// API response for a discovered AES67 stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DiscoveredStreamResponse {
    pub id: String,
    pub name: String,
    pub source: String,
    pub multicast_address: String,
    pub port: u16,
    pub channels: u8,
    pub sample_rate: u32,
    pub encoding: String,
    pub origin_host: String,
    pub first_seen_secs_ago: u64,
    pub last_seen_secs_ago: u64,
    pub ttl_secs: u64,
    /// Network interface the stream was discovered on (for SAP).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub received_on_interface: Option<String>,
}

/// Response for announced streams list.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct AnnouncedStreamResponse {
    pub flow_id: String,
    pub block_id: String,
    pub origin_ip: String,
    pub sdp: String,
    /// Network interface the stream is announced on (None = all interfaces).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub announce_interface: Option<String>,
}

// ============================================================================
// Device Discovery Types
// ============================================================================

/// Device category for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum DeviceCategory {
    /// Audio input devices (microphones, line-in).
    AudioSource,
    /// Audio output devices (speakers, headphones).
    AudioSink,
    /// Video input devices (cameras, capture cards).
    VideoSource,
    /// Network sources (NDI, etc.).
    NetworkSource,
    /// Other/unknown device types.
    Other,
}

impl DeviceCategory {
    /// Parse device category from GStreamer device class string.
    pub fn from_device_class(class: &str) -> Self {
        match class {
            "Audio/Source" => Self::AudioSource,
            "Audio/Sink" => Self::AudioSink,
            "Video/Source" => Self::VideoSource,
            "Source/Network" => Self::NetworkSource,
            _ => Self::Other,
        }
    }

    /// Get GStreamer device class filter string.
    pub fn to_filter_string(&self) -> Option<&'static str> {
        match self {
            Self::AudioSource => Some("Audio/Source"),
            Self::AudioSink => Some("Audio/Sink"),
            Self::VideoSource => Some("Video/Source"),
            Self::NetworkSource => Some("Source/Network"),
            Self::Other => None,
        }
    }
}

/// API response for a discovered device.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DeviceResponse {
    /// Unique ID for this device.
    pub id: String,
    /// Display name of the device.
    pub name: String,
    /// Device class (e.g., "Audio/Source", "Video/Source", "Source/Network").
    pub device_class: String,
    /// Device category.
    pub category: DeviceCategory,
    /// Provider that discovered this device.
    pub provider: String,
    /// Additional properties from the device.
    pub properties: HashMap<String, String>,
    /// Seconds since first discovery.
    pub first_seen_secs_ago: u64,
    /// Seconds since last seen.
    pub last_seen_secs_ago: u64,
}

/// Device discovery status response.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DeviceDiscoveryStatus {
    /// Whether device discovery is running.
    pub running: bool,
    /// Whether NDI device provider is available.
    pub ndi_available: bool,
    /// Total number of discovered devices.
    pub device_count: usize,
    /// Number of devices by category.
    pub by_category: DeviceCountByCategory,
}

/// Device counts by category.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DeviceCountByCategory {
    pub audio_source: usize,
    pub audio_sink: usize,
    pub video_source: usize,
    pub network_source: usize,
    pub other: usize,
}

/// NDI discovery status response.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct NdiDiscoveryStatus {
    /// Whether NDI discovery is available (plugin installed).
    pub available: bool,
    /// Number of discovered NDI sources.
    pub source_count: usize,
}
