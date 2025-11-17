//! Main application structure.

use egui::{CentralPanel, Color32, Context, SidePanel, TopBottomPanel};
use strom_types::element::ElementInfo;
use strom_types::{Flow, PipelineState};
use wasm_bindgen_futures::spawn_local;

use crate::api::ApiClient;
use crate::graph::GraphEditor;
use crate::palette::ElementPalette;
use crate::properties::PropertyInspector;
use crate::sse::SseClient;

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
    /// SSE client for real-time updates
    sse_client: Option<SseClient>,
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
}

impl StromApp {
    /// Create a new application instance.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Note: Dark theme is set in main.rs before creating the app

        // Detect if we're in development mode (trunk serve) by checking the window location
        let api_base_url = if let Some(window) = web_sys::window() {
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
        };

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
            sse_client: None,
            editing_properties_idx: None,
            properties_name_buffer: String::new(),
            properties_description_buffer: String::new(),
            properties_clock_type_buffer: strom_types::flow::GStreamerClockType::Monotonic,
            properties_ptp_domain_buffer: String::new(),
        };

        // Load default elements temporarily (will be replaced by API data)
        app.palette.load_default_elements();

        // Set up SSE connection for real-time updates
        app.setup_sse_connection(cc.egui_ctx.clone());

        app
    }

    /// Set up Server-Sent Events connection for real-time updates.
    fn setup_sse_connection(&mut self, ctx: egui::Context) {
        tracing::info!("Setting up SSE connection for real-time updates");

        let mut sse_client = SseClient::new("/api/events");

        // Connect with event handler
        sse_client.connect(move |event| {
            tracing::info!("Received SSE event: {}", event.description());

            // Store event in localStorage to trigger UI update
            if let Some(window) = web_sys::window() {
                if let Some(storage) = window.local_storage().ok().flatten() {
                    use strom_types::StromEvent;

                    match event {
                        StromEvent::FlowCreated { .. }
                        | StromEvent::FlowUpdated { .. }
                        | StromEvent::FlowDeleted { .. } => {
                            // Trigger flow list refresh
                            let _ = storage.set_item("strom_needs_refresh", "true");
                            tracing::info!("SSE: Triggering flow list refresh");
                        }
                        StromEvent::FlowStarted { .. }
                        | StromEvent::FlowStopped { .. }
                        | StromEvent::FlowStateChanged { .. } => {
                            // Trigger flow list refresh to update state
                            let _ = storage.set_item("strom_needs_refresh", "true");
                            tracing::info!("SSE: Triggering flow state refresh");
                        }
                        StromEvent::PipelineError {
                            flow_id,
                            error,
                            source,
                        } => {
                            // Log error prominently and store for display
                            if let Some(ref src) = source {
                                tracing::error!(
                                    "Pipeline error in flow {} from {}: {}",
                                    flow_id,
                                    src,
                                    error
                                );
                            } else {
                                tracing::error!("Pipeline error in flow {}: {}", flow_id, error);
                            }
                            // Store error for UI display
                            let error_msg = if let Some(ref src) = source {
                                format!("Pipeline error from {}: {}", src, error)
                            } else {
                                format!("Pipeline error: {}", error)
                            };
                            let _ = storage.set_item("strom_pipeline_error", &error_msg);
                        }
                        StromEvent::PipelineWarning {
                            flow_id,
                            warning,
                            source,
                        } => {
                            // Log warning
                            if let Some(src) = source {
                                tracing::warn!(
                                    "Pipeline warning in flow {} from {}: {}",
                                    flow_id,
                                    src,
                                    warning
                                );
                            } else {
                                tracing::warn!("Pipeline warning in flow {}: {}", flow_id, warning);
                            }
                        }
                        StromEvent::PipelineInfo {
                            flow_id,
                            message,
                            source,
                        } => {
                            // Log info message
                            if let Some(src) = source {
                                tracing::info!(
                                    "Pipeline info in flow {} from {}: {}",
                                    flow_id,
                                    src,
                                    message
                                );
                            } else {
                                tracing::info!("Pipeline info in flow {}: {}", flow_id, message);
                            }
                        }
                        StromEvent::PipelineEos { flow_id } => {
                            tracing::info!("Pipeline {} reached end of stream", flow_id);
                            let _ = storage.set_item("strom_needs_refresh", "true");
                        }
                        StromEvent::PropertyChanged {
                            flow_id,
                            element_id,
                            property_name,
                            value,
                        } => {
                            tracing::info!(
                                "Property {}.{} = {:?} changed in flow {}",
                                element_id,
                                property_name,
                                value,
                                flow_id
                            );
                            // Could potentially update UI to reflect property change
                        }
                        StromEvent::PadPropertyChanged {
                            flow_id,
                            element_id,
                            pad_name,
                            property_name,
                            value,
                        } => {
                            tracing::info!(
                                "Pad property {}:{}:{} = {:?} changed in flow {}",
                                element_id,
                                pad_name,
                                property_name,
                                value,
                                flow_id
                            );
                            // Could potentially update UI to reflect pad property change
                        }
                        StromEvent::Ping => {
                            // Just a keep-alive, no action needed
                        }
                    }

                    // Request repaint to process the event
                    ctx.request_repaint();
                }
            }
        });

        // Store the SSE client to keep the connection alive
        self.sse_client = Some(sse_client);
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
        let ctx = ctx.clone();

        spawn_local(async move {
            match api.list_elements().await {
                Ok(elements) => {
                    tracing::info!(
                        "Successfully fetched {} elements, storing in localStorage",
                        elements.len()
                    );
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            if let Ok(json) = serde_json::to_string(&elements) {
                                let _ = storage.set_item("strom_elements_data", &json);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to load elements: {}", e);
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item("strom_elements_error", &e.to_string());
                        }
                    }
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
        let ctx = ctx.clone();

        spawn_local(async move {
            match api.list_blocks().await {
                Ok(blocks) => {
                    tracing::info!(
                        "Successfully fetched {} blocks, storing in localStorage",
                        blocks.len()
                    );
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            if let Ok(json) = serde_json::to_string(&blocks) {
                                let _ = storage.set_item("strom_blocks_data", &json);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to load blocks: {}", e);
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item("strom_blocks_error", &e.to_string());
                        }
                    }
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
        let ctx = ctx.clone();
        let element_type_clone = element_type.clone();

        spawn_local(async move {
            match api.get_element_info(&element_type_clone).await {
                Ok(element_info) => {
                    tracing::info!(
                        "Successfully fetched properties for '{}' ({} properties), storing in localStorage",
                        element_info.name,
                        element_info.properties.len()
                    );
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            if let Ok(json) = serde_json::to_string(&element_info) {
                                let _ = storage.set_item(
                                    &format!("strom_element_properties_{}", element_info.name),
                                    &json,
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to load element properties: {}", e);
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item(
                                &format!("strom_element_properties_error_{}", element_type_clone),
                                &e.to_string(),
                            );
                        }
                    }
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
        let ctx = ctx.clone();
        let element_type_clone = element_type.clone();

        spawn_local(async move {
            match api.get_element_pad_properties(&element_type_clone).await {
                Ok(element_info) => {
                    tracing::info!(
                        "Successfully fetched pad properties for '{}' (sink_pads: {}, src_pads: {}), storing in localStorage",
                        element_info.name,
                        element_info.sink_pads.iter().map(|p| p.properties.len()).sum::<usize>(),
                        element_info.src_pads.iter().map(|p| p.properties.len()).sum::<usize>()
                    );
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            if let Ok(json) = serde_json::to_string(&element_info) {
                                let _ = storage.set_item(
                                    &format!("strom_element_pad_properties_{}", element_info.name),
                                    &json,
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to load pad properties: {}", e);
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item(
                                &format!(
                                    "strom_element_pad_properties_error_{}",
                                    element_type_clone
                                ),
                                &e.to_string(),
                            );
                        }
                    }
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
        let ctx = ctx.clone();

        // Store flows in localStorage as workaround for async closure limitation
        spawn_local(async move {
            match api.list_flows().await {
                Ok(flows) => {
                    tracing::info!(
                        "Successfully fetched {} flows, storing in localStorage",
                        flows.len()
                    );
                    // Store in localStorage so main app can pick it up
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            if let Ok(json) = serde_json::to_string(&flows) {
                                let _ = storage.set_item("strom_flows_data", &json);
                                tracing::info!("Stored {} flows in localStorage", flows.len());
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to load flows: {}", e);
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item("strom_flows_error", &e.to_string());
                        }
                    }
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

                spawn_local(async move {
                    tracing::info!("Starting async save operation for flow {}", flow_clone.id);
                    match api.update_flow(&flow_clone).await {
                        Ok(_) => {
                            tracing::info!("Flow saved successfully");
                            // Trigger refresh to update state
                            if let Some(window) = web_sys::window() {
                                if let Some(storage) = window.local_storage().ok().flatten() {
                                    let _ = storage.set_item("strom_needs_refresh", "true");
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to save flow: {}", e);
                            if let Some(window) = web_sys::window() {
                                if let Some(storage) = window.local_storage().ok().flatten() {
                                    let _ = storage.set_item("strom_flows_error", &e.to_string());
                                }
                            }
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

        spawn_local(async move {
            match api.create_flow(&new_flow).await {
                Ok(created_flow) => {
                    tracing::info!("Flow created successfully: {}", created_flow.name);
                    // Trigger refresh and auto-select the new flow
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item("strom_needs_refresh", "true");
                            // Store the new flow's ID to auto-select it after refresh
                            let _ = storage
                                .set_item("strom_selected_flow_id", &created_flow.id.to_string());
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to create flow: {}", e);
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item("strom_flows_error", &e.to_string());
                        }
                    }
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

            spawn_local(async move {
                match api.start_flow(flow_id).await {
                    Ok(_) => {
                        tracing::info!("Flow started successfully");
                        // Trigger refresh to update state
                        if let Some(window) = web_sys::window() {
                            if let Some(storage) = window.local_storage().ok().flatten() {
                                let _ = storage.set_item("strom_needs_refresh", "true");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to start flow: {}", e);
                        if let Some(window) = web_sys::window() {
                            if let Some(storage) = window.local_storage().ok().flatten() {
                                let _ = storage.set_item("strom_flows_error", &e.to_string());
                            }
                        }
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

            spawn_local(async move {
                match api.stop_flow(flow_id).await {
                    Ok(_) => {
                        tracing::info!("Flow stopped successfully");
                        // Trigger refresh to update state
                        if let Some(window) = web_sys::window() {
                            if let Some(storage) = window.local_storage().ok().flatten() {
                                let _ = storage.set_item("strom_needs_refresh", "true");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to stop flow: {}", e);
                        if let Some(window) = web_sys::window() {
                            if let Some(storage) = window.local_storage().ok().flatten() {
                                let _ = storage.set_item("strom_flows_error", &e.to_string());
                            }
                        }
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

        spawn_local(async move {
            match api.delete_flow(flow_id).await {
                Ok(_) => {
                    tracing::info!("Flow deleted successfully");
                    // Trigger refresh to reload the flow list
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item("strom_needs_refresh", "true");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to delete flow: {}", e);
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item("strom_flows_error", &e.to_string());
                        }
                    }
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

                    if ui.button("▶ Start").clicked() {
                        self.start_flow(ctx);
                    }

                    if ui.button("⏸ Stop").clicked() {
                        self.stop_flow(ctx);
                    }

                    ui.separator();

                    if ui.button("🔍 Debug Graph").clicked() {
                        // Open debug graph in new tab
                        let url = self.api.get_debug_graph_url(flow_id);
                        if let Some(window) = web_sys::window() {
                            if let Err(e) = window.open_with_url_and_target(&url, "_blank") {
                                self.error = Some(format!("Failed to open debug graph: {:?}", e));
                            }
                        } else {
                            self.error = Some("Failed to access window object".to_string());
                        }
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

                            // Persist selected flow
                            if let Some(window) = web_sys::window() {
                                if let Some(storage) = window.local_storage().ok().flatten() {
                                    let _ = storage
                                        .set_item("strom_selected_flow_id", &flow.id.to_string());
                                }
                            }
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
                            if let Some(window) = web_sys::window() {
                                if let Some(storage) = window.local_storage().ok().flatten() {
                                    let _ = storage
                                        .set_item("strom_selected_flow_id", &flow.id.to_string());
                                }
                            }
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

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.loading {
                        ui.spinner();
                    } else {
                        // Check if SSE is connected
                        let is_connected = self
                            .sse_client
                            .as_ref()
                            .map(|client| client.is_connected())
                            .unwrap_or(false);

                        if is_connected {
                            ui.colored_label(Color32::from_rgb(100, 200, 100), "● Connected");
                        } else {
                            ui.colored_label(Color32::from_rgb(200, 100, 100), "● Disconnected");
                        }
                    }
                });
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

                            spawn_local(async move {
                                match api.update_flow(&flow_clone).await {
                                    Ok(_) => {
                                        tracing::info!("Flow properties updated successfully");
                                        if let Some(window) = web_sys::window() {
                                            if let Some(storage) =
                                                window.local_storage().ok().flatten()
                                            {
                                                let _ =
                                                    storage.set_item("strom_needs_refresh", "true");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to update flow properties: {}", e);
                                        if let Some(window) = web_sys::window() {
                                            if let Some(storage) =
                                                window.local_storage().ok().flatten()
                                            {
                                                let _ = storage
                                                    .set_item("strom_flows_error", &e.to_string());
                                            }
                                        }
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
}

impl eframe::App for StromApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Check if we need to refresh
        if let Some(window) = web_sys::window() {
            if let Some(storage) = window.local_storage().ok().flatten() {
                if let Ok(Some(_)) = storage.get_item("strom_needs_refresh") {
                    self.needs_refresh = true;
                    let _ = storage.remove_item("strom_needs_refresh");
                }
            }
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

        // Check localStorage for updated elements
        if let Some(window) = web_sys::window() {
            if let Some(storage) = window.local_storage().ok().flatten() {
                if let Ok(Some(json)) = storage.get_item("strom_elements_data") {
                    use strom_types::element::ElementInfo;
                    match serde_json::from_str::<Vec<ElementInfo>>(&json) {
                        Ok(elements) => {
                            let count = elements.len();
                            tracing::info!(
                                "Updating elements from localStorage: {} elements",
                                count
                            );
                            self.palette.load_elements(elements.clone());
                            self.graph.set_all_element_info(elements);
                            self.status = format!("Loaded {} elements", count);
                            let _ = storage.remove_item("strom_elements_data");
                        }
                        Err(e) => {
                            tracing::error!("Failed to parse elements from localStorage: {}", e);
                            self.error = Some(format!("Elements: decode error: {}", e));
                            // Clear bad data
                            let _ = storage.remove_item("strom_elements_data");
                        }
                    }
                }

                // Check for element load errors
                if let Ok(Some(error)) = storage.get_item("strom_elements_error") {
                    self.error = Some(format!("Elements: {}", error));
                    let _ = storage.remove_item("strom_elements_error");
                }

                // Check for updated blocks
                if let Ok(Some(json)) = storage.get_item("strom_blocks_data") {
                    use strom_types::BlockDefinition;
                    match serde_json::from_str::<Vec<BlockDefinition>>(&json) {
                        Ok(blocks) => {
                            let count = blocks.len();
                            tracing::info!("Updating blocks from localStorage: {} blocks", count);
                            self.palette.load_blocks(blocks.clone());
                            self.graph.set_all_block_definitions(blocks);
                            self.status = format!("Loaded {} blocks", count);
                            let _ = storage.remove_item("strom_blocks_data");
                        }
                        Err(e) => {
                            tracing::error!("Failed to parse blocks from localStorage: {}", e);
                            self.error = Some(format!("Blocks: decode error: {}", e));
                            // Clear bad data
                            let _ = storage.remove_item("strom_blocks_data");
                        }
                    }
                }

                // Check for block load errors
                if let Ok(Some(error)) = storage.get_item("strom_blocks_error") {
                    self.error = Some(format!("Blocks: {}", error));
                    let _ = storage.remove_item("strom_blocks_error");
                }

                // Check for loaded element properties (lazy loading)
                // We need to check all possible element types that might have been loaded
                // Scan all localStorage keys for strom_element_properties_* pattern
                if let Ok(length) = storage.length() {
                    for i in 0..length {
                        if let Ok(Some(key)) = storage.key(i) {
                            if key.starts_with("strom_element_properties_") {
                                if let Ok(Some(json)) = storage.get_item(&key) {
                                    match serde_json::from_str::<ElementInfo>(&json) {
                                        Ok(element_info) => {
                                            tracing::info!(
                                                "Caching element properties from localStorage: {} ({} properties)",
                                                element_info.name,
                                                element_info.properties.len()
                                            );
                                            self.palette.cache_element_properties(element_info);
                                            let _ = storage.remove_item(&key);
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to parse element properties from localStorage: {}",
                                                e
                                            );
                                            let _ = storage.remove_item(&key);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Check for loaded pad properties (lazy loading)
                // Scan all localStorage keys for strom_element_pad_properties_* pattern
                if let Ok(length) = storage.length() {
                    for i in 0..length {
                        if let Ok(Some(key)) = storage.key(i) {
                            if key.starts_with("strom_element_pad_properties_") {
                                if let Ok(Some(json)) = storage.get_item(&key) {
                                    match serde_json::from_str::<ElementInfo>(&json) {
                                        Ok(element_info) => {
                                            let sink_prop_count: usize = element_info
                                                .sink_pads
                                                .iter()
                                                .map(|p| p.properties.len())
                                                .sum();
                                            let src_prop_count: usize = element_info
                                                .src_pads
                                                .iter()
                                                .map(|p| p.properties.len())
                                                .sum();
                                            tracing::info!(
                                                "Caching pad properties from localStorage: {} (sink: {} props, src: {} props)",
                                                element_info.name,
                                                sink_prop_count,
                                                src_prop_count
                                            );
                                            self.palette.cache_element_pad_properties(element_info);
                                            let _ = storage.remove_item(&key);
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to parse pad properties from localStorage: {}",
                                                e
                                            );
                                            let _ = storage.remove_item(&key);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Check for SDP fetch requests
        if let Some(window) = web_sys::window() {
            if let Some(storage) = window.local_storage().ok().flatten() {
                if let Ok(Some(sdp_key)) = storage.get_item("strom_fetch_sdp") {
                    // Remove the flag immediately
                    let _ = storage.remove_item("strom_fetch_sdp");

                    // Parse the key: format is "strom_sdp_{flow_id}_{block_id}"
                    if let Some(key_parts) = sdp_key.strip_prefix("strom_sdp_") {
                        if let Some((flow_id_str, block_id)) = key_parts.split_once('_') {
                            tracing::info!(
                                "Fetching SDP for flow {} block {}",
                                flow_id_str,
                                block_id
                            );

                            let api = self.api.clone();
                            let ctx = ctx.clone();
                            let flow_id_str = flow_id_str.to_string();
                            let block_id = block_id.to_string();
                            let sdp_key_clone = sdp_key.clone();

                            spawn_local(async move {
                                // Construct the SDP URL
                                let url = format!(
                                    "{}/flows/{}/blocks/{}/sdp",
                                    api.base_url(),
                                    flow_id_str,
                                    block_id
                                );

                                match reqwest::get(&url).await {
                                    Ok(response) if response.status().is_success() => {
                                        match response.text().await {
                                            Ok(sdp_text) => {
                                                tracing::info!(
                                                    "Successfully fetched SDP for block {}",
                                                    block_id
                                                );
                                                // Store in localStorage
                                                if let Some(window) = web_sys::window() {
                                                    if let Some(storage) =
                                                        window.local_storage().ok().flatten()
                                                    {
                                                        let _ = storage
                                                            .set_item(&sdp_key_clone, &sdp_text);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    "Failed to read SDP response: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                    Ok(response) => {
                                        tracing::error!(
                                            "Failed to fetch SDP: HTTP {}",
                                            response.status()
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to fetch SDP: {}", e);
                                    }
                                }
                                ctx.request_repaint();
                            });
                        }
                    }
                }
            }
        }

        // Check localStorage for updated flows
        if let Some(window) = web_sys::window() {
            if let Some(storage) = window.local_storage().ok().flatten() {
                // Check if there's new flow data
                if let Ok(Some(json)) = storage.get_item("strom_flows_data") {
                    if let Ok(flows) = serde_json::from_str::<Vec<Flow>>(&json) {
                        // Check if flows have changed (count, IDs, or states)
                        let flows_changed = flows.len() != self.flows.len()
                            || flows
                                .iter()
                                .zip(self.flows.iter())
                                .any(|(a, b)| a.id != b.id || a.state != b.state);

                        if flows_changed {
                            tracing::info!(
                                "Updating flows from localStorage: {} flows",
                                flows.len()
                            );
                            self.flows = flows;
                            self.status = format!("Loaded {} flows", self.flows.len());

                            // Restore selected flow from localStorage
                            if let Ok(Some(selected_flow_id)) =
                                storage.get_item("strom_selected_flow_id")
                            {
                                if let Some(idx) = self
                                    .flows
                                    .iter()
                                    .position(|f| f.id.to_string() == selected_flow_id)
                                {
                                    self.selected_flow_idx = Some(idx);
                                    if let Some(flow) = self.flows.get(idx) {
                                        self.graph.load(flow.elements.clone(), flow.links.clone());
                                        self.graph.load_blocks(flow.blocks.clone());

                                        // Restore selected element from localStorage
                                        if let Ok(Some(selected_element_id)) =
                                            storage.get_item("strom_selected_element_id")
                                        {
                                            self.graph.selected = Some(selected_element_id);
                                        }
                                    }
                                }
                            }
                        }

                        // Always reset loading state and clear data when we get a response
                        self.loading = false;
                        let _ = storage.remove_item("strom_flows_data");
                    }
                }

                // Check for errors
                if let Ok(Some(error)) = storage.get_item("strom_flows_error") {
                    self.error = Some(error.clone());
                    self.loading = false;
                    self.status = "Error loading flows".to_string();
                    let _ = storage.remove_item("strom_flows_error");
                }
            }
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
