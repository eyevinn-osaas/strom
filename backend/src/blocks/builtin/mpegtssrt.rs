//! MPEG-TS over SRT output block builder.
//!
//! This block muxes multiple video and audio streams into MPEG Transport Stream
//! and outputs via SRT (Secure Reliable Transport).
//!
//! Features:
//! - Automatic video parser selection (h264parse, h265parse, av1parse, vp9parse)
//! - Dynamic AAC encoding for raw audio inputs
//! - Fixed configuration: 1 video input + 8 audio inputs (all always available)
//! - Optimized for UDP streaming (alignment=7 on mpegtsmux)
//! - SRT with auto-reconnect and configurable latency
//!
//! Input handling:
//! - Video: Accepts encoded H.264/H.265/AV1/VP9 (adds appropriate parser)
//! - Audio: Accepts both raw audio (auto-encodes to AAC) or encoded AAC (adds parser)
//!
//! Pipeline structure:
//! ```text
//! Video (encoded) -> videoparse -> mpegtsmux -> srtsink
//! Audio (raw)     -> audioconvert -> audioresample -> avenc_aac -> aacparse -> mpegtsmux
//! Audio (encoded) -> aacparse -> mpegtsmux
//! ```

use crate::blocks::{BlockBuildError, BlockBuildResult, BlockBuilder};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use strom_types::{block::*, PropertyValue, *};
use tracing::info;

/// MPEG-TS/SRT Output block builder.
pub struct MpegTsSrtOutputBuilder;

impl BlockBuilder for MpegTsSrtOutputBuilder {
    fn get_external_pads(
        &self,
        properties: &HashMap<String, PropertyValue>,
    ) -> Option<ExternalPads> {
        // Get number of video and audio tracks from properties
        let num_video_tracks = properties
            .get("num_video_tracks")
            .and_then(|v| match v {
                PropertyValue::UInt(u) => Some(*u as usize),
                PropertyValue::Int(i) => Some(*i as usize),
                _ => None,
            })
            .unwrap_or(1);

        let num_audio_tracks = properties
            .get("num_audio_tracks")
            .and_then(|v| match v {
                PropertyValue::UInt(u) => Some(*u as usize),
                PropertyValue::Int(i) => Some(*i as usize),
                _ => None,
            })
            .unwrap_or(8);

        // Build dynamic input pads
        let mut inputs = Vec::new();

        // Add video inputs
        for i in 0..num_video_tracks {
            inputs.push(ExternalPad {
                name: if num_video_tracks == 1 {
                    "video_in".to_string()
                } else {
                    format!("video_in_{}", i)
                },
                media_type: MediaType::Video,
                internal_element_id: if num_video_tracks == 1 {
                    "video_input".to_string()
                } else {
                    format!("video_input_{}", i)
                },
                internal_pad_name: "sink".to_string(),
            });
        }

        // Add audio inputs
        for i in 0..num_audio_tracks {
            inputs.push(ExternalPad {
                name: format!("audio_in_{}", i),
                media_type: MediaType::Audio,
                internal_element_id: format!("audio_input_{}", i),
                internal_pad_name: "sink".to_string(),
            });
        }

        Some(ExternalPads {
            inputs,
            outputs: vec![], // No outputs
        })
    }

    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        info!(
            "📡 Building MPEG-TS/SRT Output block instance: {}",
            instance_id
        );

        // Get SRT URI (required)
        let srt_uri = properties
            .get("srt_uri")
            .and_then(|v| match v {
                PropertyValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                BlockBuildError::InvalidProperty("srt_uri property required".to_string())
            })?;

        // Get SRT latency (optional, default 125ms)
        let latency = properties
            .get("latency")
            .and_then(|v| match v {
                PropertyValue::UInt(u) => Some(*u as i32),
                PropertyValue::Int(i) => Some(*i as i32),
                _ => None,
            })
            .unwrap_or(125);

        // Get wait_for_connection (optional, default false per notes.txt)
        let wait_for_connection = properties
            .get("wait_for_connection")
            .and_then(|v| match v {
                PropertyValue::Bool(b) => Some(*b),
                _ => None,
            })
            .unwrap_or(false);

        // Get auto_reconnect (optional, default true per notes.txt)
        let auto_reconnect = properties
            .get("auto_reconnect")
            .and_then(|v| match v {
                PropertyValue::Bool(b) => Some(*b),
                _ => None,
            })
            .unwrap_or(true);

