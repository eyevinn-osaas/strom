use anyhow::{Context, Result};
use reqwest::Client;
use strom_types::{
    api::{CreateFlowRequest, FlowListResponse, FlowResponse},
    element::ElementInfo,
    flow::Flow,
};

/// HTTP client for Strom REST API
#[derive(Clone, Debug)]
pub struct StromClient {
    base_url: String,
    client: Client,
}

impl StromClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
        }
    }

    /// List all flows
    pub async fn list_flows(&self) -> Result<FlowListResponse> {
        let url = format!("{}/api/flows", self.base_url);
        self.client
            .get(&url)
            .send()
            .await
            .context("Failed to send request")?
            .json()
            .await
            .context("Failed to parse response")
    }

    /// Get a specific flow
    pub async fn get_flow(&self, flow_id: &str) -> Result<FlowResponse> {
        let url = format!("{}/api/flows/{}", self.base_url, flow_id);
        self.client
            .get(&url)
            .send()
            .await
            .context("Failed to send request")?
            .json()
            .await
            .context("Failed to parse response")
    }

    /// Create a new flow
    pub async fn create_flow(&self, request: CreateFlowRequest) -> Result<FlowResponse> {
        let url = format!("{}/api/flows", self.base_url);
        self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send request")?
            .json()
            .await
            .context("Failed to parse response")
    }

    /// Update a flow
    pub async fn update_flow(&self, flow_id: &str, flow: Flow) -> Result<FlowResponse> {
        let url = format!("{}/api/flows/{}", self.base_url, flow_id);
        self.client
            .post(&url)
            .json(&flow)
            .send()
            .await
            .context("Failed to send request")?
            .json()
            .await
            .context("Failed to parse response")
    }

    /// Delete a flow
    pub async fn delete_flow(&self, flow_id: &str) -> Result<()> {
        let url = format!("{}/api/flows/{}", self.base_url, flow_id);
        self.client
            .delete(&url)
            .send()
            .await
            .context("Failed to send request")?;
        Ok(())
    }

    /// Start a flow
    pub async fn start_flow(&self, flow_id: &str) -> Result<()> {
        let url = format!("{}/api/flows/{}/start", self.base_url, flow_id);
        self.client
            .post(&url)
            .send()
            .await
            .context("Failed to send request")?;
        Ok(())
    }

    /// Stop a flow
    pub async fn stop_flow(&self, flow_id: &str) -> Result<()> {
        let url = format!("{}/api/flows/{}/stop", self.base_url, flow_id);
        self.client
            .post(&url)
            .send()
            .await
            .context("Failed to send request")?;
        Ok(())
    }

    /// List available GStreamer elements
    pub async fn list_elements(&self) -> Result<Vec<ElementInfo>> {
        let url = format!("{}/api/elements", self.base_url);
        self.client
            .get(&url)
            .send()
            .await
            .context("Failed to send request")?
            .json()
            .await
            .context("Failed to parse response")
    }

    /// Get element information
    pub async fn get_element_info(&self, element_name: &str) -> Result<ElementInfo> {
        let url = format!("{}/api/elements/{}", self.base_url, element_name);
        self.client
            .get(&url)
            .send()
            .await
            .context("Failed to send request")?
            .json()
            .await
            .context("Failed to parse response")
    }
}
