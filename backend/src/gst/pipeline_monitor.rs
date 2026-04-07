//! Periodic monitor for GStreamer pipeline health.
//!
//! Warns when queue/queue2 elements accumulate too much data.
//!
//! Buffer age measurement is handled exclusively by manual pad probes
//! (`buffer_age_probe.rs`) which measure real buffer PTS on the streaming
//! thread. Polling segment positions from a timer is unreliable (stale
//! data, clock domain mismatches) and produces false warnings.

use gstreamer::prelude::*;
use std::time::Duration;
use tracing::warn;

use crate::state::AppState;

/// Nanoseconds per millisecond.
const NS_PER_MS: u64 = 1_000_000;

/// Default threshold for queue `current-level-time` (2 s).
const QUEUE_THRESHOLD_NS: u64 = 2000 * NS_PER_MS;

/// How often the monitor polls.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Queue-family factory names (multiqueue excluded — it lacks current-level-time).
const QUEUE_FACTORIES: &[&str] = &["queue", "queue2"];

/// Start the background monitor task.
pub fn start(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(POLL_INTERVAL);
        loop {
            interval.tick().await;
            check_all(&state).await;
        }
    });
}

async fn check_all(state: &AppState) {
    let pipelines = state.pipelines_read().await;

    for (flow_id, _manager) in pipelines.iter() {
        let pipeline = _manager.pipeline();

        for element in pipeline.iterate_recurse().into_iter().flatten() {
            let factory_name = match element.factory() {
                Some(f) => f.name().to_string(),
                None => continue,
            };

            if !QUEUE_FACTORIES.contains(&factory_name.as_str()) {
                continue;
            }

            let current_ns: u64 = element.property("current-level-time");
            if current_ns > QUEUE_THRESHOLD_NS {
                let current_buffers: u32 = element.property("current-level-buffers");
                let current_bytes: u32 = element.property("current-level-bytes");
                warn!(
                    flow = %flow_id,
                    queue = %element.name(),
                    kind = %factory_name,
                    current_ms = current_ns / NS_PER_MS,
                    threshold_ms = QUEUE_THRESHOLD_NS / NS_PER_MS,
                    buffers = current_buffers,
                    bytes = current_bytes,
                    "Queue level exceeds threshold"
                );
            }
        }
    }
}
