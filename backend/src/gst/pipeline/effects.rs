use super::{PipelineError, PipelineManager};
use gstreamer as gst;
use gstreamer::prelude::*;
use tracing::info;

impl PipelineManager {
    /// Trigger a transition on a compositor/mixer block.
    ///
    /// # Arguments
    /// * `block_instance_id` - The instance ID of the compositor block (e.g., "comp_1").
    /// * `from_input` - Index of the currently active input.
    /// * `to_input` - Index of the input to transition to.
    /// * `transition_type` - Type of transition ("fade", "cut", "slide_left", etc.).
    /// * `duration_ms` - Duration of the transition in milliseconds.
    pub fn trigger_transition(
        &self,
        block_instance_id: &str,
        from_input: usize,
        to_input: usize,
        transition_type: &str,
        duration_ms: u64,
    ) -> Result<(), PipelineError> {
        use crate::gst::transitions::{TransitionController, TransitionType};

        info!(
            "Triggering {} transition on {} from input {} to {} ({}ms)",
            transition_type, block_instance_id, from_input, to_input, duration_ms
        );

        // Find the mixer element for this block
        let mixer_id = format!("{}:mixer", block_instance_id);
        let mixer = self
            .elements
            .get(&mixer_id)
            .ok_or_else(|| PipelineError::ElementNotFound(mixer_id.clone()))?;

        // Parse transition type
        let trans_type = transition_type.parse::<TransitionType>().map_err(|_| {
            PipelineError::InvalidProperty {
                element: block_instance_id.to_string(),
                property: "transition_type".to_string(),
                reason: format!("Unknown transition type: {}", transition_type),
            }
        })?;

        // Get canvas dimensions from the mixer's output caps or use defaults
        // We'll try to get them from the capsfilter
        let capsfilter_id = format!("{}:capsfilter", block_instance_id);
        let (canvas_width, canvas_height) =
            if let Some(capsfilter) = self.elements.get(&capsfilter_id) {
                // Try to get dimensions from caps
                if let Some(caps) = capsfilter.property::<Option<gst::Caps>>("caps") {
                    if let Some(structure) = caps.structure(0) {
                        let width = structure.get::<i32>("width").unwrap_or(1920);
                        let height = structure.get::<i32>("height").unwrap_or(1080);
                        (width, height)
                    } else {
                        (1920, 1080)
                    }
                } else {
                    (1920, 1080)
                }
            } else {
                (1920, 1080)
            };

        // Create transition controller and execute transition
        let controller = TransitionController::new(mixer.clone(), canvas_width, canvas_height);
        controller
            .transition(
                from_input,
                to_input,
                trans_type,
                duration_ms,
                &self.pipeline,
            )
            .map_err(|e| PipelineError::TransitionError(e.to_string()))?;

        Ok(())
    }

    /// Animate a single input's position/size on a compositor block.
    #[allow(clippy::too_many_arguments)]
    pub fn animate_input(
        &self,
        block_instance_id: &str,
        input_index: usize,
        target_xpos: Option<i32>,
        target_ypos: Option<i32>,
        target_width: Option<i32>,
        target_height: Option<i32>,
        duration_ms: u64,
    ) -> Result<(), PipelineError> {
        use crate::gst::transitions::TransitionController;

        info!(
            "Animating input {} on {} to ({:?}, {:?}, {:?}, {:?}) over {}ms",
            input_index,
            block_instance_id,
            target_xpos,
            target_ypos,
            target_width,
            target_height,
            duration_ms
        );

        // Find the mixer element for this block
        let mixer_id = format!("{}:mixer", block_instance_id);
        let mixer = self
            .elements
            .get(&mixer_id)
            .ok_or_else(|| PipelineError::ElementNotFound(mixer_id.clone()))?;

        // Create transition controller and animate
        let controller = TransitionController::new(mixer.clone(), 1920, 1080);
        controller
            .animate_input(
                input_index,
                target_xpos,
                target_ypos,
                target_width,
                target_height,
                duration_ms,
                &self.pipeline,
            )
            .map_err(|e| PipelineError::TransitionError(e.to_string()))?;

        Ok(())
    }

    /// Capture a thumbnail from a compositor input.
    ///
    /// Captures a single frame from the queue element feeding the specified
    /// compositor input, scales it to the specified dimensions, and encodes
    /// it as JPEG.
    ///
    /// # Arguments
    /// * `block_id` - The compositor block instance ID (e.g., "b0")
    /// * `input_idx` - The input index (0-based)
    /// * `width` - Target thumbnail width
    /// * `height` - Target thumbnail height
    ///
    /// # Returns
    /// JPEG-encoded image bytes on success
    pub fn capture_compositor_input_thumbnail(
        &self,
        block_id: &str,
        input_idx: usize,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, PipelineError> {
        // The queue element is named "{block_id}:queue_{input_idx}"
        let element_name = format!("{}:queue_{}", block_id, input_idx);

        let config = crate::gst::ThumbnailConfig {
            width,
            height,
            quality: crate::gst::thumbnail::DEFAULT_JPEG_QUALITY,
        };

        crate::gst::capture_frame_as_jpeg(&self.pipeline, &element_name, "src", &config)
            .map_err(|e| PipelineError::ThumbnailCapture(e.to_string()))
    }
}
