//! CPU affinity management for GStreamer pipeline flows.
//!
//! Central coordinator that tracks which flows are assigned to which physical
//! CPU cores, using a least-loaded allocation strategy to distribute flows evenly.
//! On systems with SMT/hyperthreading, flows are pinned to all logical CPUs
//! (siblings) of a physical core, giving threads access to the full core's
//! execution resources instead of being constrained to a single hyperthread.

use parking_lot::RwLock;
use std::collections::HashMap;
use strom_types::FlowId;
use tracing::info;

/// A physical CPU core with its associated logical CPUs (hyperthreads/SMT siblings).
#[derive(Debug, Clone)]
struct PhysicalCore {
    /// Logical CPU IDs that share this physical core (sorted).
    /// On systems without SMT, this contains a single CPU.
    cpus: Vec<usize>,
}

/// Manages CPU core assignments for pipeline flows.
///
/// Allocates physical cores using a least-loaded strategy: when a new flow starts,
/// it gets assigned to the physical core with the fewest active flows. The returned
/// CPU set includes all sibling hyperthreads of that core, so threads can utilize
/// the full physical core. Cores are reserved for system/tokio tasks based on
/// physical core count.
pub struct AffinityManager {
    inner: RwLock<AffinityManagerInner>,
}

struct AffinityManagerInner {
    /// Total number of logical CPUs visible to this process (cgroup-aware)
    total_cpus: usize,
    /// Physical cores available for flow pinning (excludes reserved system cores)
    available_cores: Vec<PhysicalCore>,
    /// Flow -> assigned CPU set mapping (the sibling CPUs of the allocated physical core)
    flow_assignments: HashMap<FlowId, Vec<usize>>,
    /// Physical core key (first CPU) -> number of assigned flows
    core_flow_count: HashMap<usize, usize>,
}

impl AffinityManager {
    /// Create a new affinity manager.
    ///
    /// Detects physical cores (grouping SMT siblings) and reserves the
    /// highest-numbered physical cores for system/tokio tasks only on
    /// larger systems:
    /// - 1 physical core: no pinning (skip)
    /// - 2-7 physical cores: all available (tokio shares, not worth reserving)
    /// - 8-32 physical cores: reserve 1
    /// - 33+ physical cores: reserve 2
    pub fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            let allowed_cpus = detect_allowed_cpus();
            let num_cpus = allowed_cpus.len();
            let physical_cores = detect_physical_cores(&allowed_cpus);
            let num_physical = physical_cores.len();

            let reserved = match num_physical {
                0..=7 => 0,
                8..=32 => 1,
                _ => 2,
            };

            let available_cores: Vec<PhysicalCore> = if num_physical <= 1 {
                Vec::new()
            } else {
                physical_cores[..num_physical - reserved].to_vec()
            };

            let core_flow_count: HashMap<usize, usize> = available_cores
                .iter()
                .map(|core| (core.cpus[0], 0))
                .collect();

            info!(
                "AffinityManager: {} logical CPUs, {} physical cores detected, {} reserved for system, {} available for flows: {:?}",
                num_cpus,
                num_physical,
                reserved,
                available_cores.len(),
                available_cores.iter().map(|c| &c.cpus).collect::<Vec<_>>()
            );

            Self {
                inner: RwLock::new(AffinityManagerInner {
                    total_cpus: num_cpus,
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
                    total_cpus: num_cpus,
                    available_cores: Vec::new(),
                    flow_assignments: HashMap::new(),
                    core_flow_count: HashMap::new(),
                }),
            }
        }
    }

    /// Create an affinity manager with specific physical cores (for testing).
    #[cfg(test)]
    fn with_physical_cores(cores: Vec<Vec<usize>>) -> Self {
        let total_cpus: usize = cores.iter().map(|c| c.len()).sum();
        let available_cores: Vec<PhysicalCore> = cores
            .into_iter()
            .map(|cpus| PhysicalCore { cpus })
            .collect();
        let core_flow_count: HashMap<usize, usize> = available_cores
            .iter()
            .map(|core| (core.cpus[0], 0))
            .collect();

        Self {
            inner: RwLock::new(AffinityManagerInner {
                total_cpus,
                available_cores,
                flow_assignments: HashMap::new(),
                core_flow_count,
            }),
        }
    }

    /// Get the total number of logical CPUs visible to this process (cgroup-aware).
    pub fn num_cores(&self) -> usize {
        self.inner.read().total_cpus
    }

    /// Allocate a physical core for a flow using least-loaded strategy.
    ///
    /// Returns `Some(cpus)` with all logical CPUs (hyperthreads) of the assigned
    /// physical core, or `None` if no cores are available (single-core system).
    pub fn allocate(&self, flow_id: FlowId) -> Option<Vec<usize>> {
        let mut inner = self.inner.write();

        if inner.available_cores.is_empty() {
            return None;
        }

        // Already allocated?
        if let Some(cpus) = inner.flow_assignments.get(&flow_id) {
            return Some(cpus.clone());
        }

        // Find the physical core with the fewest assigned flows (lowest CPU breaks ties)
        let core = inner
            .available_cores
            .iter()
            .min_by_key(|core| {
                let key = core.cpus[0];
                let count = inner.core_flow_count.get(&key).copied().unwrap_or(0);
                (count, key)
            })
            .unwrap() // Safe: available_cores is non-empty
            .clone();

        let key = core.cpus[0];
        let cpus = core.cpus.clone();
        inner.flow_assignments.insert(flow_id, cpus.clone());
        *inner.core_flow_count.entry(key).or_insert(0) += 1;

        let count = inner.core_flow_count[&key];
        info!(
            "AffinityManager: allocated flow {} to physical core (CPUs {:?}), {} flow(s) on this core",
            flow_id, cpus, count
        );

        Some(cpus)
    }

    /// Deallocate a flow's core assignment.
    pub fn deallocate(&self, flow_id: &FlowId) {
        let mut inner = self.inner.write();

        if let Some(cpus) = inner.flow_assignments.remove(flow_id) {
            let key = cpus[0];
            if let Some(count) = inner.core_flow_count.get_mut(&key) {
                *count = count.saturating_sub(1);
            }
            info!(
                "AffinityManager: deallocated flow {} from physical core (CPUs {:?}), {} flow(s) remaining",
                flow_id,
                cpus,
                inner.core_flow_count.get(&key).copied().unwrap_or(0)
            );
        }
    }

    /// Get the CPU assignment for a flow.
    pub fn get_assignment(&self, flow_id: &FlowId) -> Option<Vec<usize>> {
        let inner = self.inner.read();
        inner.flow_assignments.get(flow_id).cloned()
    }
}