        // Create mpegtsmux with alignment=7 for UDP streaming
        let mux_id = format!("{}:mpegtsmux", instance_id);
        let mux = gst::ElementFactory::make("mpegtsmux")
            .name(&mux_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("mpegtsmux: {}", e)))?;

        // Set alignment=7 for UDP streaming (7 MPEG-TS packets = 1316 bytes)
        mux.set_property("alignment", 7i32);

        // Create srtsink
        let sink_id = format!("{}:srtsink", instance_id);
        let srtsink = gst::ElementFactory::make("srtsink")
            .name(&sink_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("srtsink: {}", e)))?;

        // Configure srtsink
        srtsink.set_property("uri", &srt_uri);
        srtsink.set_property("latency", latency);
        srtsink.set_property("wait-for-connection", wait_for_connection);
        srtsink.set_property("auto-reconnect", auto_reconnect);

        // Set async=false to prevent blocking pipeline state changes
        srtsink.set_property("async", false);

        // Set sync=true for clock-based streaming (drop frames to maintain real-time)
        srtsink.set_property("sync", true);

        info!(
            "📡 MPEG-TS/SRT configured: uri={}, latency={}ms, wait={}, auto-reconnect={}, async=false, sync=true",
            srt_uri, latency, wait_for_connection, auto_reconnect
        );

        // Get number of video and audio tracks from properties
        let num_video_tracks = properties
            .get("num_video_tracks")
            .and_then(|v| match v {
                PropertyValue::UInt(u) => Some(*u as usize),
                PropertyValue::Int(i) => Some(*i as usize),
                _ => None,
            })
            .unwrap_or(1);

        let num_audio_tracks = properties
            .get("num_audio_tracks")
            .and_then(|v| match v {
                PropertyValue::UInt(u) => Some(*u as usize),
                PropertyValue::Int(i) => Some(*i as usize),
                _ => None,
            })
            .unwrap_or(8);

        // Get video parser type (optional, default h264parse)
        let video_parser_type = properties
            .get("video_parser")
            .and_then(|v| match v {
                PropertyValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or("h264parse");

        let mut internal_links = vec![];
        let mut elements = vec![(mux_id.clone(), mux), (sink_id.clone(), srtsink)];

        let mut next_mux_pad = 0;

        // Create video input chain if requested: video_parser -> mpegtsmux
        if num_video_tracks > 0 {
            let video_input_id = format!("{}:video_input", instance_id);
            let video_parser = gst::ElementFactory::make(video_parser_type)
                .name(&video_input_id)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("{}: {}", video_parser_type, e))
                })?;

            // Set config-interval=-1 to insert SPS/PPS at regular intervals
            if video_parser.has_property("config-interval") {
                video_parser.set_property("config-interval", -1i32);
            }

            // Link video parser to mpegtsmux
            internal_links.push((
                format!("{}:src", video_input_id),
                format!("{}:sink_{}", mux_id, next_mux_pad),
            ));
            next_mux_pad += 1;

            elements.push((video_input_id.clone(), video_parser));
        }

        // Create audio input chains: audioconvert -> audioresample -> avenc_aac -> aacparse -> mpegtsmux
        // This chain handles both raw audio (encodes to AAC) and already-encoded AAC (passes through)
        for i in 0..num_audio_tracks {
            let audio_input_id = format!("{}:audio_input_{}", instance_id, i);
            let audioresample_id = format!("{}:audio_resample_{}", instance_id, i);
            let encoder_id = format!("{}:audio_encoder_{}", instance_id, i);
            let parser_id = format!("{}:audio_parser_{}", instance_id, i);

            // audioconvert is the entry point (audio_input_N)
            let audioconvert = gst::ElementFactory::make("audioconvert")
                .name(&audio_input_id)
                .build()
                .map_err(|e| BlockBuildError::ElementCreation(format!("audioconvert: {}", e)))?;

            let audioresample = gst::ElementFactory::make("audioresample")
                .name(&audioresample_id)
                .build()
                .map_err(|e| BlockBuildError::ElementCreation(format!("audioresample: {}", e)))?;

            let encoder = gst::ElementFactory::make("avenc_aac")
                .name(&encoder_id)
                .build()
                .map_err(|e| BlockBuildError::ElementCreation(format!("avenc_aac: {}", e)))?;

            let parser = gst::ElementFactory::make("aacparse")
                .name(&parser_id)
                .build()
                .map_err(|e| BlockBuildError::ElementCreation(format!("aacparse: {}", e)))?;

            // Chain: audioconvert -> audioresample -> avenc_aac -> aacparse -> mpegtsmux
            internal_links.push((
                format!("{}:src", audio_input_id),
                format!("{}:sink", audioresample_id),
            ));
            internal_links.push((
                format!("{}:src", audioresample_id),
                format!("{}:sink", encoder_id),
            ));
            internal_links.push((format!("{}:src", encoder_id), format!("{}:sink", parser_id)));
            // Link parser to mpegtsmux request pad
            internal_links.push((
                format!("{}:src", parser_id),
                format!("{}:sink_{}", mux_id, next_mux_pad),
            ));
            next_mux_pad += 1;

            elements.push((audio_input_id, audioconvert));
            elements.push((audioresample_id, audioresample));
            elements.push((encoder_id, encoder));
            elements.push((parser_id, parser));
        }

        // Link mux to sink
        internal_links.push((format!("{}:src", mux_id), format!("{}:sink", sink_id)));

        info!(
            "📡 Created MPEG-TS/SRT block with {} video track(s) and {} audio chain(s)",
            num_video_tracks, num_audio_tracks
        );

        Ok(BlockBuildResult {
            elements,
            internal_links,
            bus_message_handler: None,
        })
    }
}

