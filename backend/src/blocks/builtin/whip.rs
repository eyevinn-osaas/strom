//! WHIP (WebRTC-HTTP Ingestion Protocol) block builders.
//!
//! WHIP Output - Sends media to an external WHIP server:
//! - `whipclientsink` (new): Uses signaller interface, handles encoding internally
//! - `whipsink` (legacy): Simpler implementation, requires pre-encoded RTP input
//!
//! WHIP Input - Hosts a WHIP server for clients to connect and send media:
//! - `whipserversrc`: One element per WHIP client session, created dynamically
//!   by the WhipSessionManager when a client POSTs an SDP offer.
//!   Downstream elements (liveadder, videoconvert) are persistent and shared.
//!
//! Note: WHIP is a send-only protocol, but SMB (Symphony Media Bridge) may still
//! send RTP back to the whipsink. For `whipsink`, we handle this by detecting the
//! internal webrtcbin and linking any incoming source pads to a fakesink to prevent
//! "not-linked" errors. This workaround does not work for `whipclientsink` due to
//! its different internal structure (webrtcbin is not a direct child of the sink bin).

use crate::blocks::{
    BlockBuildContext, BlockBuildError, BlockBuildResult, BlockBuilder, WhepStreamMode,
};
use crate::whip_session_manager::WhipEndpointConfig;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use strom_types::{block::*, element::ElementPadRef, PropertyValue, *};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// WHIP Output block builder.
pub struct WHIPOutputBuilder;

/// WHIP Input block builder (hosts WHIP server).
pub struct WHIPInputBuilder;

impl BlockBuilder for WHIPOutputBuilder {
    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
        ctx: &BlockBuildContext,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        debug!("Building WHIP Output block instance: {}", instance_id);

        // Get implementation choice (default to stable whipsink)
        let use_new = properties
            .get("implementation")
            .and_then(|v| {
                if let PropertyValue::String(s) = v {
                    Some(s == "whipclientsink")
                } else {
                    None
                }
            })
            .unwrap_or(false);

        if use_new {
            build_whipclientsink(instance_id, properties, ctx)
        } else {
            build_whipsink(instance_id, properties, ctx)
        }
    }
}

impl BlockBuilder for WHIPInputBuilder {
    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
        ctx: &BlockBuildContext,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        debug!("Building WHIP Input block instance: {}", instance_id);
        build_whipserversrc(instance_id, properties, ctx)
    }

    fn get_external_pads(
        &self,
        properties: &HashMap<String, PropertyValue>,
    ) -> Option<ExternalPads> {
        let mode = properties
            .get("mode")
            .and_then(|v| match v {
                PropertyValue::String(s) => Some(WhepStreamMode::parse(s)),
                _ => None,
            })
            .unwrap_or(WhepStreamMode::AudioVideo);

        let mut outputs = Vec::new();

        if mode.has_video() {
            outputs.push(ExternalPad {
                label: Some("V0".to_string()),
                name: "video_out".to_string(),
                media_type: MediaType::Video,
                internal_element_id: "video_out_tee".to_string(),
                internal_pad_name: "src_%u".to_string(),
            });
        }

        if mode.has_audio() {
            outputs.push(ExternalPad {
                label: Some("A0".to_string()),
                name: "audio_out".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "audio_out_tee".to_string(),
                internal_pad_name: "src_%u".to_string(),
            });
        }

        Some(ExternalPads {
            inputs: vec![],
            outputs,
        })
    }
}

// ============================================================================
// WHIP Input (whipserversrc - hosts WHIP server)
// ============================================================================

