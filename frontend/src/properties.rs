//! Property inspector for editing element properties.

use crate::graph::PropertyTab;
use egui::{Color32, ScrollArea, Ui};
use strom_types::{
    block::ExposedProperty,
    element::{ElementInfo, PropertyInfo, PropertyType},
    BlockDefinition, BlockInstance, Element, PropertyValue,
};

/// Property inspector panel.
pub struct PropertyInspector;

impl PropertyInspector {
    /// Match an actual pad name (e.g., "sink_0") to a pad template (e.g., "sink_%u").
    /// Returns true if the actual pad name matches the template.
    fn matches_pad_template(actual_pad: &str, template: &str) -> bool {
        // First try exact match
        if actual_pad == template {
            return true;
        }

        // Check for request pad patterns like "sink_%u", "src_%u", "sink_%d", etc.
        // Replace common patterns with regex-like matching
        if template.contains("%u") || template.contains("%d") {
            // Extract the prefix before the pattern
            let prefix = if let Some(idx) = template.find("%u") {
                &template[..idx]
            } else if let Some(idx) = template.find("%d") {
                &template[..idx]
            } else {
                return false;
            };

            // Check if actual pad starts with the prefix
            if !actual_pad.starts_with(prefix) {
                return false;
            }

            // Check if the suffix is numeric
            let suffix = &actual_pad[prefix.len()..];
            suffix.chars().all(|c| c.is_ascii_digit() || c == '_')
        } else {
            false
        }
    }

