//! GStreamer element discovery and introspection.

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use std::collections::HashMap;
use strom_types::element::{ElementInfo, MediaType, PadInfo, PadPresence};
use tracing::{debug, warn};

/// GStreamer element discovery service.
pub struct ElementDiscovery {
    /// Cached element information
    cache: HashMap<String, ElementInfo>,
}

impl ElementDiscovery {
    /// Create a new element discovery service.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Discover all available GStreamer elements.
    pub fn discover_all(&mut self) -> Vec<ElementInfo> {
        debug!("Discovering all GStreamer elements...");

        let registry = gst::Registry::get();
        let mut elements = Vec::new();

        let features = registry.features(gst::ElementFactory::static_type());

        for feature in features {
            let Some(factory) = feature.downcast_ref::<gst::ElementFactory>() else {
                continue;
            };

            let name = factory.name().to_string();

            // Skip elements we've already cached
            if self.cache.contains_key(&name) {
                elements.push(self.cache[&name].clone());
                continue;
            }

            // Try to introspect the element
            match self.introspect_element_factory(factory) {
                Ok(info) => {
                    self.cache.insert(name, info.clone());
                    elements.push(info);
                }
                Err(e) => {
                    warn!("Failed to introspect element {}: {}", factory.name(), e);
                }
            }
        }

        debug!("Discovered {} elements", elements.len());
        elements
    }

    /// Get information about a specific element by name.
    pub fn get_element_info(&mut self, name: &str) -> Option<ElementInfo> {
        // Check cache first
        if let Some(info) = self.cache.get(name) {
            return Some(info.clone());
        }

        // Try to find and introspect the element
        let registry = gst::Registry::get();
        let factory = registry.find_feature(name, gst::ElementFactory::static_type())?;
        let factory = factory
            .downcast_ref::<gst::ElementFactory>()
            .expect("Feature is not an ElementFactory");

        match self.introspect_element_factory(factory) {
            Ok(info) => {
                self.cache.insert(name.to_string(), info.clone());
                Some(info)
            }
            Err(e) => {
                warn!("Failed to introspect element {}: {}", name, e);
                None
            }
        }
    }

    /// Introspect a GStreamer element factory.
    fn introspect_element_factory(
        &self,
        factory: &gst::ElementFactory,
    ) -> anyhow::Result<ElementInfo> {
        let name = factory.name().to_string();
        let description = factory
            .metadata("long-name")
            .map(|s| s.to_string())
            .unwrap_or_else(|| name.clone());

        // Determine category
        let klass = factory
            .metadata("klass")
            .map(|s| s.to_string())
            .unwrap_or_default();
        let category = Self::determine_category(&klass);

        // Get pad templates
        let mut src_pads = Vec::new();
        let mut sink_pads = Vec::new();

        for pad_template in factory.static_pad_templates() {
            let caps_string = pad_template.caps().to_string();

            // Determine pad presence
            let presence = match pad_template.presence() {
                gst::PadPresence::Always => PadPresence::Always,
                gst::PadPresence::Sometimes => PadPresence::Sometimes,
                gst::PadPresence::Request => PadPresence::Request,
            };

            // Determine media type from caps
            let media_type = Self::classify_media_type(&caps_string);

            let pad_info = PadInfo {
                name: pad_template.name_template().to_string(),
                caps: caps_string,
                presence,
                media_type,
            };

            match pad_template.direction() {
                gst::PadDirection::Src => src_pads.push(pad_info),
                gst::PadDirection::Sink => sink_pads.push(pad_info),
                _ => {}
            }
        }

        // Introspect element properties
        let properties = self.introspect_properties(factory)?;

        Ok(ElementInfo {
            name,
            description,
            category,
            src_pads,
            sink_pads,
            properties,
        })
    }

