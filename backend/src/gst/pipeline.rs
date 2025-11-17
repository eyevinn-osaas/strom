//! GStreamer pipeline management.

use crate::blocks::BlockRegistry;
use crate::events::EventBroadcaster;
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_net as gst_net;
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

    #[error("Property {property} on element {element} cannot be changed in {state:?} state")]
    PropertyNotMutable {
        element: String,
        property: String,
        state: PipelineState,
    },

    #[error("Pad not found: {element}:{pad}")]
    PadNotFound { element: String, pad: String },
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
    /// Flow properties (clock configuration, etc.)
    properties: strom_types::flow::FlowProperties,
    /// Pad properties to apply after pads are created (element_id -> (pad_name -> properties))
    pad_properties: HashMap<String, HashMap<String, HashMap<String, PropertyValue>>>,
}

impl PipelineManager {
    /// Create a new pipeline from a flow definition.
    pub fn new(
        flow: &Flow,
        events: EventBroadcaster,
        block_registry: &BlockRegistry,
    ) -> Result<Self, PipelineError> {
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
            properties: flow.properties.clone(),
            pad_properties: HashMap::new(),
        };

        // Expand blocks into native elements and links
        let expanded = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                super::block_expansion::expand_blocks(
                    &flow.blocks,
                    &flow.elements,
                    &flow.links,
                    block_registry,
                )
                .await
            })
        })?;

        // Create and add all elements (from both flow and expanded blocks)
        for element in &expanded.elements {
            manager.add_element(element)?;
        }

        // Analyze links and auto-insert tee elements where needed
        let processed_links = Self::insert_tees_if_needed(&expanded.links);

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

        // Apply pad properties now that pads have been created (during linking)
        // Note: Request pads (like audiomixer sink_%u) are created during linking
        manager.apply_pad_properties();

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

        let Some(bus) = self.pipeline.bus() else {
            error!(
                "Pipeline '{}' does not have a bus - cannot set up message watch",
                self.flow_name
            );
            return;
        };
        let flow_id = self.flow_id;
        let flow_name = self.flow_name.clone();
        let events = self.events.clone();

        let watch = match bus
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
            }) {
            Ok(watch) => watch,
            Err(e) => {
                error!("Failed to add bus watch for flow '{}': {}", self.flow_name, e);
                return;
            }
        };

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

        // Store pad properties for later application (after pads are created)
        if !element_def.pad_properties.is_empty() {
            debug!(
                "Storing {} pad properties for element {}",
                element_def.pad_properties.len(),
                element_def.id
            );
            self.pad_properties
                .insert(element_def.id.clone(), element_def.pad_properties.clone());
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
    /// Handles namespaced block elements like "block_0:rtpL24pay:sink".
    /// Splits from the right to get the last colon-separated part as the pad name.
    fn parse_element_pad(spec: &str) -> (&str, Option<&str>) {
        if let Some((element, pad)) = spec.rsplit_once(':') {
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

    /// Apply stored pad properties to pads after they've been created.
    /// This must be called after all linking is complete, since request pads
    /// (like audiomixer sink_%u) don't exist until they're requested during linking.
    fn apply_pad_properties(&self) {
        if self.pad_properties.is_empty() {
            return;
        }

        info!(
            "Applying pad properties for {} element(s)",
            self.pad_properties.len()
        );

        for (element_id, pad_props) in &self.pad_properties {
            let Some(element) = self.elements.get(element_id) else {
                warn!(
                    "Element {} not found when trying to apply pad properties",
                    element_id
                );
                continue;
            };

            for (pad_name, properties) in pad_props {
                // Try to get the pad - try static first, then request
                let pad = if let Some(p) = element.static_pad(pad_name) {
                    p
                } else if let Some(p) = element.request_pad_simple(pad_name) {
                    p
                } else {
                    warn!(
                        "Pad {}:{} not found when trying to apply pad properties",
                        element_id, pad_name
                    );
                    continue;
                };

                debug!(
                    "Applying {} properties to pad {}:{}",
                    properties.len(),
                    element_id,
                    pad_name
                );

                // Apply each property
                for (prop_name, prop_value) in properties {
                    if let Err(e) =
                        self.set_pad_property(&pad, element_id, pad_name, prop_name, prop_value)
                    {
                        error!(
                            "Failed to set pad property {}:{}:{}: {}",
                            element_id, pad_name, prop_name, e
                        );
                    } else {
                        info!(
                            "Set pad property {}:{}:{} = {:?}",
                            element_id, pad_name, prop_name, prop_value
                        );
                    }
                }
            }
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

    /// Configure the pipeline clock based on flow properties.
    fn configure_clock(&mut self) -> Result<(), PipelineError> {
        use strom_types::flow::GStreamerClockType;

        match self.properties.clock_type {
            GStreamerClockType::Ptp => {
                let domain = self.properties.ptp_domain.unwrap_or(0);
                info!(
                    "Configuring PTP clock for pipeline '{}' with domain {}",
                    self.flow_name, domain
                );

                // Initialize PTP clock (on all interfaces)
                // This is a global init, so it's OK if it was already initialized
                if let Err(e) = gst_net::PtpClock::init(None, &[]) {
                    warn!(
                        "PTP clock initialization warning (may already be initialized): {}",
                        e
                    );
                }

                // Create a PTP clock instance for this domain
                let ptp_clock = gst_net::PtpClock::new(None, domain as u32).map_err(|e| {
                    PipelineError::StateChange(format!("Failed to create PTP clock: {}", e))
                })?;

                // Set the PTP clock as the pipeline's clock
                self.pipeline.set_clock(Some(&ptp_clock)).map_err(|e| {
                    PipelineError::StateChange(format!("Failed to set PTP clock: {}", e))
                })?;

                // For PTP, set base_time to 0 and don't use start_time
                // This makes the pipeline refer directly to the PTP clock
                self.pipeline.set_base_time(gst::ClockTime::ZERO);
                self.pipeline.set_start_time(gst::ClockTime::NONE);

                info!(
                    "PTP clock configured: domain={}, base_time=0, start_time=None",
                    domain
                );
            }
            GStreamerClockType::Monotonic => {
                info!("Using Monotonic clock for pipeline '{}'", self.flow_name);
                // Create a system monotonic clock
                let clock = gst::SystemClock::obtain();
                self.pipeline.set_clock(Some(&clock)).map_err(|e| {
                    PipelineError::StateChange(format!("Failed to set monotonic clock: {}", e))
                })?;
            }
            GStreamerClockType::Realtime => {
                info!("Using Realtime clock for pipeline '{}'", self.flow_name);
                // For realtime, we'd need a custom clock implementation
                // For now, use the system clock which is close to realtime
                let clock = gst::SystemClock::obtain();
                self.pipeline.set_clock(Some(&clock)).map_err(|e| {
                    PipelineError::StateChange(format!("Failed to set realtime clock: {}", e))
                })?;
            }
            GStreamerClockType::PipelineDefault => {
                info!(
                    "Using pipeline default clock for pipeline '{}' (letting GStreamer choose)",
                    self.flow_name
                );
                // Don't set a clock - let GStreamer choose the default
            }
            GStreamerClockType::Ntp => {
                info!(
                    "NTP clock requested for pipeline '{}' - using system clock as fallback",
                    self.flow_name
                );
                // NTP clock implementation would require additional setup
                // For now, fall back to system clock
                let clock = gst::SystemClock::obtain();
                self.pipeline.set_clock(Some(&clock)).map_err(|e| {
                    PipelineError::StateChange(format!("Failed to set clock: {}", e))
                })?;
                warn!("NTP clock not yet fully implemented, using system clock");
            }
        }

        Ok(())
    }

    /// Start the pipeline (set to PLAYING state).
    pub fn start(&mut self) -> Result<PipelineState, PipelineError> {
        info!("Starting pipeline: {}", self.flow_name);

        // Set up bus watch before starting
        self.setup_bus_watch();

        // Configure clock before starting
        self.configure_clock()?;

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

        // Check if state change succeeded
        if let Err(e) = result {
            error!(
                "Pipeline '{}' failed to reach PLAYING state: {:?} (current: {:?}, pending: {:?})",
                self.flow_name, e, current_state, pending_state
            );
            return Err(PipelineError::StateChange(format!(
                "State change failed: {:?} - current: {:?}, pending: {:?}",
                e, current_state, pending_state
            )));
        }

        // Return the actual current state
        let actual_state = match current_state {
            gst::State::Null => PipelineState::Null,
            gst::State::Ready => PipelineState::Ready,
            gst::State::Paused => PipelineState::Paused,
            gst::State::Playing => PipelineState::Playing,
            _ => PipelineState::Null,
        };

        Ok(actual_state)
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

    /// Get the clock synchronization status for this pipeline.
    pub fn get_clock_sync_status(&self) -> strom_types::flow::ClockSyncStatus {
        use strom_types::flow::{ClockSyncStatus, GStreamerClockType};

        match self.properties.clock_type {
            GStreamerClockType::Ptp | GStreamerClockType::Ntp => {
                // Get the pipeline's clock
                if let Some(clock) = self.pipeline.clock() {
                    // For PTP/NTP clocks, check if they're synced
                    // Try to get the "synced" property (may not exist on all clock types)
                    if clock.has_property("synced") {
                        let synced = clock.property::<bool>("synced");
                        if synced {
                            ClockSyncStatus::Synced
                        } else {
                            ClockSyncStatus::NotSynced
                        }
                    } else {
                        // Property doesn't exist, fall back to checking if clock is working
                        let time = clock.time();
                        if time.is_some() {
                            // Clock is providing time, assume synced
                            ClockSyncStatus::Synced
                        } else {
                            ClockSyncStatus::NotSynced
                        }
                    }
                } else {
                    ClockSyncStatus::Unknown
                }
            }
            _ => {
                // For other clock types, sync status is not applicable
                ClockSyncStatus::Unknown
            }
        }
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

    /// Update a property on a live element in the pipeline.
    /// Validates that the property can be changed in the current pipeline state.
    pub fn update_element_property(
        &self,
        element_id: &str,
        property_name: &str,
        value: &PropertyValue,
    ) -> Result<(), PipelineError> {
        debug!(
            "Updating property {}.{} to {:?} on running pipeline",
            element_id, property_name, value
        );

        // Get element reference
        let element = self
            .elements
            .get(element_id)
            .ok_or_else(|| PipelineError::ElementNotFound(element_id.to_string()))?;

        // Get current pipeline state
        let state = self.get_state();

        // Validate property is mutable in current state
        self.validate_property_mutability(element, element_id, property_name, state)?;

        // Set the property (reuse existing set_property method)
        self.set_property(element, element_id, property_name, value)?;

        info!(
            "Successfully updated property {}.{} to {:?}",
            element_id, property_name, value
        );

        Ok(())
    }

    /// Get current value of a property from a live element.
    pub fn get_element_property(
        &self,
        element_id: &str,
        property_name: &str,
    ) -> Result<PropertyValue, PipelineError> {
        let element = self
            .elements
            .get(element_id)
            .ok_or_else(|| PipelineError::ElementNotFound(element_id.to_string()))?;

        // Get property spec to determine type
        let pspec =
            element
                .find_property(property_name)
                .ok_or_else(|| PipelineError::InvalidProperty {
                    element: element_id.to_string(),
                    property: property_name.to_string(),
                    reason: "Property not found".to_string(),
                })?;

        let type_name = pspec.value_type().name();

        // Get property value based on type
        let value = match type_name.to_string().as_str() {
            "gchararray" => {
                let v = element.property::<Option<String>>(property_name);
                v.map(PropertyValue::String)
                    .unwrap_or(PropertyValue::String(String::new()))
            }
            "gboolean" => {
                let v = element.property::<bool>(property_name);
                PropertyValue::Bool(v)
            }
            "gint" | "glong" => {
                let v = element.property::<i32>(property_name);
                PropertyValue::Int(v as i64)
            }
            "gint64" => {
                let v = element.property::<i64>(property_name);
                PropertyValue::Int(v)
            }
            "guint" | "gulong" => {
                let v = element.property::<u32>(property_name);
                PropertyValue::UInt(v as u64)
            }
            "guint64" => {
                let v = element.property::<u64>(property_name);
                PropertyValue::UInt(v)
            }
            "gfloat" => {
                let v = element.property::<f32>(property_name);
                PropertyValue::Float(v as f64)
            }
            "gdouble" => {
                let v = element.property::<f64>(property_name);
                PropertyValue::Float(v)
            }
            "GEnum" => {
                // Get enum as string
                // In GStreamer 0.24.x, enum properties have stricter types and can't always be read as i32
                // We need to use the Value API and handle type conversion carefully
                if let Some(param_spec) = pspec.downcast_ref::<glib::ParamSpecEnum>() {
                    let enum_class = param_spec.enum_class();

                    // Get the property as a Value, then try to extract the enum value
                    let value = element.property_value(property_name);

                    // Try to get as i32 (standard enum representation)
                    match value.get::<i32>() {
                        Ok(v) => {
                            if let Some(enum_value) = enum_class.value(v) {
                                PropertyValue::String(enum_value.name().to_string())
                            } else {
                                PropertyValue::Int(v as i64)
                            }
                        }
                        Err(_) => {
                            // Can't convert to i32, this enum type is not supported
                            return Err(PipelineError::InvalidProperty {
                                element: element_id.to_string(),
                                property: property_name.to_string(),
                                reason: format!(
                                    "Cannot read enum property of type {} (not convertible to i32)",
                                    type_name
                                ),
                            });
                        }
                    }
                } else {
                    // Fallback if we can't get the enum class
                    return Err(PipelineError::InvalidProperty {
                        element: element_id.to_string(),
                        property: property_name.to_string(),
                        reason: "Cannot read enum property spec".to_string(),
                    });
                }
            }
            _ => {
                return Err(PipelineError::InvalidProperty {
                    element: element_id.to_string(),
                    property: property_name.to_string(),
                    reason: format!("Unsupported property type: {}", type_name),
                });
            }
        };

        Ok(value)
    }

    /// Get all readable property values from a live element.
    pub fn get_element_properties(
        &self,
        element_id: &str,
    ) -> Result<HashMap<String, PropertyValue>, PipelineError> {
        let element = self
            .elements
            .get(element_id)
            .ok_or_else(|| PipelineError::ElementNotFound(element_id.to_string()))?;

        let mut properties = HashMap::new();

        // Get all properties from the element
        for pspec in element.list_properties() {
            let name = pspec.name().to_string();

            // Skip non-readable properties
            if !pspec.flags().contains(glib::ParamFlags::READABLE) {
                continue;
            }

            // Skip internal/private properties
            if name.starts_with('_') {
                continue;
            }

            // Try to get the property value
            if let Ok(value) = self.get_element_property(element_id, &name) {
                properties.insert(name, value);
            }
        }

        Ok(properties)
    }

    /// Update a property on a pad in the pipeline.
    /// Validates that the property can be changed in the current pipeline state.
    pub fn update_pad_property(
        &self,
        element_id: &str,
        pad_name: &str,
        property_name: &str,
        value: &PropertyValue,
    ) -> Result<(), PipelineError> {
        debug!(
            "Updating pad property {}:{}:{} to {:?}",
            element_id, pad_name, property_name, value
        );

        // Get element reference
        let element = self
            .elements
            .get(element_id)
            .ok_or_else(|| PipelineError::ElementNotFound(element_id.to_string()))?;

        // Get pad reference - try static pad first, then request pad
        let pad = if let Some(p) = element.static_pad(pad_name) {
            p
        } else if let Some(p) = element.request_pad_simple(pad_name) {
            p
        } else {
            return Err(PipelineError::PadNotFound {
                element: element_id.to_string(),
                pad: pad_name.to_string(),
            });
        };

        // Get current pipeline state
        let state = self.get_state();

        // Validate property is mutable in current state (using pad's property spec)
        self.validate_pad_property_mutability(&pad, element_id, pad_name, property_name, state)?;

        // Set the property on the pad
        self.set_pad_property(&pad, element_id, pad_name, property_name, value)?;

        info!(
            "Successfully updated pad property {}:{}:{} to {:?}",
            element_id, pad_name, property_name, value
        );

        Ok(())
    }

    /// Get current value of a property from a pad.
    pub fn get_pad_property(
        &self,
        element_id: &str,
        pad_name: &str,
        property_name: &str,
    ) -> Result<PropertyValue, PipelineError> {
        let element = self
            .elements
            .get(element_id)
            .ok_or_else(|| PipelineError::ElementNotFound(element_id.to_string()))?;

        // Get pad reference
        let pad = if let Some(p) = element.static_pad(pad_name) {
            p
        } else if let Some(p) = element.request_pad_simple(pad_name) {
            p
        } else {
            return Err(PipelineError::PadNotFound {
                element: element_id.to_string(),
                pad: pad_name.to_string(),
            });
        };

        // Get property spec to determine type
        let pspec =
            pad.find_property(property_name)
                .ok_or_else(|| PipelineError::InvalidProperty {
                    element: format!("{}:{}", element_id, pad_name),
                    property: property_name.to_string(),
                    reason: "Property not found on pad".to_string(),
                })?;

        let type_name = pspec.value_type().name();

        // Get property value based on type
        let value = match type_name.to_string().as_str() {
            "gchararray" => {
                let v = pad.property::<Option<String>>(property_name);
                v.map(PropertyValue::String)
                    .unwrap_or(PropertyValue::String(String::new()))
            }
            "gboolean" => {
                let v = pad.property::<bool>(property_name);
                PropertyValue::Bool(v)
            }
            "gint" | "glong" => {
                let v = pad.property::<i32>(property_name);
                PropertyValue::Int(v as i64)
            }
            "gint64" => {
                let v = pad.property::<i64>(property_name);
                PropertyValue::Int(v)
            }
            "guint" | "gulong" => {
                let v = pad.property::<u32>(property_name);
                PropertyValue::UInt(v as u64)
            }
            "guint64" => {
                let v = pad.property::<u64>(property_name);
                PropertyValue::UInt(v)
            }
            "gfloat" => {
                let v = pad.property::<f32>(property_name);
                PropertyValue::Float(v as f64)
            }
            "gdouble" => {
                let v = pad.property::<f64>(property_name);
                PropertyValue::Float(v)
            }
            _ => {
                return Err(PipelineError::InvalidProperty {
                    element: format!("{}:{}", element_id, pad_name),
                    property: property_name.to_string(),
                    reason: format!("Unsupported property type: {}", type_name),
                });
            }
        };

        Ok(value)
    }

    /// Get all readable property values from a pad.
    pub fn get_pad_properties(
        &self,
        element_id: &str,
        pad_name: &str,
    ) -> Result<HashMap<String, PropertyValue>, PipelineError> {
        let element = self
            .elements
            .get(element_id)
            .ok_or_else(|| PipelineError::ElementNotFound(element_id.to_string()))?;

        // Get pad reference
        let pad = if let Some(p) = element.static_pad(pad_name) {
            p
        } else if let Some(p) = element.request_pad_simple(pad_name) {
            p
        } else {
            return Err(PipelineError::PadNotFound {
                element: element_id.to_string(),
                pad: pad_name.to_string(),
            });
        };

        let mut properties = HashMap::new();

        // Get all properties from the pad
        for pspec in pad.list_properties() {
            let name = pspec.name().to_string();

            // Skip non-readable properties
            if !pspec.flags().contains(glib::ParamFlags::READABLE) {
                continue;
            }

            // Skip internal/private properties
            if name.starts_with('_') {
                continue;
            }

            // Try to get the property value
            if let Ok(value) = self.get_pad_property(element_id, pad_name, &name) {
                properties.insert(name, value);
            }
        }

        Ok(properties)
    }

    /// Set a property on a pad.
    fn set_pad_property(
        &self,
        pad: &gst::Pad,
        element_id: &str,
        pad_name: &str,
        prop_name: &str,
        prop_value: &PropertyValue,
    ) -> Result<(), PipelineError> {
        debug!(
            "Setting pad property: {}:{}:{} = {:?}",
            element_id, pad_name, prop_name, prop_value
        );

        // Set property based on type
        match prop_value {
            PropertyValue::String(v) => {
                pad.set_property_from_str(prop_name, v);
            }
            PropertyValue::Int(v) => {
                // Check property type to determine if we need i32 or i64
                if let Some(pspec) = pad.find_property(prop_name) {
                    let type_name = pspec.value_type().name();
                    if type_name == "gint" || type_name == "glong" {
                        if let Ok(v32) = i32::try_from(*v) {
                            pad.set_property(prop_name, v32);
                        } else {
                            return Err(PipelineError::InvalidProperty {
                                element: format!("{}:{}", element_id, pad_name),
                                property: prop_name.to_string(),
                                reason: format!("Value {} doesn't fit in i32", v),
                            });
                        }
                    } else {
                        pad.set_property(prop_name, *v);
                    }
                } else {
                    pad.set_property(prop_name, *v);
                }
            }
            PropertyValue::UInt(v) => {
                if let Some(pspec) = pad.find_property(prop_name) {
                    let type_name = pspec.value_type().name();
                    if type_name == "guint" || type_name == "gulong" {
                        if let Ok(v32) = u32::try_from(*v) {
                            pad.set_property(prop_name, v32);
                        } else {
                            return Err(PipelineError::InvalidProperty {
                                element: format!("{}:{}", element_id, pad_name),
                                property: prop_name.to_string(),
                                reason: format!("Value {} doesn't fit in u32", v),
                            });
                        }
                    } else {
                        pad.set_property(prop_name, *v);
                    }
                } else {
                    pad.set_property(prop_name, *v);
                }
            }
            PropertyValue::Float(v) => {
                pad.set_property(prop_name, *v);
            }
            PropertyValue::Bool(v) => {
                pad.set_property(prop_name, *v);
            }
        }

        Ok(())
    }

    /// Validate that a pad property can be changed in the current pipeline state.
    fn validate_pad_property_mutability(
        &self,
        pad: &gst::Pad,
        element_id: &str,
        pad_name: &str,
        property_name: &str,
        current_state: PipelineState,
    ) -> Result<(), PipelineError> {
        let pspec =
            pad.find_property(property_name)
                .ok_or_else(|| PipelineError::InvalidProperty {
                    element: format!("{}:{}", element_id, pad_name),
                    property: property_name.to_string(),
                    reason: "Property not found on pad".to_string(),
                })?;

        let flags = pspec.flags();

        // Check if property is writable
        if !flags.contains(glib::ParamFlags::WRITABLE) {
            return Err(PipelineError::InvalidProperty {
                element: format!("{}:{}", element_id, pad_name),
                property: property_name.to_string(),
                reason: "Property is not writable".to_string(),
            });
        }

        // Check if property is construct-only
        if flags.contains(glib::ParamFlags::CONSTRUCT_ONLY) {
            return Err(PipelineError::InvalidProperty {
                element: format!("{}:{}", element_id, pad_name),
                property: property_name.to_string(),
                reason: "Property is construct-only and cannot be changed after pad creation"
                    .to_string(),
            });
        }

        // Check if property can be changed in current state
        // GStreamer-specific flags (from gstreamer-sys)
        // GST_PARAM_MUTABLE_READY = 0x400
        // GST_PARAM_MUTABLE_PAUSED = 0x800
        // GST_PARAM_MUTABLE_PLAYING = 0x1000
        // GST_PARAM_CONTROLLABLE = 0x200
        let flags_bits = flags.bits();
        let mutable_in_ready = (flags_bits & 0x400) != 0;
        let mutable_in_paused = (flags_bits & 0x800) != 0;
        let mutable_in_playing = (flags_bits & 0x1000) != 0;
        let controllable = (flags_bits & 0x200) != 0;

        // Controllable properties can generally be changed at runtime
        let can_change_at_runtime = controllable;

        match current_state {
            PipelineState::Playing => {
                if !mutable_in_playing && !can_change_at_runtime {
                    return Err(PipelineError::PropertyNotMutable {
                        element: format!("{}:{}", element_id, pad_name),
                        property: property_name.to_string(),
                        state: current_state,
                    });
                }
            }
            PipelineState::Paused => {
                if !mutable_in_paused && !mutable_in_playing {
                    return Err(PipelineError::PropertyNotMutable {
                        element: format!("{}:{}", element_id, pad_name),
                        property: property_name.to_string(),
                        state: current_state,
                    });
                }
            }
            PipelineState::Ready => {
                if !mutable_in_ready && !mutable_in_paused && !mutable_in_playing {
                    return Err(PipelineError::PropertyNotMutable {
                        element: format!("{}:{}", element_id, pad_name),
                        property: property_name.to_string(),
                        state: current_state,
                    });
                }
            }
            PipelineState::Null => {
                // All writable, non-construct-only properties can be changed in NULL state
            }
        }

        Ok(())
    }

    /// Validate that a property can be changed in the current pipeline state.
    fn validate_property_mutability(
        &self,
        element: &gst::Element,
        element_id: &str,
        property_name: &str,
        current_state: PipelineState,
    ) -> Result<(), PipelineError> {
        let pspec =
            element
                .find_property(property_name)
                .ok_or_else(|| PipelineError::InvalidProperty {
                    element: element_id.to_string(),
                    property: property_name.to_string(),
                    reason: "Property not found".to_string(),
                })?;

        let flags = pspec.flags();

        // Check if property is writable
        if !flags.contains(glib::ParamFlags::WRITABLE) {
            return Err(PipelineError::InvalidProperty {
                element: element_id.to_string(),
                property: property_name.to_string(),
                reason: "Property is not writable".to_string(),
            });
        }

        // Check if property is construct-only
        if flags.contains(glib::ParamFlags::CONSTRUCT_ONLY) {
            return Err(PipelineError::InvalidProperty {
                element: element_id.to_string(),
                property: property_name.to_string(),
                reason: "Property is construct-only and cannot be changed after element creation"
                    .to_string(),
            });
        }

        // Check if property can be changed in current state
        // GStreamer-specific flags (from gstreamer-sys)
        // GST_PARAM_MUTABLE_READY = 0x400
        // GST_PARAM_MUTABLE_PAUSED = 0x800
        // GST_PARAM_MUTABLE_PLAYING = 0x1000
        // GST_PARAM_CONTROLLABLE = 0x200
        let flags_bits = flags.bits();
        let mutable_in_ready = (flags_bits & 0x400) != 0;
        let mutable_in_paused = (flags_bits & 0x800) != 0;
        let mutable_in_playing = (flags_bits & 0x1000) != 0;
        let controllable = (flags_bits & 0x200) != 0;

        // Controllable properties can generally be changed at runtime
        // They're designed for dynamic updates via GstController
        let can_change_at_runtime = controllable;

        match current_state {
            PipelineState::Playing => {
                if !mutable_in_playing && !can_change_at_runtime {
                    return Err(PipelineError::PropertyNotMutable {
                        element: element_id.to_string(),
                        property: property_name.to_string(),
                        state: current_state,
                    });
                }
            }
            PipelineState::Paused => {
                if !mutable_in_paused && !mutable_in_playing {
                    return Err(PipelineError::PropertyNotMutable {
                        element: element_id.to_string(),
                        property: property_name.to_string(),
                        state: current_state,
                    });
                }
            }
            PipelineState::Ready => {
                if !mutable_in_ready && !mutable_in_paused && !mutable_in_playing {
                    return Err(PipelineError::PropertyNotMutable {
                        element: element_id.to_string(),
                        property: property_name.to_string(),
                        state: current_state,
                    });
                }
            }
            PipelineState::Null => {
                // All writable, non-construct-only properties can be changed in NULL state
            }
        }

        Ok(())
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
                pad_properties: HashMap::new(),
                position: None,
            },
            Element {
                id: "sink".to_string(),
                element_type: "fakesink".to_string(),
                properties: HashMap::new(),
                pad_properties: HashMap::new(),
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
        let registry = BlockRegistry::new("test_blocks.json");
        let manager = PipelineManager::new(&flow, events, &registry);
        assert!(manager.is_ok());
    }

    #[test]
    fn test_start_stop_pipeline() {
        gst::init().unwrap();
        let flow = create_test_flow();
        let events = EventBroadcaster::default();
        let registry = BlockRegistry::new("test_blocks.json");
        let mut manager = PipelineManager::new(&flow, events, &registry).unwrap();

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
        let registry = BlockRegistry::new("test_blocks.json");
        let manager = PipelineManager::new(&flow, events, &registry);
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
                pad_properties: HashMap::new(),
                position: None,
            },
            Element {
                id: "sink1".to_string(),
                element_type: "fakesink".to_string(),
                properties: HashMap::new(),
                pad_properties: HashMap::new(),
                position: None,
            },
            Element {
                id: "sink2".to_string(),
                element_type: "fakesink".to_string(),
                properties: HashMap::new(),
                pad_properties: HashMap::new(),
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
        let registry = BlockRegistry::new("test_blocks.json");
        let manager = PipelineManager::new(&flow, events, &registry);
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
        let registry = BlockRegistry::new("test_blocks.json");
        let manager = PipelineManager::new(&flow, events, &registry).unwrap();

        // Should have only 2 original elements, no tee
        assert_eq!(manager.elements.len(), 2);
        assert!(!manager.elements.contains_key("auto_tee_src"));
    }
}
