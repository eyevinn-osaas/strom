//! EFP over SRT input block builder.
//!
//! This block receives an SRT stream carrying EFP (Eyevinn Fragment Protocol) and demuxes
//! it into separate video and audio output pads.
//!
//! Pipeline structure (decode=true, default):
//! ```text
//! srtsrc -> efpdemux -> decodebin -> videoconvert -> video_output (identity) -> [external video_out]
//!                    -> decodebin -> audioconvert -> audioresample -> audio_output_0 (identity) -> [external audio_out_0]
//! ```
//!
//! Pipeline structure (decode=false, passthrough):
//! ```text
//! srtsrc -> efpdemux -> video_output (identity) -> [external video_out]
//!                    -> audio_output_0 (identity) -> [external audio_out_0]
//! ```
//!
//! `efpdemux` has dynamic pads — uses `connect_pad_added` to link to identity
//! elements based on caps (video/ or audio/).

use crate::blocks::{BlockBuildContext, BlockBuildError, BlockBuildResult, BlockBuilder};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use strom_types::{block::*, element::ElementPadRef, PropertyValue, *};
use tracing::{debug, error, info, warn};

/// EFP/SRT Input block builder.
pub struct EfpSrtInputBuilder;

impl BlockBuilder for EfpSrtInputBuilder {
    fn get_external_pads(
        &self,
        properties: &HashMap<String, PropertyValue>,
    ) -> Option<ExternalPads> {
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
            .unwrap_or(1);

        let mut outputs = Vec::new();

        for i in 0..num_video_tracks {
            outputs.push(ExternalPad {
                label: Some(format!("V{}", i)),
                name: if num_video_tracks == 1 {
                    "video_out".to_string()
                } else {
                    format!("video_out_{}", i)
                },
                media_type: MediaType::Video,
                internal_element_id: if num_video_tracks == 1 {
                    "video_output".to_string()
                } else {
                    format!("video_output_{}", i)
                },
                internal_pad_name: "src".to_string(),
            });
        }

        for i in 0..num_audio_tracks {
            outputs.push(ExternalPad {
                label: Some(format!("A{}", i)),
                name: format!("audio_out_{}", i),
                media_type: MediaType::Audio,
                internal_element_id: format!("audio_output_{}", i),
                internal_pad_name: "src".to_string(),
            });
        }

        Some(ExternalPads {
            inputs: vec![],
            outputs,
        })
    }

    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
        _ctx: &BlockBuildContext,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        let decode = properties
            .get("decode")
            .and_then(|v| match v {
                PropertyValue::Bool(b) => Some(*b),
                _ => None,
            })
            .unwrap_or(true);

        info!(
            "Building EFP/SRT Input block instance: {} (decode={})",
            instance_id, decode
        );

        let srt_uri = properties
            .get("srt_uri")
            .and_then(|v| match v {
                PropertyValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_else(|| DEFAULT_SRT_INPUT_URI.to_string());

        let latency = properties
            .get("latency")
            .and_then(|v| match v {
                PropertyValue::UInt(u) => Some(*u as i32),
                PropertyValue::Int(i) => Some(*i as i32),
                _ => None,
            })
            .unwrap_or(DEFAULT_SRT_LATENCY_MS);

        let bucket_timeout = properties
            .get("bucket_timeout")
            .and_then(|v| match v {
                PropertyValue::UInt(u) => Some(*u as u32),
                PropertyValue::Int(i) => Some(*i as u32),
                _ => None,
            })
            .unwrap_or(DEFAULT_EFP_BUCKET_TIMEOUT);

        let hol_timeout = properties
            .get("hol_timeout")
            .and_then(|v| match v {
                PropertyValue::UInt(u) => Some(*u as u32),
                PropertyValue::Int(i) => Some(*i as u32),
                _ => None,
            })
            .unwrap_or(DEFAULT_EFP_HOL_TIMEOUT);

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
            .unwrap_or(1);

        // Create srtsrc
        let src_id = format!("{}:srtsrc", instance_id);
        let srtsrc = gst::ElementFactory::make("srtsrc")
            .name(&src_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("srtsrc: {}", e)))?;

        srtsrc.set_property("uri", &srt_uri);
        srtsrc.set_property("latency", latency);

        info!(
            "SRT source configured: uri={}, latency={}ms",
            srt_uri, latency
        );

        // Create efpdemux
        let demux_id = format!("{}:efpdemux", instance_id);
        let demux_element = gst::ElementFactory::make("efpdemux")
            .name(&demux_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("efpdemux: {}", e)))?;

