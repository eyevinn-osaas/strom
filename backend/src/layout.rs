//! Automatic graph layout for flows.
//!
//! Provides hierarchical/topological layout algorithm to arrange elements
//! visually in a left-to-right flow based on their connectivity.

use std::collections::{HashMap, VecDeque};
use strom_types::Flow;
use tracing::debug;

/// Spacing between layers (horizontal)
const LAYER_SPACING: f32 = 350.0;

/// Spacing between elements in the same layer (vertical)
const ELEMENT_SPACING: f32 = 150.0;

/// Starting X position for first layer
const START_X: f32 = 50.0;

/// Starting Y position for first element
const START_Y: f32 = 50.0;

/// Check if a flow needs auto-layout applied.
///
/// Returns true if:
/// - There are at least 3 nodes (elements + blocks) to make layout meaningful
/// - All elements and blocks have identical positions (stacked at 0,0 or same location)
pub fn needs_auto_layout(flow: &Flow) -> bool {
    // Count total nodes (elements + blocks)
    let total_nodes = flow.elements.len() + flow.blocks.len();

    // Don't auto-layout trivial graphs (2 or fewer nodes)
    // A single element or one element + one block doesn't need automatic arrangement
    if total_nodes <= 2 {
        return false;
    }

    // Need at least some elements to layout (blocks alone aren't layouted)
    if flow.elements.is_empty() {
        return false;
    }

    // Collect all positions (elements and blocks)
    let mut all_positions: Vec<(f32, f32)> = flow.elements.iter().map(|e| e.position).collect();
    for block in &flow.blocks {
        all_positions.push((block.position.x, block.position.y));
    }

    // Check if all positions are identical (stacked)
    let first_pos = all_positions[0];
    let all_same = all_positions.iter().all(|&pos| pos == first_pos);

    all_same
}

/// Apply automatic hierarchical layout to a flow's elements.
///
/// Uses topological layering to arrange elements left-to-right based on
/// their connectivity. Elements are positioned in layers, with sources on
/// the left and sinks on the right.
///
/// The algorithm:
/// 1. Build dependency graph from links
/// 2. Assign layers using longest path from sources
/// 3. Position elements based on layer and vertical spacing
/// 4. Handle disconnected components
pub fn apply_auto_layout(flow: &mut Flow) {
    if flow.elements.is_empty() {
        return;
    }

    debug!(
        "Applying auto-layout to flow '{}' with {} elements",
        flow.name,
        flow.elements.len()
    );

    // Build adjacency lists (element_id -> [connected_element_ids])
    let mut outgoing: HashMap<String, Vec<String>> = HashMap::new();
    let mut incoming: HashMap<String, Vec<String>> = HashMap::new();

    // Initialize all elements in the maps
    for element in &flow.elements {
        outgoing.entry(element.id.clone()).or_default();
        incoming.entry(element.id.clone()).or_default();
    }

    // Build connectivity graph
    for link in &flow.links {
        let from_id = extract_element_id(&link.from);
        let to_id = extract_element_id(&link.to);

        outgoing
            .entry(from_id.clone())
            .or_default()
            .push(to_id.clone());
        incoming.entry(to_id.clone()).or_default().push(from_id);
    }

    // Assign layers using longest path from sources (BFS-based layering)
    let mut layers: HashMap<String, usize> = HashMap::new();
    let mut queue = VecDeque::new();

    // Find source elements (no incoming edges) and disconnected elements
    for element in &flow.elements {
        if incoming.get(&element.id).is_none_or(|v| v.is_empty()) {
            layers.insert(element.id.clone(), 0);
            queue.push_back(element.id.clone());
        }
    }

    // BFS to assign layers based on longest path
    while let Some(current_id) = queue.pop_front() {
        let current_layer = *layers.get(&current_id).unwrap();

        if let Some(neighbors) = outgoing.get(&current_id) {
            for neighbor_id in neighbors {
                let new_layer = current_layer + 1;
                let should_update = layers
                    .get(neighbor_id)
                    .is_none_or(|&existing_layer| new_layer > existing_layer);

                if should_update {
                    layers.insert(neighbor_id.clone(), new_layer);
                    queue.push_back(neighbor_id.clone());
                }
            }
        }
    }

    // Handle elements not yet in layers (disconnected components)
    for element in &flow.elements {
        if !layers.contains_key(&element.id) {
            layers.insert(element.id.clone(), 0);
        }
    }

    // Group elements by layer
    let mut elements_by_layer: HashMap<usize, Vec<String>> = HashMap::new();
    for (element_id, &layer) in &layers {
        elements_by_layer
            .entry(layer)
            .or_default()
            .push(element_id.clone());
    }

    // Assign positions
    for element in &mut flow.elements {
        let layer = layers.get(&element.id).copied().unwrap_or(0);
        let elements_in_layer = elements_by_layer.get(&layer).unwrap();
        let index_in_layer = elements_in_layer
            .iter()
            .position(|id| id == &element.id)
            .unwrap();

        let x = START_X + (layer as f32) * LAYER_SPACING;
        let y = START_Y + (index_in_layer as f32) * ELEMENT_SPACING;

        element.position = (x, y);
        debug!(
            "Positioned element '{}' at layer {} -> ({}, {})",
            element.id, layer, x, y
        );
    }

    debug!(
        "Auto-layout complete: {} elements in {} layers",
        flow.elements.len(),
        elements_by_layer.len()
    );
}

