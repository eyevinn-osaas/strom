//! Visual compositor layout editor.
//!
//! Provides an interactive canvas for editing glcompositor block layouts with:
//! - Drag-and-drop positioning of input boxes
//! - Resize handles for changing input dimensions
//! - Real-time updates via API to running pipeline
//! - Property panel for fine-tuning alpha, zorder, sizing-policy

mod api_sync;
mod canvas;
mod rendering;
mod templates;

use egui::{Color32, Context, Pos2, Rect, Stroke, StrokeKind, Vec2};
use strom_types::{FlowId, PropertyValue};

use crate::api::ApiClient;

/// Handle for resizing an input box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeHandle {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl ResizeHandle {
    /// Get all resize handles.
    fn all() -> &'static [ResizeHandle] {
        &[
            ResizeHandle::TopLeft,
            ResizeHandle::Top,
            ResizeHandle::TopRight,
            ResizeHandle::Left,
            ResizeHandle::Right,
            ResizeHandle::BottomLeft,
            ResizeHandle::Bottom,
            ResizeHandle::BottomRight,
        ]
    }

    /// Get the cursor icon for this handle.
    fn cursor_icon(&self) -> egui::CursorIcon {
        match self {
            ResizeHandle::TopLeft | ResizeHandle::BottomRight => egui::CursorIcon::ResizeNwSe,
            ResizeHandle::TopRight | ResizeHandle::BottomLeft => egui::CursorIcon::ResizeNeSw,
            ResizeHandle::Top | ResizeHandle::Bottom => egui::CursorIcon::ResizeVertical,
            ResizeHandle::Left | ResizeHandle::Right => egui::CursorIcon::ResizeHorizontal,
        }
    }
}

/// Represents a single input in the compositor.
#[derive(Debug, Clone)]
struct InputBox {
    /// Input index (0-based)
    input_index: usize,
    /// Pad name on the mixer element (e.g., "sink_0")
    pad_name: String,

    // Current properties (synced with mixer pad)
    xpos: i32,
    ypos: i32,
    width: i32,
    height: i32,
    alpha: f64,
    zorder: u32,
    sizing_policy: String,

    // UI state
    /// Whether this input is currently selected
    selected: bool,
    /// Pending API update (shows spinner)
    pending_update: bool,
    /// Last error from API update
    last_error: Option<String>,
}

impl InputBox {
    /// Create a new input box with default values.
    fn new(input_index: usize) -> Self {
        Self {
            input_index,
            pad_name: format!("sink_{}", input_index),
            xpos: 0,
            ypos: 0,
            width: 640,
            height: 360,
            alpha: 1.0,
            zorder: input_index as u32,
            sizing_policy: "keep-aspect-ratio".to_string(),
            selected: false,
            pending_update: false,
            last_error: None,
        }
    }

    /// Get the bounding rect in canvas coordinates.
    fn rect(&self) -> Rect {
        Rect::from_min_size(
            Pos2::new(self.xpos as f32, self.ypos as f32),
            Vec2::new(self.width as f32, self.height as f32),
        )
    }

    /// Get color for this input box (based on index).
    fn color(&self) -> Color32 {
        let hue = (self.input_index as f32 * 137.5) % 360.0; // Golden angle for good color distribution
        let (r, g, b) = hsv_to_rgb(hue, 0.7, 0.9);
        Color32::from_rgba_unmultiplied(r, g, b, (self.alpha * 255.0) as u8)
    }
}

/// Convert HSV to RGB (0-255).
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// Compositor layout editor state.
pub struct CompositorEditor {
    /// Flow ID
    flow_id: FlowId,
    /// Block ID (e.g., "b0")
    block_id: String,
    /// Mixer element ID (e.g., "b0:mixer")
    mixer_element_id: String,

    /// Output canvas width
    output_width: u32,
    /// Output canvas height
    output_height: u32,

