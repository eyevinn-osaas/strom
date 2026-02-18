use crate::compositor_editor::CompositorEditor;
use crate::state::ConnectionState;
use egui::{CentralPanel, Color32, Context, TopBottomPanel};

use super::ThemePreference;
use super::*;
impl StromApp {
    /// Enter Live mode for a specific compositor block.
    pub(super) fn enter_live_mode(
        &mut self,
        flow_id: strom_types::FlowId,
        block_id: String,
        ctx: &Context,
    ) {
        // Find the flow and block to get compositor parameters
        if let Some(flow) = self.flows.iter().find(|f| f.id == flow_id) {
            if let Some(block) = flow.blocks.iter().find(|b| b.id == block_id) {
                // Extract resolution from output_resolution property
                let (output_width, output_height) = block
                    .properties
                    .get("output_resolution")
                    .and_then(|v| match v {
                        strom_types::PropertyValue::String(s) if !s.is_empty() => {
                            strom_types::parse_resolution_string(s)
                        }
                        _ => None,
                    })
                    .unwrap_or((1920, 1080));

                let num_inputs = block
                    .properties
                    .get("num_inputs")
                    .and_then(|v| match v {
                        strom_types::PropertyValue::UInt(u) => Some(*u as usize),
                        strom_types::PropertyValue::Int(i) if *i > 0 => Some(*i as usize),
                        _ => None,
                    })
                    .unwrap_or(2);

                // Create editor
                let mut editor = CompositorEditor::new(
                    flow_id,
                    block_id.clone(),
                    output_width,
                    output_height,
                    num_inputs,
                    self.api.clone(),
                );

                // Load current properties from backend
                editor.load_properties(ctx);

                self.compositor_editor = Some(editor);
            }
        }

        // Switch to Live mode
        self.app_mode = AppMode::Live { flow_id, block_id };
        tracing::info!(
            "Entered Live mode for flow {} block {:?}",
            flow_id,
            self.app_mode
        );
    }

    /// Exit Live mode and return to Admin mode.
    pub(super) fn exit_live_mode(&mut self) {
        self.app_mode = AppMode::Admin;
        self.compositor_editor = None;
        self.mixer_editor = None;
        tracing::info!("Exited Live mode");
    }

    /// Render the Live UI (minimal top bar + full-screen compositor editor).
    pub(super) fn render_live_ui(
        &mut self,
        ctx: &Context,
        flow_id: strom_types::FlowId,
        block_id: &str,
    ) {
        // Ensure editor exists (compositor or mixer)
        if self.compositor_editor.is_none() && self.mixer_editor.is_none() {
            self.enter_live_mode(flow_id, block_id.to_string(), ctx);
        }

        // Determine view type for title
        let is_mixer = self.mixer_editor.is_some();

        // Get flow and block names for display
        let flow_name = self
            .flows
            .iter()
            .find(|f| f.id == flow_id)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "Unknown Flow".to_string());