        demux_element.set_property("bucket-timeout", bucket_timeout);
        demux_element.set_property("hol-timeout", hol_timeout);

        info!(
            "EFP demuxer configured: bucket-timeout={}, hol-timeout={}",
            bucket_timeout, hol_timeout
        );

        let mut elements = vec![(src_id.clone(), srtsrc)];

        // Create video output identity elements
        let mut video_guards = Vec::new();
        for i in 0..num_video_tracks {
            let element_id = if num_video_tracks == 1 {
                format!("{}:video_output", instance_id)
            } else {
                format!("{}:video_output_{}", instance_id, i)
            };

            let identity = gst::ElementFactory::make("identity")
                .name(&element_id)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("video identity {}: {}", i, e))
                })?;

            let guard = Arc::new(AtomicBool::new(false));
            video_guards.push((identity.downgrade(), guard));
            elements.push((element_id, identity));
        }

        // Create audio output identity elements
        let mut audio_guards = Vec::new();
        for i in 0..num_audio_tracks {
            let element_id = format!("{}:audio_output_{}", instance_id, i);

            let identity = gst::ElementFactory::make("identity")
                .name(&element_id)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("audio identity {}: {}", i, e))
                })?;

            let guard = Arc::new(AtomicBool::new(false));
            audio_guards.push((identity.downgrade(), guard));
            elements.push((element_id, identity));
        }

        // Setup dynamic pad linking from efpdemux
        let instance_id_clone = instance_id.to_string();
        let mode_label = if decode { "decode" } else { "passthrough" };
        let mode_label_owned = mode_label.to_string();

        // Connect pad-added callback before moving demux_element into the elements vec
        demux_element.connect_pad_added(move |element, pad| {
            let caps = pad.current_caps().or_else(|| {
                let query_caps = pad.query_caps(None);
                if !query_caps.is_any() && !query_caps.is_empty() {
                    Some(query_caps)
                } else {
                    None
                }
            });

            let caps_name = caps
                .as_ref()
                .and_then(|c| c.structure(0))
                .map(|s| s.name().to_string());

            let pad_name = pad.name().to_string();

            let is_video = caps_name
                .as_ref()
                .map(|n| n.starts_with("video/"))
                .unwrap_or(false);
            let is_audio = caps_name
                .as_ref()
                .map(|n| n.starts_with("audio/"))
                .unwrap_or(false);

            debug!(
                "EFPSRT Input {} ({}): pad added: {} (caps: {})",
                instance_id_clone,
                mode_label_owned,
                pad_name,
                caps_name.as_deref().unwrap_or("unknown")
            );

            if is_video {
                for (weak_identity, guard) in &video_guards {
                    if guard.swap(true, Ordering::SeqCst) {
                        continue;
                    }

                    if let Some(identity) = weak_identity.upgrade() {
                        if decode {
                            if let Err(e) =
                                link_decoded_video(element, pad, &identity, &instance_id_clone)
                            {
                                error!(
                                    "EFPSRT Input {}: Failed to link decoded video pad {}: {}",
                                    instance_id_clone, pad_name, e
                                );
                                guard.store(false, Ordering::SeqCst);
                                continue;
                            }
                        } else if let Some(sink_pad) = identity.static_pad("sink") {
                            if let Err(e) = pad.link(&sink_pad) {
                                error!(
                                    "EFPSRT Input {}: Failed to link video pad {}: {:?}",
                                    instance_id_clone, pad_name, e
                                );
                                guard.store(false, Ordering::SeqCst);
                                continue;
                            }
                        }
                        info!(
                            "EFPSRT Input {}: Linked video pad {} -> {}",
                            instance_id_clone,
                            pad_name,
                            identity.name()
                        );
                        return;
                    }
                }
                warn!(
                    "EFPSRT Input {}: No available video output for pad {}",
                    instance_id_clone, pad_name
                );
            } else if is_audio {
                for (weak_identity, guard) in &audio_guards {
                    if guard.swap(true, Ordering::SeqCst) {
                        continue;
                    }

                    if let Some(identity) = weak_identity.upgrade() {
                        if decode {
                            if let Err(e) =
                                link_decoded_audio(element, pad, &identity, &instance_id_clone)
                            {
                                error!(
                                    "EFPSRT Input {}: Failed to link decoded audio pad {}: {}",
                                    instance_id_clone, pad_name, e
                                );
                                guard.store(false, Ordering::SeqCst);
                                continue;
                            }
                        } else if let Some(sink_pad) = identity.static_pad("sink") {
                            if let Err(e) = pad.link(&sink_pad) {
                                error!(
                                    "EFPSRT Input {}: Failed to link audio pad {}: {:?}",
                                    instance_id_clone, pad_name, e
                                );
                                guard.store(false, Ordering::SeqCst);
                                continue;
                            }
                        }
                        info!(
                            "EFPSRT Input {}: Linked audio pad {} -> {}",
                            instance_id_clone,
                            pad_name,
                            identity.name()
                        );
                        return;
                    }
                }
                warn!(
                    "EFPSRT Input {}: No available audio output for pad {}",
                    instance_id_clone, pad_name
                );
            } else {
                debug!(
                    "EFPSRT Input {}: Ignoring pad {} with caps {}",
                    instance_id_clone,
                    pad_name,
                    caps_name.as_deref().unwrap_or("unknown")
                );
            }
        });

        // Add demux_element to elements after setting up the pad-added callback
        elements.push((demux_id.clone(), demux_element));

        // Internal link: srtsrc -> efpdemux
        let internal_links = vec![(
            ElementPadRef::pad(&src_id, "src"),
            ElementPadRef::pad(&demux_id, "sink"),
        )];

        info!(
            "Created EFP/SRT Input block ({}) with {} video output(s) and {} audio output(s)",
            mode_label, num_video_tracks, num_audio_tracks
        );

        Ok(BlockBuildResult {
            elements,
            internal_links,
            bus_message_handler: None,
            pad_properties: HashMap::new(),
        })
    }
}

