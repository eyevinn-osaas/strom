//! CPU affinity management for GStreamer pipeline flows.
//!
//! Central coordinator that tracks which flows are assigned to which CPU cores,
//! using a least-loaded allocation strategy to distribute flows evenly.

use parking_lot::RwLock;
use std::collections::HashMap;
use strom_types::FlowId;
use tracing::info;

/// Manages CPU core assignments for pipeline flows.
///
/// Allocates cores using a least-loaded strategy: when a new flow starts,
/// it gets assigned to the core with the fewest active flows. Cores are
/// reserved for system/tokio tasks based on total core count.
pub struct AffinityManager {
    inner: RwLock<AffinityManagerInner>,
}

struct AffinityManagerInner {
    /// Cores available for flow pinning (excludes reserved system cores)
    available_cores: Vec<usize>,
    /// Flow -> assigned core mapping
    flow_assignments: HashMap<FlowId, usize>,
    /// Core -> number of assigned flows
    core_flow_count: HashMap<usize, usize>,
}

impl AffinityManager {
    /// Create a new affinity manager.
    ///
    /// Detects available cores and reserves the highest-numbered cores
    /// for system/tokio tasks:
    /// - 1 core: no pinning (skip)
    /// - 2 cores: reserve 1, available: 1
    /// - 3-8 cores: reserve 1, available: 2-7
    /// - 9-32 cores: reserve 2, available: 7-30
    /// - 33+ cores: reserve 3, available: 30+
    pub fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            let num_cpus = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);

            let reserved = match num_cpus {
                0..=1 => 0,
                2..=8 => 1,
                9..=32 => 2,
                _ => 3,
            };

            let available_cores: Vec<usize> = if num_cpus <= 1 {
                // Single core - no pinning possible
                Vec::new()
            } else {
                // Reserve highest-numbered cores, use the rest
                (0..num_cpus - reserved).collect()
            };

            let core_flow_count: HashMap<usize, usize> =
                available_cores.iter().map(|&core| (core, 0)).collect();

            info!(
                "AffinityManager: {} cores detected, {} reserved for system, {} available for flows: {:?}",
                num_cpus,
                reserved,
                available_cores.len(),
                available_cores
            );

            Self {
                inner: RwLock::new(AffinityManagerInner {
                    available_cores,
                    flow_assignments: HashMap::new(),
                    core_flow_count,
                }),
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            info!("AffinityManager: CPU affinity not supported on this platform, core pinning disabled");
            Self {
                inner: RwLock::new(AffinityManagerInner {
                    available_cores: Vec::new(),
                    flow_assignments: HashMap::new(),
                    core_flow_count: HashMap::new(),
                }),
            }
        }
    }

    /// Create an affinity manager with a specific set of available cores (for testing).
    #[cfg(test)]
    fn with_cores(available_cores: Vec<usize>) -> Self {
        let core_flow_count: HashMap<usize, usize> =
            available_cores.iter().map(|&core| (core, 0)).collect();

        Self {
            inner: RwLock::new(AffinityManagerInner {
                available_cores,
                flow_assignments: HashMap::new(),
                core_flow_count,
            }),
        }
    }

    /// Allocate a core for a flow using least-loaded strategy.
    ///
    /// Returns `Some(core)` if a core was assigned, `None` if no cores are
    /// available (single-core system or no available cores).
    pub fn allocate(&self, flow_id: FlowId) -> Option<usize> {
        let mut inner = self.inner.write();

        if inner.available_cores.is_empty() {
            return None;
        }

        // Already allocated?
        if let Some(&core) = inner.flow_assignments.get(&flow_id) {
            return Some(core);
        }

        // Find the core with the fewest assigned flows (lowest number breaks ties)
        let &core = inner
            .available_cores
            .iter()
            .min_by_key(|&&c| {
                let count = inner.core_flow_count.get(&c).copied().unwrap_or(0);
                (count, c)
            })
            .unwrap(); // Safe: available_cores is non-empty

        inner.flow_assignments.insert(flow_id, core);
        *inner.core_flow_count.entry(core).or_insert(0) += 1;

        let count = inner.core_flow_count[&core];
        info!(
            "AffinityManager: allocated flow {} to core {} ({} flow(s) on this core)",
            flow_id, core, count
        );

        Some(core)
    }

    /// Deallocate a flow's core assignment.
    pub fn deallocate(&self, flow_id: &FlowId) {
        let mut inner = self.inner.write();

        if let Some(core) = inner.flow_assignments.remove(flow_id) {
            if let Some(count) = inner.core_flow_count.get_mut(&core) {
                *count = count.saturating_sub(1);
            }
            info!(
                "AffinityManager: deallocated flow {} from core {} ({} flow(s) remaining on this core)",
                flow_id,
                core,
                inner.core_flow_count.get(&core).copied().unwrap_or(0)
            );
        }
    }

    /// Get the core assignment for a flow.
    pub fn get_assignment(&self, flow_id: &FlowId) -> Option<usize> {
        let inner = self.inner.read();
        inner.flow_assignments.get(flow_id).copied()
    }
}

