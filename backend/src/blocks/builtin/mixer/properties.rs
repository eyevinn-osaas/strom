use std::collections::HashMap;

use gstreamer as gst;
use gstreamer::prelude::*;
use strom_types::PropertyValue;

use super::{DEFAULT_CHANNELS, MAX_AUX_BUSES, MAX_CHANNELS, MAX_GROUPS};

/// Parse number of channels from properties.
pub(super) fn parse_num_channels(properties: &HashMap<String, PropertyValue>) -> usize {
    properties
        .get("num_channels")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as usize),
            PropertyValue::UInt(u) => Some(*u as usize),
            PropertyValue::String(s) => s.parse::<usize>().ok(),
            _ => None,
        })
        .unwrap_or(DEFAULT_CHANNELS)
        .clamp(1, MAX_CHANNELS)
}

/// Parse number of aux buses from properties.
pub(super) fn parse_num_aux_buses(properties: &HashMap<String, PropertyValue>) -> usize {
    properties
        .get("num_aux_buses")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as usize),
            PropertyValue::UInt(u) => Some(*u as usize),
            PropertyValue::String(s) => s.parse::<usize>().ok(),
            _ => None,
        })
        .unwrap_or(0)
        .clamp(0, MAX_AUX_BUSES)
}

/// Parse number of groups from properties.
pub(super) fn parse_num_groups(properties: &HashMap<String, PropertyValue>) -> usize {
    properties
        .get("num_groups")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as usize),
            PropertyValue::UInt(u) => Some(*u as usize),
            PropertyValue::String(s) => s.parse::<usize>().ok(),
            _ => None,
        })
        .unwrap_or(0)
        .clamp(0, MAX_GROUPS)
}

/// Get a float property with default.
pub(super) fn get_float_prop(
    properties: &HashMap<String, PropertyValue>,
    name: &str,
    default: f64,
) -> f64 {
    properties
        .get(name)
        .and_then(|v| match v {
            PropertyValue::Float(f) => Some(*f),
            PropertyValue::Int(i) => Some(*i as f64),
            _ => None,
        })
        .unwrap_or(default)
}

/// Get a bool property with default.
pub(super) fn get_bool_prop(
    properties: &HashMap<String, PropertyValue>,
    name: &str,
    default: bool,
) -> bool {
    properties
        .get(name)
        .and_then(|v| match v {
            PropertyValue::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(default)
}

/// Get a string property with default.
pub(super) fn get_string_prop<'a>(
    properties: &'a HashMap<String, PropertyValue>,
    name: &str,
    default: &'a str,
) -> &'a str {
    properties
        .get(name)
        .and_then(|v| match v {
            PropertyValue::String(s) => Some(s.as_str()),
            _ => None,
        })
        .unwrap_or(default)
}

/// Convert dB to linear scale.
pub(super) fn db_to_linear(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

/// Convert linear scale to dB.
pub(super) fn linear_to_db(linear: f64) -> f64 {
    if linear <= 0.0 {
        -120.0 // floor
    } else {
        20.0 * linear.log10()
    }
}

/// Translate a property name and value from LV2 conventions to lsp-rs conventions.
///
/// The ExposedProperty mappings use LV2 property names (gt, at, rt, al, cr, mk, kn, th, f-N, g-N, q-N).
/// When the target element is from lsp-plugins-rs, this function translates the property name
/// and adjusts the value format where needed (e.g., LV2 uses linear gain, Rust uses dB).
///
/// Returns (translated_prop_name, translated_value) or None if no translation needed.
pub fn translate_property_for_element(
    element: &gst::Element,
    prop_name: &str,
    value: &PropertyValue,
) -> Option<(String, PropertyValue)> {
    // Use GObject type name instead of factory() which can SIGSEGV
    // when static plugins and LV2 plugins coexist.
    let type_name = element.type_().name();

    if type_name == "LspRsGate" {
        let (new_name, new_value) = match prop_name {
            "gt" => {
                // LV2: gt is linear (already transformed by db_to_linear).
                // Rust: open-threshold is dB. Reverse the transform.
                let db_val = match value {
                    PropertyValue::Float(v) => linear_to_db(*v),
                    _ => return None,
                };
                ("open-threshold".to_string(), PropertyValue::Float(db_val))
            }
            "at" => ("attack".to_string(), value.clone()),
            "rt" => ("release".to_string(), value.clone()),
            "enabled" => return None, // same name, no translation needed
            _ => return None,
        };
        return Some((new_name, new_value));
    }

    if type_name == "LspRsCompressor" {
        let (new_name, new_value) = match prop_name {
            "al" => {
                // Both use linear, same transform
                ("threshold".to_string(), value.clone())
            }
            "cr" => ("ratio".to_string(), value.clone()),
            "at" => ("attack".to_string(), value.clone()),
            "rt" => ("release".to_string(), value.clone()),
            "mk" => {
                // Both use linear, same transform
                ("makeup-gain".to_string(), value.clone())
            }
            "kn" => {
                // Both use linear, same transform
                ("knee".to_string(), value.clone())
            }
            "enabled" => return None,
            _ => return None,
        };
        return Some((new_name, new_value));
    }

    if type_name == "LspRsEqualizer" {
        // EQ band properties: f-N -> bandN-frequency, g-N -> bandN-gain, q-N -> bandN-q
        if let Some(band) = prop_name.strip_prefix("f-") {
            return Some((format!("band{}-frequency", band), value.clone()));
        }
        if let Some(band) = prop_name.strip_prefix("g-") {
            // LV2: g-N is linear (already transformed by db_to_linear).
            // Rust: bandN-gain is dB. Reverse the transform.
            let db_val = match value {
                PropertyValue::Float(v) => linear_to_db(*v),
                _ => return None,
            };
            return Some((format!("band{}-gain", band), PropertyValue::Float(db_val)));
        }
        if let Some(band) = prop_name.strip_prefix("q-") {
            return Some((format!("band{}-q", band), value.clone()));
        }
        if prop_name == "enabled" {
            return None;
        }
        return None;
    }

    if type_name == "LspRsLimiter" {
        let (new_name, new_value) = match prop_name {
            "th" => {
                // LV2: th is linear (already transformed by db_to_linear).
                // Rust: threshold is dB. Reverse the transform.
                let db_val = match value {
                    PropertyValue::Float(v) => linear_to_db(*v),
                    _ => return None,
                };
                ("threshold".to_string(), PropertyValue::Float(db_val))
            }
            "enabled" => return None,
            _ => return None,
        };
        return Some((new_name, new_value));
    }

    None
}
