//! Built-in block definitions organized by protocol/function.

pub mod aes67;
pub mod meter;

use crate::blocks::BlockBuilder;
use std::sync::Arc;
use strom_types::BlockDefinition;

/// Get all built-in block definitions.
pub fn get_all_builtin_blocks() -> Vec<BlockDefinition> {
    let mut blocks = Vec::new();

    // Add AES67 blocks
    blocks.extend(aes67::get_blocks());

    // Add Meter blocks
    blocks.extend(meter::get_blocks());

    // Future: Add more protocols here
    // blocks.extend(ndi::get_blocks());
    // blocks.extend(rtmp::get_blocks());
    // blocks.extend(hls::get_blocks());

    blocks
}

/// Get a BlockBuilder instance for a built-in block by its definition ID.
pub fn get_builder(block_definition_id: &str) -> Option<Arc<dyn BlockBuilder>> {
    match block_definition_id {
        "builtin.aes67_input" => Some(Arc::new(aes67::AES67InputBuilder)),
        "builtin.aes67_output" => Some(Arc::new(aes67::AES67OutputBuilder)),
        "builtin.meter" => Some(Arc::new(meter::MeterBuilder)),
        // Future: Add more builders here
        _ => None,
    }
}
