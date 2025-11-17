//! Server-Sent Events for real-time updates across clients.

use crate::element::PropertyValue;
use crate::FlowId;
use serde::{Deserialize, Serialize};

/// Event types that can be broadcast to all connected clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum StromEvent {
    /// A flow was created
    FlowCreated { flow_id: FlowId },
    /// A flow was updated
    FlowUpdated { flow_id: FlowId },
    /// A flow was deleted
    FlowDeleted { flow_id: FlowId },
    /// A flow was started
    FlowStarted { flow_id: FlowId },
    /// A flow was stopped
    FlowStopped { flow_id: FlowId },
    /// A flow's state changed
    FlowStateChanged { flow_id: FlowId, state: String },
    /// Pipeline error occurred
    PipelineError {
        flow_id: FlowId,
        error: String,
        source: Option<String>,
    },
    /// Pipeline warning message
    PipelineWarning {
        flow_id: FlowId,
        warning: String,
        source: Option<String>,
    },
    /// Pipeline info message
    PipelineInfo {
        flow_id: FlowId,
        message: String,
        source: Option<String>,
    },
    /// Pipeline reached end of stream
    PipelineEos { flow_id: FlowId },
    /// Element property was changed on a running pipeline
    PropertyChanged {
        flow_id: FlowId,
        element_id: String,
        property_name: String,
        value: PropertyValue,
    },
    /// Pad property was changed on a running pipeline
    PadPropertyChanged {
        flow_id: FlowId,
        element_id: String,
        pad_name: String,
        property_name: String,
        value: PropertyValue,
    },
    /// Ping event to keep connection alive
    Ping,
}

impl StromEvent {
    /// Convert event to SSE format (data: <json>\n\n)
    pub fn to_sse_message(&self) -> String {
        match serde_json::to_string(self) {
            Ok(json) => format!("data: {}\n\n", json),
            Err(e) => {
                // Can't use tracing in types crate
                eprintln!("Failed to serialize event: {}", e);
                format!("data: {{\"type\":\"Error\",\"message\":\"{}\"}}\n\n", e)
            }
        }
    }

    /// Get a human-readable description of the event
    pub fn description(&self) -> String {
        match self {
            StromEvent::FlowCreated { flow_id } => format!("Flow {} created", flow_id),
            StromEvent::FlowUpdated { flow_id } => format!("Flow {} updated", flow_id),
            StromEvent::FlowDeleted { flow_id } => format!("Flow {} deleted", flow_id),
            StromEvent::FlowStarted { flow_id } => format!("Flow {} started", flow_id),
            StromEvent::FlowStopped { flow_id } => format!("Flow {} stopped", flow_id),
            StromEvent::FlowStateChanged { flow_id, state } => {
                format!("Flow {} state changed to {}", flow_id, state)
            }
            StromEvent::PipelineError {
                flow_id,
                error,
                source,
            } => {
                if let Some(src) = source {
                    format!("Pipeline error in flow {} from {}: {}", flow_id, src, error)
                } else {
                    format!("Pipeline error in flow {}: {}", flow_id, error)
                }
            }
            StromEvent::PipelineWarning {
                flow_id,
                warning,
                source,
            } => {
                if let Some(src) = source {
                    format!(
                        "Pipeline warning in flow {} from {}: {}",
                        flow_id, src, warning
                    )
                } else {
                    format!("Pipeline warning in flow {}: {}", flow_id, warning)
                }
            }
            StromEvent::PipelineInfo {
                flow_id,
                message,
                source,
            } => {
                if let Some(src) = source {
                    format!(
                        "Pipeline info in flow {} from {}: {}",
                        flow_id, src, message
                    )
                } else {
                    format!("Pipeline info in flow {}: {}", flow_id, message)
                }
            }
            StromEvent::PipelineEos { flow_id } => {
                format!("Pipeline {} reached end of stream", flow_id)
            }
            StromEvent::PropertyChanged {
                flow_id,
                element_id,
                property_name,
                value,
            } => {
                format!(
                    "Property {}.{} changed to {:?} in flow {}",
                    element_id, property_name, value, flow_id
                )
            }
            StromEvent::PadPropertyChanged {
                flow_id,
                element_id,
                pad_name,
                property_name,
                value,
            } => {
                format!(
                    "Pad property {}:{}:{} changed to {:?} in flow {}",
                    element_id, pad_name, property_name, value, flow_id
                )
            }
            StromEvent::Ping => "Ping".to_string(),
        }
    }
}
