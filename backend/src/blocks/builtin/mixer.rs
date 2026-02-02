//! Stereo Mixer block - a digital mixing console for audio.
//!
//! This block provides a mixer similar to digital consoles like Behringer X32:
//! - Configurable number of input channels (1-32)
//! - Per-channel: gate, compressor, 4-band parametric EQ, pan, fader, mute
//! - Main stereo bus with audiomixer
//! - Per-channel metering
//!
//! Future phases will add: aux sends, subgroups, PFL
//!
//! Pipeline structure per channel:
//! ```text
//! input_N → audioconvert → capsfilter(F32LE) → gate → compressor → EQ →
//!           audiopanorama_N → volume_N → level_N → audiomixer (main)
//! ```
//!
//! Processing uses LSP LV2 plugins for professional-quality gate, compressor, and EQ.
//! The audiomixer sink pads also have volume/mute properties that can be used.

use crate::blocks::{BlockBuildContext, BlockBuildError, BlockBuildResult, BlockBuilder};
use crate::events::EventBroadcaster;
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use strom_types::{
    block::*, element::ElementPadRef, EnumValue, FlowId, MediaType, PropertyValue, StromEvent,
};
use tracing::{debug, info, trace};

/// Maximum number of input channels
const MAX_CHANNELS: usize = 32;
/// Default number of channels
const DEFAULT_CHANNELS: usize = 8;

/// Mixer block builder.
pub struct MixerBuilder;

impl BlockBuilder for MixerBuilder {
    fn get_external_pads(
        &self,
        properties: &HashMap<String, PropertyValue>,
    ) -> Option<ExternalPads> {
        let num_channels = parse_num_channels(properties);

        // Create input pads dynamically
        let inputs = (0..num_channels)
            .map(|i| ExternalPad {
                name: format!("input_{}", i + 1),
                media_type: MediaType::Audio,
                internal_element_id: format!("convert_{}", i),
                internal_pad_name: "sink".to_string(),
            })
            .collect();

        // Main stereo output
        let outputs = vec![ExternalPad {
            name: "main_out".to_string(),
            media_type: MediaType::Audio,
            internal_element_id: "main_level".to_string(),
            internal_pad_name: "src".to_string(),
        }];

        Some(ExternalPads { inputs, outputs })
    }

    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
        _ctx: &BlockBuildContext,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        info!("Building Mixer block instance: {}", instance_id);

        let num_channels = parse_num_channels(properties);
        info!("Mixer config: {} input channels", num_channels);

        let mut elements = Vec::new();
        let mut internal_links = Vec::new();

