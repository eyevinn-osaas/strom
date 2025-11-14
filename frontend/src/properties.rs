//! Property inspector for editing element properties.

use egui::{Color32, ScrollArea, Ui};
use strom_types::{
    block::ExposedProperty,
    element::{ElementInfo, PropertyInfo, PropertyType},
    BlockDefinition, BlockInstance, Element, PropertyValue,
};

/// Property inspector panel.
pub struct PropertyInspector;

impl PropertyInspector {
    /// Show the property inspector for the given element.
    pub fn show(ui: &mut Ui, element: &mut Element, element_info: Option<&ElementInfo>) {
        let element_id = element.id.clone();
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

            ui.label("💡 Only modified properties are saved");

            ScrollArea::both()
                .id_salt("properties_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Show properties from metadata if available
                    if let Some(info) = element_info {
                        if !info.properties.is_empty() {
                            for prop_info in &info.properties {
                                Self::show_property_from_info(ui, element, prop_info);
                            }
                        } else {
                            ui.label("No properties available for this element");
                        }
                    } else {
                        ui.label("No element metadata available");
                    }

                    // Show any additional custom properties that aren't in the metadata
                    if let Some(info) = element_info {
                        let known_props: std::collections::HashSet<String> =
                            info.properties.iter().map(|p| p.name.clone()).collect();

                        let custom_keys: Vec<String> = element
                            .properties
                            .keys()
                            .filter(|k| !known_props.contains(*k))
                            .cloned()
                            .collect();

                        if !custom_keys.is_empty() {
                            ui.separator();
                            ui.heading("Custom Properties");
                            ui.add_space(4.0);
                            for key in custom_keys {
                                let should_remove = ui
                                    .horizontal(|ui| {
                                        ui.label(format!("{}:", key));
                                        if let Some(value) = element.properties.get_mut(&key) {
                                            Self::show_property_editor(ui, value, None, None);
                                        }
                                        ui.small_button("🗑")
                                            .on_hover_text("Remove property")
                                            .clicked()
                                    })
                                    .inner;

                                if should_remove {
                                    element.properties.remove(&key);
                                }
                                ui.add_space(8.0);
                            }
                        }
                    }

                    ui.separator();

                    // Add new property
                    ui.collapsing("Add Custom Property", |ui| {
                        ui.label("Add custom properties manually:");

                        // Use egui's memory system for persistent state
                        let id = ui.make_persistent_id("new_property_state");
                        let mut state = ui.memory_mut(|mem| {
                            mem.data
                                .get_temp::<(String, String)>(id)
                                .unwrap_or_else(|| (String::new(), String::new()))
                        });

                        ui.horizontal(|ui| {
                            ui.label("Key:");
                            ui.text_edit_singleline(&mut state.0);
                        });

                        ui.horizontal(|ui| {
                            ui.label("Value:");
                            ui.text_edit_singleline(&mut state.1);
                        });

                        let should_add = ui.button("Add Property").clicked();

                        if should_add && !state.0.is_empty() {
                            element
                                .properties
                                .insert(state.0.clone(), PropertyValue::String(state.1.clone()));
                            state.0.clear();
                            state.1.clear();
                        }

                        // Save state back to memory
                        ui.memory_mut(|mem| {
                            mem.data.insert_temp(id, state);
                        });
                    });
                });
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
                            Self::show_exposed_property(ui, block, exposed_prop);
                        }
                    } else {
                        ui.label("This block has no configurable properties");
                    }

                    // Show SDP for AES67 output blocks
                    if definition.id == "builtin.aes67_output" && flow_id.is_some() {
                        ui.separator();
                        ui.heading("📡 SDP (Session Description)");
                        ui.add_space(4.0);
                        ui.label("Copy this SDP to configure receivers:");
                        ui.add_space(4.0);

                        // Store SDP in localStorage with flow+block specific key
                        let sdp_key = format!("strom_sdp_{}_{}", flow_id.unwrap(), block_id);

                        // Try to get SDP from localStorage
                        let sdp: Option<String> = if let Some(window) = web_sys::window() {
                            if let Some(storage) = window.local_storage().ok().flatten() {
                                storage.get_item(&sdp_key).ok().flatten()
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        if let Some(sdp_text) = &sdp {
                            // Display SDP in a code-style text box
                            ui.add(
                                egui::TextEdit::multiline(&mut sdp_text.as_str())
                                    .desired_rows(12)
                                    .desired_width(f32::INFINITY)
                                    .code_editor()
                                    .interactive(false),
                            );

                            ui.add_space(4.0);

                            // Copy button
                            if ui.button("📋 Copy to Clipboard").clicked() {
                                ui.ctx().copy_text(sdp_text.clone());
                            }
                        } else {
                            ui.label("Fetching SDP...");
                            // Signal that we need to fetch SDP
                            if let Some(window) = web_sys::window() {
                                if let Some(storage) = window.local_storage().ok().flatten() {
                                    let _ = storage.set_item("strom_fetch_sdp", &sdp_key);
                                }
                            }
                        }
                    }
                });
        });
    }

    fn show_exposed_property(
        ui: &mut Ui,
        block: &mut BlockInstance,
        exposed_prop: &ExposedProperty,
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

    fn show_property_from_info(ui: &mut Ui, element: &mut Element, prop_info: &PropertyInfo) {
        let prop_name = &prop_info.name;
        let default_value = prop_info.default_value.as_ref();

        // Get current value or use default
        let mut current_value = element.properties.get(prop_name).cloned();
        let has_custom_value = current_value.is_some();

        if current_value.is_none() {
            current_value = default_value.cloned();
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
