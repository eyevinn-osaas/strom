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
    /// Total number of cores visible to this process (cgroup-aware)
    total_cores: usize,
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
    /// for system/tokio tasks only on larger systems where the overhead
    /// is negligible:
    /// - 1 core: no pinning (skip)
    /// - 2-7 cores: all cores available (tokio shares, not worth reserving)
    /// - 8-32 cores: reserve 1, available: 7-31
    /// - 33+ cores: reserve 2, available: 31+
    pub fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            let allowed_cores = detect_allowed_cores();
            let num_cpus = allowed_cores.len();

            let reserved = match num_cpus {
                0..=7 => 0,
                8..=32 => 1,
                _ => 2,
            };

            let available_cores: Vec<usize> = if num_cpus <= 1 {
                // Single core - no pinning possible
                Vec::new()
            } else {
                // Use all allowed cores on small systems, reserve highest-numbered on large
                allowed_cores[..num_cpus - reserved].to_vec()
            };

            let core_flow_count: HashMap<usize, usize> =
                available_cores.iter().map(|&core| (core, 0)).collect();

            info!(
                "AffinityManager: {} cores detected (allowed: {:?}), {} reserved for system, {} available for flows: {:?}",
                num_cpus,
                allowed_cores,
                reserved,
                available_cores.len(),
                available_cores
            );

            Self {
                inner: RwLock::new(AffinityManagerInner {
                    total_cores: num_cpus,
                    available_cores,
                    flow_assignments: HashMap::new(),
                    core_flow_count,
                }),
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            let num_cpus = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
            info!("AffinityManager: CPU affinity not supported on this platform, core pinning disabled");
            Self {
                inner: RwLock::new(AffinityManagerInner {
                    total_cores: num_cpus,
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
        let total_cores = available_cores.len();
        let core_flow_count: HashMap<usize, usize> =
            available_cores.iter().map(|&core| (core, 0)).collect();

        Self {
            inner: RwLock::new(AffinityManagerInner {
                total_cores,
                available_cores,
                flow_assignments: HashMap::new(),
                core_flow_count,
            }),
        }
    }

    /// Get the total number of CPU cores visible to this process (cgroup-aware).
    pub fn num_cores(&self) -> usize {
        self.inner.read().total_cores
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

/// Detect which CPU cores this process is allowed to use.
///
/// On Linux, checks cgroup cpuset constraints (both v2 and v1) to handle
/// container environments (Docker, k8s) where the process may only have
/// access to a subset of host cores. Falls back to `available_parallelism()`
/// if cgroup info is unavailable.
#[cfg(target_os = "linux")]
fn detect_allowed_cores() -> Vec<usize> {
    // Try cgroups v2 first, then v1
    let cpuset = std::fs::read_to_string("/sys/fs/cgroup/cpuset.cpus.effective")
        .or_else(|_| std::fs::read_to_string("/sys/fs/cgroup/cpuset/cpuset.cpus"));

    if let Ok(cpuset_str) = cpuset {
        let cores = parse_cpuset(&cpuset_str);
        if !cores.is_empty() {
            info!(
                "AffinityManager: detected cgroup cpuset: {} cores {:?}",
                cores.len(),
                cores
            );
            return cores;
        }
    }

    // Fallback: assume all cores 0..N are available
    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    info!(
        "AffinityManager: no cgroup cpuset found, assuming {} cores (0..{})",
        num_cpus, num_cpus
    );
    (0..num_cpus).collect()
}

/// Parse a cpuset string like "0-3,7,9-11" into a sorted Vec of core indices.
#[cfg(target_os = "linux")]
fn parse_cpuset(s: &str) -> Vec<usize> {
    let mut cores = Vec::new();
    for part in s.trim().split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(s), Ok(e)) = (start.trim().parse::<usize>(), end.trim().parse::<usize>()) {
                cores.extend(s..=e);
            }
        } else if let Ok(n) = part.parse::<usize>() {
            cores.push(n);
        }
    }
    cores.sort_unstable();
    cores.dedup();
    cores
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
        // 2-core: no reservation, available: cores 0-1
        let manager = AffinityManager::with_cores(vec![0, 1]);
        let flow1 = Uuid::new_v4();
        let flow2 = Uuid::new_v4();

        assert_eq!(manager.allocate(flow1), Some(0));
        assert_eq!(manager.allocate(flow2), Some(1)); // Spread across both cores
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

    #[test]
    fn test_num_cores() {
        let manager = AffinityManager::with_cores(vec![0, 1, 2, 3]);
        assert_eq!(manager.num_cores(), 4);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_cpuset_range() {
        assert_eq!(parse_cpuset("0-3"), vec![0, 1, 2, 3]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_cpuset_mixed() {
        assert_eq!(parse_cpuset("0-3,7,9-11"), vec![0, 1, 2, 3, 7, 9, 10, 11]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_cpuset_single() {
        assert_eq!(parse_cpuset("5"), vec![5]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_cpuset_empty() {
        assert_eq!(parse_cpuset(""), Vec::<usize>::new());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_cpuset_whitespace() {
        assert_eq!(parse_cpuset("  0-2 , 5 \n"), vec![0, 1, 2, 5]);
    }
}
