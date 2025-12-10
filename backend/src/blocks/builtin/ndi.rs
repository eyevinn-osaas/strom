//! NDI (Network Device Interface) video and audio input/output block builders.
//!
//! Provides separate video and audio input/output blocks for NDI streaming.
//! Uses the gst-plugin-ndi GStreamer plugin for NDI integration.
//! Requires the NDI SDK from NewTek/Vizrt to be installed.

use crate::blocks::{BlockBuildError, BlockBuildResult, BlockBuilder};
use gstreamer as gst;
use std::collections::HashMap;
use strom_types::{block::*, element::ElementPadRef, EnumValue, PropertyValue, *};
use tracing::info;

// NDI Input defaults
const NDI_INPUT_DEFAULT_TIMEOUT_MS: u32 = 5000;
const NDI_INPUT_DEFAULT_CONNECT_TIMEOUT_MS: u32 = 10000;

/// NDI bandwidth modes
fn bandwidth_enum_values() -> Vec<EnumValue> {
    vec![
        EnumValue {
            value: "100".to_string(),
            label: Some("Highest".to_string()),
        },
        EnumValue {
            value: "10".to_string(),
            label: Some("Audio Only".to_string()),
        },
        EnumValue {
            value: "-10".to_string(),
            label: Some("Metadata Only".to_string()),
        },
    ]
}

/// NDI Video Input block builder.
pub struct NDIVideoInputBuilder;

impl BlockBuilder for NDIVideoInputBuilder {
    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        info!("Building NDI Video Input block: {}", instance_id);

        // Parse properties
        let ndi_name = properties
            .get("ndi_name")
            .and_then(|v| match v {
                PropertyValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let url_address = properties
            .get("url_address")
            .and_then(|v| match v {
                PropertyValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let bandwidth = properties
            .get("bandwidth")
            .and_then(|v| match v {
                PropertyValue::String(s) => s.parse::<i32>().ok(),
                PropertyValue::Int(i) => Some(*i as i32),
                _ => None,
            })
            .unwrap_or(100); // Default: highest

        let timeout_ms = properties
            .get("timeout_ms")
            .and_then(|v| match v {
                PropertyValue::Int(i) => Some(*i as u32),
                PropertyValue::UInt(u) => Some(*u as u32),
                _ => None,
            })
            .unwrap_or(NDI_INPUT_DEFAULT_TIMEOUT_MS);

        let connect_timeout_ms = properties
            .get("connect_timeout_ms")
            .and_then(|v| match v {
                PropertyValue::Int(i) => Some(*i as u32),
                PropertyValue::UInt(u) => Some(*u as u32),
                _ => None,
            })
            .unwrap_or(NDI_INPUT_DEFAULT_CONNECT_TIMEOUT_MS);

        // Create elements with namespaced IDs
        let ndisrc_id = format!("{}:ndisrc", instance_id);
        let videoconvert_id = format!("{}:videoconvert", instance_id);
        let capsfilter_id = format!("{}:capsfilter", instance_id);

        let mut ndisrc_builder = gst::ElementFactory::make("ndisrc")
            .name(&ndisrc_id)
            .property("bandwidth", bandwidth)
            .property("timeout", timeout_ms)
            .property("connect-timeout", connect_timeout_ms);

        // Set ndi-name or url-address (one must be provided)
        if !ndi_name.is_empty() {
            ndisrc_builder = ndisrc_builder.property("ndi-name", &ndi_name);
        }
        if !url_address.is_empty() {
            ndisrc_builder = ndisrc_builder.property("url-address", &url_address);
        }

        let ndisrc = ndisrc_builder
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("ndisrc: {}", e)))?;

        let videoconvert = gst::ElementFactory::make("videoconvert")
            .name(&videoconvert_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("videoconvert: {}", e)))?;

        // Capsfilter with generic video/x-raw caps
        let caps = gst::Caps::builder("video/x-raw").build();
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .name(&capsfilter_id)
            .property("caps", &caps)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("capsfilter: {}", e)))?;

        info!(
            "NDI Video Input configured: ndi_name={}, url_address={}, bandwidth={}, timeout={}ms",
            ndi_name, url_address, bandwidth, timeout_ms
        );

        // Chain: ndisrc -> videoconvert -> capsfilter
        let internal_links = vec![
            (
                ElementPadRef::pad(&ndisrc_id, "video"),
                ElementPadRef::pad(&videoconvert_id, "sink"),
            ),
            (
                ElementPadRef::pad(&videoconvert_id, "src"),
                ElementPadRef::pad(&capsfilter_id, "sink"),
            ),
        ];

        Ok(BlockBuildResult {
            elements: vec![
                (ndisrc_id, ndisrc),
                (videoconvert_id, videoconvert),
                (capsfilter_id, capsfilter),
            ],
            internal_links,
            bus_message_handler: None,
            pad_properties: HashMap::new(),
        })
    }
}

