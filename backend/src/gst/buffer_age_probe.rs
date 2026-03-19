//! Buffer age pad probes for latency monitoring.
//!
//! Provides both manual (on-demand) and automatic (always-on) probe modes.
//!
//! **Manual probes** are activated via the API, broadcast `BufferAgeProbe` events
//! for every Nth buffer, and auto-remove after a timeout.
//!
//! **Automatic probes** are attached at pipeline start to key measurement points,
//! only broadcast `BufferAgeWarning` when the age exceeds a threshold, and are
//! cleaned up when the pipeline stops.

use crate::events::EventBroadcaster;
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use strom_types::{
    BlockDefinition, BlockInstance, FlowId, StromEvent, BUFFER_AGE_WARNING_THRESHOLD_MS,
};
use tracing::{debug, info};
use uuid::Uuid;

/// Sample interval for automatic probes (measure every Nth buffer).
const AUTO_SAMPLE_INTERVAL: u32 = 30;

/// Buffer age threshold for automatic warnings (milliseconds).
const AUTO_WARNING_THRESHOLD_MS: u64 = BUFFER_AGE_WARNING_THRESHOLD_MS;

/// Measure the age of a buffer on a pad (pipeline_running_time - buffer_running_time).
///
/// Returns `Some(age_ms)` if the measurement succeeded, `None` otherwise.
/// This runs on the GStreamer streaming thread inside a pad probe callback.
fn measure_buffer_age(
    pad: &gst::Pad,
    info: &gst::PadProbeInfo,
    pipeline: &gst::Pipeline,
) -> Option<u64> {
    let buffer = info.buffer()?;
    let pts = buffer.pts()?;

    let segment = pad.sticky_event::<gst::event::Segment>(0)?;
    let time_segment = segment.segment().downcast_ref::<gst::format::Time>()?;
    let buffer_rt = time_segment.to_running_time(pts)?;

    let clock = pipeline.clock()?;
    let base_time = pipeline.base_time()?;
    let clock_time = clock.time();
    if clock_time < base_time {
        return None;
    }
    let pipeline_rt = clock_time - base_time;

    if pipeline_rt >= buffer_rt {
        let age_ns = (pipeline_rt - buffer_rt).nseconds();
        Some(age_ns / 1_000_000)
    } else {
        None
    }
}

/// Resolve a pad on an element by name, with fallback to first sink pad.
fn resolve_pad(element: &gst::Element, pad_name: &str) -> Result<gst::Pad, String> {
    element
        .static_pad(pad_name)
        .or_else(|| {
            element
                .pads()
                .into_iter()
                .find(|p| p.name().as_str() == pad_name)
        })
        .or_else(|| element.sink_pads().into_iter().next())
        .ok_or_else(|| {
            let available: Vec<String> = element
                .pads()
                .into_iter()
                .map(|p| p.name().to_string())
                .collect();
            format!("No pad '{}' found (available: {:?})", pad_name, available)
        })
}

/// State for a single active probe.
struct ProbeState {
    probe_id: String,
    flow_id: FlowId,
    element_id: String,
    pad_name: String,
    /// GStreamer pad probe ID (for removal)
    gst_probe_id: Option<gst::PadProbeId>,
    /// The pad this probe is attached to
    pad: gst::Pad,
    /// Auto-remove timeout handle
    timeout_handle: Option<tokio::task::JoinHandle<()>>,
    /// Sample counter (total buffers seen by this probe)
    sample_count: Arc<std::sync::atomic::AtomicU64>,
    /// Whether this is an automatic monitoring probe (excluded from manual API)
    automatic: bool,
}

/// Manages all active buffer age probes for a pipeline.
pub struct ProbeManager {
    probes: Arc<Mutex<HashMap<String, ProbeState>>>,
    events: EventBroadcaster,
    flow_id: FlowId,
}

impl ProbeManager {
    /// Create a new probe manager for a flow.
    pub fn new(flow_id: FlowId, events: EventBroadcaster) -> Self {
        Self {
            probes: Arc::new(Mutex::new(HashMap::new())),
            events,
            flow_id,
        }
    }

