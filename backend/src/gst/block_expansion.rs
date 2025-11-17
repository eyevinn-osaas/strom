//! Block expansion logic - converts block instances to native GStreamer elements.

use crate::blocks::BlockRegistry;
use std::io::Write;
use strom_types::{BlockInstance, Element, ExternalPad, Link};
use tracing::debug;

use super::PipelineError;

/// Result of expanding blocks into elements and links.
pub struct ExpandedPipeline {
    pub elements: Vec<Element>,
    pub links: Vec<Link>,
}

/// Expand block instances into native GStreamer elements and links.
pub async fn expand_blocks(
    blocks: &[BlockInstance],
    elements: &[Element],
    links: &[Link],
    registry: &BlockRegistry,
) -> Result<ExpandedPipeline, PipelineError> {
    let mut expanded_elements = elements.to_vec();
    let mut expanded_links = Vec::new();

    debug!("Expanding {} block instance(s)", blocks.len());

    for block_instance in blocks {
        // Get block definition from registry
        let definition = registry
            .get_by_id(&block_instance.block_definition_id)
            .await
            .ok_or_else(|| {
                PipelineError::InvalidFlow(format!(
                    "Block definition not found: {}",
                    block_instance.block_definition_id
                ))
            })?;

        debug!(
            "Expanding block instance '{}' (definition: {})",
            block_instance.id, definition.name
        );

        // Clone and namespace internal elements
        for internal_elem in &definition.elements {
            let mut namespaced_elem = internal_elem.clone();
            // Namespace the element ID with block instance ID
            namespaced_elem.id = format!("{}:{}", block_instance.id, internal_elem.id);
            // Clear position (internal elements don't have visual positions)
            namespaced_elem.position = None;

            // Apply exposed property values to internal elements
            for (prop_name, prop_value) in &block_instance.properties {
                // Find the exposed property definition
                if let Some(exposed_prop) = definition
                    .exposed_properties
                    .iter()
                    .find(|p| p.name == *prop_name)
                {
                    // Check if this exposed property maps to this internal element
                    if exposed_prop.mapping.element_id == internal_elem.id {
                        debug!(
                            "Mapping property '{}' -> {}:{}",
                            prop_name, namespaced_elem.id, exposed_prop.mapping.property_name
                        );

                        // Handle transformations if specified
                        let final_value = if let Some(transform) = &exposed_prop.mapping.transform {
                            match transform.as_str() {
                                "write_temp_file" => {
                                    // Write property value to a temp file and return path
                                    if let strom_types::PropertyValue::String(content) = prop_value
                                    {
                                        match write_temp_file(content) {
                                            Ok(path) => strom_types::PropertyValue::String(path),
                                            Err(e) => {
                                                return Err(PipelineError::InvalidFlow(format!(
                                                    "Failed to write temp file for property '{}': {}",
                                                    prop_name, e
                                                )));
                                            }
                                        }
                                    } else {
                                        return Err(PipelineError::InvalidFlow(format!(
                                            "Property '{}' must be a string for write_temp_file transform",
                                            prop_name
                                        )));
                                    }
                                }
                                other => {
                                    return Err(PipelineError::InvalidFlow(format!(
                                        "Unknown property transform: {}",
                                        other
                                    )));
                                }
                            }
                        } else {
                            prop_value.clone()
                        };

                        namespaced_elem
                            .properties
                            .insert(exposed_prop.mapping.property_name.clone(), final_value);
                    }
                }
            }

            expanded_elements.push(namespaced_elem);
        }

        // Namespace and add internal links
        for internal_link in &definition.internal_links {
            let namespaced_link = Link {
                from: namespace_pad(&block_instance.id, &internal_link.from),
                to: namespace_pad(&block_instance.id, &internal_link.to),
            };
            debug!(
                "Internal link: {} -> {}",
                namespaced_link.from, namespaced_link.to
            );
            expanded_links.push(namespaced_link);
        }
    }

    // Resolve and add external links (between elements and/or blocks)
    for link in links {
        let from = resolve_pad(link.from.as_str(), blocks, registry).await?;
        let to = resolve_pad(link.to.as_str(), blocks, registry).await?;

        debug!("External link: {} -> {}", from, to);
        expanded_links.push(Link { from, to });
    }

    debug!(
        "Block expansion complete: {} elements, {} links",
        expanded_elements.len(),
        expanded_links.len()
    );

    Ok(ExpandedPipeline {
        elements: expanded_elements,
        links: expanded_links,
    })
}