/// Build WHIP Input downstream elements (liveadder, videoconvert).
///
/// At build time, only persistent downstream elements are created.
/// The actual whipserversrc elements are created dynamically per-session
/// by `create_whipserversrc_for_session` when clients connect.
fn build_whipserversrc(
    instance_id: &str,
    properties: &HashMap<String, PropertyValue>,
    ctx: &BlockBuildContext,
) -> Result<BlockBuildResult, BlockBuildError> {
    info!("Building WHIP Input downstream elements (per-session whipserversrc)");

    // Get mode (audio_video, audio, or video)
    let mode = properties
        .get("mode")
        .and_then(|v| match v {
            PropertyValue::String(s) => Some(WhepStreamMode::parse(s)),
            _ => None,
        })
        .unwrap_or(WhepStreamMode::AudioVideo);

    info!("WHIP Input mode: {:?}", mode);

    // Get endpoint_id (user-configurable, defaults to UUID)
    let endpoint_id = properties
        .get("endpoint_id")
        .and_then(|v| {
            if let PropertyValue::String(s) = v {
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            } else {
                None
            }
        })
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Create downstream elements (these outlive individual WebRTC sessions)
    let mut elements: Vec<(String, gst::Element)> = Vec::new();
    let mut internal_links: Vec<(ElementPadRef, ElementPadRef)> = Vec::new();

    // Create audio output chain if mode includes audio
    // Chain: liveadder → capsfilter → audioconvert → audioresample → audio_out_tee
    // The output tee has allow-not-linked=true to prevent backpressure from stalling
    // the webrtcbin's NiceAgent when downstream can't consume fast enough.
    if mode.has_audio() {
        let liveadder_id = format!("{}:liveadder", instance_id);
        let capsfilter_id = format!("{}:capsfilter", instance_id);
        let output_audioconvert_id = format!("{}:output_audioconvert", instance_id);
        let output_audioresample_id = format!("{}:output_audioresample", instance_id);
        let audio_out_tee_id = format!("{}:audio_out_tee", instance_id);

        let liveadder = gst::ElementFactory::make("liveadder")
            .name(&liveadder_id)
            .property("latency", 30u32)
            .property("force-live", true)
            .property_from_str("start-time-selection", "first")
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("liveadder: {}", e)))?;

        let caps = gst::Caps::builder("audio/x-raw")
            .field("rate", 48000i32)
            .field("channels", 2i32)
            .build();
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .name(&capsfilter_id)
            .property("caps", &caps)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("capsfilter: {}", e)))?;

        let output_audioconvert = gst::ElementFactory::make("audioconvert")
            .name(&output_audioconvert_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("output_audioconvert: {}", e)))?;

        let output_audioresample = gst::ElementFactory::make("audioresample")
            .name(&output_audioresample_id)
            .build()
            .map_err(|e| {
                BlockBuildError::ElementCreation(format!("output_audioresample: {}", e))
            })?;

        let audio_out_tee = gst::ElementFactory::make("tee")
            .name(&audio_out_tee_id)
            .property("allow-not-linked", true)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("audio_out_tee: {}", e)))?;

        internal_links.push((
            ElementPadRef::pad(&liveadder_id, "src"),
            ElementPadRef::pad(&capsfilter_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&capsfilter_id, "src"),
            ElementPadRef::pad(&output_audioconvert_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&output_audioconvert_id, "src"),
            ElementPadRef::pad(&output_audioresample_id, "sink"),
        ));
        internal_links.push((
            ElementPadRef::pad(&output_audioresample_id, "src"),
            ElementPadRef::pad(&audio_out_tee_id, "sink"),
        ));

        elements.push((liveadder_id, liveadder));
        elements.push((capsfilter_id, capsfilter));
        elements.push((output_audioconvert_id, output_audioconvert));
        elements.push((output_audioresample_id, output_audioresample));
        elements.push((audio_out_tee_id, audio_out_tee));
    }

    // Create video output chain if mode includes video
    // Chain: output_videoconvert → video_out_tee
    // The output tee has allow-not-linked=true to prevent backpressure from stalling
    // the webrtcbin's NiceAgent when downstream can't consume fast enough.
    if mode.has_video() {
        let output_videoconvert_id = format!("{}:output_videoconvert", instance_id);
        let video_out_tee_id = format!("{}:video_out_tee", instance_id);

        let output_videoconvert = gst::ElementFactory::make("videoconvert")
            .name(&output_videoconvert_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("output_videoconvert: {}", e)))?;

        let video_out_tee = gst::ElementFactory::make("tee")
            .name(&video_out_tee_id)
            .property("allow-not-linked", true)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("video_out_tee: {}", e)))?;

        internal_links.push((
            ElementPadRef::pad(&output_videoconvert_id, "src"),
            ElementPadRef::pad(&video_out_tee_id, "sink"),
        ));

        elements.push((output_videoconvert_id, output_videoconvert));
        elements.push((video_out_tee_id, video_out_tee));
    }

    // Get weak refs for downstream elements used in session callbacks
    let liveadder_weak: Option<gst::glib::WeakRef<gst::Element>> = if mode.has_audio() {
        elements
            .iter()
            .find(|(id, _)| id.ends_with(":liveadder"))
            .map(|(_, e)| e.downgrade())
    } else {
        None
    };
    let videoconvert_weak: Option<gst::glib::WeakRef<gst::Element>> = if mode.has_video() {
        elements
            .iter()
            .find(|(id, _)| id.ends_with(":output_videoconvert"))
            .map(|(_, e)| e.downgrade())
    } else {
        None
    };

    let stun_server = ctx.stun_server();
    let turn_server = ctx.turn_server();

    let decode = properties
        .get("decode")
        .and_then(|v| match v {
            PropertyValue::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(true);

    info!(
        "WHIP Input configured: endpoint_id='{}', stun={:?}, turn={:?}, mode={:?}, decode={} (whipserversrc created per-session)",
        endpoint_id, stun_server, turn_server, mode, decode
    );

    // Register WHIP endpoint with the build context (port=0 placeholder, sessions get their own ports)
    ctx.register_whip_endpoint(instance_id, &endpoint_id, 0, mode);

    // Store endpoint config for the session manager (will be wired up in start_flow)
    ctx.register_whip_endpoint_config(
        endpoint_id,
        WhipEndpointConfig {
            instance_id: instance_id.to_string(),
            endpoint_id: String::new(), // will be set by the manager
            mode,
            stun_server,
            turn_server,
            ice_transport_policy: ctx.ice_transport_policy().to_string(),
            liveadder_weak,
            videoconvert_weak,
            pipeline_weak: gst::glib::WeakRef::new(),
            decode,
            dynamic_webrtcbin_store: ctx.dynamic_webrtcbin_store(),
        },
    );

    Ok(BlockBuildResult {
        elements,
        internal_links,
        bus_message_handler: None,
        pad_properties: HashMap::new(),
    })
}

/// Create a new whipserversrc element for a single WHIP client session.
///
/// Each session runs in its own isolated GStreamer pipeline to avoid
/// libnice issue #52 (multiple NiceAgent instances in the same pipeline
/// cause outbound UDP to stop working).
///
/// Decoded media is bridged to the main pipeline via appsink→appsrc.
///
/// Returns (element, session_pipeline, port) on success.
pub fn create_whipserversrc_for_session(
    config: &WhipEndpointConfig,
) -> Result<(gst::Element, gst::Pipeline, u16), String> {
    // Allocate a free port
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Failed to find free port: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {}", e))?
        .port();
    drop(listener);

    let host_addr = format!("http://127.0.0.1:{}", port);
    let session_uuid = Uuid::new_v4();
    let element_name = format!("{}:whipserversrc_{}", config.instance_id, session_uuid);

    info!(
        "WHIP Input: Creating whipserversrc '{}' on port {} in isolated pipeline",
        element_name, port
    );

    // Create an isolated pipeline for this session
    let session_pipeline = gst::Pipeline::builder()
        .name(format!("whip-session-{}", session_uuid))
        .build();

    // Create whipserversrc element
    let whipserversrc = gst::ElementFactory::make("whipserversrc")
        .name(&element_name)
        .build()
        .map_err(|e| format!("Failed to create whipserversrc: {}", e))?;

    // Set ICE server properties
    match config.stun_server {
        Some(ref stun) => whipserversrc.set_property("stun-server", stun),
        None => whipserversrc.set_property("stun-server", None::<&str>),
    }
    if let Some(ref turn) = config.turn_server {
        let turn_servers = gst::Array::new([turn]);
        whipserversrc.set_property("turn-servers", turn_servers);
    }

    // Set signaller host-addr
    let signaller = whipserversrc.property::<gst::glib::Object>("signaller");
    signaller.set_property("host-addr", &host_addr);

    // Configure codec negotiation based on mode
    if config.mode.has_audio() {
        let audio_codecs = gst::Array::new(["OPUS"]);
        whipserversrc.set_property("audio-codecs", &audio_codecs);
    } else {
        let empty = gst::Array::new(Vec::<&str>::new());
        whipserversrc.set_property("audio-codecs", &empty);
    }
    if config.mode.has_video() {
        let video_codecs = gst::Array::new(["H264"]);
        whipserversrc.set_property("video-codecs", &video_codecs);
    } else {
        let empty = gst::Array::new(Vec::<&str>::new());
        whipserversrc.set_property("video-codecs", &empty);
    }

    // deep-element-added: ICE policy, TWCC, keyframe recovery
    let dynamic_webrtcbin_store = config.dynamic_webrtcbin_store.clone();
    let block_id_for_callback = config.instance_id.clone();
    let ice_transport_policy = config.ice_transport_policy.clone();

    if let Ok(bin) = whipserversrc.clone().downcast::<gst::Bin>() {
        bin.connect("deep-element-added", false, move |values| {
            let element = values[2].get::<gst::Element>().unwrap();
            let element_name = element.name();

            if element_name.starts_with("webrtcbin") {
                if element.has_property("ice-transport-policy") {
                    element
                        .set_property_from_str("ice-transport-policy", &ice_transport_policy);
                    info!(
                        "WHIP Input: Set ice-transport-policy={} on webrtcbin {}",
                        ice_transport_policy, element_name
                    );
                }

                if let Ok(mut store) = dynamic_webrtcbin_store.lock() {
                    store
                        .entry(block_id_for_callback.clone())
                        .or_default()
                        .push(("whip-client".to_string(), element.clone()));
                }

                let wrtc_name = element_name.to_string();
                element.connect_notify(Some("ice-connection-state"), move |elem, _pspec| {
                    let val = elem.property_value("ice-connection-state");
                    info!(
                        "WHIP Input: [SERVER] {} ice-connection-state changed (raw: {:?})",
                        wrtc_name, val
                    );
                });
            }

            let factory_name = element
                .factory()
                .map(|f| f.name().to_string())
                .unwrap_or_default();
            if factory_name == "rtpsession" && element.has_property("internal-session") {
                let internal: gst::glib::Object = element.property("internal-session");
                if internal.has_property("twcc-feedback-interval") {
                    let interval: u64 = 200_000_000;
                    internal.set_property("twcc-feedback-interval", interval);
                    info!(
                        "WHIP Input: Set twcc-feedback-interval=200ms on {}",
                        element_name
                    );
                }
            }

            let element_klass = element
                .factory()
                .and_then(|f| f.metadata("klass").map(|s| s.to_string()))
                .unwrap_or_default();
            if element_klass.contains("Decoder") && element_klass.contains("Video") {
                let decoder_name = element_name.to_string();
                let decoder_weak = element.downgrade();
                let fku_epoch = Instant::now();
                let last_fku_ms = Arc::new(AtomicU64::new(0));
                let block_id = block_id_for_callback.clone();

                if let Some(sink_pad) = element.static_pad("sink") {
                    sink_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, info| {
                        if let Some(gst::PadProbeData::Buffer(ref buffer)) = info.data {
                            if buffer.flags().contains(gst::BufferFlags::DISCONT) {
                                let now_ms = fku_epoch.elapsed().as_millis() as u64;
                                let last = last_fku_ms.load(Ordering::Relaxed);
                                if now_ms.saturating_sub(last) >= 1000 {
                                    last_fku_ms.store(now_ms, Ordering::Relaxed);
                                    if let Some(decoder) = decoder_weak.upgrade() {
                                        debug!(
                                            "WHIP Input [{}]: Discontinuity on {} sink, requesting keyframe (PLI)",
                                            block_id, decoder_name
                                        );
                                        let fku =
                                            gst_video::UpstreamForceKeyUnitEvent::builder()
                                                .all_headers(true)
                                                .build();
                                        decoder.send_event(fku);
                                    }
                                }
                            }
                        }
                        gst::PadProbeReturn::Ok
                    });
                    info!(
                        "WHIP Input: Installed keyframe recovery probe on {} sink pad",
                        element_name
                    );
                }
            }
            None
        });
    }

    // pad-added: tee → fakesink (drain, prevents backpressure) + appsink (bridge to main pipeline)
    // Session pipeline always has a drain so NiceAgent is never blocked.
    // Appsink forwards RTP samples via push_sample to appsrc in the main pipeline.
    // Decoding (if needed) happens in the main pipeline after appsrc.
    {
        let session_pipeline_weak = session_pipeline.downgrade();
        let main_pipeline_weak = config.pipeline_weak.clone();
        let liveadder_weak = config.liveadder_weak.clone();
        let videoconvert_weak = config.videoconvert_weak.clone();
        let prefix = element_name.clone();
        let stream_counter = Arc::new(AtomicUsize::new(0));
        let video_connected = Arc::new(AtomicBool::new(false));
        let decode = config.decode;

        whipserversrc.connect_pad_added(move |_src, pad| {
            let pad_name = pad.name();
            let stream_num = stream_counter.fetch_add(1, Ordering::SeqCst);

            let session_pipeline: Option<gst::Pipeline> = session_pipeline_weak.upgrade();
            let Some(session_pipeline) = session_pipeline else {
                error!("WHIP Input: Session pipeline destroyed");
                return;
            };

            let main_pipeline: Option<gst::Pipeline> = main_pipeline_weak.upgrade();
            let Some(main_pipeline) = main_pipeline else {
                error!("WHIP Input: Main pipeline destroyed");
                return;
            };

            // Session pipeline: pad → tee → fakesink (drain) + appsink (bridge)
            let tee = gst::ElementFactory::make("tee")
                .property("allow-not-linked", true)
                .build().unwrap();
            let fakesink = gst::ElementFactory::make("fakesink")
                .property("sync", false)
                .property("async", false)
                .build().unwrap();
            let appsink = gst_app::AppSink::builder()
                .name(format!("{}:{}_appsink_{}", prefix, pad_name, stream_num))
                .sync(false)
                .build();

            session_pipeline.add_many([&tee, &fakesink, appsink.upcast_ref()]).unwrap();
            pad.link(&tee.static_pad("sink").unwrap()).unwrap();
            tee.request_pad_simple("src_%u").unwrap()
                .link(&fakesink.static_pad("sink").unwrap()).unwrap();
            tee.request_pad_simple("src_%u").unwrap()
                .link(&appsink.static_pad("sink").unwrap()).unwrap();
            tee.sync_state_with_parent().unwrap();
            fakesink.sync_state_with_parent().unwrap();
            appsink.sync_state_with_parent().unwrap();

            // Main pipeline: appsrc → [decodebin →] downstream (liveadder or videoconvert)
            let appsrc = gst_app::AppSrc::builder()
                .name(format!("{}:{}_appsrc_{}", prefix, pad_name, stream_num))
                .format(gst::Format::Time)
                .is_live(true)
                .handle_segment_change(true)
                .leaky_type(gst_app::AppLeakyType::Downstream)
                .automatic_eos(false)
                .build();

            let appsrc_elem = appsrc.upcast_ref::<gst::Element>();
            main_pipeline.add(appsrc_elem).unwrap();
            appsrc_elem.sync_state_with_parent().unwrap();

            if decode {
                // decode=true: appsrc → decodebin → audioconvert/audioresample → liveadder (audio)
                //              appsrc → decodebin → videoconvert (video)
                let decodebin = gst::ElementFactory::make("decodebin")
                    .name(format!("{}:{}_decodebin_{}", prefix, pad_name, stream_num))
                    .build().unwrap();

                main_pipeline.add(&decodebin).unwrap();
                appsrc.upcast_ref::<gst::Element>().link(&decodebin).unwrap();
                decodebin.sync_state_with_parent().unwrap();

                if pad_name.starts_with("audio_") {
                    let liveadder = match liveadder_weak {
                        Some(ref w) => w.upgrade(),
                        None => None,
                    };
                    if let Some(liveadder) = liveadder {
                        let main_pipeline_clone = main_pipeline.clone();
                        let prefix_clone = prefix.clone();
                        decodebin.connect_pad_added(move |_dec, src_pad| {
                            if src_pad.direction() != gst::PadDirection::Src { return; }

                            let conv_name = format!("{}:audio_conv_{}", prefix_clone, stream_num);
                            let resamp_name = format!("{}:audio_resamp_{}", prefix_clone, stream_num);
                            let conv = gst::ElementFactory::make("audioconvert").name(&conv_name).build().unwrap();
                            let resamp = gst::ElementFactory::make("audioresample").name(&resamp_name).build().unwrap();

                            main_pipeline_clone.add_many([&conv, &resamp]).unwrap();
                            src_pad.link(&conv.static_pad("sink").unwrap()).unwrap();
                            conv.link(&resamp).unwrap();

                            let la_sink = liveadder.request_pad_simple("sink_%u").unwrap();
                            resamp.static_pad("src").unwrap().link(&la_sink).unwrap();

                            conv.sync_state_with_parent().unwrap();
                            resamp.sync_state_with_parent().unwrap();

                            info!("WHIP Input: Audio stream {} decoded and linked to liveadder", stream_num);
                        });
                    }
                } else if pad_name.starts_with("video_")
                    && !video_connected.swap(true, Ordering::SeqCst)
                {
                    let vc = match videoconvert_weak {
                        Some(ref w) => w.upgrade(),
                        None => None,
                    };
                    if let Some(videoconvert) = vc {
                        decodebin.connect_pad_added(move |_dec, src_pad| {
                            if src_pad.direction() != gst::PadDirection::Src {
                                return;
                            }
                            let vc_sink = videoconvert.static_pad("sink").unwrap();
                            if !vc_sink.is_linked() {
                                src_pad.link(&vc_sink).unwrap();
                                info!("WHIP Input: Video stream {} decoded and linked to videoconvert", stream_num);
                            }
                        });
                    }
                }
            } else {
                // decode=false: appsrc → directly to liveadder/videoconvert (RTP passthrough)
                if pad_name.starts_with("audio_") {
                    let liveadder = match liveadder_weak {
                        Some(ref w) => w.upgrade(),
                        None => None,
                    };
                    if let Some(liveadder) = liveadder {
                        let la_sink = liveadder.request_pad_simple("sink_%u").unwrap();
                        appsrc
                            .upcast_ref::<gst::Element>()
                            .static_pad("src")
                            .unwrap()
                            .link(&la_sink)
                            .unwrap();
                        info!("WHIP Input: Audio stream {} RTP passthrough to liveadder", stream_num);
                    }
                } else if pad_name.starts_with("video_")
                    && !video_connected.swap(true, Ordering::SeqCst)
                {
                    let vc = match videoconvert_weak {
                        Some(ref w) => w.upgrade(),
                        None => None,
                    };
                    if let Some(videoconvert) = vc {
                        let vc_sink = videoconvert.static_pad("sink").unwrap();
                        appsrc
                            .upcast_ref::<gst::Element>()
                            .static_pad("src")
                            .unwrap()
                            .link(&vc_sink)
                            .unwrap();
                        info!("WHIP Input: Video stream {} RTP passthrough to videoconvert", stream_num);
                    }
                }
            }

            // Bridge: appsink → appsrc via push_sample.
            // Ignore push errors (e.g. decodebin not ready yet for first H264 buffers).
            // The session pipeline's fakesink drain ensures no backpressure regardless.
            appsink.set_callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |sink| {
                        let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        let _ = appsrc.push_sample(&sample);
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );

            info!(
                "WHIP Input: Pad {} (stream {}) → tee(fakesink+appsink) → appsrc → decodebin → downstream",
                pad_name, stream_num
            );
        });
    }

    // Add whipserversrc to the SESSION pipeline (not main pipeline)
    session_pipeline
        .add(&whipserversrc)
        .map_err(|e| format!("Failed to add whipserversrc to session pipeline: {}", e))?;

    // Set session pipeline to PLAYING and wait
    session_pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| format!("Failed to set session pipeline to Playing: {:?}", e))?;

    let (result, current, _pending) = session_pipeline.state(gst::ClockTime::from_seconds(5));
    if result == Err(gst::StateChangeError) {
        return Err(format!(
            "Session pipeline state change to Playing failed (current: {:?})",
            current
        ));
    }
    info!(
        "WHIP Input: Session pipeline '{}' on port {}, state: {:?}",
        session_pipeline.name(),
        port,
        current
    );

    Ok((whipserversrc, session_pipeline, port))
}

