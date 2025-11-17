//! AES67 audio-over-IP blocks.

use std::collections::HashMap;
use strom_types::{block::*, *};

/// Get all AES67-related blocks.
pub fn get_blocks() -> Vec<BlockDefinition> {
    vec![aes67_input(), aes67_output()]
}

/// AES67 Input block - receives AES67 audio via RTP using SDP data.
///
/// Note: The SDP text is written to a temporary file which is then read by filesrc.
/// This avoids the complexity of using appsrc while still not requiring the user
/// to manually create a file.
fn aes67_input() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.aes67_input".to_string(),
        name: "AES67 Input".to_string(),
        description: "Receive AES67 audio stream via RTP using SDP description".to_string(),
        category: "Inputs".to_string(),
        elements: vec![
            Element {
                id: "filesrc".to_string(),
                element_type: "filesrc".to_string(),
                properties: HashMap::new(),
                pad_properties: HashMap::new(),
                position: None,
            },
            Element {
                id: "sdpdemux".to_string(),
                element_type: "sdpdemux".to_string(),
                properties: HashMap::new(),
                pad_properties: HashMap::new(),
                position: None,
            },
        ],
        internal_links: vec![Link {
            from: "filesrc:src".to_string(),
            to: "sdpdemux:sink".to_string(),
        }],
        exposed_properties: vec![ExposedProperty {
            name: "SDP".to_string(),
            description: "SDP text describing the AES67 stream (paste SDP content here)"
                .to_string(),
            property_type: PropertyType::Multiline,
            default_value: None,
            mapping: PropertyMapping {
                element_id: "filesrc".to_string(),
                property_name: "location".to_string(),
                transform: Some("write_temp_file".to_string()), // Write SDP to temp file and set location
            },
        }],
        external_pads: ExternalPads {
            inputs: vec![],
            outputs: vec![ExternalPad {
                name: "audio_out".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "sdpdemux".to_string(),
                internal_pad_name: "src_0".to_string(),
            }],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("🎵".to_string()),
            color: Some("#4CAF50".to_string()),
            width: Some(2.0),
            height: Some(1.5),
        }),
    }
}

/// AES67 Output block - sends AES67 audio via RTP.
fn aes67_output() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.aes67_output".to_string(),
        name: "AES67 Output".to_string(),
        description: "Send AES67 audio stream via RTP".to_string(),
        category: "Outputs".to_string(),
        elements: vec![
            Element {
                id: "rtpL24pay".to_string(),
                element_type: "rtpL24pay".to_string(),
                properties: HashMap::from([(
                    "timestamp-offset".to_string(),
                    PropertyValue::UInt(0),
                )]),
                pad_properties: HashMap::new(),
                position: None,
            },
            Element {
                id: "udpsink".to_string(),
                element_type: "udpsink".to_string(),
                properties: HashMap::from([
                    ("async".to_string(), PropertyValue::Bool(false)),
                    ("sync".to_string(), PropertyValue::Bool(false)),
                ]),
                pad_properties: HashMap::new(),
                position: None,
            },
        ],
        internal_links: vec![Link {
            from: "rtpL24pay:src".to_string(),
            to: "udpsink:sink".to_string(),
        }],
        exposed_properties: vec![
            ExposedProperty {
                name: "host".to_string(),
                description: "Destination IP address".to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String("239.69.1.1".to_string())),
                mapping: PropertyMapping {
                    element_id: "udpsink".to_string(),
                    property_name: "host".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "port".to_string(),
                description: "Destination UDP port".to_string(),
                property_type: PropertyType::Int,
                default_value: Some(PropertyValue::Int(5004)),
                mapping: PropertyMapping {
                    element_id: "udpsink".to_string(),
                    property_name: "port".to_string(),
                    transform: None,
                },
            },
        ],
        external_pads: ExternalPads {
            inputs: vec![ExternalPad {
                name: "audio_in".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "rtpL24pay".to_string(),
                internal_pad_name: "sink".to_string(),
            }],
            outputs: vec![],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("📡".to_string()),
            color: Some("#2196F3".to_string()),
            width: Some(2.0),
            height: Some(1.5),
        }),
    }
}