/// Dynamically insert decodebin + videoconvert between an efpdemux video pad and an identity element.
/// efpdemux pad -> decodebin -> videoconvert -> identity
fn link_decoded_video(
    element: &gst::Element,
    src_pad: &gst::Pad,
    identity: &gst::Element,
    instance_id: &str,
) -> Result<(), String> {
    let bin = element
        .parent()
        .and_then(|p| p.downcast::<gst::Bin>().ok())
        .ok_or("parent is not a Bin")?;

    let decodebin_name = format!("{}:video_decodebin_{}", instance_id, src_pad.name());
    let decodebin = gst::ElementFactory::make("decodebin")
        .name(&decodebin_name)
        .build()
        .map_err(|e| format!("decodebin: {}", e))?;

    let convert_name = format!("{}:videoconvert_{}", instance_id, src_pad.name());
    let videoconvert = gst::ElementFactory::make("videoconvert")
        .name(&convert_name)
        .build()
        .map_err(|e| format!("videoconvert: {}", e))?;

    bin.add_many([&decodebin, &videoconvert])
        .map_err(|e| format!("add decode elements: {}", e))?;

    // Link videoconvert -> identity first (downstream before upstream)
    let convert_src = videoconvert
        .static_pad("src")
        .ok_or("videoconvert has no src pad")?;
    let identity_sink = identity
        .static_pad("sink")
        .ok_or("identity has no sink pad")?;
    convert_src
        .link(&identity_sink)
        .map_err(|e| format!("link videoconvert -> identity: {:?}", e))?;

    // Connect decodebin's dynamic pad to videoconvert
    let videoconvert_weak = videoconvert.downgrade();
    let instance_id_owned = instance_id.to_string();
    decodebin.connect_pad_added(move |_element, pad| {
        let caps_name = pad
            .current_caps()
            .or_else(|| {
                let qc = pad.query_caps(None);
                if !qc.is_any() && !qc.is_empty() {
                    Some(qc)
                } else {
                    None
                }
            })
            .and_then(|c| c.structure(0).map(|s| s.name().to_string()));

        if caps_name
            .as_ref()
            .map(|n| n.starts_with("video/"))
            .unwrap_or(false)
        {
            if let Some(videoconvert) = videoconvert_weak.upgrade() {
                if let Some(sink) = videoconvert.static_pad("sink") {
                    if let Err(e) = pad.link(&sink) {
                        error!(
                            "EFPSRT Input {}: Failed to link decodebin to videoconvert: {:?}",
                            instance_id_owned, e
                        );
                    }
                }
            }
        }
    });

    videoconvert
        .sync_state_with_parent()
        .map_err(|e| format!("sync videoconvert: {}", e))?;
    decodebin
        .sync_state_with_parent()
        .map_err(|e| format!("sync decodebin: {}", e))?;

    // Link efpdemux pad -> decodebin last to start data flow when chain is ready
    let decodebin_sink = decodebin
        .static_pad("sink")
        .ok_or("decodebin has no sink pad")?;
    src_pad
        .link(&decodebin_sink)
        .map_err(|e| format!("link efpdemux -> decodebin: {:?}", e))?;

    debug!(
        "EFPSRT Input {}: Inserted decodebin + videoconvert for pad {}",
        instance_id,
        src_pad.name()
    );
    Ok(())
}

