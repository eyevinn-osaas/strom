//! Shared types for the Strom GStreamer flow engine.
//!
//! This crate contains domain models and API types shared between
//! the backend and frontend components.

pub mod api;
pub mod block;
pub mod element;
pub mod events;
pub mod flow;
pub mod state;

// Re-export commonly used types
pub use block::{
    BlockDefinition, BlockInstance, BlockListResponse, BlockResponse, CreateBlockRequest,
    ExposedProperty, ExternalPad, ExternalPads, PropertyMapping, PropertyType,
};
pub use element::{Element, ElementId, Link, MediaType, PropertyValue};
pub use events::StromEvent;
pub use flow::{Flow, FlowId};
pub use state::PipelineState;
