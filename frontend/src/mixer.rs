//! Mixer editor - fullscreen audio mixer view.
//!
//! Provides an interactive mixer console similar to digital mixers like Behringer X32:
//! - Per-channel faders, pan controls, mute buttons
//! - Real-time metering
//! - Keyboard shortcuts for quick mixing
//!
//! Phase 1: Basic faders, pan, mute, metering
//! Future: Gate, compressor, EQ, aux sends, subgroups, PFL

use egui::{Color32, Context, CornerRadius, Rect, Response, Sense, Stroke, Ui, Vec2};
use std::collections::HashMap;
use strom_types::{FlowId, PropertyValue};

use crate::api::ApiClient;
use crate::meter::{MeterData, MeterDataStore};

/// Default fader value (~-6dB)
const DEFAULT_FADER: f32 = 0.75;

/// A single channel strip in the mixer.
#[derive(Debug, Clone)]
struct ChannelStrip {
    /// Channel number (1-indexed)
    channel_num: usize,
    /// Channel label
    label: String,
    /// Pan position (-1.0 to 1.0)
    pan: f32,
    /// Fader level (0.0 to 2.0)
    fader: f32,
    /// Mute state
    mute: bool,
    /// PFL (Pre-Fader Listen) state
    pfl: bool,
    /// Gate enabled
    gate_enabled: bool,
    /// Gate threshold (dB)
    gate_threshold: f32,
    /// Gate attack (ms)
    gate_attack: f32,
    /// Gate release (ms)
    gate_release: f32,
    /// Compressor enabled
    comp_enabled: bool,
    /// Compressor threshold (dB)
    comp_threshold: f32,
    /// Compressor ratio
    comp_ratio: f32,
    /// Compressor attack (ms)
    comp_attack: f32,
    /// Compressor release (ms)
    comp_release: f32,
    /// Compressor makeup gain (dB)
    comp_makeup: f32,
    /// EQ enabled
    eq_enabled: bool,
    /// EQ bands: (freq, gain_db, q) for 4 bands
    eq_bands: [(f32, f32, f32); 4],
    /// Pending API update
    pending_update: bool,
}

impl ChannelStrip {
    fn new(channel_num: usize) -> Self {
        Self {
            channel_num,
            label: format!("Ch {}", channel_num),
            pan: 0.0,
            fader: DEFAULT_FADER,
            mute: false,
            pfl: false,
            gate_enabled: false,
            gate_threshold: -40.0,
            gate_attack: 5.0,
            gate_release: 100.0,
            comp_enabled: false,
            comp_threshold: -20.0,
            comp_ratio: 4.0,
            comp_attack: 10.0,
            comp_release: 100.0,
            comp_makeup: 0.0,
            eq_enabled: false,
            eq_bands: [
                (80.0, 0.0, 1.0),   // Low
                (400.0, 0.0, 1.0),  // Low-mid
                (2000.0, 0.0, 1.0), // High-mid
                (8000.0, 0.0, 1.0), // High
            ],
            pending_update: false,
        }
    }
}

/// What control is currently being adjusted (for value display).
#[derive(Debug, Clone, PartialEq)]
enum ActiveControl {
    None,
    Pan(usize),   // Channel index
    Fader(usize), // Channel index
    MainFader,
}

/// Mixer editor state.
pub struct MixerEditor {
    /// Flow ID
    flow_id: FlowId,
    /// Block ID (e.g., "b0")
    block_id: String,

    /// Number of channels
    num_channels: usize,
    /// Channel strips
    channels: Vec<ChannelStrip>,
    /// Currently selected channel (for keyboard control)
    selected_channel: Option<usize>,
    /// Currently active control (for value display)
    active_control: ActiveControl,

    /// Main fader level
    main_fader: f32,
    /// Main mute
    main_mute: bool,

    /// API client
    api: ApiClient,

    /// Status message
    status: String,
    /// Error message
    error: Option<String>,

    /// Live updates enabled
    live_updates: bool,
    /// Last update time (for throttling)
    last_update: instant::Instant,
}

impl MixerEditor {
    /// Create a new mixer editor.
    pub fn new(flow_id: FlowId, block_id: String, num_channels: usize, api: ApiClient) -> Self {
        let channels = (1..=num_channels).map(ChannelStrip::new).collect();

        Self {
            flow_id,
            block_id,
            num_channels,
            channels,
            selected_channel: None,
            active_control: ActiveControl::None,
            main_fader: 1.0,
            main_mute: false,
            api,
            status: String::new(),
            error: None,
            live_updates: true,
            last_update: instant::Instant::now(),
        }
    }