    /// Activate a new probe on the specified pad.
    ///
    /// - `pipeline`: the GStreamer pipeline (to obtain clock/base_time)
    /// - `element`: the GStreamer element whose pad we probe
    /// - `element_id`: the strom element_id (for events)
    /// - `pad_name`: name of the pad to probe
    /// - `sample_interval`: measure every Nth buffer (default 1)
    /// - `timeout_secs`: auto-remove after this many seconds (default 60)
    pub fn activate(
        &self,
        pipeline: &gst::Pipeline,
        element: &gst::Element,
        element_id: String,
        pad_name: String,
        sample_interval: u32,
        timeout_secs: u32,
    ) -> Result<String, String> {
        let pad = resolve_pad(element, &pad_name)?;

        // Use the actual pad name (may differ from requested name)
        let pad_name = pad.name().to_string();

        let probe_id = Uuid::new_v4().to_string();
        let sample_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let sample_interval = sample_interval.max(1);

        // Capture values for the probe callback (runs on GStreamer streaming thread)
        let probe_id_cb = probe_id.clone();
        let element_id_cb = element_id.clone();
        let pad_name_cb = pad_name.clone();
        let flow_id = self.flow_id;
        let events = self.events.clone();
        let sample_count_cb = sample_count.clone();
        let pipeline_weak = pipeline.downgrade();

        let gst_probe_id = pad.add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
            let count = sample_count_cb.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;

            if !count.is_multiple_of(sample_interval as u64) {
                return gst::PadProbeReturn::Ok;
            }

            let Some(pipeline) = pipeline_weak.upgrade() else {
                return gst::PadProbeReturn::Remove;
            };

            if let Some(age_ms) = measure_buffer_age(pad, info, &pipeline) {
                events.broadcast(StromEvent::BufferAgeProbe {
                    flow_id,
                    probe_id: probe_id_cb.clone(),
                    element_id: element_id_cb.clone(),
                    pad_name: pad_name_cb.clone(),
                    age_ms,
                    sample_number: count,
                });
            }

            gst::PadProbeReturn::Ok
        });

        let gst_probe_id = match gst_probe_id {
            Some(id) => id,
            None => return Err("Failed to attach pad probe".to_string()),
        };

        // Set up auto-removal timeout
        let probes_ref = self.probes.clone();
        let probe_id_timeout = probe_id.clone();
        let events_timeout = self.events.clone();
        let flow_id_timeout = self.flow_id;
        let timeout_handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(timeout_secs as u64)).await;
            let removed = {
                let mut probes = probes_ref.lock().unwrap();
                probes.remove(&probe_id_timeout)
            };
            if let Some(state) = removed {
                state.pad.remove_probe(state.gst_probe_id.unwrap());
                info!(probe_id = %probe_id_timeout, "Buffer age probe auto-removed after timeout");
                events_timeout.broadcast(StromEvent::BufferAgeProbeDeactivated {
                    flow_id: flow_id_timeout,
                    probe_id: probe_id_timeout,
                    reason: "timeout".to_string(),
                });
            }
        });

        // Broadcast activation event
        self.events.broadcast(StromEvent::BufferAgeProbeActivated {
            flow_id: self.flow_id,
            probe_id: probe_id.clone(),
            element_id: element_id.clone(),
            pad_name: pad_name.clone(),
        });

        // Store probe state
        let state = ProbeState {
            probe_id: probe_id.clone(),
            flow_id: self.flow_id,
            element_id,
            pad_name,
            gst_probe_id: Some(gst_probe_id),
            pad,
            timeout_handle: Some(timeout_handle),
            sample_count,
            automatic: false,
        };

        self.probes.lock().unwrap().insert(probe_id.clone(), state);

        debug!(probe_id = %probe_id, "Buffer age probe activated");
        Ok(probe_id)
    }

    /// Activate probes on all sink pads of an element.
    /// Returns the list of probe IDs created.
    pub fn activate_all_sinks(
        &self,
        pipeline: &gst::Pipeline,
        element: &gst::Element,
        element_id: String,
        sample_interval: u32,
        timeout_secs: u32,
    ) -> Result<Vec<String>, String> {
        let sink_pads = element.sink_pads();
        if sink_pads.is_empty() {
            let available: Vec<String> = element
                .pads()
                .into_iter()
                .map(|p| p.name().to_string())
                .collect();
            return Err(format!(
                "No sink pads on element (available pads: {:?})",
                available
            ));
        }

        let mut probe_ids = Vec::new();
        for pad in sink_pads {
            let pad_name = pad.name().to_string();
            match self.activate(
                pipeline,
                element,
                element_id.clone(),
                pad_name.clone(),
                sample_interval,
                timeout_secs,
            ) {
                Ok(id) => probe_ids.push(id),
                Err(e) => {
                    info!(pad = %pad_name, error = %e, "Skipping pad");
                }
            }
        }

        if probe_ids.is_empty() {
            Err("Failed to attach probe to any sink pad".to_string())
        } else {
            Ok(probe_ids)
        }
    }

    /// Attach automatic buffer age monitoring probes to key measurement points.
    ///
    /// For standalone elements: probes are attached to each sink pad.
    /// For blocks: probes are attached to the internal elements that receive external input.
    ///
    /// Automatic probes only broadcast `BufferAgeWarning` when the buffer age
    /// exceeds the threshold, keeping WebSocket traffic minimal.
    pub fn attach_automatic(
        &self,
        pipeline: &gst::Pipeline,
        elements: &HashMap<String, gst::Element>,
        blocks: &[BlockInstance],
        block_definitions: &HashMap<String, BlockDefinition>,
    ) {
        let mut count = 0;

        // Attach to standalone elements (keys without ':' are standalone)
        for (element_id, element) in elements {
            if element_id.contains(':') || element_id.starts_with("auto_tee_") {
                continue;
            }
            let sink_pads = element.sink_pads();
            if sink_pads.is_empty() {
                continue;
            }
            for pad in sink_pads {
                match self.attach_automatic_probe(
                    pipeline,
                    element,
                    element_id.clone(),
                    pad.name().to_string(),
                ) {
                    Ok(_) => count += 1,
                    Err(e) => {
                        debug!(
                            element = %element_id,
                            pad = %pad.name(),
                            error = %e,
                            "Skipping automatic probe"
                        );
                    }
                }
            }
        }

        // Attach to block input pads
        for block in blocks {
            let definition = match block_definitions.get(&block.block_definition_id) {
                Some(def) => def,
                None => {
                    debug!(
                        block = %block.id,
                        definition = %block.block_definition_id,
                        "No block definition found, skipping automatic probes"
                    );
                    continue;
                }
            };

            // Use computed_external_pads if available, otherwise fall back to definition
            let inputs = match &block.computed_external_pads {
                Some(pads) => &pads.inputs,
                None => &definition.external_pads.inputs,
            };

            for input_pad in inputs {
                // Resolve to internal element: "block_id:internal_element_id"
                let element_key = format!("{}:{}", block.id, input_pad.internal_element_id);
                let element = match elements.get(&element_key) {
                    Some(el) => el,
                    None => {
                        debug!(
                            block = %block.id,
                            element_key = %element_key,
                            "Internal element not found for automatic probe"
                        );
                        continue;
                    }
                };

                // Probe the specific internal pad that receives external input
                match self.attach_automatic_probe(
                    pipeline,
                    element,
                    block.id.clone(),
                    input_pad.internal_pad_name.clone(),
                ) {
                    Ok(_) => count += 1,
                    Err(e) => {
                        debug!(
                            block = %block.id,
                            element_key = %element_key,
                            pad = %input_pad.internal_pad_name,
                            error = %e,
                            "Skipping automatic probe on block input"
                        );
                    }
                }
            }
        }

        if count > 0 {
            info!(count, "Automatic buffer age probes attached");
        }
    }

    /// Attach a single automatic probe to a pad.
    fn attach_automatic_probe(
        &self,
        pipeline: &gst::Pipeline,
        element: &gst::Element,
        element_id: String,
        pad_name: String,
    ) -> Result<String, String> {
        let pad = resolve_pad(element, &pad_name)?;

        let pad_name = pad.name().to_string();
        let probe_id = format!("auto-{}", Uuid::new_v4());
        let sample_count = Arc::new(std::sync::atomic::AtomicU64::new(0));

        let element_id_cb = element_id.clone();
        let pad_name_cb = pad_name.clone();
        let flow_id = self.flow_id;
        let events = self.events.clone();
        let sample_count_cb = sample_count.clone();
        let pipeline_weak = pipeline.downgrade();

        let gst_probe_id = pad.add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
            let count = sample_count_cb.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;

            if !count.is_multiple_of(AUTO_SAMPLE_INTERVAL as u64) {
                return gst::PadProbeReturn::Ok;
            }

            let Some(pipeline) = pipeline_weak.upgrade() else {
                return gst::PadProbeReturn::Remove;
            };

            if let Some(age_ms) = measure_buffer_age(pad, info, &pipeline) {
                if age_ms > AUTO_WARNING_THRESHOLD_MS {
                    events.broadcast(StromEvent::BufferAgeWarning {
                        flow_id,
                        element_id: element_id_cb.clone(),
                        pad_name: pad_name_cb.clone(),
                        age_ms,
                        threshold_ms: AUTO_WARNING_THRESHOLD_MS,
                    });
                }
            }

            gst::PadProbeReturn::Ok
        });

        let gst_probe_id = match gst_probe_id {
            Some(id) => id,
            None => return Err("Failed to attach pad probe".to_string()),
        };

        let state = ProbeState {
            probe_id: probe_id.clone(),
            flow_id: self.flow_id,
            element_id,
            pad_name,
            gst_probe_id: Some(gst_probe_id),
            pad,
            timeout_handle: None,
            sample_count,
            automatic: true,
        };

        self.probes.lock().unwrap().insert(probe_id.clone(), state);
        Ok(probe_id)
    }

    /// Deactivate a specific probe.
    pub fn deactivate(&self, probe_id: &str) -> Result<(), String> {
        let state = {
            let mut probes = self.probes.lock().unwrap();
            probes
                .remove(probe_id)
                .ok_or_else(|| format!("Probe '{}' not found", probe_id))?
        };

        if let Some(gst_id) = state.gst_probe_id {
            state.pad.remove_probe(gst_id);
        }
        if let Some(handle) = state.timeout_handle {
            handle.abort();
        }

        self.events
            .broadcast(StromEvent::BufferAgeProbeDeactivated {
                flow_id: self.flow_id,
                probe_id: probe_id.to_string(),
                reason: "manual".to_string(),
            });

        info!(probe_id = %probe_id, "Buffer age probe deactivated");
        Ok(())
    }

    /// List all active manual probes (excludes automatic probes).
    pub fn list(&self) -> Vec<strom_types::api::ProbeInfo> {
        let probes = self.probes.lock().unwrap();
        probes
            .values()
            .filter(|s| !s.automatic)
            .map(|s| strom_types::api::ProbeInfo {
                probe_id: s.probe_id.clone(),
                element_id: s.element_id.clone(),
                pad_name: s.pad_name.clone(),
                sample_count: s.sample_count.load(std::sync::atomic::Ordering::Relaxed),
            })
            .collect()
    }

    /// Deactivate all probes (called when flow stops).
    pub fn deactivate_all(&self) {
        let probes: Vec<ProbeState> = {
            let mut map = self.probes.lock().unwrap();
            map.drain().map(|(_, v)| v).collect()
        };

        let manual_count = probes.iter().filter(|s| !s.automatic).count();
        let auto_count = probes.iter().filter(|s| s.automatic).count();

        for state in probes {
            if let Some(gst_id) = state.gst_probe_id {
                state.pad.remove_probe(gst_id);
            }
            if let Some(handle) = state.timeout_handle {
                handle.abort();
            }

            // Only broadcast deactivation events for manual probes
            if !state.automatic {
                self.events
                    .broadcast(StromEvent::BufferAgeProbeDeactivated {
                        flow_id: state.flow_id,
                        probe_id: state.probe_id.clone(),
                        reason: "flow_stopped".to_string(),
                    });
            }
        }

        if manual_count > 0 || auto_count > 0 {
            debug!(
                manual = manual_count,
                auto = auto_count,
                "Deactivated all buffer age probes"
            );
        }
    }
}

