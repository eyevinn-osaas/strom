//! Video Thumbnail block — passthrough tee with on-demand thumbnail capture.
//!
//! Inserts a tee element into a video chain. When thumbnails are requested
//! via the REST API, a processing branch is lazily attached to produce
//! JPEG thumbnails using GStreamer-native conversion and scaling.
//!
//! Chain: [tee (allow-not-linked=true)] — zero overhead when no thumbnail is requested.

use crate::blocks::{BlockBuildContext, BlockBuildError, BlockBuildResult, BlockBuilder};
use gstreamer as gst;
use std::collections::HashMap;
use strom_types::{block::*, PropertyValue, *};
use tracing::info;

/// Video Thumbnail block builder.
pub struct ThumbnailBuilder;

impl BlockBuilder for ThumbnailBuilder {
    fn build(
        &self,
        instance_id: &str,
        _properties: &HashMap<String, PropertyValue>,
        _ctx: &BlockBuildContext,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        info!("Building Thumbnail block instance: {}", instance_id);

        let tee_id = format!("{}:tee", instance_id);
        let tee = gst::ElementFactory::make("tee")
            .name(&tee_id)
            .property("allow-not-linked", true)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("tee: {}", e)))?;

        Ok(BlockBuildResult {
            elements: vec![(tee_id, tee)],
            internal_links: vec![],
            bus_message_handler: None,
            pad_properties: HashMap::new(),
        })
    }
}

/// Get block definitions for the Thumbnail block.
pub fn get_blocks() -> Vec<BlockDefinition> {
    vec![BlockDefinition {
        id: "builtin.thumbnail".to_string(),
        name: "Video Thumbnail".to_string(),
        description: "Passthrough video element with on-demand thumbnail capture via REST API."
            .to_string(),
        category: "Video".to_string(),
        exposed_properties: vec![],
        external_pads: ExternalPads {
            inputs: vec![ExternalPad {
                label: Some("V0".to_string()),
                name: "video_in".to_string(),
                media_type: MediaType::Video,
                internal_element_id: "tee".to_string(),
                internal_pad_name: "sink".to_string(),
            }],
            outputs: vec![ExternalPad {
                label: Some("V0".to_string()),
                name: "video_out".to_string(),
                media_type: MediaType::Video,
                internal_element_id: "tee".to_string(),
                internal_pad_name: "src_%u".to_string(),
            }],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("\u{1f5bc}".to_string()),
            ..Default::default()
        }),
    }]
}