// ============================================================================
// WHIP Output (whipclientsink / whipsink)
// ============================================================================

/// Build using the new whipclientsink (signaller-based) implementation
fn build_whipclientsink(
    instance_id: &str,
    properties: &HashMap<String, PropertyValue>,
    ctx: &BlockBuildContext,
) -> Result<BlockBuildResult, BlockBuildError> {
    info!("Building WHIP Output using whipclientsink (new implementation)");

    // Get required WHIP endpoint
    let whip_endpoint = properties
        .get("whip_endpoint")
        .and_then(|v| {
            if let PropertyValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            BlockBuildError::InvalidProperty("whip_endpoint property required".to_string())
        })?;

    // Get optional auth token
    let auth_token = properties.get("auth_token").and_then(|v| {
        if let PropertyValue::String(s) = v {
            if s.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        } else {
            None
        }
    });

    // Get ICE servers from application config
    let stun_server = ctx.stun_server();
    let turn_server = ctx.turn_server();

    // Create namespaced element IDs
    let whipclientsink_id = format!("{}:whipclientsink", instance_id);
    let audioconvert_id = format!("{}:audioconvert", instance_id);
    let audioresample_id = format!("{}:audioresample", instance_id);

    // Create audio processing elements
    let audioconvert = gst::ElementFactory::make("audioconvert")
        .name(&audioconvert_id)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("audioconvert: {}", e)))?;

    let audioresample = gst::ElementFactory::make("audioresample")
        .name(&audioresample_id)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("audioresample: {}", e)))?;

    // Create whipclientsink element
    let whipclientsink = gst::ElementFactory::make("whipclientsink")
        .name(&whipclientsink_id)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("whipclientsink: {}", e)))?;

    // Set ICE server properties (explicitly clear defaults when not configured,
    // since webrtcsink defaults to stun://stun.l.google.com:19302)
    match stun_server {
        Some(ref stun) => whipclientsink.set_property("stun-server", stun),
        None => whipclientsink.set_property("stun-server", None::<&str>),
    }
    if let Some(ref turn) = turn_server {
        let turn_servers = gst::Array::new([turn]);
        whipclientsink.set_property("turn-servers", turn_servers);
    }

    // Disable video codecs by setting video-caps to empty
    whipclientsink.set_property("video-caps", gst::Caps::new_empty());

    // Access the signaller child and set its properties
    let signaller = whipclientsink.property::<gst::glib::Object>("signaller");
    signaller.set_property("whip-endpoint", &whip_endpoint);

    if let Some(token) = &auth_token {
        signaller.set_property("auth-token", token);
    }

    // Read Opus encoder settings
    let opus_complexity = properties
        .get("opus_complexity")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as i32),
            _ => None,
        })
        .unwrap_or(DEFAULT_OPUS_COMPLEXITY);

    let opus_bitrate = properties
        .get("opus_bitrate")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as i32),
            _ => None,
        })
        .unwrap_or(DEFAULT_OPUS_BITRATE);

    // Configure internal elements via deep-element-added:
    // - ICE transport policy on webrtcbin
    // - Opus encoder settings on opusenc
    if let Ok(bin) = whipclientsink.clone().downcast::<gst::Bin>() {
        let ice_transport_policy = ctx.ice_transport_policy().to_string();
        bin.connect("deep-element-added", false, move |values| {
            let element = values[2].get::<gst::Element>().unwrap();
            let element_name = element.name();

            if element_name.starts_with("webrtcbin") && element.has_property("ice-transport-policy")
            {
                element.set_property_from_str("ice-transport-policy", &ice_transport_policy);
                info!(
                    "WHIP (whipclientsink): Set ice-transport-policy={} on webrtcbin {}",
                    ice_transport_policy, element_name
                );
            }

            if element_name.starts_with("opusenc") {
                element.set_property("complexity", opus_complexity);
                element.set_property("bitrate", opus_bitrate);
                info!(
                    "WHIP (whipclientsink): Set opusenc {}: complexity={}, bitrate={}",
                    element_name, opus_complexity, opus_bitrate
                );
            }
            None
        });
    }

    debug!(
        "WHIP Output (whipclientsink) configured: endpoint={}, stun={:?}, turn={:?}",
        whip_endpoint, stun_server, turn_server
    );

    // Define internal links
    let internal_links = vec![
        (
            ElementPadRef::pad(&audioconvert_id, "src"),
            ElementPadRef::pad(&audioresample_id, "sink"),
        ),
        (
            ElementPadRef::pad(&audioresample_id, "src"),
            ElementPadRef::pad(&whipclientsink_id, "audio_0"),
        ),
    ];

    Ok(BlockBuildResult {
        elements: vec![
            (audioconvert_id, audioconvert),
            (audioresample_id, audioresample),
            (whipclientsink_id, whipclientsink),
        ],
        internal_links,
        bus_message_handler: None,
        pad_properties: HashMap::new(),
    })
}

