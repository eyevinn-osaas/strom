//! Statistics collector for running pipelines.

use crate::stats::rtp::collect_all_jitterbuffer_stats;
use gstreamer as gst;
use gstreamer::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};
use strom_types::block::BlockInstance;
use strom_types::stats::{BlockStats, FlowStats, Statistic};
use strom_types::Flow;
use tracing::{debug, trace, warn};

/// Collector for pipeline statistics.
pub struct StatsCollector;

impl StatsCollector {
    /// Collect statistics for a running flow.
    pub fn collect_flow_stats(pipeline: &gst::Pipeline, flow: &Flow) -> FlowStats {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let mut block_stats = Vec::new();

        // Collect stats for each block in the flow
        for block in &flow.blocks {
            if let Some(stats) = Self::collect_block_stats(pipeline, block) {
                block_stats.push(stats);
            }
        }

        FlowStats {
            flow_id: flow.id,
            flow_name: flow.name.clone(),
            block_stats,
            collected_at: now,
        }
    }

    /// Collect statistics for a specific block.
    fn collect_block_stats(pipeline: &gst::Pipeline, block: &BlockInstance) -> Option<BlockStats> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        // Determine what kind of stats to collect based on block definition
        let stats = match block.block_definition_id.as_str() {
            "builtin.aes67_input" => Self::collect_aes67_input_stats(pipeline, &block.id),
            "builtin.aes67_output" => {
                // AES67 output doesn't have jitterbuffer stats, could add other stats later
                vec![]
            }
            "builtin.meter" => {
                // Meter block stats could be added here
                vec![]
            }
            _ => {
                // Unknown block type - no stats available
                vec![]
            }
        };

        if stats.is_empty() {
            return None;
        }

        Some(BlockStats {
            block_instance_id: block.id.clone(),
            block_definition_id: block.block_definition_id.clone(),
            block_name: block
                .name
                .clone()
                .unwrap_or_else(|| block.block_definition_id.clone()),
            stats,
            collected_at: now,
        })
    }

    /// Collect statistics for AES67 Input block (RTP jitterbuffer stats).
    fn collect_aes67_input_stats(pipeline: &gst::Pipeline, instance_id: &str) -> Vec<Statistic> {
        let mut all_stats = Vec::new();

        // Find the sdpdemux element for this block
        let sdpdemux_name = format!("{}:sdpdemux", instance_id);
        if let Some(sdpdemux) = pipeline.by_name(&sdpdemux_name) {
            debug!("Found sdpdemux element: {}", sdpdemux_name);

            // Cast sdpdemux to Bin to search for jitterbuffers
            if let Ok(bin) = sdpdemux.dynamic_cast::<gst::Bin>() {
                let jb_stats = collect_all_jitterbuffer_stats(&bin);
                let jb_count = jb_stats.len();
                trace!("Found {} jitterbuffer(s) in {}", jb_count, sdpdemux_name);

                for (jb_name, stats) in jb_stats {
                    debug!("Jitterbuffer '{}' stats: {:?}", jb_name, stats);
                    // Add stats with jitterbuffer name prefix for multi-stream support
                    for mut stat in stats.to_statistics() {
                        if jb_count > 1 {
                            stat.id = format!("{}_{}", jb_name, stat.id);
                            stat.metadata.display_name =
                                format!("{} ({})", stat.metadata.display_name, jb_name);
                        }
                        all_stats.push(stat);
                    }
                }
            } else {
                warn!("Failed to cast sdpdemux to Bin: {}", sdpdemux_name);
            }
        } else {
            warn!("Could not find sdpdemux element: {}", sdpdemux_name);
        }

        all_stats
    }
}