    /// Determine element category from its klass.
    fn determine_category(klass: &str) -> String {
        if klass.contains("Source") {
            "Source".to_string()
        } else if klass.contains("Sink") {
            "Sink".to_string()
        } else if klass.contains("Codec") || klass.contains("Encoder") || klass.contains("Decoder")
        {
            "Codec".to_string()
        } else if klass.contains("Filter") || klass.contains("Effect") {
            "Filter".to_string()
        } else if klass.contains("Converter") {
            "Converter".to_string()
        } else if klass.contains("Muxer") {
            "Muxer".to_string()
        } else if klass.contains("Demuxer") || klass.contains("Parser") {
            "Demuxer".to_string()
        } else if klass.contains("Network") {
            "Network".to_string()
        } else {
            "Other".to_string()
        }
    }

    /// Classify media type from caps string.
    fn classify_media_type(caps: &str) -> MediaType {
        let caps_lower = caps.to_lowercase();

        // Check for audio patterns
        let is_audio = caps_lower.contains("audio/") || caps_lower.contains("audio,");

        // Check for video patterns
        let is_video = caps_lower.contains("video/")
            || caps_lower.contains("video,")
            || caps_lower.contains("image/");

        // Classify based on what we found
        match (is_audio, is_video) {
            (true, true) => MediaType::Generic, // Both audio and video = generic/muxed
            (true, false) => MediaType::Audio,  // Audio only
            (false, true) => MediaType::Video,  // Video only
            (false, false) => MediaType::Generic, // Unknown or ANY caps = generic
        }
    }

