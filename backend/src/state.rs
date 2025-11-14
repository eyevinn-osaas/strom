//! Application state management.

use crate::events::EventBroadcaster;
use crate::gst::{ElementDiscovery, PipelineError, PipelineManager};
use crate::storage::{JsonFileStorage, Storage};
use std::collections::HashMap;
use std::sync::Arc;
use strom_types::element::ElementInfo;
use strom_types::{Flow, FlowId, PipelineState, StromEvent};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    /// All flows, indexed by ID
    flows: RwLock<HashMap<FlowId, Flow>>,
    /// Storage backend
    storage: Arc<dyn Storage>,
    /// GStreamer element discovery
    element_discovery: RwLock<ElementDiscovery>,
    /// Active pipelines
    pipelines: RwLock<HashMap<FlowId, PipelineManager>>,
    /// Event broadcaster for SSE
    events: EventBroadcaster,
}

impl AppState {
    /// Create new application state with the given storage backend.
    pub fn new(storage: impl Storage + 'static) -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                flows: RwLock::new(HashMap::new()),
                storage: Arc::new(storage),
                element_discovery: RwLock::new(ElementDiscovery::new()),
                pipelines: RwLock::new(HashMap::new()),
                events: EventBroadcaster::default(),
            }),
        }
    }

    /// Get the event broadcaster.
    pub fn events(&self) -> &EventBroadcaster {
        &self.inner.events
    }

    /// Create new application state with JSON file storage.
    pub fn with_json_storage(path: impl AsRef<std::path::Path>) -> Self {
        Self::new(JsonFileStorage::new(path))
    }

    /// Load flows from storage into memory.
    pub async fn load_from_storage(&self) -> anyhow::Result<()> {
        info!("Loading flows from storage...");
        match self.inner.storage.load_all().await {
            Ok(flows) => {
                let count = flows.len();
                let mut state_flows = self.inner.flows.write().await;
                *state_flows = flows;
                info!("Loaded {} flows from storage", count);
                Ok(())
            }
            Err(e) => {
                error!("Failed to load flows from storage: {}", e);
                Err(e.into())
            }
        }
    }

    /// Get all flows.
    pub async fn get_flows(&self) -> Vec<Flow> {
        let flows = self.inner.flows.read().await;
        flows.values().cloned().collect()
    }

    /// Get a specific flow by ID.
    pub async fn get_flow(&self, id: &FlowId) -> Option<Flow> {
        let flows = self.inner.flows.read().await;
        flows.get(id).cloned()
    }

    /// Add or update a flow and persist to storage.
    pub async fn upsert_flow(&self, flow: Flow) -> anyhow::Result<()> {
        let is_new = {
            let flows = self.inner.flows.read().await;
            !flows.contains_key(&flow.id)
        };

        // Update in-memory state
        {
            let mut flows = self.inner.flows.write().await;
            flows.insert(flow.id, flow.clone());
        }

        // Persist to storage
        if let Err(e) = self.inner.storage.save_flow(&flow).await {
            error!("Failed to save flow to storage: {}", e);
            return Err(e.into());
        }

        // Broadcast event
        if is_new {
            self.inner
                .events
                .broadcast(StromEvent::FlowCreated { flow_id: flow.id });
        } else {
            self.inner
                .events
                .broadcast(StromEvent::FlowUpdated { flow_id: flow.id });
        }

        Ok(())
    }

    /// Delete a flow and persist to storage.
    pub async fn delete_flow(&self, id: &FlowId) -> anyhow::Result<bool> {
        // Check if flow exists
        let exists = {
            let flows = self.inner.flows.read().await;
            flows.contains_key(id)
        };

        if !exists {
            return Ok(false);
        }

        // Delete from storage first
        if let Err(e) = self.inner.storage.delete_flow(id).await {
            error!("Failed to delete flow from storage: {}", e);
            return Err(e.into());
        }

        // Delete from in-memory state
        {
            let mut flows = self.inner.flows.write().await;
            flows.remove(id);
        }

        // Broadcast event
        self.inner
            .events
            .broadcast(StromEvent::FlowDeleted { flow_id: *id });

        Ok(true)
    }

    /// Discover all available GStreamer elements.
    pub async fn discover_elements(&self) -> Vec<ElementInfo> {
        let mut discovery = self.inner.element_discovery.write().await;
        discovery.discover_all()
    }

    /// Get information about a specific element.
    pub async fn get_element_info(&self, name: &str) -> Option<ElementInfo> {
        let mut discovery = self.inner.element_discovery.write().await;
        discovery.get_element_info(name)
    }

    /// Start a flow (create and start its pipeline).
    pub async fn start_flow(&self, id: &FlowId) -> Result<PipelineState, PipelineError> {
        // Get the flow definition
        let flow = {
            let flows = self.inner.flows.read().await;
            flows.get(id).cloned()
        };

        let Some(mut flow) = flow else {
            return Err(PipelineError::InvalidFlow(format!(
                "Flow not found: {}",
                id
            )));
        };

        // Check if pipeline is already running
        {
            let pipelines = self.inner.pipelines.read().await;
            if pipelines.contains_key(id) {
                warn!("Pipeline already running for flow: {}", id);
                return Ok(PipelineState::Playing);
            }
        }

        info!("Starting flow: {} ({})", flow.name, id);

        // Create pipeline with event broadcaster
        let mut manager = PipelineManager::new(&flow, self.inner.events.clone())?;

        // Start pipeline
        let state = manager.start()?;

        // Store pipeline manager
        {
            let mut pipelines = self.inner.pipelines.write().await;
            pipelines.insert(*id, manager);
        }

        // Update flow state and persist
        flow.state = Some(state);
        {
            let mut flows = self.inner.flows.write().await;
            flows.insert(*id, flow.clone());
        }
        if let Err(e) = self.inner.storage.save_flow(&flow).await {
            error!("Failed to save flow state: {}", e);
        }

        // Broadcast events
        self.inner
            .events
            .broadcast(StromEvent::FlowStarted { flow_id: *id });
        self.inner.events.broadcast(StromEvent::FlowStateChanged {
            flow_id: *id,
            state: format!("{:?}", state),
        });

        Ok(state)
    }

    /// Stop a flow (stop and remove its pipeline).
    pub async fn stop_flow(&self, id: &FlowId) -> Result<PipelineState, PipelineError> {
        info!("Stopping flow: {}", id);

        // Get and remove the pipeline
        let manager = {
            let mut pipelines = self.inner.pipelines.write().await;
            pipelines.remove(id)
        };

        let Some(mut manager) = manager else {
            warn!("No active pipeline for flow: {}", id);
            return Ok(PipelineState::Null);
        };

        // Stop the pipeline
        let state = manager.stop()?;

        // Update flow state and persist
        let flow = {
            let mut flows = self.inner.flows.write().await;
            if let Some(flow) = flows.get_mut(id) {
                flow.state = Some(state);
                Some(flow.clone())
            } else {
                None
            }
        };

        if let Some(flow) = flow {
            if let Err(e) = self.inner.storage.save_flow(&flow).await {
                error!("Failed to save flow state: {}", e);
            }
        }

        // Broadcast events
        self.inner
            .events
            .broadcast(StromEvent::FlowStopped { flow_id: *id });
        self.inner.events.broadcast(StromEvent::FlowStateChanged {
            flow_id: *id,
            state: format!("{:?}", state),
        });

        Ok(state)
    }

    /// Get the state of a flow's pipeline.
    pub async fn get_flow_state(&self, id: &FlowId) -> Option<PipelineState> {
        let pipelines = self.inner.pipelines.read().await;
        pipelines.get(id).map(|p| p.get_state())
    }

    /// Generate a debug DOT graph for a flow's pipeline.
    /// Returns the DOT graph content as a string.
    pub async fn generate_debug_graph(&self, id: &FlowId) -> Option<String> {
        let pipelines = self.inner.pipelines.read().await;
        pipelines.get(id).map(|p| p.generate_dot_graph())
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::with_json_storage("flows.json")
    }
}
