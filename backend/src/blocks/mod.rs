//! Block management system for reusable element groupings.

pub mod builder;
pub mod builtin;
pub mod registry;
pub mod sdp;
pub mod storage;

pub use builder::{BlockBuildError, BlockBuildResult, BlockBuilder, BusMessageConnectFn};
pub use registry::BlockRegistry;
