//! GStreamer integration.

pub mod discovery;
pub mod pipeline;

pub use discovery::ElementDiscovery;
pub use pipeline::{PipelineError, PipelineManager};
