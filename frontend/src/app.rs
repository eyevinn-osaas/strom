//! Main application structure.

use egui::{CentralPanel, Color32, Context, SidePanel, TopBottomPanel};
use strom_types::{Flow, PipelineState};

use crate::api::ApiClient;
use crate::graph::GraphEditor;
use crate::palette::ElementPalette;
use crate::properties::PropertyInspector;
use crate::state::{AppMessage, AppStateChannels, ConnectionState};
use crate::ws::WebSocketClient;

// Cross-platform task spawning
#[cfg(target_arch = "wasm32")]
fn spawn_task<F>(future: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(future);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_task<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(future);
}

/// The main Strom application.
pub struct StromApp {
    /// API client for backend communication
    api: ApiClient,
    /// List of all flows
    flows: Vec<Flow>,
    /// Currently selected flow index
    selected_flow_idx: Option<usize>,
    /// Graph editor for the current flow
    graph: GraphEditor,
    /// Element palette
    palette: ElementPalette,
    /// Status message
    status: String,
    /// Error message
    error: Option<String>,
    /// Loading state
    loading: bool,
    /// Whether flow list needs refresh
    needs_refresh: bool,
    /// New flow name input
    new_flow_name: String,
    /// Show new flow dialog
    show_new_flow_dialog: bool,
    /// Whether elements have been loaded
    elements_loaded: bool,
    /// Whether blocks have been loaded
    blocks_loaded: bool,
    /// Flow pending deletion (for confirmation dialog)
    flow_pending_deletion: Option<(strom_types::FlowId, String)>,
    /// WebSocket client for real-time updates
    ws_client: Option<WebSocketClient>,
    /// Connection state
    connection_state: ConnectionState,
    /// Channel-based state management
    channels: AppStateChannels,
    /// Flow properties being edited (flow index)
    editing_properties_idx: Option<usize>,
    /// Temporary name buffer for properties dialog
    properties_name_buffer: String,
    /// Temporary description buffer for properties dialog
    properties_description_buffer: String,
    /// Temporary clock type for properties dialog
    properties_clock_type_buffer: strom_types::flow::GStreamerClockType,
    /// Temporary PTP domain buffer for properties dialog
    properties_ptp_domain_buffer: String,
    /// Shutdown flag for Ctrl+C handling (native mode only)
    #[cfg(not(target_arch = "wasm32"))]
    shutdown_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl StromApp {
    /// Create a new application instance.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Note: Dark theme is set in main.rs before creating the app

        // Detect API base URL - different logic for WASM vs native
        #[cfg(target_arch = "wasm32")]
        let api_base_url = {
            // WASM: Detect if we're in development mode (trunk serve) by checking the window location
            if let Some(window) = web_sys::window() {
                if let Ok(location) = window.location().host() {
                    // If we're on port 8080 (trunk serve), connect to backend on port 3000
                    if location.contains(":8080") {
                        "http://localhost:3000/api"
                    } else {
                        // Otherwise use relative URL (embedded in backend)
                        "/api"
                    }
                } else {
                    "/api"
                }
            } else {
                "/api"
            }
        };

        #[cfg(not(target_arch = "wasm32"))]
        let api_base_url = "http://localhost:3000/api";

        // Create channels for async communication
        let channels = AppStateChannels::new();

        let mut app = Self {
            api: ApiClient::new(api_base_url),
            flows: Vec::new(),
            selected_flow_idx: None,
            graph: GraphEditor::new(),
            palette: ElementPalette::new(),
            status: "Ready".to_string(),
            error: None,
            loading: false,
            needs_refresh: true,
            new_flow_name: String::new(),
            show_new_flow_dialog: false,
            elements_loaded: false,
            blocks_loaded: false,
            flow_pending_deletion: None,
            ws_client: None,
            connection_state: ConnectionState::Disconnected,
            channels,
            editing_properties_idx: None,
            properties_name_buffer: String::new(),
            properties_description_buffer: String::new(),
            properties_clock_type_buffer: strom_types::flow::GStreamerClockType::Monotonic,
            properties_ptp_domain_buffer: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            shutdown_flag: None,
        };

        // Load default elements temporarily (will be replaced by API data)
        app.palette.load_default_elements();

        // Set up WebSocket connection for real-time updates
        app.setup_websocket_connection(cc.egui_ctx.clone());

        app
    }

    /// Create a new application instance with shutdown handler (native mode only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_with_shutdown(
        cc: &eframe::CreationContext<'_>,
        shutdown_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        // Note: Dark theme is set in main.rs before creating the app

        // Detect API base URL - different logic for WASM vs native
        let api_base_url = "http://localhost:3000/api";

        // Create channels for async communication
        let channels = AppStateChannels::new();

        let mut app = Self {
            api: ApiClient::new(api_base_url),
            flows: Vec::new(),
            selected_flow_idx: None,
            graph: GraphEditor::new(),
            palette: ElementPalette::new(),
            status: "Ready".to_string(),
            error: None,
            loading: false,
            needs_refresh: true,
            new_flow_name: String::new(),
            show_new_flow_dialog: false,
            elements_loaded: false,
            blocks_loaded: false,
            flow_pending_deletion: None,
            ws_client: None,
            connection_state: ConnectionState::Disconnected,
            channels,
            editing_properties_idx: None,
            properties_name_buffer: String::new(),
            properties_description_buffer: String::new(),
            properties_clock_type_buffer: strom_types::flow::GStreamerClockType::Monotonic,
            properties_ptp_domain_buffer: String::new(),
            shutdown_flag: Some(shutdown_flag),
        };

        // Load default elements temporarily (will be replaced by API data)
        app.palette.load_default_elements();

        // Set up WebSocket connection for real-time updates
        app.setup_websocket_connection(cc.egui_ctx.clone());

