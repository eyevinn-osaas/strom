//! GStreamer element discovery and introspection.

use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use strom_types::element::{ElementInfo, PadInfo};
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
            let pad_info = PadInfo {
                name: pad_template.name_template().to_string(),
                caps: pad_template.caps().to_string(),
            };

            match pad_template.direction() {
                gst::PadDirection::Src => src_pads.push(pad_info),
                gst::PadDirection::Sink => sink_pads.push(pad_info),
                _ => {}
            }
        }

        // For now, skip detailed property introspection as it's complex
        // We'll add a simplified version
        let properties = Vec::new();

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

    /// Clear the cache (useful for testing or forcing refresh).
    pub fn clear_cache(&mut self) {
        self.cache.clear();
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
