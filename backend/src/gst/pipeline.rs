//! GStreamer pipeline management.

use crate::events::EventBroadcaster;
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use std::collections::HashMap;
use strom_types::{Element, Flow, FlowId, Link, PipelineState, PropertyValue, StromEvent};
use thiserror::Error;
use tracing::{debug, error, info, warn};

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
        };

        // Create and add all elements
        for element in &flow.elements {
            manager.add_element(element)?;
        }

        // Link elements according to flow definition
        for link in &flow.links {
            manager.link_elements(link)?;
        }

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
                        // Only log state changes from the pipeline itself, not individual elements
                        if let Some(source) = msg.src() {
                            if source.type_() == gst::Pipeline::static_type() {
                                debug!(
                                    "Pipeline '{}' state changed: {:?} -> {:?}",
                                    flow_name,
                                    state_changed.old(),
                                    state_changed.current()
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
                element.set_property(prop_name, *v);
            }
            PropertyValue::UInt(v) => {
                element.set_property(prop_name, *v);
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

    /// Link two elements according to a link definition.
    fn link_elements(&self, link: &Link) -> Result<(), PipelineError> {
        debug!("Linking: {} -> {}", link.from, link.to);

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
        if let (Some(src_pad), Some(sink_pad)) = (from_pad, to_pad) {
            let src_pad_obj = src
                .static_pad(src_pad)
                .ok_or_else(|| PipelineError::LinkError(link.from.clone(), link.to.clone()))?;

            let sink_pad_obj = sink
                .static_pad(sink_pad)
                .ok_or_else(|| PipelineError::LinkError(link.from.clone(), link.to.clone()))?;

            src_pad_obj.link(&sink_pad_obj).map_err(|e| {
                PipelineError::LinkError(link.from.clone(), format!("{} - {}", link.to, e))
            })?;
        } else {
            // Simple link without pad names
            src.link(sink).map_err(|e| {
                PipelineError::LinkError(link.from.clone(), format!("{} - {}", link.to, e))
            })?;
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

    /// Start the pipeline (set to PLAYING state).
    pub fn start(&mut self) -> Result<PipelineState, PipelineError> {
        info!("Starting pipeline: {}", self.flow_name);

        // Set up bus watch before starting
        self.setup_bus_watch();

        self.pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| PipelineError::StateChange(format!("Failed to start: {}", e)))?;

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
}