/// Namespace a pad reference with block instance ID.
/// Example: "filesrc:src" -> "block1:filesrc:src"
fn namespace_pad(block_id: &str, pad_ref: &str) -> String {
    format!("{}:{}", block_id, pad_ref)
}

/// Write content to a temporary file and return its path.
///
/// This is used for property transforms like "write_temp_file" to allow
/// passing string data (like SDP content) to elements that expect file paths.
fn write_temp_file(content: &str) -> std::io::Result<String> {
    use tempfile::NamedTempFile;

    // Create a persistent temporary file (won't be deleted automatically)
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(content.as_bytes())?;
    temp_file.flush()?;

    // Get the path and convert temp file to persistent (keeps file after drop)
    let (_file, path) = temp_file.keep()?;
    let path_str = path.to_string_lossy().to_string();

    debug!("Created temp file for property transform: {}", path_str);
    Ok(path_str)
}

/// Resolve a pad reference, handling block external pads.
///
/// If the pad reference is to a block's external pad, resolve it to the
/// internal element:pad that it maps to. Otherwise, return as-is.
async fn resolve_pad(
    pad_ref: &str,
    blocks: &[BlockInstance],
    registry: &BlockRegistry,
) -> Result<String, PipelineError> {
    // Check if this references a block's external pad
    // Format: "block_id:external_pad_name"
    for block_instance in blocks {
        let block_prefix = format!("{}:", block_instance.id);
        if pad_ref.starts_with(&block_prefix) {
            // Extract external pad name
            let external_pad_name = &pad_ref[block_prefix.len()..];

            // Get block definition
            let definition = registry
                .get_by_id(&block_instance.block_definition_id)
                .await
                .ok_or_else(|| {
                    PipelineError::InvalidFlow(format!(
                        "Block definition not found: {}",
                        block_instance.block_definition_id
                    ))
                })?;

            // Find matching external pad in definition
            let all_pads: Vec<&ExternalPad> = definition
                .external_pads
                .inputs
                .iter()
                .chain(definition.external_pads.outputs.iter())
                .collect();

            if let Some(external_pad) = all_pads.iter().find(|p| p.name == external_pad_name) {
                // Resolve to namespaced internal element:pad
                let resolved = format!(
                    "{}:{}:{}",
                    block_instance.id,
                    external_pad.internal_element_id,
                    external_pad.internal_pad_name
                );

                debug!(
                    "Resolved block external pad '{}' -> '{}'",
                    pad_ref, resolved
                );

                return Ok(resolved);
            } else {
                return Err(PipelineError::InvalidFlow(format!(
                    "External pad '{}' not found in block '{}'",
                    external_pad_name, block_instance.id
                )));
            }
        }
    }

    // Not a block reference, return as-is (regular element:pad)
    Ok(pad_ref.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_namespace_pad() {
        assert_eq!(namespace_pad("block1", "elem:src"), "block1:elem:src");
        assert_eq!(namespace_pad("myblock", "src"), "myblock:src");
    }

    #[tokio::test]
    async fn test_expand_no_blocks() {
        let registry = crate::blocks::BlockRegistry::new("blocks.json");

        let elements = vec![Element {
            id: "src".to_string(),
            element_type: "videotestsrc".to_string(),
            properties: HashMap::new(),
            pad_properties: HashMap::new(),
            position: None,
        }];

        let result = expand_blocks(&[], &elements, &[], &registry).await;
        assert!(result.is_ok());

        let expanded = result.unwrap();
        assert_eq!(expanded.elements.len(), 1);
        assert_eq!(expanded.links.len(), 0);
    }
}