/// NDI Audio Input block builder.
pub struct NDIAudioInputBuilder;

impl BlockBuilder for NDIAudioInputBuilder {
    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        info!("Building NDI Audio Input block: {}", instance_id);

        // Parse properties
        let ndi_name = properties
            .get("ndi_name")
            .and_then(|v| match v {
                PropertyValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let url_address = properties
            .get("url_address")
            .and_then(|v| match v {
                PropertyValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let bandwidth = properties
            .get("bandwidth")
            .and_then(|v| match v {
                PropertyValue::String(s) => s.parse::<i32>().ok(),
                PropertyValue::Int(i) => Some(*i as i32),
                _ => None,
            })
            .unwrap_or(10); // Default: audio-only for audio input

        let timeout_ms = properties
            .get("timeout_ms")
            .and_then(|v| match v {
                PropertyValue::Int(i) => Some(*i as u32),
                PropertyValue::UInt(u) => Some(*u as u32),
                _ => None,
            })
            .unwrap_or(NDI_INPUT_DEFAULT_TIMEOUT_MS);

        let connect_timeout_ms = properties
            .get("connect_timeout_ms")
            .and_then(|v| match v {
                PropertyValue::Int(i) => Some(*i as u32),
                PropertyValue::UInt(u) => Some(*u as u32),
                _ => None,
            })
            .unwrap_or(NDI_INPUT_DEFAULT_CONNECT_TIMEOUT_MS);

        // Create elements with namespaced IDs
        let ndisrc_id = format!("{}:ndisrc", instance_id);
        let audioconvert_id = format!("{}:audioconvert", instance_id);
        let audioresample_id = format!("{}:audioresample", instance_id);
        let capsfilter_id = format!("{}:capsfilter", instance_id);

        let mut ndisrc_builder = gst::ElementFactory::make("ndisrc")
            .name(&ndisrc_id)
            .property("bandwidth", bandwidth)
            .property("timeout", timeout_ms)
            .property("connect-timeout", connect_timeout_ms);

        // Set ndi-name or url-address (one must be provided)
        if !ndi_name.is_empty() {
            ndisrc_builder = ndisrc_builder.property("ndi-name", &ndi_name);
        }
        if !url_address.is_empty() {
            ndisrc_builder = ndisrc_builder.property("url-address", &url_address);
        }

        let ndisrc = ndisrc_builder
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("ndisrc: {}", e)))?;

        let audioconvert = gst::ElementFactory::make("audioconvert")
            .name(&audioconvert_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("audioconvert: {}", e)))?;

        let audioresample = gst::ElementFactory::make("audioresample")
            .name(&audioresample_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("audioresample: {}", e)))?;

        // Capsfilter with generic audio/x-raw caps
        let caps = gst::Caps::builder("audio/x-raw").build();
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .name(&capsfilter_id)
            .property("caps", &caps)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("capsfilter: {}", e)))?;

        info!(
            "NDI Audio Input configured: ndi_name={}, url_address={}, bandwidth={}, timeout={}ms",
            ndi_name, url_address, bandwidth, timeout_ms
        );

        // Chain: ndisrc -> audioconvert -> audioresample -> capsfilter
        let internal_links = vec![
            (
                ElementPadRef::pad(&ndisrc_id, "audio"),
                ElementPadRef::pad(&audioconvert_id, "sink"),
            ),
            (
                ElementPadRef::pad(&audioconvert_id, "src"),
                ElementPadRef::pad(&audioresample_id, "sink"),
            ),
            (
                ElementPadRef::pad(&audioresample_id, "src"),
                ElementPadRef::pad(&capsfilter_id, "sink"),
            ),
        ];

        Ok(BlockBuildResult {
            elements: vec![
                (ndisrc_id, ndisrc),
                (audioconvert_id, audioconvert),
                (audioresample_id, audioresample),
                (capsfilter_id, capsfilter),
            ],
            internal_links,
            bus_message_handler: None,
            pad_properties: HashMap::new(),
        })
    }
}

/// NDI Video Output block builder.
pub struct NDIVideoOutputBuilder;

impl BlockBuilder for NDIVideoOutputBuilder {
    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        info!("Building NDI Video Output block: {}", instance_id);

        // Parse properties
        let ndi_name = properties
            .get("ndi_name")
            .and_then(|v| match v {
                PropertyValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "Strom Video".to_string());

        // Create elements with namespaced IDs
        let videoconvert_id = format!("{}:videoconvert", instance_id);
        let ndisink_id = format!("{}:ndisink", instance_id);

        let videoconvert = gst::ElementFactory::make("videoconvert")
            .name(&videoconvert_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("videoconvert: {}", e)))?;