/// Dynamically insert decodebin + audioconvert + audioresample between an efpdemux audio pad and an identity element.
/// efpdemux pad -> decodebin -> audioconvert -> audioresample -> identity
fn link_decoded_audio(
    element: &gst::Element,
    src_pad: &gst::Pad,
    identity: &gst::Element,
    instance_id: &str,
) -> Result<(), String> {
    let bin = element
        .parent()
        .and_then(|p| p.downcast::<gst::Bin>().ok())
        .ok_or("parent is not a Bin")?;

    let decodebin_name = format!("{}:audio_decodebin_{}", instance_id, src_pad.name());
    let decodebin = gst::ElementFactory::make("decodebin")
        .name(&decodebin_name)
        .build()
        .map_err(|e| format!("decodebin: {}", e))?;

    let convert_name = format!("{}:audioconvert_{}", instance_id, src_pad.name());
    let resample_name = format!("{}:audioresample_{}", instance_id, src_pad.name());

    let audioconvert = gst::ElementFactory::make("audioconvert")
        .name(&convert_name)
        .build()
        .map_err(|e| format!("audioconvert: {}", e))?;
    let audioresample = gst::ElementFactory::make("audioresample")
        .name(&resample_name)
        .build()
        .map_err(|e| format!("audioresample: {}", e))?;

    bin.add_many([&decodebin, &audioconvert, &audioresample])
        .map_err(|e| format!("add decode elements: {}", e))?;

    // Link downstream first: audioconvert -> audioresample -> identity
    let resample_src = audioresample
        .static_pad("src")
        .ok_or("audioresample has no src pad")?;
    let identity_sink = identity
        .static_pad("sink")
        .ok_or("identity has no sink pad")?;

    audioconvert
        .link(&audioresample)
        .map_err(|e| format!("link audioconvert -> audioresample: {}", e))?;
    resample_src
        .link(&identity_sink)
        .map_err(|e| format!("link audioresample -> identity: {:?}", e))?;

    // Connect decodebin's dynamic pad to audioconvert
    let audioconvert_weak = audioconvert.downgrade();
    let instance_id_owned = instance_id.to_string();
    decodebin.connect_pad_added(move |_element, pad| {
        let caps_name = pad
            .current_caps()
            .or_else(|| {
                let qc = pad.query_caps(None);
                if !qc.is_any() && !qc.is_empty() {
                    Some(qc)
                } else {
                    None
                }
            })
            .and_then(|c| c.structure(0).map(|s| s.name().to_string()));

        if caps_name
            .as_ref()
            .map(|n| n.starts_with("audio/"))
            .unwrap_or(false)
        {
            if let Some(audioconvert) = audioconvert_weak.upgrade() {
                if let Some(sink) = audioconvert.static_pad("sink") {
                    if let Err(e) = pad.link(&sink) {
                        error!(
                            "EFPSRT Input {}: Failed to link decodebin to audioconvert: {:?}",
                            instance_id_owned, e
                        );
                    }
                }
            }
        }
    });

    audioconvert
        .sync_state_with_parent()
        .map_err(|e| format!("sync audioconvert: {}", e))?;
    audioresample
        .sync_state_with_parent()
        .map_err(|e| format!("sync audioresample: {}", e))?;
    decodebin
        .sync_state_with_parent()
        .map_err(|e| format!("sync decodebin: {}", e))?;

    // Link efpdemux pad -> decodebin last
    let decodebin_sink = decodebin
        .static_pad("sink")
        .ok_or("decodebin has no sink pad")?;
    src_pad
        .link(&decodebin_sink)
        .map_err(|e| format!("link efpdemux -> decodebin: {:?}", e))?;

    debug!(
        "EFPSRT Input {}: Inserted decodebin + audioconvert + audioresample for pad {}",
        instance_id,
        src_pad.name()
    );
    Ok(())
}