        // ========================================================================
        // Create main audiomixer
        // ========================================================================
        let mixer_id = format!("{}:audiomixer", instance_id);
        let audiomixer = gst::ElementFactory::make("audiomixer")
            .name(&mixer_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("audiomixer: {}", e)))?;
        elements.push((mixer_id.clone(), audiomixer.clone()));

        // Main output volume (master fader)
        let main_volume_id = format!("{}:main_volume", instance_id);
        let main_volume = gst::ElementFactory::make("volume")
            .name(&main_volume_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("main volume: {}", e)))?;

        // Set main fader from properties
        let main_fader = properties
            .get("main_fader")
            .and_then(|v| match v {
                PropertyValue::Float(f) => Some(*f),
                _ => None,
            })
            .unwrap_or(1.0);
        main_volume.set_property("volume", main_fader);
        elements.push((main_volume_id.clone(), main_volume));

        // Main level meter (for main mix metering)
        let main_level_id = format!("{}:main_level", instance_id);
        let main_level = gst::ElementFactory::make("level")
            .name(&main_level_id)
            .property("interval", 100_000_000u64) // 100ms
            .property("post-messages", true)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("main level: {}", e)))?;
        elements.push((main_level_id.clone(), main_level));

        // Link: mixer → main_volume → main_level
        internal_links.push((
            ElementPadRef::pad(&mixer_id, "src"),
            ElementPadRef::pad(&main_volume_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&main_volume_id, "src"),
            ElementPadRef::pad(&main_level_id, "sink"),
        ));

        // ========================================================================
        // Create per-channel processing
        // ========================================================================
        for ch in 0..num_channels {
            let ch_num = ch + 1; // 1-indexed for display

            // Get channel properties
            let pan = properties
                .get(&format!("ch{}_pan", ch_num))
                .and_then(|v| match v {
                    PropertyValue::Float(f) => Some(*f),
                    _ => None,
                })
                .unwrap_or(0.0);

            let fader = properties
                .get(&format!("ch{}_fader", ch_num))
                .and_then(|v| match v {
                    PropertyValue::Float(f) => Some(*f),
                    _ => None,
                })
                .unwrap_or(0.75); // Default ~-6dB

            let mute = properties
                .get(&format!("ch{}_mute", ch_num))
                .and_then(|v| match v {
                    PropertyValue::Bool(b) => Some(*b),
                    _ => None,
                })
                .unwrap_or(false);

            // audioconvert (ensure proper format for processing)
            let convert_id = format!("{}:convert_{}", instance_id, ch);
            let convert = gst::ElementFactory::make("audioconvert")
                .name(&convert_id)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("audioconvert ch{}: {}", ch_num, e))
                })?;
            elements.push((convert_id.clone(), convert));

            // capsfilter to ensure F32LE stereo format for LV2 plugins
            let caps_id = format!("{}:caps_{}", instance_id, ch);
            let caps = gst::Caps::builder("audio/x-raw")
                .field("format", "F32LE")
                .field("channels", 2i32)
                .field("layout", "interleaved")
                .build();
            let capsfilter = gst::ElementFactory::make("capsfilter")
                .name(&caps_id)
                .property("caps", &caps)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("capsfilter ch{}: {}", ch_num, e))
                })?;
            elements.push((caps_id.clone(), capsfilter));

            // ----------------------------------------------------------------
            // Gate (LSP Gate Stereo) - noise gate
            // ----------------------------------------------------------------
            let gate_enabled =
                get_bool_prop(properties, &format!("ch{}_gate_enabled", ch_num), false);
            let gate_threshold =
                get_float_prop(properties, &format!("ch{}_gate_threshold", ch_num), -40.0);
            let gate_attack = get_float_prop(properties, &format!("ch{}_gate_attack", ch_num), 5.0);
            let gate_release =
                get_float_prop(properties, &format!("ch{}_gate_release", ch_num), 100.0);

            let gate_id = format!("{}:gate_{}", instance_id, ch);
            let gate = gst::ElementFactory::make("lsp-plug-in-plugins-lv2-gate-stereo")
                .name(&gate_id)
                .property("enabled", gate_enabled)
                .property("gt", db_to_linear(gate_threshold) as f32) // threshold in linear
                .property("at", gate_attack as f32) // attack in ms
                .property("rt", gate_release as f32) // release in ms
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("gate ch{}: {}", ch_num, e))
                })?;
            elements.push((gate_id.clone(), gate));

            // ----------------------------------------------------------------
            // Compressor (LSP Compressor Stereo)
            // ----------------------------------------------------------------
            let comp_enabled =
                get_bool_prop(properties, &format!("ch{}_comp_enabled", ch_num), false);
            let comp_threshold =
                get_float_prop(properties, &format!("ch{}_comp_threshold", ch_num), -20.0);
            let comp_ratio = get_float_prop(properties, &format!("ch{}_comp_ratio", ch_num), 4.0);
            let comp_attack =
                get_float_prop(properties, &format!("ch{}_comp_attack", ch_num), 10.0);
            let comp_release =
                get_float_prop(properties, &format!("ch{}_comp_release", ch_num), 100.0);
            let comp_makeup = get_float_prop(properties, &format!("ch{}_comp_makeup", ch_num), 0.0);

            let comp_id = format!("{}:comp_{}", instance_id, ch);
            let compressor = gst::ElementFactory::make("lsp-plug-in-plugins-lv2-compressor-stereo")
                .name(&comp_id)
                .property("enabled", comp_enabled)
                .property("al", db_to_linear(comp_threshold) as f32) // attack threshold
                .property("cr", comp_ratio as f32) // ratio
                .property("at", comp_attack as f32) // attack in ms
                .property("rt", comp_release as f32) // release in ms
                .property("mk", db_to_linear(comp_makeup) as f32) // makeup gain
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("compressor ch{}: {}", ch_num, e))
                })?;
            elements.push((comp_id.clone(), compressor));

            // ----------------------------------------------------------------
            // EQ (LSP Parametric Equalizer x8 Stereo) - 4 bands used
            // ----------------------------------------------------------------
            let eq_enabled = get_bool_prop(properties, &format!("ch{}_eq_enabled", ch_num), false);

            let eq_id = format!("{}:eq_{}", instance_id, ch);
            let eq = gst::ElementFactory::make("lsp-plug-in-plugins-lv2-para-equalizer-x8-stereo")
                .name(&eq_id)
                .property("enabled", eq_enabled)
                .build()
                .map_err(|e| BlockBuildError::ElementCreation(format!("eq ch{}: {}", ch_num, e)))?;

            // Configure 4 EQ bands (low, low-mid, high-mid, high)
            // Default frequencies: 80Hz, 400Hz, 2kHz, 8kHz
            let eq_defaults = [
                (80.0f32, 1.0f32, 1.0f32),   // Band 0: Low
                (400.0f32, 1.0f32, 1.0f32),  // Band 1: Low-mid
                (2000.0f32, 1.0f32, 1.0f32), // Band 2: High-mid
                (8000.0f32, 1.0f32, 1.0f32), // Band 3: High
            ];

            for (band, (def_freq, _def_gain, def_q)) in eq_defaults.iter().enumerate() {
                let freq = get_float_prop(
                    properties,
                    &format!("ch{}_eq{}_freq", ch_num, band + 1),
                    *def_freq as f64,
                );
                let gain = get_float_prop(
                    properties,
                    &format!("ch{}_eq{}_gain", ch_num, band + 1),
                    0.0,
                ); // in dB
                let q = get_float_prop(
                    properties,
                    &format!("ch{}_eq{}_q", ch_num, band + 1),
                    *def_q as f64,
                );

                // Set filter type to "Bell" using string representation for enum
                eq.set_property_from_str(&format!("ft-{}", band), "Bell");
                eq.set_property(&format!("f-{}", band), freq as f32);
                eq.set_property(&format!("g-{}", band), db_to_linear(gain) as f32);
                eq.set_property(&format!("q-{}", band), q as f32);
            }
            elements.push((eq_id.clone(), eq));

            // audiopanorama (pan control)
            let pan_id = format!("{}:pan_{}", instance_id, ch);
            let panorama = gst::ElementFactory::make("audiopanorama")
                .name(&pan_id)
                .property("panorama", pan as f32)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("audiopanorama ch{}: {}", ch_num, e))
                })?;
            elements.push((pan_id.clone(), panorama));

            // volume (channel fader + mute)
            let volume_id = format!("{}:volume_{}", instance_id, ch);
            let effective_volume = if mute { 0.0 } else { fader };
            let volume = gst::ElementFactory::make("volume")
                .name(&volume_id)
                .property("volume", effective_volume)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("volume ch{}: {}", ch_num, e))
                })?;
            elements.push((volume_id.clone(), volume));

            // level (metering) - tee before mixer to get pre-mixer levels
            let level_id = format!("{}:level_{}", instance_id, ch);
            let level = gst::ElementFactory::make("level")
                .name(&level_id)
                .property("interval", 100_000_000u64) // 100ms
                .property("post-messages", true)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("level ch{}: {}", ch_num, e))
                })?;
            elements.push((level_id.clone(), level));

            // Chain: convert → caps → gate → comp → eq → pan → volume → level → mixer
            internal_links.push((
                ElementPadRef::pad(&convert_id, "src"),
                ElementPadRef::pad(&caps_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&caps_id, "src"),
                ElementPadRef::pad(&gate_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&gate_id, "src"),
                ElementPadRef::pad(&comp_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&comp_id, "src"),
                ElementPadRef::pad(&eq_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&eq_id, "src"),
                ElementPadRef::pad(&pan_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&pan_id, "src"),
                ElementPadRef::pad(&volume_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&volume_id, "src"),
                ElementPadRef::pad(&level_id, "sink"),
            ));
            // Link to audiomixer (request pad)
            internal_links.push((
                ElementPadRef::pad(&level_id, "src"),
                ElementPadRef::element(&mixer_id), // Request pad from audiomixer
            ));

            debug!(
                "Channel {} created: pan={}, fader={}, mute={}",
                ch_num, pan, fader, mute
            );
        }

        info!("Mixer block created with {} channels", num_channels);

        // Create bus message handler for metering
        let handler_instance_id = instance_id.to_string();
        let bus_message_handler = Some(Box::new(
            move |bus: &gst::Bus, flow_id: FlowId, events: EventBroadcaster| {
                connect_mixer_meter_handler(bus, flow_id, events, handler_instance_id.clone())
            },
        ) as crate::blocks::BusMessageConnectFn);

        Ok(BlockBuildResult {
            elements,
            internal_links,
            bus_message_handler,
            pad_properties: HashMap::new(),
        })
    }
}