impl Default for AffinityManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect which logical CPUs this process is allowed to use.
///
/// On Linux, checks cgroup cpuset constraints (both v2 and v1) to handle
/// container environments (Docker, k8s) where the process may only have
/// access to a subset of host cores. Falls back to `available_parallelism()`
/// if cgroup info is unavailable.
#[cfg(target_os = "linux")]
fn detect_allowed_cpus() -> Vec<usize> {
    // Try cgroups v2 first, then v1
    let cpuset = std::fs::read_to_string("/sys/fs/cgroup/cpuset.cpus.effective")
        .or_else(|_| std::fs::read_to_string("/sys/fs/cgroup/cpuset/cpuset.cpus"));

    if let Ok(cpuset_str) = cpuset {
        let cpus = parse_cpuset(&cpuset_str);
        if !cpus.is_empty() {
            info!(
                "AffinityManager: detected cgroup cpuset: {} CPUs {:?}",
                cpus.len(),
                cpus
            );
            return cpus;
        }
    }

    // Fallback: assume all CPUs 0..N are available
    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    info!(
        "AffinityManager: no cgroup cpuset found, assuming {} CPUs (0..{})",
        num_cpus, num_cpus
    );
    (0..num_cpus).collect()
}

/// Detect physical cores by reading SMT sibling topology.
///
/// Groups logical CPUs into physical cores using
/// `/sys/devices/system/cpu/cpu*/topology/thread_siblings_list`.
/// Only includes CPUs from `allowed_cpus`. Falls back to treating each
/// logical CPU as its own physical core if topology info is unavailable.
#[cfg(target_os = "linux")]
fn detect_physical_cores(allowed_cpus: &[usize]) -> Vec<PhysicalCore> {
    use std::collections::BTreeMap;

    // Group CPUs by their physical core (keyed by lowest sibling CPU)
    let mut cores: BTreeMap<usize, Vec<usize>> = BTreeMap::new();

    for &cpu in allowed_cpus {
        let path = format!(
            "/sys/devices/system/cpu/cpu{}/topology/thread_siblings_list",
            cpu
        );
        if let Ok(siblings_str) = std::fs::read_to_string(&path) {
            let siblings = parse_cpuset(&siblings_str);
            // Use the lowest sibling as the physical core key
            if let Some(&first) = siblings.first() {
                let entry = cores.entry(first).or_default();
                if !entry.contains(&cpu) {
                    entry.push(cpu);
                }
            }
        } else {
            // No topology info for this CPU, treat it as its own core
            cores.entry(cpu).or_default().push(cpu);
        }
    }

    if cores.is_empty() {
        // Fallback: each CPU is its own physical core
        return allowed_cpus
            .iter()
            .map(|&cpu| PhysicalCore { cpus: vec![cpu] })
            .collect();
    }

    cores
        .into_values()
        .map(|mut cpus| {
            cpus.sort_unstable();
            cpus.dedup();
            PhysicalCore { cpus }
        })
        .collect()
}