    /// Input boxes
    inputs: Vec<InputBox>,
    /// Currently selected input index
    selected_input: Option<usize>,

    /// Dragging state
    dragging_input: Option<usize>,
    drag_start_pos: Pos2,
    drag_start_xpos: i32,
    drag_start_ypos: i32,

    /// Resizing state
    resizing_input: Option<(usize, ResizeHandle)>,
    resize_start_pos: Pos2,
    resize_start_width: i32,
    resize_start_height: i32,
    resize_start_xpos: i32,
    resize_start_ypos: i32,

    /// Settings
    snap_to_grid: bool,
    grid_size: u32,
    live_updates: bool,
    /// Animate position/size changes (instead of instant)
    animate_moves: bool,

    /// API client
    api: ApiClient,

    /// Status message
    status: String,

    /// Last time we sent a live update (for throttling)
    last_live_update: instant::Instant,

    // Transition settings
    /// From input for transition
    transition_from: usize,
    /// To input for transition
    transition_to: usize,
    /// Transition type
    transition_type: String,
    /// Transition duration in milliseconds
    transition_duration_ms: u64,
    /// Last transition status message
    transition_status: Option<String>,

    // Thumbnail state
    /// Cached thumbnail textures by input index
    thumbnails: std::collections::HashMap<usize, egui::TextureHandle>,
    /// Last thumbnail fetch time by input index
    thumbnail_fetch_times: std::collections::HashMap<usize, instant::Instant>,
    /// Inputs currently being fetched (to avoid duplicate requests)
    thumbnail_loading: std::collections::HashSet<usize>,
    /// Thumbnail refresh interval in milliseconds
    thumbnail_refresh_ms: u64,
    /// Whether thumbnails are enabled
    thumbnails_enabled: bool,
}

impl CompositorEditor {
    /// Create a new compositor editor.
    pub fn new(
        flow_id: FlowId,
        block_id: String,
        output_width: u32,
        output_height: u32,
        num_inputs: usize,
        api: ApiClient,
    ) -> Self {
        let mixer_element_id = format!("{}:mixer", block_id);

        // Create input boxes
        let inputs: Vec<_> = (0..num_inputs).map(InputBox::new).collect();

        Self {
            flow_id,
            block_id,
            mixer_element_id,
            output_width,
            output_height,
            inputs,
            selected_input: None,
            dragging_input: None,
            drag_start_pos: Pos2::ZERO,
            drag_start_xpos: 0,
            drag_start_ypos: 0,
            resizing_input: None,
            resize_start_pos: Pos2::ZERO,
            resize_start_width: 0,
            resize_start_height: 0,
            resize_start_xpos: 0,
            resize_start_ypos: 0,
            snap_to_grid: false,
            grid_size: 10,
            live_updates: true,
            animate_moves: true,
            api,
            status: "Loading...".to_string(),
            last_live_update: instant::Instant::now(),
            // Transition settings
            transition_from: 0,
            transition_to: if num_inputs > 1 { 1 } else { 0 },
            transition_type: "dip_to_black".to_string(),
            transition_duration_ms: 300,
            transition_status: None,
            // Thumbnail state
            thumbnails: std::collections::HashMap::new(),
            thumbnail_fetch_times: std::collections::HashMap::new(),
            thumbnail_loading: std::collections::HashSet::new(),
            thumbnail_refresh_ms: 1000, // 1 second default
            thumbnails_enabled: true,
        }
    }

    /// Snap value to grid if enabled.
    pub(super) fn snap(&self, value: i32) -> i32 {
        if self.snap_to_grid {
            let grid = self.grid_size as i32;
            ((value + grid / 2) / grid) * grid
        } else {
            value
        }
    }

    /// Toggle input selection (select if not selected, deselect if already selected).
    pub(super) fn toggle_input_selection(&mut self, idx: usize) {
        if self.selected_input == Some(idx) {
            // Deselect
            self.selected_input = None;
            for i in 0..self.inputs.len() {
                self.inputs[i].selected = false;
            }
        } else {
            // Select
            self.selected_input = Some(idx);
            for i in 0..self.inputs.len() {
                self.inputs[i].selected = i == idx;
            }
        }
    }