        // Top bar with back button and info
        TopBottomPanel::top("live_bar")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Back button (only show if we didn't start in live mode via URL)
                    if !self.started_in_live_mode {
                        if ui
                            .button("Back")
                            .on_hover_text("Return to admin interface")
                            .clicked()
                        {
                            self.exit_live_mode();
                        }
                        ui.separator();
                    }

                    // Title
                    let title = if is_mixer { "Live Audio" } else { "Live View" };
                    ui.label(egui::RichText::new(title).strong().size(16.0));

                    ui.separator();

                    // Flow and block info
                    ui.label(&flow_name);
                    ui.label("›");
                    ui.label(block_id);

                    // Right side: connection status, theme picker, and copy URL button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Copy URL button (WASM only)
                        #[cfg(target_arch = "wasm32")]
                        {
                            if ui
                                .button("📋 Copy URL")
                                .on_hover_text("Copy live URL to clipboard")
                                .clicked()
                            {
                                if let Some(window) = web_sys::window() {
                                    if let Ok(location) = window.location().origin() {
                                        let url =
                                            format!("{}/live/{}/{}", location, flow_id, block_id);
                                        crate::clipboard::copy_text_with_ctx(ui.ctx(), &url);
                                        tracing::info!("Copied live URL: {}", url);
                                    }
                                }
                            }
                        }

                        ui.separator();

                        // Theme picker
                        let theme_name = match self.settings.theme {
                            ThemePreference::EguiDark => "Dark",
                            ThemePreference::EguiLight => "Light",
                            ThemePreference::NordDark => "Nord Dark",
                            ThemePreference::NordLight => "Nord Light",
                            ThemePreference::TokyoNight => "Tokyo Night",
                            ThemePreference::TokyoNightStorm => "Tokyo Storm",
                            ThemePreference::TokyoNightLight => "Tokyo Light",
                            ThemePreference::ClaudeDark => "Claude Dark",
                            ThemePreference::ClaudeLight => "Claude Light",
                        };

                        egui::ComboBox::from_id_salt("live_theme_selector")
                            .selected_text(theme_name)
                            .show_ui(ui, |ui| {
                                let themes = [
                                    (ThemePreference::EguiDark, "Dark (default)"),
                                    (ThemePreference::EguiLight, "Light (default)"),
                                    (ThemePreference::ClaudeDark, "Claude Dark"),
                                    (ThemePreference::ClaudeLight, "Claude Light"),
                                    (ThemePreference::NordDark, "Nord Dark"),
                                    (ThemePreference::NordLight, "Nord Light"),
                                    (ThemePreference::TokyoNight, "Tokyo Night"),
                                    (ThemePreference::TokyoNightStorm, "Tokyo Night Storm"),
                                    (ThemePreference::TokyoNightLight, "Tokyo Night Light"),
                                ];
                                for (theme, label) in themes {
                                    if ui
                                        .selectable_label(self.settings.theme == theme, label)
                                        .clicked()
                                    {
                                        self.settings.theme = theme;
                                        self.apply_theme(ctx.clone());
                                    }
                                }
                            });

                        ui.separator();

                        // Connection status indicator
                        let (status_color, status_text) = match self.connection_state {
                            ConnectionState::Connected => (Color32::GREEN, "Connected"),
                            ConnectionState::Reconnecting { .. } => {
                                (Color32::YELLOW, "Reconnecting")
                            }
                            ConnectionState::Disconnected => (Color32::RED, "Disconnected"),
                        };
                        ui.label(egui::RichText::new(status_text).color(status_color).small());
                    });
                });
            });

        // Full-screen editor (compositor or mixer)
        if let Some(ref mut editor) = self.compositor_editor {
            CentralPanel::default().show(ctx, |ui| {
                editor.show_fullscreen(ui, ctx);
            });
        } else if let Some(ref mut editor) = self.mixer_editor {
            CentralPanel::default().show(ctx, |ui| {
                editor.show_fullscreen(ui, ctx, &self.meter_data);
            });

            // Handle mixer save request
            if editor.needs_save() {
                let props = if editor.is_reset() {
                    editor.collect_structural_properties()
                } else {
                    editor.collect_properties()
                };
                editor.clear_save();
                let block_id = editor.block_id().to_string();
                let flow_id = editor.flow_id();

                if let Some(flow) = self.flows.iter_mut().find(|f| f.id == flow_id) {
                    if let Some(block) = flow.blocks.iter_mut().find(|b| b.id == block_id) {
                        block.properties = props;
                    }
                    let flow_clone = flow.clone();
                    let api = self.api.clone();
                    let ctx_clone = ctx.clone();
                    spawn_task(async move {
                        match api.update_flow(&flow_clone).await {
                            Ok(_) => {
                                tracing::info!("Mixer state saved");
                                set_local_storage("mixer_save_status", "ok");
                            }
                            Err(e) => {
                                tracing::error!("Failed to save mixer state: {}", e);
                                set_local_storage("mixer_save_status", &format!("error: {}", e));
                            }
                        }
                        ctx_clone.request_repaint();
                    });
                }
            }
        } else {
            // Show loading state
            CentralPanel::default().show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                    ui.label("Loading...");
                });
            });
        }
    }
}