    /// Load channel values from block properties.
    pub fn load_from_properties(&mut self, properties: &HashMap<String, PropertyValue>) {
        // Load main fader
        if let Some(PropertyValue::Float(f)) = properties.get("main_fader") {
            self.main_fader = *f as f32;
        }

        // Load per-channel properties
        for ch in &mut self.channels {
            let ch_num = ch.channel_num;

            if let Some(PropertyValue::Float(f)) = properties.get(&format!("ch{}_pan", ch_num)) {
                ch.pan = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) = properties.get(&format!("ch{}_fader", ch_num)) {
                ch.fader = *f as f32;
            }
            if let Some(PropertyValue::Bool(b)) = properties.get(&format!("ch{}_mute", ch_num)) {
                ch.mute = *b;
            }
            // Gate
            if let Some(PropertyValue::Bool(b)) =
                properties.get(&format!("ch{}_gate_enabled", ch_num))
            {
                ch.gate_enabled = *b;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_gate_threshold", ch_num))
            {
                ch.gate_threshold = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_gate_attack", ch_num))
            {
                ch.gate_attack = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_gate_release", ch_num))
            {
                ch.gate_release = *f as f32;
            }
            // Compressor
            if let Some(PropertyValue::Bool(b)) =
                properties.get(&format!("ch{}_comp_enabled", ch_num))
            {
                ch.comp_enabled = *b;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_threshold", ch_num))
            {
                ch.comp_threshold = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_ratio", ch_num))
            {
                ch.comp_ratio = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_attack", ch_num))
            {
                ch.comp_attack = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_release", ch_num))
            {
                ch.comp_release = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_makeup", ch_num))
            {
                ch.comp_makeup = *f as f32;
            }
            // EQ
            if let Some(PropertyValue::Bool(b)) =
                properties.get(&format!("ch{}_eq_enabled", ch_num))
            {
                ch.eq_enabled = *b;
            }
            for band in 0..4 {
                let band_num = band + 1;
                if let Some(PropertyValue::Float(f)) =
                    properties.get(&format!("ch{}_eq{}_freq", ch_num, band_num))
                {
                    ch.eq_bands[band].0 = *f as f32;
                }
                if let Some(PropertyValue::Float(f)) =
                    properties.get(&format!("ch{}_eq{}_gain", ch_num, band_num))
                {
                    ch.eq_bands[band].1 = *f as f32;
                }
                if let Some(PropertyValue::Float(f)) =
                    properties.get(&format!("ch{}_eq{}_q", ch_num, band_num))
                {
                    ch.eq_bands[band].2 = *f as f32;
                }
            }
        }
    }

    /// Get the block ID.
    pub fn block_id(&self) -> &str {
        &self.block_id
    }

    /// Show the mixer in fullscreen mode.
    pub fn show_fullscreen(&mut self, ui: &mut Ui, ctx: &Context, meter_store: &MeterDataStore) {
        // Handle keyboard shortcuts
        self.handle_keyboard(ui, ctx);

        // Main layout - use available height for channel strips, reserve space for detail panel
        let available_height = ui.available_height();
        let detail_panel_height = if self.selected_channel.is_some() {
            180.0
        } else {
            0.0
        };
        let status_bar_height = 30.0;
        let channel_area_height = available_height - detail_panel_height - status_bar_height - 20.0;

        ui.vertical(|ui| {
            // Channel strips area (scrollable horizontally, fixed height)
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), channel_area_height.max(200.0)),
                egui::Layout::left_to_right(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Render channel strips
                                for i in 0..self.channels.len() {
                                    let meter_key = format!("{}:meter:{}", self.block_id, i + 1);
                                    let meter_data = meter_store.get(&self.flow_id, &meter_key);

                                    self.render_channel_strip(ui, ctx, i, meter_data);
                                    ui.add_space(4.0);
                                }

                                ui.add_space(16.0);
                                ui.separator();
                                ui.add_space(16.0);

                                // Main/Master section
                                self.render_main_strip(ui, ctx, meter_store);
                            });
                        });
                },
            );

            // Detail panel for selected channel (Gate/Comp/EQ)
            if self.selected_channel.is_some() {
                ui.separator();
                self.render_detail_panel(ui, ctx);
            }

            // Status bar
            ui.separator();
            ui.horizontal(|ui| {
                if let Some(error) = &self.error {
                    ui.colored_label(Color32::RED, error);
                } else if !self.status.is_empty() {
                    ui.label(&self.status);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(ch) = self.selected_channel {
                        ui.label(format!("Selected: Ch {}", ch + 1));
                    }
                    ui.checkbox(&mut self.live_updates, "Live");
                });
            });
        });
    }