impl Default for AffinityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_single_core_returns_none() {
        let manager = AffinityManager::with_cores(vec![]);
        let flow_id = Uuid::new_v4();
        assert_eq!(manager.allocate(flow_id), None);
    }

    #[test]
    fn test_two_core_system() {
        // 2-core: reserve 1, available: core 0
        let manager = AffinityManager::with_cores(vec![0]);
        let flow1 = Uuid::new_v4();
        let flow2 = Uuid::new_v4();

        assert_eq!(manager.allocate(flow1), Some(0));
        assert_eq!(manager.allocate(flow2), Some(0)); // Both share core 0
    }

    #[test]
    fn test_eight_core_three_flows_spread() {
        // 8-core: reserve 1, available: cores 0-6
        let manager = AffinityManager::with_cores((0..7).collect());
        let flow1 = Uuid::new_v4();
        let flow2 = Uuid::new_v4();
        let flow3 = Uuid::new_v4();

        let c1 = manager.allocate(flow1).unwrap();
        let c2 = manager.allocate(flow2).unwrap();
        let c3 = manager.allocate(flow3).unwrap();

        // All three should be on different cores
        assert_ne!(c1, c2);
        assert_ne!(c2, c3);
        assert_ne!(c1, c3);
    }

    #[test]
    fn test_deallocate_decreases_count() {
        let manager = AffinityManager::with_cores(vec![0, 1]);
        let flow1 = Uuid::new_v4();
        let flow2 = Uuid::new_v4();
        let flow3 = Uuid::new_v4();

        let c1 = manager.allocate(flow1).unwrap();
        let _c2 = manager.allocate(flow2).unwrap();

        // Deallocate flow1
        manager.deallocate(&flow1);

        // flow3 should go to the core that was freed (c1)
        let c3 = manager.allocate(flow3).unwrap();
        assert_eq!(c3, c1);
    }

    #[test]
    fn test_more_flows_than_cores_share_evenly() {
        let manager = AffinityManager::with_cores(vec![0, 1, 2]);
        let mut assignments = Vec::new();

        // Allocate 6 flows across 3 cores
        for _ in 0..6 {
            let flow = Uuid::new_v4();
            assignments.push(manager.allocate(flow).unwrap());
        }

        // Count flows per core
        let mut counts = HashMap::new();
        for core in &assignments {
            *counts.entry(*core).or_insert(0usize) += 1;
        }

        // Each core should have exactly 2 flows
        for core in 0..3 {
            assert_eq!(counts.get(&core).copied().unwrap_or(0), 2);
        }
    }

    #[test]
    fn test_idempotent_allocate() {
        let manager = AffinityManager::with_cores(vec![0, 1]);
        let flow = Uuid::new_v4();

        let c1 = manager.allocate(flow).unwrap();
        let c2 = manager.allocate(flow).unwrap();
        assert_eq!(c1, c2); // Same flow, same core
    }

    #[test]
    fn test_get_assignment() {
        let manager = AffinityManager::with_cores(vec![0, 1]);
        let flow = Uuid::new_v4();

        assert_eq!(manager.get_assignment(&flow), None);
        let core = manager.allocate(flow).unwrap();
        assert_eq!(manager.get_assignment(&flow), Some(core));
        manager.deallocate(&flow);
        assert_eq!(manager.get_assignment(&flow), None);
    }
}
