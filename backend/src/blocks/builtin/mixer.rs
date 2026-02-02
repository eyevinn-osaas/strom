//! Stereo Mixer block - a digital mixing console for audio.
//!
//! This block provides a mixer similar to digital consoles like Behringer X32:
//! - Configurable number of input channels (1-32)
//! - Per-channel: gate, compressor, 4-band parametric EQ, pan, fader, mute
//! - Aux sends (0-4 configurable aux buses)
//! - Subgroups (0-4 configurable)
//! - PFL (Pre-Fader Listen) bus
//! - Main stereo bus with audiomixer
//! - Per-channel and bus metering
//!
//! Pipeline structure per channel:
//! ```text
//! input_N → audioconvert → capsfilter(F32LE) → gate → compressor → EQ →
//!           pre_fader_tee → audiopanorama_N → volume_N → post_fader_tee →
//!           level_N → [subgroup or main audiomixer]
//!
//! pre_fader_tee → pfl_volume_N → pfl_queue_N → pfl_mixer
//! post_fader_tee → aux_send_N_M → aux_queue_N_M → aux_M_mixer
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
/// Maximum number of aux buses
const MAX_AUX_BUSES: usize = 4;
/// Maximum number of subgroups
const MAX_SUBGROUPS: usize = 4;

/// Mixer block builder.
pub struct MixerBuilder;

impl BlockBuilder for MixerBuilder {
    fn get_external_pads(
        &self,
        properties: &HashMap<String, PropertyValue>,
    ) -> Option<ExternalPads> {
        let num_channels = parse_num_channels(properties);
        let num_aux_buses = parse_num_aux_buses(properties);

        // Create input pads dynamically
        let inputs = (0..num_channels)
            .map(|i| ExternalPad {
                name: format!("input_{}", i + 1),
                label: Some(format!("{}", i + 1)),
                media_type: MediaType::Audio,
                internal_element_id: format!("convert_{}", i),
                internal_pad_name: "sink".to_string(),
            })
            .collect();

        // Output pads
        let mut outputs = vec![
            // Main stereo output
            ExternalPad {
                name: "main_out".to_string(),
                label: Some("Main".to_string()),
                media_type: MediaType::Audio,
                internal_element_id: "main_level".to_string(),
                internal_pad_name: "src".to_string(),
            },
            // PFL output (always present)
            ExternalPad {
                name: "pfl_out".to_string(),
                label: Some("PFL".to_string()),
                media_type: MediaType::Audio,
                internal_element_id: "pfl_level".to_string(),
                internal_pad_name: "src".to_string(),
            },
        ];

        // Add aux outputs
        for aux in 0..num_aux_buses {
            outputs.push(ExternalPad {
                name: format!("aux_out_{}", aux + 1),
                label: Some(format!("Aux{}", aux + 1)),
                media_type: MediaType::Audio,
                internal_element_id: format!("aux{}_level", aux),
                internal_pad_name: "src".to_string(),
            });
        }

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
        let num_aux_buses = parse_num_aux_buses(properties);
        let num_subgroups = parse_num_subgroups(properties);
        info!(
            "Mixer config: {} channels, {} aux buses, {} subgroups",
            num_channels, num_aux_buses, num_subgroups
        );

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
        // Create PFL (Pre-Fader Listen) bus
        // ========================================================================
        let pfl_mixer_id = format!("{}:pfl_mixer", instance_id);
        let pfl_mixer = gst::ElementFactory::make("audiomixer")
            .name(&pfl_mixer_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("pfl_mixer: {}", e)))?;
        elements.push((pfl_mixer_id.clone(), pfl_mixer));

