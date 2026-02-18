use super::PipelineManager;
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use strom_types::flow::ThreadPriorityStatus;
use strom_types::{FlowId, PipelineState};
use tracing::info;

impl PipelineManager {
    /// Get the current state of the pipeline.
    /// Uses cached state to avoid querying async sinks during initialization (prevents SRT crashes).
    pub fn get_state(&self) -> PipelineState {
        // Return cached state to avoid querying the pipeline during async state changes
        // This prevents crashes with SRT sink and other async elements
        *self.cached_state.read().unwrap()
    }

    /// Get the flow ID this pipeline manages.
    pub fn flow_id(&self) -> FlowId {
        self.flow_id
    }

    /// Get runtime dynamic pads that have been auto-linked to tees.
    /// Returns a map of element_id -> {pad_name -> tee_element_name}
    /// These are pads that appeared at runtime without defined links.
    pub fn get_dynamic_pads(&self) -> HashMap<String, HashMap<String, String>> {
        self.dynamic_pad_tees
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Get the thread priority status for this pipeline.
    /// Returns None if pipeline hasn't been started yet.
    pub fn get_thread_priority_status(&self) -> Option<ThreadPriorityStatus> {
        self.thread_priority_state
            .as_ref()
            .map(|state| state.get_status())
    }

    /// Get the clock synchronization status for this pipeline.
    pub fn get_clock_sync_status(&self) -> strom_types::flow::ClockSyncStatus {
        use strom_types::flow::{ClockSyncStatus, GStreamerClockType};

        match self.properties.clock_type {
            GStreamerClockType::Ptp => {
                // For PTP clocks, use the stored PTP clock reference for accurate sync status
                // This ensures consistency with get_ptp_info()
                if let Some(ref ptp_clock) = self.ptp_clock {
                    if ptp_clock.is_synced() {
                        ClockSyncStatus::Synced
                    } else {
                        ClockSyncStatus::NotSynced
                    }
                } else {
                    // No PTP clock stored (shouldn't happen if properly configured)
                    ClockSyncStatus::NotSynced
                }
            }
            GStreamerClockType::Ntp => {
                // For NTP clocks, use the is_synced() method from gst::Clock trait
                // Note: NTP clock not fully implemented yet, so this is best-effort
                if let Some(clock) = self.pipeline.clock() {
                    if clock.is_synced() {
                        ClockSyncStatus::Synced
                    } else {
                        ClockSyncStatus::NotSynced
                    }
                } else {
                    ClockSyncStatus::NotSynced
                }
            }
            _ => {
                // For other clock types (Monotonic, Realtime, PipelineDefault),
                // sync status is not applicable - these are local clocks
                ClockSyncStatus::Unknown
            }
        }
    }

    /// Get detailed PTP clock information.
    /// Returns None if the pipeline is not using a PTP clock.
    pub fn get_ptp_info(&self) -> Option<strom_types::flow::PtpInfo> {
        use strom_types::flow::PtpInfo;

        // Use the stored PTP clock reference
        let ptp_clock = self.ptp_clock.as_ref()?;

        // Get the actual domain from the running clock (not from saved properties)
        let actual_domain = ptp_clock.domain() as u8;
        let gm_id = ptp_clock.grandmaster_clock_id();
        let master_id = ptp_clock.master_clock_id();

        // Only include IDs if they're non-zero (indicating valid data)
        let grandmaster_clock_id = if gm_id != 0 {
            Some(PtpInfo::format_clock_id(gm_id))
        } else {
            None
        };
        let master_clock_id = if master_id != 0 {
            Some(PtpInfo::format_clock_id(master_id))
        } else {
            None
        };

        // Check if configured domain differs from running domain
        let configured_domain = self.properties.ptp_domain.unwrap_or(0);
        let restart_needed = configured_domain != actual_domain;

        // Get statistics from the stored stats
        let stats = self.ptp_stats.read().ok().and_then(|guard| guard.clone());

        // Get sync status directly from the PTP clock
        let synced = ptp_clock.is_synced();

        Some(PtpInfo {
            domain: actual_domain,
            synced,
            grandmaster_clock_id,
            master_clock_id,
            restart_needed,
            stats,
        })
    }

    /// Get the underlying GStreamer pipeline (for debugging).
    pub fn pipeline(&self) -> &gst::Pipeline {
        &self.pipeline
    }

    /// Set the thread registry for tracking streaming threads.
    ///
    /// This should be called before start() to enable thread CPU monitoring.
    pub fn set_thread_registry(&mut self, registry: crate::thread_registry::ThreadRegistry) {
        self.thread_registry = Some(registry);
    }

    /// Get WHEP endpoints registered by blocks in this pipeline.
    pub fn whep_endpoints(&self) -> &[crate::blocks::WhepEndpointInfo] {
        &self.whep_endpoints
    }

    /// Get WHIP endpoints registered by blocks in this pipeline.
    pub fn whip_endpoints(&self) -> &[crate::blocks::WhipEndpointInfo] {
        &self.whip_endpoints
    }

    /// Generate a DOT graph of the pipeline for debugging.
    /// Returns the DOT graph content as a string.
    pub fn generate_dot_graph(&self) -> String {
        use gst::prelude::*;

        info!("Generating DOT graph for pipeline: {}", self.flow_name);

        // Use GStreamer's debug graph dump functionality
        // The details level determines how much information is included
        let dot = self
            .pipeline
            .debug_to_dot_data(gst::DebugGraphDetails::all());

        dot.to_string()
    }

    /// Get debug information about the pipeline.
    /// Provides timing, clock, latency, and state information for troubleshooting.
    pub fn get_debug_info(&self) -> strom_types::api::FlowDebugInfo {
        use gst::prelude::*;
        use strom_types::api::FlowDebugInfo;
        use strom_types::flow::GStreamerClockType;

        // Get pipeline clock
        let clock = self.pipeline.clock();

        // Get base_time and clock_time
        let base_time = self.pipeline.base_time();
        let clock_time: Option<gst::ClockTime> = clock.as_ref().map(|c| c.time());

        // Calculate running_time = clock_time - base_time
        let running_time: Option<gst::ClockTime> = match (clock_time, base_time) {
            (Some(ct), Some(bt)) if ct >= bt => Some(ct - bt),
            _ => None,
        };

        // Get clock type description
        let clock_type = match self.properties.clock_type {
            GStreamerClockType::Ptp => Some("PTP".to_string()),
            GStreamerClockType::Monotonic => Some("Monotonic".to_string()),
            GStreamerClockType::Realtime => Some("Realtime".to_string()),
            GStreamerClockType::Ntp => Some("NTP".to_string()),
        };

        // Get PTP grandmaster if using PTP clock
        let ptp_grandmaster = self.ptp_clock.as_ref().and_then(|ptp| {
            let gm_id = ptp.grandmaster_clock_id();
            if gm_id != 0 {
                Some(strom_types::flow::PtpInfo::format_clock_id(gm_id))
            } else {
                None
            }
        });

        // Get pipeline state
        let (_, state, _) = self.pipeline.state(gst::ClockTime::ZERO);
        let pipeline_state = Some(format!("{:?}", state));

        // Query latency
        let (latency_min_ns, latency_max_ns, is_live) = self
            .query_latency()
            .map(|(min, max, live)| (Some(min), Some(max), Some(live)))
            .unwrap_or((None, None, None));

        // Count elements
        let element_count = Some(self.elements.len() as u32);

        // Helper to format nanoseconds as duration
        fn format_duration(ns: u64) -> String {
            let secs = ns as f64 / 1_000_000_000.0;
            if secs < 1.0 {
                format!("{:.2} ms", secs * 1000.0)
            } else if secs < 60.0 {
                format!("{:.3} s", secs)
            } else if secs < 3600.0 {
                format!("{:.1} min", secs / 60.0)
            } else {
                format!("{:.2} h", secs / 3600.0)
            }
        }

        // Format latency
        let latency_formatted = match (latency_min_ns, latency_max_ns) {
            (Some(min), Some(max)) if min == max => Some(format_duration(min)),
            (Some(min), Some(max)) => Some(format!(
                "{} - {}",
                format_duration(min),
                format_duration(max)
            )),
            _ => None,
        };

        FlowDebugInfo {
            flow_id: self.flow_id,
            flow_name: self.flow_name.clone(),
            pipeline_state,
            is_live,
            base_time_ns: base_time.map(|t| t.nseconds()),
            clock_time_ns: clock_time.map(|t| t.nseconds()),
            running_time_ns: running_time.map(|t| t.nseconds()),
            running_time_formatted: running_time.map(|t| format_duration(t.nseconds())),
            clock_type,
            ptp_grandmaster,
            latency_min_ns,
            latency_max_ns,
            latency_formatted,
            element_count,
        }
    }
}
