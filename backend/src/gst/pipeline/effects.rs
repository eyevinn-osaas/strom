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

    /// Reset accumulated loudness measurements on an EBU R128 meter block.
    pub fn reset_loudness(&self, block_instance_id: &str) -> Result<(), PipelineError> {
        let element_id = format!("{}:ebur128level", block_instance_id);
        let element = self
            .elements
            .get(&element_id)
            .ok_or_else(|| PipelineError::ElementNotFound(element_id.clone()))?;
        element.emit_by_name::<()>("reset", &[]);
        info!("Reset loudness measurements on {}", block_instance_id);
        Ok(())
    }

    /// Force an immediate file split on a recorder block.
    ///
    /// Emits the `split-now` signal on the splitmuxsink element, which triggers
    /// a file split at the next keyframe boundary.
    pub fn recorder_split_now(&self, block_instance_id: &str) -> Result<(), PipelineError> {
        use crate::blocks::builtin::recorder::SPLITMUXSINK_SUFFIX;
        let element_id = format!("{}:{}", block_instance_id, SPLITMUXSINK_SUFFIX);
        let element = self.elements.get(&element_id).ok_or_else(|| {
            PipelineError::ElementNotFound(format!(
                "{} (is this a recorder block in ts_passthrough mode?)",
                element_id
            ))
        })?;
        element.emit_by_name::<()>("split-now", &[]);
        info!(
            "Triggered split-now on recorder block {}",
            block_instance_id
        );
        Ok(())
    }

    /// Capture a thumbnail from a compositor input.
    ///
    /// Uses ThumbnailTap to lazily attach a GStreamer-native processing branch
    /// to the compositor input's tee element. The branch does format conversion
    /// and scaling using GStreamer elements, with lightweight JPEG encoding
    /// in the appsink callback.
    ///
    /// # Arguments
    /// * `block_id` - The compositor block instance ID (e.g., "b0")
    /// * `input_idx` - The input index (0-based)
    /// * `width` - Target thumbnail width (currently unused — tap uses fixed 160x90)
    /// * `height` - Target thumbnail height (currently unused — tap uses fixed 160x90)
    ///
    /// # Returns
    /// JPEG-encoded image bytes on success
    pub fn capture_compositor_input_thumbnail(
        &self,
        block_id: &str,
        input_idx: usize,
        _width: u32,
        _height: u32,
    ) -> Result<Vec<u8>, PipelineError> {
        use crate::gst::thumbnail_tap::{ThumbnailTap, ThumbnailTapConfig};

        let mut taps = self.thumbnail_taps.lock().unwrap();
        let block_taps = taps.entry(block_id.to_string()).or_default();

        // Ensure we have a tap for this input index (lazy creation)
        while block_taps.len() <= input_idx {
            let idx = block_taps.len();
            let tee_name = format!("{}:thumb_tee_{}", block_id, idx);
            let tee = self.pipeline.by_name(&tee_name).ok_or_else(|| {
                PipelineError::ElementNotFound(format!(
                    "Thumbnail tee not found: {} (is this a compositor block?)",
                    tee_name
                ))
            })?;

            let name_prefix = format!("{}:input_{}", block_id, idx);
            let tap = ThumbnailTap::new_with_tee(
                &self.pipeline,
                &name_prefix,
                tee,
                ThumbnailTapConfig::default(),
            );
            block_taps.push(tap);
        }

        let tap = &block_taps[input_idx];
        tap.get_thumbnail()
            .map_err(|e| PipelineError::ThumbnailCapture(e.to_string()))
    }

    /// Capture a thumbnail from a block's tee element.
    ///
    /// Works with the `builtin.thumbnail` block (which contains a single tee
    /// named `{block_id}:tee`) and any other block that exposes a thumbnail tee.
    pub fn get_block_thumbnail(&self, block_id: &str) -> Result<Vec<u8>, PipelineError> {
        use crate::gst::thumbnail_tap::{ThumbnailTap, ThumbnailTapConfig};

        let mut taps = self.thumbnail_taps.lock().unwrap();
        let block_taps = taps.entry(block_id.to_string()).or_default();

        // Lazy creation: the thumbnail block has a single tee at index 0
        if block_taps.is_empty() {
            let tee_name = format!("{}:tee", block_id);
            let tee = self.pipeline.by_name(&tee_name).ok_or_else(|| {
                PipelineError::ElementNotFound(format!(
                    "Thumbnail tee not found: {} (is this a thumbnail block?)",
                    tee_name
                ))
            })?;

            let name_prefix = format!("{}:thumb", block_id);
            let tap = ThumbnailTap::new_with_tee(
                &self.pipeline,
                &name_prefix,
                tee,
                ThumbnailTapConfig::default(),
            );
            block_taps.push(tap);
        }

        block_taps[0]
            .get_thumbnail()
            .map_err(|e| PipelineError::ThumbnailCapture(e.to_string()))
    }
}
