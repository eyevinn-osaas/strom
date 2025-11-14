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
        }
    }
}
