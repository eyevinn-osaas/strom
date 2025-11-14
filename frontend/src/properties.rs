//! Property inspector for editing element properties.

use egui::{Color32, ScrollArea, Ui};
use strom_types::{
    element::{ElementInfo, PropertyInfo, PropertyType},
    Element, PropertyValue,
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