        let pfl_level_id = format!("{}:pfl_level", instance_id);
        let pfl_level = gst::ElementFactory::make("level")
            .name(&pfl_level_id)
            .property("interval", 100_000_000u64)
            .property("post-messages", true)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("pfl_level: {}", e)))?;
        elements.push((pfl_level_id.clone(), pfl_level));

        // Link: pfl_mixer → pfl_level
        internal_links.push((
            ElementPadRef::pad(&pfl_mixer_id, "src"),
            ElementPadRef::pad(&pfl_level_id, "sink"),
        ));

        // ========================================================================
        // Create Aux buses
        // ========================================================================
        for aux in 0..num_aux_buses {
            let aux_mixer_id = format!("{}:aux{}_mixer", instance_id, aux);
            let aux_mixer = gst::ElementFactory::make("audiomixer")
                .name(&aux_mixer_id)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("aux{}_mixer: {}", aux, e))
                })?;
            elements.push((aux_mixer_id.clone(), aux_mixer));

            let aux_fader = get_float_prop(properties, &format!("aux{}_fader", aux + 1), 1.0);
            let aux_mute = get_bool_prop(properties, &format!("aux{}_mute", aux + 1), false);
            let aux_volume_val = if aux_mute { 0.0 } else { aux_fader };

            let aux_volume_id = format!("{}:aux{}_volume", instance_id, aux);
            let aux_volume = gst::ElementFactory::make("volume")
                .name(&aux_volume_id)
                .property("volume", aux_volume_val)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("aux{}_volume: {}", aux, e))
                })?;
            elements.push((aux_volume_id.clone(), aux_volume));

            let aux_level_id = format!("{}:aux{}_level", instance_id, aux);
            let aux_level = gst::ElementFactory::make("level")
                .name(&aux_level_id)
                .property("interval", 100_000_000u64)
                .property("post-messages", true)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("aux{}_level: {}", aux, e))
                })?;
            elements.push((aux_level_id.clone(), aux_level));

            // Link: aux_mixer → aux_volume → aux_level
            internal_links.push((
                ElementPadRef::pad(&aux_mixer_id, "src"),
                ElementPadRef::pad(&aux_volume_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&aux_volume_id, "src"),
                ElementPadRef::pad(&aux_level_id, "sink"),
            ));
        }

        // ========================================================================
        // Create Subgroups
        // ========================================================================
        for sg in 0..num_subgroups {
            let sg_mixer_id = format!("{}:subgroup{}_mixer", instance_id, sg);
            let sg_mixer = gst::ElementFactory::make("audiomixer")
                .name(&sg_mixer_id)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("subgroup{}_mixer: {}", sg, e))
                })?;
            elements.push((sg_mixer_id.clone(), sg_mixer));

            let sg_fader = get_float_prop(properties, &format!("subgroup{}_fader", sg + 1), 1.0);
            let sg_mute = get_bool_prop(properties, &format!("subgroup{}_mute", sg + 1), false);
            let sg_volume_val = if sg_mute { 0.0 } else { sg_fader };

            let sg_volume_id = format!("{}:subgroup{}_volume", instance_id, sg);
            let sg_volume = gst::ElementFactory::make("volume")
                .name(&sg_volume_id)
                .property("volume", sg_volume_val)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("subgroup{}_volume: {}", sg, e))
                })?;
            elements.push((sg_volume_id.clone(), sg_volume));

            let sg_level_id = format!("{}:subgroup{}_level", instance_id, sg);
            let sg_level = gst::ElementFactory::make("level")
                .name(&sg_level_id)
                .property("interval", 100_000_000u64)
                .property("post-messages", true)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("subgroup{}_level: {}", sg, e))
                })?;
            elements.push((sg_level_id.clone(), sg_level));

            // Link: subgroup_mixer → subgroup_volume → subgroup_level → main audiomixer
            internal_links.push((
                ElementPadRef::pad(&sg_mixer_id, "src"),
                ElementPadRef::pad(&sg_volume_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&sg_volume_id, "src"),
                ElementPadRef::pad(&sg_level_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&sg_level_id, "src"),
                ElementPadRef::element(&mixer_id), // Request pad from main audiomixer
            ));
        }

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

            // ----------------------------------------------------------------
            // Pre-fader tee (for PFL tap - after EQ, before pan)
            // ----------------------------------------------------------------
            let pre_fader_tee_id = format!("{}:pre_fader_tee_{}", instance_id, ch);
            let pre_fader_tee = gst::ElementFactory::make("tee")
                .name(&pre_fader_tee_id)
                .property("allow-not-linked", true)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("pre_fader_tee ch{}: {}", ch_num, e))
                })?;
            elements.push((pre_fader_tee_id.clone(), pre_fader_tee));

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

            // ----------------------------------------------------------------
            // Post-fader tee (for aux sends - after volume, before level)
            // ----------------------------------------------------------------
            let post_fader_tee_id = format!("{}:post_fader_tee_{}", instance_id, ch);
            let post_fader_tee = gst::ElementFactory::make("tee")
                .name(&post_fader_tee_id)
                .property("allow-not-linked", true)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("post_fader_tee ch{}: {}", ch_num, e))
                })?;
            elements.push((post_fader_tee_id.clone(), post_fader_tee));

            // level (metering)
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

            // ----------------------------------------------------------------
            // Routing tee (after level, for multi-destination routing)
            // ----------------------------------------------------------------
            let routing_tee_id = format!("{}:routing_tee_{}", instance_id, ch);
            let routing_tee = gst::ElementFactory::make("tee")
                .name(&routing_tee_id)
                .property("allow-not-linked", true)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("routing_tee ch{}: {}", ch_num, e))
                })?;
            elements.push((routing_tee_id.clone(), routing_tee));

            // ----------------------------------------------------------------
            // PFL path (pre-fader listen)
            // ----------------------------------------------------------------
            let pfl_enabled = get_bool_prop(properties, &format!("ch{}_pfl", ch_num), false);

            let pfl_volume_id = format!("{}:pfl_volume_{}", instance_id, ch);
            let pfl_volume = gst::ElementFactory::make("volume")
                .name(&pfl_volume_id)
                .property("volume", if pfl_enabled { 1.0 } else { 0.0 })
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("pfl_volume ch{}: {}", ch_num, e))
                })?;
            elements.push((pfl_volume_id.clone(), pfl_volume));

            let pfl_queue_id = format!("{}:pfl_queue_{}", instance_id, ch);
            let pfl_queue = gst::ElementFactory::make("queue")
                .name(&pfl_queue_id)
                .property("max-size-buffers", 3u32)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("pfl_queue ch{}: {}", ch_num, e))
                })?;
            elements.push((pfl_queue_id.clone(), pfl_queue));

            // ----------------------------------------------------------------
            // Aux send paths (post-fader)
            // ----------------------------------------------------------------
            for aux in 0..num_aux_buses {
                let aux_send_level = get_float_prop(
                    properties,
                    &format!("ch{}_aux{}_level", ch_num, aux + 1),
                    0.0,
                );

                let aux_send_id = format!("{}:aux_send_{}_{}", instance_id, ch, aux);
                let aux_send = gst::ElementFactory::make("volume")
                    .name(&aux_send_id)
                    .property("volume", aux_send_level)
                    .build()
                    .map_err(|e| {
                        BlockBuildError::ElementCreation(format!(
                            "aux_send ch{} aux{}: {}",
                            ch_num,
                            aux + 1,
                            e
                        ))
                    })?;
                elements.push((aux_send_id.clone(), aux_send));

                let aux_queue_id = format!("{}:aux_queue_{}_{}", instance_id, ch, aux);
                let aux_queue = gst::ElementFactory::make("queue")
                    .name(&aux_queue_id)
                    .property("max-size-buffers", 3u32)
                    .build()
                    .map_err(|e| {
                        BlockBuildError::ElementCreation(format!(
                            "aux_queue ch{} aux{}: {}",
                            ch_num,
                            aux + 1,
                            e
                        ))
                    })?;
                elements.push((aux_queue_id.clone(), aux_queue));

                // Link: post_fader_tee → aux_send → aux_queue → aux_mixer
                let aux_mixer_id = format!("{}:aux{}_mixer", instance_id, aux);
                internal_links.push((
                    ElementPadRef::element(&post_fader_tee_id), // Request pad from tee
                    ElementPadRef::pad(&aux_send_id, "sink"),
                ));
                internal_links.push((
                    ElementPadRef::pad(&aux_send_id, "src"),
                    ElementPadRef::pad(&aux_queue_id, "sink"),
                ));
                internal_links.push((
                    ElementPadRef::pad(&aux_queue_id, "src"),
                    ElementPadRef::element(&aux_mixer_id), // Request pad from aux_mixer
                ));
            }

            // ----------------------------------------------------------------
            // Main chain links
            // ----------------------------------------------------------------
            // Chain: convert → caps → gate → comp → eq → pre_fader_tee
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
                ElementPadRef::pad(&pre_fader_tee_id, "sink"),
            ));

            // pre_fader_tee → pan → volume → post_fader_tee → level → routing_tee
            internal_links.push((
                ElementPadRef::element(&pre_fader_tee_id), // Request pad from tee
                ElementPadRef::pad(&pan_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&pan_id, "src"),
                ElementPadRef::pad(&volume_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&volume_id, "src"),
                ElementPadRef::pad(&post_fader_tee_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::element(&post_fader_tee_id), // Request pad from tee
                ElementPadRef::pad(&level_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&level_id, "src"),
                ElementPadRef::pad(&routing_tee_id, "sink"),
            ));

            // PFL path: pre_fader_tee → pfl_volume → pfl_queue → pfl_mixer
            internal_links.push((
                ElementPadRef::element(&pre_fader_tee_id), // Request pad from tee
                ElementPadRef::pad(&pfl_volume_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&pfl_volume_id, "src"),
                ElementPadRef::pad(&pfl_queue_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&pfl_queue_id, "src"),
                ElementPadRef::element(&pfl_mixer_id), // Request pad from pfl_mixer
            ));

            // ----------------------------------------------------------------
            // Multi-destination routing (Main + Subgroups)
            // Each destination has a volume element to enable/disable routing
            // ----------------------------------------------------------------

            // Route to main mixer
            let to_main_enabled = get_bool_prop(properties, &format!("ch{}_to_main", ch_num), true);
            let to_main_vol_id = format!("{}:to_main_vol_{}", instance_id, ch);
            let to_main_vol = gst::ElementFactory::make("volume")
                .name(&to_main_vol_id)
                .property("volume", if to_main_enabled { 1.0 } else { 0.0 })
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("to_main_vol ch{}: {}", ch_num, e))
                })?;
            elements.push((to_main_vol_id.clone(), to_main_vol));

            let to_main_queue_id = format!("{}:to_main_queue_{}", instance_id, ch);
            let to_main_queue = gst::ElementFactory::make("queue")
                .name(&to_main_queue_id)
                .property("max-size-buffers", 3u32)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("to_main_queue ch{}: {}", ch_num, e))
                })?;
            elements.push((to_main_queue_id.clone(), to_main_queue));

            // Link: routing_tee → to_main_vol → to_main_queue → main_mixer
            internal_links.push((
                ElementPadRef::element(&routing_tee_id),
                ElementPadRef::pad(&to_main_vol_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&to_main_vol_id, "src"),
                ElementPadRef::pad(&to_main_queue_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&to_main_queue_id, "src"),
                ElementPadRef::element(&mixer_id),
            ));

            // Route to subgroups
            for sg in 0..num_subgroups {
                let to_sg_enabled =
                    get_bool_prop(properties, &format!("ch{}_to_sg{}", ch_num, sg + 1), false);

                let to_sg_vol_id = format!("{}:to_sg{}_vol_{}", instance_id, sg, ch);
                let to_sg_vol = gst::ElementFactory::make("volume")
                    .name(&to_sg_vol_id)
                    .property("volume", if to_sg_enabled { 1.0 } else { 0.0 })
                    .build()
                    .map_err(|e| {
                        BlockBuildError::ElementCreation(format!(
                            "to_sg{}_vol ch{}: {}",
                            sg + 1,
                            ch_num,
                            e
                        ))
                    })?;
                elements.push((to_sg_vol_id.clone(), to_sg_vol));

                let to_sg_queue_id = format!("{}:to_sg{}_queue_{}", instance_id, sg, ch);
                let to_sg_queue = gst::ElementFactory::make("queue")
                    .name(&to_sg_queue_id)
                    .property("max-size-buffers", 3u32)
                    .build()
                    .map_err(|e| {
                        BlockBuildError::ElementCreation(format!(
                            "to_sg{}_queue ch{}: {}",
                            sg + 1,
                            ch_num,
                            e
                        ))
                    })?;
                elements.push((to_sg_queue_id.clone(), to_sg_queue));

                // Link: routing_tee → to_sg_vol → to_sg_queue → subgroup_mixer
                let sg_mixer_id = format!("{}:subgroup{}_mixer", instance_id, sg);
                internal_links.push((
                    ElementPadRef::element(&routing_tee_id),
                    ElementPadRef::pad(&to_sg_vol_id, "sink"),
                ));
                internal_links.push((
                    ElementPadRef::pad(&to_sg_vol_id, "src"),
                    ElementPadRef::pad(&to_sg_queue_id, "sink"),
                ));
                internal_links.push((
                    ElementPadRef::pad(&to_sg_queue_id, "src"),
                    ElementPadRef::element(&sg_mixer_id),
                ));

                if to_sg_enabled {
                    debug!("Channel {} routed to subgroup {}", ch_num, sg + 1);
                }
            }

            debug!(
                "Channel {} created: pan={}, fader={}, mute={}, pfl={}, to_main={}",
                ch_num, pan, fader, mute, pfl_enabled, to_main_enabled
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

/// Parse number of aux buses from properties.
fn parse_num_aux_buses(properties: &HashMap<String, PropertyValue>) -> usize {
    properties
        .get("num_aux_buses")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as usize),
            PropertyValue::UInt(u) => Some(*u as usize),
            PropertyValue::String(s) => s.parse::<usize>().ok(),
            _ => None,
        })
        .unwrap_or(0)
        .clamp(0, MAX_AUX_BUSES)
}

