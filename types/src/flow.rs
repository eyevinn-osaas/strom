//! Flow (pipeline) definitions.

use crate::block::BlockInstance;
use crate::element::{Element, Link};
use crate::state::PipelineState;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Unique identifier for a flow.
pub type FlowId = Uuid;

/// Thread priority level for GStreamer streaming threads.
///
/// Controls the scheduling priority of GStreamer's internal streaming threads.
/// Higher priorities help ensure smooth media processing under system load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThreadPriority {
    /// Normal priority (default OS scheduling, no elevation)
    Normal,
    /// Elevated priority (nice -10 equivalent, good for most use cases)
    #[default]
    High,
    /// Real-time priority (SCHED_FIFO, requires privileges)
    /// Warning: May require root or CAP_SYS_NICE capability
    Realtime,
}

impl ThreadPriority {
    /// Get the human-readable description of this priority level.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Normal => "Normal - default OS scheduling",
            Self::High => "High - elevated priority (recommended)",
            Self::Realtime => "Realtime - SCHED_FIFO (requires privileges)",
        }
    }

    /// Get all available priority levels.
    pub fn all() -> &'static [ThreadPriority] {
        &[Self::Normal, Self::High, Self::Realtime]
    }
}

/// Status of thread priority configuration for a running pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ThreadPriorityStatus {
    /// The requested thread priority level
    pub requested: ThreadPriority,
    /// Whether the requested priority was successfully applied
    pub achieved: bool,
    /// Error message if priority could not be set (empty if achieved)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Number of threads that had priority set
    pub threads_configured: u32,
}

/// GStreamer clock type selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum GStreamerClockType {
    /// System monotonic clock (default, recommended for most use cases)
    #[default]
    Monotonic,
    /// System realtime clock (wall clock time)
    Realtime,
    /// Let GStreamer choose the default clock for the pipeline
    PipelineDefault,
    /// Precision Time Protocol clock (for synchronized multi-device scenarios)
    Ptp,
    /// Network Time Protocol clock
    Ntp,
}

/// Clock synchronization status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ClockSyncStatus {
    /// Clock is synchronized
    Synced,
    /// Clock is not synchronized
    NotSynced,
    /// Synchronization status unknown or not applicable
    Unknown,
}

impl GStreamerClockType {
    /// Get the human-readable description of this clock type.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Monotonic => {
                "Monotonic (recommended) - stable system clock, not affected by time changes"
            }
            Self::Realtime => "Realtime - wall clock time, may jump if system time changes",
            Self::PipelineDefault => "Pipeline Default - let GStreamer choose automatically",
            Self::Ptp => "PTP - Precision Time Protocol for synchronized multi-device setups",
            Self::Ntp => "NTP - Network Time Protocol",
        }
    }
}

/// Flow configuration properties.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct FlowProperties {
    /// Human-readable description (multiline text)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// GStreamer clock type to use for this flow
    #[serde(default)]
    pub clock_type: GStreamerClockType,
    /// PTP domain (0-255, only used when clock_type is PTP)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ptp_domain: Option<u8>,
    /// NTP server address (hostname or IP, only used when clock_type is NTP)
    /// If not set but clock_type is NTP, will signal as "ntp=/traceable/"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ntp_server: Option<String>,
    /// Clock synchronization status (updated by backend for running pipelines)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock_sync_status: Option<ClockSyncStatus>,

    /// Thread priority for GStreamer streaming threads
    /// Default is High (elevated but not realtime)
    #[serde(default)]
    pub thread_priority: ThreadPriority,

    /// Status of thread priority configuration (updated by backend when pipeline starts)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_priority_status: Option<ThreadPriorityStatus>,

    /// Whether this flow should be automatically restarted when the backend starts
    /// (set to true when starting a flow, false when manually stopping it)
    #[serde(default)]
    pub auto_restart: bool,
}

/// A complete GStreamer pipeline definition.
///
/// A flow represents a named, configured GStreamer pipeline that can be
/// started, stopped, and persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct Flow {
    /// Unique identifier for this flow
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = Uuid))]
    pub id: FlowId,
    /// Human-readable name
    pub name: String,
    /// Elements in this flow
    #[serde(default)]
    pub elements: Vec<Element>,
    /// Block instances in this flow
    #[serde(default)]
    pub blocks: Vec<BlockInstance>,
    /// Links between element pads and/or block external pads
    #[serde(default)]
    pub links: Vec<Link>,
    /// Current runtime state (persisted to storage for automatic restart)
    #[serde(default)]
    pub state: Option<PipelineState>,
    /// Flow configuration properties
    #[serde(default)]
    pub properties: FlowProperties,
}

impl Flow {
    /// Create a new empty flow with a generated ID.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            elements: Vec::new(),
            blocks: Vec::new(),
            links: Vec::new(),
            state: Some(PipelineState::Null),
            properties: FlowProperties::default(),
        }
    }

    /// Create a new flow with a specific ID (useful for loading from storage).
    pub fn with_id(id: FlowId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            elements: Vec::new(),
            blocks: Vec::new(),
            links: Vec::new(),
            state: Some(PipelineState::Null),
            properties: FlowProperties::default(),
        }
    }
}