impl Drop for ProbeManager {
    fn drop(&mut self) {
        self.deactivate_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBroadcaster;

    fn init_gst() {
        gst::init().unwrap();
    }

    fn create_test_pipeline() -> (gst::Pipeline, gst::Element) {
        let pipeline = gst::Pipeline::builder().name("test-pipeline").build();
        let src = gst::ElementFactory::make("videotestsrc")
            .name("src")
            .property("is-live", true)
            .build()
            .unwrap();
        let sink = gst::ElementFactory::make("fakesink")
            .name("sink")
            .build()
            .unwrap();
        pipeline.add_many([&src, &sink]).unwrap();
        src.link(&sink).unwrap();
        (pipeline, sink)
    }

    #[test]
    fn test_resolve_pad_static() {
        init_gst();
        let sink = gst::ElementFactory::make("fakesink")
            .name("test-sink")
            .build()
            .unwrap();
        let pad = resolve_pad(&sink, "sink");
        assert!(pad.is_ok());
        assert_eq!(pad.unwrap().name().as_str(), "sink");
    }

    #[test]
    fn test_resolve_pad_fallback() {
        init_gst();
        let sink = gst::ElementFactory::make("fakesink")
            .name("test-sink")
            .build()
            .unwrap();
        // Request a non-existent pad name; should fall back to "sink"
        let pad = resolve_pad(&sink, "nonexistent");
        assert!(pad.is_ok());
        assert_eq!(pad.unwrap().name().as_str(), "sink");
    }

    #[test]
    fn test_resolve_pad_no_sink() {
        init_gst();
        // videotestsrc has only a src pad, no sink pad
        let src = gst::ElementFactory::make("videotestsrc")
            .name("test-src")
            .build()
            .unwrap();
        let pad = resolve_pad(&src, "sink");
        assert!(pad.is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_probe_manager_lifecycle() {
        init_gst();
        let (pipeline, sink) = create_test_pipeline();
        pipeline.set_state(gst::State::Playing).unwrap();

        // Wait for pipeline to start producing buffers
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let flow_id = FlowId::from(uuid::Uuid::new_v4());
        let events = EventBroadcaster::default();
        let pm = ProbeManager::new(flow_id, events);

        // Activate a probe
        let result = pm.activate(
            &pipeline,
            &sink,
            "sink".to_string(),
            "sink".to_string(),
            1,
            60,
        );
        assert!(result.is_ok());
        let probe_id = result.unwrap();

        // List should show one probe
        let list = pm.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].probe_id, probe_id);

        // Deactivate
        assert!(pm.deactivate(&probe_id).is_ok());

        // List should be empty
        assert!(pm.list().is_empty());

        pipeline.set_state(gst::State::Null).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_probe_manager_deactivate_all() {
        init_gst();
        let (pipeline, sink) = create_test_pipeline();
        pipeline.set_state(gst::State::Playing).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let flow_id = FlowId::from(uuid::Uuid::new_v4());
        let events = EventBroadcaster::default();
        let pm = ProbeManager::new(flow_id, events);

        // Activate two probes
        pm.activate(
            &pipeline,
            &sink,
            "s1".to_string(),
            "sink".to_string(),
            1,
            60,
        )
        .unwrap();
        pm.activate(
            &pipeline,
            &sink,
            "s2".to_string(),
            "sink".to_string(),
            1,
            60,
        )
        .unwrap();
        assert_eq!(pm.list().len(), 2);

        pm.deactivate_all();
        assert!(pm.list().is_empty());

        pipeline.set_state(gst::State::Null).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_list_excludes_automatic() {
        init_gst();
        let (pipeline, sink) = create_test_pipeline();
        pipeline.set_state(gst::State::Playing).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let flow_id = FlowId::from(uuid::Uuid::new_v4());
        let events = EventBroadcaster::default();
        let pm = ProbeManager::new(flow_id, events);

        // Activate a manual probe
        pm.activate(
            &pipeline,
            &sink,
            "manual".to_string(),
            "sink".to_string(),
            1,
            60,
        )
        .unwrap();

        // Attach automatic probes (on the sink element)
        let mut elements = HashMap::new();
        elements.insert("sink".to_string(), sink.clone());
        pm.attach_automatic(&pipeline, &elements, &[], &HashMap::new());

        // list() should only return the manual probe
        let list = pm.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].element_id, "manual");

        // But deactivate_all clears everything
        pm.deactivate_all();
        assert!(pm.list().is_empty());

        pipeline.set_state(gst::State::Null).unwrap();
    }

    #[test]
    fn test_deactivate_nonexistent() {
        init_gst();
        let flow_id = FlowId::from(uuid::Uuid::new_v4());
        let events = EventBroadcaster::default();
        let pm = ProbeManager::new(flow_id, events);

        assert!(pm.deactivate("nonexistent").is_err());
    }
}
