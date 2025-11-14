//! Main application structure.

use egui::{CentralPanel, Color32, Context, SidePanel, TopBottomPanel};
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
    /// Flow pending deletion (for confirmation dialog)
    flow_pending_deletion: Option<(strom_types::FlowId, String)>,
    /// SSE client for real-time updates
    sse_client: Option<SseClient>,
    /// Flow being renamed (flow index)
    renaming_flow_idx: Option<usize>,
    /// Temporary name for renaming
    rename_buffer: String,
}

impl StromApp {
    /// Create a new application instance.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Set dark theme
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let mut app = Self {
            api: ApiClient::new("http://localhost:3000/api"),
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
            flow_pending_deletion: None,
            sse_client: None,
            renaming_flow_idx: None,
            rename_buffer: String::new(),
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

        let mut sse_client = SseClient::new("http://localhost:3000/api/events");

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
                    // Trigger refresh by setting flag in localStorage
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item("strom_needs_refresh", "true");
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

                    let state_color = match state {
                        PipelineState::Null => Color32::GRAY,
                        PipelineState::Ready => Color32::YELLOW,
                        PipelineState::Paused => Color32::from_rgb(255, 165, 0),
                        PipelineState::Playing => Color32::GREEN,
                    };

                    ui.colored_label(state_color, format!("State: {:?}", state));
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
            .show(ctx, |ui| {
                ui.heading("Flows");
                ui.separator();

                if self.flows.is_empty() {
                    ui.label("No flows yet");
                    ui.label("Click 'New Flow' to get started");
                } else {
                    // Collect the rename action outside the iteration
                    let mut rename_action: Option<(usize, String)> = None;

                    for (idx, flow) in self.flows.iter().enumerate() {
                        let selected = self.selected_flow_idx == Some(idx);
                        let is_renaming = self.renaming_flow_idx == Some(idx);

                        if is_renaming {
                            // Show text edit field for renaming
                            ui.horizontal(|ui| {
                                ui.add_space(4.0);
                                let text_edit_response =
                                    ui.text_edit_singleline(&mut self.rename_buffer);

                                // Auto-focus the text field when renaming starts
                                if text_edit_response.gained_focus() {
                                    text_edit_response.request_focus();
                                }

                                // Save on Enter key
                                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    if !self.rename_buffer.is_empty()
                                        && self.rename_buffer != flow.name
                                    {
                                        // Schedule the rename action
                                        rename_action = Some((idx, self.rename_buffer.clone()));
                                    }
                                    self.renaming_flow_idx = None;
                                    self.rename_buffer.clear();
                                }

                                // Cancel on Escape or loss of focus (clicking elsewhere)
                                if ui.input(|i| i.key_pressed(egui::Key::Escape))
                                    || text_edit_response.lost_focus()
                                {
                                    self.renaming_flow_idx = None;
                                    self.rename_buffer.clear();
                                }
                            });
                        } else {
                            // Create a full-width selectable area
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 20.0),
                                egui::Sense::click(),
                            );

                            if response.clicked() {
                                if selected {
                                    // Already selected - enter rename mode
                                    self.renaming_flow_idx = Some(idx);
                                    self.rename_buffer = flow.name.clone();
                                } else {
                                    // Not selected - select it
                                    self.selected_flow_idx = Some(idx);
                                    // Load flow into graph editor
                                    self.graph.load(flow.elements.clone(), flow.links.clone());

                                    // Persist selected flow
                                    if let Some(window) = web_sys::window() {
                                        if let Some(storage) = window.local_storage().ok().flatten()
                                        {
                                            let _ = storage.set_item(
                                                "strom_selected_flow_id",
                                                &flow.id.to_string(),
                                            );
                                        }
                                    }
                                }
                            }

                            // Draw background for selected item
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

                            // Draw flow name and delete button
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

                            child_ui.colored_label(text_color, &flow.name);

                            // Delete button on the right
                            child_ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(4.0);
                                    if ui.small_button("🗑").on_hover_text("Delete flow").clicked()
                                    {
                                        self.flow_pending_deletion =
                                            Some((flow.id, flow.name.clone()));
                                    }
                                },
                            );
                        }
                    }

                    // Process rename action after iteration
                    if let Some((idx, new_name)) = rename_action {
                        if let Some(flow_to_rename) = self.flows.get_mut(idx) {
                            flow_to_rename.name = new_name;
                            let flow_clone = flow_to_rename.clone();
                            let api = self.api.clone();
                            let ctx_clone = ctx.clone();

                            spawn_local(async move {
                                match api.update_flow(&flow_clone).await {
                                    Ok(_) => {
                                        tracing::info!("Flow renamed successfully");
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
                                        tracing::error!("Failed to rename flow: {}", e);
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
                    }
                }
            });
    }

    /// Render the element palette sidebar.
    fn render_palette(&mut self, ctx: &Context) {
        SidePanel::right("palette")
            .default_width(250.0)
            .show(ctx, |ui| {
                // Show either the palette or the property inspector, not both
                if let Some(element) = self.graph.get_selected_element_mut() {
                    // Element selected: show ONLY property inspector
                    ui.heading("Properties");
                    ui.separator();
                    let element_info = self.palette.get_element_info(&element.element_type);
                    PropertyInspector::show(ui, element, element_info);
                } else {
                    // No element selected: show ONLY the palette
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
                            self.palette.load_elements(elements);
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
    }
}