        app
    }

    /// Set up WebSocket connection for real-time updates.
    fn setup_websocket_connection(&mut self, ctx: egui::Context) {
        tracing::info!("Setting up WebSocket connection for real-time updates");

        // WebSocket URL - different logic for WASM vs native
        #[cfg(target_arch = "wasm32")]
        let ws_url = {
            // WASM: Use the same URL detection logic as the API client
            if let Some(window) = web_sys::window() {
                if let Ok(location) = window.location().host() {
                    // If we're on port 8080 (trunk serve), connect to backend on port 3000
                    if location.contains(":8080") {
                        "ws://localhost:3000/api/ws".to_string()
                    } else {
                        // Otherwise use relative URL (embedded in backend)
                        // Determine ws:// or wss:// based on current protocol
                        if window.location().protocol().ok().as_deref() == Some("https:") {
                            format!("wss://{}/api/ws", location)
                        } else {
                            format!("ws://{}/api/ws", location)
                        }
                    }
                } else {
                    "/api/ws".to_string()
                }
            } else {
                "/api/ws".to_string()
            }
        };

        #[cfg(not(target_arch = "wasm32"))]
        let ws_url = "ws://localhost:3000/api/ws".to_string();

        tracing::info!("Connecting WebSocket to: {}", ws_url);
        let mut ws_client = WebSocketClient::new(ws_url);

        // Connect the WebSocket with the channel sender
        ws_client.connect(self.channels.sender(), ctx);

        // Store the WebSocket client to keep the connection alive
        self.ws_client = Some(ws_client);
    }

    /// Get the currently selected flow.
    fn current_flow(&self) -> Option<&Flow> {
        self.selected_flow_idx.and_then(|idx| self.flows.get(idx))
    }

    /// Get the currently selected flow mutably.
    fn current_flow_mut(&mut self) -> Option<&mut Flow> {
        self.selected_flow_idx
            .and_then(|idx| self.flows.get_mut(idx))
    }

    /// Load GStreamer elements from the backend.
    fn load_elements(&mut self, ctx: &Context) {
        tracing::info!("Starting to load GStreamer elements...");
        self.status = "Loading elements...".to_string();

        let api = self.api.clone();
        let tx = self.channels.sender();
        let ctx = ctx.clone();

        spawn_task(async move {
            match api.list_elements().await {
                Ok(elements) => {
                    tracing::info!("Successfully fetched {} elements", elements.len());
                    let _ = tx.send(AppMessage::ElementsLoaded(elements));
                }
                Err(e) => {
                    tracing::error!("Failed to load elements: {}", e);
                    let _ = tx.send(AppMessage::ElementsError(e.to_string()));
                }
            }
            ctx.request_repaint();
        });
    }

    /// Load blocks from the backend.
    fn load_blocks(&mut self, ctx: &Context) {
        tracing::info!("Starting to load blocks...");
        self.status = "Loading blocks...".to_string();

        let api = self.api.clone();
        let tx = self.channels.sender();
        let ctx = ctx.clone();

        spawn_task(async move {
            match api.list_blocks().await {
                Ok(blocks) => {
                    tracing::info!("Successfully fetched {} blocks", blocks.len());
                    let _ = tx.send(AppMessage::BlocksLoaded(blocks));
                }
                Err(e) => {
                    tracing::error!("Failed to load blocks: {}", e);
                    let _ = tx.send(AppMessage::BlocksError(e.to_string()));
                }
            }
            ctx.request_repaint();
        });
    }

    /// Load element properties from the backend (lazy loading).
    /// Properties are cached after first load.
    fn load_element_properties(&mut self, element_type: String, ctx: &Context) {
        tracing::info!("Starting to load properties for element: {}", element_type);

        let api = self.api.clone();
        let tx = self.channels.sender();
        let ctx = ctx.clone();

        spawn_task(async move {
            match api.get_element_info(&element_type).await {
                Ok(element_info) => {
                    tracing::info!(
                        "Successfully fetched properties for '{}' ({} properties)",
                        element_info.name,
                        element_info.properties.len()
                    );
                    let _ = tx.send(AppMessage::ElementPropertiesLoaded(element_info));
                }
                Err(e) => {
                    tracing::error!("Failed to load element properties: {}", e);
                    let _ = tx.send(AppMessage::ElementPropertiesError(e.to_string()));
                }
            }
            ctx.request_repaint();
        });
    }

    /// Load pad properties from the backend (on-demand lazy loading).
    /// Pad properties are cached separately after first load.
    fn load_element_pad_properties(&mut self, element_type: String, ctx: &Context) {
        tracing::info!(
            "Starting to load pad properties for element: {}",
            element_type
        );

        let api = self.api.clone();
        let tx = self.channels.sender();
        let ctx = ctx.clone();

        spawn_task(async move {
            match api.get_element_pad_properties(&element_type).await {
                Ok(element_info) => {
                    tracing::info!(
                        "Successfully fetched pad properties for '{}' (sink_pads: {}, src_pads: {})",
                        element_info.name,
                        element_info.sink_pads.iter().map(|p| p.properties.len()).sum::<usize>(),
                        element_info.src_pads.iter().map(|p| p.properties.len()).sum::<usize>()
                    );
                    let _ = tx.send(AppMessage::ElementPadPropertiesLoaded(element_info));
                }
                Err(e) => {
                    tracing::error!("Failed to load pad properties: {}", e);
                    let _ = tx.send(AppMessage::ElementPadPropertiesError(e.to_string()));
                }
            }
            ctx.request_repaint();
        });
    }

    /// Load flows from the backend.
    fn load_flows(&mut self, ctx: &Context) {
        if self.loading {
            return;
        }

        tracing::info!("Starting to load flows...");
        self.loading = true;
        self.status = "Loading flows...".to_string();
        self.error = None;

        let api = self.api.clone();
        let tx = self.channels.sender();
        let ctx = ctx.clone();

        spawn_task(async move {
            match api.list_flows().await {
                Ok(flows) => {
                    tracing::info!("Successfully fetched {} flows", flows.len());
                    let _ = tx.send(AppMessage::FlowsLoaded(flows));
                }
                Err(e) => {
                    tracing::error!("Failed to load flows: {}", e);
                    let _ = tx.send(AppMessage::FlowsError(e.to_string()));
                }
            }
            ctx.request_repaint();
        });
    }

    /// Save the current flow to the backend.
    fn save_current_flow(&mut self, ctx: &Context) {
        tracing::info!(
            "save_current_flow called, selected_flow_idx: {:?}",
            self.selected_flow_idx
        );

        if let Some(idx) = self.selected_flow_idx {
            // Update flow with current graph state
            if let Some(flow) = self.flows.get_mut(idx) {
                flow.elements = self.graph.elements.clone();
                flow.blocks = self.graph.blocks.clone();
                flow.links = self.graph.links.clone();

                tracing::info!(
                    "Preparing to save flow: id={}, name='{}', elements={}, links={}",
                    flow.id,
                    flow.name,
                    flow.elements.len(),
                    flow.links.len()
                );

                let flow_clone = flow.clone();
                let api = self.api.clone();
                let ctx = ctx.clone();

                self.status = "Saving flow...".to_string();

                spawn_task(async move {
                    tracing::info!("Starting async save operation for flow {}", flow_clone.id);
                    match api.update_flow(&flow_clone).await {
                        Ok(_) => {
                            tracing::info!(
                                "Flow saved successfully - WebSocket event will trigger refresh"
                            );
                        }
                        Err(e) => {
                            tracing::error!("Failed to save flow: {}", e);
                        }
                    }
                    ctx.request_repaint();
                });
            } else {
                tracing::warn!("save_current_flow: No flow found at index {}", idx);
            }
        } else {
            tracing::warn!("save_current_flow: No flow selected");
        }
    }

    /// Create a new flow.
    fn create_flow(&mut self, ctx: &Context) {
        if self.new_flow_name.is_empty() {
            self.error = Some("Flow name cannot be empty".to_string());
            return;
        }

        let new_flow = Flow::new(self.new_flow_name.clone());
        let api = self.api.clone();
        let ctx = ctx.clone();

        self.status = "Creating flow...".to_string();
        self.show_new_flow_dialog = false;
        self.new_flow_name.clear();

        spawn_task(async move {
            match api.create_flow(&new_flow).await {
                Ok(created_flow) => {
                    tracing::info!(
                        "Flow created successfully: {} - WebSocket event will trigger refresh",
                        created_flow.name
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to create flow: {}", e);
                }
            }
            ctx.request_repaint();
        });
    }

    /// Start the current flow.
    fn start_flow(&mut self, ctx: &Context) {
        if let Some(flow) = self.current_flow() {
            let flow_id = flow.id;
            let api = self.api.clone();
            let ctx = ctx.clone();

            self.status = "Starting flow...".to_string();

            spawn_task(async move {
                match api.start_flow(flow_id).await {
                    Ok(_) => {
                        tracing::info!(
                            "Flow started successfully - WebSocket event will trigger refresh"
                        );
                    }
                    Err(e) => {
                        tracing::error!("Failed to start flow: {}", e);
                    }
                }
                ctx.request_repaint();
            });
        }
    }

    /// Stop the current flow.
    fn stop_flow(&mut self, ctx: &Context) {
        if let Some(flow) = self.current_flow() {
            let flow_id = flow.id;
            let api = self.api.clone();
            let ctx = ctx.clone();

            self.status = "Stopping flow...".to_string();

            spawn_task(async move {
                match api.stop_flow(flow_id).await {
                    Ok(_) => {
                        tracing::info!(
                            "Flow stopped successfully - WebSocket event will trigger refresh"
                        );
                    }
                    Err(e) => {
                        tracing::error!("Failed to stop flow: {}", e);
                    }
                }
                ctx.request_repaint();
            });
        }
    }

    /// Delete a flow.
    fn delete_flow(&mut self, flow_id: strom_types::FlowId, ctx: &Context) {
        let api = self.api.clone();
        let ctx = ctx.clone();

        self.status = "Deleting flow...".to_string();

        spawn_task(async move {
            match api.delete_flow(flow_id).await {
                Ok(_) => {
                    tracing::info!(
                        "Flow deleted successfully - WebSocket event will trigger refresh"
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to delete flow: {}", e);
                }
            }
            ctx.request_repaint();
        });
    }

    /// Render the top toolbar.
    fn render_toolbar(&mut self, ctx: &Context) {
        TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("⚡ Strom");
                ui.separator();

                if ui.button("New Flow").clicked() {
                    self.show_new_flow_dialog = true;
                }

                if ui.button("Refresh").clicked() {
                    self.needs_refresh = true;
                }

                if ui.button("Save").clicked() {
                    self.save_current_flow(ctx);
                }

                ui.separator();

                // Flow controls
                let flow_info = self.current_flow().map(|f| (f.id, f.state));

                if let Some((flow_id, state)) = flow_info {
                    let state = state.unwrap_or(PipelineState::Null);

                    // Map internal states to user-friendly names
                    let (state_text, state_color) = match state {
                        PipelineState::Null | PipelineState::Ready => ("Stopped", Color32::GRAY),
                        PipelineState::Paused => ("Paused", Color32::from_rgb(255, 165, 0)),
                        PipelineState::Playing => ("Started", Color32::GREEN),
                    };

                    ui.colored_label(state_color, format!("State: {}", state_text));
                    ui.separator();

                    // Show Start or Restart button depending on state
                    let is_running = matches!(state, PipelineState::Playing);
                    let button_text = if is_running {
                        "🔄 Restart"
                    } else {
                        "▶ Start"
                    };

                    if ui.button(button_text).clicked() {
                        if is_running {
                            // For restart: stop first, then start
                            let api = self.api.clone();
                            let ctx_clone = ctx.clone();

                            self.status = "Restarting flow...".to_string();

                            spawn_task(async move {
                                // First stop the flow
                                match api.stop_flow(flow_id).await {
                                    Ok(_) => {
                                        tracing::info!("Flow stopped, now starting...");
                                        // Then start it again
                                        match api.start_flow(flow_id).await {
                                            Ok(_) => {
                                                tracing::info!("Flow restarted successfully - WebSocket events will trigger refresh");
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    "Failed to start flow after stop: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to stop flow for restart: {}", e);
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        } else {
                            self.start_flow(ctx);
                        }
                    }

                    if ui.button("⏸ Stop").clicked() {
                        self.stop_flow(ctx);
                    }

                    ui.separator();

                    if ui.button("🔍 Debug Graph").clicked() {
                        // Open debug graph in new tab (works on both WASM and native)
                        let url = self.api.get_debug_graph_url(flow_id);
                        ctx.open_url(egui::OpenUrl::new_tab(&url));
                    }

                    ui.separator();

                    if ui
                        .button("ℹ Help")
                        .on_hover_text("Show instructions")
                        .clicked()
                    {
                        self.error = None; // Clear any errors to show help
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label("GStreamer Flow Engine");
                });
            });
        });
    }

    /// Render the flow list sidebar.
    fn render_flow_list(&mut self, ctx: &Context) {
        SidePanel::left("flow_list")
            .default_width(200.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Flows");
                ui.separator();

                if self.flows.is_empty() {
                    ui.label("No flows yet");
                    ui.label("Click 'New Flow' to get started");
                } else {
                    // Create sorted list of (original_index, flow) tuples
                    let mut sorted_flows: Vec<(usize, &Flow)> =
                        self.flows.iter().enumerate().collect();
                    sorted_flows
                        .sort_by(|a, b| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()));

                    for (idx, flow) in sorted_flows {
                        let selected = self.selected_flow_idx == Some(idx);

                        // Create full-width selectable area
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 20.0),
                            egui::Sense::click(),
                        );

                        if response.clicked() {
                            // Select the flow
                            self.selected_flow_idx = Some(idx);
                            // Load flow into graph editor
                            self.graph.load(flow.elements.clone(), flow.links.clone());
                            self.graph.load_blocks(flow.blocks.clone());
                        }

                        // Draw background for selected/hovered item
                        if selected {
                            ui.painter()
                                .rect_filled(rect, 2.0, ui.visuals().selection.bg_fill);
                        } else if response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                2.0,
                                ui.visuals().widgets.hovered.bg_fill,
                            );
                        }

                        // Draw flow name and buttons
                        let mut child_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        child_ui.add_space(4.0);

                        let text_color = if selected {
                            ui.visuals().selection.stroke.color
                        } else {
                            ui.visuals().text_color()
                        };

                        // Show running state icon
                        let state_icon = match flow.state {
                            Some(PipelineState::Playing) => "▶",
                            Some(PipelineState::Paused) => "⏸",
                            Some(PipelineState::Ready) | Some(PipelineState::Null) | None => "⏹",
                        };
                        let state_color = match flow.state {
                            Some(PipelineState::Playing) => Color32::from_rgb(0, 200, 0),
                            Some(PipelineState::Paused) => Color32::from_rgb(255, 165, 0),
                            Some(PipelineState::Ready) | Some(PipelineState::Null) | None => {
                                Color32::GRAY
                            }
                        };
                        child_ui.colored_label(state_color, state_icon);
                        child_ui.add_space(4.0);

                        // Show flow name with hover tooltip - make it clickable too
                        let name_label = child_ui
                            .colored_label(text_color, &flow.name)
                            .interact(egui::Sense::click());

                        // Handle click on the text itself (in addition to the background)
                        if name_label.clicked() {
                            self.selected_flow_idx = Some(idx);
                            self.graph.load(flow.elements.clone(), flow.links.clone());
                            self.graph.load_blocks(flow.blocks.clone());
                        }

                        // Add hover tooltip with flow details
                        name_label.on_hover_ui(|ui| {
                            ui.label(egui::RichText::new(&flow.name).strong());
                            ui.separator();

                            if let Some(ref desc) = flow.properties.description {
                                if !desc.is_empty() {
                                    ui.label("Description:");
                                    ui.label(desc);
                                    ui.add_space(5.0);
                                }
                            }

                            ui.label(format!("Clock: {:?}", flow.properties.clock_type));

                            if let Some(domain) = flow.properties.ptp_domain {
                                ui.label(format!("PTP Domain: {}", domain));
                            }

                            if let Some(sync_status) = flow.properties.clock_sync_status {
                                use strom_types::flow::ClockSyncStatus;
                                let status_text = match sync_status {
                                    ClockSyncStatus::Synced => "Synced",
                                    ClockSyncStatus::NotSynced => "Not Synced",
                                    ClockSyncStatus::Unknown => "Unknown",
                                };
                                ui.label(format!("Sync Status: {}", status_text));
                            }

                            ui.add_space(5.0);
                            let state_text = match flow.state {
                                Some(PipelineState::Playing) => "Running",
                                Some(PipelineState::Paused) => "Paused",
                                Some(PipelineState::Ready) | Some(PipelineState::Null) | None => {
                                    "Stopped"
                                }
                            };
                            ui.label(format!("State: {}", state_text));
                        });

                        // Buttons on the right
                        child_ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.add_space(4.0);
                                if ui.small_button("🗑").on_hover_text("Delete flow").clicked() {
                                    self.flow_pending_deletion = Some((flow.id, flow.name.clone()));
                                }
                                if ui
                                    .small_button("⚙")
                                    .on_hover_text("Flow properties")
                                    .clicked()
                                {
                                    self.editing_properties_idx = Some(idx);
                                    self.properties_name_buffer = flow.name.clone();
                                    self.properties_description_buffer =
                                        flow.properties.description.clone().unwrap_or_default();
                                    self.properties_clock_type_buffer = flow.properties.clock_type;
                                    self.properties_ptp_domain_buffer = flow
                                        .properties
                                        .ptp_domain
                                        .map(|d| d.to_string())
                                        .unwrap_or_else(|| "0".to_string());
                                }

                                // Show clock type indicator (before settings gear)
                                use strom_types::flow::{ClockSyncStatus, GStreamerClockType};
                                let clock_label = match flow.properties.clock_type {
                                    GStreamerClockType::Ptp => Some("PTP"),
                                    GStreamerClockType::Ntp => Some("NTP"),
                                    GStreamerClockType::Realtime => Some("RT"),
                                    GStreamerClockType::PipelineDefault => Some("SYS"),
                                    GStreamerClockType::Monotonic => None,
                                };

                                if let Some(label) = clock_label {
                                    // Determine color based on sync status for PTP/NTP
                                    let text_color = match flow.properties.clock_type {
                                        GStreamerClockType::Ptp | GStreamerClockType::Ntp => {
                                            match flow.properties.clock_sync_status {
                                                Some(ClockSyncStatus::Synced) => {
                                                    Color32::from_rgb(0, 200, 0)
                                                }
                                                Some(ClockSyncStatus::NotSynced) => {
                                                    Color32::from_rgb(200, 0, 0)
                                                }
                                                _ => Color32::GRAY,
                                            }
                                        }
                                        _ => Color32::GRAY,
                                    };

                                    // Draw bordered text badge
                                    ui.add_space(2.0);
                                    egui::Frame::NONE
                                        .stroke(egui::Stroke::new(1.0, text_color))
                                        .inner_margin(egui::Margin::symmetric(2, 0))
                                        .corner_radius(1.0)
                                        .show(ui, |ui| {
                                            ui.add(egui::Label::new(
                                                egui::RichText::new(label)
                                                    .size(9.0)
                                                    .color(text_color),
                                            ));
                                        });
                                }
                            },
                        );
                    }
                }
            });
    }

    /// Render the element palette sidebar.
    fn render_palette(&mut self, ctx: &Context) {
        SidePanel::right("palette")
            .default_width(250.0)
            .resizable(true)
            .show(ctx, |ui| {
                // Check if an element is selected and trigger property loading if needed
                // Do this BEFORE getting mutable reference to avoid borrow checker issues
                if let Some((selected_element_type, active_tab)) = self
                    .graph
                    .get_selected_element()
                    .map(|e| (e.element_type.clone(), self.graph.active_property_tab))
                {
                    // Trigger lazy loading if properties not cached
                    if !self.palette.has_properties_cached(&selected_element_type) {
                        tracing::info!(
                            "Element '{}' selected but properties not cached, triggering lazy load",
                            selected_element_type
                        );
                        self.load_element_properties(selected_element_type.clone(), ctx);
                    }

                    // Trigger pad properties loading if on Input/Output Pads tabs
                    use crate::graph::PropertyTab;
                    if matches!(active_tab, PropertyTab::InputPads | PropertyTab::OutputPads)
                        && !self.palette.has_pad_properties_cached(&selected_element_type)
                    {
                        tracing::info!(
                            "Element '{}' showing pad tab but pad properties not cached, triggering lazy load",
                            selected_element_type
                        );
                        self.load_element_pad_properties(selected_element_type.clone(), ctx);
                    }
                }

                // Show either the palette or the property inspector, not both
                // Collect data BEFORE getting mutable reference to avoid borrow checker issues
                let selected_element_data = self.graph.get_selected_element().map(|element| {
                    let active_tab = self.graph.active_property_tab;

                    // Use pad properties if showing pad tabs, otherwise regular properties
                    use crate::graph::PropertyTab;
                    let element_info = if matches!(active_tab, PropertyTab::InputPads | PropertyTab::OutputPads) {
                        self.palette.get_element_info_with_pads(&element.element_type)
                    } else {
                        self.palette.get_element_info(&element.element_type)
                    };

                    let element_id = element.id.clone();
                    let focused_pad = self.graph.focused_pad.clone();
                    let input_pads = self.graph.get_actual_input_pads(&element_id);
                    let output_pads = self.graph.get_actual_output_pads(&element_id);
                    (element_info, active_tab, focused_pad, input_pads, output_pads)
                });

                if let Some((element_info, active_tab, focused_pad, input_pads, output_pads)) = selected_element_data {
                    // Element selected: show ONLY property inspector
                    ui.heading("Properties");
                    ui.separator();

                    // Split borrow: get mutable access to graph fields separately
                    let graph = &mut self.graph;
                    if let Some(element) = graph.get_selected_element_mut() {
                        graph.active_property_tab = PropertyInspector::show(
                            ui,
                            element,
                            element_info,
                            active_tab,
                            focused_pad,
                            input_pads,
                            output_pads,
                        );
                    }
                } else if let Some(block_def_id) = self
                    .graph
                    .get_selected_block()
                    .map(|b| b.block_definition_id.clone())
                {
                    // Block selected: show block property inspector
                    ui.heading("Block Properties");
                    ui.separator();

                    // Clone definition to avoid borrow checker issues
                    let definition_opt = self
                        .graph
                        .get_block_definition_by_id(&block_def_id)
                        .cloned();
                    let flow_id = self.current_flow().map(|f| f.id);

                    // Then get mutable reference to block
                    if let (Some(block), Some(def)) =
                        (self.graph.get_selected_block_mut(), definition_opt)
                    {
                        PropertyInspector::show_block(ui, block, &def, flow_id);
                    } else {
                        ui.label("Block definition not found");
                    }
                } else {
                    // No element or block selected: show ONLY the palette
                    self.palette.show(ui);
                }
            });
    }

    /// Render the main canvas area.
    fn render_canvas(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            if self.current_flow().is_some() {
                // Show compact instructions banner at the top
                egui::Frame::new()
                    .fill(Color32::from_rgb(40, 40, 50))
                    .inner_margin(4.0)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("💡");
                            ui.small("Click palette elements to add");
                            ui.separator();
                            ui.small("Drag orange→green to link");
                            ui.separator();
                            ui.small("Drag to move | Pan background | Scroll=zoom | Del=delete");
                        });
                    });

                ui.add_space(2.0);

                // Show graph editor
                let response = self.graph.show(ui);

                // Handle adding elements from palette
                if let Some(element_type) = self.palette.take_dragging_element() {
                    // Add element at center of visible area
                    let center = response.rect.center();
                    let world_pos = ((center - response.rect.min - self.graph.pan_offset)
                        / self.graph.zoom)
                        .to_pos2();
                    self.graph.add_element(element_type, world_pos);
                }

                // Handle adding blocks from palette
                if let Some(block_id) = self.palette.take_dragging_block() {
                    // Add block at center of visible area
                    let center = response.rect.center();
                    let world_pos = ((center - response.rect.min - self.graph.pan_offset)
                        / self.graph.zoom)
                        .to_pos2();
                    self.graph.add_block(block_id, world_pos);
                }

                // Handle delete key for elements and links
                if ui.input(|i| i.key_pressed(egui::Key::Delete)) {
                    self.graph.remove_selected(); // Remove selected element (if any)
                    self.graph.remove_selected_link(); // Remove selected link (if any)
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("Welcome to Strom");
                    ui.label("Select a flow from the sidebar or create a new one");
                });
            }
        });
    }

    /// Render the status bar.
    fn render_status_bar(&mut self, ctx: &Context) {
        TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.separator();
                ui.label(format!("Flows: {}", self.flows.len()));

                if let Some(error) = &self.error {
                    ui.separator();
                    ui.colored_label(Color32::RED, format!("Error: {}", error));
                }

                // Connection state is now shown via full-screen overlay when disconnected
            });
        });
    }

    /// Render the new flow dialog.
    fn render_new_flow_dialog(&mut self, ctx: &Context) {
        if !self.show_new_flow_dialog {
            return;
        }

        egui::Window::new("New Flow")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut self.new_flow_name);
                });

                // Check for Enter key to create flow
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.new_flow_name.is_empty() {
                    self.create_flow(ctx);
                }

                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() {
                        self.create_flow(ctx);
                    }

                    if ui.button("Cancel").clicked() {
                        self.show_new_flow_dialog = false;
                        self.new_flow_name.clear();
                    }
                });
            });
    }

    /// Render the delete confirmation dialog.
    fn render_delete_confirmation_dialog(&mut self, ctx: &Context) {
        if self.flow_pending_deletion.is_none() {
            return;
        }

        let (flow_id, flow_name) = self.flow_pending_deletion.as_ref().unwrap().clone();

        egui::Window::new("Delete Flow")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Are you sure you want to delete this flow?");
                ui.add_space(5.0);
                ui.colored_label(Color32::YELLOW, format!("Flow: {}", flow_name));
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui.button("❌ Delete").clicked() {
                        self.delete_flow(flow_id, ctx);
                        self.flow_pending_deletion = None;
                    }

                    if ui.button("Cancel").clicked() {
                        self.flow_pending_deletion = None;
                    }
                });
            });
    }

    /// Render the flow properties dialog.
    fn render_flow_properties_dialog(&mut self, ctx: &Context) {
        if self.editing_properties_idx.is_none() {
            return;
        }

        let idx = self.editing_properties_idx.unwrap();
        let flow = match self.flows.get(idx) {
            Some(f) => f,
            None => {
                self.editing_properties_idx = None;
                return;
            }
        };

        let flow_name = flow.name.clone();

        egui::Window::new(format!("⚙ {} - Properties", flow_name))
            .collapsible(false)
            .resizable(true)
            .default_width(400.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.heading("Flow Properties");
                ui.add_space(5.0);

                // Name
                ui.label("Name:");
                ui.text_edit_singleline(&mut self.properties_name_buffer);
                ui.add_space(10.0);

                // Description
                ui.label("Description:");
                ui.add(
                    egui::TextEdit::multiline(&mut self.properties_description_buffer)
                        .desired_width(f32::INFINITY)
                        .desired_rows(5)
                        .hint_text("Optional description for this flow..."),
                );

                ui.add_space(10.0);

                // Clock Type
                ui.label("GStreamer Clock Type:");
                ui.horizontal(|ui| {
                    use strom_types::flow::GStreamerClockType;

                    egui::ComboBox::from_id_salt("clock_type_selector")
                        .selected_text(format!("{:?}", self.properties_clock_type_buffer))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.properties_clock_type_buffer,
                                GStreamerClockType::Monotonic,
                                "Monotonic (recommended)",
                            );
                            ui.selectable_value(
                                &mut self.properties_clock_type_buffer,
                                GStreamerClockType::Realtime,
                                "Realtime",
                            );
                            ui.selectable_value(
                                &mut self.properties_clock_type_buffer,
                                GStreamerClockType::PipelineDefault,
                                "Pipeline Default",
                            );
                            ui.selectable_value(
                                &mut self.properties_clock_type_buffer,
                                GStreamerClockType::Ptp,
                                "PTP",
                            );
                            ui.selectable_value(
                                &mut self.properties_clock_type_buffer,
                                GStreamerClockType::Ntp,
                                "NTP",
                            );
                        });
                });

                // Show description of selected clock type
                ui.label(self.properties_clock_type_buffer.description());

                // Show PTP domain field only when PTP is selected
                if matches!(
                    self.properties_clock_type_buffer,
                    strom_types::flow::GStreamerClockType::Ptp
                ) {
                    ui.add_space(10.0);
                    ui.label("PTP Domain (0-255):");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.properties_ptp_domain_buffer)
                            .desired_width(100.0)
                            .hint_text("0"),
                    );
                    ui.label("The PTP domain for clock synchronization");
                }

                // Show clock sync status for PTP/NTP clocks
                if matches!(
                    self.properties_clock_type_buffer,
                    strom_types::flow::GStreamerClockType::Ptp
                        | strom_types::flow::GStreamerClockType::Ntp
                ) {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label("Clock Status:");
                        if let Some(flow) = self.flows.get(idx) {
                            if let Some(sync_status) = flow.properties.clock_sync_status {
                                use strom_types::flow::ClockSyncStatus;
                                match sync_status {
                                    ClockSyncStatus::Synced => {
                                        ui.colored_label(Color32::from_rgb(0, 200, 0), "● Synced");
                                    }
                                    ClockSyncStatus::NotSynced => {
                                        ui.colored_label(
                                            Color32::from_rgb(200, 0, 0),
                                            "● Not Synced",
                                        );
                                    }
                                    ClockSyncStatus::Unknown => {
                                        ui.colored_label(Color32::GRAY, "● Unknown");
                                    }
                                }
                            } else {
                                ui.colored_label(Color32::GRAY, "● Unknown");
                            }
                        }
                    });
                }

                ui.add_space(15.0);

                // Buttons
                ui.horizontal(|ui| {
                    if ui.button("💾 Save").clicked() {
                        // Update flow properties
                        if let Some(flow) = self.flows.get_mut(idx) {
                            // Update flow name
                            flow.name = self.properties_name_buffer.clone();

                            flow.properties.description =
                                if self.properties_description_buffer.is_empty() {
                                    None
                                } else {
                                    Some(self.properties_description_buffer.clone())
                                };
                            flow.properties.clock_type = self.properties_clock_type_buffer;

                            // Parse and set PTP domain if PTP clock is selected
                            flow.properties.ptp_domain = if matches!(
                                self.properties_clock_type_buffer,
                                strom_types::flow::GStreamerClockType::Ptp
                            ) {
                                self.properties_ptp_domain_buffer.parse::<u8>().ok()
                            } else {
                                None
                            };

                            let flow_clone = flow.clone();
                            let api = self.api.clone();
                            let ctx_clone = ctx.clone();

                            spawn_task(async move {
                                match api.update_flow(&flow_clone).await {
                                    Ok(_) => {
                                        tracing::info!("Flow properties updated successfully - WebSocket event will trigger refresh");
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to update flow properties: {}", e);
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        self.editing_properties_idx = None;
                    }

                    if ui.button("Cancel").clicked() {
                        self.editing_properties_idx = None;
                    }
                });
            });
    }

    /// Render the full-screen disconnect overlay when WebSocket is not connected.
    fn render_disconnect_overlay(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            // Center everything vertically and horizontally
            ui.vertical_centered(|ui| {
                // Add vertical spacing to center content
                let available_height = ui.available_height();
                ui.add_space(available_height * 0.35);

                // Show large icon and status based on connection state
                match self.connection_state {
                    ConnectionState::Disconnected => {
                        ui.heading(
                            egui::RichText::new("⚠")
                                .size(80.0)
                                .color(Color32::from_rgb(255, 165, 0))
                        );
                        ui.add_space(20.0);
                        ui.heading(
                            egui::RichText::new("Disconnected from Backend")
                                .size(32.0)
                                .color(Color32::from_rgb(200, 200, 200))
                        );
                    }
                    ConnectionState::Reconnecting { attempt } => {
                        // Animated spinner
                        ui.add(egui::Spinner::new().size(80.0));
                        ui.add_space(20.0);
                        ui.heading(
                            egui::RichText::new(format!("Reconnecting (Attempt {})", attempt))
                                .size(32.0)
                                .color(Color32::from_rgb(200, 200, 200))
                        );
                    }
                    ConnectionState::Connected => {
                        // Should not reach here, but just in case
                        ui.heading(
                            egui::RichText::new("✓")
                                .size(80.0)
                                .color(Color32::from_rgb(0, 200, 0))
                        );
                        ui.add_space(20.0);
                        ui.heading(
                            egui::RichText::new("Connected")
                                .size(32.0)
                                .color(Color32::from_rgb(200, 200, 200))
                        );
                    }
                }

                ui.add_space(15.0);
                ui.label(
                    egui::RichText::new("Please wait while we attempt to reconnect to the Strom backend...")
                        .size(16.0)
                        .color(Color32::from_rgb(150, 150, 150))
                );

                ui.add_space(30.0);
                ui.separator();
                ui.add_space(10.0);

                // Show connection details
                ui.label(
                    egui::RichText::new("The application will automatically reconnect when the backend is available.")
                        .size(14.0)
                        .color(Color32::from_rgb(120, 120, 120))
                );
            });
        });
    }
}