    /// Clear the cache (useful for testing or forcing refresh).
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Introspect element properties from a factory.
    fn introspect_properties(
        &self,
        factory: &gst::ElementFactory,
    ) -> anyhow::Result<Vec<strom_types::element::PropertyInfo>> {
        use strom_types::element::{PropertyInfo, PropertyType, PropertyValue};

        // Create a temporary element instance to introspect properties
        let element = factory
            .create()
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create element: {}", e))?;

        let mut properties = Vec::new();

        // Get all properties from the element
        for pspec in element.list_properties() {
            let name = pspec.name().to_string();
            let description = pspec.blurb().map(|s| s.to_string()).unwrap_or_default();

            // Skip internal/private properties
            if name.starts_with("_") {
                continue;
            }

            // Skip write-only properties (not readable)
            if !pspec.flags().contains(glib::ParamFlags::READABLE) {
                debug!("Skipping write-only property: {}", name);
                continue;
            }

            // Determine property type and get default value
            let type_name = pspec.value_type().name();
            let (property_type, default_value) = match type_name {
                "gchararray" => {
                    // String property - use catch_unwind to handle potential panics
                    let default = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        element.property::<Option<String>>(&name)
                    }))
                    .ok()
                    .flatten()
                    .map(PropertyValue::String);
                    (PropertyType::String, default)
                }
                "gboolean" => {
                    // Boolean property
                    let default = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        element.property::<bool>(&name)
                    }))
                    .ok()
                    .map(PropertyValue::Bool);
                    (PropertyType::Bool, default)
                }
                "gint" | "glong" => {
                    // Signed integer property
                    if let Some(param_spec) = pspec.downcast_ref::<glib::ParamSpecInt>() {
                        let min = param_spec.minimum() as i64;
                        let max = param_spec.maximum() as i64;
                        let default =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                element.property::<i32>(&name)
                            }))
                            .ok()
                            .map(|v| PropertyValue::Int(v as i64));
                        (PropertyType::Int { min, max }, default)
                    } else if let Some(param_spec) = pspec.downcast_ref::<glib::ParamSpecLong>() {
                        let min = param_spec.minimum();
                        let max = param_spec.maximum();
                        let default =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                element.property::<i64>(&name)
                            }))
                            .ok()
                            .map(PropertyValue::Int);
                        (PropertyType::Int { min, max }, default)
                    } else {
                        continue;
                    }
                }
                "guint" | "gulong" => {
                    // Unsigned integer property
                    if let Some(param_spec) = pspec.downcast_ref::<glib::ParamSpecUInt>() {
                        let min = param_spec.minimum() as u64;
                        let max = param_spec.maximum() as u64;
                        let default =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                element.property::<u32>(&name)
                            }))
                            .ok()
                            .map(|v| PropertyValue::UInt(v as u64));
                        (PropertyType::UInt { min, max }, default)
                    } else if let Some(param_spec) = pspec.downcast_ref::<glib::ParamSpecULong>() {
                        let min = param_spec.minimum();
                        let max = param_spec.maximum();
                        let default =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                element.property::<u64>(&name)
                            }))
                            .ok()
                            .map(PropertyValue::UInt);
                        (PropertyType::UInt { min, max }, default)
                    } else {
                        continue;
                    }
                }
                "gint64" => {
                    // 64-bit signed integer
                    if let Some(param_spec) = pspec.downcast_ref::<glib::ParamSpecInt64>() {
                        let min = param_spec.minimum();
                        let max = param_spec.maximum();
                        let default =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                element.property::<i64>(&name)
                            }))
                            .ok()
                            .map(PropertyValue::Int);
                        (PropertyType::Int { min, max }, default)
                    } else {
                        continue;
                    }
                }
                "guint64" => {
                    // 64-bit unsigned integer
                    if let Some(param_spec) = pspec.downcast_ref::<glib::ParamSpecUInt64>() {
                        let min = param_spec.minimum();
                        let max = param_spec.maximum();
                        let default =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                element.property::<u64>(&name)
                            }))
                            .ok()
                            .map(PropertyValue::UInt);
                        (PropertyType::UInt { min, max }, default)
                    } else {
                        continue;
                    }
                }
                "gfloat" => {
                    // Float property
                    if let Some(param_spec) = pspec.downcast_ref::<glib::ParamSpecFloat>() {
                        let min = param_spec.minimum() as f64;
                        let max = param_spec.maximum() as f64;
                        let default =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                element.property::<f32>(&name)
                            }))
                            .ok()
                            .map(|v| PropertyValue::Float(v as f64));
                        (PropertyType::Float { min, max }, default)
                    } else {
                        continue;
                    }
                }
                "gdouble" => {
                    // Double property
                    if let Some(param_spec) = pspec.downcast_ref::<glib::ParamSpecDouble>() {
                        let min = param_spec.minimum();
                        let max = param_spec.maximum();
                        let default =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                element.property::<f64>(&name)
                            }))
                            .ok()
                            .map(PropertyValue::Float);
                        (PropertyType::Float { min, max }, default)
                    } else {
                        continue;
                    }
                }
                "GEnum" => {
                    // Enum property
                    if let Some(param_spec) = pspec.downcast_ref::<glib::ParamSpecEnum>() {
                        let enum_class = param_spec.enum_class();
                        let values: Vec<String> = enum_class
                            .values()
                            .iter()
                            .map(|v| v.name().to_string())
                            .collect();

                        // Try to get default value as enum index
                        let default =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                element.property::<i32>(&name)
                            }))
                            .ok()
                            .and_then(|idx| {
                                enum_class
                                    .value(idx)
                                    .map(|v| PropertyValue::String(v.name().to_string()))
                            });

                        (PropertyType::Enum { values }, default)
                    } else {
                        continue;
                    }
                }
                _ => {
                    // Skip unsupported property types
                    debug!(
                        "Skipping unsupported property type: {} ({})",
                        name, type_name
                    );
                    continue;
                }
            };

            properties.push(PropertyInfo {
                name,
                description,
                property_type,
                default_value,
            });
        }

        Ok(properties)
    }
}

impl Default for ElementDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_discovery() {
        gst::init().unwrap();
        let mut discovery = ElementDiscovery::new();
        let elements = discovery.discover_all();
        assert!(!elements.is_empty(), "Should discover some elements");
    }

    #[test]
    fn test_get_specific_element() {
        gst::init().unwrap();
        let mut discovery = ElementDiscovery::new();

        // Try to get a common element
        let info = discovery.get_element_info("fakesrc");
        assert!(info.is_some(), "Should find fakesrc element");

        if let Some(info) = info {
            assert_eq!(info.name, "fakesrc");
        }
    }
}