/// Parse a cpuset string like "0-3,7,9-11" into a sorted Vec of CPU indices.
#[cfg(target_os = "linux")]
fn parse_cpuset(s: &str) -> Vec<usize> {
    let mut cpus = Vec::new();
    for part in s.trim().split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(s), Ok(e)) = (start.trim().parse::<usize>(), end.trim().parse::<usize>()) {
                cpus.extend(s..=e);
            }
        } else if let Ok(n) = part.parse::<usize>() {
            cpus.push(n);
        }
    }
    cpus.sort_unstable();
    cpus.dedup();
    cpus
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_single_core_returns_none() {
        let manager = AffinityManager::with_physical_cores(vec![]);
        let flow_id = Uuid::new_v4();
        assert_eq!(manager.allocate(flow_id), None);
    }

    #[test]
    fn test_two_physical_cores_with_smt() {
        // 2 physical cores, each with 2 hyperthreads
        let manager = AffinityManager::with_physical_cores(vec![vec![0, 1], vec![2, 3]]);
        let flow1 = Uuid::new_v4();
        let flow2 = Uuid::new_v4();

        let cpus1 = manager.allocate(flow1).unwrap();
        let cpus2 = manager.allocate(flow2).unwrap();

        // Each flow gets a full physical core (both hyperthreads)
        assert_eq!(cpus1, vec![0, 1]);
        assert_eq!(cpus2, vec![2, 3]);
    }

    #[test]
    fn test_no_smt_system() {
        // 4 physical cores, no hyperthreading (1 CPU each)
        let manager =
            AffinityManager::with_physical_cores(vec![vec![0], vec![1], vec![2], vec![3]]);
        let flow1 = Uuid::new_v4();
        let flow2 = Uuid::new_v4();

        let cpus1 = manager.allocate(flow1).unwrap();
        let cpus2 = manager.allocate(flow2).unwrap();

        assert_eq!(cpus1, vec![0]);
        assert_eq!(cpus2, vec![1]);
    }

    #[test]
    fn test_flows_spread_across_physical_cores() {
        // 4 physical cores with SMT
        let manager = AffinityManager::with_physical_cores(vec![
            vec![0, 1],
            vec![2, 3],
            vec![4, 5],
            vec![6, 7],
        ]);
        let flow1 = Uuid::new_v4();
        let flow2 = Uuid::new_v4();
        let flow3 = Uuid::new_v4();

        let cpus1 = manager.allocate(flow1).unwrap();
        let cpus2 = manager.allocate(flow2).unwrap();
        let cpus3 = manager.allocate(flow3).unwrap();

        // All three should be on different physical cores
        assert_ne!(cpus1, cpus2);
        assert_ne!(cpus2, cpus3);
        assert_ne!(cpus1, cpus3);
    }

    #[test]
    fn test_deallocate_frees_physical_core() {
        let manager = AffinityManager::with_physical_cores(vec![vec![0, 1], vec![2, 3]]);
        let flow1 = Uuid::new_v4();
        let flow2 = Uuid::new_v4();
        let flow3 = Uuid::new_v4();

        let cpus1 = manager.allocate(flow1).unwrap();
        let _cpus2 = manager.allocate(flow2).unwrap();

        // Deallocate flow1
        manager.deallocate(&flow1);

        // flow3 should go to the freed physical core
        let cpus3 = manager.allocate(flow3).unwrap();
        assert_eq!(cpus3, cpus1);
    }

    #[test]
    fn test_more_flows_than_physical_cores() {
        // 3 physical cores with SMT
        let manager =
            AffinityManager::with_physical_cores(vec![vec![0, 1], vec![2, 3], vec![4, 5]]);
        let mut assignments = Vec::new();

        // Allocate 6 flows across 3 physical cores
        for _ in 0..6 {
            let flow = Uuid::new_v4();
            assignments.push(manager.allocate(flow).unwrap());
        }

        // Count flows per physical core (keyed by first CPU)
        let mut counts = HashMap::new();
        for cpus in &assignments {
            *counts.entry(cpus[0]).or_insert(0usize) += 1;
        }

        // Each physical core should have exactly 2 flows
        for key in [0, 2, 4] {
            assert_eq!(counts.get(&key).copied().unwrap_or(0), 2);
        }
    }

    #[test]
    fn test_idempotent_allocate() {
        let manager = AffinityManager::with_physical_cores(vec![vec![0, 1], vec![2, 3]]);
        let flow = Uuid::new_v4();

        let cpus1 = manager.allocate(flow).unwrap();
        let cpus2 = manager.allocate(flow).unwrap();
        assert_eq!(cpus1, cpus2); // Same flow, same physical core
    }

    #[test]
    fn test_get_assignment() {
        let manager = AffinityManager::with_physical_cores(vec![vec![0, 1], vec![2, 3]]);
        let flow = Uuid::new_v4();

        assert_eq!(manager.get_assignment(&flow), None);
        let cpus = manager.allocate(flow).unwrap();
        assert_eq!(manager.get_assignment(&flow), Some(cpus));
        manager.deallocate(&flow);
        assert_eq!(manager.get_assignment(&flow), None);
    }

    #[test]
    fn test_num_cores_returns_logical_cpus() {
        // 2 physical cores with 2 hyperthreads each = 4 logical CPUs
        let manager = AffinityManager::with_physical_cores(vec![vec![0, 1], vec![2, 3]]);
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