        let ndisink = gst::ElementFactory::make("ndisink")
            .name(&ndisink_id)
            .property("ndi-name", &ndi_name)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("ndisink: {}", e)))?;

        info!("NDI Video Output configured: ndi_name={}", ndi_name);

        // Chain: videoconvert -> ndisink
        let internal_links = vec![(
            ElementPadRef::pad(&videoconvert_id, "src"),
            ElementPadRef::pad(&ndisink_id, "video"),
        )];

        Ok(BlockBuildResult {
            elements: vec![(videoconvert_id, videoconvert), (ndisink_id, ndisink)],
            internal_links,
            bus_message_handler: None,
            pad_properties: HashMap::new(),
        })
    }
}

/// NDI Audio Output block builder.
pub struct NDIAudioOutputBuilder;

impl BlockBuilder for NDIAudioOutputBuilder {
    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        info!("Building NDI Audio Output block: {}", instance_id);

        // Parse properties
        let ndi_name = properties
            .get("ndi_name")
            .and_then(|v| match v {
                PropertyValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "Strom Audio".to_string());

        // Create elements with namespaced IDs
        let audioconvert_id = format!("{}:audioconvert", instance_id);
        let audioresample_id = format!("{}:audioresample", instance_id);
        let ndisink_id = format!("{}:ndisink", instance_id);

        let audioconvert = gst::ElementFactory::make("audioconvert")
            .name(&audioconvert_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("audioconvert: {}", e)))?;

        let audioresample = gst::ElementFactory::make("audioresample")
            .name(&audioresample_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("audioresample: {}", e)))?;

        let ndisink = gst::ElementFactory::make("ndisink")
            .name(&ndisink_id)
            .property("ndi-name", &ndi_name)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("ndisink: {}", e)))?;

        info!("NDI Audio Output configured: ndi_name={}", ndi_name);

        // Chain: audioconvert -> audioresample -> ndisink
        let internal_links = vec![
            (
                ElementPadRef::pad(&audioconvert_id, "src"),
                ElementPadRef::pad(&audioresample_id, "sink"),
            ),
            (
                ElementPadRef::pad(&audioresample_id, "src"),
                ElementPadRef::pad(&ndisink_id, "audio"),
            ),
        ];

        Ok(BlockBuildResult {
            elements: vec![
                (audioconvert_id, audioconvert),
                (audioresample_id, audioresample),
                (ndisink_id, ndisink),
            ],
            internal_links,
            bus_message_handler: None,
            pad_properties: HashMap::new(),
        })
    }
}

/// Get metadata for NDI blocks (for UI/API).
pub fn get_blocks() -> Vec<BlockDefinition> {
    vec![
        ndi_video_input_definition(),
        ndi_audio_input_definition(),
        ndi_video_output_definition(),
        ndi_audio_output_definition(),
    ]
}

/// Get NDI Video Input block definition.
fn ndi_video_input_definition() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.ndi_video_input".to_string(),
        name: "NDI Video Input".to_string(),
        description: "Receives video from an NDI source over the network. Requires NDI SDK.".to_string(),
        category: "Inputs".to_string(),
        exposed_properties: vec![
            ExposedProperty {
                name: "ndi_name".to_string(),
                label: "NDI Source Name".to_string(),
                description: "NDI source name (e.g., 'HOSTNAME (Source Name)'). Use NDI discovery to find sources.".to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String(String::new())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "ndi_name".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "url_address".to_string(),
                label: "URL Address".to_string(),
                description: "Alternative to NDI name: direct URL/address:port of the sender".to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String(String::new())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "url_address".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "bandwidth".to_string(),
                label: "Bandwidth".to_string(),
                description: "Bandwidth mode for receiving".to_string(),
                property_type: PropertyType::Enum {
                    values: bandwidth_enum_values(),
                },
                default_value: Some(PropertyValue::String("100".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "bandwidth".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "timeout_ms".to_string(),
                label: "Receive Timeout (ms)".to_string(),
                description: "Receive timeout in milliseconds".to_string(),
                property_type: PropertyType::Int,
                default_value: Some(PropertyValue::Int(NDI_INPUT_DEFAULT_TIMEOUT_MS as i64)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "timeout_ms".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "connect_timeout_ms".to_string(),
                label: "Connect Timeout (ms)".to_string(),
                description: "Connection timeout in milliseconds".to_string(),
                property_type: PropertyType::Int,
                default_value: Some(PropertyValue::Int(NDI_INPUT_DEFAULT_CONNECT_TIMEOUT_MS as i64)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "connect_timeout_ms".to_string(),
                    transform: None,
                },
            },
        ],
        external_pads: ExternalPads {
            inputs: vec![],
            outputs: vec![ExternalPad {
                name: "video_out".to_string(),
                media_type: MediaType::Video,
                internal_element_id: "capsfilter".to_string(),
                internal_pad_name: "src".to_string(),
            }],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("📹".to_string()),
            width: Some(2.0),
            height: Some(1.5),
            ..Default::default()
        }),
    }
}

