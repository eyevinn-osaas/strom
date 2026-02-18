//! Stereo Mixer block - a digital mixing console for audio.
//!
//! This block provides a mixer similar to digital consoles like Behringer X32:
//! - Configurable number of input channels (1-32)
//! - Per-channel: input gain, gate, compressor, 4-band parametric EQ, pan, fader, mute
//! - Aux sends (0-4 configurable aux buses, switchable pre/post fader)
//! - Groups (0-4 configurable, with output pads)
//! - PFL (Pre-Fader Listen) bus with master level
//! - Main stereo bus with compressor, EQ, limiter, and master fader
//! - Per-channel and bus metering
//!
//! Pipeline structure per channel:
//! ```text
//! input_N → audioconvert → capsfilter(F32LE) → gain → hpf → gate → compressor → EQ →
//!           pre_fader_tee → audiopanorama_N → volume_N → post_fader_tee →
//!           level_N → [group or main audiomixer]
//!
//! (pre_fader_tee | post_fader_tee) → solo_volume_N → solo_queue_N → pfl_mixer
//!   (source depends on solo_mode: pfl=pre-fader, afl=post-fader)
//! (pre_fader_tee | post_fader_tee) → aux_send_N_M → aux_queue_N_M → aux_M_mixer
//! ```
//!
//! Main bus: audiomixer → main_comp → main_eq → main_limiter → main_volume → main_level → main_out_tee
//!
//! All output buses terminate in a tee with allow-not-linked=true, so unconnected
//! output pads don't cause NOT_LINKED flow errors. Audiomixer elements use
//! force-live=true so unconnected input pads don't stall the pipeline.
//!
//! Processing uses LSP LV2 plugins when available. Falls back to identity passthrough
//! when LV2 plugins are not installed.

use crate::blocks::{BlockBuildContext, BlockBuildError, BlockBuildResult, BlockBuilder};
use crate::events::EventBroadcaster;
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use strom_types::{
    block::*, element::ElementPadRef, EnumValue, FlowId, MediaType, PropertyValue, StromEvent,
};
use tracing::{debug, info, trace, warn};

/// Maximum number of input channels
const MAX_CHANNELS: usize = 32;
/// Default number of channels
const DEFAULT_CHANNELS: usize = 8;
/// Maximum number of aux buses
const MAX_AUX_BUSES: usize = 4;
/// Maximum number of groups
const MAX_GROUPS: usize = 4;

/// Mixer block builder.
pub struct MixerBuilder;

impl BlockBuilder for MixerBuilder {
    fn get_external_pads(
        &self,
        properties: &HashMap<String, PropertyValue>,
    ) -> Option<ExternalPads> {
        let num_channels = parse_num_channels(properties);
        let num_aux_buses = parse_num_aux_buses(properties);
        let num_groups = parse_num_groups(properties);
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

        // Output pads - point to output tees so unconnected outputs don't cause
        // NOT_LINKED flow errors. Each output tee has allow-not-linked=true.
        let mut outputs = vec![
            // Main stereo output
            ExternalPad {
                name: "main_out".to_string(),
                label: Some("Main".to_string()),
                media_type: MediaType::Audio,
                internal_element_id: "main_out_tee".to_string(),
                internal_pad_name: "src_%u".to_string(),
            },
            // PFL output (always present)
            ExternalPad {
                name: "pfl_out".to_string(),
                label: Some("PFL".to_string()),
                media_type: MediaType::Audio,
                internal_element_id: "pfl_out_tee".to_string(),
                internal_pad_name: "src_%u".to_string(),
            },
        ];

        // Add aux outputs
        for aux in 0..num_aux_buses {
            outputs.push(ExternalPad {
                name: format!("aux_out_{}", aux + 1),
                label: Some(format!("Aux{}", aux + 1)),
                media_type: MediaType::Audio,
                internal_element_id: format!("aux{}_out_tee", aux),
                internal_pad_name: "src_%u".to_string(),
            });
        }

        // Add group outputs
        for sg in 0..num_groups {
            outputs.push(ExternalPad {
                name: format!("group_out_{}", sg + 1),
                label: Some(format!("Grp{}", sg + 1)),
                media_type: MediaType::Audio,
                internal_element_id: format!("group{}_out_tee", sg),
                internal_pad_name: "src_%u".to_string(),
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
        let num_groups = parse_num_groups(properties);
        let dsp_backend = get_string_prop(properties, "dsp_backend", "lv2");
        let solo_mode_afl = get_string_prop(properties, "solo_mode", "pfl") == "afl";
        info!(
            "Mixer config: {} channels, {} aux buses, {} groups, solo={}, dsp={}",
            num_channels,
            num_aux_buses,
            num_groups,
            if solo_mode_afl { "afl" } else { "pfl" },
            dsp_backend,
        );

        let mut elements = Vec::new();
        let mut internal_links = Vec::new();

        // Mixer aggregator settings
        let force_live = get_bool_prop(properties, "force_live", true);
        let latency_ms = get_float_prop(properties, "latency", 30.0) as u64;
        let min_upstream_latency_ms =
            get_float_prop(properties, "min_upstream_latency", 30.0) as u64;

        // ========================================================================
        // Create main audiomixer
        // ========================================================================
        let mixer_id = format!("{}:audiomixer", instance_id);
        let audiomixer =
            make_audiomixer(&mixer_id, force_live, latency_ms, min_upstream_latency_ms)?;
        elements.push((mixer_id.clone(), audiomixer.clone()));

        // ========================================================================
        // Main bus processing: comp → EQ → limiter
        // ========================================================================
        let main_comp_enabled = get_bool_prop(properties, "main_comp_enabled", false);
        let main_comp_threshold = get_float_prop(properties, "main_comp_threshold", -20.0);
        let main_comp_ratio = get_float_prop(properties, "main_comp_ratio", 4.0);
        let main_comp_attack = get_float_prop(properties, "main_comp_attack", 10.0);
        let main_comp_release = get_float_prop(properties, "main_comp_release", 100.0);
        let main_comp_makeup = get_float_prop(properties, "main_comp_makeup", 0.0);
        let main_comp_knee = get_float_prop(properties, "main_comp_knee", -6.0);

        let main_comp_id = format!("{}:main_comp", instance_id);
        let main_comp = make_compressor_element(
            &main_comp_id,
            main_comp_enabled,
            main_comp_threshold,
            main_comp_ratio,
            main_comp_attack,
            main_comp_release,
            main_comp_makeup,
            dsp_backend,
        )?;
        // Set knee - Rust backend uses "knee" (linear), LV2 uses "kn" (linear)
        if main_comp.find_property("knee").is_some() {
            let kn_val = db_to_linear(main_comp_knee).clamp(0.0631, 1.0) as f32;
            main_comp.set_property("knee", kn_val);
        } else if main_comp.find_property("kn").is_some() {
            let kn_val = db_to_linear(main_comp_knee).clamp(0.0631, 1.0) as f32;
            main_comp.set_property("kn", kn_val);
        }
        elements.push((main_comp_id.clone(), main_comp));

        let main_eq_enabled = get_bool_prop(properties, "main_eq_enabled", false);
        let main_eq_bands = [
            (
                get_float_prop(properties, "main_eq1_freq", 80.0),
                get_float_prop(properties, "main_eq1_gain", 0.0),
                get_float_prop(properties, "main_eq1_q", 1.0),
            ),
            (
                get_float_prop(properties, "main_eq2_freq", 400.0),
                get_float_prop(properties, "main_eq2_gain", 0.0),
                get_float_prop(properties, "main_eq2_q", 1.0),
            ),
            (
                get_float_prop(properties, "main_eq3_freq", 2000.0),
                get_float_prop(properties, "main_eq3_gain", 0.0),
                get_float_prop(properties, "main_eq3_q", 1.0),
            ),
            (
                get_float_prop(properties, "main_eq4_freq", 8000.0),
                get_float_prop(properties, "main_eq4_gain", 0.0),
                get_float_prop(properties, "main_eq4_q", 1.0),
            ),
        ];
        let main_eq_id = format!("{}:main_eq", instance_id);
        let main_eq = make_eq_element(&main_eq_id, main_eq_enabled, &main_eq_bands, dsp_backend)?;
        elements.push((main_eq_id.clone(), main_eq));

        let main_limiter_enabled = get_bool_prop(properties, "main_limiter_enabled", false);
        let main_limiter_threshold = get_float_prop(properties, "main_limiter_threshold", -3.0);
        let main_limiter_id = format!("{}:main_limiter", instance_id);
        let main_limiter = make_limiter_element(
            &main_limiter_id,
            main_limiter_enabled,
            main_limiter_threshold,
            dsp_backend,
        )?;
        elements.push((main_limiter_id.clone(), main_limiter));

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

        // Main output tee (allow-not-linked so unconnected main_out doesn't stall pipeline)
        let main_out_tee_id = format!("{}:main_out_tee", instance_id);
        let main_out_tee = gst::ElementFactory::make("tee")
            .name(&main_out_tee_id)
            .property("allow-not-linked", true)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("main_out_tee: {}", e)))?;
        elements.push((main_out_tee_id.clone(), main_out_tee));

        // Link: mixer → main_comp → main_eq → main_limiter → main_volume → main_level → main_out_tee
        internal_links.push((
            ElementPadRef::pad(&mixer_id, "src"),
            ElementPadRef::pad(&main_comp_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&main_comp_id, "src"),
            ElementPadRef::pad(&main_eq_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&main_eq_id, "src"),
            ElementPadRef::pad(&main_limiter_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&main_limiter_id, "src"),
            ElementPadRef::pad(&main_volume_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&main_volume_id, "src"),
            ElementPadRef::pad(&main_level_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&main_level_id, "src"),
            ElementPadRef::pad(&main_out_tee_id, "sink"),
        ));

        // ========================================================================
        // Create PFL (Pre-Fader Listen) bus with master level
        // ========================================================================
        let pfl_mixer_id = format!("{}:pfl_mixer", instance_id);
        let pfl_mixer = make_audiomixer(
            &pfl_mixer_id,
            force_live,
            latency_ms,
            min_upstream_latency_ms,
        )?;
        elements.push((pfl_mixer_id.clone(), pfl_mixer));

        // PFL master volume
        let pfl_master_vol_id = format!("{}:pfl_master_vol", instance_id);
        let pfl_master_level = get_float_prop(properties, "pfl_level", 1.0);
        let pfl_master_vol = gst::ElementFactory::make("volume")
            .name(&pfl_master_vol_id)
            .property("volume", pfl_master_level)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("pfl master vol: {}", e)))?;
        elements.push((pfl_master_vol_id.clone(), pfl_master_vol));