/// Build using the stable whipsink implementation
fn build_whipsink(
    instance_id: &str,
    properties: &HashMap<String, PropertyValue>,
    ctx: &BlockBuildContext,
) -> Result<BlockBuildResult, BlockBuildError> {
    info!("Building WHIP Output using whipsink (stable)");

    let whip_endpoint = properties
        .get("whip_endpoint")
        .and_then(|v| {
            if let PropertyValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            BlockBuildError::InvalidProperty("whip_endpoint property required".to_string())
        })?;

    let auth_token = properties.get("auth_token").and_then(|v| {
        if let PropertyValue::String(s) = v {
            if s.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        } else {
            None
        }
    });

    let stun_server = ctx.stun_server();
    let turn_server = ctx.turn_server();

    let whipsink_id = format!("{}:whipsink", instance_id);
    let audioconvert_id = format!("{}:audioconvert", instance_id);
    let audioresample_id = format!("{}:audioresample", instance_id);
    let opusenc_id = format!("{}:opusenc", instance_id);
    let rtpopuspay_id = format!("{}:rtpopuspay", instance_id);

    let audioconvert = gst::ElementFactory::make("audioconvert")
        .name(&audioconvert_id)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("audioconvert: {}", e)))?;

    let audioresample = gst::ElementFactory::make("audioresample")
        .name(&audioresample_id)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("audioresample: {}", e)))?;

    let opus_complexity = properties
        .get("opus_complexity")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as i32),
            _ => None,
        })
        .unwrap_or(DEFAULT_OPUS_COMPLEXITY);

    let opus_bitrate = properties
        .get("opus_bitrate")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as i32),
            _ => None,
        })
        .unwrap_or(DEFAULT_OPUS_BITRATE);

    let opusenc = gst::ElementFactory::make("opusenc")
        .name(&opusenc_id)
        .property("complexity", opus_complexity)
        .property("bitrate", opus_bitrate)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("opusenc: {}", e)))?;

    info!(
        "WHIP Output opusenc: complexity={}, bitrate={}",
        opus_complexity, opus_bitrate
    );

    let rtpopuspay = gst::ElementFactory::make("rtpopuspay")
        .name(&rtpopuspay_id)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("rtpopuspay: {}", e)))?;

    let whipsink = gst::ElementFactory::make("whipsink")
        .name(&whipsink_id)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("whipsink: {}", e)))?;

    whipsink.set_property("whip-endpoint", &whip_endpoint);
    // Explicitly clear defaults when not configured,
    // since whipsink defaults to stun://stun.l.google.com:19302
    match stun_server {
        Some(ref stun) => whipsink.set_property("stun-server", stun),
        None => whipsink.set_property("stun-server", None::<&str>),
    }
    if let Some(ref turn) = turn_server {
        whipsink.set_property("turn-server", turn);
    }
    if let Some(token) = &auth_token {
        whipsink.set_property("auth-token", token);
    }

    debug!(
        "WHIP Output (whipsink legacy) configured: endpoint={}, stun={:?}, turn={:?}",
        whip_endpoint, stun_server, turn_server
    );

    setup_incoming_rtp_handler(&whipsink, instance_id, ctx.ice_transport_policy());

    let internal_links = vec![
        (
            ElementPadRef::pad(&audioconvert_id, "src"),
            ElementPadRef::pad(&audioresample_id, "sink"),
        ),
        (
            ElementPadRef::pad(&audioresample_id, "src"),
            ElementPadRef::pad(&opusenc_id, "sink"),
        ),
        (
            ElementPadRef::pad(&opusenc_id, "src"),
            ElementPadRef::pad(&rtpopuspay_id, "sink"),
        ),
        (
            ElementPadRef::pad(&rtpopuspay_id, "src"),
            ElementPadRef::pad(&whipsink_id, "sink_0"),
        ),
    ];

    Ok(BlockBuildResult {
        elements: vec![
            (audioconvert_id, audioconvert),
            (audioresample_id, audioresample),
            (opusenc_id, opusenc),
            (rtpopuspay_id, rtpopuspay),
            (whipsink_id, whipsink),
        ],
        internal_links,
        bus_message_handler: None,
        pad_properties: HashMap::new(),
    })
}

