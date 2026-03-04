//! Links page for quick access to WHEP players, SRT streams, and API endpoints.

use egui::{Context, Ui};
use std::collections::HashSet;
use strom_types::{Flow, PropertyValue};

use crate::api::ApiClient;
use crate::app::{download_file, escape_xml, generate_vlc_playlist};
use crate::qr::QrCache;

/// Information about an SRT listener stream.
struct SrtListenerInfo {
    flow_name: String,
    srt_uri: String,
}

/// Tab selection for Links page.
#[derive(Default, Clone, Copy, PartialEq)]
enum LinksTab {
    #[default]
    Whep,
    Srt,
    Api,
}

/// Links page state.
pub struct LinksPage {
    selected_tab: LinksTab,
    /// URLs for which the QR code is currently shown.
    qr_visible: HashSet<String>,
    /// Cached QR code textures.
    qr_cache: QrCache,
}

impl LinksPage {
    pub fn new() -> Self {
        Self {
            selected_tab: LinksTab::default(),
            qr_visible: HashSet::new(),
            qr_cache: QrCache::new(),
        }
    }

    /// Extract SRT listener streams from flows.
    fn get_srt_listeners(flows: &[Flow]) -> Vec<SrtListenerInfo> {
        let mut listeners = Vec::new();

        for flow in flows {
            for block in &flow.blocks {
                if block.block_definition_id == "builtin.mpegtssrt_output" {
                    if let Some(PropertyValue::String(srt_uri)) = block.properties.get("srt_uri") {
                        if srt_uri.contains("mode=listener") {
                            listeners.push(SrtListenerInfo {
                                flow_name: flow.name.clone(),
                                srt_uri: srt_uri.clone(),
                            });
                        }
                    }
                }
            }
        }

        listeners
    }

