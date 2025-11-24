//! AES67 audio-over-IP block builders.

use crate::blocks::{BlockBuildError, BlockBuildResult, BlockBuilder};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use std::io::Write;
use std::str::FromStr;
use strom_types::{block::*, PropertyValue, *};
use tracing::{debug, info, warn};

/// AES67 Input block builder.
pub struct AES67InputBuilder;

impl BlockBuilder for AES67InputBuilder {
    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        debug!("Building AES67 Input block instance: {}", instance_id);

        // Get SDP property
        let sdp_content = properties
            .get("SDP")
            .and_then(|v| {
                if let PropertyValue::String(s) = v {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .ok_or_else(|| BlockBuildError::InvalidProperty("SDP property required".to_string()))?;

        // Write SDP to temp file
        let sdp_file_path = write_temp_file(sdp_content)?;

        // Create elements with namespaced IDs
        let filesrc_id = format!("{}:filesrc", instance_id);
        let sdpdemux_id = format!("{}:sdpdemux", instance_id);

        let filesrc = gst::ElementFactory::make("filesrc")
            .name(&filesrc_id)
            .property("location", &sdp_file_path)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("filesrc: {}", e)))?;

        let sdpdemux = gst::ElementFactory::make("sdpdemux")
            .name(&sdpdemux_id)
            .property("rtcp-mode", 0i32) // Disable RTCP for AES67 input
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("sdpdemux: {}", e)))?;

        // Set up pad-added handler to log new SSRC/streams
        // This is important for debugging SSRC changes in AES67 streams
        let sdpdemux_id_for_handler = sdpdemux_id.clone();
        sdpdemux.connect_pad_added(move |element, new_pad| {
            let pad_name = new_pad.name();
            let caps = new_pad.current_caps();

            info!(
                "AES67 Input [{}]: New pad added: {}",
                sdpdemux_id_for_handler, pad_name
            );

            // Log caps information if available
            if let Some(caps) = caps {
                info!(
                    "AES67 Input [{}]: Pad {} caps: {}",
                    sdpdemux_id_for_handler, pad_name, caps
                );
            }

            // Try to extract SSRC from the pad name or caps
            // sdpdemux typically names pads like "src_0", "src_1" etc.
            // The SSRC might be available in the caps or via RTP info
            if let Some(caps) = new_pad.current_caps() {
                if let Some(structure) = caps.structure(0) {
                    // Check for payload type, clock-rate, encoding-name
                    if let Ok(pt) = structure.get::<i32>("payload") {
                        info!(
                            "AES67 Input [{}]: Pad {} payload type: {}",
                            sdpdemux_id_for_handler, pad_name, pt
                        );
                    }
                    if let Ok(clock_rate) = structure.get::<i32>("clock-rate") {
                        info!(
                            "AES67 Input [{}]: Pad {} clock-rate: {}",
                            sdpdemux_id_for_handler, pad_name, clock_rate
                        );
                    }
                    if let Ok(encoding_name) = structure.get::<&str>("encoding-name") {
                        info!(
                            "AES67 Input [{}]: Pad {} encoding: {}",
                            sdpdemux_id_for_handler, pad_name, encoding_name
                        );
                    }
                }
            }

            // Log element state for debugging
            let (_, current_state, _) = element.state(gst::ClockTime::ZERO);
            info!(
                "AES67 Input [{}]: Element state when pad added: {:?}",
                sdpdemux_id_for_handler, current_state
            );

            // Check if pad is already linked
            if new_pad.is_linked() {
                info!(
                    "AES67 Input [{}]: Pad {} is already linked",
                    sdpdemux_id_for_handler, pad_name
                );
            } else {
                warn!(
                    "AES67 Input [{}]: Pad {} is NOT linked - downstream needs to handle this!",
                    sdpdemux_id_for_handler, pad_name
                );
            }
        });

        Ok(BlockBuildResult {
            elements: vec![
                (filesrc_id.clone(), filesrc),
                (sdpdemux_id.clone(), sdpdemux),
            ],
            internal_links: vec![(
                format!("{}:src", filesrc_id),
                format!("{}:sink", sdpdemux_id),
            )],
            bus_watch_setup: None,
        })
    }
}