/// Parse number of channels from properties.
fn parse_num_channels(properties: &HashMap<String, PropertyValue>) -> usize {
    properties
        .get("num_channels")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as usize),
            PropertyValue::UInt(u) => Some(*u as usize),
            PropertyValue::String(s) => s.parse::<usize>().ok(),
            _ => None,
        })
        .unwrap_or(DEFAULT_CHANNELS)
        .clamp(1, MAX_CHANNELS)
}

/// Get a float property with default.
fn get_float_prop(properties: &HashMap<String, PropertyValue>, name: &str, default: f64) -> f64 {
    properties
        .get(name)
        .and_then(|v| match v {
            PropertyValue::Float(f) => Some(*f),
            PropertyValue::Int(i) => Some(*i as f64),
            _ => None,
        })
        .unwrap_or(default)
}

/// Get a bool property with default.
fn get_bool_prop(properties: &HashMap<String, PropertyValue>, name: &str, default: bool) -> bool {
    properties
        .get(name)
        .and_then(|v| match v {
            PropertyValue::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(default)
}

/// Convert dB to linear scale.
fn db_to_linear(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

/// Extract f64 values from a GValueArray field in a GStreamer structure.
fn extract_level_values(structure: &gst::StructureRef, field_name: &str) -> Vec<f64> {
    use gstreamer::glib;

    if let Ok(array) = structure.get::<glib::ValueArray>(field_name) {
        array.iter().filter_map(|v| v.get::<f64>().ok()).collect()
    } else {
        Vec::new()
    }
}

/// Connect message handler for all level elements in this mixer block.
fn connect_mixer_meter_handler(
    bus: &gst::Bus,
    flow_id: FlowId,
    events: EventBroadcaster,
    instance_id: String,
) -> gst::glib::SignalHandlerId {
    use gst::MessageView;

    debug!(
        "Connecting mixer meter handler for flow {} instance {}",
        flow_id, instance_id
    );

    bus.add_signal_watch();

    let level_prefix = format!("{}:level_", instance_id);
    let main_level_id = format!("{}:main_level", instance_id);

    bus.connect_message(None, move |_bus, msg| {
        if let MessageView::Element(element_msg) = msg.view() {
            if let Some(s) = element_msg.structure() {
                if s.name() == "level" {
                    if let Some(source) = msg.src() {
                        let source_name = source.name().to_string();

                        let rms = extract_level_values(s, "rms");
                        let peak = extract_level_values(s, "peak");
                        let decay = extract_level_values(s, "decay");

                        if rms.is_empty() {
                            return;
                        }

                        // Check if this is the main level meter
                        if source_name == main_level_id {
                            trace!("Mixer main meter: rms={:?}, peak={:?}", rms, peak);

                            // Format: "block_id:meter:main" for main mix
                            let element_id = format!("{}:meter:main", instance_id);

                            events.broadcast(StromEvent::MeterData {
                                flow_id,
                                element_id,
                                rms,
                                peak,
                                decay,
                            });
                            return;
                        }

                        // Check if this is a channel level meter
                        if !source_name.starts_with(&level_prefix) {
                            return;
                        }

                        // Extract channel number from element name
                        // Format: "instance_id:level_N" -> extract N
                        let channel_str = source_name.strip_prefix(&level_prefix).unwrap_or("0");
                        let channel_num: usize = channel_str.parse().unwrap_or(0) + 1;

                        trace!(
                            "Mixer meter ch{}: rms={:?}, peak={:?}",
                            channel_num,
                            rms,
                            peak
                        );

                        // Use element_id format that frontend can parse
                        // Format: "block_id:meter:N" for channel N
                        let element_id = format!("{}:meter:{}", instance_id, channel_num);

                        events.broadcast(StromEvent::MeterData {
                            flow_id,
                            element_id,
                            rms,
                            peak,
                            decay,
                        });
                    }
                }
            }
        }
    })
}

/// Get metadata for Mixer block (for UI/API).
pub fn get_blocks() -> Vec<BlockDefinition> {
    vec![mixer_definition()]
}

/// Get Mixer block definition (metadata only).
fn mixer_definition() -> BlockDefinition {
    // Generate channel properties
    let mut exposed_properties = vec![
        // Global: number of channels
        ExposedProperty {
            name: "num_channels".to_string(),
            label: "Channels".to_string(),
            description: "Number of input channels".to_string(),
            property_type: PropertyType::Enum {
                values: vec![
                    EnumValue {
                        value: "2".to_string(),
                        label: Some("2".to_string()),
                    },
                    EnumValue {
                        value: "4".to_string(),
                        label: Some("4".to_string()),
                    },
                    EnumValue {
                        value: "8".to_string(),
                        label: Some("8".to_string()),
                    },
                    EnumValue {
                        value: "12".to_string(),
                        label: Some("12".to_string()),
                    },
                    EnumValue {
                        value: "16".to_string(),
                        label: Some("16".to_string()),
                    },
                    EnumValue {
                        value: "24".to_string(),
                        label: Some("24".to_string()),
                    },
                    EnumValue {
                        value: "32".to_string(),
                        label: Some("32".to_string()),
                    },
                ],
            },
            default_value: Some(PropertyValue::String("8".to_string())),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: "num_channels".to_string(),
                transform: None,
            },
        },
        // Main fader
        ExposedProperty {
            name: "main_fader".to_string(),
            label: "Main Fader".to_string(),
            description: "Main output level (0.0 to 2.0)".to_string(),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(1.0)),
            mapping: PropertyMapping {
                element_id: "main_volume".to_string(),
                property_name: "volume".to_string(),
                transform: None,
            },
        },
    ];

    // Add per-channel properties (we'll generate for max channels, UI will show based on num_channels)
    for ch in 1..=MAX_CHANNELS {
        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_pan", ch),
            label: format!("Ch {} Pan", ch),
            description: format!("Channel {} pan (-1.0=L, 0.0=C, 1.0=R)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(0.0)),
            mapping: PropertyMapping {
                element_id: format!("pan_{}", ch - 1),
                property_name: "panorama".to_string(),
                transform: None,
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_fader", ch),
            label: format!("Ch {} Fader", ch),
            description: format!("Channel {} volume (0.0 to 2.0)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(0.75)),
            mapping: PropertyMapping {
                element_id: format!("volume_{}", ch - 1),
                property_name: "volume".to_string(),
                transform: None,
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_mute", ch),
            label: format!("Ch {} Mute", ch),
            description: format!("Mute channel {}", ch),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(false)),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: format!("ch{}_mute", ch),
                transform: None,
            },
        });

        // ============================================================
        // Gate properties
        // ============================================================
        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_gate_enabled", ch),
            label: format!("Ch {} Gate", ch),
            description: format!("Enable gate on channel {}", ch),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(false)),
            mapping: PropertyMapping {
                element_id: format!("gate_{}", ch - 1),
                property_name: "enabled".to_string(),
                transform: None,
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_gate_threshold", ch),
            label: format!("Ch {} Gate Thresh", ch),
            description: format!("Channel {} gate threshold in dB (-60 to 0)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(-40.0)),
            mapping: PropertyMapping {
                element_id: format!("gate_{}", ch - 1),
                property_name: "gt".to_string(),
                transform: Some("db_to_linear".to_string()),
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_gate_attack", ch),
            label: format!("Ch {} Gate Atk", ch),
            description: format!("Channel {} gate attack in ms (0-200)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(5.0)),
            mapping: PropertyMapping {
                element_id: format!("gate_{}", ch - 1),
                property_name: "at".to_string(),
                transform: None,
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_gate_release", ch),
            label: format!("Ch {} Gate Rel", ch),
            description: format!("Channel {} gate release in ms (10-1000)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(100.0)),
            mapping: PropertyMapping {
                element_id: format!("gate_{}", ch - 1),
                property_name: "rt".to_string(),
                transform: None,
            },
        });

        // ============================================================
        // Compressor properties
        // ============================================================
        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_comp_enabled", ch),
            label: format!("Ch {} Comp", ch),
            description: format!("Enable compressor on channel {}", ch),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(false)),
            mapping: PropertyMapping {
                element_id: format!("comp_{}", ch - 1),
                property_name: "enabled".to_string(),
                transform: None,
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_comp_threshold", ch),
            label: format!("Ch {} Comp Thresh", ch),
            description: format!("Channel {} compressor threshold in dB (-60 to 0)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(-20.0)),
            mapping: PropertyMapping {
                element_id: format!("comp_{}", ch - 1),
                property_name: "al".to_string(),
                transform: Some("db_to_linear".to_string()),
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_comp_ratio", ch),
            label: format!("Ch {} Comp Ratio", ch),
            description: format!("Channel {} compressor ratio (1:1 to 20:1)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(4.0)),
            mapping: PropertyMapping {
                element_id: format!("comp_{}", ch - 1),
                property_name: "cr".to_string(),
                transform: None,
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_comp_attack", ch),
            label: format!("Ch {} Comp Atk", ch),
            description: format!("Channel {} compressor attack in ms (0-200)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(10.0)),
            mapping: PropertyMapping {
                element_id: format!("comp_{}", ch - 1),
                property_name: "at".to_string(),
                transform: None,
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_comp_release", ch),
            label: format!("Ch {} Comp Rel", ch),
            description: format!("Channel {} compressor release in ms (10-1000)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(100.0)),
            mapping: PropertyMapping {
                element_id: format!("comp_{}", ch - 1),
                property_name: "rt".to_string(),
                transform: None,
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_comp_makeup", ch),
            label: format!("Ch {} Comp Makeup", ch),
            description: format!("Channel {} compressor makeup gain in dB (0 to 24)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(0.0)),
            mapping: PropertyMapping {
                element_id: format!("comp_{}", ch - 1),
                property_name: "mk".to_string(),
                transform: Some("db_to_linear".to_string()),
            },
        });

        // ============================================================
        // EQ properties - 4 bands
        // ============================================================
        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_eq_enabled", ch),
            label: format!("Ch {} EQ", ch),
            description: format!("Enable parametric EQ on channel {}", ch),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(false)),
            mapping: PropertyMapping {
                element_id: format!("eq_{}", ch - 1),
                property_name: "enabled".to_string(),
                transform: None,
            },
        });

        // 4 EQ bands with default frequencies: 80Hz, 400Hz, 2kHz, 8kHz
        let eq_band_defaults = [
            (80.0, "Low"),
            (400.0, "Low-Mid"),
            (2000.0, "Hi-Mid"),
            (8000.0, "High"),
        ];
        for (band, (def_freq, band_name)) in eq_band_defaults.iter().enumerate() {
            let band_num = band + 1;

            exposed_properties.push(ExposedProperty {
                name: format!("ch{}_eq{}_freq", ch, band_num),
                label: format!("Ch {} EQ{} Freq", ch, band_num),
                description: format!(
                    "Channel {} EQ band {} ({}) frequency in Hz",
                    ch, band_num, band_name
                ),
                property_type: PropertyType::Float,
                default_value: Some(PropertyValue::Float(*def_freq)),
                mapping: PropertyMapping {
                    element_id: format!("eq_{}", ch - 1),
                    property_name: format!("f-{}", band),
                    transform: None,
                },
            });

            exposed_properties.push(ExposedProperty {
                name: format!("ch{}_eq{}_gain", ch, band_num),
                label: format!("Ch {} EQ{} Gain", ch, band_num),
                description: format!(
                    "Channel {} EQ band {} gain in dB (-15 to +15)",
                    ch, band_num
                ),
                property_type: PropertyType::Float,
                default_value: Some(PropertyValue::Float(0.0)),
                mapping: PropertyMapping {
                    element_id: format!("eq_{}", ch - 1),
                    property_name: format!("g-{}", band),
                    transform: Some("db_to_linear".to_string()),
                },
            });

            exposed_properties.push(ExposedProperty {
                name: format!("ch{}_eq{}_q", ch, band_num),
                label: format!("Ch {} EQ{} Q", ch, band_num),
                description: format!("Channel {} EQ band {} Q factor (0.1 to 10)", ch, band_num),
                property_type: PropertyType::Float,
                default_value: Some(PropertyValue::Float(1.0)),
                mapping: PropertyMapping {
                    element_id: format!("eq_{}", ch - 1),
                    property_name: format!("q-{}", band),
                    transform: None,
                },
            });
        }
    }

    BlockDefinition {
        id: "builtin.mixer".to_string(),
        name: "Audio Mixer".to_string(),
        description: "Stereo audio mixer with per-channel pan, fader, mute and metering. Combines multiple audio inputs into a single stereo output.".to_string(),
        category: "Audio".to_string(),
        exposed_properties,
        // External pads are computed dynamically based on num_channels
        external_pads: ExternalPads {
            inputs: (0..DEFAULT_CHANNELS)
                .map(|i| ExternalPad {
                    name: format!("input_{}", i + 1),
                    media_type: MediaType::Audio,
                    internal_element_id: format!("convert_{}", i),
                    internal_pad_name: "sink".to_string(),
                })
                .collect(),
            outputs: vec![ExternalPad {
                name: "main_out".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "main_level".to_string(),
                internal_pad_name: "src".to_string(),
            }],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("🎚".to_string()),
            width: Some(3.0),
            height: Some(4.0),
            ..Default::default()
        }),
    }
}