// ============================================================================
// Block Definitions
// ============================================================================

/// Get metadata for WHIP blocks (for UI/API).
pub fn get_blocks() -> Vec<BlockDefinition> {
    vec![whip_output_definition(), whip_input_definition()]
}

/// WHIP Output block definition.
fn whip_output_definition() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.whip_output".to_string(),
        name: "WHIP Output".to_string(),
        description: "Sends audio via WebRTC WHIP protocol. Default uses stable whipsink element.".to_string(),
        category: "Outputs".to_string(),
        exposed_properties: vec![
            ExposedProperty {
                name: "implementation".to_string(),
                label: "Implementation".to_string(),
                description: "Choose GStreamer element: whipsink (stable) or whipclientsink (new, may have issues with some servers)".to_string(),
                property_type: PropertyType::Enum {
                    values: vec![
                        EnumValue {
                            value: "whipsink".to_string(),
                            label: Some("whipsink (stable)".to_string()),
                        },
                        EnumValue {
                            value: "whipclientsink".to_string(),
                            label: Some("whipclientsink (new)".to_string()),
                        },
                    ],
                },
                default_value: Some(PropertyValue::String("whipsink".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "implementation".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "whip_endpoint".to_string(),
                label: "WHIP Endpoint".to_string(),
                description: "WHIP server endpoint URL (e.g., https://example.com/whip/room1)"
                    .to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String("".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "whip_endpoint".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "auth_token".to_string(),
                label: "Auth Token".to_string(),
                description: "Bearer token for authentication (optional)".to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String("".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "auth_token".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "opus_complexity".to_string(),
                label: "Opus Complexity".to_string(),
                description: "Opus encoder complexity (0-10). Lower values use less CPU. 5 is recommended for real-time.".to_string(),
                property_type: PropertyType::Int,
                default_value: Some(PropertyValue::Int(DEFAULT_OPUS_COMPLEXITY as i64)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "opus_complexity".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "opus_bitrate".to_string(),
                label: "Opus Bitrate".to_string(),
                description: "Opus encoder bitrate in bps (4000-650000)".to_string(),
                property_type: PropertyType::Int,
                default_value: Some(PropertyValue::Int(DEFAULT_OPUS_BITRATE as i64)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "opus_bitrate".to_string(),
                    transform: None,
                },
            },
        ],
        external_pads: ExternalPads {
            inputs: vec![ExternalPad {
                label: None,
                name: "audio_in".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "audioconvert".to_string(),
                internal_pad_name: "sink".to_string(),
            }],
            outputs: vec![],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("🌐".to_string()),
            width: Some(2.5),
            height: Some(1.5),
            ..Default::default()
        }),
    }
}

/// WHIP Input block definition (server mode - hosts WHIP endpoint).
fn whip_input_definition() -> BlockDefinition {
    BlockDefinition {
        id: "builtin.whip_input".to_string(),
        name: "WHIP Input".to_string(),
        description: "Hosts a WHIP server endpoint. Clients (browsers, OBS, encoders) connect via WHIP to send media. Access ingest page at /player/whip-ingest".to_string(),
        category: "Inputs".to_string(),
        exposed_properties: vec![
            ExposedProperty {
                name: "mode".to_string(),
                label: "Stream Mode".to_string(),
                description: "What media to accept: audio + video, audio only, or video only".to_string(),
                property_type: PropertyType::Enum {
                    values: vec![
                        EnumValue {
                            value: "audio_video".to_string(),
                            label: Some("Audio + Video".to_string()),
                        },
                        EnumValue {
                            value: "audio".to_string(),
                            label: Some("Audio Only".to_string()),
                        },
                        EnumValue {
                            value: "video".to_string(),
                            label: Some("Video Only".to_string()),
                        },
                    ],
                },
                default_value: Some(PropertyValue::String("audio_video".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "mode".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "endpoint_id".to_string(),
                label: "Endpoint ID".to_string(),
                description: "Unique identifier for this WHIP endpoint. Leave empty to auto-generate. Ingest at /whip/{endpoint_id}".to_string(),
                property_type: PropertyType::String,
                default_value: Some(PropertyValue::String("".to_string())),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "endpoint_id".to_string(),
                    transform: None,
                },
            },
            ExposedProperty {
                name: "decode".to_string(),
                label: "Decode".to_string(),
                description: "Decode incoming RTP to raw audio/video. When disabled, outputs RTP (application/x-rtp).".to_string(),
                property_type: PropertyType::Bool,
                default_value: Some(PropertyValue::Bool(true)),
                mapping: PropertyMapping {
                    element_id: "_block".to_string(),
                    property_name: "decode".to_string(),
                    transform: None,
                },
            },
        ],
        // Note: external_pads here are the static defaults for audio_video mode.
        // Actual pads are determined dynamically by WHIPInputBuilder::get_external_pads().
        external_pads: ExternalPads {
            inputs: vec![],
            outputs: vec![
                ExternalPad {
                    label: Some("V0".to_string()),
                    name: "video_out".to_string(),
                    media_type: MediaType::Video,
                    internal_element_id: "video_out_tee".to_string(),
                    internal_pad_name: "src_%u".to_string(),
                },
                ExternalPad {
                    label: Some("A0".to_string()),
                    name: "audio_out".to_string(),
                    media_type: MediaType::Audio,
                    internal_element_id: "audio_out_tee".to_string(),
                    internal_pad_name: "src_%u".to_string(),
                },
            ],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("📹".to_string()),
            width: Some(2.5),
            height: Some(1.5),
            ..Default::default()
        }),
    }
}

// ============================================================================
// WHIP Output Helper (incoming RTP handler for legacy whipsink)
// ============================================================================

/// Setup handler for unexpected incoming RTP on WHIP sink elements.
fn setup_incoming_rtp_handler(
    whip_element: &gst::Element,
    instance_id: &str,
    ice_transport_policy: &str,
) {
    let bin = match whip_element.clone().downcast::<gst::Bin>() {
        Ok(b) => b,
        Err(_) => {
            warn!("WHIP: Element is not a bin, cannot setup incoming RTP handler");
            return;
        }
    };

    let ice_transport_policy = ice_transport_policy.to_string();

    bin.connect("deep-element-added", false, move |values| {
        let parent_bin = values[0].get::<gst::Bin>().unwrap();
        let element = values[2].get::<gst::Element>().unwrap();
        let element_name = element.name();
        let element_type = element.type_().name();

        if element_name.starts_with("webrtcbin") && element.has_property("ice-transport-policy") {
            element.set_property_from_str("ice-transport-policy", &ice_transport_policy);
            info!(
                "WHIP: Set ice-transport-policy={} on webrtcbin {}",
                ice_transport_policy, element_name
            );
        }

        if element_type == "TransportReceiveBin" {
            info!(
                "WHIP: Found {} (parent bin: {}), checking for unlinked src pads",
                element_name,
                parent_bin.name()
            );

            let element_name_clone = element_name.to_string();

            for pad in element.src_pads() {
                let pad_name = pad.name();
                if !pad.is_linked() && pad_name.contains("rtp_src") {
                    let direct_parent = match element.parent() {
                        Some(p) => match p.downcast::<gst::Bin>() {
                            Ok(bin) => bin,
                            Err(_) => continue,
                        },
                        None => continue,
                    };

                    let fakesink_name = format!("whip_fakesink_{}", pad_name);
                    if let Ok(fakesink) = gst::ElementFactory::make("fakesink")
                        .name(&fakesink_name)
                        .property("sync", false)
                        .property("async", false)
                        .build()
                    {
                        if direct_parent.add(&fakesink).is_err() {
                            continue;
                        }
                        let _ = fakesink.sync_state_with_parent();
                        if let Some(sink_pad) = fakesink.static_pad("sink") {
                            if pad.link(&sink_pad).is_ok() {
                                info!("WHIP: Linked {} to fakesink", pad_name);
                            }
                        }
                    }
                }
            }

            element.connect_pad_added(move |elem, pad| {
                let pad_name = pad.name();
                if pad.direction() != gst::PadDirection::Src {
                    return;
                }

                info!("WHIP: {} pad-added: {}", element_name_clone, pad_name);

                if pad.is_linked() || !pad_name.contains("rtp_src") {
                    return;
                }

                let direct_parent = match elem.parent() {
                    Some(p) => match p.downcast::<gst::Bin>() {
                        Ok(bin) => bin,
                        Err(_) => return,
                    },
                    None => return,
                };

                let fakesink_name = format!("whip_fakesink_{}", pad_name);
                if let Ok(fakesink) = gst::ElementFactory::make("fakesink")
                    .name(&fakesink_name)
                    .property("sync", false)
                    .property("async", false)
                    .build()
                {
                    if direct_parent.add(&fakesink).is_err() {
                        return;
                    }
                    let _ = fakesink.sync_state_with_parent();
                    if let Some(sink_pad) = fakesink.static_pad("sink") {
                        if pad.link(&sink_pad).is_ok() {
                            info!("WHIP: Linked new pad {} to fakesink", pad_name);
                        }
                    }
                }
            });
        }

        None
    });

    info!("WHIP: Incoming RTP handler installed for {}", instance_id);
}