    /// Generate a combined VLC playlist for all SRT listeners.
    fn generate_combined_playlist(listeners: &[SrtListenerInfo]) -> String {
        let mut tracks = String::new();

        for listener in listeners {
            let vlc_uri = crate::app::transform_srt_uri_for_vlc(&listener.srt_uri);
            let escaped_uri = escape_xml(&vlc_uri);
            let escaped_title = escape_xml(&format!("{} ({})", listener.flow_name, vlc_uri));

            tracks.push_str(&format!(
                r#"    <track>
      <location>{}</location>
      <title>{}</title>
      <extension application="http://www.videolan.org/vlc/playlist/0">
        <vlc:option>network-caching=1000</vlc:option>
      </extension>
    </track>
"#,
                escaped_uri, escaped_title
            ));
        }

        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist xmlns="http://xspf.org/ns/0/" xmlns:vlc="http://www.videolan.org/vlc/playlist/ns/0/" version="1">
  <title>Strom SRT Streams</title>
  <trackList>
{}  </trackList>
</playlist>
"#,
            tracks
        )
    }

    /// Render the links page.
    pub fn render(
        &mut self,
        ui: &mut Ui,
        api: &ApiClient,
        ctx: &Context,
        flows: &[Flow],
        server_hostname: Option<&str>,
    ) {
        let server_base = api.base_url().trim_end_matches("/api");

        ui.add_space(8.0);

        // Tab bar
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.selected_tab, LinksTab::Whep, "WHIP/WHEP");
            ui.selectable_value(&mut self.selected_tab, LinksTab::Srt, "MPEG-TS/SRT");
            ui.selectable_value(&mut self.selected_tab, LinksTab::Api, "API");
        });

        ui.separator();

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(16.0);

                match self.selected_tab {
                    LinksTab::Whep => Self::render_whep_tab(
                        ui,
                        ctx,
                        server_base,
                        &mut self.qr_visible,
                        &mut self.qr_cache,
                        server_hostname,
                    ),
                    LinksTab::Srt => self.render_srt_tab(ui, ctx, flows),
                    LinksTab::Api => self.render_api_tab(ui, ctx, server_base),
                }
            });
    }

    fn render_whep_tab(
        ui: &mut Ui,
        ctx: &Context,
        server_base: &str,
        qr_visible: &mut HashSet<String>,
        qr_cache: &mut QrCache,
        server_hostname: Option<&str>,
    ) {
        ui.heading("WHIP/WHEP");
        ui.add_space(8.0);
        ui.label("WebRTC ingest (WHIP) and playback (WHEP) for low-latency streaming.");
        ui.add_space(16.0);

        // WHIP Ingest
        egui::Frame::group(ui.style())
            .inner_margin(12.0)
            .show(ui, |ui| {
                let ingest_url = format!("{}/player/whip-ingest", server_base);
                ui.horizontal(|ui| {
                    if ui
                        .link(egui::RichText::new("WHIP Ingest").strong())
                        .on_hover_text(&ingest_url)
                        .clicked()
                    {
                        ctx.open_url(egui::OpenUrl::new_tab(&ingest_url));
                    }
                    if ui
                        .small_button(egui_phosphor::regular::COPY)
                        .on_hover_text("Copy URL to clipboard")
                        .clicked()
                    {
                        crate::clipboard::copy_text_with_ctx(ctx, &ingest_url);
                    }
                    Self::qr_toggle_button(ui, &ingest_url, qr_visible);
                });

                if qr_visible.contains(&ingest_url) {
                    Self::show_inline_qr(ui, ctx, &ingest_url, qr_cache, server_hostname);
                }

                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Send audio/video from a browser to a Strom flow via WebRTC.",
                    )
                    .weak(),
                );
            });

        ui.add_space(12.0);

        // Combined streams player
        egui::Frame::group(ui.style())
            .inner_margin(12.0)
            .show(ui, |ui| {
                let streams_url = format!("{}/player/whep-streams", server_base);
                ui.horizontal(|ui| {
                    if ui
                        .link(egui::RichText::new("WHEP Streams").strong())
                        .on_hover_text(&streams_url)
                        .clicked()
                    {
                        ctx.open_url(egui::OpenUrl::new_tab(&streams_url));
                    }
                    if ui
                        .small_button(egui_phosphor::regular::COPY)
                        .on_hover_text("Copy URL to clipboard")
                        .clicked()
                    {
                        crate::clipboard::copy_text_with_ctx(ctx, &streams_url);
                    }
                    Self::qr_toggle_button(ui, &streams_url, qr_visible);
                });

                if qr_visible.contains(&streams_url) {
                    Self::show_inline_qr(ui, ctx, &streams_url, qr_cache, server_hostname);
                }

                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Page showing all active WHEP streams with mini-players.")
                        .weak(),
                );
            });

        ui.add_space(12.0);

        // Individual player base URL
        egui::Frame::group(ui.style())
            .inner_margin(12.0)
            .show(ui, |ui| {
                let player_base = format!("{}/player/whep", server_base);
                ui.horizontal(|ui| {
                    if ui
                        .link(egui::RichText::new("Single Stream Player").strong())
                        .on_hover_text(&player_base)
                        .clicked()
                    {
                        ctx.open_url(egui::OpenUrl::new_tab(&player_base));
                    }
                    if ui
                        .small_button(egui_phosphor::regular::COPY)
                        .on_hover_text("Copy URL to clipboard")
                        .clicked()
                    {
                        crate::clipboard::copy_text_with_ctx(ctx, &player_base);
                    }
                    Self::qr_toggle_button(ui, &player_base, qr_visible);
                });

                if qr_visible.contains(&player_base) {
                    Self::show_inline_qr(ui, ctx, &player_base, qr_cache, server_hostname);
                }

                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Use with ?endpoint=/whep/<endpoint_id> parameter.\n\
                         Individual player URLs are available from WHEP Output block properties.",
                    )
                    .weak(),
                );
            });
    }

    /// Render a QR toggle button. Call inside a `ui.horizontal` block.
    fn qr_toggle_button(ui: &mut Ui, url: &str, qr_visible: &mut HashSet<String>) {
        let is_visible = qr_visible.contains(url);
        if ui
            .small_button(egui_phosphor::regular::QR_CODE)
            .on_hover_text("Toggle QR code for mobile access")
            .clicked()
        {
            if is_visible {
                qr_visible.remove(url);
            } else {
                qr_visible.insert(url.to_string());
            }
        }
    }

    /// Show an inline QR code image below the URL.
    /// The QR code encodes the external URL (localhost replaced with hostname).
    fn show_inline_qr(
        ui: &mut Ui,
        ctx: &Context,
        url: &str,
        qr_cache: &mut QrCache,
        server_hostname: Option<&str>,
    ) {
        let external_url = crate::app::make_external_url(url, server_hostname);
        ui.add_space(4.0);
        if let Some(texture) = qr_cache.get_or_create(ctx, &external_url) {
            ui.image(egui::load::SizedTexture::new(
                texture.id(),
                egui::vec2(160.0, 160.0),
            ));
        }
        if external_url != url {
            ui.label(
                egui::RichText::new(&external_url)
                    .monospace()
                    .small()
                    .weak(),
            );
        }
    }

    fn render_srt_tab(&self, ui: &mut Ui, ctx: &Context, flows: &[Flow]) {
        ui.heading("MPEG-TS/SRT Streams");
        ui.add_space(8.0);
        ui.label("SRT listener streams that can be played with VLC or other players.");
        ui.add_space(16.0);

        let mut listeners = Self::get_srt_listeners(flows);
        listeners.sort_by(|a, b| a.flow_name.cmp(&b.flow_name));

        egui::Frame::group(ui.style())
            .inner_margin(12.0)
            .show(ui, |ui| {
                if listeners.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "No SRT listener streams configured.\n\n\
                             Add an MPEGTSSRT Output block with mode=listener to see streams here.",
                        )
                        .weak(),
                    );
                } else {
                    // Header with download all button
                    ui.horizontal(|ui| {
                        ui.strong(format!(
                            "{} stream{} available",
                            listeners.len(),
                            if listeners.len() == 1 { "" } else { "s" }
                        ));

                        if ui
                            .button("Download All (VLC)")
                            .on_hover_text("Download a VLC playlist containing all SRT streams")
                            .clicked()
                        {
                            let content = Self::generate_combined_playlist(&listeners);
                            download_file(
                                "strom-srt-streams.xspf",
                                &content,
                                "application/xspf+xml",
                            );
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // List individual streams
                    for listener in &listeners {
                        let client_uri = crate::app::transform_srt_uri_for_vlc(&listener.srt_uri);

                        ui.horizontal(|ui| {
                            ui.label(&listener.flow_name);

                            if ui
                                .small_button(egui_phosphor::regular::COPY)
                                .on_hover_text(&client_uri)
                                .clicked()
                            {
                                crate::clipboard::copy_text_with_ctx(ctx, &client_uri);
                            }

                            if ui
                                .small_button("VLC")
                                .on_hover_text("Download VLC playlist")
                                .clicked()
                            {
                                let content = generate_vlc_playlist(
                                    &listener.srt_uri,
                                    1000,
                                    &listener.flow_name,
                                );
                                let safe_name: String = listener
                                    .flow_name
                                    .chars()
                                    .map(|c| {
                                        if c.is_alphanumeric() || c == '-' || c == '_' {
                                            c
                                        } else {
                                            '_'
                                        }
                                    })
                                    .collect();
                                download_file(
                                    &format!("{}.xspf", safe_name),
                                    &content,
                                    "application/xspf+xml",
                                );
                            }
                        });
                    }
                }
            });
    }

    fn render_api_tab(&self, ui: &mut Ui, ctx: &Context, server_base: &str) {
        ui.heading("API Documentation");
        ui.add_space(8.0);
        ui.label("REST API endpoints and documentation.");
        ui.add_space(16.0);

        // Documentation section
        egui::Frame::group(ui.style())
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.strong("Documentation");
                ui.add_space(8.0);

                let swagger_url = format!("{}/swagger-ui/", server_base);
                Self::link_row(ui, ctx, "Swagger UI", &swagger_url);
                ui.add_space(4.0);
                let openapi_url = format!("{}/api-docs/openapi.json", server_base);
                Self::link_row(ui, ctx, "OpenAPI Spec", &openapi_url);
            });

        ui.add_space(12.0);

        // Endpoints section
        egui::Frame::group(ui.style())
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.strong("Endpoints");
                ui.add_space(8.0);

                let endpoints = [
                    ("Flows", format!("{}/api/flows", server_base)),
                    ("WHEP Streams", format!("{}/api/whep-streams", server_base)),
                    ("Version", format!("{}/api/version", server_base)),
                    ("Blocks", format!("{}/api/blocks", server_base)),
                    ("Elements", format!("{}/api/elements", server_base)),
                ];

                for (i, (label, url)) in endpoints.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(4.0);
                    }
                    Self::link_row(ui, ctx, label, url);
                }
            });
    }

    /// Render a link row: clickable label (hover shows URL) + copy button.
    fn link_row(ui: &mut Ui, ctx: &Context, label: &str, url: &str) {
        ui.horizontal(|ui| {
            if ui.link(label).on_hover_text(url).clicked() {
                ctx.open_url(egui::OpenUrl::new_tab(url));
            }
            if ui
                .small_button(egui_phosphor::regular::COPY)
                .on_hover_text("Copy URL to clipboard")
                .clicked()
            {
                crate::clipboard::copy_text_with_ctx(ctx, url);
            }
        });
    }
}
