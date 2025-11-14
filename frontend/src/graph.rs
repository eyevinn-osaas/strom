//! Node-based graph editor for GStreamer pipelines.

use egui::{pos2, vec2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
use std::collections::HashMap;
use strom_types::{Element, ElementId, Link};

/// Represents the state of the graph editor.
pub struct GraphEditor {
    /// Elements (nodes) in the graph
    pub elements: Vec<Element>,
    /// Links (edges) between elements
    pub links: Vec<Link>,
    /// Currently selected element ID
    pub selected: Option<ElementId>,
    /// Element being dragged
    dragging: Option<ElementId>,
    /// Offset for panning the canvas
    pub pan_offset: Vec2,
    /// Zoom level
    pub zoom: f32,
    /// Link being created (source element and pad)
    creating_link: Option<(ElementId, String)>,
    /// Hover state for pads (element_id, pad_name)
    hovered_pad: Option<(ElementId, String)>,
    /// Hover state for elements
    hovered_element: Option<ElementId>,
    /// Currently selected link index
    selected_link: Option<usize>,
    /// Hovered link index
    hovered_link: Option<usize>,
}

impl Default for GraphEditor {
    fn default() -> Self {
        Self {
            elements: Vec::new(),
            links: Vec::new(),
            selected: None,
            dragging: None,
            pan_offset: Vec2::ZERO,
            zoom: 1.0,
            creating_link: None,
            hovered_pad: None,
            hovered_element: None,
            selected_link: None,
            hovered_link: None,
        }
    }
}

impl GraphEditor {
    /// Create a new graph editor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load elements and links into the editor.
    pub fn load(&mut self, elements: Vec<Element>, links: Vec<Link>) {
        self.elements = elements;
        self.links = links;
        self.selected = None;
        self.dragging = None;
        self.creating_link = None;
        self.hovered_element = None;
    }

    /// Add a new element to the graph at the given position.
    pub fn add_element(&mut self, element_type: String, pos: Pos2) {
        let id = format!("elem_{}", self.elements.len());
        let element = Element {
            id: id.clone(),
            element_type,
            properties: HashMap::new(),
            position: Some((pos.x, pos.y)),
        };
        self.elements.push(element);
        self.selected = Some(id);
    }

    /// Remove the currently selected element.
    pub fn remove_selected(&mut self) {
        if let Some(id) = &self.selected {
            // Remove element
            self.elements.retain(|e| &e.id != id);
            // Remove links connected to this element
            let id_clone = id.clone();
            self.links.retain(|link| {
                !link.from.starts_with(&id_clone) && !link.to.starts_with(&id_clone)
            });
            self.selected = None;

            // Clear selected element from localStorage
            if let Some(window) = web_sys::window() {
                if let Some(storage) = window.local_storage().ok().flatten() {
                    let _ = storage.remove_item("strom_selected_element_id");
                }
            }
        }
    }

    /// Remove the currently selected link.
    pub fn remove_selected_link(&mut self) {
        if let Some(idx) = self.selected_link {
            if idx < self.links.len() {
                self.links.remove(idx);
                self.selected_link = None;
            }
        }
    }

    /// Render the graph editor.
    pub fn show(&mut self, ui: &mut Ui) -> Response {
        ui.push_id("graph_editor", |ui| {
            let (response, painter) =
                ui.allocate_painter(ui.available_size_before_wrap(), Sense::click_and_drag());

            let zoom = self.zoom;
            let pan_offset = self.pan_offset;
            let rect_min = response.rect.min;

            let to_screen = |pos: Pos2| -> Pos2 { rect_min + (pos.to_vec2() * zoom) + pan_offset };

            let from_screen =
                |pos: Pos2| -> Pos2 { ((pos - rect_min - pan_offset) / zoom).to_pos2() };

            // Handle zoom
            if let Some(hover_pos) = response.hover_pos() {
                let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll_delta != 0.0 {
                    let zoom_delta = scroll_delta * 0.001;
                    self.zoom = (self.zoom + zoom_delta).clamp(0.1, 3.0);

                    // Adjust pan to zoom towards cursor
                    let world_pos = from_screen(hover_pos);
                    let new_screen_pos = to_screen(world_pos);
                    self.pan_offset += hover_pos - new_screen_pos;
                }
            }

            // Draw grid
            self.draw_grid(&painter, response.rect);

            // Draw nodes and handle interaction (must happen before panning)
            let mut elements_to_update = Vec::new();
            let mut pad_interactions = Vec::new();

            for element in &self.elements {
                let pos = element.position.unwrap_or((100.0, 100.0));
                let screen_pos = to_screen(pos2(pos.0, pos.1));

                let node_rect =
                    Rect::from_min_size(screen_pos, vec2(200.0 * self.zoom, 80.0 * self.zoom));

                let is_selected = self.selected.as_ref() == Some(&element.id);
                let is_hovered = self.hovered_element.as_ref() == Some(&element.id);
                let node_response =
                    self.draw_node(ui, &painter, element, node_rect, is_selected, is_hovered);

                // Track hover state
                if node_response.hovered() {
                    self.hovered_element = Some(element.id.clone());
                } else if self.hovered_element.as_ref() == Some(&element.id) {
                    self.hovered_element = None;
                }

                // Handle node selection - select on click OR when starting to drag
                if node_response.clicked() || (node_response.dragged() && self.dragging.is_none()) {
                    self.selected = Some(element.id.clone());
                    self.selected_link = None; // Deselect any link

                    // Persist selected element to localStorage
                    if let Some(window) = web_sys::window() {
                        if let Some(storage) = window.local_storage().ok().flatten() {
                            let _ = storage.set_item("strom_selected_element_id", &element.id);
                        }
                    }
                }

                // Handle node dragging
                if node_response.dragged() {
                    if self.dragging.is_none() {
                        self.dragging = Some(element.id.clone());
                    }

                    if self.dragging.as_ref() == Some(&element.id) {
                        let delta = node_response.drag_delta() / self.zoom;
                        let new_pos = (pos.0 + delta.x, pos.1 + delta.y);
                        elements_to_update.push((element.id.clone(), new_pos));
                    }
                }

                // Collect pad interactions for later processing
                pad_interactions.push((element.id.clone(), node_rect));
            }

            // Handle pad interactions
            for (element_id, rect) in pad_interactions {
                if let Some(element) = self.elements.iter().find(|e| e.id == element_id).cloned() {
                    self.handle_pad_interaction(ui, &element, rect);
                }
            }

            // Update element positions
            for (id, new_pos) in elements_to_update {
                if let Some(elem) = self.elements.iter_mut().find(|e| e.id == id) {
                    elem.position = Some(new_pos);
                }
            }

            // Handle canvas panning (only if not dragging a node)
            if response.dragged() && self.dragging.is_none() && self.creating_link.is_none() {
                self.pan_offset += response.drag_delta();
            }

            // Deselect when clicking on empty space (not a link, not a node)
            if response.clicked() && self.hovered_link.is_none() && self.hovered_element.is_none() {
                self.selected = None;
                self.selected_link = None;
            }

            // Reset dragging state when mouse is released
            if !ui.input(|i| i.pointer.primary_down()) {
                self.dragging = None;

                // Finalize link creation
                if let Some((from_id, from_pad)) = self.creating_link.take() {
                    if let Some((to_id, to_pad)) = &self.hovered_pad {
                        if from_id != *to_id {
                            let link = Link {
                                from: format!("{}:{}", from_id, from_pad),
                                to: format!("{}:{}", to_id, to_pad),
                            };
                            self.links.push(link);
                        }
                    }
                }
            }

            // Draw links AFTER nodes so they appear on top
            let links_clone = self.links.clone();
            self.hovered_link = None; // Reset hover state

            for (idx, link) in links_clone.iter().enumerate() {
                let is_selected = self.selected_link == Some(idx);

                if let Some(hover_pos) = response.hover_pos() {
                    // Check if mouse is near this link
                    if self.is_point_near_link(link, hover_pos, &to_screen) {
                        self.hovered_link = Some(idx);
                    }
                }

                let is_hovered = self.hovered_link == Some(idx);
                self.draw_link(&painter, link, &to_screen, is_selected, is_hovered);
            }

            // Handle link selection on click
            if response.clicked() && self.hovered_link.is_some() {
                self.selected_link = self.hovered_link;
                self.selected = None; // Deselect any element
            }

            // Draw link being created (on top of everything)
            if let Some((from_id, _)) = &self.creating_link {
                if let Some(from_elem) = self.elements.iter().find(|e| &e.id == from_id) {
                    let from_pos = from_elem.position.unwrap_or((100.0, 100.0));
                    // Output port is at the right edge, centered vertically (200 width, 80 height)
                    let from_screen_pos = to_screen(pos2(from_pos.0 + 200.0, from_pos.1 + 40.0));

                    let to_pos = ui.input(|i| i.pointer.hover_pos().unwrap_or(from_screen_pos));

                    painter.line_segment(
                        [from_screen_pos, to_pos],
                        Stroke::new(2.0, Color32::from_rgb(100, 150, 255)),
                    );
                }
            }

            response
        })
        .inner
    }

    fn draw_grid(&self, painter: &egui::Painter, rect: Rect) {
        let grid_spacing = 50.0 * self.zoom;
        let color = Color32::from_gray(40);

        let start_x = (rect.min.x / grid_spacing).floor() * grid_spacing;
        let start_y = (rect.min.y / grid_spacing).floor() * grid_spacing;

        // Vertical lines
        let mut x = start_x;
        while x < rect.max.x {
            painter.line_segment(
                [pos2(x, rect.min.y), pos2(x, rect.max.y)],
                Stroke::new(1.0, color),
            );
            x += grid_spacing;
        }

        // Horizontal lines
        let mut y = start_y;
        while y < rect.max.y {
            painter.line_segment(
                [pos2(rect.min.x, y), pos2(rect.max.x, y)],
                Stroke::new(1.0, color),
            );
            y += grid_spacing;
        }
    }

    fn draw_node(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        element: &Element,
        rect: Rect,
        is_selected: bool,
        is_hovered: bool,
    ) -> Response {
        let stroke_color = if is_selected {
            Color32::from_rgb(100, 150, 255)
        } else if is_hovered {
            Color32::from_gray(154) // Brighter border on hover
        } else {
            Color32::from_gray(120)
        };

        let stroke_width = if is_selected {
            2.0
        } else if is_hovered {
            1.5
        } else {
            1.0
        };

        let fill_color = if is_selected {
            Color32::from_gray(60) // Brighter background when selected
        } else if is_hovered {
            Color32::from_gray(53) // Lighter background on hover
        } else {
            Color32::from_gray(30)
        };

        // Draw node background
        painter.rect(
            rect,
            5.0,
            fill_color,
            Stroke::new(stroke_width, stroke_color),
            egui::epaint::StrokeKind::Inside,
        );

        // Draw element type
        let text_pos = rect.min + vec2(10.0, 10.0);
        painter.text(
            text_pos,
            egui::Align2::LEFT_TOP,
            &element.element_type,
            FontId::proportional(14.0 * self.zoom),
            Color32::WHITE,
        );

        // Draw element ID
        let id_pos = rect.min + vec2(10.0, 30.0);
        painter.text(
            id_pos,
            egui::Align2::LEFT_TOP,
            &element.id,
            FontId::proportional(12.0 * self.zoom),
            Color32::from_gray(180),
        );

        // Check if ports are hovered
        let input_hovered = self
            .hovered_pad
            .as_ref()
            .map(|(id, pad)| id == &element.id && pad == "sink")
            .unwrap_or(false);
        let output_hovered = self
            .hovered_pad
            .as_ref()
            .map(|(id, pad)| id == &element.id && pad == "src")
            .unwrap_or(false);

        // Determine which ports to show based on element type
        let is_source = element.element_type.ends_with("src");
        let is_sink = element.element_type.ends_with("sink");
        let show_input = !is_source; // Show input for sinks and filters
        let show_output = !is_sink; // Show output for sources and filters

        let port_size = 12.0 * self.zoom;

        // Draw input pad (sink) as box - only for sinks and filters
        if show_input {
            let input_center = pos2(rect.min.x, rect.center().y);
            let input_rect = Rect::from_center_size(input_center, vec2(port_size, port_size));

            if input_hovered {
                // Draw glow effect
                let glow_rect = Rect::from_center_size(
                    input_center,
                    vec2(port_size + 10.0 * self.zoom, port_size + 10.0 * self.zoom),
                );
                painter.rect(
                    glow_rect,
                    3.0,
                    Color32::from_rgba_premultiplied(100, 200, 100, 77), // ~30% opacity
                    Stroke::NONE,
                    egui::epaint::StrokeKind::Inside,
                );
                painter.rect(
                    input_rect,
                    2.0,
                    Color32::from_rgb(126, 232, 126), // Brighter on hover
                    Stroke::NONE,
                    egui::epaint::StrokeKind::Inside,
                );
            } else {
                painter.rect(
                    input_rect,
                    2.0,
                    Color32::from_rgb(100, 200, 100),
                    Stroke::NONE,
                    egui::epaint::StrokeKind::Inside,
                );
            }
        }

        // Draw output pad (src) as box - only for sources and filters
        if show_output {
            let output_center = pos2(rect.max.x, rect.center().y);
            let output_rect = Rect::from_center_size(output_center, vec2(port_size, port_size));

            if output_hovered {
                // Draw glow effect
                let glow_rect = Rect::from_center_size(
                    output_center,
                    vec2(port_size + 10.0 * self.zoom, port_size + 10.0 * self.zoom),
                );
                painter.rect(
                    glow_rect,
                    3.0,
                    Color32::from_rgba_premultiplied(255, 150, 100, 77), // ~30% opacity
                    Stroke::NONE,
                    egui::epaint::StrokeKind::Inside,
                );
                painter.rect(
                    output_rect,
                    2.0,
                    Color32::from_rgb(255, 176, 128), // Brighter on hover
                    Stroke::NONE,
                    egui::epaint::StrokeKind::Inside,
                );
            } else {
                painter.rect(
                    output_rect,
                    2.0,
                    Color32::from_rgb(255, 150, 100),
                    Stroke::NONE,
                    egui::epaint::StrokeKind::Inside,
                );
            }
        }

        ui.interact(rect, ui.id().with(&element.id), Sense::click_and_drag())
    }

    fn draw_link(
        &self,
        painter: &egui::Painter,
        link: &Link,
        to_screen: &impl Fn(Pos2) -> Pos2,
        is_selected: bool,
        is_hovered: bool,
    ) {
        // Parse element IDs from link
        let from_id = link.from.split(':').next().unwrap_or("");
        let to_id = link.to.split(':').next().unwrap_or("");

        // Find elements
        let from_elem = self.elements.iter().find(|e| e.id == from_id);
        let to_elem = self.elements.iter().find(|e| e.id == to_id);

        if let (Some(from), Some(to)) = (from_elem, to_elem) {
            let from_pos = from.position.unwrap_or((100.0, 100.0));
            let to_pos = to.position.unwrap_or((300.0, 100.0));

            let from_screen = to_screen(pos2(from_pos.0 + 200.0, from_pos.1 + 40.0));
            let to_screen = to_screen(pos2(to_pos.0, to_pos.1 + 40.0));

            // Draw cubic bezier curve
            let control_offset = 50.0 * self.zoom;
            let control1 = from_screen + vec2(control_offset, 0.0);
            let control2 = to_screen - vec2(control_offset, 0.0);

            // Determine color and width based on state
            let (color, width) = if is_selected {
                (Color32::from_rgb(100, 150, 255), 3.0) // Blue and thicker when selected
            } else if is_hovered {
                (Color32::from_rgb(200, 200, 200), 2.5) // Brighter when hovered
            } else {
                (Color32::from_rgb(150, 150, 150), 2.0) // Default gray
            };

            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [from_screen, control1, control2, to_screen],
                false,
                Color32::TRANSPARENT,
                Stroke::new(width, color),
            ));
        }
    }

    /// Check if a point is near a bezier curve (for click detection)
    fn is_point_near_link(
        &self,
        link: &Link,
        point: Pos2,
        to_screen: &impl Fn(Pos2) -> Pos2,
    ) -> bool {
        // Parse element IDs from link
        let from_id = link.from.split(':').next().unwrap_or("");
        let to_id = link.to.split(':').next().unwrap_or("");

        // Find elements
        let from_elem = self.elements.iter().find(|e| e.id == from_id);
        let to_elem = self.elements.iter().find(|e| e.id == to_id);

        if let (Some(from), Some(to)) = (from_elem, to_elem) {
            let from_pos = from.position.unwrap_or((100.0, 100.0));
            let to_pos = to.position.unwrap_or((300.0, 100.0));

            let from_screen = to_screen(pos2(from_pos.0 + 200.0, from_pos.1 + 40.0));
            let to_screen_pos = to_screen(pos2(to_pos.0, to_pos.1 + 40.0));

            let control_offset = 50.0 * self.zoom;
            let control1 = from_screen + vec2(control_offset, 0.0);
            let control2 = to_screen_pos - vec2(control_offset, 0.0);

            // Sample points along the bezier curve and check distance
            let threshold = 10.0; // Click detection threshold in pixels
            let samples = 20;

            for i in 0..=samples {
                let t = i as f32 / samples as f32;
                let bezier_point =
                    self.evaluate_cubic_bezier(from_screen, control1, control2, to_screen_pos, t);

                let distance = point.distance(bezier_point);
                if distance < threshold {
                    return true;
                }
            }
        }

        false
    }

    /// Evaluate a cubic bezier curve at parameter t
    fn evaluate_cubic_bezier(&self, p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, t: f32) -> Pos2 {
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;

        pos2(
            mt3 * p0.x + 3.0 * mt2 * t * p1.x + 3.0 * mt * t2 * p2.x + t3 * p3.x,
            mt3 * p0.y + 3.0 * mt2 * t * p1.y + 3.0 * mt * t2 * p2.y + t3 * p3.y,
        )
    }

    fn handle_pad_interaction(&mut self, ui: &Ui, element: &Element, rect: Rect) {
        let port_size = 12.0 * self.zoom;
        let interaction_size = port_size + 4.0 * self.zoom; // Slightly larger for easier interaction

        // Determine which ports to show based on element type
        let is_source = element.element_type.ends_with("src");
        let is_sink = element.element_type.ends_with("sink");
        let show_input = !is_source; // Show input for sinks and filters
        let show_output = !is_sink; // Show output for sources and filters

        let mut output_hovered = false;
        let mut input_hovered = false;

        // Output pad (src) - only for sources and filters
        if show_output {
            let output_center = pos2(rect.max.x, rect.center().y);
            let output_rect =
                Rect::from_center_size(output_center, vec2(interaction_size, interaction_size));
            let output_response = ui.interact(
                output_rect,
                ui.id().with((&element.id, "src")),
                Sense::click_and_drag(),
            );

            // Start creating link when dragging from output port
            if output_response.drag_started()
                || (output_response.dragged() && self.creating_link.is_none())
            {
                self.creating_link = Some((element.id.clone(), "src".to_string()));
            }

            if output_response.hovered() {
                self.hovered_pad = Some((element.id.clone(), "src".to_string()));
                output_hovered = true;
            }
        }

        // Input pad (sink) - only for sinks and filters
        if show_input {
            let input_center = pos2(rect.min.x, rect.center().y);
            let input_rect =
                Rect::from_center_size(input_center, vec2(interaction_size, interaction_size));
            let input_response = ui.interact(
                input_rect,
                ui.id().with((&element.id, "sink")),
                Sense::hover(),
            );

            if input_response.hovered() {
                self.hovered_pad = Some((element.id.clone(), "sink".to_string()));
                input_hovered = true;
            }
        }

        // Clear hovered_pad if mouse is not over any pad
        if !input_hovered && !output_hovered {
            if let Some((id, _)) = &self.hovered_pad {
                if id == &element.id {
                    self.hovered_pad = None;
                }
            }
        }
    }

    /// Get the currently selected element.
    pub fn get_selected_element(&self) -> Option<&Element> {
        self.selected
            .as_ref()
            .and_then(|id| self.elements.iter().find(|e| &e.id == id))
    }

    /// Get a mutable reference to the currently selected element.
    pub fn get_selected_element_mut(&mut self) -> Option<&mut Element> {
        let selected_id = self.selected.clone()?;
        self.elements.iter_mut().find(|e| e.id == selected_id)
    }
}
