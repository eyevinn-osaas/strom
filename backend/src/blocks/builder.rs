//! Block builder trait for runtime GStreamer element creation.

use gstreamer as gst;
use std::collections::HashMap;
use strom_types::PropertyValue;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BlockBuildError {
    #[error("GStreamer error: {0}")]
    GStreamer(#[from] gst::glib::Error),

    #[error("GStreamer boolean error: {0}")]
    BoolError(#[from] gst::glib::BoolError),

    #[error("Failed to create element: {0}")]
    ElementCreation(String),

    #[error("Failed to link elements: {0}")]
    LinkError(String),

    #[error("Invalid property: {0}")]
    InvalidProperty(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

/// Result of building a block - contains GStreamer elements with namespaced IDs and link specifications.
pub struct BlockBuildResult {
    /// GStreamer elements with their namespaced IDs (format: "block_instance_id:internal_element_id")
    pub elements: Vec<(String, gst::Element)>,

    /// Internal links between elements (using namespaced IDs)
    pub internal_links: Vec<(String, String)>, // (from_pad, to_pad)
}

/// Trait for building GStreamer elements from block instances.
///
/// Implementors create actual GStreamer elements at runtime based on block properties.
/// Elements are namespaced with the block instance ID to avoid conflicts.
pub trait BlockBuilder: Send + Sync {
    /// Build GStreamer elements for this block instance.
    ///
    /// # Arguments
    /// * `instance_id` - Unique ID for this block instance (used for namespacing)
    /// * `properties` - Property values from the block instance
    ///
    /// # Returns
    /// A vector of (element_id, gst::Element) tuples where element_id is already namespaced.
    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
    ) -> Result<BlockBuildResult, BlockBuildError>;
}
