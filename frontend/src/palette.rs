//! Element palette for browsing and adding GStreamer elements.

use egui::{ScrollArea, Ui};
use strom_types::element::ElementInfo;

/// Manages the element palette UI.
#[derive(Default)]
pub struct ElementPalette {
    /// Available GStreamer elements
    #[allow(dead_code)]
    elements: Vec<ElementInfo>,
    /// Search filter text
    search: String,
    /// Selected category filter (None = all categories)
    category_filter: Option<String>,
    /// Element being dragged from the palette
    pub dragging_element: Option<String>,
}

impl ElementPalette {
    /// Create a new element palette.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load elements into the palette.
    pub fn load_elements(&mut self, elements: Vec<ElementInfo>) {
        tracing::info!("Loading {} elements into palette", elements.len());
        self.elements = elements;
    }

    /// Add some common GStreamer elements as defaults for testing.
    pub fn load_default_elements(&mut self) {
        self.elements = vec![
            ElementInfo {
                name: "videotestsrc".to_string(),
                description: "Video test pattern generator".to_string(),
                category: "Source".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "audiotestsrc".to_string(),
                description: "Audio test tone generator".to_string(),
                category: "Source".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "filesrc".to_string(),
                description: "Read from a file".to_string(),
                category: "Source".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "rtspsrc".to_string(),
                description: "Receive data from an RTSP server".to_string(),
                category: "Source".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "x264enc".to_string(),
                description: "H.264 video encoder".to_string(),
                category: "Codec".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "vp8enc".to_string(),
                description: "VP8 video encoder".to_string(),
                category: "Codec".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "avenc_aac".to_string(),
                description: "AAC audio encoder".to_string(),
                category: "Codec".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "queue".to_string(),
                description: "Simple data queue".to_string(),
                category: "Generic".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "tee".to_string(),
                description: "Split data to multiple outputs".to_string(),
                category: "Generic".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "filesink".to_string(),
                description: "Write to a file".to_string(),
                category: "Sink".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "autovideosink".to_string(),
                description: "Auto-detect video output".to_string(),
                category: "Sink".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "autoaudiosink".to_string(),
                description: "Auto-detect audio output".to_string(),
                category: "Sink".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "rtmpsink".to_string(),
                description: "Send data to an RTMP server".to_string(),
                category: "Sink".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "hlssink2".to_string(),
                description: "HTTP Live Streaming sink".to_string(),
                category: "Sink".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "videoconvert".to_string(),
                description: "Convert video format".to_string(),
                category: "Filter".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "audioresample".to_string(),
                description: "Resample audio".to_string(),
                category: "Filter".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
            ElementInfo {
                name: "capsfilter".to_string(),
                description: "Enforce caps on stream".to_string(),
                category: "Filter".to_string(),
                src_pads: vec![],
                sink_pads: vec![],
                properties: vec![],
            },
        ];
    }

    /// Render the element palette.
    pub fn show(&mut self, ui: &mut Ui) {
        ui.heading("Elements");
        ui.separator();

        // Search box
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.search);
        });

        ui.add_space(5.0);

        // Category filter
        let mut categories: Vec<String> = self
            .elements
            .iter()
            .map(|e| e.category.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        categories.sort();

        ui.horizontal(|ui| {
            ui.label("Category:");
            egui::ComboBox::from_id_salt("category_filter")
                .selected_text(self.category_filter.as_ref().unwrap_or(&"All".to_string()))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.category_filter.is_none(), "All")
                        .clicked()
                    {
                        self.category_filter = None;
                    }
                    for category in &categories {
                        let selected = self.category_filter.as_ref() == Some(category);
                        if ui.selectable_label(selected, category).clicked() {
                            self.category_filter = Some(category.clone());
                        }
                    }
                });
        });

        ui.separator();

        // Element list
        let search = self.search.clone();
        let category_filter = self.category_filter.clone();

        // Limit the palette height to leave space for property inspector below
        let available_height = ui.available_height();
        let palette_max_height = (available_height * 0.4).max(200.0); // Use at most 40% of available space, minimum 200px

        ScrollArea::both()
            .id_salt("palette_scroll")
            .max_height(palette_max_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let filtered: Vec<ElementInfo> = self
                    .elements
                    .iter()
                    .filter(|e| {
                        // If search is empty, only show audiotestsrc and autoaudiosink
                        if search.is_empty() {
                            if e.name != "audiotestsrc" && e.name != "autoaudiosink" {
                                return false;
                            }
                        } else {
                            // Otherwise, filter by search text
                            let matches_search =
                                e.name.to_lowercase().contains(&search.to_lowercase())
                                    || e.description
                                        .to_lowercase()
                                        .contains(&search.to_lowercase());
                            if !matches_search {
                                return false;
                            }
                        }

                        let matches_category = category_filter.is_none()
                            || category_filter.as_ref() == Some(&e.category);

                        matches_category
                    })
                    .cloned()
                    .collect();

                if filtered.is_empty() {
                    ui.label("No elements found");
                } else {
                    for element in filtered {
                        self.draw_element_item(ui, &element);
                    }
                }
            });
    }

    fn draw_element_item(&mut self, ui: &mut Ui, element: &ElementInfo) {
        let name = element.name.clone();
        let description = element.description.clone();

        ui.push_id(&name, |ui| {
            // Main horizontal layout for element item
            ui.horizontal(|ui| {
                // Element name label with truncation
                let available_width = ui.available_width() - 70.0; // Reserve space for button
                ui.allocate_ui_with_layout(
                    egui::vec2(available_width, ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(egui::Label::new(&name).truncate())
                            .on_hover_text(&description);
                    },
                );

                // Add button on the right
                if ui.button("+ Add").on_hover_text("Add to canvas").clicked() {
                    self.dragging_element = Some(name.clone());
                }
            });

            // Show category and description below (wrapped)
            ui.horizontal_wrapped(|ui| {
                ui.small(&element.category);
                ui.small("|");
                ui.small(&element.description);
            });

            ui.separator();
        });
    }

    /// Check if an element is being dragged and return it.
    pub fn take_dragging_element(&mut self) -> Option<String> {
        self.dragging_element.take()
    }

    /// Get element info for a specific element type.
    pub fn get_element_info(&self, element_type: &str) -> Option<&ElementInfo> {
        self.elements.iter().find(|e| e.name == element_type)
    }
}
