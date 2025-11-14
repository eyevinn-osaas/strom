//! Built-in block definitions organized by protocol/function.

pub mod aes67;

use strom_types::BlockDefinition;

/// Get all built-in block definitions.
pub fn get_all_builtin_blocks() -> Vec<BlockDefinition> {
    let mut blocks = Vec::new();

    // Add AES67 blocks
    blocks.extend(aes67::get_blocks());

    // Future: Add more protocols here
    // blocks.extend(ndi::get_blocks());
    // blocks.extend(rtmp::get_blocks());
    // blocks.extend(hls::get_blocks());

    blocks
}
