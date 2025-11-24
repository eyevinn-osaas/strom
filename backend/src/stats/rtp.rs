//! RTP statistics collection from GStreamer jitterbuffer elements.

use gstreamer as gst;
use gstreamer::prelude::*;
use strom_types::RtpJitterbufferStats;
use tracing::{debug, warn};

/// Collect RTP jitterbuffer statistics from an rtpjitterbuffer element.
///
/// The rtpjitterbuffer element exposes a "stats" property containing:
/// - num-pushed, num-lost, num-late, num-duplicates
/// - avg-jitter
/// - rtx-count, rtx-success-count, rtx-per-packet, rtx-rtt
pub fn collect_rtp_jitterbuffer_stats(element: &gst::Element) -> Option<RtpJitterbufferStats> {
    let factory = element.factory()?;
    let factory_name = factory.name();

    if factory_name != "rtpjitterbuffer" {
        warn!("Expected rtpjitterbuffer element, got {}", factory_name);
        return None;
    }

    // Get the stats property - it's a GstStructure
    let stats: gst::Structure = element.property("stats");

    debug!("RTP jitterbuffer stats structure: {:?}", stats);

    // Extract values from the structure
    let num_pushed = stats.get::<u64>("num-pushed").unwrap_or(0);
    let num_lost = stats.get::<u64>("num-lost").unwrap_or(0);
    let num_late = stats.get::<u64>("num-late").unwrap_or(0);
    let num_duplicates = stats.get::<u64>("num-duplicates").unwrap_or(0);
    let avg_jitter = stats.get::<u64>("avg-jitter").unwrap_or(0);
    let rtx_count = stats.get::<u64>("rtx-count").unwrap_or(0);
    let rtx_success_count = stats.get::<u64>("rtx-success-count").unwrap_or(0);
    let rtx_per_packet = stats.get::<f64>("rtx-per-packet").unwrap_or(0.0);
    let rtx_rtt = stats.get::<u64>("rtx-rtt").unwrap_or(0);

    Some(RtpJitterbufferStats {
        num_pushed,
        num_lost,
        num_late,
        num_duplicates,
        avg_jitter_ns: avg_jitter,
        rtx_count,
        rtx_success_count,
        rtx_per_packet,
        rtx_rtt_ns: rtx_rtt,
    })
}

/// Find rtpjitterbuffer elements within an sdpdemux element.
///
/// sdpdemux creates rtpbin which in turn creates rtpjitterbuffer elements.
/// We need to traverse the bin hierarchy to find them.
pub fn find_jitterbuffers_in_bin(bin: &gst::Bin) -> Vec<gst::Element> {
    let mut jitterbuffers = Vec::new();

    // Iterate through all elements in the bin
    for element in bin.iterate_elements().into_iter().flatten() {
        if let Some(factory) = element.factory() {
            let factory_name = factory.name();
            if factory_name == "rtpjitterbuffer" {
                jitterbuffers.push(element.clone());
            }
        }

        // If this element is also a bin, recurse into it
        if let Ok(sub_bin) = element.clone().dynamic_cast::<gst::Bin>() {
            jitterbuffers.extend(find_jitterbuffers_in_bin(&sub_bin));
        }
    }

    jitterbuffers
}

/// Collect all RTP jitterbuffer stats from a bin (like sdpdemux).
pub fn collect_all_jitterbuffer_stats(bin: &gst::Bin) -> Vec<(String, RtpJitterbufferStats)> {
    let jitterbuffers = find_jitterbuffers_in_bin(bin);

    jitterbuffers
        .into_iter()
        .filter_map(|jb| {
            let name = jb.name().to_string();
            collect_rtp_jitterbuffer_stats(&jb).map(|stats| (name, stats))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_jitterbuffers_empty_bin() {
        gst::init().unwrap();
        let bin = gst::Bin::new();
        let jitterbuffers = find_jitterbuffers_in_bin(&bin);
        assert!(jitterbuffers.is_empty());
    }
}