/// Get NDI Audio Input block definition.
fn ndi_audio_input_definition() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.ndi_audio_input".to_string(),
        name: "NDI Audio Input".to_string(),
        description: "Receives audio from an NDI source over the network. Requires NDI SDK.".to_string(),
        category: "Inputs".to_string(),
        exposed_properties: vec![
            ExposedProperty {
                name: "ndi_name".to_string(),
                label: "NDI Source Name".to_string(),
                description: "NDI source name (e.g., 'HOSTNAME (Source Name)'). Use NDI discovery to find sources.".to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String(String::new())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "ndi_name".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "url_address".to_string(),
                label: "URL Address".to_string(),
                description: "Alternative to NDI name: direct URL/address:port of the sender".to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String(String::new())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "url_address".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "bandwidth".to_string(),
                label: "Bandwidth".to_string(),
                description: "Bandwidth mode for receiving".to_string(),
                property_type: PropertyType::Enum {
                    values: bandwidth_enum_values(),
                },
                default_value: Some(PropertyValue::String("10".to_string())), // Audio-only default
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "bandwidth".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "timeout_ms".to_string(),
                label: "Receive Timeout (ms)".to_string(),
                description: "Receive timeout in milliseconds".to_string(),
                property_type: PropertyType::Int,
                default_value: Some(PropertyValue::Int(NDI_INPUT_DEFAULT_TIMEOUT_MS as i64)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "timeout_ms".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "connect_timeout_ms".to_string(),
                label: "Connect Timeout (ms)".to_string(),
                description: "Connection timeout in milliseconds".to_string(),
                property_type: PropertyType::Int,
                default_value: Some(PropertyValue::Int(NDI_INPUT_DEFAULT_CONNECT_TIMEOUT_MS as i64)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "connect_timeout_ms".to_string(),
                    transform: None,
                },
            },
        ],
        external_pads: ExternalPads {
            inputs: vec![],
            outputs: vec![ExternalPad {
                name: "audio_out".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "capsfilter".to_string(),
                internal_pad_name: "src".to_string(),
            }],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("🎵".to_string()),
            width: Some(2.0),
            height: Some(1.5),
            ..Default::default()
        }),
    }
}

/// Get NDI Video Output block definition.
fn ndi_video_output_definition() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.ndi_video_output".to_string(),
        name: "NDI Video Output".to_string(),
        description: "Sends video to an NDI destination over the network. Requires NDI SDK."
            .to_string(),
        category: "Outputs".to_string(),
        exposed_properties: vec![ExposedProperty {
            name: "ndi_name".to_string(),
            label: "NDI Stream Name".to_string(),
            description:
                "The name this NDI stream will be published as (will be prefixed with hostname)"
                    .to_string(),
            property_type: PropertyType::String,
            default_value: Some(PropertyValue::String("Strom Video".to_string())),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: "ndi_name".to_string(),
                transform: None,
            },
        }],
        external_pads: ExternalPads {
            inputs: vec![ExternalPad {
                name: "video_in".to_string(),
                media_type: MediaType::Video,
                internal_element_id: "videoconvert".to_string(),
                internal_pad_name: "sink".to_string(),
            }],
            outputs: vec![],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("📺".to_string()),
            width: Some(2.0),
            height: Some(1.5),
            ..Default::default()
        }),
    }
}

/// Get NDI Audio Output block definition.
fn ndi_audio_output_definition() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.ndi_audio_output".to_string(),
        name: "NDI Audio Output".to_string(),
        description: "Sends audio to an NDI destination over the network. Requires NDI SDK."
            .to_string(),
        category: "Outputs".to_string(),
        exposed_properties: vec![ExposedProperty {
            name: "ndi_name".to_string(),
            label: "NDI Stream Name".to_string(),
            description:
                "The name this NDI stream will be published as (will be prefixed with hostname)"
                    .to_string(),
            property_type: PropertyType::String,
            default_value: Some(PropertyValue::String("Strom Audio".to_string())),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: "ndi_name".to_string(),
                transform: None,
            },
        }],
        external_pads: ExternalPads {
            inputs: vec![ExternalPad {
                name: "audio_in".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "audioconvert".to_string(),
                internal_pad_name: "sink".to_string(),
            }],
            outputs: vec![],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("🔊".to_string()),
            width: Some(2.0),
            height: Some(1.5),
            ..Default::default()
        }),
    }
}