        let pfl_level_id = format!("{}:pfl_level", instance_id);
        let pfl_level = gst::ElementFactory::make("level")
            .name(&pfl_level_id)
            .property("interval", 100_000_000u64)
            .property("post-messages", true)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("pfl_level: {}", e)))?;
        elements.push((pfl_level_id.clone(), pfl_level));

        // PFL output tee (allow-not-linked so unconnected pfl_out doesn't stall pipeline)
        let pfl_out_tee_id = format!("{}:pfl_out_tee", instance_id);
        let pfl_out_tee = gst::ElementFactory::make("tee")
            .name(&pfl_out_tee_id)
            .property("allow-not-linked", true)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("pfl_out_tee: {}", e)))?;
        elements.push((pfl_out_tee_id.clone(), pfl_out_tee));

        // Link: pfl_mixer → pfl_master_vol → pfl_level → pfl_out_tee
        internal_links.push((
            ElementPadRef::pad(&pfl_mixer_id, "src"),
            ElementPadRef::pad(&pfl_master_vol_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&pfl_master_vol_id, "src"),
            ElementPadRef::pad(&pfl_level_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&pfl_level_id, "src"),
            ElementPadRef::pad(&pfl_out_tee_id, "sink"),
        ));

        // ========================================================================
        // Create Aux buses
        // ========================================================================
        for aux in 0..num_aux_buses {
            let aux_mixer_id = format!("{}:aux{}_mixer", instance_id, aux);
            let aux_mixer = make_audiomixer(
                &aux_mixer_id,
                force_live,
                latency_ms,
                min_upstream_latency_ms,
            )?;
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

            // Aux output tee (allow-not-linked so unconnected aux_out doesn't stall pipeline)
            let aux_out_tee_id = format!("{}:aux{}_out_tee", instance_id, aux);
            let aux_out_tee = gst::ElementFactory::make("tee")
                .name(&aux_out_tee_id)
                .property("allow-not-linked", true)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("aux{}_out_tee: {}", aux, e))
                })?;
            elements.push((aux_out_tee_id.clone(), aux_out_tee));

            // Link: aux_mixer → aux_volume → aux_level → aux_out_tee
            internal_links.push((
                ElementPadRef::pad(&aux_mixer_id, "src"),
                ElementPadRef::pad(&aux_volume_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&aux_volume_id, "src"),
                ElementPadRef::pad(&aux_level_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&aux_level_id, "src"),
                ElementPadRef::pad(&aux_out_tee_id, "sink"),
            ));
        }

        // ========================================================================
        // Create Groups
        // ========================================================================
        for sg in 0..num_groups {
            let sg_mixer_id = format!("{}:group{}_mixer", instance_id, sg);
            let sg_mixer = make_audiomixer(
                &sg_mixer_id,
                force_live,
                latency_ms,
                min_upstream_latency_ms,
            )?;
            elements.push((sg_mixer_id.clone(), sg_mixer));

            let sg_fader = get_float_prop(properties, &format!("group{}_fader", sg + 1), 1.0);
            let sg_mute = get_bool_prop(properties, &format!("group{}_mute", sg + 1), false);
            let sg_volume_val = if sg_mute { 0.0 } else { sg_fader };

            let sg_volume_id = format!("{}:group{}_volume", instance_id, sg);
            let sg_volume = gst::ElementFactory::make("volume")
                .name(&sg_volume_id)
                .property("volume", sg_volume_val)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("group{}_volume: {}", sg, e))
                })?;
            elements.push((sg_volume_id.clone(), sg_volume));

            let sg_level_id = format!("{}:group{}_level", instance_id, sg);
            let sg_level = gst::ElementFactory::make("level")
                .name(&sg_level_id)
                .property("interval", 100_000_000u64)
                .property("post-messages", true)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("group{}_level: {}", sg, e))
                })?;
            elements.push((sg_level_id.clone(), sg_level));

            // Group output tee - allows both external output AND feeding main mixer
            // Also prevents NOT_LINKED when group_out isn't connected externally.
            let sg_out_tee_id = format!("{}:group{}_out_tee", instance_id, sg);
            let sg_out_tee = gst::ElementFactory::make("tee")
                .name(&sg_out_tee_id)
                .property("allow-not-linked", true)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("group{}_out_tee: {}", sg, e))
                })?;
            elements.push((sg_out_tee_id.clone(), sg_out_tee));

            // Queue between group tee and main mixer (isolates scheduling)
            let sg_to_main_queue_id = format!("{}:group{}_to_main_queue", instance_id, sg);
            let sg_to_main_queue = gst::ElementFactory::make("queue")
                .name(&sg_to_main_queue_id)
                .property("max-size-buffers", 3u32)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("group{}_to_main_queue: {}", sg, e))
                })?;
            elements.push((sg_to_main_queue_id.clone(), sg_to_main_queue));

            // Link: group_mixer → group_volume → group_level → group_out_tee
            //        group_out_tee → queue → main audiomixer
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
                ElementPadRef::pad(&sg_out_tee_id, "sink"),
            ));
            // One branch from tee feeds the main mixer
            internal_links.push((
                ElementPadRef::element(&sg_out_tee_id),
                ElementPadRef::pad(&sg_to_main_queue_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&sg_to_main_queue_id, "src"),
                ElementPadRef::element(&mixer_id), // Request pad from main audiomixer
            ));
        }

        // ========================================================================
        // Create per-channel processing
        // ========================================================================
        for ch in 0..num_channels {
            let ch_num = ch + 1; // 1-indexed for display

            // Get channel properties
            let gain_db = get_float_prop(properties, &format!("ch{}_gain", ch_num), 0.0);

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
                .unwrap_or(1.0); // Default 0 dB (unity)

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
            // Input gain stage
            // ----------------------------------------------------------------
            let gain_id = format!("{}:gain_{}", instance_id, ch);
            let gain_linear = db_to_linear(gain_db);
            let gain_elem = gst::ElementFactory::make("volume")
                .name(&gain_id)
                .property("volume", gain_linear)
                .build()
                .map_err(|e| {
                    BlockBuildError::ElementCreation(format!("gain ch{}: {}", ch_num, e))
                })?;
            elements.push((gain_id.clone(), gain_elem));

            // ----------------------------------------------------------------
            // HPF (High-Pass Filter)
            // ----------------------------------------------------------------
            let hpf_enabled =
                get_bool_prop(properties, &format!("ch{}_hpf_enabled", ch_num), false);
            let hpf_freq = get_float_prop(properties, &format!("ch{}_hpf_freq", ch_num), 80.0);

            let hpf_id = format!("{}:hpf_{}", instance_id, ch);
            let hpf = make_hpf_element(&hpf_id, hpf_enabled, hpf_freq)?;
            elements.push((hpf_id.clone(), hpf));

            // ----------------------------------------------------------------
            // Gate (LSP Gate Stereo with fallback)
            // ----------------------------------------------------------------
            let gate_enabled =
                get_bool_prop(properties, &format!("ch{}_gate_enabled", ch_num), false);
            let gate_threshold =
                get_float_prop(properties, &format!("ch{}_gate_threshold", ch_num), -40.0);
            let gate_attack = get_float_prop(properties, &format!("ch{}_gate_attack", ch_num), 5.0);
            let gate_release =
                get_float_prop(properties, &format!("ch{}_gate_release", ch_num), 100.0);
            let gate_range = get_float_prop(properties, &format!("ch{}_gate_range", ch_num), -80.0);

            let gate_id = format!("{}:gate_{}", instance_id, ch);
            let gate = make_gate_element(
                &gate_id,
                gate_enabled,
                gate_threshold,
                gate_attack,
                gate_release,
                gate_range,
                dsp_backend,
            )?;
            elements.push((gate_id.clone(), gate));

            // ----------------------------------------------------------------
            // Compressor (LSP Compressor Stereo with fallback)
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
            let comp_knee = get_float_prop(properties, &format!("ch{}_comp_knee", ch_num), -6.0);

            let comp_id = format!("{}:comp_{}", instance_id, ch);
            let compressor = make_compressor_element(
                &comp_id,
                comp_enabled,
                comp_threshold,
                comp_ratio,
                comp_attack,
                comp_release,
                comp_makeup,
                dsp_backend,
            )?;
            // Set knee - Rust backend uses "knee" (linear), LV2 uses "kn" (linear)
            // kn range: 0.0631..1.0 (linear gain, default ~0.5 = -6dB)
            if compressor.find_property("knee").is_some() {
                let kn_val = db_to_linear(comp_knee).clamp(0.0631, 1.0) as f32;
                compressor.set_property("knee", kn_val);
            } else if compressor.find_property("kn").is_some() {
                let kn_val = db_to_linear(comp_knee).clamp(0.0631, 1.0) as f32;
                compressor.set_property("kn", kn_val);
            }
            elements.push((comp_id.clone(), compressor));

            // ----------------------------------------------------------------
            // EQ (LSP Parametric Equalizer x8 Stereo with fallback)
            // ----------------------------------------------------------------
            let eq_enabled = get_bool_prop(properties, &format!("ch{}_eq_enabled", ch_num), false);

            let eq_defaults: [(f64, f64); 4] =
                [(80.0, 1.0), (400.0, 1.0), (2000.0, 1.0), (8000.0, 1.0)];
            let eq_bands: [(f64, f64, f64); 4] = std::array::from_fn(|band| {
                let (def_freq, def_q) = eq_defaults[band];
                let freq = get_float_prop(
                    properties,
                    &format!("ch{}_eq{}_freq", ch_num, band + 1),
                    def_freq,
                );
                let gain = get_float_prop(
                    properties,
                    &format!("ch{}_eq{}_gain", ch_num, band + 1),
                    0.0,
                );
                let q =
                    get_float_prop(properties, &format!("ch{}_eq{}_q", ch_num, band + 1), def_q);
                (freq, gain, q)
            });

            let eq_id = format!("{}:eq_{}", instance_id, ch);
            let eq = make_eq_element(&eq_id, eq_enabled, &eq_bands, dsp_backend)?;
            elements.push((eq_id.clone(), eq));

            // ----------------------------------------------------------------
            // Pre-fader tee (for PFL tap and pre-fader aux sends)
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
            // Post-fader tee (for post-fader aux sends)
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
            // Aux send paths (pre or post fader)
            // ----------------------------------------------------------------
            for aux in 0..num_aux_buses {
                let aux_send_level = get_float_prop(
                    properties,
                    &format!("ch{}_aux{}_level", ch_num, aux + 1),
                    0.0,
                );
                let aux_pre = get_bool_prop(
                    properties,
                    &format!("ch{}_aux{}_pre", ch_num, aux + 1),
                    aux < 2, // Default: aux 1-2 pre-fader, aux 3-4 post-fader
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

                // Source tee depends on pre/post setting
                let source_tee_id = if aux_pre {
                    &pre_fader_tee_id
                } else {
                    &post_fader_tee_id
                };

                // Link: (pre|post)_fader_tee → aux_send → aux_queue → aux_mixer
                let aux_mixer_id = format!("{}:aux{}_mixer", instance_id, aux);
                internal_links.push((
                    ElementPadRef::element(source_tee_id), // Request pad from tee
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
            // Chain: convert → caps → gain → hpf → gate → comp → eq → pre_fader_tee
            internal_links.push((
                ElementPadRef::pad(&convert_id, "src"),
                ElementPadRef::pad(&caps_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&caps_id, "src"),
                ElementPadRef::pad(&gain_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&gain_id, "src"),
                ElementPadRef::pad(&hpf_id, "sink"),
            ));
            internal_links.push((
                ElementPadRef::pad(&hpf_id, "src"),
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

            // Solo path: PFL (pre-fader) or AFL (post-fader) based on solo_mode
            let solo_source_tee_id = if solo_mode_afl {
                &post_fader_tee_id
            } else {
                &pre_fader_tee_id
            };
            internal_links.push((
                ElementPadRef::element(solo_source_tee_id),
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
            // Multi-destination routing (Main + Groups)
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

            // Route to groups
            for sg in 0..num_groups {
                let to_grp_enabled =
                    get_bool_prop(properties, &format!("ch{}_to_grp{}", ch_num, sg + 1), false);

                let to_grp_vol_id = format!("{}:to_grp{}_vol_{}", instance_id, sg, ch);
                let to_grp_vol = gst::ElementFactory::make("volume")
                    .name(&to_grp_vol_id)
                    .property("volume", if to_grp_enabled { 1.0 } else { 0.0 })
                    .build()
                    .map_err(|e| {
                        BlockBuildError::ElementCreation(format!(
                            "to_grp{}_vol ch{}: {}",
                            sg + 1,
                            ch_num,
                            e
                        ))
                    })?;
                elements.push((to_grp_vol_id.clone(), to_grp_vol));

                let to_grp_queue_id = format!("{}:to_grp{}_queue_{}", instance_id, sg, ch);
                let to_grp_queue = gst::ElementFactory::make("queue")
                    .name(&to_grp_queue_id)
                    .property("max-size-buffers", 3u32)
                    .build()
                    .map_err(|e| {
                        BlockBuildError::ElementCreation(format!(
                            "to_grp{}_queue ch{}: {}",
                            sg + 1,
                            ch_num,
                            e
                        ))
                    })?;
                elements.push((to_grp_queue_id.clone(), to_grp_queue));

                // Link: routing_tee → to_grp_vol → to_grp_queue → group_mixer
                let sg_mixer_id = format!("{}:group{}_mixer", instance_id, sg);
                internal_links.push((
                    ElementPadRef::element(&routing_tee_id),
                    ElementPadRef::pad(&to_grp_vol_id, "sink"),
                ));
                internal_links.push((
                    ElementPadRef::pad(&to_grp_vol_id, "src"),
                    ElementPadRef::pad(&to_grp_queue_id, "sink"),
                ));
                internal_links.push((
                    ElementPadRef::pad(&to_grp_queue_id, "src"),
                    ElementPadRef::element(&sg_mixer_id),
                ));

                if to_grp_enabled {
                    debug!("Channel {} routed to group {}", ch_num, sg + 1);
                }
            }

            debug!(
                "Channel {} created: gain={:.1}dB, pan={}, fader={}, mute={}, pfl={}, to_main={}",
                ch_num, gain_db, pan, fader, mute, pfl_enabled, to_main_enabled
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

// ============================================================================
// Helper functions for element creation
// ============================================================================

/// Create a configured audiomixer element with force-live, latency, and start-time-selection.
fn make_audiomixer(
    name: &str,
    force_live: bool,
    latency_ms: u64,
    min_upstream_latency_ms: u64,
) -> Result<gst::Element, BlockBuildError> {
    // Check if force-live is available (construct-only, must be set at build time)
    let has_force_live = {
        let probe = gst::ElementFactory::make("audiomixer")
            .build()
            .map_err(|e| {
                BlockBuildError::ElementCreation(format!("audiomixer probe {}: {}", name, e))
            })?;
        probe.find_property("force-live").is_some()
    };

    let mut builder = gst::ElementFactory::make("audiomixer").name(name);
    if has_force_live {
        builder = builder.property("force-live", force_live);
    }
    let mixer = builder
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("audiomixer {}: {}", name, e)))?;

    // start-time-selection=first: use first buffer's timestamp as start time
    mixer.set_property_from_str("start-time-selection", "first");

    // latency: aggregator timeout in nanoseconds
    let latency_ns = latency_ms * 1_000_000;
    mixer.set_property("latency", latency_ns * gst::ClockTime::NSECOND);

    // min-upstream-latency: reported to upstream elements
    if mixer.find_property("min-upstream-latency").is_some() {
        let min_upstream_ns = min_upstream_latency_ms * 1_000_000;
        mixer.set_property(
            "min-upstream-latency",
            min_upstream_ns * gst::ClockTime::NSECOND,
        );
    }

    Ok(mixer)
}

// ============================================================================
// Helper functions for LV2 fallback
// ============================================================================

/// Create a gate element, falling back to identity passthrough if unavailable.
fn make_gate_element(
    name: &str,
    enabled: bool,
    threshold_db: f64,
    attack_ms: f64,
    release_ms: f64,
    _range_db: f64,
    backend: &str,
) -> Result<gst::Element, BlockBuildError> {
    if backend == "rust" {
        if let Ok(gate) = gst::ElementFactory::make("lsp-rs-gate").name(name).build() {
            gate.set_property("enabled", enabled);
            gate.set_property("open-threshold", threshold_db as f32);
            gate.set_property("close-threshold", threshold_db as f32);
            gate.set_property("attack", attack_ms as f32);
            gate.set_property("release", release_ms as f32);
            return Ok(gate);
        }
        warn!(
            "lsp-rs-gate not available for {}, trying LV2 fallback",
            name
        );
    }
    if let Ok(gate) = gst::ElementFactory::make("lsp-plug-in-plugins-lv2-gate-stereo")
        .name(name)
        .build()
    {
        if gate.find_property("enabled").is_some() {
            gate.set_property("enabled", enabled);
        }
        if gate.find_property("gt").is_some() {
            gate.set_property("gt", db_to_linear(threshold_db) as f32);
        }
        if gate.find_property("at").is_some() {
            gate.set_property("at", attack_ms as f32);
        }
        if gate.find_property("rt").is_some() {
            gate.set_property("rt", release_ms as f32);
        }
        return Ok(gate);
    }
    warn!("No gate plugin available for {}, using passthrough", name);
    gst::ElementFactory::make("identity")
        .name(name)
        .property("silent", true)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("gate fallback {}: {}", name, e)))
}

/// Create a compressor element, falling back to identity passthrough if unavailable.
#[allow(clippy::too_many_arguments)]
fn make_compressor_element(
    name: &str,
    enabled: bool,
    threshold_db: f64,
    ratio: f64,
    attack_ms: f64,
    release_ms: f64,
    makeup_db: f64,
    backend: &str,
) -> Result<gst::Element, BlockBuildError> {
    if backend == "rust" {
        if let Ok(comp) = gst::ElementFactory::make("lsp-rs-compressor")
            .name(name)
            .build()
        {
            comp.set_property("enabled", enabled);
            comp.set_property("threshold", db_to_linear(threshold_db) as f32);
            comp.set_property("ratio", ratio as f32);
            comp.set_property("attack", attack_ms as f32);
            comp.set_property("release", release_ms as f32);
            comp.set_property("makeup-gain", db_to_linear(makeup_db) as f32);
            return Ok(comp);
        }
        warn!(
            "lsp-rs-compressor not available for {}, trying LV2 fallback",
            name
        );
    }
    if let Ok(comp) = gst::ElementFactory::make("lsp-plug-in-plugins-lv2-compressor-stereo")
        .name(name)
        .build()
    {
        if comp.find_property("enabled").is_some() {
            comp.set_property("enabled", enabled);
        }
        if comp.find_property("al").is_some() {
            comp.set_property("al", db_to_linear(threshold_db) as f32);
        }
        if comp.find_property("cr").is_some() {
            comp.set_property("cr", ratio as f32);
        }
        if comp.find_property("at").is_some() {
            comp.set_property("at", attack_ms as f32);
        }
        if comp.find_property("rt").is_some() {
            comp.set_property("rt", release_ms as f32);
        }
        if comp.find_property("mk").is_some() {
            comp.set_property("mk", db_to_linear(makeup_db) as f32);
        }
        return Ok(comp);
    }
    warn!(
        "No compressor plugin available for {}, using passthrough",
        name
    );
    gst::ElementFactory::make("identity")
        .name(name)
        .property("silent", true)
        .build()
        .map_err(|e| {
            BlockBuildError::ElementCreation(format!("compressor fallback {}: {}", name, e))
        })
}

/// Create a parametric EQ element, falling back to identity passthrough if unavailable.
fn make_eq_element(
    name: &str,
    enabled: bool,
    bands: &[(f64, f64, f64); 4],
    backend: &str,
) -> Result<gst::Element, BlockBuildError> {
    if backend == "rust" {
        if let Ok(eq) = gst::ElementFactory::make("lsp-rs-equalizer")
            .name(name)
            .build()
        {
            eq.set_property("enabled", enabled);
            eq.set_property("num-bands", 4u32);
            for (band, (freq, gain_db, q)) in bands.iter().enumerate() {
                eq.set_property(&format!("band{}-type", band), 7i32); // 7 = Peaking/Bell
                eq.set_property(&format!("band{}-frequency", band), *freq as f32);
                eq.set_property(&format!("band{}-gain", band), *gain_db as f32); // dB directly
                eq.set_property(&format!("band{}-q", band), *q as f32);
                eq.set_property(&format!("band{}-enabled", band), true);
            }
            return Ok(eq);
        }
        warn!(
            "lsp-rs-equalizer not available for {}, trying LV2 fallback",
            name
        );
    }
    if let Ok(eq) = gst::ElementFactory::make("lsp-plug-in-plugins-lv2-para-equalizer-x8-stereo")
        .name(name)
        .build()
    {
        if eq.find_property("enabled").is_some() {
            eq.set_property("enabled", enabled);
        }
        for (band, (freq, gain_db, q)) in bands.iter().enumerate() {
            let ft_prop = format!("ft-{}", band);
            let f_prop = format!("f-{}", band);
            let g_prop = format!("g-{}", band);
            let q_prop = format!("q-{}", band);
            if eq.find_property(&ft_prop).is_some() {
                eq.set_property_from_str(&ft_prop, "Bell");
            }
            if eq.find_property(&f_prop).is_some() {
                eq.set_property(&f_prop, *freq as f32);
            }
            if eq.find_property(&g_prop).is_some() {
                eq.set_property(&g_prop, db_to_linear(*gain_db) as f32);
            }
            if eq.find_property(&q_prop).is_some() {
                eq.set_property(&q_prop, *q as f32);
            }
        }
        return Ok(eq);
    }
    warn!("No EQ plugin available for {}, using passthrough", name);
    gst::ElementFactory::make("identity")
        .name(name)
        .property("silent", true)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("eq fallback {}: {}", name, e)))
}

/// Create a limiter element, falling back to identity passthrough if unavailable.
fn make_limiter_element(
    name: &str,
    enabled: bool,
    threshold_db: f64,
    backend: &str,
) -> Result<gst::Element, BlockBuildError> {
    if backend == "rust" {
        if let Ok(lim) = gst::ElementFactory::make("lsp-rs-limiter")
            .name(name)
            .build()
        {
            lim.set_property("enabled", enabled);
            lim.set_property("threshold", threshold_db as f32); // dB directly
            return Ok(lim);
        }
        warn!(
            "lsp-rs-limiter not available for {}, trying LV2 fallback",
            name
        );
    }
    if let Ok(lim) = gst::ElementFactory::make("lsp-plug-in-plugins-lv2-limiter-stereo")
        .name(name)
        .build()
    {
        if lim.find_property("enabled").is_some() {
            lim.set_property("enabled", enabled);
        }
        if lim.find_property("th").is_some() {
            lim.set_property("th", db_to_linear(threshold_db) as f32);
        }
        return Ok(lim);
    }
    warn!(
        "No limiter plugin available for {}, using passthrough",
        name
    );
    gst::ElementFactory::make("identity")
        .name(name)
        .property("silent", true)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("limiter fallback {}: {}", name, e)))
}

/// Create a high-pass filter element. Uses audiocheblimit from gst-plugins-good,
/// falls back to identity passthrough if unavailable.
fn make_hpf_element(
    name: &str,
    enabled: bool,
    cutoff_hz: f64,
) -> Result<gst::Element, BlockBuildError> {
    if let Ok(hpf) = gst::ElementFactory::make("audiocheblimit")
        .name(name)
        .build()
    {
        // mode: 0=low-pass, 1=high-pass
        hpf.set_property_from_str("mode", "high-pass");
        hpf.set_property("cutoff", cutoff_hz as f32);
        hpf.set_property_from_str("poles", "4"); // 24dB/oct slope
        if !enabled {
            // Bypass by setting cutoff to minimum
            hpf.set_property("cutoff", 1.0f32);
        }
        return Ok(hpf);
    }
    // Try audiowsinclimit as alternative
    if let Ok(hpf) = gst::ElementFactory::make("audiowsinclimit")
        .name(name)
        .build()
    {
        hpf.set_property_from_str("mode", "high-pass");
        hpf.set_property("cutoff", cutoff_hz as f32);
        if !enabled {
            hpf.set_property("cutoff", 1.0f32);
        }
        return Ok(hpf);
    }
    warn!("No HPF plugin available for {}, using passthrough", name);
    gst::ElementFactory::make("identity")
        .name(name)
        .property("silent", true)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("hpf fallback {}: {}", name, e)))
}

