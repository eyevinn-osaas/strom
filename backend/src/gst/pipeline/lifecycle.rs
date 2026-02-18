use super::{PipelineError, PipelineManager};
use crate::gst::thread_priority;
use gstreamer as gst;
use gstreamer::prelude::*;
use strom_types::PipelineState;
use tracing::{error, info};

impl PipelineManager {
    /// Start the pipeline (set to PLAYING state).
    pub fn start(&mut self) -> Result<PipelineState, PipelineError> {
        info!("Starting pipeline: {}", self.flow_name);
        info!("Pipeline has {} elements", self.elements.len());

        // Set up thread priority handler FIRST (before any state changes)
        // This must be done before the pipeline starts so we catch all thread enter events
        info!(
            "Setting up thread priority handler (requested: {:?}, registry: {})...",
            self.properties.thread_priority,
            self.thread_registry.is_some()
        );
        let priority_state = thread_priority::setup_thread_priority_handler(
            &self.pipeline,
            self.properties.thread_priority,
            self.flow_id,
            self.thread_registry.clone(),
        );
        self.thread_priority_state = Some(priority_state);
        info!("Thread priority handler installed");

        // Set up bus watch before starting
        info!("Setting up bus watch...");
        self.setup_bus_watch();
        info!("Bus watch set up");

        // Start QoS aggregation and periodic broadcast task
        info!("Starting QoS stats aggregation task...");
        self.start_qos_broadcast_task();
        info!("QoS stats task started");

        // Configure clock before starting
        info!(
            "Configuring clock (type: {:?})...",
            self.properties.clock_type
        );
        self.configure_clock()?;
        info!("Clock configured");

        // Set to READY state first to ensure aggregator request pads are fully initialized
        info!("Setting pipeline '{}' to READY state...", self.flow_name);
        self.pipeline
            .set_state(gst::State::Ready)
            .map_err(|e| PipelineError::StateChange(format!("Failed to reach READY: {}", e)))?;
        info!("Pipeline in READY state");

        // Now apply pad properties (aggregator request pads are now accessible)
        info!("Applying pad properties after READY state...");
        self.apply_pad_properties();
        info!("Pad properties applied");

        info!(
            "Setting pipeline '{}' to PLAYING state (this may block)...",
            self.flow_name
        );
        let state_change_result = self.pipeline.set_state(gst::State::Playing);
        info!("set_state(Playing) call returned");

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

        let state_change_success = state_change_result
            .map_err(|e| PipelineError::StateChange(format!("Failed to start: {}", e)))?;

        // For async state changes (like SRT sink), don't query state immediately
        // The state will change asynchronously and we'll get state-changed messages on the bus
        // Also treat NoPreroll (live sources) as async since they transition on their own timeline
        if matches!(
            state_change_success,
            gst::StateChangeSuccess::Async | gst::StateChangeSuccess::NoPreroll
        ) {
            info!(
                "Pipeline '{}' state change is async/live, skipping immediate state query to avoid race conditions",
                self.flow_name
            );
            // Update cached state - the bus watch will update it when the actual transition happens
            *self.cached_state.write().unwrap() = PipelineState::Playing;
            return Ok(PipelineState::Playing);
        }

        // For synchronous state changes, verify the state was reached
        info!("Querying pipeline state to verify synchronous state change...");
        let (result, current_state, pending_state) =
            self.pipeline.state(gst::ClockTime::from_mseconds(500));
        info!(
            "Pipeline '{}' state after start: result={:?}, current={:?}, pending={:?}",
            self.flow_name, result, current_state, pending_state
        );

        // Check if we've reached the target state
        // If current_state is Playing and pending is VoidPending, that's success!
        let target_reached =
            current_state == gst::State::Playing && pending_state == gst::State::VoidPending;

        if !target_reached {
            // Only fail if we haven't reached the target state
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
        } else {
            info!(
                "Pipeline '{}' successfully reached PLAYING state",
                self.flow_name
            );
        }

        // Return the actual current state
        let actual_state = match current_state {
            gst::State::Null => PipelineState::Null,
            gst::State::Ready => PipelineState::Ready,
            gst::State::Paused => PipelineState::Paused,
            gst::State::Playing => PipelineState::Playing,
            _ => PipelineState::Null,
        };

        // Update cached state
        *self.cached_state.write().unwrap() = actual_state;

        Ok(actual_state)
    }

    /// Stop the pipeline (set to NULL state).
    pub fn stop(&mut self) -> Result<PipelineState, PipelineError> {
        info!("Stopping pipeline: {}", self.flow_name);

        // Run set_state on a dedicated OS thread to avoid "Cannot start a runtime
        // from within a runtime" panics. Some GStreamer elements (e.g. whipserversrc)
        // internally call block_on() during state transitions, which is incompatible
        // with being called from within a tokio runtime context.
        let pipeline = self.pipeline.clone();
        let result = std::thread::spawn(move || pipeline.set_state(gst::State::Null))
            .join()
            .map_err(|_| PipelineError::StateChange("set_state thread panicked".to_string()))?
            .map_err(|e| PipelineError::StateChange(format!("Failed to stop: {}", e)))?;
        let _ = result;

        // Remove bus watch when stopped to free resources
        self.remove_bus_watch();

        // Stop QoS broadcast task
        self.stop_qos_broadcast_task();

        // Remove thread priority handler
        thread_priority::remove_thread_priority_handler(&self.pipeline);
        self.thread_priority_state = None;

        // Unregister all threads belonging to this flow from the registry
        if let Some(ref registry) = self.thread_registry {
            registry.unregister_flow(&self.flow_id);
        }

        // Update cached state
        *self.cached_state.write().unwrap() = PipelineState::Null;

        Ok(PipelineState::Null)
    }

    /// Pause the pipeline.
    pub fn pause(&self) -> Result<PipelineState, PipelineError> {
        info!("Pausing pipeline: {}", self.flow_name);

        self.pipeline
            .set_state(gst::State::Paused)
            .map_err(|e| PipelineError::StateChange(format!("Failed to pause: {}", e)))?;

        // Update cached state
        *self.cached_state.write().unwrap() = PipelineState::Paused;

        Ok(PipelineState::Paused)
    }
}
