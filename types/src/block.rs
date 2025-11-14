//! Block definitions and instances for reusable element groupings.

use crate::{Element, Link, MediaType, PropertyValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Property type enumeration for exposed properties
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum PropertyType {
    String,
    Multiline,
    Int,
    UInt,
    Float,
    Bool,
}

/// Block definition - template for creating block instances
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BlockDefinition {
    /// Unique identifier for this block definition
    pub id: String,

    /// Human-readable name (e.g., "AES67 Input")
    pub name: String,

    /// Description of what this block does
    pub description: String,

    /// Category for organization (e.g., "Inputs", "Outputs", "Codecs")
    pub category: String,

    /// Internal elements that make up this block
    pub elements: Vec<Element>,

    /// Internal links between elements within the block
    pub internal_links: Vec<Link>,

    /// Exposed properties that users can configure
    pub exposed_properties: Vec<ExposedProperty>,

    /// External pads exposed by this block
    pub external_pads: ExternalPads,

    /// Whether this is a built-in block (read-only) or user-defined
    pub built_in: bool,

    /// Visual representation settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_metadata: Option<BlockUIMetadata>,
}

/// Property exposed by a block to the outside
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExposedProperty {
    /// Name of the exposed property
    pub name: String,

    /// Description for users
    pub description: String,

    /// Type of property
    pub property_type: PropertyType,

    /// Default value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<PropertyValue>,

    /// Mapping to internal element property
    pub mapping: PropertyMapping,
}

/// Maps an exposed property to one or more internal element properties
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PropertyMapping {
    /// Which internal element's property to set
    pub element_id: String,

    /// Property name on that element
    pub property_name: String,

    /// Optional transformation (for future use)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
}

/// External pads that the block exposes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExternalPads {
    /// Input pads (mapped to internal element pads)
    pub inputs: Vec<ExternalPad>,

    /// Output pads (mapped to internal element pads)
    pub outputs: Vec<ExternalPad>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExternalPad {
    /// External name for this pad
    pub name: String,

    /// Media type (audio, video, generic)
    pub media_type: MediaType,

    /// Which internal element and pad this maps to
    pub internal_element_id: String,
    pub internal_pad_name: String,
}

/// Block instance in a flow
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BlockInstance {
    /// Unique ID for this instance
    pub id: String,

    /// Reference to the block definition
    pub block_definition_id: String,

    /// User-assigned name for this instance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Property values for this instance
    pub properties: HashMap<String, PropertyValue>,

    /// Position in the visual editor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

/// Position in the visual editor
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

/// UI metadata for block rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BlockUIMetadata {
    /// Icon or visual identifier (emoji or name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Color for visual distinction (hex color)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    /// Width in the editor (in grid units)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,

    /// Height in the editor (in grid units)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
}

/// Request to create a new block definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateBlockRequest {
    pub name: String,
    pub description: String,
    pub category: String,
    pub elements: Vec<Element>,
    pub internal_links: Vec<Link>,
    pub exposed_properties: Vec<ExposedProperty>,
    pub external_pads: ExternalPads,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_metadata: Option<BlockUIMetadata>,
}

/// Response containing a block definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BlockResponse {
    pub block: BlockDefinition,
}

/// Response containing a list of blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BlockListResponse {
    pub blocks: Vec<BlockDefinition>,
}

/// Response containing block categories
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BlockCategoriesResponse {
    pub categories: Vec<String>,
}
