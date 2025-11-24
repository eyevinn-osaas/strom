//! GStreamer integration.

mod block_expansion;
pub mod discovery;
pub mod pipeline;
pub mod thread_priority;

pub use discovery::ElementDiscovery;
pub use pipeline::{PipelineError, PipelineManager};
pub use thread_priority::{setup_thread_priority_handler, ThreadPriorityState};
