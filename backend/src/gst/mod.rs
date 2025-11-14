//! GStreamer integration.

mod block_expansion;
pub mod discovery;
pub mod pipeline;

pub use discovery::ElementDiscovery;
pub use pipeline::{PipelineError, PipelineManager};