/// Get metadata for MPEG-TS/SRT output blocks (for UI/API).
pub fn get_blocks() -> Vec<BlockDefinition> {
    vec![mpegtssrt_output_definition()]
}

/// Get MPEG-TS/SRT Output block definition (metadata only).
fn mpegtssrt_output_definition() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.mpegtssrt_output".to_string(),
        name: "MPEG-TS/SRT Output".to_string(),
        description: "Muxes multiple audio/video streams to MPEG Transport Stream and outputs via SRT. Automatically handles video parsing (H.264/H.265/AV1/VP9) and AAC encoding for raw audio inputs. Optimized for UDP streaming with alignment=7.".to_string(),
        category: "Outputs".to_string(),
        exposed_properties: vec![
            ExposedProperty {
                name: "num_video_tracks".to_string(),
                label: "Number of Video Tracks".to_string(),
                description: "Number of video input tracks (0-16)".to_string(),
                property_type: PropertyType::UInt,
                default_value: Some(PropertyValue::UInt(1)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "num_video_tracks".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "num_audio_tracks".to_string(),
                label: "Number of Audio Tracks".to_string(),
                description: "Number of audio input tracks (0-32)".to_string(),
                property_type: PropertyType::UInt,
                default_value: Some(PropertyValue::UInt(8)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "num_audio_tracks".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "srt_uri".to_string(),
                label: "SRT URI".to_string(),
                description: "SRT URI (e.g., 'srt://127.0.0.1:5000?mode=caller' or 'srt://:5000?mode=listener')".to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String("srt://:5000?mode=listener".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "srt_uri".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "video_parser".to_string(),
                label: "Video Parser".to_string(),
                description: "Video parser type to use (h264parse, h265parse, av1parse, vp9parse)".to_string(),
                property_type: PropertyType::Enum {
                    values: vec![
                        EnumValue {
                            value: "h264parse".to_string(),
                            label: Some("H.264 Parser".to_string()),
                        },
                        EnumValue {
                            value: "h265parse".to_string(),
                            label: Some("H.265 Parser".to_string()),
                        },
                        EnumValue {
                            value: "av1parse".to_string(),
                            label: Some("AV1 Parser".to_string()),
                        },
                        EnumValue {
                            value: "vp9parse".to_string(),
                            label: Some("VP9 Parser (if available)".to_string()),
                        },
                    ],
                },
                default_value: Some(PropertyValue::String("h264parse".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "video_parser".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "latency".to_string(),
                label: "SRT Latency (ms)".to_string(),
                description: "SRT latency in milliseconds (default: 125ms)".to_string(),
                property_type: PropertyType::Int,
                default_value: Some(PropertyValue::Int(125)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "latency".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "wait_for_connection".to_string(),
                label: "Wait For Connection".to_string(),
                description: "Block the stream until a client connects (default: false)".to_string(),
                property_type: PropertyType::Bool,
                default_value: Some(PropertyValue::Bool(false)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "wait_for_connection".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "auto_reconnect".to_string(),
                label: "Auto Reconnect".to_string(),
                description: "Automatically reconnect when connection fails (default: true)".to_string(),
                property_type: PropertyType::Bool,
                default_value: Some(PropertyValue::Bool(true)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "auto_reconnect".to_string(),
                    transform: None,
                },
            },
        ],
        // External pads are now computed dynamically based on num_video_tracks and num_audio_tracks properties
        // This is just the default/fallback configuration
        external_pads: ExternalPads {
            inputs: vec![
                ExternalPad {
                    name: "video_in".to_string(),
                    media_type: MediaType::Video,
                    internal_element_id: "video_input".to_string(),
                    internal_pad_name: "sink".to_string(),
                },
            ],
            outputs: vec![],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("📡".to_string()),
            color: Some("#FF6B35".to_string()), // Orange-red for transport stream output
            width: Some(2.5),
            height: Some(3.0),
        }),
    }
}