impl eframe::App for StromApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Check shutdown flag (Ctrl+C handler for native mode)
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(ref flag) = self.shutdown_flag {
            use std::sync::atomic::Ordering;
            if flag.load(Ordering::SeqCst) {
                tracing::info!("Shutdown flag set, closing GUI...");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }

        // Process all pending channel messages
        while let Ok(msg) = self.channels.rx.try_recv() {
            match msg {
                AppMessage::FlowsLoaded(flows) => {
                    tracing::info!("Received FlowsLoaded: {} flows", flows.len());
                    self.flows = flows;
                    self.status = format!("Loaded {} flows", self.flows.len());
                    self.loading = false;
                }
                AppMessage::FlowsError(error) => {
                    tracing::error!("Received FlowsError: {}", error);
                    self.error = Some(format!("Flows: {}", error));
                    self.loading = false;
                    self.status = "Error loading flows".to_string();
                }
                AppMessage::ElementsLoaded(elements) => {
                    let count = elements.len();
                    tracing::info!("Received ElementsLoaded: {} elements", count);
                    self.palette.load_elements(elements.clone());
                    self.graph.set_all_element_info(elements);
                    self.status = format!("Loaded {} elements", count);
                }
                AppMessage::ElementsError(error) => {
                    tracing::error!("Received ElementsError: {}", error);
                    self.error = Some(format!("Elements: {}", error));
                }
                AppMessage::BlocksLoaded(blocks) => {
                    let count = blocks.len();
                    tracing::info!("Received BlocksLoaded: {} blocks", count);
                    self.palette.load_blocks(blocks.clone());
                    self.graph.set_all_block_definitions(blocks);
                    self.status = format!("Loaded {} blocks", count);
                }
                AppMessage::BlocksError(error) => {
                    tracing::error!("Received BlocksError: {}", error);
                    self.error = Some(format!("Blocks: {}", error));
                }
                AppMessage::ElementPropertiesLoaded(info) => {
                    tracing::info!(
                        "Received ElementPropertiesLoaded: {} ({} properties)",
                        info.name,
                        info.properties.len()
                    );
                    self.palette.cache_element_properties(info);
                }
                AppMessage::ElementPropertiesError(error) => {
                    tracing::error!("Received ElementPropertiesError: {}", error);
                    self.error = Some(format!("Element properties: {}", error));
                }
                AppMessage::ElementPadPropertiesLoaded(info) => {
                    let sink_prop_count: usize =
                        info.sink_pads.iter().map(|p| p.properties.len()).sum();
                    let src_prop_count: usize =
                        info.src_pads.iter().map(|p| p.properties.len()).sum();
                    tracing::info!(
                        "Received ElementPadPropertiesLoaded: {} (sink: {} props, src: {} props)",
                        info.name,
                        sink_prop_count,
                        src_prop_count
                    );
                    self.palette.cache_element_pad_properties(info);
                }
                AppMessage::ElementPadPropertiesError(error) => {
                    tracing::error!("Received ElementPadPropertiesError: {}", error);
                    self.error = Some(format!("Pad properties: {}", error));
                }
                AppMessage::Event(event) => {
                    tracing::info!("Received WebSocket event: {}", event.description());
                    // Handle flow state changes
                    use strom_types::StromEvent;
                    match event {
                        StromEvent::FlowCreated { .. } | StromEvent::FlowDeleted { .. } => {
                            // Only refresh flow list for create/delete events
                            tracing::info!("Flow created or deleted, triggering full refresh");
                            self.needs_refresh = true;
                        }
                        StromEvent::FlowUpdated { flow_id }
                        | StromEvent::FlowStarted { flow_id }
                        | StromEvent::FlowStopped { flow_id } => {
                            // For updates/start/stop, fetch the specific flow to update it in-place
                            tracing::info!(
                                "Flow {} updated/started/stopped, fetching updated flow",
                                flow_id
                            );
                            let api = self.api.clone();
                            let tx = self.channels.sender();
                            let ctx = ctx.clone();

                            spawn_task(async move {
                                match api.get_flow(flow_id).await {
                                    Ok(flow) => {
                                        tracing::info!("Fetched updated flow: {}", flow.name);
                                        let _ = tx.send(AppMessage::FlowFetched(flow));
                                        ctx.request_repaint();
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to fetch updated flow: {}", e);
                                        // Fall back to full refresh
                                        let _ = tx.send(AppMessage::RefreshNeeded);
                                        ctx.request_repaint();
                                    }
                                }
                            });
                        }
                        StromEvent::PipelineError { error, .. } => {
                            self.error = Some(format!("Pipeline error: {}", error));
                        }
                        _ => {}
                    }
                }
                AppMessage::ConnectionStateChanged(state) => {
                    tracing::info!("Connection state changed: {:?}", state);

                    // If we're transitioning to Connected state, invalidate all cached data
                    let was_disconnected = !self.connection_state.is_connected();
                    let now_connected = state.is_connected();

                    if was_disconnected && now_connected {
                        tracing::info!("Reconnected to backend - invalidating all cached state");
                        // Trigger reload of all data from backend
                        self.needs_refresh = true;
                        self.elements_loaded = false;
                        self.blocks_loaded = false;
                    }

                    self.connection_state = state;
                }
                AppMessage::FlowFetched(flow) => {
                    tracing::info!("Received updated flow: {} (id={})", flow.name, flow.id);

                    // Check if this is the currently selected flow BEFORE updating
                    let current_flow_id = self.current_flow().map(|f| f.id);
                    let is_selected_flow = current_flow_id == Some(flow.id);

                    tracing::info!(
                        "Current selected flow: {:?}, Fetched flow: {}, Is selected: {}",
                        current_flow_id,
                        flow.id,
                        is_selected_flow
                    );

                    // Log runtime_data for AES67 blocks
                    for block in &flow.blocks {
                        if block.block_definition_id == "builtin.aes67_output" {
                            let has_sdp = block
                                .runtime_data
                                .as_ref()
                                .and_then(|data| data.get("sdp"))
                                .is_some();
                            tracing::info!("AES67 block {} has SDP: {}", block.id, has_sdp);
                        }
                    }

                    // Update the specific flow in-place
                    if let Some(existing_flow) = self.flows.iter_mut().find(|f| f.id == flow.id) {
                        *existing_flow = flow.clone();
                        tracing::info!("Updated flow in self.flows");

                        // If this is the currently selected flow, update the graph editor in-place
                        if is_selected_flow {
                            tracing::info!("This is the selected flow - updating graph editor");

                            // Log before update
                            for block in &self.graph.blocks {
                                if block.block_definition_id == "builtin.aes67_output" {
                                    tracing::info!(
                                        "BEFORE UPDATE: Graph block {} has runtime_data: {}",
                                        block.id,
                                        block.runtime_data.is_some()
                                    );
                                }
                            }

                            // Update the graph editor's data to match the updated flow
                            // This ensures property inspector sees the latest runtime_data
                            self.graph.elements = flow.elements.clone();
                            self.graph.links = flow.links.clone();
                            self.graph.blocks = flow.blocks.clone();

                            // Log after update
                            for block in &self.graph.blocks {
                                if block.block_definition_id == "builtin.aes67_output" {
                                    tracing::info!(
                                        "AFTER UPDATE: Graph block {} has runtime_data: {}",
                                        block.id,
                                        block.runtime_data.is_some()
                                    );
                                }
                            }

                            tracing::info!(
                                "Graph editor updated with {} blocks",
                                flow.blocks.len()
                            );
                        } else {
                            tracing::info!("Not the selected flow - skipping graph editor update");
                        }
                    } else {
                        tracing::warn!("Flow not found in list, adding it");
                        self.flows.push(flow);
                    }
                }
                AppMessage::RefreshNeeded => {
                    tracing::info!("Refresh requested due to flow fetch failure");
                    self.needs_refresh = true;
                }
                _ => {
                    tracing::debug!("Received unhandled AppMessage variant");
                }
            }
        }

        // Check if we're disconnected - if so, show blocking overlay and don't render normal UI
        if !self.connection_state.is_connected() {
            self.render_disconnect_overlay(ctx);
            return;
        }

        // Load elements on first frame
        if !self.elements_loaded {
            self.load_elements(ctx);
            self.elements_loaded = true;
        }

        // Load blocks on first frame
        if !self.blocks_loaded {
            self.load_blocks(ctx);
            self.blocks_loaded = true;
        }

        // Load flows on first frame or when refresh is needed
        if self.needs_refresh {
            self.load_flows(ctx);
            self.needs_refresh = false;
        }

        self.render_toolbar(ctx);
        self.render_flow_list(ctx);

        // Always show palette, even if no flow selected
        if self.current_flow().is_some() {
            self.render_palette(ctx);
        } else {
            // Show simplified palette when no flow is selected
            SidePanel::right("palette")
                .default_width(250.0)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.heading("Elements");
                    ui.separator();
                    ui.label("Select or create a flow to see the element palette");
                });
        }

        self.render_canvas(ctx);
        self.render_status_bar(ctx);
        self.render_new_flow_dialog(ctx);
        self.render_delete_confirmation_dialog(ctx);
        self.render_flow_properties_dialog(ctx);
    }
}