// ============================================================================
// Property parsing helpers
// ============================================================================

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

/// Parse number of groups from properties.
fn parse_num_groups(properties: &HashMap<String, PropertyValue>) -> usize {
    properties
        .get("num_groups")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as usize),
            PropertyValue::UInt(u) => Some(*u as usize),
            PropertyValue::String(s) => s.parse::<usize>().ok(),
            _ => None,
        })
        .unwrap_or(0)
        .clamp(0, MAX_GROUPS)
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

/// Get a string property with default.
fn get_string_prop<'a>(
    properties: &'a HashMap<String, PropertyValue>,
    name: &str,
    default: &'a str,
) -> &'a str {
    properties
        .get(name)
        .and_then(|v| match v {
            PropertyValue::String(s) => Some(s.as_str()),
            _ => None,
        })
        .unwrap_or(default)
}

/// Convert dB to linear scale.
fn db_to_linear(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

/// Translate a property name and value from LV2 conventions to lsp-rs conventions.
///
/// The ExposedProperty mappings use LV2 property names (gt, at, rt, al, cr, mk, kn, th, f-N, g-N, q-N).
/// When the target element is from lsp-plugins-rs, this function translates the property name
/// and adjusts the value format where needed (e.g., LV2 uses linear gain, Rust uses dB).
///
/// Returns (translated_prop_name, translated_value) or None if no translation needed.
pub fn translate_property_for_element(
    element: &gst::Element,
    prop_name: &str,
    value: &PropertyValue,
) -> Option<(String, PropertyValue)> {
    // Use GObject type name instead of factory() which can SIGSEGV
    // when static plugins and LV2 plugins coexist.
    let type_name = element.type_().name();

    if type_name == "LspRsGate" {
        let (new_name, new_value) = match prop_name {
            "gt" => {
                // LV2: gt is linear (already transformed by db_to_linear).
                // Rust: open-threshold is dB. Reverse the transform.
                let db_val = match value {
                    PropertyValue::Float(v) => linear_to_db(*v),
                    _ => return None,
                };
                ("open-threshold".to_string(), PropertyValue::Float(db_val))
            }
            "at" => ("attack".to_string(), value.clone()),
            "rt" => ("release".to_string(), value.clone()),
            "enabled" => return None, // same name, no translation needed
            _ => return None,
        };
        return Some((new_name, new_value));
    }

    if type_name == "LspRsCompressor" {
        let (new_name, new_value) = match prop_name {
            "al" => {
                // Both use linear, same transform
                ("threshold".to_string(), value.clone())
            }
            "cr" => ("ratio".to_string(), value.clone()),
            "at" => ("attack".to_string(), value.clone()),
            "rt" => ("release".to_string(), value.clone()),
            "mk" => {
                // Both use linear, same transform
                ("makeup-gain".to_string(), value.clone())
            }
            "kn" => {
                // Both use linear, same transform
                ("knee".to_string(), value.clone())
            }
            "enabled" => return None,
            _ => return None,
        };
        return Some((new_name, new_value));
    }

    if type_name == "LspRsEqualizer" {
        // EQ band properties: f-N -> bandN-frequency, g-N -> bandN-gain, q-N -> bandN-q
        if let Some(band) = prop_name.strip_prefix("f-") {
            return Some((format!("band{}-frequency", band), value.clone()));
        }
        if let Some(band) = prop_name.strip_prefix("g-") {
            // LV2: g-N is linear (already transformed by db_to_linear).
            // Rust: bandN-gain is dB. Reverse the transform.
            let db_val = match value {
                PropertyValue::Float(v) => linear_to_db(*v),
                _ => return None,
            };
            return Some((format!("band{}-gain", band), PropertyValue::Float(db_val)));
        }
        if let Some(band) = prop_name.strip_prefix("q-") {
            return Some((format!("band{}-q", band), value.clone()));
        }
        if prop_name == "enabled" {
            return None;
        }
        return None;
    }

    if type_name == "LspRsLimiter" {
        let (new_name, new_value) = match prop_name {
            "th" => {
                // LV2: th is linear (already transformed by db_to_linear).
                // Rust: threshold is dB. Reverse the transform.
                let db_val = match value {
                    PropertyValue::Float(v) => linear_to_db(*v),
                    _ => return None,
                };
                ("threshold".to_string(), PropertyValue::Float(db_val))
            }
            "enabled" => return None,
            _ => return None,
        };
        return Some((new_name, new_value));
    }

    None
}

/// Convert linear scale to dB.
fn linear_to_db(linear: f64) -> f64 {
    if linear <= 0.0 {
        -120.0 // floor
    } else {
        20.0 * linear.log10()
    }
}

// ============================================================================
// Metering
// ============================================================================

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
    let group_level_prefix = format!("{}:group", instance_id);

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

                        // Check if this is a group level meter
                        // Format: "instance_id:groupN_level"
                        if source_name.starts_with(&group_level_prefix)
                            && source_name.contains("_level")
                        {
                            if let Some(sg_part) =
                                source_name.strip_prefix(&format!("{}:group", instance_id))
                            {
                                if let Some(sg_num_str) = sg_part.strip_suffix("_level") {
                                    if let Ok(sg_num) = sg_num_str.parse::<usize>() {
                                        trace!(
                                            "Mixer group{} meter: rms={:?}, peak={:?}",
                                            sg_num + 1,
                                            rms,
                                            peak
                                        );
                                        let element_id =
                                            format!("{}:meter:group{}", instance_id, sg_num + 1);
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

// ============================================================================
// Block definition (metadata for UI/API)
// ============================================================================

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
        // DSP Backend selection
        ExposedProperty {
            name: "dsp_backend".to_string(),
            label: "DSP Backend".to_string(),
            description: "LV2 uses external C++ LSP plugins, Rust uses built-in lsp-plugins-rs"
                .to_string(),
            property_type: PropertyType::Enum {
                values: vec![
                    EnumValue {
                        value: "lv2".to_string(),
                        label: Some("LV2".to_string()),
                    },
                    EnumValue {
                        value: "rust".to_string(),
                        label: Some("Rust".to_string()),
                    },
                ],
            },
            default_value: Some(PropertyValue::String("lv2".to_string())),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: "dsp_backend".to_string(),
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
        // Number of groups
        ExposedProperty {
            name: "num_groups".to_string(),
            label: "Groups".to_string(),
            description: "Number of group buses (0-4)".to_string(),
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
                property_name: "num_groups".to_string(),
                transform: None,
            },
        },
        // PFL master level
        ExposedProperty {
            name: "pfl_level".to_string(),
            label: "PFL Level".to_string(),
            description: "PFL/AFL bus master level (0.0 to 2.0)".to_string(),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(1.0)),
            mapping: PropertyMapping {
                element_id: "pfl_master_vol".to_string(),
                property_name: "volume".to_string(),
                transform: None,
            },
        },
        // Solo mode (PFL or AFL)
        ExposedProperty {
            name: "solo_mode".to_string(),
            label: "Solo Mode".to_string(),
            description: "Solo listen mode: PFL (pre-fader) or AFL (after-fader)".to_string(),
            property_type: PropertyType::Enum {
                values: vec![
                    EnumValue {
                        value: "pfl".to_string(),
                        label: Some("PFL".to_string()),
                    },
                    EnumValue {
                        value: "afl".to_string(),
                        label: Some("AFL".to_string()),
                    },
                ],
            },
            default_value: Some(PropertyValue::String("pfl".to_string())),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: "solo_mode".to_string(),
                transform: None,
            },
        },
    ];

    // ========================================================================
    // Aggregator / live mode properties
    // ========================================================================
    exposed_properties.push(ExposedProperty {
        name: "force_live".to_string(),
        label: "Force Live".to_string(),
        description: "Always operate in live mode. Prevents mixer from hanging when not all inputs are connected. Construction-time only.".to_string(),
        property_type: PropertyType::Bool,
        default_value: Some(PropertyValue::Bool(true)),
        mapping: PropertyMapping {
            element_id: "_block".to_string(),
            property_name: "force_live".to_string(),
            transform: None,
        },
    });
    exposed_properties.push(ExposedProperty {
        name: "latency".to_string(),
        label: "Latency".to_string(),
        description: "Mixer aggregator latency in milliseconds. Time to wait for slower inputs before producing output. Construction-time only.".to_string(),
        property_type: PropertyType::Float,
        default_value: Some(PropertyValue::Float(30.0)),
        mapping: PropertyMapping {
            element_id: "_block".to_string(),
            property_name: "latency".to_string(),
            transform: None,
        },
    });
    exposed_properties.push(ExposedProperty {
        name: "min_upstream_latency".to_string(),
        label: "Min Upstream Latency".to_string(),
        description: "Minimum upstream latency reported to upstream elements in milliseconds. Construction-time only.".to_string(),
        property_type: PropertyType::Float,
        default_value: Some(PropertyValue::Float(30.0)),
        mapping: PropertyMapping {
            element_id: "_block".to_string(),
            property_name: "min_upstream_latency".to_string(),
            transform: None,
        },
    });

    // ========================================================================
    // Main bus processing properties
    // ========================================================================
    exposed_properties.push(ExposedProperty {
        name: "main_comp_enabled".to_string(),
        label: "Main Comp".to_string(),
        description: "Enable compressor on main bus".to_string(),
        property_type: PropertyType::Bool,
        default_value: Some(PropertyValue::Bool(false)),
        mapping: PropertyMapping {
            element_id: "main_comp".to_string(),
            property_name: "enabled".to_string(),
            transform: None,
        },
    });
    for (prop_suffix, label, gst_prop, default, desc, transform) in [
        (
            "main_comp_threshold",
            "Main Comp Thresh",
            "al",
            -20.0,
            "Main bus compressor threshold in dB (-60 to 0)",
            Some("db_to_linear"),
        ),
        (
            "main_comp_ratio",
            "Main Comp Ratio",
            "cr",
            4.0,
            "Main bus compressor ratio (1:1 to 20:1)",
            None,
        ),
        (
            "main_comp_attack",
            "Main Comp Atk",
            "at",
            10.0,
            "Main bus compressor attack in ms (0-200)",
            None,
        ),
        (
            "main_comp_release",
            "Main Comp Rel",
            "rt",
            100.0,
            "Main bus compressor release in ms (10-1000)",
            None,
        ),
        (
            "main_comp_makeup",
            "Main Comp Makeup",
            "mk",
            0.0,
            "Main bus compressor makeup gain in dB (0 to 24)",
            Some("db_to_linear"),
        ),
    ] {
        exposed_properties.push(ExposedProperty {
            name: prop_suffix.to_string(),
            label: label.to_string(),
            description: desc.to_string(),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(default)),
            mapping: PropertyMapping {
                element_id: "main_comp".to_string(),
                property_name: gst_prop.to_string(),
                transform: transform.map(|s| s.to_string()),
            },
        });
    }

    // Main EQ
    exposed_properties.push(ExposedProperty {
        name: "main_eq_enabled".to_string(),
        label: "Main EQ".to_string(),
        description: "Enable parametric EQ on main bus".to_string(),
        property_type: PropertyType::Bool,
        default_value: Some(PropertyValue::Bool(false)),
        mapping: PropertyMapping {
            element_id: "main_eq".to_string(),
            property_name: "enabled".to_string(),
            transform: None,
        },
    });
    let main_eq_band_defaults = [
        (80.0, "Low"),
        (400.0, "Low-Mid"),
        (2000.0, "Hi-Mid"),
        (8000.0, "High"),
    ];
    for (band, (def_freq, band_name)) in main_eq_band_defaults.iter().enumerate() {
        let band_num = band + 1;
        exposed_properties.push(ExposedProperty {
            name: format!("main_eq{}_freq", band_num),
            label: format!("Main EQ{} Freq", band_num),
            description: format!(
                "Main bus EQ band {} ({}) frequency in Hz",
                band_num, band_name
            ),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(*def_freq)),
            mapping: PropertyMapping {
                element_id: "main_eq".to_string(),
                property_name: format!("f-{}", band),
                transform: None,
            },
        });
        exposed_properties.push(ExposedProperty {
            name: format!("main_eq{}_gain", band_num),
            label: format!("Main EQ{} Gain", band_num),
            description: format!("Main bus EQ band {} gain in dB (-15 to +15)", band_num),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(0.0)),
            mapping: PropertyMapping {
                element_id: "main_eq".to_string(),
                property_name: format!("g-{}", band),
                transform: Some("db_to_linear".to_string()),
            },
        });
        exposed_properties.push(ExposedProperty {
            name: format!("main_eq{}_q", band_num),
            label: format!("Main EQ{} Q", band_num),
            description: format!("Main bus EQ band {} Q factor (0.1 to 10)", band_num),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(1.0)),
            mapping: PropertyMapping {
                element_id: "main_eq".to_string(),
                property_name: format!("q-{}", band),
                transform: None,
            },
        });
    }

    // Main limiter
    exposed_properties.push(ExposedProperty {
        name: "main_limiter_enabled".to_string(),
        label: "Main Limiter".to_string(),
        description: "Enable limiter on main bus".to_string(),
        property_type: PropertyType::Bool,
        default_value: Some(PropertyValue::Bool(false)),
        mapping: PropertyMapping {
            element_id: "main_limiter".to_string(),
            property_name: "enabled".to_string(),
            transform: None,
        },
    });
    exposed_properties.push(ExposedProperty {
        name: "main_limiter_threshold".to_string(),
        label: "Main Lim Thresh".to_string(),
        description: "Main bus limiter threshold in dB (-20 to 0)".to_string(),
        property_type: PropertyType::Float,
        default_value: Some(PropertyValue::Float(-0.3)),
        mapping: PropertyMapping {
            element_id: "main_limiter".to_string(),
            property_name: "th".to_string(),
            transform: Some("db_to_linear".to_string()),
        },
    });

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

    // Add group properties
    for sg in 1..=MAX_GROUPS {
        exposed_properties.push(ExposedProperty {
            name: format!("group{}_fader", sg),
            label: format!("Group {} Fader", sg),
            description: format!("Group {} level (0.0 to 2.0)", sg),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(1.0)),
            mapping: PropertyMapping {
                element_id: format!("group{}_volume", sg - 1),
                property_name: "volume".to_string(),
                transform: None,
            },
        });
        exposed_properties.push(ExposedProperty {
            name: format!("group{}_mute", sg),
            label: format!("Group {} Mute", sg),
            description: format!("Mute group {}", sg),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(false)),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: format!("group{}_mute", sg),
                transform: None,
            },
        });
    }

    // Add per-channel properties (we'll generate for max channels, UI will show based on num_channels)
    for ch in 1..=MAX_CHANNELS {
        // Channel label
        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_label", ch),
            label: format!("Ch {} Label", ch),
            description: format!("Channel {} display name", ch),
            property_type: PropertyType::String,
            default_value: Some(PropertyValue::String(format!("Ch {}", ch))),
            mapping: PropertyMapping {
                element_id: "_block".to_string(),
                property_name: format!("ch{}_label", ch),
                transform: None,
            },
        });

        // Input gain
        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_gain", ch),
            label: format!("Ch {} Gain", ch),
            description: format!("Channel {} input gain in dB (-20 to +20)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(0.0)),
            mapping: PropertyMapping {
                element_id: format!("gain_{}", ch - 1),
                property_name: "volume".to_string(),
                transform: Some("db_to_linear".to_string()),
            },
        });

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
            label: format!("Ch {} -> Main", ch),
            description: format!("Route channel {} to main mix", ch),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(true)),
            mapping: PropertyMapping {
                element_id: format!("to_main_vol_{}", ch - 1),
                property_name: "volume".to_string(),
                transform: Some("bool_to_volume".to_string()),
            },
        });

        // Routing to groups
        for sg in 1..=MAX_GROUPS {
            exposed_properties.push(ExposedProperty {
                name: format!("ch{}_to_grp{}", ch, sg),
                label: format!("Ch {} -> SG{}", ch, sg),
                description: format!("Route channel {} to group {}", ch, sg),
                property_type: PropertyType::Bool,
                default_value: Some(PropertyValue::Bool(false)),
                mapping: PropertyMapping {
                    element_id: format!("to_grp{}_vol_{}", sg - 1, ch - 1),
                    property_name: "volume".to_string(),
                    transform: Some("bool_to_volume".to_string()),
                },
            });
        }

        // Aux send levels and pre/post toggle (per aux bus)
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
            exposed_properties.push(ExposedProperty {
                name: format!("ch{}_aux{}_pre", ch, aux),
                label: format!("Ch {} Aux {} Pre", ch, aux),
                description: format!(
                    "Channel {} aux {} pre-fader (true) or post-fader (false)",
                    ch, aux
                ),
                property_type: PropertyType::Bool,
                default_value: Some(PropertyValue::Bool(aux <= 2)), // aux 1-2 pre, 3-4 post
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: format!("ch{}_aux{}_pre", ch, aux),
                    transform: None,
                },
            });
        }

        // ============================================================
        // HPF properties
        // ============================================================
        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_hpf_enabled", ch),
            label: format!("Ch {} HPF", ch),
            description: format!("Enable high-pass filter on channel {}", ch),
            property_type: PropertyType::Bool,
            default_value: Some(PropertyValue::Bool(false)),
            mapping: PropertyMapping {
                element_id: format!("hpf_{}", ch - 1),
                property_name: "cutoff".to_string(),
                transform: None,
            },
        });

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_hpf_freq", ch),
            label: format!("Ch {} HPF Freq", ch),
            description: format!(
                "Channel {} high-pass filter cutoff frequency in Hz (20-500)",
                ch
            ),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(80.0)),
            mapping: PropertyMapping {
                element_id: format!("hpf_{}", ch - 1),
                property_name: "cutoff".to_string(),
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

        // Note: LSP gate has no settable range property
        // ("rr" doesn't exist, "gr" is a read-only reduction meter)

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

        exposed_properties.push(ExposedProperty {
            name: format!("ch{}_comp_knee", ch),
            label: format!("Ch {} Comp Knee", ch),
            description: format!("Channel {} compressor knee in dB (-24 to 0)", ch),
            property_type: PropertyType::Float,
            default_value: Some(PropertyValue::Float(-6.0)),
            mapping: PropertyMapping {
                element_id: format!("comp_{}", ch - 1),
                property_name: "kn".to_string(),
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
        description: "Stereo audio mixer with per-channel gain, gate, compressor, EQ, pan, fader, mute and metering. Main bus with compressor, EQ and limiter. Supports aux sends (pre/post) and subgroups.".to_string(),
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
                    internal_element_id: "main_out_tee".to_string(),
                    internal_pad_name: "src_%u".to_string(),
                },
                ExternalPad {
                    name: "pfl_out".to_string(),
                    label: Some("PFL".to_string()),
                    media_type: MediaType::Audio,
                    internal_element_id: "pfl_out_tee".to_string(),
                    internal_pad_name: "src_%u".to_string(),
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn init_gst() {
        let _ = gst::init();
        let _ = gst_plugins_lsp::plugin_register_static();
    }

    fn is_element_available(name: &str) -> bool {
        gst::ElementFactory::make(name).build().is_ok()
    }

    // ---- Pure function tests (no GStreamer needed) ----

    #[test]
    fn test_db_to_linear_unity() {
        let result = db_to_linear(0.0);
        assert!((result - 1.0).abs() < 1e-10, "0 dB should be 1.0 linear");
    }

    #[test]
    fn test_db_to_linear_minus_6() {
        let result = db_to_linear(-6.0);
        assert!(
            (result - 0.5012).abs() < 0.001,
            "-6 dB should be ~0.501, got {}",
            result
        );
    }

    #[test]
    fn test_db_to_linear_minus_20() {
        let result = db_to_linear(-20.0);
        assert!(
            (result - 0.1).abs() < 1e-10,
            "-20 dB should be 0.1, got {}",
            result
        );
    }

    #[test]
    fn test_db_to_linear_minus_60() {
        let result = db_to_linear(-60.0);
        assert!(
            (result - 0.001).abs() < 1e-10,
            "-60 dB should be 0.001, got {}",
            result
        );
    }

    #[test]
    fn test_db_to_linear_plus_6() {
        let result = db_to_linear(6.0);
        assert!(
            (result - 1.9953).abs() < 0.001,
            "+6 dB should be ~1.995, got {}",
            result
        );
    }

    #[test]
    fn test_parse_num_channels_default() {
        let props = HashMap::new();
        assert_eq!(parse_num_channels(&props), DEFAULT_CHANNELS);
    }

    #[test]
    fn test_parse_num_channels_from_string() {
        let mut props = HashMap::new();
        props.insert(
            "num_channels".to_string(),
            PropertyValue::String("4".to_string()),
        );
        assert_eq!(parse_num_channels(&props), 4);
    }

    #[test]
    fn test_parse_num_channels_clamped() {
        let mut props = HashMap::new();
        props.insert(
            "num_channels".to_string(),
            PropertyValue::String("100".to_string()),
        );
        assert_eq!(parse_num_channels(&props), MAX_CHANNELS);

        props.insert(
            "num_channels".to_string(),
            PropertyValue::String("0".to_string()),
        );
        assert_eq!(parse_num_channels(&props), 1);
    }

    #[test]
    fn test_parse_num_aux_buses_default() {
        let props = HashMap::new();
        assert_eq!(parse_num_aux_buses(&props), 0);
    }

    #[test]
    fn test_parse_num_aux_buses_clamped() {
        let mut props = HashMap::new();
        props.insert(
            "num_aux_buses".to_string(),
            PropertyValue::String("10".to_string()),
        );
        assert_eq!(parse_num_aux_buses(&props), MAX_AUX_BUSES);
    }

    #[test]
    fn test_parse_num_groups_default() {
        let props = HashMap::new();
        assert_eq!(parse_num_groups(&props), 0);
    }

    #[test]
    fn test_get_float_prop_default() {
        let props = HashMap::new();
        assert_eq!(get_float_prop(&props, "volume", 0.5), 0.5);
    }

    #[test]
    fn test_get_float_prop_value() {
        let mut props = HashMap::new();
        props.insert("volume".to_string(), PropertyValue::Float(0.75));
        assert_eq!(get_float_prop(&props, "volume", 0.5), 0.75);
    }

    #[test]
    fn test_get_float_prop_from_int() {
        let mut props = HashMap::new();
        props.insert("volume".to_string(), PropertyValue::Int(3));
        assert_eq!(get_float_prop(&props, "volume", 0.5), 3.0);
    }

    #[test]
    fn test_get_bool_prop_default() {
        let props = HashMap::new();
        assert!(!get_bool_prop(&props, "mute", false));
        assert!(get_bool_prop(&props, "mute", true));
    }

    #[test]
    fn test_get_bool_prop_value() {
        let mut props = HashMap::new();
        props.insert("mute".to_string(), PropertyValue::Bool(true));
        assert!(get_bool_prop(&props, "mute", false));
    }

    #[test]
    fn test_get_string_prop_default() {
        let props = HashMap::new();
        assert_eq!(get_string_prop(&props, "mode", "pfl"), "pfl");
    }

    #[test]
    fn test_get_string_prop_value() {
        let mut props = HashMap::new();
        props.insert("mode".to_string(), PropertyValue::String("afl".to_string()));
        assert_eq!(get_string_prop(&props, "mode", "pfl"), "afl");
    }

    #[test]
    fn test_comp_knee_db_to_linear_in_range() {
        // Default knee -6 dB should map to ~0.5 (within LSP range 0.0631..1.0)
        let kn = db_to_linear(-6.0).clamp(0.0631, 1.0);
        assert!(kn > 0.49 && kn < 0.52, "Knee -6dB = {}, expected ~0.5", kn);

        // 0 dB should map to 1.0 (max)
        let kn = db_to_linear(0.0).clamp(0.0631, 1.0);
        assert!((kn - 1.0).abs() < 1e-6, "Knee 0dB = {}, expected 1.0", kn);

        // -24 dB should map to ~0.063 (near min)
        let kn = db_to_linear(-24.0).clamp(0.0631, 1.0);
        assert!(kn >= 0.0631, "Knee -24dB = {}, should be >= 0.0631", kn);

        // +6 dB would exceed max, should clamp to 1.0
        let kn = db_to_linear(6.0).clamp(0.0631, 1.0);
        assert!((kn - 1.0).abs() < 1e-6, "Knee +6dB should clamp to 1.0");
    }

    // ---- Property mapping tests ----

    #[test]
    fn test_mixer_definition_has_bypass_mappings() {
        let def = mixer_definition();
        let bypass_props = [
            "main_comp_enabled",
            "main_eq_enabled",
            "main_limiter_enabled",
        ];
        for prop_name in &bypass_props {
            let prop = def
                .exposed_properties
                .iter()
                .find(|p| p.name == *prop_name)
                .unwrap_or_else(|| panic!("Missing property: {}", prop_name));
            assert_eq!(
                prop.mapping.property_name, "enabled",
                "{} should map to 'enabled', got '{}'",
                prop_name, prop.mapping.property_name
            );
            assert_eq!(
                prop.mapping.transform, None,
                "{} should have no transform",
                prop_name
            );
        }
    }

    #[test]
    fn test_mixer_definition_channel_bypass_mappings() {
        let def = mixer_definition();
        // Check that per-channel gate/comp/eq enabled properties map to bypass
        for suffix in &["gate_enabled", "comp_enabled", "eq_enabled"] {
            let prop_name = format!("ch1_{}", suffix);
            let prop = def
                .exposed_properties
                .iter()
                .find(|p| p.name == prop_name)
                .unwrap_or_else(|| panic!("Missing property: {}", prop_name));
            assert_eq!(
                prop.mapping.property_name, "enabled",
                "{} should map to 'enabled', got '{}'",
                prop_name, prop.mapping.property_name
            );
            assert_eq!(
                prop.mapping.transform, None,
                "{} should have no transform",
                prop_name
            );
        }
    }

    #[test]
    fn test_mixer_definition_no_gate_range_property() {
        let def = mixer_definition();
        // There should be no gate range exposed property (LSP doesn't support it)
        let gate_range = def
            .exposed_properties
            .iter()
            .find(|p| p.name.contains("gate_range"));
        assert!(
            gate_range.is_none(),
            "Gate range property should not exist (LSP has no settable range)"
        );
    }

    #[test]
    fn test_mixer_definition_comp_knee_defaults() {
        let def = mixer_definition();
        let knee = def
            .exposed_properties
            .iter()
            .find(|p| p.name == "ch1_comp_knee")
            .expect("Missing ch1_comp_knee");
        match &knee.default_value {
            Some(PropertyValue::Float(v)) => assert!(
                (*v - (-6.0)).abs() < 1e-6,
                "Knee default should be -6.0 dB, got {}",
                v
            ),
            other => panic!("Knee default should be Float(-6.0), got {:?}", other),
        }
        assert_eq!(
            knee.mapping.transform,
            Some("db_to_linear".to_string()),
            "Knee should have db_to_linear transform"
        );
    }

    #[test]
    fn test_mixer_definition_db_to_linear_transforms() {
        let def = mixer_definition();
        // Properties that should have db_to_linear transform
        let db_props = [
            "ch1_gate_threshold",
            "ch1_comp_threshold",
            "ch1_comp_makeup",
            "ch1_comp_knee",
            "main_comp_threshold",
            "main_comp_makeup",
        ];
        for prop_name in &db_props {
            let prop = def
                .exposed_properties
                .iter()
                .find(|p| p.name == *prop_name)
                .unwrap_or_else(|| panic!("Missing property: {}", prop_name));
            assert_eq!(
                prop.mapping.transform,
                Some("db_to_linear".to_string()),
                "{} should have 'db_to_linear' transform, got {:?}",
                prop_name,
                prop.mapping.transform
            );
        }
    }

    #[test]
    fn test_mixer_definition_channel_count() {
        let def = mixer_definition();
        // Default is 8 channels, should have properties for ch1..ch8
        let ch8_fader = def
            .exposed_properties
            .iter()
            .find(|p| p.name == "ch8_fader");
        assert!(
            ch8_fader.is_some(),
            "Should have ch8_fader for default 8 channels"
        );
    }

    #[test]
    fn test_mixer_definition_aux_group_outputs() {
        let def = mixer_definition();
        // Should have main, PFL, aux, and group output pads
        let pads = &def.external_pads;
        assert!(
            pads.outputs.iter().any(|p| p.name == "main_out"),
            "Should have main_out pad"
        );
        assert!(
            pads.outputs.iter().any(|p| p.name == "pfl_out"),
            "Should have pfl_out pad"
        );
    }

    // ---- GStreamer element tests (conditional on plugin availability) ----

    #[test]
    fn test_make_gate_element_lsp() {
        init_gst();
        if !is_element_available("lsp-plug-in-plugins-lv2-gate-stereo") {
            println!("LSP gate not available, skipping");
            return;
        }
        let gate = make_gate_element("test_gate", true, -40.0, 5.0, 100.0, -80.0, "lv2");
        assert!(gate.is_ok(), "Should create gate element");
        let gate = gate.unwrap();

        // Verify bypass property was set (enabled=true means bypass=false)
        if gate.find_property("enabled").is_some() {
            let enabled_val: bool = gate.property("enabled");
            assert!(enabled_val, "Gate enabled=true should set enabled=true");
        }
    }

    #[test]
    fn test_make_gate_element_disabled() {
        init_gst();
        if !is_element_available("lsp-plug-in-plugins-lv2-gate-stereo") {
            println!("LSP gate not available, skipping");
            return;
        }
        let gate = make_gate_element("test_gate_off", false, -40.0, 5.0, 100.0, -80.0, "lv2");
        assert!(gate.is_ok());
        let gate = gate.unwrap();

        if gate.find_property("enabled").is_some() {
            let enabled_val: bool = gate.property("enabled");
            assert!(!enabled_val, "Gate enabled=false should set enabled=false");
        }
    }

    #[test]
    fn test_make_compressor_element_lsp() {
        init_gst();
        if !is_element_available("lsp-plug-in-plugins-lv2-compressor-stereo") {
            println!("LSP compressor not available, skipping");
            return;
        }
        let comp = make_compressor_element("test_comp", true, -20.0, 4.0, 10.0, 100.0, 0.0, "lv2");
        assert!(comp.is_ok(), "Should create compressor element");
        let comp = comp.unwrap();

        if comp.find_property("enabled").is_some() {
            let enabled_val: bool = comp.property("enabled");
            assert!(enabled_val, "Comp enabled=true should set enabled=true");
        }

        // Verify threshold was converted to linear
        if comp.find_property("al").is_some() {
            let al: f32 = comp.property("al");
            let expected = db_to_linear(-20.0) as f32;
            assert!(
                (al - expected).abs() < 0.001,
                "Threshold -20dB: expected {}, got {}",
                expected,
                al
            );
        }
    }

    #[test]
    fn test_make_eq_element_lsp() {
        init_gst();
        if !is_element_available("lsp-plug-in-plugins-lv2-para-equalizer-x8-stereo") {
            println!("LSP EQ not available, skipping");
            return;
        }
        let bands = [
            (1000.0, 0.0, 1.0),
            (2000.0, 3.0, 1.0),
            (4000.0, -3.0, 1.0),
            (8000.0, 0.0, 1.0),
        ];
        let eq = make_eq_element("test_eq", true, &bands, "lv2");
        assert!(eq.is_ok(), "Should create EQ element");
        let eq = eq.unwrap();

        if eq.find_property("enabled").is_some() {
            let enabled_val: bool = eq.property("enabled");
            assert!(enabled_val, "EQ enabled=true should set enabled=true");
        }

        // Verify first band frequency
        if eq.find_property("f-0").is_some() {
            let f0: f32 = eq.property("f-0");
            assert!(
                (f0 - 1000.0).abs() < 1.0,
                "Band 0 freq should be 1000, got {}",
                f0
            );
        }
    }

    #[test]
    fn test_make_limiter_element_lsp() {
        init_gst();
        if !is_element_available("lsp-plug-in-plugins-lv2-limiter-stereo") {
            println!("LSP limiter not available, skipping");
            return;
        }
        let lim = make_limiter_element("test_lim", true, -3.0, "lv2");
        assert!(lim.is_ok(), "Should create limiter element");
    }

    #[test]
    fn test_make_hpf_element() {
        init_gst();
        if !is_element_available("audiocheblimit") && !is_element_available("audiowsinclimit") {
            println!("No HPF element available, skipping");
            return;
        }
        let hpf = make_hpf_element("test_hpf", true, 80.0);
        assert!(hpf.is_ok(), "Should create HPF element");
    }

    #[test]
    fn test_make_hpf_element_disabled_uses_min_cutoff() {
        init_gst();
        if !is_element_available("audiocheblimit") {
            println!("audiocheblimit not available, skipping");
            return;
        }
        let hpf = make_hpf_element("test_hpf_off", false, 80.0);
        assert!(hpf.is_ok());
        let hpf = hpf.unwrap();

        if hpf.find_property("cutoff").is_some() {
            let cutoff: f32 = hpf.property("cutoff");
            assert!(
                (cutoff - 1.0).abs() < 0.1,
                "Disabled HPF should have cutoff=1.0, got {}",
                cutoff
            );
        }
    }

    #[test]
    fn test_make_audiomixer() {
        init_gst();
        if !is_element_available("audiomixer") {
            println!("audiomixer not available, skipping");
            return;
        }
        let mixer = make_audiomixer("test_mixer", true, 30, 30);
        assert!(mixer.is_ok(), "Should create audiomixer: {:?}", mixer.err());
    }

    #[test]
    fn test_make_gate_fallback_to_identity() {
        init_gst();
        // If LSP is available this just tests normal path, but it shouldn't panic
        let gate = make_gate_element("test_gate_fb", true, -40.0, 5.0, 100.0, -80.0, "lv2");
        assert!(
            gate.is_ok(),
            "Gate should succeed (LSP or identity fallback)"
        );
    }

    #[test]
    fn test_make_compressor_fallback_to_identity() {
        init_gst();
        let comp =
            make_compressor_element("test_comp_fb", true, -20.0, 4.0, 10.0, 100.0, 0.0, "lv2");
        assert!(
            comp.is_ok(),
            "Compressor should succeed (LSP or identity fallback)"
        );
    }

    #[test]
    fn test_make_eq_fallback_to_identity() {
        init_gst();
        let bands = [
            (100.0, 0.0, 1.0),
            (1000.0, 0.0, 1.0),
            (5000.0, 0.0, 1.0),
            (10000.0, 0.0, 1.0),
        ];
        let eq = make_eq_element("test_eq_fb", true, &bands, "lv2");
        assert!(eq.is_ok(), "EQ should succeed (LSP or identity fallback)");
    }

    #[test]
    fn test_extract_level_values_empty() {
        init_gst();
        let structure = gst::Structure::builder("level").build();
        let values = extract_level_values(structure.as_ref(), "peak");
        assert!(
            values.is_empty(),
            "Should return empty vec for missing field"
        );
    }

    // ---- Rust backend (lsp-plugins-rs) tests ----

    #[test]
    fn test_make_gate_element_rust() {
        init_gst();
        let gate = make_gate_element("test_gate_rs", true, -40.0, 5.0, 100.0, -80.0, "rust");
        assert!(
            gate.is_ok(),
            "Should create gate element (rust or fallback): {:?}",
            gate.err()
        );
        let gate = gate.unwrap();
        // Use find_property to check element type (factory() can SIGSEGV in test context)
        if gate.find_property("open-threshold").is_some() {
            let enabled_val: bool = gate.property("enabled");
            assert!(enabled_val, "Gate should be enabled");
            let thresh: f32 = gate.property("open-threshold");
            assert!(
                (thresh - (-40.0)).abs() < 0.1,
                "Threshold should be -40 dB, got {}",
                thresh
            );
        }
    }

    #[test]
    fn test_make_gate_element_rust_disabled() {
        init_gst();
        let gate = make_gate_element("test_gate_rs_off", false, -40.0, 5.0, 100.0, -80.0, "rust");
        assert!(gate.is_ok());
        let gate = gate.unwrap();
        if gate.find_property("open-threshold").is_some() {
            let enabled_val: bool = gate.property("enabled");
            assert!(!enabled_val, "Gate should be disabled");
        }
    }

    #[test]
    fn test_make_compressor_element_rust() {
        init_gst();
        let comp =
            make_compressor_element("test_comp_rs", true, -20.0, 4.0, 10.0, 100.0, 6.0, "rust");
        assert!(
            comp.is_ok(),
            "Should create compressor element (rust or fallback): {:?}",
            comp.err()
        );
        let comp = comp.unwrap();
        if comp.find_property("ratio").is_some() {
            let enabled_val: bool = comp.property("enabled");
            assert!(enabled_val, "Compressor should be enabled");
            let ratio: f32 = comp.property("ratio");
            assert!(
                (ratio - 4.0).abs() < 0.1,
                "Ratio should be 4.0, got {}",
                ratio
            );
        }
    }

    #[test]
    fn test_make_eq_element_rust() {
        init_gst();
        let bands = [
            (1000.0, 3.0, 1.0),
            (2000.0, -3.0, 2.0),
            (4000.0, 0.0, 1.0),
            (8000.0, 6.0, 0.7),
        ];
        let eq = make_eq_element("test_eq_rs", true, &bands, "rust");
        assert!(
            eq.is_ok(),
            "Should create EQ element (rust or fallback): {:?}",
            eq.err()
        );
        let eq = eq.unwrap();
        if eq.find_property("band0-frequency").is_some() {
            let enabled_val: bool = eq.property("enabled");
            assert!(enabled_val, "EQ should be enabled");
            let f0: f32 = eq.property("band0-frequency");
            assert!(
                (f0 - 1000.0).abs() < 1.0,
                "Band 0 freq should be 1000, got {}",
                f0
            );
            // Rust EQ gain is dB directly
            let g0: f32 = eq.property("band0-gain");
            assert!(
                (g0 - 3.0).abs() < 0.1,
                "Band 0 gain should be 3.0 dB, got {}",
                g0
            );
        }
    }

    #[test]
    fn test_make_limiter_element_rust() {
        init_gst();
        let lim = make_limiter_element("test_lim_rs", true, -3.0, "rust");
        assert!(
            lim.is_ok(),
            "Should create limiter element (rust or fallback): {:?}",
            lim.err()
        );
        let lim = lim.unwrap();
        if lim.find_property("lookahead").is_some() {
            let enabled_val: bool = lim.property("enabled");
            assert!(enabled_val, "Limiter should be enabled");
            let thresh: f32 = lim.property("threshold");
            assert!(
                (thresh - (-3.0)).abs() < 0.1,
                "Threshold should be -3 dB, got {}",
                thresh
            );
        }
    }

    // ---- Property translation tests ----

    #[test]
    fn test_linear_to_db() {
        assert!(
            (linear_to_db(1.0) - 0.0).abs() < 1e-6,
            "1.0 linear should be 0 dB"
        );
        assert!(
            (linear_to_db(0.1) - (-20.0)).abs() < 1e-6,
            "0.1 linear should be -20 dB"
        );
    }

    #[test]
    fn test_linear_to_db_zero() {
        let result = linear_to_db(0.0);
        assert!(
            result <= -120.0,
            "0.0 linear should be <= -120 dB, got {}",
            result
        );
    }

    #[test]
    fn test_translate_gate_property() {
        init_gst();
        if !is_element_available("lsp-rs-gate") {
            println!("lsp-rs-gate not available, skipping translation test");
            return;
        }
        let gate = gst::ElementFactory::make("lsp-rs-gate")
            .name("translate_test_gate")
            .build()
            .unwrap();

        // gt (linear) -> open-threshold (dB): 0.1 linear = -20 dB
        let result = translate_property_for_element(&gate, "gt", &PropertyValue::Float(0.1));
        assert!(result.is_some(), "Should translate 'gt' for lsp-rs-gate");
        let (name, value) = result.unwrap();
        assert_eq!(name, "open-threshold");
        if let PropertyValue::Float(v) = value {
            assert!(
                (v - (-20.0)).abs() < 0.1,
                "0.1 linear should translate to -20 dB, got {}",
                v
            );
        } else {
            panic!("Expected Float value");
        }
    }

    #[test]
    fn test_translate_compressor_property() {
        init_gst();
        if !is_element_available("lsp-rs-compressor") {
            println!("lsp-rs-compressor not available, skipping translation test");
            return;
        }
        let comp = gst::ElementFactory::make("lsp-rs-compressor")
            .name("translate_test_comp")
            .build()
            .unwrap();

        // al -> threshold (both linear, no value change)
        let result = translate_property_for_element(&comp, "al", &PropertyValue::Float(0.1));
        assert!(result.is_some());
        let (name, _) = result.unwrap();
        assert_eq!(name, "threshold");

        // cr -> ratio
        let result = translate_property_for_element(&comp, "cr", &PropertyValue::Float(4.0));
        assert!(result.is_some());
        let (name, _) = result.unwrap();
        assert_eq!(name, "ratio");

        // enabled -> no translation needed
        let result = translate_property_for_element(&comp, "enabled", &PropertyValue::Bool(true));
        assert!(result.is_none(), "enabled should not need translation");
    }

    #[test]
    fn test_translate_eq_property() {
        init_gst();
        if !is_element_available("lsp-rs-equalizer") {
            println!("lsp-rs-equalizer not available, skipping translation test");
            return;
        }
        let eq = gst::ElementFactory::make("lsp-rs-equalizer")
            .name("translate_test_eq")
            .build()
            .unwrap();

        // f-0 -> band0-frequency
        let result = translate_property_for_element(&eq, "f-0", &PropertyValue::Float(1000.0));
        assert!(result.is_some());
        let (name, _) = result.unwrap();
        assert_eq!(name, "band0-frequency");

        // g-0 (linear) -> band0-gain (dB)
        let result = translate_property_for_element(&eq, "g-0", &PropertyValue::Float(1.0));
        assert!(result.is_some());
        let (name, value) = result.unwrap();
        assert_eq!(name, "band0-gain");
        if let PropertyValue::Float(v) = value {
            assert!(
                v.abs() < 0.1,
                "1.0 linear should translate to 0 dB, got {}",
                v
            );
        }
    }

    #[test]
    fn test_translate_limiter_property() {
        init_gst();
        if !is_element_available("lsp-rs-limiter") {
            println!("lsp-rs-limiter not available, skipping translation test");
            return;
        }
        let lim = gst::ElementFactory::make("lsp-rs-limiter")
            .name("translate_test_lim")
            .build()
            .unwrap();

        // th (linear) -> threshold (dB)
        let result = translate_property_for_element(&lim, "th", &PropertyValue::Float(0.1));
        assert!(result.is_some());
        let (name, value) = result.unwrap();
        assert_eq!(name, "threshold");
        if let PropertyValue::Float(v) = value {
            assert!(
                (v - (-20.0)).abs() < 0.1,
                "0.1 linear should translate to -20 dB, got {}",
                v
            );
        }
    }

    #[test]
    fn test_translate_no_translation_for_lv2() {
        init_gst();
        // For LV2 elements (or any non-lsp-rs element), translation should return None
        if let Ok(elem) = gst::ElementFactory::make("identity")
            .name("translate_test_identity")
            .build()
        {
            let result = translate_property_for_element(&elem, "gt", &PropertyValue::Float(0.1));
            assert!(
                result.is_none(),
                "Should not translate properties for non-lsp-rs elements"
            );
        }
    }

    #[test]
    fn test_mixer_definition_has_dsp_backend_property() {
        let def = mixer_definition();
        let dsp_prop = def
            .exposed_properties
            .iter()
            .find(|p| p.name == "dsp_backend");
        assert!(dsp_prop.is_some(), "Should have dsp_backend property");
        let dsp_prop = dsp_prop.unwrap();
        match &dsp_prop.default_value {
            Some(PropertyValue::String(s)) => {
                assert_eq!(s, "lv2", "Default should be lv2, got {}", s);
            }
            other => panic!("Expected String(\"lv2\"), got {:?}", other),
        }
        match &dsp_prop.property_type {
            PropertyType::Enum { values } => {
                assert_eq!(values.len(), 2);
                assert_eq!(values[0].value, "lv2");
                assert_eq!(values[1].value, "rust");
            }
            other => panic!("Expected Enum type, got {:?}", other),
        }
    }
}
