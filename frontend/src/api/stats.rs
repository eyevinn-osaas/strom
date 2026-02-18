use serde::Deserialize;
use strom_types::FlowId;

use super::*;

/// Flow RTP statistics information (jitterbuffer stats from RTP-based blocks like AES67 Input)
#[derive(Debug, Clone, Deserialize)]
pub struct FlowRtpStatsInfo {
    /// The flow ID
    pub flow_id: FlowId,
    /// The flow name
    pub flow_name: String,
    /// RTP statistics for each block in the flow
    pub blocks: Vec<BlockRtpStatsInfo>,
    /// Timestamp when stats were collected (nanoseconds since UNIX epoch)
    pub collected_at: u64,
}

/// RTP statistics for a single block instance
#[derive(Debug, Clone, Deserialize)]
pub struct BlockRtpStatsInfo {
    /// The block instance ID
    pub block_instance_id: String,
    /// The block definition ID (e.g., "builtin.aes67_input")
    pub block_definition_id: String,
    /// Human-readable block name
    pub block_name: String,
    /// Collection of RTP statistics for this block (jitterbuffer metrics)
    pub stats: Vec<RtpStatisticInfo>,
    /// Timestamp when these stats were collected
    pub collected_at: u64,
}

/// A single RTP statistic with its value and metadata
#[derive(Debug, Clone, Deserialize)]
pub struct RtpStatisticInfo {
    /// Unique identifier for this statistic within the block
    pub id: String,
    /// Current value
    pub value: RtpStatValueInfo,
    /// Metadata about this statistic
    pub metadata: RtpStatMetadataInfo,
}

/// An RTP statistic value
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum RtpStatValueInfo {
    /// Counter - monotonically increasing value (e.g., packets received)
    Counter(u64),
    /// Gauge - value that can go up or down (e.g., buffer level)
    Gauge(i64),
    /// Float value (e.g., average jitter)
    Float(f64),
    /// Boolean flag (e.g., is_synced)
    Bool(bool),
    /// String value (e.g., current SSRC)
    String(String),
    /// Duration in nanoseconds
    DurationNs(u64),
    /// Timestamp in nanoseconds since epoch
    TimestampNs(u64),
}

/// Metadata about an RTP statistic
#[derive(Debug, Clone, Deserialize)]
pub struct RtpStatMetadataInfo {
    /// Human-readable name for display
    pub display_name: String,
    /// Description of what this statistic measures
    pub description: String,
    /// Unit of measurement (e.g., "packets", "ms", "bytes")
    pub unit: Option<String>,
    /// Category for grouping in UI (e.g., "RTP", "Buffer", "Network")
    pub category: Option<String>,
}

impl RtpStatValueInfo {
    /// Format the value for display
    pub fn format(&self) -> String {
        match self {
            RtpStatValueInfo::Counter(v) => format!("{}", v),
            RtpStatValueInfo::Gauge(v) => format!("{}", v),
            RtpStatValueInfo::Float(v) => format!("{:.2}", v),
            RtpStatValueInfo::Bool(v) => if *v { "Yes" } else { "No" }.to_string(),
            RtpStatValueInfo::String(v) => v.clone(),
            RtpStatValueInfo::DurationNs(v) => {
                if *v < 1_000 {
                    format!("{} ns", v)
                } else if *v < 1_000_000 {
                    format!("{:.2} us", *v as f64 / 1_000.0)
                } else if *v < 1_000_000_000 {
                    format!("{:.2} ms", *v as f64 / 1_000_000.0)
                } else {
                    format!("{:.2} s", *v as f64 / 1_000_000_000.0)
                }
            }
            RtpStatValueInfo::TimestampNs(v) => format!("{}", v),
        }
    }
}

impl ApiClient {
    /// Get WebRTC statistics from a running flow.
    pub async fn get_webrtc_stats(&self, id: FlowId) -> ApiResult<strom_types::api::WebRtcStats> {
        use strom_types::api::WebRtcStatsResponse;
        use tracing::trace;

        let url = format!("{}/flows/{}/webrtc-stats", self.base_url, id);
        trace!("Fetching WebRTC stats from: {}", url);

        let response = self
            .with_auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        let stats_response: WebRtcStatsResponse = response
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))?;

        trace!(
            "Successfully fetched WebRTC stats: {} connections",
            stats_response.stats.connections.len()
        );
        Ok(stats_response.stats)
    }

    /// Get RTP statistics for a running flow (jitterbuffer stats from AES67 Input blocks).
    pub async fn get_flow_rtp_stats(&self, id: FlowId) -> ApiResult<FlowRtpStatsInfo> {
        let url = format!("{}/flows/{}/rtp-stats", self.base_url, id);
        let response = self
            .with_auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        let rtp_stats_info: FlowRtpStatsInfo = response
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))?;

        Ok(rtp_stats_info)
    }
}