    /// Render a single channel strip.
    fn render_channel_strip(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        index: usize,
        meter_data: Option<&MeterData>,
    ) {
        // Extract values from channel to avoid borrow issues
        let channel_label = self.channels[index].label.clone();
        let channel_pan = self.channels[index].pan;
        let channel_fader = self.channels[index].fader;
        let channel_mute = self.channels[index].mute;
        let channel_pfl = self.channels[index].pfl;
        let channel_gate = self.channels[index].gate_enabled;
        let channel_comp = self.channels[index].comp_enabled;
        let channel_eq = self.channels[index].eq_enabled;
        let is_selected = self.selected_channel == Some(index);
        let strip_width = 60.0;

        // Channel strip frame
        let frame_color = if is_selected {
            Color32::from_rgb(60, 80, 100)
        } else {
            Color32::from_rgb(40, 40, 45)
        };

        egui::Frame::default()
            .fill(frame_color)
            .corner_radius(CornerRadius::same(4))
            .inner_margin(4.0)
            .show(ui, |ui| {
                ui.set_min_width(strip_width);
                ui.set_max_width(strip_width);

                ui.vertical_centered(|ui| {
                    // Channel label - click to select
                    if ui
                        .add(
                            egui::Label::new(egui::RichText::new(&channel_label).strong())
                                .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        self.selected_channel = Some(index);
                    }

                    ui.add_space(4.0);

                    // Gate / Comp / EQ toggle buttons
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;

                        // Gate button
                        let gate_color = if channel_gate {
                            Color32::from_rgb(0, 150, 0)
                        } else {
                            Color32::from_rgb(50, 50, 55)
                        };
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("G").small().color(
                                    if channel_gate {
                                        Color32::WHITE
                                    } else {
                                        Color32::GRAY
                                    },
                                ))
                                .fill(gate_color)
                                .min_size(Vec2::new(20.0, 18.0)),
                            )
                            .clicked()
                        {
                            self.channels[index].gate_enabled = !self.channels[index].gate_enabled;
                            self.update_channel_property(ctx, index, "gate_enabled");
                        }

                        // Compressor button
                        let comp_color = if channel_comp {
                            Color32::from_rgb(180, 100, 0)
                        } else {
                            Color32::from_rgb(50, 50, 55)
                        };
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("C").small().color(
                                    if channel_comp {
                                        Color32::WHITE
                                    } else {
                                        Color32::GRAY
                                    },
                                ))
                                .fill(comp_color)
                                .min_size(Vec2::new(20.0, 18.0)),
                            )
                            .clicked()
                        {
                            self.channels[index].comp_enabled = !self.channels[index].comp_enabled;
                            self.update_channel_property(ctx, index, "comp_enabled");
                        }

                        // EQ button
                        let eq_color = if channel_eq {
                            Color32::from_rgb(0, 100, 180)
                        } else {
                            Color32::from_rgb(50, 50, 55)
                        };
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("E").small().color(
                                    if channel_eq {
                                        Color32::WHITE
                                    } else {
                                        Color32::GRAY
                                    },
                                ))
                                .fill(eq_color)
                                .min_size(Vec2::new(20.0, 18.0)),
                            )
                            .clicked()
                        {
                            self.channels[index].eq_enabled = !self.channels[index].eq_enabled;
                            self.update_channel_property(ctx, index, "eq_enabled");
                        }
                    });

                    ui.add_space(4.0);

                    // Value display - shows pan normally, or fader dB when adjusting
                    let display_text = match &self.active_control {
                        ActiveControl::Fader(ch) if *ch == index => {
                            // Show fader dB when adjusting
                            if channel_fader <= 0.001 {
                                "-inf dB".to_string()
                            } else {
                                format!("{:.1} dB", 20.0 * channel_fader.log10())
                            }
                        }
                        _ => {
                            // Show pan value normally
                            if channel_pan < -0.01 {
                                format!("L{:.0}", (-channel_pan * 100.0))
                            } else if channel_pan > 0.01 {
                                format!("R{:.0}", (channel_pan * 100.0))
                            } else {
                                "C".to_string()
                            }
                        }
                    };

                    // Styled display box (like small LCD)
                    let display_rect = ui
                        .allocate_exact_size(Vec2::new(strip_width - 12.0, 18.0), Sense::hover())
                        .0;
                    let painter = ui.painter();
                    painter.rect_filled(
                        display_rect,
                        CornerRadius::same(2),
                        Color32::from_rgb(20, 25, 30),
                    );
                    painter.rect_stroke(
                        display_rect,
                        CornerRadius::same(2),
                        Stroke::new(1.0, Color32::from_rgb(50, 55, 60)),
                        egui::epaint::StrokeKind::Inside,
                    );
                    painter.text(
                        display_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &display_text,
                        egui::FontId::monospace(11.0),
                        Color32::from_rgb(100, 200, 100), // Green LCD-like color
                    );

                    ui.add_space(2.0);

                    // Pan control
                    let pan_response = self.render_pan_control(ui, index);
                    if pan_response.dragged() {
                        self.active_control = ActiveControl::Pan(index);
                    } else if pan_response.drag_stopped() {
                        self.active_control = ActiveControl::None;
                    }
                    if pan_response.changed() {
                        self.update_channel_property(ctx, index, "pan");
                    }

                    ui.add_space(4.0);

                    // Fader with meter and scale - use fixed height container
                    let fader_height = 180.0;
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), fader_height),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            // Stereo meter (L/R)
                            self.render_stereo_meter(ui, meter_data, fader_height);

                            ui.add_space(2.0);

                            // dB scale
                            self.render_db_scale(ui, fader_height);

                            ui.add_space(2.0);

                            // Fader
                            let fader_response = self.render_fader(ui, index, fader_height);
                            if fader_response.dragged() {
                                self.active_control = ActiveControl::Fader(index);
                                self.update_channel_property(ctx, index, "fader");
                            } else if fader_response.drag_stopped() {
                                self.active_control = ActiveControl::None;
                            }
                        },
                    );

                    ui.add_space(4.0);

                    // Mute button
                    let mute_text = "M";
                    let mute_color = if channel_mute {
                        Color32::RED
                    } else {
                        Color32::from_rgb(60, 60, 65)
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(mute_text).color(Color32::WHITE).small(),
                            )
                            .fill(mute_color)
                            .min_size(Vec2::new(strip_width - 8.0, 20.0)),
                        )
                        .clicked()
                    {
                        self.channels[index].mute = !self.channels[index].mute;
                        self.update_channel_property(ctx, index, "mute");
                    }

                    // PFL button (not yet implemented - requires PFL bus)
                    ui.add_space(2.0);
                    let pfl_color = if channel_pfl {
                        Color32::YELLOW
                    } else {
                        Color32::from_rgb(50, 50, 55)
                    };
                    let pfl_btn = ui.add(
                        egui::Button::new(
                            egui::RichText::new("PFL")
                                .color(if channel_pfl {
                                    Color32::BLACK
                                } else {
                                    Color32::from_gray(100)
                                })
                                .small(),
                        )
                        .fill(pfl_color)
                        .min_size(Vec2::new(strip_width - 8.0, 18.0)),
                    );
                    if pfl_btn.clicked() {
                        self.channels[index].pfl = !self.channels[index].pfl;
                        // PFL not yet implemented in backend - visual toggle only
                    }
                    pfl_btn.on_hover_text("Pre-Fader Listen (not yet implemented)");
                });
            });
    }

    /// Render the main/master strip.
    fn render_main_strip(&mut self, ui: &mut Ui, ctx: &Context, meter_store: &MeterDataStore) {
        let strip_width = 120.0;

        // Get main meter data
        let main_meter_key = format!("{}:meter:main", self.block_id);
        let main_meter_data = meter_store.get(&self.flow_id, &main_meter_key);

        egui::Frame::default()
            .fill(Color32::from_rgb(50, 50, 60))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_min_width(strip_width);
                ui.set_max_width(strip_width);

                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("MAIN").strong().size(16.0));

                    ui.add_space(8.0);

                    // Value display (shows dB)
                    let db = if self.main_fader > 0.0 {
                        20.0 * self.main_fader.log10()
                    } else {
                        -60.0
                    };
                    let display_text = if db <= -59.0 {
                        "-inf dB".to_string()
                    } else {
                        format!("{:.1} dB", db)
                    };

                    // Styled display box (like small LCD)
                    let display_rect = ui
                        .allocate_exact_size(Vec2::new(strip_width - 20.0, 22.0), Sense::hover())
                        .0;
                    let painter = ui.painter();
                    painter.rect_filled(
                        display_rect,
                        CornerRadius::same(2),
                        Color32::from_rgb(20, 25, 30),
                    );
                    painter.rect_stroke(
                        display_rect,
                        CornerRadius::same(2),
                        Stroke::new(1.0, Color32::from_rgb(50, 55, 60)),
                        egui::epaint::StrokeKind::Inside,
                    );
                    painter.text(
                        display_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &display_text,
                        egui::FontId::monospace(12.0),
                        Color32::from_rgb(100, 200, 100), // Green LCD-like color
                    );

                    ui.add_space(8.0);

                    // Main fader with stereo meter and scale - use fixed height container
                    let fader_height = 180.0;
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), fader_height),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            // Stereo meter
                            self.render_stereo_meter(ui, main_meter_data, fader_height);

                            ui.add_space(2.0);

                            // dB scale
                            self.render_db_scale(ui, fader_height);

                            ui.add_space(2.0);

                            // Main fader - custom widget in dB scale
                            let mut main_fader_db = linear_to_db(self.main_fader as f64) as f32;

                            let (rect, response) = ui
                                .allocate_exact_size(Vec2::new(20.0, fader_height), Sense::drag());

                            if response.dragged() {
                                self.active_control = ActiveControl::MainFader;
                                let delta = -response.drag_delta().y;
                                let db_per_pixel = 66.0 / fader_height;
                                main_fader_db =
                                    (main_fader_db + delta * db_per_pixel).clamp(-60.0, 6.0);
                                self.main_fader = db_to_linear_f32(main_fader_db);
                            } else if response.drag_stopped() {
                                self.active_control = ActiveControl::None;
                            }

                            // Draw fader track
                            let painter = ui.painter();
                            let track_width = 4.0;
                            let track_rect = Rect::from_center_size(
                                rect.center(),
                                Vec2::new(track_width, fader_height - 10.0),
                            );
                            painter.rect_filled(
                                track_rect,
                                CornerRadius::same(2),
                                Color32::from_gray(60),
                            );

                            // Draw fader handle
                            let normalized = (main_fader_db - (-60.0)) / 66.0;
                            let handle_y = rect.max.y - 5.0 - (normalized * (fader_height - 10.0));
                            let handle_rect = Rect::from_center_size(
                                egui::pos2(rect.center().x, handle_y),
                                Vec2::new(18.0, 10.0),
                            );
                            let handle_color = if response.dragged() {
                                Color32::from_rgb(100, 150, 255)
                            } else if response.hovered() {
                                Color32::from_rgb(200, 200, 200)
                            } else {
                                Color32::from_rgb(160, 160, 160)
                            };
                            painter.rect_filled(handle_rect, CornerRadius::same(2), handle_color);

                            if response.drag_stopped() || response.dragged() {
                                self.update_main_fader(ctx);
                            }
                        },
                    );

                    ui.add_space(8.0);

                    // Main mute
                    let mute_text = if self.main_mute { "MUTE" } else { "M" };
                    let mute_color = if self.main_mute {
                        Color32::RED
                    } else {
                        Color32::GRAY
                    };
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(mute_text).color(Color32::WHITE))
                                .fill(mute_color)
                                .min_size(Vec2::new(strip_width - 16.0, 30.0)),
                        )
                        .clicked()
                    {
                        self.main_mute = !self.main_mute;
                        self.update_main_mute(ctx);
                    }
                });
            });
    }

    /// Render a pan control (horizontal slider).
    fn render_pan_control(&mut self, ui: &mut Ui, index: usize) -> Response {
        let channel = &mut self.channels[index];

        // Compact pan slider
        ui.add_sized(
            Vec2::new(50.0, 18.0),
            egui::Slider::new(&mut channel.pan, -1.0..=1.0)
                .custom_formatter(|v, _| {
                    if v < -0.01 {
                        format!("L{:.0}", (-v * 100.0))
                    } else if v > 0.01 {
                        format!("R{:.0}", (v * 100.0))
                    } else {
                        "C".to_string()
                    }
                })
                .show_value(false),
        )
    }

    /// Render a vertical fader using dB scale.
    /// Internally converts between dB (-60 to +6) and linear (0.0 to 2.0).
    fn render_fader(&mut self, ui: &mut Ui, index: usize, height: f32) -> Response {
        let channel = &mut self.channels[index];

        // Convert linear fader value to dB for display
        let mut fader_db = linear_to_db(channel.fader as f64) as f32;

        // Allocate exact size for the fader
        let (rect, response) = ui.allocate_exact_size(Vec2::new(16.0, height), Sense::drag());

        if response.dragged() {
            // Calculate new dB value based on drag position
            let delta = -response.drag_delta().y; // Negative because y increases downward
            let db_per_pixel = 66.0 / height; // -60 to +6 = 66 dB range
            fader_db = (fader_db + delta * db_per_pixel).clamp(-60.0, 6.0);
            channel.fader = db_to_linear_f32(fader_db);
        }

        // Draw fader track
        let painter = ui.painter();
        let track_width = 4.0;
        let track_rect =
            Rect::from_center_size(rect.center(), Vec2::new(track_width, height - 10.0));
        painter.rect_filled(track_rect, CornerRadius::same(2), Color32::from_gray(60));

        // Draw fader handle
        let normalized = (fader_db - (-60.0)) / 66.0; // 0.0 to 1.0
        let handle_y = rect.max.y - 5.0 - (normalized * (height - 10.0));
        let handle_rect =
            Rect::from_center_size(egui::pos2(rect.center().x, handle_y), Vec2::new(14.0, 8.0));
        let handle_color = if response.dragged() {
            Color32::from_rgb(100, 150, 255)
        } else if response.hovered() {
            Color32::from_rgb(200, 200, 200)
        } else {
            Color32::from_rgb(160, 160, 160)
        };
        painter.rect_filled(handle_rect, CornerRadius::same(2), handle_color);

        response
    }

    /// Render a stereo vertical meter (L/R side by side).
    fn render_stereo_meter(&self, ui: &mut Ui, meter_data: Option<&MeterData>, height: f32) {
        let bar_width = 6.0;
        let gap = 2.0;
        let total_width = bar_width * 2.0 + gap;
        let (rect, _response) =
            ui.allocate_exact_size(Vec2::new(total_width, height), Sense::hover());

        let painter = ui.painter();

        // Left channel rect
        let left_rect = Rect::from_min_size(rect.min, Vec2::new(bar_width, height));
        // Right channel rect
        let right_rect = Rect::from_min_size(
            egui::pos2(rect.min.x + bar_width + gap, rect.min.y),
            Vec2::new(bar_width, height),
        );

        // Background for both
        painter.rect_filled(
            left_rect,
            CornerRadius::same(2),
            Color32::from_rgb(20, 20, 20),
        );
        painter.rect_filled(
            right_rect,
            CornerRadius::same(2),
            Color32::from_rgb(20, 20, 20),
        );

        if let Some(data) = meter_data {
            // Get L/R peak values (or use same for both if mono)
            let (left_peak, right_peak) = if data.peak.len() >= 2 {
                (data.peak[0], data.peak[1])
            } else if !data.peak.is_empty() {
                (data.peak[0], data.peak[0])
            } else {
                return;
            };

            // Draw left channel
            let left_level = db_to_level(left_peak);
            let left_bar_height = height * left_level;
            let left_bar_rect = Rect::from_min_max(
                egui::pos2(left_rect.min.x, left_rect.max.y - left_bar_height),
                left_rect.max,
            );
            painter.rect_filled(
                left_bar_rect,
                CornerRadius::same(2),
                level_to_color(left_level),
            );

            // Draw right channel
            let right_level = db_to_level(right_peak);
            let right_bar_height = height * right_level;
            let right_bar_rect = Rect::from_min_max(
                egui::pos2(right_rect.min.x, right_rect.max.y - right_bar_height),
                right_rect.max,
            );
            painter.rect_filled(
                right_bar_rect,
                CornerRadius::same(2),
                level_to_color(right_level),
            );

            // Draw decay lines if available
            if data.decay.len() >= 2 {
                let left_decay = db_to_level(data.decay[0]);
                let left_decay_y = left_rect.max.y - height * left_decay;
                painter.line_segment(
                    [
                        egui::pos2(left_rect.min.x, left_decay_y),
                        egui::pos2(left_rect.max.x, left_decay_y),
                    ],
                    Stroke::new(1.0, Color32::WHITE),
                );

                let right_decay = db_to_level(data.decay[1]);
                let right_decay_y = right_rect.max.y - height * right_decay;
                painter.line_segment(
                    [
                        egui::pos2(right_rect.min.x, right_decay_y),
                        egui::pos2(right_rect.max.x, right_decay_y),
                    ],
                    Stroke::new(1.0, Color32::WHITE),
                );
            }
        }

        // Borders
        painter.rect_stroke(
            left_rect,
            CornerRadius::same(2),
            Stroke::new(1.0, Color32::from_gray(60)),
            egui::epaint::StrokeKind::Inside,
        );
        painter.rect_stroke(
            right_rect,
            CornerRadius::same(2),
            Stroke::new(1.0, Color32::from_gray(60)),
            egui::epaint::StrokeKind::Inside,
        );
    }

    /// Render a dB scale next to the fader.
    /// Scale is in dB with equal visual spacing for equal dB steps.
    fn render_db_scale(&self, ui: &mut Ui, height: f32) {
        let width = 18.0;
        let (rect, _response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());

        let painter = ui.painter();

        // dB marks with equal visual spacing
        // Range: -60 dB to +6 dB (66 dB total range)
        let min_db = -60.0_f32;
        let max_db = 6.0_f32;
        let db_range = max_db - min_db;

        let marks: &[f32] = &[6.0, 0.0, -6.0, -12.0, -20.0, -30.0, -40.0, -60.0];

        let text_color = Color32::from_gray(140);

        for &db in marks {
            // Map dB to position (equal dB spacing = equal visual spacing)
            let normalized = (db - min_db) / db_range; // 0.0 to 1.0
            let y = rect.max.y - (normalized * height);

            let label = if db > 0.0 {
                format!("+{}", db as i32)
            } else {
                format!("{}", db as i32)
            };

            // Draw tick mark
            painter.line_segment(
                [egui::pos2(rect.max.x - 3.0, y), egui::pos2(rect.max.x, y)],
                Stroke::new(1.0, Color32::from_gray(100)),
            );

            // Draw label
            painter.text(
                egui::pos2(rect.min.x, y),
                egui::Align2::LEFT_CENTER,
                &label,
                egui::FontId::proportional(8.0),
                text_color,
            );
        }
    }

    /// Render the detail panel for the selected channel (Gate/Comp/EQ controls).
    fn render_detail_panel(&mut self, ui: &mut Ui, ctx: &Context) {
        let Some(index) = self.selected_channel else {
            return;
        };
        let ch_num = index + 1;

        egui::Frame::default()
            .fill(Color32::from_rgb(35, 35, 40))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Channel {} - Processing", ch_num)).strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            self.selected_channel = None;
                        }
                    });
                });

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    // Gate section
                    self.render_gate_section(ui, ctx, index);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // Compressor section
                    self.render_comp_section(ui, ctx, index);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // EQ section
                    self.render_eq_section(ui, ctx, index);
                });
            });
    }

    /// Render Gate controls.
    fn render_gate_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        ui.vertical(|ui| {
            let enabled = self.channels[index].gate_enabled;
            let header_color = if enabled {
                Color32::from_rgb(0, 150, 0)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("GATE").color(header_color).strong());
                if ui
                    .checkbox(&mut self.channels[index].gate_enabled, "")
                    .changed()
                {
                    self.update_channel_property(ctx, index, "gate_enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            ui.horizontal(|ui| {
                ui.label("Thresh:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].gate_threshold)
                            .range(-60.0..=0.0)
                            .suffix(" dB")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "gate", "threshold");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Attack:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].gate_attack)
                            .range(0.1..=200.0)
                            .suffix(" ms")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "gate", "attack");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Release:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].gate_release)
                            .range(10.0..=1000.0)
                            .suffix(" ms")
                            .speed(1.0),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "gate", "release");
                }
            });
        });
    }

    /// Render Compressor controls.
    fn render_comp_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        ui.vertical(|ui| {
            let enabled = self.channels[index].comp_enabled;
            let header_color = if enabled {
                Color32::from_rgb(180, 100, 0)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("COMPRESSOR")
                        .color(header_color)
                        .strong(),
                );
                if ui
                    .checkbox(&mut self.channels[index].comp_enabled, "")
                    .changed()
                {
                    self.update_channel_property(ctx, index, "comp_enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            ui.horizontal(|ui| {
                ui.label("Thresh:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_threshold)
                            .range(-60.0..=0.0)
                            .suffix(" dB")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "threshold");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Ratio:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_ratio)
                            .range(1.0..=20.0)
                            .suffix(":1")
                            .speed(0.1),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "ratio");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Attack:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_attack)
                            .range(0.1..=200.0)
                            .suffix(" ms")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "attack");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Release:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_release)
                            .range(10.0..=1000.0)
                            .suffix(" ms")
                            .speed(1.0),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "release");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Makeup:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_makeup)
                            .range(0.0..=24.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "makeup");
                }
            });
        });
    }

    /// Render EQ controls.
    fn render_eq_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        ui.vertical(|ui| {
            let enabled = self.channels[index].eq_enabled;
            let header_color = if enabled {
                Color32::from_rgb(0, 100, 180)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("EQ").color(header_color).strong());
                if ui
                    .checkbox(&mut self.channels[index].eq_enabled, "")
                    .changed()
                {
                    self.update_channel_property(ctx, index, "eq_enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            let band_names = ["Low", "Lo-Mid", "Hi-Mid", "High"];

            ui.horizontal(|ui| {
                for (band, name) in band_names.iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(*name).small());

                        // Frequency
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.channels[index].eq_bands[band].0)
                                    .range(20.0..=20000.0)
                                    .suffix(" Hz")
                                    .speed(10.0),
                            )
                            .changed()
                        {
                            self.update_eq_param(ctx, index, band, "freq");
                        }

                        // Gain
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.channels[index].eq_bands[band].1)
                                    .range(-15.0..=15.0)
                                    .suffix(" dB")
                                    .speed(0.1),
                            )
                            .changed()
                        {
                            self.update_eq_param(ctx, index, band, "gain");
                        }

                        // Q
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.channels[index].eq_bands[band].2)
                                    .range(0.1..=10.0)
                                    .prefix("Q ")
                                    .speed(0.05),
                            )
                            .changed()
                        {
                            self.update_eq_param(ctx, index, band, "q");
                        }
                    });

                    if band < 3 {
                        ui.add_space(8.0);
                    }
                }
            });
        });
    }

    /// Update a processing parameter (gate/comp).
    fn update_processing_param(
        &mut self,
        ctx: &Context,
        index: usize,
        processor: &str,
        param: &str,
    ) {
        if !self.live_updates {
            return;
        }

        let channel = &self.channels[index];

        let (element_suffix, gst_prop, value) = match (processor, param) {
            ("gate", "threshold") => (
                format!("gate_{}", index),
                "gt".to_string(),
                PropertyValue::Float(db_to_linear_f64(channel.gate_threshold as f64)),
            ),
            ("gate", "attack") => (
                format!("gate_{}", index),
                "at".to_string(),
                PropertyValue::Float(channel.gate_attack as f64),
            ),
            ("gate", "release") => (
                format!("gate_{}", index),
                "rt".to_string(),
                PropertyValue::Float(channel.gate_release as f64),
            ),
            ("comp", "threshold") => (
                format!("comp_{}", index),
                "al".to_string(),
                PropertyValue::Float(db_to_linear_f64(channel.comp_threshold as f64)),
            ),
            ("comp", "ratio") => (
                format!("comp_{}", index),
                "cr".to_string(),
                PropertyValue::Float(channel.comp_ratio as f64),
            ),
            ("comp", "attack") => (
                format!("comp_{}", index),
                "at".to_string(),
                PropertyValue::Float(channel.comp_attack as f64),
            ),
            ("comp", "release") => (
                format!("comp_{}", index),
                "rt".to_string(),
                PropertyValue::Float(channel.comp_release as f64),
            ),
            ("comp", "makeup") => (
                format!("comp_{}", index),
                "mk".to_string(),
                PropertyValue::Float(db_to_linear_f64(channel.comp_makeup as f64)),
            ),
            _ => return,
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:{}", self.block_id, element_suffix);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, &gst_prop, value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update an EQ band parameter.
    fn update_eq_param(&mut self, ctx: &Context, index: usize, band: usize, param: &str) {
        if !self.live_updates {
            return;
        }

        let channel = &self.channels[index];
        let (freq, gain, q) = channel.eq_bands[band];

        let (gst_prop, value) = match param {
            "freq" => (format!("f-{}", band), PropertyValue::Float(freq as f64)),
            "gain" => (
                format!("g-{}", band),
                PropertyValue::Float(db_to_linear_f64(gain as f64)),
            ),
            "q" => (format!("q-{}", band), PropertyValue::Float(q as f64)),
            _ => return,
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:eq_{}", self.block_id, index);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, &gst_prop, value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Handle keyboard shortcuts.
    fn handle_keyboard(&mut self, ui: &mut Ui, ctx: &Context) {
        // Number keys 1-9, 0 for channel selection
        for (key, ch) in [
            (egui::Key::Num1, 0),
            (egui::Key::Num2, 1),
            (egui::Key::Num3, 2),
            (egui::Key::Num4, 3),
            (egui::Key::Num5, 4),
            (egui::Key::Num6, 5),
            (egui::Key::Num7, 6),
            (egui::Key::Num8, 7),
            (egui::Key::Num9, 8),
            (egui::Key::Num0, 9),
        ] {
            if ui.input(|i| i.key_pressed(key)) && ch < self.channels.len() {
                self.selected_channel = Some(ch);
            }
        }

        // M = Mute selected channel
        if ui.input(|i| i.key_pressed(egui::Key::M)) {
            if let Some(ch) = self.selected_channel {
                self.channels[ch].mute = !self.channels[ch].mute;
                self.update_channel_property(ctx, ch, "mute");
            }
        }

        // P = PFL selected channel
        if ui.input(|i| i.key_pressed(egui::Key::P)) {
            if let Some(ch) = self.selected_channel {
                self.channels[ch].pfl = !self.channels[ch].pfl;
            }
        }

        // Up/Down = Adjust fader
        if let Some(ch) = self.selected_channel {
            let fader_step = 0.05;
            if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                self.channels[ch].fader = (self.channels[ch].fader + fader_step).min(2.0);
                self.update_channel_property(ctx, ch, "fader");
            }
            if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                self.channels[ch].fader = (self.channels[ch].fader - fader_step).max(0.0);
                self.update_channel_property(ctx, ch, "fader");
            }

            // Left/Right = Adjust pan
            let pan_step = 0.1;
            if ui.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                self.channels[ch].pan = (self.channels[ch].pan - pan_step).max(-1.0);
                self.update_channel_property(ctx, ch, "pan");
            }
            if ui.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                self.channels[ch].pan = (self.channels[ch].pan + pan_step).min(1.0);
                self.update_channel_property(ctx, ch, "pan");
            }
        }
    }

    /// Update a channel property via API.
    fn update_channel_property(&mut self, ctx: &Context, index: usize, property: &str) {
        if !self.live_updates {
            return;
        }

        // Throttle updates
        if self.last_update.elapsed().as_millis() < 50 {
            return;
        }
        self.last_update = instant::Instant::now();

        let channel = &self.channels[index];

        // Map channel property to GStreamer element and property
        // The element_id format is "block_id:element_name"
        let (element_suffix, gst_prop, value) = match property {
            "pan" => (
                format!("pan_{}", index),
                "panorama",
                PropertyValue::Float(channel.pan as f64),
            ),
            "fader" => {
                // If muted, set volume to 0, otherwise use fader value
                let effective_volume = if channel.mute {
                    0.0
                } else {
                    channel.fader as f64
                };
                (
                    format!("volume_{}", index),
                    "volume",
                    PropertyValue::Float(effective_volume),
                )
            }
            "mute" => {
                // Mute is implemented by setting volume to 0
                let effective_volume = if channel.mute {
                    0.0
                } else {
                    channel.fader as f64
                };
                (
                    format!("volume_{}", index),
                    "volume",
                    PropertyValue::Float(effective_volume),
                )
            }
            "gate_enabled" => (
                format!("gate_{}", index),
                "enabled",
                PropertyValue::Bool(channel.gate_enabled),
            ),
            "comp_enabled" => (
                format!("comp_{}", index),
                "enabled",
                PropertyValue::Bool(channel.comp_enabled),
            ),
            "eq_enabled" => (
                format!("eq_{}", index),
                "enabled",
                PropertyValue::Bool(channel.eq_enabled),
            ),
            _ => return,
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:{}", self.block_id, element_suffix);
        let gst_prop = gst_prop.to_string();
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, &gst_prop, value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update main fader via API.
    fn update_main_fader(&mut self, ctx: &Context) {
        if !self.live_updates {
            return;
        }

        // Throttle updates
        if self.last_update.elapsed().as_millis() < 50 {
            return;
        }
        self.last_update = instant::Instant::now();

        // Apply mute: if muted, send 0, otherwise send fader value
        let effective_volume = if self.main_mute {
            0.0
        } else {
            self.main_fader as f64
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:main_volume", self.block_id);
        let value = PropertyValue::Float(effective_volume);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, "volume", value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update main mute via API.
    fn update_main_mute(&mut self, ctx: &Context) {
        if !self.live_updates {
            return;
        }

        // Mute is implemented by setting volume to 0
        let effective_volume = if self.main_mute {
            0.0
        } else {
            self.main_fader as f64
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:main_volume", self.block_id);
        let value = PropertyValue::Float(effective_volume);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, "volume", value)
                .await;
            ctx.request_repaint();
        });
    }
}

/// Convert dB to linear level (0.0-1.0).
fn db_to_level(db: f64) -> f32 {
    let min_db = -60.0;
    let max_db = 0.0;
    ((db - min_db) / (max_db - min_db)).clamp(0.0, 1.0) as f32
}

/// Get color for a level value.
fn level_to_color(level: f32) -> Color32 {
    if level < 0.7 {
        Color32::from_rgb(0, 200, 0) // Green
    } else if level < 0.85 {
        Color32::from_rgb(255, 220, 0) // Yellow
    } else if level < 0.9 {
        Color32::from_rgb(255, 165, 0) // Orange
    } else {
        Color32::from_rgb(255, 0, 0) // Red
    }
}

/// Convert dB to linear scale (f64).
fn db_to_linear_f64(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

/// Convert dB to linear scale (f32).
fn db_to_linear_f32(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Convert linear to dB scale.
fn linear_to_db(linear: f64) -> f64 {
    if linear <= 0.0001 {
        -60.0 // Clamp to -60dB for very small values
    } else {
        20.0 * linear.log10()
    }
}
