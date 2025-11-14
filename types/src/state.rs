//! Pipeline and element state definitions.

use serde::{Deserialize, Serialize};

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// GStreamer pipeline state.
///
/// These states correspond to the GStreamer GST_STATE enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum PipelineState {
    /// No state has been set (initial state)
    #[default]
    Null,
    /// The pipeline is ready to go to PAUSED
    Ready,
    /// The pipeline is paused
    Paused,
    /// The pipeline is playing/running
    Playing,
}

impl std::fmt::Display for PipelineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Ready => write!(f, "READY"),
            Self::Paused => write!(f, "PAUSED"),
            Self::Playing => write!(f, "PLAYING"),
        }
    }
}