/// AES67 Output block builder.
pub struct AES67OutputBuilder;

impl BlockBuilder for AES67OutputBuilder {
    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        debug!("Building AES67 Output block instance: {}", instance_id);

        // Extract properties with defaults
        let bit_depth = properties
            .get("bit_depth")
            .and_then(|v| match v {
                PropertyValue::Int(i) => Some(*i),
                PropertyValue::String(s) => s.parse::<i64>().ok(),
                _ => None,
            })
            .unwrap_or(24);

        let sample_rate = properties
            .get("sample_rate")
            .and_then(|v| match v {
                PropertyValue::Int(i) => Some(*i),
                PropertyValue::String(s) => s.parse::<i64>().ok(),
                _ => None,
            })
            .unwrap_or(48000);

        let channels = properties
            .get("channels")
            .and_then(|v| {
                if let PropertyValue::Int(i) = v {
                    Some(*i)
                } else {
                    None
                }
            })
            .unwrap_or(2);

        let ptime_ms = properties
            .get("ptime")
            .and_then(|v| match v {
                PropertyValue::Float(f) => Some(*f),
                PropertyValue::String(s) => s.parse::<f64>().ok(),
                _ => None,
            })
            .unwrap_or(1.0);

        let host = properties
            .get("host")
            .and_then(|v| {
                if let PropertyValue::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "239.69.1.1".to_string());

        let port = properties
            .get("port")
            .and_then(|v| {
                if let PropertyValue::Int(i) = v {
                    Some(*i as i32)
                } else {
                    None
                }
            })
            .unwrap_or(5004);

        // Create namespaced element IDs
        let audioconvert_id = format!("{}:audioconvert", instance_id);
        let audioresample_id = format!("{}:audioresample", instance_id);
        let capsfilter_id = format!("{}:capsfilter", instance_id);
        let payloader_id = format!("{}:payloader", instance_id);
        let udpsink_id = format!("{}:udpsink", instance_id);

        // Create elements
        let audioconvert = gst::ElementFactory::make("audioconvert")
            .name(&audioconvert_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("audioconvert: {}", e)))?;

        let audioresample = gst::ElementFactory::make("audioresample")
            .name(&audioresample_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("audioresample: {}", e)))?;

        // Build caps string
        let caps_str = format!("audio/x-raw,channels={},rate={}", channels, sample_rate);
        let caps = gst::Caps::from_str(&caps_str)
            .map_err(|_| BlockBuildError::InvalidProperty(format!("Invalid caps: {}", caps_str)))?;

        let capsfilter = gst::ElementFactory::make("capsfilter")
            .name(&capsfilter_id)
            .property("caps", &caps)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("capsfilter: {}", e)))?;

        // Select payloader based on bit depth
        let payloader_type = match bit_depth {
            16 => "rtpL16pay",
            24 => "rtpL24pay",
            _ => {
                return Err(BlockBuildError::InvalidConfiguration(format!(
                    "Unsupported bit depth: {}. Must be 16 or 24.",
                    bit_depth
                )))
            }
        };

        // Convert ptime from ms to ns
        let ptime_ns = (ptime_ms * 1_000_000.0) as i64;

        let payloader = gst::ElementFactory::make(payloader_type)
            .name(&payloader_id)
            .property("timestamp-offset", 0u32)
            .property("min-ptime", ptime_ns)
            .property("max-ptime", ptime_ns)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("{}: {}", payloader_type, e)))?;

        let udpsink = gst::ElementFactory::make("udpsink")
            .name(&udpsink_id)
            .property("host", &host)
            .property("port", port)
            .property("async", false)
            .property("sync", false)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("udpsink: {}", e)))?;

        // Define internal links
        let internal_links = vec![
            (
                format!("{}:src", audioconvert_id),
                format!("{}:sink", audioresample_id),
            ),
            (
                format!("{}:src", audioresample_id),
                format!("{}:sink", capsfilter_id),
            ),
            (
                format!("{}:src", capsfilter_id),
                format!("{}:sink", payloader_id),
            ),
            (
                format!("{}:src", payloader_id),
                format!("{}:sink", udpsink_id),
            ),
        ];

        Ok(BlockBuildResult {
            elements: vec![
                (audioconvert_id, audioconvert),
                (audioresample_id, audioresample),
                (capsfilter_id, capsfilter),
                (payloader_id, payloader),
                (udpsink_id, udpsink),
            ],
            internal_links,
            bus_watch_setup: None,
        })
    }
}