/// Get metadata for EFP/SRT input blocks (for UI/API).
pub fn get_blocks() -> Vec<BlockDefinition> {
    vec![efpsrt_input_definition()]
}

/// Get EFP/SRT Input block definition (metadata only).
fn efpsrt_input_definition() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.efpsrt_input".to_string(),
        name: "EFP/SRT Input".to_string(),
        description: "Receives an SRT stream carrying EFP (Eyevinn Fragment Protocol) and demuxes it into separate video and audio outputs. Supports decode (default) and passthrough modes.".to_string(),
        category: "Inputs".to_string(),
        exposed_properties: vec![
            ExposedProperty {
                name: "srt_uri".to_string(),
                label: "SRT URI".to_string(),
                description: "SRT URI (e.g., 'srt://:4000?mode=listener' or 'srt://192.0.2.1:4000?mode=caller')".to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String(
                    DEFAULT_SRT_INPUT_URI.to_string(),
                )),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "srt_uri".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "latency".to_string(),
                label: "SRT Latency (ms)".to_string(),
                description: "SRT latency in milliseconds".to_string(),
                property_type: PropertyType::Int,
                default_value: Some(PropertyValue::Int(DEFAULT_SRT_LATENCY_MS as i64)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "latency".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "decode".to_string(),
                label: "Decode".to_string(),
                description: "Decode video/audio streams (true) or pass through encoded elementary streams (false)".to_string(),
                property_type: PropertyType::Bool,
                default_value: Some(PropertyValue::Bool(true)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "decode".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "bucket_timeout".to_string(),
                label: "Bucket Timeout".to_string(),
                description: "EFP bucket timeout in units of 10ms (default: 5 = 50ms)".to_string(),
                property_type: PropertyType::UInt,
                default_value: Some(PropertyValue::UInt(DEFAULT_EFP_BUCKET_TIMEOUT as u64)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "bucket_timeout".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "hol_timeout".to_string(),
                label: "HOL Timeout".to_string(),
                description: "EFP head-of-line timeout in units of 10ms (default: 5 = 50ms)".to_string(),
                property_type: PropertyType::UInt,
                default_value: Some(PropertyValue::UInt(DEFAULT_EFP_HOL_TIMEOUT as u64)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "hol_timeout".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "num_video_tracks".to_string(),
                label: "Number of Video Tracks".to_string(),
                description: "Number of video output tracks".to_string(),
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
                description: "Number of audio output tracks".to_string(),
                property_type: PropertyType::UInt,
                default_value: Some(PropertyValue::UInt(1)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "num_audio_tracks".to_string(),
                    transform: None,
                },
            },
        ],
        external_pads: ExternalPads {
            inputs: vec![],
            outputs: vec![
                ExternalPad {
                    label: Some("V0".to_string()),
                    name: "video_out".to_string(),
                    media_type: MediaType::Video,
                    internal_element_id: "video_output".to_string(),
                    internal_pad_name: "src".to_string(),
                },
                ExternalPad {
                    label: Some("A0".to_string()),
                    name: "audio_out_0".to_string(),
                    media_type: MediaType::Audio,
                    internal_element_id: "audio_output_0".to_string(),
                    internal_pad_name: "src".to_string(),
                },
            ],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("📡".to_string()),
            width: Some(2.5),
            height: Some(2.0),
            ..Default::default()
        }),
    }
}
