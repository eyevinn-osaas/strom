//! PTP clock monitoring with history and graphs.

use std::collections::{HashMap, VecDeque};
use strom_types::FlowId;

const HISTORY_SIZE: usize = 60; // Keep 60 seconds of history

/// PTP stats data received from WebSocket event.
#[derive(Clone, Debug)]
pub struct PtpStatsData {
    pub domain: u8,
    pub synced: bool,
    pub mean_path_delay_ns: Option<u64>,
    pub clock_offset_ns: Option<i64>,
    pub r_squared: Option<f64>,
    pub clock_rate: Option<f64>,
    pub grandmaster_id: Option<u64>,
    pub master_id: Option<u64>,
}

/// Data store for PTP statistics with history per flow.
#[derive(Clone, Default)]
pub struct PtpStatsStore {
    /// History per flow ID
    flows: HashMap<FlowId, PtpFlowHistory>,
}

/// PTP stats history for a single flow.
#[derive(Clone)]
pub struct PtpFlowHistory {
    /// Clock offset history in microseconds
    clock_offset_history: VecDeque<f64>,
    /// R-squared history (0.0-1.0)
    r_squared_history: VecDeque<f64>,
    /// Path delay history in microseconds
    path_delay_history: VecDeque<f64>,
    /// Latest stats
    latest: Option<PtpStatsData>,
}

impl PtpFlowHistory {
    /// Get the latest PTP stats.
    pub fn latest(&self) -> Option<&PtpStatsData> {
        self.latest.as_ref()
    }

    /// Get the clock offset history (in microseconds).
    pub fn clock_offset_history(&self) -> &VecDeque<f64> {
        &self.clock_offset_history
    }

    /// Get the R² history.
    pub fn r_squared_history(&self) -> &VecDeque<f64> {
        &self.r_squared_history
    }

    /// Get the path delay history (in microseconds).
    pub fn path_delay_history(&self) -> &VecDeque<f64> {
        &self.path_delay_history
    }
}

impl Default for PtpFlowHistory {
    fn default() -> Self {
        Self {
            clock_offset_history: VecDeque::with_capacity(HISTORY_SIZE),
            r_squared_history: VecDeque::with_capacity(HISTORY_SIZE),
            path_delay_history: VecDeque::with_capacity(HISTORY_SIZE),
            latest: None,
        }
    }
}

impl PtpStatsStore {
    /// Create a new PTP stats store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update stats for a flow.
    pub fn update(&mut self, flow_id: FlowId, stats: PtpStatsData) {
        let history = self.flows.entry(flow_id).or_default();

        // Update clock offset history (convert ns to µs)
        if let Some(offset_ns) = stats.clock_offset_ns {
            let offset_us = offset_ns as f64 / 1000.0;
            history.clock_offset_history.push_back(offset_us);
            if history.clock_offset_history.len() > HISTORY_SIZE {
                history.clock_offset_history.pop_front();
            }
        }

        // Update R-squared history
        if let Some(r_squared) = stats.r_squared {
            history.r_squared_history.push_back(r_squared);
            if history.r_squared_history.len() > HISTORY_SIZE {
                history.r_squared_history.pop_front();
            }
        }

        // Update path delay history (convert ns to µs)
        if let Some(delay_ns) = stats.mean_path_delay_ns {
            let delay_us = delay_ns as f64 / 1000.0;
            history.path_delay_history.push_back(delay_us);
            if history.path_delay_history.len() > HISTORY_SIZE {
                history.path_delay_history.pop_front();
            }
        }

        history.latest = Some(stats);
    }

    /// Get the history for a flow.
    pub fn get_history(&self, flow_id: &FlowId) -> Option<&PtpFlowHistory> {
        self.flows.get(flow_id).filter(|h| h.latest.is_some())
    }
}