/// Parse number of subgroups from properties.
fn parse_num_subgroups(properties: &HashMap<String, PropertyValue>) -> usize {
    properties
        .get("num_subgroups")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as usize),
            PropertyValue::UInt(u) => Some(*u as usize),
            PropertyValue::String(s) => s.parse::<usize>().ok(),
            _ => None,
        })
        .unwrap_or(0)
        .clamp(0, MAX_SUBGROUPS)
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
    let pfl_level_id = format!("{}:pfl_level", instance_id);
    let aux_level_prefix = format!("{}:aux", instance_id);
    let subgroup_level_prefix = format!("{}:subgroup", instance_id);

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

                        // Check if this is the PFL level meter
                        if source_name == pfl_level_id {
                            trace!("Mixer PFL meter: rms={:?}, peak={:?}", rms, peak);
                            let element_id = format!("{}:meter:pfl", instance_id);
                            events.broadcast(StromEvent::MeterData {
                                flow_id,
                                element_id,
                                rms,
                                peak,
                                decay,
                            });
                            return;
                        }

                        // Check if this is an aux level meter
                        // Format: "instance_id:auxN_level"
                        if source_name.starts_with(&aux_level_prefix)
                            && source_name.contains("_level")
                        {
                            // Extract aux number from "auxN_level"
                            if let Some(aux_part) =
                                source_name.strip_prefix(&format!("{}:aux", instance_id))
                            {
                                if let Some(aux_num_str) = aux_part.strip_suffix("_level") {
                                    if let Ok(aux_num) = aux_num_str.parse::<usize>() {
                                        trace!(
                                            "Mixer aux{} meter: rms={:?}, peak={:?}",
                                            aux_num + 1,
                                            rms,
                                            peak
                                        );
                                        let element_id =
                                            format!("{}:meter:aux{}", instance_id, aux_num + 1);
                                        events.broadcast(StromEvent::MeterData {
                                            flow_id,
                                            element_id,
                                            rms,
                                            peak,
                                            decay,
                                        });
                                        return;
                                    }
                                }
                            }
                        }

                        // Check if this is a subgroup level meter
                        // Format: "instance_id:subgroupN_level"
                        if source_name.starts_with(&subgroup_level_prefix)
                            && source_name.contains("_level")
                        {
                            if let Some(sg_part) =
                                source_name.strip_prefix(&format!("{}:subgroup", instance_id))
                            {
                                if let Some(sg_num_str) = sg_part.strip_suffix("_level") {
                                    if let Ok(sg_num) = sg_num_str.parse::<usize>() {
                                        trace!(
                                            "Mixer subgroup{} meter: rms={:?}, peak={:?}",
                                            sg_num + 1,
                                            rms,
                                            peak
                                        );
                                        let element_id =
                                            format!("{}:meter:subgroup{}", instance_id, sg_num + 1);
                                        events.broadcast(StromEvent::MeterData {
                                            flow_id,
                                            element_id,
                                            rms,
                                            peak,
                                            decay,
                                        });
                                        return;
                                    }
                                }
                            }
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
        // Number of aux buses
        ExposedProperty {
            name: "num_aux_buses".to_string(),
            label: "Aux Buses".to_string(),
            description: "Number of aux send buses (0-4)".to_string(),
            property_type: PropertyType::Enum {
                values: vec![
                    EnumValue {
                        value: "0".to_string(),
                        label: Some("None".to_string()),
                    },
                    EnumValue {
                        value: "1".to_string(),
                        label: Some("1".to_string()),
                    },
                    EnumValue {
                        value: "2".to_string(),
                        label: Some("2".to_string()),
                    },
                    EnumValue {
                        value: "3".to_string(),
                        label: Some("3".to_string()),
                    },
                    EnumValue {
                        value: "4".to_string(),
                        label: Some("4".to_string()),
                    },
                ],
            },
            default_value: Some(PropertyValue::String("0".to_string())),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: "num_aux_buses".to_string(),
                transform: None,
            },
        },
        // Number of subgroups
        ExposedProperty {
            name: "num_subgroups".to_string(),
            label: "Subgroups".to_string(),
            description: "Number of subgroup buses (0-4)".to_string(),
            property_type: PropertyType::Enum {
                values: vec![
                    EnumValue {
                        value: "0".to_string(),
                        label: Some("None".to_string()),
                    },
                    EnumValue {
                        value: "1".to_string(),
                        label: Some("1".to_string()),
                    },
                    EnumValue {
                        value: "2".to_string(),
                        label: Some("2".to_string()),
                    },
                    EnumValue {
                        value: "3".to_string(),
                        label: Some("3".to_string()),
                    },
                    EnumValue {
                        value: "4".to_string(),
                        label: Some("4".to_string()),
                    },
                ],
            },
            default_value: Some(PropertyValue::String("0".to_string())),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: "num_subgroups".to_string(),
                transform: None,
            },
        },
    ];

    // Add aux bus master properties
    for aux in 1..=MAX_AUX_BUSES {
        exposed_properties.push(ExposedProperty {
            name: format!("aux{}_fader", aux),
            label: format!("Aux {} Fader", aux),
            description: format!("Aux bus {} master level (0.0 to 2.0)", aux),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(1.0)),
            mapping: PropertyMapping {
                element_id: format!("aux{}_volume", aux - 1),
                property_name: "volume".to_string(),
                transform: None,
            },
        });
        exposed_properties.push(ExposedProperty {
            name: format!("aux{}_mute", aux),
            label: format!("Aux {} Mute", aux),
            description: format!("Mute aux bus {}", aux),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(false)),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: format!("aux{}_mute", aux),
                transform: None,
            },
        });
    }

    // Add subgroup properties
    for sg in 1..=MAX_SUBGROUPS {
        exposed_properties.push(ExposedProperty {
            name: format!("subgroup{}_fader", sg),
            label: format!("Subgroup {} Fader", sg),
            description: format!("Subgroup {} level (0.0 to 2.0)", sg),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(1.0)),
            mapping: PropertyMapping {
                element_id: format!("subgroup{}_volume", sg - 1),
                property_name: "volume".to_string(),
                transform: None,
            },
        });
        exposed_properties.push(ExposedProperty {
            name: format!("subgroup{}_mute", sg),
            label: format!("Subgroup {} Mute", sg),
            description: format!("Mute subgroup {}", sg),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(false)),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: format!("subgroup{}_mute", sg),
                transform: None,
            },
        });
    }

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

        // PFL (Pre-Fader Listen)
        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_pfl", ch),
            label: format!("Ch {} PFL", ch),
            description: format!("Enable PFL (Pre-Fader Listen) on channel {}", ch),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(false)),
            mapping: PropertyMapping {
                element_id: format!("pfl_volume_{}", ch - 1),
                property_name: "volume".to_string(),
                transform: Some("bool_to_volume".to_string()),
            },
        });

        // Routing to main
        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_to_main", ch),
            label: format!("Ch {} → Main", ch),
            description: format!("Route channel {} to main mix", ch),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(true)),
            mapping: PropertyMapping {
                element_id: format!("to_main_vol_{}", ch - 1),
                property_name: "volume".to_string(),
                transform: Some("bool_to_volume".to_string()),
            },
        });

        // Routing to subgroups
        for sg in 1..=MAX_SUBGROUPS {
            exposed_properties.push(ExposedProperty {
                name: format!("ch{}_to_sg{}", ch, sg),
                label: format!("Ch {} → SG{}", ch, sg),
                description: format!("Route channel {} to subgroup {}", ch, sg),
                property_type: PropertyType::Bool,
                default_value: Some(PropertyValue::Bool(false)),
                mapping: PropertyMapping {
                    element_id: format!("to_sg{}_vol_{}", sg - 1, ch - 1),
                    property_name: "volume".to_string(),
                    transform: Some("bool_to_volume".to_string()),
                },
            });
        }

        // Aux send levels (per aux bus)
        for aux in 1..=MAX_AUX_BUSES {
            exposed_properties.push(ExposedProperty {
                name: format!("ch{}_aux{}_level", ch, aux),
                label: format!("Ch {} Aux {} Send", ch, aux),
                description: format!("Channel {} send level to aux bus {} (0.0 to 2.0)", ch, aux),
                property_type: PropertyType::Float,
                default_value: Some(PropertyValue::Float(0.0)),
                mapping: PropertyMapping {
                    element_id: format!("aux_send_{}_{}", ch - 1, aux - 1),
                    property_name: "volume".to_string(),
                    transform: None,
                },
            });
        }

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
        // (this is the default, get_external_pads() provides dynamic version)
        external_pads: ExternalPads {
            inputs: (0..DEFAULT_CHANNELS)
                .map(|i| ExternalPad {
                    name: format!("input_{}", i + 1),
                    label: Some(format!("{}", i + 1)),
                    media_type: MediaType::Audio,
                    internal_element_id: format!("convert_{}", i),
                    internal_pad_name: "sink".to_string(),
                })
                .collect(),
            outputs: vec![
                ExternalPad {
                    name: "main_out".to_string(),
                    label: Some("Main".to_string()),
                    media_type: MediaType::Audio,
                    internal_element_id: "main_level".to_string(),
                    internal_pad_name: "src".to_string(),
                },
                ExternalPad {
                    name: "pfl_out".to_string(),
                    label: Some("PFL".to_string()),
                    media_type: MediaType::Audio,
                    internal_element_id: "pfl_level".to_string(),
                    internal_pad_name: "src".to_string(),
                },
            ],
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
