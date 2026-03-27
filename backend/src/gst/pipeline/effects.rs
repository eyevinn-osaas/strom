use super::{PipelineError, PipelineManager};
use gstreamer as gst;
use gstreamer::prelude::*;
use tracing::{debug, info};

impl PipelineManager {
    /// Get the distribution compositor canvas size from its capsfilter.
    fn dist_canvas_size(&self, block_instance_id: &str) -> (i32, i32) {
        let default =
            strom_types::parse_resolution_string(strom_types::vision_mixer::DEFAULT_PGM_RESOLUTION)
                .map(|(w, h)| (w as i32, h as i32))
                .expect("DEFAULT_PGM_RESOLUTION must be valid");

        let capsfilter_id = format!("{}:capsfilter_dist", block_instance_id);
        self.elements
            .get(&capsfilter_id)
            .and_then(|cf| cf.property::<Option<gst::Caps>>("caps"))
            .and_then(|caps| {
                let s = caps.structure(0)?;
                Some((
                    s.get::<i32>("width").unwrap_or(default.0),
                    s.get::<i32>("height").unwrap_or(default.1),
                ))
            })
            .unwrap_or(default)
    }

    /// Trigger a transition on a compositor/mixer block.
    ///
    /// Uses the server's authoritative PGM/PVW groups from overlay state.
    /// For single-source groups, uses standard single-pad transitions.
    /// For multi-source groups, cross-fades between group layouts.
    ///
    /// Returns (was_ftb_cancelled, old_pgm_group, new_pgm_group).
    pub fn trigger_transition(
        &self,
        block_instance_id: &str,
        from_input: usize,
        to_input: usize,
        transition_type: &str,
        duration_ms: u64,
    ) -> Result<(bool, Vec<usize>, Vec<usize>), PipelineError> {
        use crate::gst::transitions::{TransitionController, TransitionType};

        debug!(
            "Triggering {} transition on {} from input {} to {} ({}ms)",
            transition_type, block_instance_id, from_input, to_input, duration_ms
        );

        // Find the mixer element for this block
        let mixer_id = format!("{}:mixer", block_instance_id);
        let mixer = self
            .elements
            .get(&mixer_id)
            .ok_or_else(|| PipelineError::ElementNotFound(mixer_id.clone()))?;

        // Read authoritative PGM/PVW groups from overlay state
        let overlay_state =
            crate::blocks::builtin::vision_mixer::overlay::get_overlay_state(block_instance_id);
        let num_video_inputs = overlay_state
            .as_ref()
            .map(|s| s.num_inputs)
            .unwrap_or(usize::MAX);
        let old_pgm_group = overlay_state
            .as_ref()
            .map(|s| s.pgm_group())
            .unwrap_or_else(|| vec![from_input]);
        let new_pgm_group = overlay_state
            .as_ref()
            .map(|s| s.pvw_group())
            .unwrap_or_else(|| vec![to_input]);

        // Auto-cancel FTB if active
        let was_ftb = overlay_state
            .as_ref()
            .map(|s| {
                s.ftb_active
                    .swap(false, std::sync::atomic::Ordering::Relaxed)
            })
            .unwrap_or(false);
        if was_ftb {
            info!(
                "Auto-cancelling FTB before transition on {}",
                block_instance_id
            );
        }

        let (canvas_width, canvas_height) = self.dist_canvas_size(block_instance_id);

        // Reset all video pads to a clean state before the transition:
        // clear control bindings, restore alpha/position/size for current PGM group.
        // Background pad stays fullscreen at low z-order.
        let pgm_rects = strom_types::vision_mixer::compute_group_rects(
            0,
            0,
            canvas_width,
            canvas_height,
            old_pgm_group.len(),
        );
        let bg_input = overlay_state.as_ref().and_then(|s| s.background_input());

        for pad in mixer.sink_pads() {
            let name = pad.name();
            if name.starts_with("sink_") {
                if let Ok(idx) = name.trim_start_matches("sink_").parse::<usize>() {
                    for prop in ["alpha", "xpos", "ypos", "width", "height"] {
                        if let Some(binding) = pad.control_binding(prop) {
                            pad.remove_control_binding(&binding);
                        }
                    }
                    if idx < num_video_inputs {
                        if let Some(slot) = old_pgm_group.iter().position(|&x| x == idx) {
                            // This pad is in the current PGM group — restore its position
                            let (x, y, w, h) = pgm_rects.get(slot).copied().unwrap_or((
                                0,
                                0,
                                canvas_width,
                                canvas_height,
                            ));
                            pad.set_property("alpha", 1.0f64);
                            pad.set_property("xpos", x);
                            pad.set_property("ypos", y);
                            pad.set_property("width", w);
                            pad.set_property("height", h);
                            pad.set_property("zorder", strom_types::vision_mixer::DIST_PGM_ZORDER);
                        } else if bg_input == Some(idx) {
                            // Background source: fullscreen, low z-order
                            pad.set_property("alpha", 1.0f64);
                            pad.set_property("xpos", 0i32);
                            pad.set_property("ypos", 0i32);
                            pad.set_property("width", canvas_width);
                            pad.set_property("height", canvas_height);
                            pad.set_property(
                                "zorder",
                                strom_types::vision_mixer::DIST_BACKGROUND_ZORDER,
                            );
                        } else {
                            pad.set_property("alpha", 0.0f64);
                            pad.set_property("xpos", 0i32);
                            pad.set_property("ypos", 0i32);
                            pad.set_property("width", canvas_width);
                            pad.set_property("height", canvas_height);
                        }
                    } else if let Some(state) = overlay_state.as_ref() {
                        let dsk_idx = idx - num_video_inputs;
                        let enabled = dsk_idx < state.dsk_enabled.len()
                            && state.dsk_enabled[dsk_idx]
                                .load(std::sync::atomic::Ordering::Relaxed);
                        let alpha = if enabled { 1.0f64 } else { 0.0f64 };
                        pad.set_property("alpha", alpha);
                    }
                }
            }
        }

        // Parse transition type
        let trans_type = transition_type.parse::<TransitionType>().map_err(|_| {
            PipelineError::InvalidProperty {
                element: block_instance_id.to_string(),
                property: "transition_type".to_string(),
                reason: format!("Unknown transition type: {}", transition_type),
            }
        })?;

        // Check if this is a single-to-single transition (use existing optimized path)
        let single_to_single = old_pgm_group.len() == 1 && new_pgm_group.len() == 1;

        if single_to_single {
            let from = old_pgm_group[0];
            let to = new_pgm_group[0];
            let controller = TransitionController::new(mixer.clone(), canvas_width, canvas_height);
            controller
                .transition(from, to, trans_type, duration_ms, &self.pipeline)
                .map_err(|e| PipelineError::TransitionError(e.to_string()))?;
        } else {
            // Group transition: position incoming pads at target sub-rects, then cross-fade
            let new_rects = strom_types::vision_mixer::compute_group_rects(
                0,
                0,
                canvas_width,
                canvas_height,
                new_pgm_group.len(),
            );

            // Set incoming pad positions (invisible at alpha=0, above background)
            for (slot, &idx) in new_pgm_group.iter().enumerate() {
                if let Some(pad) = mixer.static_pad(&format!("sink_{}", idx)) {
                    let (x, y, w, h) =
                        new_rects
                            .get(slot)
                            .copied()
                            .unwrap_or((0, 0, canvas_width, canvas_height));
                    pad.set_property("xpos", x);
                    pad.set_property("ypos", y);
                    pad.set_property("width", w);
                    pad.set_property("height", h);
                    pad.set_property("alpha", 0.0f64);
                    pad.set_property("zorder", strom_types::vision_mixer::DIST_PGM_ZORDER);
                }
            }

            // For group transitions, always use fade (slides don't make sense)
            let effective_type = if matches!(trans_type, TransitionType::Cut) {
                TransitionType::Cut
            } else {
                TransitionType::Fade
            };

            if effective_type == TransitionType::Cut {
                // Instant: hide old, show new
                for &idx in &old_pgm_group {
                    if let Some(pad) = mixer.static_pad(&format!("sink_{}", idx)) {
                        pad.set_property("alpha", 0.0f64);
                    }
                }
                for &idx in &new_pgm_group {
                    if let Some(pad) = mixer.static_pad(&format!("sink_{}", idx)) {
                        pad.set_property("alpha", 1.0f64);
                    }
                }
            } else {
                // Fade: cross-fade all outgoing/incoming pads
                let controller =
                    TransitionController::new(mixer.clone(), canvas_width, canvas_height);
                controller
                    .transition_groups(&old_pgm_group, &new_pgm_group, duration_ms, &self.pipeline)
                    .map_err(|e| PipelineError::TransitionError(e.to_string()))?;
            }
        }

        Ok((was_ftb, old_pgm_group, new_pgm_group))
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

        let (canvas_width, canvas_height) = self.dist_canvas_size(block_instance_id);

        // Create transition controller and animate
        let controller = TransitionController::new(mixer.clone(), canvas_width, canvas_height);
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

    /// Capture a thumbnail from a block's tee element at the given index.
    ///
    /// Lazily attaches a GStreamer-native processing branch to the block's tee
    /// element. The branch does format conversion and scaling using GStreamer
    /// elements, with lightweight JPEG encoding in the appsink callback.
    ///
    /// The meaning of `index` depends on the block type:
    /// - **Compositor**: input index (each input has its own tee named `{block_id}:thumb_tee_{index}`)
    /// - **Thumbnail block**: always 0 (single tee named `{block_id}:tee`)
    pub fn capture_block_thumbnail(
        &self,
        block_id: &str,
        index: usize,
    ) -> Result<Vec<u8>, PipelineError> {
        use crate::gst::thumbnail_tap::{ThumbnailTap, ThumbnailTapConfig};

        let mut taps = self.thumbnail_taps.lock().unwrap();
        let block_taps = taps.entry(block_id.to_string()).or_default();

        // Ensure we have a tap for this index (lazy creation)
        while block_taps.len() <= index {
            let idx = block_taps.len();
            // Try compositor naming first ({block_id}:thumb_tee_{idx}),
            // fall back to simple naming ({block_id}:tee) for index 0.
            let tee_name = format!("{}:thumb_tee_{}", block_id, idx);
            let tee = self
                .pipeline
                .by_name(&tee_name)
                .or_else(|| {
                    if idx == 0 {
                        self.pipeline.by_name(&format!("{}:tee", block_id))
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    PipelineError::ElementNotFound(format!(
                        "Thumbnail tee not found: {} (block {})",
                        tee_name, block_id
                    ))
                })?;

            let name_prefix = format!("{}:thumb_{}", block_id, idx);
            let tap = ThumbnailTap::new_with_tee(
                &self.pipeline,
                &name_prefix,
                tee,
                ThumbnailTapConfig::default(),
            );
            block_taps.push(tap);
        }

        block_taps[index]
            .get_thumbnail()
            .map_err(|e| PipelineError::ThumbnailCapture(e.to_string()))
    }

    /// Select a preview input on a vision mixer block.
    ///
    /// If `multi` is false, replaces the PVW group with a single source (standard behavior).
    /// If `multi` is true, toggles the input in/out of the current PVW group (shift+click).
    ///
    /// Returns (pvw_group, pgm_group).
    pub fn select_vision_mixer_preview(
        &self,
        block_instance_id: &str,
        input: usize,
        num_inputs: usize,
        multi: bool,
    ) -> Result<(Vec<usize>, Vec<usize>), PipelineError> {
        use crate::blocks::builtin::vision_mixer::{layout, overlay};

        let mv_comp_id = format!("{}:mv_comp", block_instance_id);
        let mv_comp = self
            .elements
            .get(&mv_comp_id)
            .ok_or_else(|| PipelineError::ElementNotFound(mv_comp_id.clone()))?;

        let state = overlay::get_overlay_state(block_instance_id).ok_or_else(|| {
            PipelineError::ElementNotFound(format!(
                "Vision mixer overlay state not found for {}",
                block_instance_id
            ))
        })?;

        let old_pvw_group = state.pvw_group();
        let pgm_group = state.pgm_group();

        if input >= num_inputs {
            return Err(PipelineError::InvalidProperty {
                element: block_instance_id.to_string(),
                property: "preview_input".to_string(),
                reason: format!("Input {} out of range (max {})", input, num_inputs - 1),
            });
        }

        // Background source is exclusive — can't be used in PVW/PGM groups
        if state.background_input() == Some(input) {
            return Err(PipelineError::InvalidProperty {
                element: block_instance_id.to_string(),
                property: "preview_input".to_string(),
                reason: format!("Input {} is the background source", input),
            });
        }

        // Compute new PVW group
        let new_pvw_group = if multi {
            // Toggle mode: add/remove the input from the PVW group
            let mut group = old_pvw_group.clone();
            if let Some(pos) = group.iter().position(|&x| x == input) {
                // Remove if present (unless it's the last one)
                if group.len() > 1 {
                    group.remove(pos);
                } else {
                    return Ok((old_pvw_group, pgm_group));
                }
            } else if group.len() < strom_types::vision_mixer::MAX_GROUP_SIZE {
                group.push(input);
            }
            group
        } else {
            // Single select: replace entire PVW group
            // Only block if this input IS the entire PGM (single-source PGM).
            // If PGM is multi-source, previewing one of its members is fine.
            if pgm_group.len() == 1 && pgm_group[0] == input {
                return Err(PipelineError::InvalidProperty {
                    element: block_instance_id.to_string(),
                    property: "preview_input".to_string(),
                    reason: format!("Input {} is already the sole program source", input),
                });
            }
            vec![input]
        };

        // Update multiview PVW candidate pads
        // First hide all old PVW pads (and old background PVW pad)
        for &old_idx in &old_pvw_group {
            if !pgm_group.contains(&old_idx) {
                if let Some(pad) = find_pad(mv_comp, &format!("sink_{}", num_inputs + 1 + old_idx))
                {
                    pad.set_property("alpha", 0.0f64);
                }
            }
        }

        let bg = state.background_input();

        // If there's a background, show it fullscreen in the PVW area at lower z-order
        if let Some(bg_idx) = bg {
            if !new_pvw_group.contains(&bg_idx) {
                if let Some(pad) = find_pad(mv_comp, &format!("sink_{}", num_inputs + 1 + bg_idx)) {
                    let r = &state.layout.pvw_rect;
                    pad.set_property("xpos", r.x as i32);
                    pad.set_property("ypos", r.y as i32);
                    pad.set_property("width", r.w as i32);
                    pad.set_property("height", r.h as i32);
                    pad.set_property("alpha", 1.0f64);
                    pad.set_property(
                        "zorder",
                        strom_types::vision_mixer::MV_BIG_DISPLAY_ZORDER - 1,
                    );
                }
            }
        }

        // Position new PVW pads in sub-rects of the PVW area (above background)
        let sub_rects =
            layout::compute_group_sub_rects(&state.layout.pvw_rect, new_pvw_group.len());
        for (slot, &idx) in new_pvw_group.iter().enumerate() {
            if let Some(pad) = find_pad(mv_comp, &format!("sink_{}", num_inputs + 1 + idx)) {
                if let Some(r) = sub_rects.get(slot) {
                    pad.set_property("xpos", r.x as i32);
                    pad.set_property("ypos", r.y as i32);
                    pad.set_property("width", r.w as i32);
                    pad.set_property("height", r.h as i32);
                    pad.set_property("alpha", 1.0f64);
                    pad.set_property("zorder", strom_types::vision_mixer::MV_BIG_DISPLAY_ZORDER);
                }
            }
        }

        state.set_pvw_group(&new_pvw_group);
        overlay::trigger_overlay_update(block_instance_id);

        info!(
            "Vision mixer {} preview changed: {:?} -> {:?}",
            block_instance_id, old_pvw_group, new_pvw_group
        );

        Ok((new_pvw_group, pgm_group))
    }

    /// Update the multiview compositor after a PGM transition on a vision mixer.
    ///
    /// Swaps PGM and PVW groups: old PVW group becomes new PGM, old PGM group becomes new PVW.
    pub fn update_vision_mixer_after_take(
        &self,
        block_instance_id: &str,
        new_pgm_group: &[usize],
        new_pvw_group: &[usize],
        num_inputs: usize,
    ) -> Result<(), PipelineError> {
        use crate::blocks::builtin::vision_mixer::{layout, overlay};

        let mv_comp_id = format!("{}:mv_comp", block_instance_id);
        let mv_comp = self
            .elements
            .get(&mv_comp_id)
            .ok_or_else(|| PipelineError::ElementNotFound(mv_comp_id.clone()))?;

        let state = overlay::get_overlay_state(block_instance_id).ok_or_else(|| {
            PipelineError::ElementNotFound(format!(
                "Vision mixer overlay state not found for {}",
                block_instance_id
            ))
        })?;

        // PGM big display (sink_N) is fed from tee_pgm — it always shows the dist_comp
        // output automatically, so no pad manipulation needed for PGM.
        // Only update PVW: hide all old PVW pads, show new PVW group pads.

        // Hide all PVW candidate pads first
        for i in 0..num_inputs {
            if let Some(pad) = find_pad(mv_comp, &format!("sink_{}", num_inputs + 1 + i)) {
                pad.set_property("alpha", 0.0f64);
            }
        }

        let bg = state.background_input();

        // If there's a background, show it fullscreen in the PVW area at lower z-order
        if let Some(bg_idx) = bg {
            if !new_pvw_group.contains(&bg_idx) {
                if let Some(pad) = find_pad(mv_comp, &format!("sink_{}", num_inputs + 1 + bg_idx)) {
                    let r = &state.layout.pvw_rect;
                    pad.set_property("xpos", r.x as i32);
                    pad.set_property("ypos", r.y as i32);
                    pad.set_property("width", r.w as i32);
                    pad.set_property("height", r.h as i32);
                    pad.set_property("alpha", 1.0f64);
                    pad.set_property(
                        "zorder",
                        strom_types::vision_mixer::MV_BIG_DISPLAY_ZORDER - 1,
                    );
                }
            }
        }

        // Position new PVW group in sub-rects of the PVW area (above background)
        let sub_rects =
            layout::compute_group_sub_rects(&state.layout.pvw_rect, new_pvw_group.len());
        for (slot, &idx) in new_pvw_group.iter().enumerate() {
            if let Some(pad) = find_pad(mv_comp, &format!("sink_{}", num_inputs + 1 + idx)) {
                if let Some(r) = sub_rects.get(slot) {
                    pad.set_property("xpos", r.x as i32);
                    pad.set_property("ypos", r.y as i32);
                    pad.set_property("width", r.w as i32);
                    pad.set_property("height", r.h as i32);
                    pad.set_property("alpha", 1.0f64);
                    pad.set_property("zorder", strom_types::vision_mixer::MV_BIG_DISPLAY_ZORDER);
                }
            }
        }

        // Update state
        state.set_pgm_group(new_pgm_group);
        state.set_pvw_group(new_pvw_group);

        overlay::trigger_overlay_update(block_instance_id);

        info!(
            "Vision mixer {} take: PGM -> {:?}, PVW -> {:?}",
            block_instance_id, new_pgm_group, new_pvw_group
        );

        Ok(())
    }

    /// Set or clear the background source on a vision mixer block.
    ///
    /// The background source is placed fullscreen at a low z-order on the distribution
    /// compositor, behind the PGM group sources. Visible through gaps in split-screen layouts.
    pub fn set_vision_mixer_background(
        &self,
        block_instance_id: &str,
        input: Option<usize>,
    ) -> Result<(), PipelineError> {
        use crate::blocks::builtin::vision_mixer::overlay;
        use strom_types::vision_mixer;

        let mixer_id = format!("{}:mixer", block_instance_id);
        let mixer = self
            .elements
            .get(&mixer_id)
            .ok_or_else(|| PipelineError::ElementNotFound(mixer_id.clone()))?;

        let state = overlay::get_overlay_state(block_instance_id).ok_or_else(|| {
            PipelineError::ElementNotFound(format!(
                "Vision mixer overlay state not found for {}",
                block_instance_id
            ))
        })?;

        let (canvas_width, canvas_height) = self.dist_canvas_size(block_instance_id);

        // Clear old background pad (if any)
        if let Some(old_bg) = state.background_input() {
            if let Some(pad) = mixer.static_pad(&format!("sink_{}", old_bg)) {
                pad.set_property("alpha", 0.0f64);
            }
        }

        // Set new background pad
        if let Some(bg_idx) = input {
            if bg_idx >= state.num_inputs {
                return Err(PipelineError::InvalidProperty {
                    element: block_instance_id.to_string(),
                    property: "background_input".to_string(),
                    reason: format!(
                        "Input {} out of range (max {})",
                        bg_idx,
                        state.num_inputs - 1
                    ),
                });
            }
            // Background is exclusive — can't use a source that's in PGM or PVW
            let pgm_group = state.pgm_group();
            let pvw_group = state.pvw_group();
            if pgm_group.contains(&bg_idx) || pvw_group.contains(&bg_idx) {
                return Err(PipelineError::InvalidProperty {
                    element: block_instance_id.to_string(),
                    property: "background_input".to_string(),
                    reason: format!("Input {} is in use as a PGM/PVW source", bg_idx),
                });
            }
            if let Some(pad) = mixer.static_pad(&format!("sink_{}", bg_idx)) {
                pad.set_property("xpos", 0i32);
                pad.set_property("ypos", 0i32);
                pad.set_property("width", canvas_width);
                pad.set_property("height", canvas_height);
                pad.set_property("zorder", vision_mixer::DIST_BACKGROUND_ZORDER);
                pad.set_property("alpha", 1.0f64);
            }
        }

        state.set_background_input(input);
        overlay::trigger_overlay_update(block_instance_id);

        info!("Vision mixer {} background: {:?}", block_instance_id, input);

        Ok(())
    }

    /// Toggle a DSK (Downstream Keyer) layer on or off.
    pub fn set_dsk_enabled(
        &self,
        block_instance_id: &str,
        dsk_index: usize,
        num_inputs: usize,
        enabled: bool,
    ) -> Result<(), PipelineError> {
        // DSK pads are on the dist compositor (mixer) at sink_{num_inputs + dsk_index}
        let mixer_id = format!("{}:mixer", block_instance_id);
        let mixer = self
            .elements
            .get(&mixer_id)
            .ok_or_else(|| PipelineError::ElementNotFound(mixer_id.clone()))?;

        let pad_name = format!("sink_{}", num_inputs + dsk_index);
        if let Some(pad) = find_pad(mixer, &pad_name) {
            let alpha = if enabled { 1.0f64 } else { 0.0f64 };
            pad.set_property("alpha", alpha);
            // Update overlay state for DSK tracking
            if let Some(state) =
                crate::blocks::builtin::vision_mixer::overlay::get_overlay_state(block_instance_id)
            {
                if dsk_index < state.dsk_enabled.len() {
                    state.dsk_enabled[dsk_index]
                        .store(enabled, std::sync::atomic::Ordering::Relaxed);
                }
            }
            info!(
                "Vision mixer {} DSK {} {}",
                block_instance_id,
                dsk_index,
                if enabled { "enabled" } else { "disabled" }
            );
            Ok(())
        } else {
            Err(PipelineError::PadNotFound {
                element: mixer_id,
                pad: pad_name,
            })
        }
    }

    /// Toggle Fade to Black on a vision mixer block.
    ///
    /// Animates ALL mixer sink pads alpha to 0 (fade out) or restores them (fade in).
    /// Returns the new FTB state (true = active/black).
    pub fn fade_to_black(
        &self,
        block_instance_id: &str,
        duration_ms: u64,
    ) -> Result<bool, PipelineError> {
        use crate::blocks::builtin::vision_mixer::overlay;
        use gstreamer_controller::prelude::*;
        use gstreamer_controller::{
            DirectControlBinding, InterpolationControlSource, InterpolationMode,
        };

        let mixer_id = format!("{}:mixer", block_instance_id);
        let mixer = self
            .elements
            .get(&mixer_id)
            .ok_or_else(|| PipelineError::ElementNotFound(mixer_id.clone()))?;

        let state = overlay::get_overlay_state(block_instance_id).ok_or_else(|| {
            PipelineError::ElementNotFound(format!(
                "Vision mixer overlay state not found for {}",
                block_instance_id
            ))
        })?;

        let was_active = state.ftb_active.load(std::sync::atomic::Ordering::Relaxed);
        let pgm_group = state.pgm_group();
        let now_active = !was_active;

        let current_time = self
            .pipeline
            .query_position::<gst::ClockTime>()
            .unwrap_or(gst::ClockTime::ZERO);
        let end_time = current_time + gst::ClockTime::from_mseconds(duration_ms);

        // Collect control sources so they stay alive for the duration of the animation
        let mut control_sources: Vec<InterpolationControlSource> = Vec::new();

        for pad in mixer.sink_pads() {
            let name = pad.name();
            if name.starts_with("sink_") {
                if let Ok(idx) = name.trim_start_matches("sink_").parse::<usize>() {
                    let bg = state.background_input();
                    let (start_alpha, end_alpha) = if now_active {
                        // FTB on: fade current alpha to 0
                        let current = pad.property::<f64>("alpha");
                        (current, 0.0)
                    } else if pgm_group.contains(&idx) || bg == Some(idx) {
                        (0.0, 1.0)
                    } else if idx >= state.num_inputs {
                        let dsk_idx = idx - state.num_inputs;
                        let enabled = dsk_idx < state.dsk_enabled.len()
                            && state.dsk_enabled[dsk_idx]
                                .load(std::sync::atomic::Ordering::Relaxed);
                        if enabled {
                            (0.0, 1.0)
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    };

                    if (start_alpha - end_alpha).abs() < f64::EPSILON {
                        continue;
                    }

                    // Clear any existing alpha control binding
                    if let Some(binding) = pad.control_binding("alpha") {
                        pad.remove_control_binding(&binding);
                    }

                    let cs = InterpolationControlSource::new();
                    cs.set_mode(InterpolationMode::Linear);

                    // Ease-in-out keyframes
                    let duration_ns = (end_time - current_time).nseconds() as f64;
                    let num_keyframes = strom_types::vision_mixer::TRANSITION_KEYFRAMES as u32;
                    for i in 0..=num_keyframes {
                        let t = i as f64 / num_keyframes as f64;
                        let eased = (1.0 - (t * std::f64::consts::PI).cos()) / 2.0;
                        let value = start_alpha + (end_alpha - start_alpha) * eased;
                        let time =
                            current_time + gst::ClockTime::from_nseconds((duration_ns * t) as u64);
                        cs.set(time, value);
                    }

                    let binding = DirectControlBinding::new(&pad, "alpha", &cs);
                    let _ = pad.add_control_binding(&binding);
                    control_sources.push(cs);
                }
            }
        }

        // Keep control sources alive until the animation completes, then clean up bindings
        if !control_sources.is_empty() {
            let cleanup_mixer = mixer.clone();
            let cleanup_duration = duration_ms + 100; // small margin
            gst::glib::timeout_add_once(
                std::time::Duration::from_millis(cleanup_duration),
                move || {
                    for pad in cleanup_mixer.sink_pads() {
                        if let Some(binding) = pad.control_binding("alpha") {
                            pad.remove_control_binding(&binding);
                        }
                    }
                    drop(control_sources);
                },
            );
        }

        state
            .ftb_active
            .store(now_active, std::sync::atomic::Ordering::Relaxed);

        overlay::trigger_overlay_update(block_instance_id);

        info!(
            "Vision mixer {} FTB {}",
            block_instance_id,
            if now_active {
                "activated"
            } else {
                "deactivated"
            }
        );

        Ok(now_active)
    }
}

/// Find a pad by name on an element, checking both static and request pads.
/// `static_pad()` doesn't find request pads on aggregator elements like glvideomixer.
fn find_pad(element: &gst::Element, pad_name: &str) -> Option<gst::Pad> {
    element.static_pad(pad_name).or_else(|| {
        element
            .pads()
            .into_iter()
            .find(|p| p.name().as_str() == pad_name)
    })
}