/// Extract element ID from a link endpoint (format: "element_id" or "element_id:pad_name")
fn extract_element_id(endpoint: &str) -> String {
    endpoint.split(':').next().unwrap_or(endpoint).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use strom_types::Element;

    fn create_element(id: &str, position: (f32, f32)) -> Element {
        Element {
            id: id.to_string(),
            element_type: "test".to_string(),
            properties: HashMap::new(),
            pad_properties: HashMap::new(),
            position,
        }
    }

    #[test]
    fn test_needs_auto_layout_all_stacked_at_origin() {
        let mut flow = Flow::new("test");
        flow.elements.push(create_element("elem1", (0.0, 0.0)));
        flow.elements.push(create_element("elem2", (0.0, 0.0)));
        flow.elements.push(create_element("elem3", (0.0, 0.0)));

        // 3 elements stacked at origin should trigger auto-layout
        assert!(needs_auto_layout(&flow));
    }

    #[test]
    fn test_needs_auto_layout_all_stacked_same_location() {
        let mut flow = Flow::new("test");
        flow.elements.push(create_element("elem1", (50.0, 50.0)));
        flow.elements.push(create_element("elem2", (50.0, 50.0)));
        flow.elements.push(create_element("elem3", (50.0, 50.0)));

        // 3 elements stacked at same location should trigger auto-layout
        assert!(needs_auto_layout(&flow));
    }

    #[test]
    fn test_needs_auto_layout_spread_out() {
        let mut flow = Flow::new("test");
        flow.elements.push(create_element("elem1", (0.0, 0.0)));
        flow.elements.push(create_element("elem2", (100.0, 100.0)));
        flow.elements.push(create_element("elem3", (200.0, 200.0)));

        // Elements already spread out should not trigger auto-layout
        assert!(!needs_auto_layout(&flow));
    }

    #[test]
    fn test_needs_auto_layout_trivial_graph() {
        // Two elements should not trigger auto-layout (trivial graph)
        let mut flow = Flow::new("test");
        flow.elements.push(create_element("elem1", (0.0, 0.0)));
        flow.elements.push(create_element("elem2", (0.0, 0.0)));

        assert!(!needs_auto_layout(&flow));
    }

    #[test]
    fn test_needs_auto_layout_one_element_one_block() {
        use std::collections::HashMap;
        use strom_types::block::{BlockInstance, Position};

        // One element + one block should not trigger auto-layout
        let mut flow = Flow::new("test");
        flow.elements.push(create_element("elem1", (100.0, 100.0)));
        flow.blocks.push(BlockInstance {
            id: "block1".to_string(),
            block_definition_id: "test".to_string(),
            name: None,
            properties: HashMap::new(),
            position: Position { x: 100.0, y: 100.0 },
            runtime_data: None,
        });

        assert!(!needs_auto_layout(&flow));
    }

    #[test]
    fn test_apply_auto_layout_simple_chain() {
        let mut flow = Flow::new("test");
        flow.elements.push(create_element("elem1", (0.0, 0.0)));
        flow.elements.push(create_element("elem2", (0.0, 0.0)));
        flow.elements.push(create_element("elem3", (0.0, 0.0)));
        flow.links.push(strom_types::Link {
            from: "elem1:src".to_string(),
            to: "elem2:sink".to_string(),
        });
        flow.links.push(strom_types::Link {
            from: "elem2:src".to_string(),
            to: "elem3:sink".to_string(),
        });

        apply_auto_layout(&mut flow);

        // Check that elements are positioned in layers
        let elem1 = &flow.elements[0];
        let elem2 = &flow.elements[1];
        let elem3 = &flow.elements[2];

        // elem1 should be leftmost (layer 0)
        assert_eq!(elem1.position.0, START_X);

        // elem2 should be in layer 1
        assert_eq!(elem2.position.0, START_X + LAYER_SPACING);

        // elem3 should be in layer 2
        assert_eq!(elem3.position.0, START_X + LAYER_SPACING * 2.0);
    }

    #[test]
    fn test_extract_element_id() {
        assert_eq!(extract_element_id("elem1"), "elem1");
        assert_eq!(extract_element_id("elem1:src"), "elem1");
        assert_eq!(extract_element_id("elem1:sink_0"), "elem1");
    }
}
