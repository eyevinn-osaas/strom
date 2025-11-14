//! GStreamer pipeline management.

use crate::events::EventBroadcaster;
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use std::collections::HashMap;
use strom_types::{Element, Flow, FlowId, Link, PipelineState, PropertyValue, StromEvent};
use thiserror::Error;
use tracing::{debug, error, info, warn};

/// Result of processing links with automatic tee insertion.
struct ProcessedLinks {
    /// Final list of links (including links to/from tees)
    links: Vec<Link>,
    /// Map of tee element IDs to their source spec (element:pad they're connected to)
    tees: HashMap<String, String>,
}

#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("GStreamer error: {0}")]
    GStreamer(#[from] gst::glib::Error),

    #[error("GStreamer boolean error: {0}")]
    BoolError(#[from] gst::glib::BoolError),

    #[error("Element not found: {0}")]
    ElementNotFound(String),

    #[error("Failed to create element: {0}")]
    ElementCreation(String),

    #[error("Failed to link elements: {0} -> {1}")]
    LinkError(String, String),

    #[error("Invalid property value for {element}.{property}: {reason}")]
    InvalidProperty {
        element: String,
        property: String,
        reason: String,
    },

    #[error("Pipeline state change failed: {0}")]
    StateChange(String),

    #[error("Invalid flow: {0}")]
    InvalidFlow(String),
}

/// Manages a single GStreamer pipeline for a flow.
pub struct PipelineManager {
    flow_id: FlowId,
    flow_name: String,
    pipeline: gst::Pipeline,
    elements: HashMap<String, gst::Element>,
    bus_watch: Option<gst::bus::BusWatchGuard>,
    events: EventBroadcaster,
    /// Pending links that couldn't be made because source pads don't exist yet (dynamic pads)
    pending_links: Vec<Link>,
}

impl PipelineManager {
    /// Create a new pipeline from a flow definition.
    pub fn new(flow: &Flow, events: EventBroadcaster) -> Result<Self, PipelineError> {
        info!("Creating pipeline for flow: {} ({})", flow.name, flow.id);

        let pipeline = gst::Pipeline::builder()
            .name(format!("flow-{}", flow.id))
            .build();

        let mut manager = Self {
            flow_id: flow.id,
            flow_name: flow.name.clone(),
            pipeline,
            elements: HashMap::new(),
            bus_watch: None,
            events,
            pending_links: Vec::new(),
        };

        // Create and add all elements
        for element in &flow.elements {
            manager.add_element(element)?;
        }

        // Analyze links and auto-insert tee elements where needed
        let processed_links = Self::insert_tees_if_needed(&flow.links);

        // Create tee elements
        for tee_id in processed_links.tees.keys() {
            manager.add_tee_element(tee_id)?;
        }

        // Link elements according to processed links
        for link in &processed_links.links {
            if let Err(e) = manager.try_link_elements(link) {
                debug!(
                    "Could not link immediately: {} - will try when pad becomes available",
                    e
                );
                // Store as pending link
                manager.pending_links.push(link.clone());
            }
        }

        // Set up dynamic pad handlers for all elements that might have dynamic pads
        manager.setup_dynamic_pad_handlers();

        // Note: Bus watch is set up when pipeline starts, not here
        debug!("Pipeline created successfully for flow: {}", flow.name);
        Ok(manager)
    }

    /// Set up the bus watch to monitor pipeline messages.
    fn setup_bus_watch(&mut self) {
        // Clean up any existing watch first
        if self.bus_watch.is_some() {
            debug!("Removing existing bus watch for flow: {}", self.flow_name);
            self.bus_watch = None;
        }

        let bus = self.pipeline.bus().expect("Pipeline should have a bus");
        let flow_id = self.flow_id;
        let flow_name = self.flow_name.clone();
        let events = self.events.clone();

        let watch = bus
            .add_watch(move |_bus, msg| {
                use gst::MessageView;

                match msg.view() {
                    MessageView::Error(err) => {
                        let error_msg = err.error().to_string();
                        let debug_info = err.debug();
                        let source = err.src().map(|s| s.name().to_string());

                        error!(
                            "Pipeline error in flow '{}': {} (debug: {:?}, source: {:?})",
                            flow_name, error_msg, debug_info, source
                        );

                        events.broadcast(StromEvent::PipelineError {
                            flow_id,
                            error: error_msg,
                            source,
                        });
                    }
                    MessageView::Warning(warn) => {
                        let warning_msg = warn.error().to_string();
                        let debug_info = warn.debug();
                        let source = warn.src().map(|s| s.name().to_string());

                        warn!(
                            "Pipeline warning in flow '{}': {} (debug: {:?}, source: {:?})",
                            flow_name, warning_msg, debug_info, source
                        );

                        events.broadcast(StromEvent::PipelineWarning {
                            flow_id,
                            warning: warning_msg,
                            source,
                        });
                    }
                    MessageView::Info(inf) => {
                        let info_msg = inf.error().to_string();
                        let source = inf.src().map(|s| s.name().to_string());

                        info!(
                            "Pipeline info in flow '{}': {} (source: {:?})",
                            flow_name, info_msg, source
                        );

                        events.broadcast(StromEvent::PipelineInfo {
                            flow_id,
                            message: info_msg,
                            source,
                        });
                    }
                    MessageView::Eos(_) => {
                        info!("Pipeline '{}' reached end of stream", flow_name);
                        events.broadcast(StromEvent::PipelineEos { flow_id });
                    }
                    MessageView::StateChanged(state_changed) => {
                        // Log state changes from all elements to debug pausing issues
                        if let Some(source) = msg.src() {
                            let source_name = source.name();
                            let old_state = state_changed.old();
                            let new_state = state_changed.current();
                            let pending_state = state_changed.pending();

                            if source.type_() == gst::Pipeline::static_type() {
                                info!(
                                    "Pipeline '{}' state changed: {:?} -> {:?} (pending: {:?})",
                                    flow_name,
                                    old_state,
                                    new_state,
                                    pending_state
                                );
                            } else {
                                // Log all element state changes for debugging
                                info!(
                                    "Element '{}' in pipeline '{}' state changed: {:?} -> {:?} (pending: {:?})",
                                    source_name,
                                    flow_name,
                                    old_state,
                                    new_state,
                                    pending_state
                                );
                            }
                        }
                    }
                    _ => {
                        // Ignore other message types
                    }
                }

                glib::ControlFlow::Continue
            })
            .expect("Failed to add bus watch");

        self.bus_watch = Some(watch);
        debug!("Bus watch set up for flow: {}", self.flow_name);
    }

    /// Remove the bus watch.
    fn remove_bus_watch(&mut self) {
        if self.bus_watch.is_some() {
            debug!("Removing bus watch for flow: {}", self.flow_name);
            self.bus_watch = None;
        }
    }

    /// Add an element to the pipeline.
    fn add_element(&mut self, element_def: &Element) -> Result<(), PipelineError> {
        debug!(
            "Creating element: {} (type: {})",
            element_def.id, element_def.element_type
        );

        // Create the element
        let element = gst::ElementFactory::make(&element_def.element_type)
            .name(&element_def.id)
            .build()
            .map_err(|e| {
                PipelineError::ElementCreation(format!(
                    "{}: {} - {}",
                    element_def.id, element_def.element_type, e
                ))
            })?;

        // Set properties
        for (prop_name, prop_value) in &element_def.properties {
            self.set_property(&element, &element_def.id, prop_name, prop_value)?;
        }

        // Add to pipeline
        self.pipeline.add(&element).map_err(|e| {
            PipelineError::ElementCreation(format!(
                "Failed to add {} to pipeline: {}",
                element_def.id, e
            ))
        })?;

        self.elements.insert(element_def.id.clone(), element);
        Ok(())
    }

    /// Set a property on an element.
    fn set_property(
        &self,
        element: &gst::Element,
        element_id: &str,
        prop_name: &str,
        prop_value: &PropertyValue,
    ) -> Result<(), PipelineError> {
        debug!(
            "Setting property: {}.{} = {:?}",
            element_id, prop_name, prop_value
        );

        // Set property based on type
        match prop_value {
            PropertyValue::String(v) => {
                element.set_property_from_str(prop_name, v);
            }
            PropertyValue::Int(v) => {
                // Check property type to determine if we need i32 or i64
                if let Some(pspec) = element.find_property(prop_name) {
                    let type_name = pspec.value_type().name();
                    if type_name == "gint" || type_name == "glong" {
                        // Property expects i32
                        if let Ok(v32) = i32::try_from(*v) {
                            element.set_property(prop_name, v32);
                        } else {
                            return Err(PipelineError::InvalidProperty {
                                element: element_id.to_string(),
                                property: prop_name.to_string(),
                                reason: format!("Value {} doesn't fit in i32", v),
                            });
                        }
                    } else if type_name == "gint64" {
                        // Property expects i64
                        element.set_property(prop_name, *v);
                    } else {
                        // Try i64, might work
                        element.set_property(prop_name, *v);
                    }
                } else {
                    // Property not found, try anyway
                    element.set_property(prop_name, *v);
                }
            }
            PropertyValue::UInt(v) => {
                // Check property type to determine if we need u32 or u64
                if let Some(pspec) = element.find_property(prop_name) {
                    let type_name = pspec.value_type().name();
                    if type_name == "guint" || type_name == "gulong" {
                        // Property expects u32
                        if let Ok(v32) = u32::try_from(*v) {
                            element.set_property(prop_name, v32);
                        } else {
                            return Err(PipelineError::InvalidProperty {
                                element: element_id.to_string(),
                                property: prop_name.to_string(),
                                reason: format!("Value {} doesn't fit in u32", v),
                            });
                        }
                    } else if type_name == "guint64" {
                        // Property expects u64
                        element.set_property(prop_name, *v);
                    } else {
                        // Try u64, might work
                        element.set_property(prop_name, *v);
                    }
                } else {
                    // Property not found, try anyway
                    element.set_property(prop_name, *v);
                }
            }
            PropertyValue::Float(v) => {
                element.set_property(prop_name, *v);
            }
            PropertyValue::Bool(v) => {
                element.set_property(prop_name, *v);
            }
        }

        Ok(())
    }

    /// Try to link two elements according to a link definition.
    /// Returns Ok if successful, Err if pads don't exist yet (dynamic pads).
    fn try_link_elements(&self, link: &Link) -> Result<(), PipelineError> {
        debug!("Trying to link: {} -> {}", link.from, link.to);

        // Parse element:pad format (e.g., "src" or "src:pad_name")
        let (from_element, from_pad) = Self::parse_element_pad(&link.from);
        let (to_element, to_pad) = Self::parse_element_pad(&link.to);

        let src = self
            .elements
            .get(from_element)
            .ok_or_else(|| PipelineError::ElementNotFound(from_element.to_string()))?;

        let sink = self
            .elements
            .get(to_element)
            .ok_or_else(|| PipelineError::ElementNotFound(to_element.to_string()))?;

        // Link with or without specific pads
        if let (Some(src_pad_name), Some(sink_pad_name)) = (from_pad, to_pad) {
            // Try to get the pad - try static first, then request if not found
            let src_pad_obj = if let Some(pad) = src.static_pad(src_pad_name) {
                pad
            } else if let Some(pad) = src.request_pad_simple(src_pad_name) {
                // Request pad (for elements like tee with src_%u pads)
                pad
            } else {
                // Pad doesn't exist - might be a dynamic pad
                return Err(PipelineError::LinkError(
                    link.from.clone(),
                    format!(
                        "Source pad {} not available yet (dynamic pad)",
                        src_pad_name
                    ),
                ));
            };

            // Try to get sink pad - try static first, then request if not found
            let sink_pad_obj = if let Some(pad) = sink.static_pad(sink_pad_name) {
                pad
            } else if let Some(pad) = sink.request_pad_simple(sink_pad_name) {
                // Request pad (for elements with request sink pads)
                pad
            } else {
                // Pad doesn't exist - might be a dynamic pad
                return Err(PipelineError::LinkError(
                    link.to.clone(),
                    format!("Sink pad {} not available yet (dynamic pad)", sink_pad_name),
                ));
            };

            src_pad_obj.link(&sink_pad_obj).map_err(|e| {
                PipelineError::LinkError(link.from.clone(), format!("{} - {}", link.to, e))
            })?;

            debug!("Successfully linked: {} -> {}", link.from, link.to);
        } else {
            // Simple link without pad names
            src.link(sink).map_err(|e| {
                PipelineError::LinkError(link.from.clone(), format!("{} - {}", link.to, e))
            })?;

            debug!("Successfully linked: {} -> {}", link.from, link.to);
        }

        Ok(())
    }

    /// Parse element:pad format into (element_id, optional pad_name).
    fn parse_element_pad(spec: &str) -> (&str, Option<&str>) {
        if let Some((element, pad)) = spec.split_once(':') {
            (element, Some(pad))
        } else {
            (spec, None)
        }
    }

    /// Set up pad-added signal handlers for elements with dynamic pads.
    fn setup_dynamic_pad_handlers(&mut self) {
        if self.pending_links.is_empty() {
            return;
        }

        info!(
            "Setting up dynamic pad handlers for {} pending link(s)",
            self.pending_links.len()
        );

        // For each element that might have dynamic pads, connect to pad-added signal
        let elements_map = self.elements.clone();
        let pending_links = self.pending_links.clone();

        for (element_id, element) in &self.elements {
            let element_id = element_id.clone();
            let elements_map = elements_map.clone();
            let pending_links = pending_links.clone();

            // Connect to pad-added signal
            element.connect_pad_added(move |_elem, new_pad| {
                let new_pad_name = new_pad.name();
                debug!("Pad added on element {}: {}", element_id, new_pad_name);

                // Check if any pending links match this pad
                for link in &pending_links {
                    let (from_elem, from_pad) = Self::parse_element_pad(&link.from);
                    let (to_elem, to_pad) = Self::parse_element_pad(&link.to);

                    // Check if this new pad matches a pending source pad
                    if from_elem == element_id {
                        if let Some(expected_pad_name) = from_pad {
                            if new_pad_name == expected_pad_name {
                                // This is the source pad we're waiting for
                                if let (Some(_src_elem), Some(sink_elem)) =
                                    (elements_map.get(from_elem), elements_map.get(to_elem))
                                {
                                    if let Some(sink_pad_name) = to_pad {
                                        // Get the sink pad
                                        let sink_pad = if let Some(pad) =
                                            sink_elem.static_pad(sink_pad_name)
                                        {
                                            pad
                                        } else if let Some(pad) =
                                            sink_elem.request_pad_simple(sink_pad_name)
                                        {
                                            pad
                                        } else {
                                            warn!(
                                                "Sink pad {} not found on {}",
                                                sink_pad_name, to_elem
                                            );
                                            continue;
                                        };

                                        // Try to link
                                        match new_pad.link(&sink_pad) {
                                            Ok(_) => {
                                                info!(
                                                    "Successfully linked dynamic pad: {} -> {}",
                                                    link.from, link.to
                                                );
                                            }
                                            Err(e) => {
                                                error!(
                                                    "Failed to link dynamic pad {} -> {}: {}",
                                                    link.from, link.to, e
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }
    }

    /// Analyze links and insert tee elements where multiple links share the same source.
    fn insert_tees_if_needed(original_links: &[Link]) -> ProcessedLinks {
        use std::collections::HashMap;

        // Count how many times each source spec appears
        let mut source_counts: HashMap<String, usize> = HashMap::new();
        for link in original_links {
            *source_counts.entry(link.from.clone()).or_insert(0) += 1;
        }

        // Find sources that need a tee (appear more than once)
        let sources_needing_tee: Vec<String> = source_counts
            .iter()
            .filter(|(_, &count)| count > 1)
            .map(|(src, _)| src.clone())
            .collect();

        if sources_needing_tee.is_empty() {
            // No tees needed, return original links
            info!("No tee elements needed");
            return ProcessedLinks {
                links: original_links.to_vec(),
                tees: HashMap::new(),
            };
        }

        info!(
            "Auto-inserting {} tee element(s) for sources with multiple outputs",
            sources_needing_tee.len()
        );

        let mut new_links = Vec::new();
        let mut tees = HashMap::new();
        let mut tee_src_counters: HashMap<String, usize> = HashMap::new();

        for src_spec in &sources_needing_tee {
            let tee_id = format!("auto_tee_{}", src_spec.replace(":", "_"));
            tees.insert(tee_id.clone(), src_spec.clone());

            // Add link from original source to tee (without explicit sink pad, tee will auto-connect)
            new_links.push(Link {
                from: src_spec.clone(),
                to: format!("{}:sink", tee_id),
            });

            info!("Created tee element '{}' for source '{}'", tee_id, src_spec);
        }

        // Process original links
        for link in original_links {
            if sources_needing_tee.contains(&link.from) {
                // This source needs a tee - link from tee to destination
                let tee_id = format!("auto_tee_{}", link.from.replace(":", "_"));

                // Get next src pad from tee (src_0, src_1, src_2, ...)
                let counter = tee_src_counters.entry(tee_id.clone()).or_insert(0);
                let tee_src_pad = format!("{}:src_{}", tee_id, counter);
                *counter += 1;

                new_links.push(Link {
                    from: tee_src_pad,
                    to: link.to.clone(),
                });
            } else {
                // No tee needed, keep original link
                new_links.push(link.clone());
            }
        }

        ProcessedLinks {
            links: new_links,
            tees,
        }
    }

    /// Add a tee element to the pipeline.
    fn add_tee_element(&mut self, tee_id: &str) -> Result<(), PipelineError> {
        debug!("Creating auto-inserted tee element: {}", tee_id);

        let tee = gst::ElementFactory::make("tee")
            .name(tee_id)
            .build()
            .map_err(|e| {
                PipelineError::ElementCreation(format!("Failed to create tee {}: {}", tee_id, e))
            })?;

        // Configure tee to allow branches to not be linked without affecting other branches
        // This prevents one branch from blocking others when it goes to PAUSED
        tee.set_property("allow-not-linked", true);

        self.pipeline.add(&tee).map_err(|e| {
            PipelineError::ElementCreation(format!(
                "Failed to add tee {} to pipeline: {}",
                tee_id, e
            ))
        })?;

        self.elements.insert(tee_id.to_string(), tee);
        Ok(())
    }

    /// Start the pipeline (set to PLAYING state).
    pub fn start(&mut self) -> Result<PipelineState, PipelineError> {
        info!("Starting pipeline: {}", self.flow_name);

        // Set up bus watch before starting
        self.setup_bus_watch();

        info!("Setting pipeline '{}' to PLAYING state", self.flow_name);
        let state_change_result = self.pipeline.set_state(gst::State::Playing);

        match &state_change_result {
            Ok(gst::StateChangeSuccess::Success) => {
                info!("Pipeline '{}' set to PLAYING: Success", self.flow_name);
            }
            Ok(gst::StateChangeSuccess::Async) => {
                info!(
                    "Pipeline '{}' set to PLAYING: Async (state change in progress)",
                    self.flow_name
                );
            }
            Ok(gst::StateChangeSuccess::NoPreroll) => {
                info!(
                    "Pipeline '{}' set to PLAYING: NoPreroll (live source)",
                    self.flow_name
                );
            }
            Err(e) => {
                error!("Pipeline '{}' failed to start: {}", self.flow_name, e);
            }
        }

        state_change_result
            .map_err(|e| PipelineError::StateChange(format!("Failed to start: {}", e)))?;

        // Wait a moment to get the actual state
        std::thread::sleep(std::time::Duration::from_millis(100));
        let (result, current_state, pending_state) =
            self.pipeline.state(gst::ClockTime::from_mseconds(100));
        info!(
            "Pipeline '{}' state after start: result={:?}, current={:?}, pending={:?}",
            self.flow_name, result, current_state, pending_state
        );

        Ok(PipelineState::Playing)
    }

    /// Stop the pipeline (set to NULL state).
    pub fn stop(&mut self) -> Result<PipelineState, PipelineError> {
        info!("Stopping pipeline: {}", self.flow_name);

        self.pipeline
            .set_state(gst::State::Null)
            .map_err(|e| PipelineError::StateChange(format!("Failed to stop: {}", e)))?;

        // Remove bus watch when stopped to free resources
        self.remove_bus_watch();

        Ok(PipelineState::Null)
    }

    /// Pause the pipeline.
    pub fn pause(&self) -> Result<PipelineState, PipelineError> {
        info!("Pausing pipeline: {}", self.flow_name);

        self.pipeline
            .set_state(gst::State::Paused)
            .map_err(|e| PipelineError::StateChange(format!("Failed to pause: {}", e)))?;

        Ok(PipelineState::Paused)
    }

    /// Get the current state of the pipeline.
    pub fn get_state(&self) -> PipelineState {
        let (_, state, _) = self.pipeline.state(gst::ClockTime::from_seconds(1));

        match state {
            gst::State::Null => PipelineState::Null,
            gst::State::Ready => PipelineState::Ready,
            gst::State::Paused => PipelineState::Paused,
            gst::State::Playing => PipelineState::Playing,
            _ => PipelineState::Null,
        }
    }

    /// Get the flow ID this pipeline manages.
    pub fn flow_id(&self) -> FlowId {
        self.flow_id
    }

    /// Get the underlying GStreamer pipeline (for debugging).
    pub fn pipeline(&self) -> &gst::Pipeline {
        &self.pipeline
    }

    /// Generate a DOT graph of the pipeline for debugging.
    /// Returns the DOT graph content as a string.
    pub fn generate_dot_graph(&self) -> String {
        use gst::prelude::*;

        info!("Generating DOT graph for pipeline: {}", self.flow_name);

        // Use GStreamer's debug graph dump functionality
        // The details level determines how much information is included
        let dot = self
            .pipeline
            .debug_to_dot_data(gst::DebugGraphDetails::all());

        dot.to_string()
    }
}

impl Drop for PipelineManager {
    fn drop(&mut self) {
        debug!("Dropping pipeline for flow: {}", self.flow_name);
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_flow() -> Flow {
        let mut flow = Flow::new("Test Pipeline");
        flow.elements = vec![
            Element {
                id: "src".to_string(),
                element_type: "videotestsrc".to_string(),
                properties: HashMap::from([("is-live".to_string(), PropertyValue::Bool(true))]),
                position: None,
            },
            Element {
                id: "sink".to_string(),
                element_type: "fakesink".to_string(),
                properties: HashMap::new(),
                position: None,
            },
        ];
        flow.links = vec![Link {
            from: "src".to_string(),
            to: "sink".to_string(),
        }];
        flow
    }

    #[test]
    fn test_create_pipeline() {
        gst::init().unwrap();
        let flow = create_test_flow();
        let events = EventBroadcaster::default();
        let manager = PipelineManager::new(&flow, events);
        assert!(manager.is_ok());
    }

    #[test]
    fn test_start_stop_pipeline() {
        gst::init().unwrap();
        let flow = create_test_flow();
        let events = EventBroadcaster::default();
        let mut manager = PipelineManager::new(&flow, events).unwrap();

        // Start pipeline
        let state = manager.start();
        assert!(state.is_ok());
        assert_eq!(state.unwrap(), PipelineState::Playing);

        // Check state
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(manager.get_state(), PipelineState::Playing);

        // Stop pipeline
        let state = manager.stop();
        assert!(state.is_ok());
        assert_eq!(state.unwrap(), PipelineState::Null);
    }

    #[test]
    fn test_invalid_element() {
        gst::init().unwrap();
        let mut flow = create_test_flow();
        flow.elements[0].element_type = "nonexistentelement".to_string();

        let events = EventBroadcaster::default();
        let manager = PipelineManager::new(&flow, events);
        assert!(manager.is_err());
    }

    #[test]
    fn test_auto_tee_insertion() {
        gst::init().unwrap();

        // Create a flow with one source and two sinks (should auto-insert a tee)
        let mut flow = Flow::new("Auto-Tee Test");
        flow.elements = vec![
            Element {
                id: "src".to_string(),
                element_type: "videotestsrc".to_string(),
                properties: HashMap::new(),
                position: None,
            },
            Element {
                id: "sink1".to_string(),
                element_type: "fakesink".to_string(),
                properties: HashMap::new(),
                position: None,
            },
            Element {
                id: "sink2".to_string(),
                element_type: "fakesink".to_string(),
                properties: HashMap::new(),
                position: None,
            },
        ];
        flow.links = vec![
            Link {
                from: "src".to_string(),
                to: "sink1".to_string(),
            },
            Link {
                from: "src".to_string(),
                to: "sink2".to_string(),
            },
        ];

        let events = EventBroadcaster::default();
        let manager = PipelineManager::new(&flow, events);
        assert!(manager.is_ok());

        let manager = manager.unwrap();
        // Should have 3 original elements + 1 auto-inserted tee
        assert_eq!(manager.elements.len(), 4);
        // Check that tee element was created
        assert!(manager.elements.contains_key("auto_tee_src"));
    }

    #[test]
    fn test_no_tee_insertion_when_not_needed() {
        gst::init().unwrap();

        let flow = create_test_flow(); // Simple 1-to-1 connection

        let events = EventBroadcaster::default();
        let manager = PipelineManager::new(&flow, events).unwrap();

        // Should have only 2 original elements, no tee
        assert_eq!(manager.elements.len(), 2);
        assert!(!manager.elements.contains_key("auto_tee_src"));
    }
}