/// Get metadata for AES67 blocks (for UI/API).
pub fn get_blocks() -> Vec<BlockDefinition> {
    vec![aes67_input_definition(), aes67_output_definition()]
}

/// Get AES67 Input block definition (metadata only).
fn aes67_input_definition() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.aes67_input".to_string(),
        name: "AES67 Input".to_string(),
        description: "Receive AES67 audio stream via RTP using SDP description".to_string(),
        category: "Inputs".to_string(),
        exposed_properties: vec![ExposedProperty {
            name: "SDP".to_string(),
            description: "SDP text describing the AES67 stream (paste SDP content here)"
                .to_string(),
            property_type: PropertyType::Multiline,
            default_value: None,
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: "SDP".to_string(),
                transform: None,
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

/// Get AES67 Output block definition (metadata only).
fn aes67_output_definition() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.aes67_output".to_string(),
        name: "AES67 Output".to_string(),
        description: "Send AES67 audio stream via RTP with configurable format".to_string(),
        category: "Outputs".to_string(),
        exposed_properties: vec![
            ExposedProperty {
                name: "bit_depth".to_string(),
                description: "Audio bit depth".to_string(),
                property_type: PropertyType::Enum {
                    values: vec!["16".to_string(), "24".to_string()],
                },
                default_value: Some(PropertyValue::String("24".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "bit_depth".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "sample_rate".to_string(),
                description: "Sample rate in Hz".to_string(),
                property_type: PropertyType::Enum {
                    values: vec![
                        "32000".to_string(),
                        "44100".to_string(),
                        "48000".to_string(),
                        "88200".to_string(),
                        "96000".to_string(),
                        "176400".to_string(),
                        "192000".to_string(),
                    ],
                },
                default_value: Some(PropertyValue::String("48000".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "sample_rate".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "channels".to_string(),
                description: "Number of audio channels (1-8)".to_string(),
                property_type: PropertyType::Int,
                default_value: Some(PropertyValue::Int(2)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "channels".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "ptime".to_string(),
                description: "Packet time in milliseconds".to_string(),
                property_type: PropertyType::Enum {
                    values: vec![
                        "0.125".to_string(),
                        "0.25".to_string(),
                        "1.0".to_string(),
                        "4.0".to_string(),
                    ],
                },
                default_value: Some(PropertyValue::String("1.0".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "ptime".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "host".to_string(),
                description: "Destination IP address (multicast)".to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String("239.69.1.1".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
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
                    element_id: "_block".to_string(),
                    property_name: "port".to_string(),
                    transform: None,
                },
            },
        ],
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
            icon: Some("📡".to_string()),
            color: Some("#2196F3".to_string()),
            width: Some(2.5),
            height: Some(2.0),
        }),
    }
}

/// Write content to a temporary file and return its path.
fn write_temp_file(content: &str) -> Result<String, BlockBuildError> {
    use tempfile::NamedTempFile;

    let mut temp_file = NamedTempFile::new().map_err(|e| {
        BlockBuildError::InvalidConfiguration(format!("Failed to create temp file: {}", e))
    })?;

    temp_file.write_all(content.as_bytes()).map_err(|e| {
        BlockBuildError::InvalidConfiguration(format!("Failed to write temp file: {}", e))
    })?;

    temp_file.flush().map_err(|e| {
        BlockBuildError::InvalidConfiguration(format!("Failed to flush temp file: {}", e))
    })?;

    let (_file, path) = temp_file.keep().map_err(|e| {
        BlockBuildError::InvalidConfiguration(format!("Failed to keep temp file: {}", e))
    })?;

    let path_str = path.to_string_lossy().to_string();
    debug!("Created temp file for SDP: {}", path_str);

    Ok(path_str)
}
