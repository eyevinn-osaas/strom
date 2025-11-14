//! API client for communicating with the Strom backend.

use gloo_net::http::Request;
use strom_types::element::ElementInfo;
use strom_types::{Flow, FlowId};

/// Result type for API operations.
pub type ApiResult<T> = Result<T, ApiError>;

/// API client errors.
#[derive(Debug, Clone)]
pub enum ApiError {
    /// Network error
    Network(String),
    /// HTTP error with status code
    Http(u16, String),
    /// Deserialization error
    Decode(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Network(msg) => write!(f, "Network error: {}", msg),
            ApiError::Http(code, msg) => write!(f, "HTTP {} error: {}", code, msg),
            ApiError::Decode(msg) => write!(f, "Decode error: {}", msg),
        }
    }
}

/// Client for the Strom REST API.
#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
}

impl ApiClient {
    /// Create a new API client with the given base URL.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    /// List all flows.
    pub async fn list_flows(&self) -> ApiResult<Vec<Flow>> {
        use strom_types::api::FlowListResponse;
        use tracing::info;

        let url = format!("{}/flows", self.base_url);
        info!("Fetching flows from: {}", url);

        let response = Request::get(&url).send().await.map_err(|e| {
            tracing::error!("Network error fetching flows: {}", e);
            ApiError::Network(e.to_string())
        })?;

        info!("Flows response status: {}", response.status());

        if !response.ok() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("HTTP error {}: {}", status, text);
            return Err(ApiError::Http(status, text));
        }

        let flow_list: FlowListResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse flow list response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!("Successfully loaded {} flows", flow_list.flows.len());
        Ok(flow_list.flows)
    }

    /// Get a specific flow by ID.
    pub async fn get_flow(&self, id: FlowId) -> ApiResult<Flow> {
        let url = format!("{}/flows/{}", self.base_url, id);
        let response = Request::get(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.ok() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        response
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))
    }

    /// Create a new flow.
    pub async fn create_flow(&self, flow: &Flow) -> ApiResult<Flow> {
        use strom_types::api::{CreateFlowRequest, FlowResponse};
        use tracing::info;

        let url = format!("{}/flows", self.base_url);
        info!("Creating flow via API: POST {}", url);
        info!("Flow data: name='{}'", flow.name);

        let request = CreateFlowRequest {
            name: flow.name.clone(),
        };

        let response = Request::post(&url)
            .json(&request)
            .map_err(|e| {
                tracing::error!("Failed to serialize create request: {}", e);
                ApiError::Network(e.to_string())
            })?
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Network request failed: {}", e);
                ApiError::Network(e.to_string())
            })?;

        info!("Response status: {}", response.status());

        if !response.ok() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("HTTP error {}: {}", status, text);
            return Err(ApiError::Http(status, text));
        }

        let flow_response: FlowResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse flow response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!("Successfully created flow: {}", flow_response.flow.name);
        Ok(flow_response.flow)
    }

    /// Update an existing flow.
    pub async fn update_flow(&self, flow: &Flow) -> ApiResult<Flow> {
        use strom_types::api::FlowResponse;
        use tracing::info;

        let url = format!("{}/flows/{}", self.base_url, flow.id);
        info!("Updating flow via API: POST {}", url);
        info!(
            "Flow data: id={}, name='{}', elements={}, links={}",
            flow.id,
            flow.name,
            flow.elements.len(),
            flow.links.len()
        );

        let response = Request::post(&url)
            .json(flow)
            .map_err(|e| {
                tracing::error!("Failed to serialize update request: {}", e);
                ApiError::Network(e.to_string())
            })?
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Network request failed: {}", e);
                ApiError::Network(e.to_string())
            })?;

        info!("Response status: {}", response.status());

        if !response.ok() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("HTTP error {}: {}", status, text);
            return Err(ApiError::Http(status, text));
        }

        let flow_response: FlowResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse flow response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!("Successfully updated flow: {}", flow_response.flow.name);
        Ok(flow_response.flow)
    }

    /// Delete a flow.
    pub async fn delete_flow(&self, id: FlowId) -> ApiResult<()> {
        let url = format!("{}/flows/{}", self.base_url, id);
        let response = Request::delete(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.ok() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        Ok(())
    }

    /// Start a flow.
    pub async fn start_flow(&self, id: FlowId) -> ApiResult<()> {
        let url = format!("{}/flows/{}/start", self.base_url, id);
        let response = Request::post(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.ok() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        Ok(())
    }

    /// Stop a flow.
    pub async fn stop_flow(&self, id: FlowId) -> ApiResult<()> {
        let url = format!("{}/flows/{}/stop", self.base_url, id);
        let response = Request::post(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.ok() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        Ok(())
    }

    /// List available GStreamer elements.
    pub async fn list_elements(&self) -> ApiResult<Vec<ElementInfo>> {
        use strom_types::api::ElementListResponse;
        use tracing::info;

        let url = format!("{}/elements", self.base_url);
        info!("Fetching elements from: {}", url);

        let response = Request::get(&url).send().await.map_err(|e| {
            tracing::error!("Network error fetching elements: {}", e);
            ApiError::Network(e.to_string())
        })?;

        info!("Elements response status: {}", response.status());

        if !response.ok() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("HTTP error {}: {}", status, text);
            return Err(ApiError::Http(status, text));
        }

        let element_list: ElementListResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse element list response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!(
            "Successfully loaded {} elements",
            element_list.elements.len()
        );
        Ok(element_list.elements)
    }

    /// Get details about a specific element type.
    pub async fn get_element_info(&self, name: &str) -> ApiResult<ElementInfo> {
        let url = format!("{}/elements/{}", self.base_url, name);
        let response = Request::get(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.ok() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        response
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))
    }

    /// Get the debug graph URL for a flow.
    /// Returns the full URL that can be opened in a new tab.
    pub fn get_debug_graph_url(&self, id: FlowId) -> String {
        format!("{}/flows/{}/debug-graph", self.base_url, id)
    }
}