    /// Show the property inspector for the given element with tabbed interface.
    /// Returns the new active tab if it was changed.
    pub fn show(
        ui: &mut Ui,
        element: &mut Element,
        element_info: Option<&ElementInfo>,
        active_tab: PropertyTab,
        focused_pad: Option<String>,
        input_pads: Vec<String>,
        output_pads: Vec<String>,
    ) -> PropertyTab {
        let element_id = element.id.clone();
        let mut new_tab = active_tab;

        ui.push_id(&element_id, |ui| {
            // Element type (read-only)
            ui.horizontal(|ui| {
                ui.label("Type:");
                ui.monospace(&element.element_type);
            });

            // Element ID (read-only)
            ui.horizontal(|ui| {
                ui.label("ID:");
                ui.monospace(&element.id);
            });

            ui.separator();

            // Tab buttons
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(new_tab == PropertyTab::Element, "Element Properties")
                    .clicked()
                {
                    new_tab = PropertyTab::Element;
                }
                if ui
                    .selectable_label(new_tab == PropertyTab::InputPads, "Input Pads")
                    .clicked()
                {
                    new_tab = PropertyTab::InputPads;
                }
                if ui
                    .selectable_label(new_tab == PropertyTab::OutputPads, "Output Pads")
                    .clicked()
                {
                    new_tab = PropertyTab::OutputPads;
                }
            });

            ui.separator();

            // Tab content
            match new_tab {
                PropertyTab::Element => {
                    Self::show_element_properties_tab(ui, element, element_info);
                }
                PropertyTab::InputPads => {
                    Self::show_input_pads_tab(
                        ui,
                        element,
                        element_info,
                        &input_pads,
                        focused_pad.as_deref(),
                    );
                }
                PropertyTab::OutputPads => {
                    Self::show_output_pads_tab(
                        ui,
                        element,
                        element_info,
                        &output_pads,
                        focused_pad.as_deref(),
                    );
                }
            }
        });

        new_tab
    }

    /// Show the Element Properties tab content.
    fn show_element_properties_tab(
        ui: &mut Ui,
        element: &mut Element,
        element_info: Option<&ElementInfo>,
    ) {
        ui.label("💡 Only modified properties are saved");

        ScrollArea::both()
            .id_salt("element_properties_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(info) = element_info {
                    if !info.properties.is_empty() {
                        for prop_info in &info.properties {
                            Self::show_property_from_info(ui, element, prop_info);
                        }
                    } else {
                        ui.label("No element properties available");
                    }
                } else {
                    ui.label("No element metadata available");
                }
            });
    }

    /// Show the Input Pads tab content.
    fn show_input_pads_tab(
        ui: &mut Ui,
        element: &mut Element,
        element_info: Option<&ElementInfo>,
        actual_pads: &[String],
        focused_pad: Option<&str>,
    ) {
        ui.label("💡 Only modified properties are saved");

        ScrollArea::both()
            .id_salt("input_pads_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if actual_pads.is_empty() {
                    ui.label("No input pads connected");
                    return;
                }

                for pad_name in actual_pads {
                    // Highlight focused pad
                    let is_focused = focused_pad == Some(pad_name.as_str());
                    if is_focused {
                        ui.colored_label(
                            Color32::from_rgb(255, 200, 100),
                            format!("▶ Input Pad: {}", pad_name),
                        );
                    } else {
                        ui.label(format!("Input Pad: {}", pad_name));
                    }

                    ui.indent(pad_name, |ui| {
                        // Find properties for this pad from element_info
                        if let Some(info) = element_info {
                            // Check if there's a matching sink pad in metadata (try template matching)
                            let pad_info = info
                                .sink_pads
                                .iter()
                                .find(|p| Self::matches_pad_template(pad_name, &p.name));

                            if let Some(pad_info) = pad_info {
                                if !pad_info.properties.is_empty() {
                                    for prop_info in &pad_info.properties {
                                        Self::show_pad_property_from_info(
                                            ui, element, pad_name, prop_info,
                                        );
                                    }
                                } else {
                                    ui.small("No configurable properties");
                                }
                            } else {
                                ui.small(format!(
                                    "No metadata for pad (tried matching: {})",
                                    pad_name
                                ));
                            }
                        } else {
                            ui.small("No element metadata available");
                        }
                    });
                    ui.add_space(8.0);
                }
            });
    }

    /// Show the Output Pads tab content.
    fn show_output_pads_tab(
        ui: &mut Ui,
        element: &mut Element,
        element_info: Option<&ElementInfo>,
        actual_pads: &[String],
        focused_pad: Option<&str>,
    ) {
        ui.label("💡 Only modified properties are saved");

        ScrollArea::both()
            .id_salt("output_pads_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if actual_pads.is_empty() {
                    ui.label("No output pads connected");
                    return;
                }

                for pad_name in actual_pads {
                    // Highlight focused pad
                    let is_focused = focused_pad == Some(pad_name.as_str());
                    if is_focused {
                        ui.colored_label(
                            Color32::from_rgb(255, 200, 100),
                            format!("▶ Output Pad: {}", pad_name),
                        );
                    } else {
                        ui.label(format!("Output Pad: {}", pad_name));
                    }

                    ui.indent(pad_name, |ui| {
                        // Find properties for this pad from element_info
                        if let Some(info) = element_info {
                            // Check if there's a matching source pad in metadata (try template matching)
                            let pad_info = info
                                .src_pads
                                .iter()
                                .find(|p| Self::matches_pad_template(pad_name, &p.name));

                            if let Some(pad_info) = pad_info {
                                if !pad_info.properties.is_empty() {
                                    for prop_info in &pad_info.properties {
                                        Self::show_pad_property_from_info(
                                            ui, element, pad_name, prop_info,
                                        );
                                    }
                                } else {
                                    ui.small("No configurable properties");
                                }
                            } else {
                                ui.small(format!(
                                    "No metadata for pad (tried matching: {})",
                                    pad_name
                                ));
                            }
                        } else {
                            ui.small("No element metadata available");
                        }
                    });
                    ui.add_space(8.0);
                }
            });
    }

    /// Show the property inspector for the given block.
    pub fn show_block(
        ui: &mut Ui,
        block: &mut BlockInstance,
        definition: &BlockDefinition,
        flow_id: Option<strom_types::FlowId>,
    ) {
        let block_id = block.id.clone();
        ui.push_id(&block_id, |ui| {
            // Block name (read-only)
            ui.horizontal(|ui| {
                ui.label("Block:");
                ui.monospace(&definition.name);
            });

            // Block ID (read-only)
            ui.horizontal(|ui| {
                ui.label("ID:");
                ui.monospace(&block.id);
            });

            ui.separator();

            ui.label("💡 Only modified properties are saved");

            ScrollArea::both()
                .id_salt("block_properties_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if !definition.exposed_properties.is_empty() {
                        for exposed_prop in &definition.exposed_properties {
                            Self::show_exposed_property(
                                ui,
                                block,
                                exposed_prop,
                                definition,
                                flow_id,
                            );
                        }
                    } else {
                        ui.label("This block has no configurable properties");
                    }

                    // Show SDP for AES67 output blocks
                    if definition.id == "builtin.aes67_output" {
                        ui.separator();
                        ui.heading("📡 SDP (Session Description)");
                        ui.add_space(4.0);

                        // Get SDP from runtime_data (only available when flow is running)
                        let sdp = block
                            .runtime_data
                            .as_ref()
                            .and_then(|data| data.get("sdp"))
                            .map(|s| s.as_str());

                        if let Some(mut sdp_text) = sdp {
                            ui.label("Copy this SDP to configure receivers:");
                            ui.add_space(4.0);

                            // Display SDP in a code-style text box
                            ui.add(
                                egui::TextEdit::multiline(&mut sdp_text)
                                    .desired_rows(12)
                                    .desired_width(f32::INFINITY)
                                    .code_editor()
                                    .interactive(false),
                            );

                            ui.add_space(4.0);

                            // Copy button
                            if ui.button("📋 Copy to Clipboard").clicked() {
                                ui.ctx().copy_text(sdp_text.to_string());
                            }
                        } else {
                            ui.colored_label(
                                Color32::from_rgb(200, 200, 100),
                                "⚠ SDP is only available when the flow is running",
                            );
                            ui.add_space(4.0);
                            ui.small("Start the flow to generate SDP based on the actual stream capabilities.");
                        }
                    }
                });
        });
    }

    fn show_exposed_property(
        ui: &mut Ui,
        block: &mut BlockInstance,
        exposed_prop: &ExposedProperty,
        _definition: &BlockDefinition,
        _flow_id: Option<strom_types::FlowId>,
    ) {
        let prop_name = &exposed_prop.name;
        let default_value = exposed_prop.default_value.as_ref();
        let is_multiline = matches!(
            exposed_prop.property_type,
            strom_types::block::PropertyType::Multiline
        );

        // Get current value or use default
        let mut current_value = block.properties.get(prop_name).cloned();
        let has_custom_value = current_value.is_some();

        if current_value.is_none() {
            current_value = default_value.cloned();
        }

        // For multiline, use vertical layout
        if is_multiline {
            // Property name with indicator
            ui.horizontal(|ui| {
                if has_custom_value {
                    ui.colored_label(
                        Color32::from_rgb(150, 100, 255), // Purple for blocks
                        format!("● {}:", prop_name),
                    );
                } else {
                    ui.label(format!("{}:", prop_name));
                }

                // Reset button if modified
                if has_custom_value
                    && ui
                        .small_button("↺")
                        .on_hover_text("Reset to default")
                        .clicked()
                {
                    block.properties.remove(prop_name);
                }
            });

            // Multiline editor
            let mut text = match current_value {
                Some(PropertyValue::String(s)) => s,
                _ => String::new(),
            };

            let response = ui.add(
                egui::TextEdit::multiline(&mut text)
                    .desired_rows(6)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );

            if response.changed() {
                // Only save if different from default
                if let Some(PropertyValue::String(default)) = default_value {
                    if text != *default {
                        block
                            .properties
                            .insert(prop_name.clone(), PropertyValue::String(text));
                    } else {
                        block.properties.remove(prop_name);
                    }
                } else if !text.is_empty() {
                    block
                        .properties
                        .insert(prop_name.clone(), PropertyValue::String(text));
                } else {
                    block.properties.remove(prop_name);
                }
            }
        } else {
            // For non-multiline, use horizontal layout
            ui.horizontal(|ui| {
                // Show property name with indicator if modified
                if has_custom_value {
                    ui.colored_label(
                        Color32::from_rgb(150, 100, 255), // Purple for blocks
                        format!("● {}:", prop_name),
                    );
                } else {
                    ui.label(format!("{}:", prop_name));
                }

                if let Some(mut value) = current_value {
                    // Note: Block properties use strom_types::PropertyType which is different from
                    // element::PropertyType. For now, we pass None to show_property_editor which means
                    // no constraints (no sliders/enums). We could add a conversion or separate editor later.
                    let changed = Self::show_property_editor(
                        ui,
                        &mut value,
                        None, // TODO: Convert block::PropertyType to element::PropertyType
                        default_value,
                    );

                    if changed {
                        // Only save if different from default
                        if let Some(default) = default_value {
                            if !Self::values_equal(&value, default) {
                                block.properties.insert(prop_name.clone(), value);
                            } else {
                                block.properties.remove(prop_name);
                            }
                        } else {
                            block.properties.insert(prop_name.clone(), value);
                        }
                    }
                }

                // Reset button if modified
                if has_custom_value
                    && ui
                        .small_button("↺")
                        .on_hover_text("Reset to default")
                        .clicked()
                {
                    block.properties.remove(prop_name);
                }
            });
        }

        // Show description
        if !exposed_prop.description.is_empty() {
            ui.indent(prop_name, |ui| {
                ui.small(&exposed_prop.description);
            });
        }

        // Add spacing after each property
        ui.add_space(8.0);
    }

    fn show_pad_property_from_info(
        ui: &mut Ui,
        element: &mut Element,
        pad_name: &str,
        prop_info: &PropertyInfo,
    ) {
        let prop_name = &prop_info.name;
        let default_value = prop_info.default_value.as_ref();

        // Get current value from pad_properties or use default
        let mut current_value = element
            .pad_properties
            .get(pad_name)
            .and_then(|props| props.get(prop_name))
            .cloned();
        let has_custom_value = current_value.is_some();

        if current_value.is_none() {
            current_value = default_value.cloned();

            // For enum properties without default value, initialize to first option
            if current_value.is_none() {
                if let PropertyType::Enum { values } = &prop_info.property_type {
                    if let Some(first_value) = values.first() {
                        current_value = Some(PropertyValue::String(first_value.clone()));
                    }
                }
            }
        }

        ui.horizontal(|ui| {
            // Show property name with indicator if modified
            if has_custom_value {
                ui.colored_label(
                    Color32::from_rgb(255, 150, 100), // Orange for pad properties
                    format!("● {}:", prop_name),
                );
            } else {
                ui.label(format!("{}:", prop_name));
            }

            if let Some(mut value) = current_value {
                let changed = Self::show_property_editor(
                    ui,
                    &mut value,
                    Some(&prop_info.property_type),
                    default_value,
                );

                if changed {
                    // Ensure the pad_properties map exists
                    element
                        .pad_properties
                        .entry(pad_name.to_string())
                        .or_default();

                    // Only save if different from default
                    if let Some(default) = default_value {
                        if !Self::values_equal(&value, default) {
                            element
                                .pad_properties
                                .get_mut(pad_name)
                                .unwrap()
                                .insert(prop_name.clone(), value);
                        } else {
                            // Remove if same as default
                            if let Some(props) = element.pad_properties.get_mut(pad_name) {
                                props.remove(prop_name);
                                // Clean up empty pad property maps
                                if props.is_empty() {
                                    element.pad_properties.remove(pad_name);
                                }
                            }
                        }
                    } else {
                        element
                            .pad_properties
                            .get_mut(pad_name)
                            .unwrap()
                            .insert(prop_name.clone(), value);
                    }
                }
            }

            // Reset button if modified
            if has_custom_value
                && ui
                    .small_button("↺")
                    .on_hover_text("Reset to default")
                    .clicked()
            {
                if let Some(props) = element.pad_properties.get_mut(pad_name) {
                    props.remove(prop_name);
                    // Clean up empty pad property maps
                    if props.is_empty() {
                        element.pad_properties.remove(pad_name);
                    }
                }
            }
        });

        // Show description
        if !prop_info.description.is_empty() {
            ui.indent(prop_name, |ui| {
                ui.small(&prop_info.description);
            });
        }

        // Add spacing after each property
        ui.add_space(8.0);
    }

    fn show_property_from_info(ui: &mut Ui, element: &mut Element, prop_info: &PropertyInfo) {
        let prop_name = &prop_info.name;
        let default_value = prop_info.default_value.as_ref();

        // Get current value or use default
        let mut current_value = element.properties.get(prop_name).cloned();
        let has_custom_value = current_value.is_some();

        if current_value.is_none() {
            current_value = default_value.cloned();

            // For enum properties without default value, initialize to first option
            if current_value.is_none() {
                if let PropertyType::Enum { values } = &prop_info.property_type {
                    if let Some(first_value) = values.first() {
                        current_value = Some(PropertyValue::String(first_value.clone()));
                    }
                }
            }
        }

        ui.horizontal(|ui| {
            // Show property name with indicator if modified
            if has_custom_value {
                ui.colored_label(
                    Color32::from_rgb(100, 200, 255),
                    format!("● {}:", prop_name),
                );
            } else {
                ui.label(format!("{}:", prop_name));
            }

            if let Some(mut value) = current_value {
                let changed = Self::show_property_editor(
                    ui,
                    &mut value,
                    Some(&prop_info.property_type),
                    default_value,
                );

                if changed {
                    // Only save if different from default
                    if let Some(default) = default_value {
                        if !Self::values_equal(&value, default) {
                            element.properties.insert(prop_name.clone(), value);
                        } else {
                            element.properties.remove(prop_name);
                        }
                    } else {
                        element.properties.insert(prop_name.clone(), value);
                    }
                }
            }

            // Reset button if modified
            if has_custom_value
                && ui
                    .small_button("↺")
                    .on_hover_text("Reset to default")
                    .clicked()
            {
                element.properties.remove(prop_name);
            }
        });

        // Show description
        if !prop_info.description.is_empty() {
            ui.indent(prop_name, |ui| {
                ui.small(&prop_info.description);
            });
        }

        // Add spacing after each property
        ui.add_space(8.0);
    }

    fn values_equal(a: &PropertyValue, b: &PropertyValue) -> bool {
        match (a, b) {
            (PropertyValue::String(a), PropertyValue::String(b)) => a == b,
            (PropertyValue::Int(a), PropertyValue::Int(b)) => a == b,
            (PropertyValue::UInt(a), PropertyValue::UInt(b)) => a == b,
            (PropertyValue::Float(a), PropertyValue::Float(b)) => (a - b).abs() < 0.0001,
            (PropertyValue::Bool(a), PropertyValue::Bool(b)) => a == b,
            _ => false,
        }
    }

    fn show_property_editor(
        ui: &mut Ui,
        value: &mut PropertyValue,
        prop_type: Option<&PropertyType>,
        _default_value: Option<&PropertyValue>,
    ) -> bool {
        match (value, prop_type) {
            (PropertyValue::String(s), Some(PropertyType::Enum { values })) => {
                // Enum dropdown
                let mut changed = false;
                egui::ComboBox::from_id_salt(ui.next_auto_id())
                    .selected_text(s.as_str())
                    .show_ui(ui, |ui| {
                        for val in values {
                            if ui.selectable_label(s == val, val).clicked() {
                                *s = val.clone();
                                changed = true;
                            }
                        }
                    });
                changed
            }
            (PropertyValue::String(s), _) => ui.text_edit_singleline(s).changed(),
            (PropertyValue::Int(i), Some(PropertyType::Int { min, max })) => {
                ui.add(egui::Slider::new(i, *min..=*max)).changed()
            }
            (PropertyValue::Int(i), _) => ui.add(egui::DragValue::new(i)).changed(),
            (PropertyValue::UInt(u), Some(PropertyType::UInt { min, max })) => {
                ui.add(egui::Slider::new(u, *min..=*max)).changed()
            }
            (PropertyValue::UInt(u), _) => ui.add(egui::DragValue::new(u)).changed(),
            (PropertyValue::Float(f), Some(PropertyType::Float { min, max })) => {
                ui.add(egui::Slider::new(f, *min..=*max)).changed()
            }
            (PropertyValue::Float(f), _) => ui.add(egui::DragValue::new(f).speed(0.1)).changed(),
            (PropertyValue::Bool(b), _) => ui.checkbox(b, "").changed(),
        }
    }
}