    /// Deselect any selected input.
    pub(super) fn deselect_input(&mut self) {
        self.selected_input = None;
        for i in 0..self.inputs.len() {
            self.inputs[i].selected = false;
        }
    }

    /// Helper to set input position and send updates.
    pub(super) fn set_input_position(&mut self, ctx: &Context, idx: usize, x: i32, y: i32) {
        self.inputs[idx].xpos = x;
        self.inputs[idx].ypos = y;
        if self.live_updates {
            if self.animate_moves {
                self.animate_input_to(ctx, idx, Some(x), Some(y), None, None);
            } else {
                self.update_pad_property(ctx, idx, "xpos", PropertyValue::Int(x as i64));
                self.update_pad_property(ctx, idx, "ypos", PropertyValue::Int(y as i64));
            }
        }
    }

    /// Helper to set input size and send updates.
    pub(super) fn set_input_size(&mut self, ctx: &Context, idx: usize, w: i32, h: i32) {
        self.inputs[idx].width = w;
        self.inputs[idx].height = h;
        if self.live_updates {
            if self.animate_moves {
                self.animate_input_to(ctx, idx, None, None, Some(w), Some(h));
            } else {
                self.update_pad_property(ctx, idx, "width", PropertyValue::Int(w as i64));
                self.update_pad_property(ctx, idx, "height", PropertyValue::Int(h as i64));
            }
        }
    }

    /// Reset input to default: position (0,0), full size, alpha 1.0
    pub(super) fn reset_input(&mut self, ctx: &Context, idx: usize, out_w: i32, out_h: i32) {
        // Update local state
        self.inputs[idx].xpos = 0;
        self.inputs[idx].ypos = 0;
        self.inputs[idx].width = out_w;
        self.inputs[idx].height = out_h;
        self.inputs[idx].alpha = 1.0;

        if self.live_updates {
            if self.animate_moves {
                // Animate position and size
                self.animate_input_to(ctx, idx, Some(0), Some(0), Some(out_w), Some(out_h));
            } else {
                // Immediate update
                self.update_pad_property(ctx, idx, "xpos", PropertyValue::Int(0));
                self.update_pad_property(ctx, idx, "ypos", PropertyValue::Int(0));
                self.update_pad_property(ctx, idx, "width", PropertyValue::Int(out_w as i64));
                self.update_pad_property(ctx, idx, "height", PropertyValue::Int(out_h as i64));
            }
            // Alpha is always immediate (not animated)
            self.update_pad_property(ctx, idx, "alpha", PropertyValue::Float(1.0));
        }
    }

    /// Animate an input to target position/size.
    fn animate_input_to(
        &mut self,
        ctx: &Context,
        idx: usize,
        xpos: Option<i32>,
        ypos: Option<i32>,
        width: Option<i32>,
        height: Option<i32>,
    ) {
        let flow_id = self.flow_id;
        let block_id = self.block_id.clone();
        let duration_ms = self.transition_duration_ms;
        let api = self.api.clone();
        let ctx = ctx.clone();

        tracing::info!(
            "Animating input {} to ({:?}, {:?}, {:?}, {:?}) over {}ms",
            idx,
            xpos,
            ypos,
            width,
            height,
            duration_ms
        );

        crate::app::spawn_task(async move {
            match api
                .animate_input(
                    &flow_id.to_string(),
                    &block_id,
                    idx,
                    xpos,
                    ypos,
                    width,
                    height,
                    duration_ms,
                )
                .await
            {
                Ok(_) => {
                    tracing::info!("Animation started");
                }
                Err(e) => {
                    tracing::error!("Animation failed: {}", e);
                }
            }
            ctx.request_repaint();
        });
    }
}
