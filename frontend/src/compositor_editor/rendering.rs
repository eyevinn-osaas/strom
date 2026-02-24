use super::*;

impl CompositorEditor {
    /// Show the compositor editor in fullscreen mode (for Live view).
    /// Renders directly into the provided UI without a window frame.
    pub fn show_fullscreen(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        // Check for loaded properties and status updates
        self.check_loaded_properties();
        self.check_update_results();
        self.check_transition_status();
        self.check_loaded_thumbnails(ctx);
        self.refresh_thumbnails(ctx);

        // Keyboard shortcuts for setting transition target (0-9)
        for (key, idx) in [
            (egui::Key::Num0, 0),
            (egui::Key::Num1, 1),
            (egui::Key::Num2, 2),
            (egui::Key::Num3, 3),
            (egui::Key::Num4, 4),
            (egui::Key::Num5, 5),
            (egui::Key::Num6, 6),
            (egui::Key::Num7, 7),
            (egui::Key::Num8, 8),
            (egui::Key::Num9, 9),
        ] {
            if ui.input(|i| i.key_pressed(key)) && idx < self.inputs.len() {
                self.transition_to = idx;
                self.toggle_input_selection(idx);
            }
        }
        // § on Swedish keyboard OR ` on US keyboard (left of 1) - input 0
        if ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Text(t) if t == "§" || t == "`"))
        }) && !self.inputs.is_empty()
        {
            self.transition_to = 0;
            self.toggle_input_selection(0);
        }

        // Space = Trigger transition (Go)
        if ui.input(|i| i.key_pressed(egui::Key::Space)) {
            self.trigger_transition(ctx);
        }

        // Esc = Deselect
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.deselect_input();
        }

        // Keyboard shortcuts for layout actions (when input is selected)
        if let Some(idx) = self.selected_input {
            let out_w = self.output_width as i32;
            let out_h = self.output_height as i32;

            // F = Fullscreen
            if ui.input(|i| i.key_pressed(egui::Key::F)) {
                self.set_input_position(ctx, idx, 0, 0);
                self.set_input_size(ctx, idx, out_w, out_h);
            }
            // R = Reset input
            if ui.input(|i| i.key_pressed(egui::Key::R)) {
                self.reset_input(ctx, idx, out_w, out_h);
            }
            // Home = Send to back (z=0)
            if ui.input(|i| i.key_pressed(egui::Key::Home)) {
                self.inputs[idx].zorder = 0;
                if self.live_updates {
                    self.update_pad_property(ctx, idx, "zorder", PropertyValue::UInt(0));
                }
            }
            // End = Bring to front
            if ui.input(|i| i.key_pressed(egui::Key::End)) {
                let max_z = self.inputs.iter().map(|i| i.zorder).max().unwrap_or(0);
                self.inputs[idx].zorder = max_z + 1;
                if self.live_updates {
                    self.update_pad_property(
                        ctx,
                        idx,
                        "zorder",
                        PropertyValue::UInt(self.inputs[idx].zorder as u64),
                    );
                }
            }
            // PageDown = Move down one layer
            if ui.input(|i| i.key_pressed(egui::Key::PageDown)) && self.inputs[idx].zorder > 0 {
                self.inputs[idx].zorder -= 1;
                if self.live_updates {
                    self.update_pad_property(
                        ctx,
                        idx,
                        "zorder",
                        PropertyValue::UInt(self.inputs[idx].zorder as u64),
                    );
                }
            }
            // PageUp = Move up one layer
            if ui.input(|i| i.key_pressed(egui::Key::PageUp)) {
                self.inputs[idx].zorder += 1;
                if self.live_updates {
                    self.update_pad_property(
                        ctx,
                        idx,
                        "zorder",
                        PropertyValue::UInt(self.inputs[idx].zorder as u64),
                    );
                }
            }
        }

        // Toolbar row - Settings, templates, and input selection
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.snap_to_grid, "Snap");
            if self.snap_to_grid {
                ui.add(
                    egui::DragValue::new(&mut self.grid_size)
                        .prefix("")
                        .suffix("px"),
                );
            }
            ui.separator();

            ui.checkbox(&mut self.live_updates, "Live");
            ui.checkbox(&mut self.animate_moves, "Animate")
                .on_hover_text("Animate position/size changes");

            if !self.live_updates && ui.button("Apply").clicked() {
                self.apply_all_properties(ctx);
            }

            ui.separator();

            // Layout templates dropdown
            let mut template_applied = false;
            egui::ComboBox::from_id_salt("layout_templates_fullscreen")
                .selected_text("Templates")
                .show_ui(ui, |ui| {
                    if ui.selectable_label(false, "Multiview (2+4+N)").clicked() {
                        self.apply_template_multiview();
                        template_applied = true;
                    }
                    if ui
                        .selectable_label(false, "Full Screen (Input 0)")
                        .clicked()
                    {
                        self.apply_template_fullscreen();
                        template_applied = true;
                    }
                    if ui.selectable_label(false, "Picture-in-Picture").clicked() {
                        self.apply_template_pip();
                        template_applied = true;
                    }
                    if ui.selectable_label(false, "Side by Side").clicked() {
                        self.apply_template_side_by_side();
                        template_applied = true;
                    }
                    if ui.selectable_label(false, "Top / Bottom").clicked() {
                        self.apply_template_top_bottom();
                        template_applied = true;
                    }
                    if ui.selectable_label(false, "2x2 Grid").clicked() {
                        self.apply_template_grid_2x2();
                        template_applied = true;
                    }
                    if ui.selectable_label(false, "3x3 Grid").clicked() {
                        self.apply_template_grid_3x3();
                        template_applied = true;
                    }
                    if ui.selectable_label(false, "1 Large + 2 Small").clicked() {
                        self.apply_template_1_large_2_small();
                        template_applied = true;
                    }
                    if ui.selectable_label(false, "1 Large + 3 Small").clicked() {
                        self.apply_template_1_large_3_small();
                        template_applied = true;
                    }
                    if ui.selectable_label(false, "Vertical Stack").clicked() {
                        self.apply_template_vertical_stack();
                        template_applied = true;
                    }
                    if ui.selectable_label(false, "Horizontal Stack").clicked() {
                        self.apply_template_horizontal_stack();
                        template_applied = true;
                    }
                });

            if template_applied && self.live_updates {
                self.apply_all_properties(ctx);
            }

            ui.separator();

            // Input selection buttons
            for idx in 0..self.inputs.len() {
                let is_selected = self.selected_input == Some(idx);
                let color = self.inputs[idx].color();
                let button = egui::Button::new(format!("{}", idx))
                    .fill(if is_selected {
                        color
                    } else {
                        Color32::from_gray(60)
                    })
                    .min_size(Vec2::new(24.0, 18.0));
                if ui.add(button).clicked() {
                    self.toggle_input_selection(idx);
                }
            }

            // Deselect button
            if self.selected_input.is_some()
                && ui
                    .add(egui::Button::new("x").min_size(Vec2::new(18.0, 18.0)))
                    .on_hover_text("Deselect (Esc)")
                    .clicked()
            {
                self.deselect_input();
            }

            ui.separator();
            ui.label(format!("{}x{}", self.output_width, self.output_height));
        });

        // Transitions row
        ui.horizontal(|ui| {
            ui.label("Transition:");

            // From input selector
            egui::ComboBox::from_id_salt("transition_from_fullscreen")
                .selected_text(format!("From: {}", self.transition_from))
                .width(70.0)
                .show_ui(ui, |ui| {
                    for idx in 0..self.inputs.len() {
                        if ui
                            .selectable_label(self.transition_from == idx, format!("{}", idx))
                            .clicked()
                        {
                            self.transition_from = idx;
                        }
                    }
                });

            // To input selector
            egui::ComboBox::from_id_salt("transition_to_fullscreen")
                .selected_text(format!("To: {}", self.transition_to))
                .width(70.0)
                .show_ui(ui, |ui| {
                    for idx in 0..self.inputs.len() {
                        if ui
                            .selectable_label(self.transition_to == idx, format!("{}", idx))
                            .clicked()
                        {
                            self.transition_to = idx;
                        }
                    }
                });

            // Transition type selector
            const TRANSITION_TYPES: &[(&str, &str)] = &[
                ("cut", "Cut"),
                ("fade", "Fade"),
                ("dip_to_black", "Dip to Black"),
                ("slide_left", "Slide Left"),
                ("slide_right", "Slide Right"),
                ("slide_up", "Slide Up"),
                ("slide_down", "Slide Down"),
                ("push_left", "Push Left"),
                ("push_right", "Push Right"),
                ("push_up", "Push Up"),
                ("push_down", "Push Down"),
            ];
            let selected_label = TRANSITION_TYPES
                .iter()
                .find(|(v, _)| *v == self.transition_type)
                .map(|(_, l)| *l)
                .unwrap_or(&self.transition_type);
            egui::ComboBox::from_id_salt("transition_type_fullscreen")
                .selected_text(selected_label)
                .width(90.0)
                .show_ui(ui, |ui| {
                    for (value, label) in TRANSITION_TYPES {
                        if ui
                            .selectable_label(self.transition_type == *value, *label)
                            .clicked()
                        {
                            self.transition_type = value.to_string();
                        }
                    }
                });

            // Duration slider
            ui.add(
                egui::Slider::new(&mut self.transition_duration_ms, 100..=3000)
                    .suffix("ms")
                    .logarithmic(true),
            );

            // Go button
            let can_go = self.transition_from != self.transition_to;
            if ui
                .add_enabled(can_go, egui::Button::new("Go"))
                .on_hover_text(if can_go {
                    format!(
                        "{} {} → {} ({}ms) [Space]",
                        self.transition_type,
                        self.transition_from,
                        self.transition_to,
                        self.transition_duration_ms
                    )
                } else {
                    "Select different from/to inputs".to_string()
                })
                .clicked()
            {
                let _ = self.trigger_transition(ctx);
            }

            // Swap button
            if ui.button("<>").on_hover_text("Swap from/to").clicked() {
                std::mem::swap(&mut self.transition_from, &mut self.transition_to);
            }

            // Status message
            if let Some(status) = &self.transition_status {
                ui.separator();
                ui.label(status);
            }
        });

        ui.separator();

        // Canvas and properties panel
        let remaining = ui.available_size();
        let properties_width = 200.0;
        let spacing = 8.0;
        let canvas_width = (remaining.x - properties_width - spacing).max(100.0);
        let content_height = remaining.y.max(100.0);

        ui.horizontal(|ui| {
            // Canvas area
            ui.group(|ui| {
                ui.set_min_size(Vec2::new(canvas_width, content_height));
                ui.set_max_size(Vec2::new(canvas_width, content_height));
                self.show_canvas(ui);
            });

            // Properties panel
            ui.group(|ui| {
                ui.set_min_size(Vec2::new(properties_width, content_height));
                ui.set_max_size(Vec2::new(properties_width, content_height));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(idx) = self.selected_input {
                        self.show_properties_panel(ui, idx);
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label("Select an input");
                            ui.label("to edit properties");
                        });
                    }
                });
            });
        });
    }
}
