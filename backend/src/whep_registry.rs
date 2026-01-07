//! WHEP endpoint registry.
//!
//! Maps endpoint IDs to internal localhost ports for WHEP Output blocks.
//! The axum proxy uses this to route requests to the correct whepserversink instance.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Registry mapping endpoint IDs to internal ports.
#[derive(Debug, Clone, Default)]
pub struct WhepRegistry {
    inner: Arc<RwLock<HashMap<String, u16>>>,
}

impl WhepRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an endpoint with its internal port.
    pub async fn register(&self, endpoint_id: String, port: u16) {
        let mut map = self.inner.write().await;
        map.insert(endpoint_id, port);
    }

    /// Unregister an endpoint.
    pub async fn unregister(&self, endpoint_id: &str) {
        let mut map = self.inner.write().await;
        map.remove(endpoint_id);
    }

    /// Look up the internal port for an endpoint ID.
    pub async fn get_port(&self, endpoint_id: &str) -> Option<u16> {
        let map = self.inner.read().await;
        map.get(endpoint_id).copied()
    }

    /// Check if an endpoint ID is already registered.
    pub async fn contains(&self, endpoint_id: &str) -> bool {
        let map = self.inner.read().await;
        map.contains_key(endpoint_id)
    }

    /// Get all registered endpoints (for debugging).
    pub async fn list_all(&self) -> Vec<(String, u16)> {
        let map = self.inner.read().await;
        map.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }
}
