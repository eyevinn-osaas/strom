//! GStreamer element and property definitions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Unique identifier for an element instance within a flow.
pub type ElementId = String;

/// Represents a GStreamer element instance in a flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct Element {
    /// Unique identifier for this element instance
    pub id: ElementId,
    /// GStreamer element type (e.g., "videotestsrc", "x264enc", "filesink")
    pub element_type: String,
    /// Element properties as key-value pairs
    #[serde(default)]
    pub properties: HashMap<String, PropertyValue>,
    /// Pad properties (pad_name -> property_name -> value)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub pad_properties: HashMap<String, HashMap<String, PropertyValue>>,
    /// Optional display position in the visual editor (x, y)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<(f32, f32)>,
}

/// A link between two element pads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct Link {
    /// Source element and pad (format: "element_id" or "element_id:pad_name")
    pub from: String,
    /// Destination element and pad (format: "element_id" or "element_id:pad_name")
    pub to: String,
}

/// Property value that can be various types.
///
/// GStreamer properties can be strings, numbers, booleans, enums, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(untagged)]
pub enum PropertyValue {
    String(String),
    Int(i64),
    UInt(u64),
    Float(f64),
    Bool(bool),
}

impl From<String> for PropertyValue {
    fn from(s: String) -> Self {
        PropertyValue::String(s)
    }
}

impl From<&str> for PropertyValue {
    fn from(s: &str) -> Self {
        PropertyValue::String(s.to_string())
    }
}

impl From<i64> for PropertyValue {
    fn from(i: i64) -> Self {
        PropertyValue::Int(i)
    }
}

impl From<u64> for PropertyValue {
    fn from(u: u64) -> Self {
        PropertyValue::UInt(u)
    }
}

impl From<f64> for PropertyValue {
    fn from(f: f64) -> Self {
        PropertyValue::Float(f)
    }
}

impl From<bool> for PropertyValue {
    fn from(b: bool) -> Self {
        PropertyValue::Bool(b)
    }
}

/// Information about a GStreamer element type (for discovery/palette).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ElementInfo {
    /// Element type name
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Element category (e.g., "Source", "Filter", "Sink", "Codec")
    pub category: String,
    /// Available source pads
    pub src_pads: Vec<PadInfo>,
    /// Available sink pads
    pub sink_pads: Vec<PadInfo>,
    /// Available properties
    pub properties: Vec<PropertyInfo>,
}

/// Pad presence type (static, dynamic, or request).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum PadPresence {
    /// Always present (static pad)
    Always,
    /// Created at runtime based on stream (dynamic/sometimes pad)
    Sometimes,
    /// Created on request
    Request,
}

/// Media type classification for pads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum MediaType {
    /// Generic/mixed or unknown media (blue)
    Generic,
    /// Audio media (green)
    Audio,
    /// Video media (orange)
    Video,
}

/// Information about an element pad.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PadInfo {
    /// Pad name
    pub name: String,
    /// Pad capabilities (simplified)
    pub caps: String,
    /// Pad presence type
    pub presence: PadPresence,
    /// Media type classification
    pub media_type: MediaType,
    /// Properties available on this pad template
    #[serde(default)]
    pub properties: Vec<PropertyInfo>,
}

/// Information about an element property.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PropertyInfo {
    /// Property name
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Property type
    pub property_type: PropertyType,
    /// Default value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<PropertyValue>,
    /// Property is writable
    #[serde(default)]
    pub writable: bool,
    /// Property can only be set during construction
    #[serde(default)]
    pub construct_only: bool,
    /// Property can be changed in NULL state
    #[serde(default)]
    pub mutable_in_null: bool,
    /// Property can be changed in READY state
    #[serde(default)]
    pub mutable_in_ready: bool,
    /// Property can be changed in PAUSED state
    #[serde(default)]
    pub mutable_in_paused: bool,
    /// Property can be changed in PLAYING state (live editing!)
    #[serde(default)]
    pub mutable_in_playing: bool,
    /// Property can be controlled over time with GstController
    #[serde(default)]
    pub controllable: bool,
}

/// Types of properties that elements can have.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum PropertyType {
    String,
    Int { min: i64, max: i64 },
    UInt { min: u64, max: u64 },
    Float { min: f64, max: f64 },
    Bool,
    Enum { values: Vec<String> },
}
