use super::*;

impl MixerEditor {
    /// Render the detail panel for the current selection.
    pub(super) fn render_detail_panel(&mut self, ui: &mut Ui, ctx: &Context) {
        match self.selection.clone() {
            Some(Selection::Channel(index)) => {
                self.render_channel_detail_panel(ui, ctx, index);
            }
            Some(Selection::Main) => {
                self.render_main_detail_panel(ui, ctx);
            }
            None => {}
        }
    }

    /// Render the detail panel for a selected channel (HPF/Gate/Comp/EQ).
    pub(super) fn render_channel_detail_panel(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
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
                            self.selection = None;
                        }
                    });
                });

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    // Gain section
                    self.render_gain_section(ui, ctx, index);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // HPF section
                    self.render_hpf_section(ui, ctx, index);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

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

    /// Render the detail panel for the main bus (Comp/EQ/Limiter).
    pub(super) fn render_main_detail_panel(&mut self, ui: &mut Ui, ctx: &Context) {
        egui::Frame::default()
            .fill(Color32::from_rgb(35, 35, 40))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("MAIN - Processing")
                            .strong()
                            .color(Color32::from_rgb(200, 200, 255)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            self.selection = None;
                        }
                    });
                });

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    // Compressor section
                    self.render_main_comp_section(ui, ctx);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // EQ section
                    self.render_main_eq_section(ui, ctx);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // Limiter section
                    self.render_main_limiter_section(ui, ctx);
                });
            });
    }

    /// Render main bus compressor controls.
    pub(super) fn render_main_comp_section(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.vertical(|ui| {
            let enabled = self.main_comp_enabled;
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
                if ui.checkbox(&mut self.main_comp_enabled, "").changed() {
                    self.update_main_processing_param(ctx, "comp", "enabled");
                }
                if ui.small_button("Reset").clicked() {
                    self.main_comp_enabled = false;
                    self.main_comp_threshold = DEFAULT_COMP_THRESHOLD;
                    self.main_comp_ratio = DEFAULT_COMP_RATIO;
                    self.main_comp_attack = DEFAULT_COMP_ATTACK;
                    self.main_comp_release = DEFAULT_COMP_RELEASE;
                    self.main_comp_makeup = DEFAULT_COMP_MAKEUP;
                    self.main_comp_knee = DEFAULT_COMP_KNEE;
                    self.update_main_processing_param(ctx, "comp", "enabled");
                    self.update_main_processing_param(ctx, "comp", "threshold");
                    self.update_main_processing_param(ctx, "comp", "ratio");
                    self.update_main_processing_param(ctx, "comp", "attack");
                    self.update_main_processing_param(ctx, "comp", "release");
                    self.update_main_processing_param(ctx, "comp", "makeup");
                    self.update_main_processing_param(ctx, "comp", "knee");
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
                        egui::DragValue::new(&mut self.main_comp_threshold)
                            .range(-60.0..=0.0)
                            .suffix(" dB")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "threshold");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Ratio:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_ratio)
                            .range(1.0..=20.0)
                            .suffix(":1")
                            .speed(0.1),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "ratio");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Attack:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_attack)
                            .range(0.1..=200.0)
                            .suffix(" ms")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "attack");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Release:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_release)
                            .range(10.0..=1000.0)
                            .suffix(" ms")
                            .speed(1.0),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "release");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Makeup:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_makeup)
                            .range(0.0..=24.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "makeup");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Knee:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_knee)
                            .range(-24.0..=0.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "knee");
                }
            });
        });
    }

    /// Render main bus EQ controls.
    pub(super) fn render_main_eq_section(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.vertical(|ui| {
            let enabled = self.main_eq_enabled;
            let header_color = if enabled {
                Color32::from_rgb(0, 100, 180)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("EQ").color(header_color).strong());
                if ui.checkbox(&mut self.main_eq_enabled, "").changed() {
                    self.update_main_processing_param(ctx, "eq", "enabled");
                }
                if ui.small_button("Reset").clicked() {
                    self.main_eq_enabled = false;
                    self.main_eq_bands = DEFAULT_EQ_BANDS;
                    self.update_main_processing_param(ctx, "eq", "enabled");
                    for band in 0..4 {
                        self.update_main_eq_param(ctx, band, "freq");
                        self.update_main_eq_param(ctx, band, "gain");
                        self.update_main_eq_param(ctx, band, "q");
                    }
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

                        if ui
                            .add(
                                egui::DragValue::new(&mut self.main_eq_bands[band].0)
                                    .range(20.0..=20000.0)
                                    .suffix(" Hz")
                                    .speed(10.0),
                            )
                            .changed()
                        {
                            self.update_main_eq_param(ctx, band, "freq");
                        }

                        if ui
                            .add(
                                egui::DragValue::new(&mut self.main_eq_bands[band].1)
                                    .range(-15.0..=15.0)
                                    .suffix(" dB")
                                    .speed(0.1),
                            )
                            .changed()
                        {
                            self.update_main_eq_param(ctx, band, "gain");
                        }

                        if ui
                            .add(
                                egui::DragValue::new(&mut self.main_eq_bands[band].2)
                                    .range(0.1..=10.0)
                                    .prefix("Q ")
                                    .speed(0.05),
                            )
                            .changed()
                        {
                            self.update_main_eq_param(ctx, band, "q");
                        }
                    });

                    if band < 3 {
                        ui.add_space(8.0);
                    }
                }
            });
        });
    }

    /// Render main bus limiter controls.
    pub(super) fn render_main_limiter_section(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.vertical(|ui| {
            let enabled = self.main_limiter_enabled;
            let header_color = if enabled {
                Color32::from_rgb(200, 60, 60)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("LIMITER").color(header_color).strong());
                if ui.checkbox(&mut self.main_limiter_enabled, "").changed() {
                    self.update_main_processing_param(ctx, "limiter", "enabled");
                }
                if ui.small_button("Reset").clicked() {
                    self.main_limiter_enabled = false;
                    self.main_limiter_threshold = DEFAULT_LIMITER_THRESHOLD;
                    self.update_main_processing_param(ctx, "limiter", "enabled");
                    self.update_main_processing_param(ctx, "limiter", "threshold");
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
                        egui::DragValue::new(&mut self.main_limiter_threshold)
                            .range(-20.0..=0.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "limiter", "threshold");
                }
            });
        });
    }

    /// Render input gain control.
    pub(super) fn render_gain_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("GAIN")
                        .color(Color32::from_rgb(200, 200, 200))
                        .strong(),
                );
                if ui.small_button("Reset").clicked() {
                    self.channels[index].gain = DEFAULT_GAIN;
                    self.update_channel_property(ctx, index, "gain");
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Gain:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].gain)
                            .range(-20.0..=20.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_channel_property(ctx, index, "gain");
                }
            });
        });
    }

    /// Render HPF controls.
    pub(super) fn render_hpf_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        ui.vertical(|ui| {
            let enabled = self.channels[index].hpf_enabled;
            let header_color = if enabled {
                Color32::from_rgb(150, 80, 150)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("HPF").color(header_color).strong());
                if ui
                    .checkbox(&mut self.channels[index].hpf_enabled, "")
                    .changed()
                {
                    self.update_processing_param(ctx, index, "hpf", "enabled");
                }
                if ui.small_button("Reset").clicked() {
                    self.channels[index].hpf_enabled = false;
                    self.channels[index].hpf_freq = DEFAULT_HPF_FREQ;
                    self.update_processing_param(ctx, index, "hpf", "enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            ui.horizontal(|ui| {
                ui.label("Cutoff:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].hpf_freq)
                            .range(20.0..=500.0)
                            .suffix(" Hz")
                            .speed(1.0),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "hpf", "freq");
                }
            });
        });
    }

    /// Render Gate controls.
    pub(super) fn render_gate_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
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
                if ui.small_button("Reset").clicked() {
                    self.channels[index].gate_enabled = false;
                    self.channels[index].gate_threshold = DEFAULT_GATE_THRESHOLD;
                    self.channels[index].gate_attack = DEFAULT_GATE_ATTACK;
                    self.channels[index].gate_release = DEFAULT_GATE_RELEASE;
                    self.update_channel_property(ctx, index, "gate_enabled");
                    self.update_processing_param(ctx, index, "gate", "threshold");
                    self.update_processing_param(ctx, index, "gate", "attack");
                    self.update_processing_param(ctx, index, "gate", "release");
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
    pub(super) fn render_comp_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
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
                if ui.small_button("Reset").clicked() {
                    self.channels[index].comp_enabled = false;
                    self.channels[index].comp_threshold = DEFAULT_COMP_THRESHOLD;
                    self.channels[index].comp_ratio = DEFAULT_COMP_RATIO;
                    self.channels[index].comp_attack = DEFAULT_COMP_ATTACK;
                    self.channels[index].comp_release = DEFAULT_COMP_RELEASE;
                    self.channels[index].comp_makeup = DEFAULT_COMP_MAKEUP;
                    self.channels[index].comp_knee = DEFAULT_COMP_KNEE;
                    self.update_channel_property(ctx, index, "comp_enabled");
                    self.update_processing_param(ctx, index, "comp", "threshold");
                    self.update_processing_param(ctx, index, "comp", "ratio");
                    self.update_processing_param(ctx, index, "comp", "attack");
                    self.update_processing_param(ctx, index, "comp", "release");
                    self.update_processing_param(ctx, index, "comp", "makeup");
                    self.update_processing_param(ctx, index, "comp", "knee");
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

            ui.horizontal(|ui| {
                ui.label("Knee:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_knee)
                            .range(-24.0..=0.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "knee");
                }
            });
        });
    }

    /// Render EQ controls.
    pub(super) fn render_eq_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
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
                if ui.small_button("Reset").clicked() {
                    self.channels[index].eq_enabled = false;
                    self.channels[index].eq_bands = DEFAULT_EQ_BANDS;
                    self.update_channel_property(ctx, index, "eq_enabled");
                    for band in 0..4 {
                        self.update_eq_param(ctx, index, band, "freq");
                        self.update_eq_param(ctx, index, band, "gain");
                        self.update_eq_param(ctx, index, band, "q");
                    }
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
}
