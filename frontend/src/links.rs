//! Links page for quick access to WHEP players and streaming endpoints.

use egui::{Context, Ui};

use crate::api::ApiClient;

/// Links page state.
pub struct LinksPage;

impl LinksPage {
    pub fn new() -> Self {
        Self
    }

    /// Render the links page.
    pub fn render(&mut self, ui: &mut Ui, api: &ApiClient, ctx: &Context) {
        let server_base = api.base_url().trim_end_matches("/api");

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(16.0);

                ui.heading("Streaming Links");
                ui.add_space(8.0);

                ui.label("Quick access to WHEP players and streaming endpoints.");
                ui.add_space(16.0);

                // WHEP Players section
                egui::Frame::group(ui.style())
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.heading("WHEP Players");
                        ui.add_space(8.0);

                        // Combined streams player
                        ui.horizontal(|ui| {
                            ui.label("All Streams Player:");
                            let streams_url = format!("{}/player/whep-streams", server_base);
                            if ui.link(&streams_url).clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(&streams_url));
                            }
                            if ui.button("Copy").clicked() {
                                ctx.copy_text(streams_url.clone());
                            }
                        });

                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "Opens a page showing all active WHEP streams with mini-players.",
                            )
                            .weak(),
                        );

                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(12.0);

                        // Individual player base URL
                        ui.horizontal(|ui| {
                            ui.label("Single Stream Player:");
                            let player_base = format!("{}/player/whep", server_base);
                            ui.label(egui::RichText::new(&player_base).monospace());
                            if ui.button("Copy").clicked() {
                                ctx.copy_text(player_base);
                            }
                        });

                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "Use with ?endpoint=/whep/<endpoint_id> parameter.\n\
                                 Individual player URLs are available from WHEP Output block properties.",
                            )
                            .weak(),
                        );
                    });

                ui.add_space(16.0);

                // API Endpoints section
                egui::Frame::group(ui.style())
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.heading("API Endpoints");
                        ui.add_space(8.0);

                        // WHEP streams list
                        ui.horizontal(|ui| {
                            ui.label("WHEP Streams API:");
                            let streams_api = format!("{}/api/whep-streams", server_base);
                            if ui.link(&streams_api).clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(&streams_api));
                            }
                            if ui.button("Copy").clicked() {
                                ctx.copy_text(streams_api.clone());
                            }
                        });

                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("Returns JSON list of all active WHEP endpoints.")
                                .weak(),
                        );
                    });
            });
    }
}
