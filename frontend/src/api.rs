//! API client for communicating with the Strom backend.

use serde::{Deserialize, Serialize};
use strom_types::element::ElementInfo;
use strom_types::{Flow, FlowId};

/// Version information from the backend
#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    pub version: String,
    pub git_hash: String,
    pub git_tag: String,
    pub git_branch: String,
    pub git_dirty: bool,
    pub build_timestamp: String,
}

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
    client: reqwest::Client,
}

impl ApiClient {
    /// Create a new API client with the given base URL.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Get the base URL for the API.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// List all flows.
    pub async fn list_flows(&self) -> ApiResult<Vec<Flow>> {
        use strom_types::api::FlowListResponse;
        use tracing::info;

        let url = format!("{}/flows", self.base_url);
        info!("Fetching flows from: {}", url);

        let response = self.client.get(&url).send().await.map_err(|e| {
            tracing::error!("Network error fetching flows: {}", e);
            ApiError::Network(e.to_string())
        })?;

        info!("Flows response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status().as_u16();
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
        use strom_types::api::FlowResponse;
        use tracing::info;

        let url = format!("{}/flows/{}", self.base_url, id);
        info!("Fetching flow from: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        info!("Flow response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        let flow_response: FlowResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse flow response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!("Successfully fetched flow: {}", flow_response.flow.name);
        Ok(flow_response.flow)
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

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Network request failed: {}", e);
                ApiError::Network(e.to_string())
            })?;

        info!("Response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status().as_u16();
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

        let response = self
            .client
            .post(&url)
            .json(flow)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Network request failed: {}", e);
                ApiError::Network(e.to_string())
            })?;

        info!("Response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status().as_u16();
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
        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        Ok(())
    }

    /// Start a flow.
    pub async fn start_flow(&self, id: FlowId) -> ApiResult<()> {
        let url = format!("{}/flows/{}/start", self.base_url, id);
        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        Ok(())
    }

    /// Stop a flow.
    pub async fn stop_flow(&self, id: FlowId) -> ApiResult<()> {
        let url = format!("{}/flows/{}/stop", self.base_url, id);
        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
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

        let response = self.client.get(&url).send().await.map_err(|e| {
            tracing::error!("Network error fetching elements: {}", e);
            ApiError::Network(e.to_string())
        })?;

        info!("Elements response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status().as_u16();
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
        use strom_types::api::ElementInfoResponse;
        use tracing::info;

        let url = format!("{}/elements/{}", self.base_url, name);
        info!("Fetching element info from: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        let element_response: ElementInfoResponse = response
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))?;

        info!("Successfully loaded element info for: {}", name);
        Ok(element_response.element)
    }

    /// Get pad properties for a specific element type (on-demand introspection).
    pub async fn get_element_pad_properties(&self, name: &str) -> ApiResult<ElementInfo> {
        use strom_types::api::ElementInfoResponse;
        use tracing::info;

        let url = format!("{}/elements/{}/pads", self.base_url, name);
        info!("Fetching element pad properties from: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        let element_response: ElementInfoResponse = response
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))?;

        info!("Successfully loaded pad properties for: {}", name);
        Ok(element_response.element)
    }

    /// Get the debug graph URL for a flow.
    /// Returns the full URL that can be opened in a new tab.
    pub fn get_debug_graph_url(&self, id: FlowId) -> String {
        format!("{}/flows/{}/debug-graph", self.base_url, id)
    }

    /// List all block definitions (built-in + user-defined).
    pub async fn list_blocks(&self) -> ApiResult<Vec<strom_types::BlockDefinition>> {
        use strom_types::block::BlockListResponse;
        use tracing::info;

        let url = format!("{}/blocks", self.base_url);
        info!("Fetching blocks from: {}", url);

        let response = self.client.get(&url).send().await.map_err(|e| {
            tracing::error!("Network error fetching blocks: {}", e);
            ApiError::Network(e.to_string())
        })?;

        info!("Blocks response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("HTTP error {}: {}", status, text);
            return Err(ApiError::Http(status, text));
        }

        let block_list: BlockListResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse block list response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!("Successfully loaded {} blocks", block_list.blocks.len());
        Ok(block_list.blocks)
    }

    /// Get a specific block definition by ID.
    pub async fn get_block(&self, id: &str) -> ApiResult<strom_types::BlockDefinition> {
        use strom_types::block::BlockResponse;
        use tracing::info;

        let url = format!("{}/blocks/{}", self.base_url, id);
        info!("Fetching block from: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        let block_response: BlockResponse = response
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))?;

        info!("Successfully loaded block: {}", id);
        Ok(block_response.block)
    }

    /// Create a new user-defined block.
    pub async fn create_block(
        &self,
        block: &strom_types::block::CreateBlockRequest,
    ) -> ApiResult<strom_types::BlockDefinition> {
        use strom_types::block::BlockResponse;
        use tracing::info;

        let url = format!("{}/blocks", self.base_url);
        info!("Creating block via API: POST {}", url);

        let response = self
            .client
            .post(&url)
            .json(block)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Network request failed: {}", e);
                ApiError::Network(e.to_string())
            })?;

        info!("Response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("HTTP error {}: {}", status, text);
            return Err(ApiError::Http(status, text));
        }

        let block_response: BlockResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse block response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!("Successfully created block: {}", block_response.block.name);
        Ok(block_response.block)
    }

    /// Update an existing user-defined block.
    pub async fn update_block(
        &self,
        block: &strom_types::BlockDefinition,
    ) -> ApiResult<strom_types::BlockDefinition> {
        use strom_types::block::BlockResponse;
        use tracing::info;

        let url = format!("{}/blocks/{}", self.base_url, block.id);
        info!("Updating block via API: PUT {}", url);

        let response = self
            .client
            .put(&url)
            .json(block)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Network request failed: {}", e);
                ApiError::Network(e.to_string())
            })?;

        info!("Response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("HTTP error {}: {}", status, text);
            return Err(ApiError::Http(status, text));
        }

        let block_response: BlockResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse block response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!("Successfully updated block: {}", block_response.block.name);
        Ok(block_response.block)
    }

    /// Delete a user-defined block.
    pub async fn delete_block(&self, id: &str) -> ApiResult<()> {
        let url = format!("{}/blocks/{}", self.base_url, id);
        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        Ok(())
    }

    /// Get all block categories.
    pub async fn get_block_categories(&self) -> ApiResult<Vec<String>> {
        use strom_types::block::BlockCategoriesResponse;
        use tracing::info;

        let url = format!("{}/blocks/categories", self.base_url);
        info!("Fetching block categories from: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        let categories_response: BlockCategoriesResponse = response
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))?;

        info!(
            "Successfully loaded {} categories",
            categories_response.categories.len()
        );
        Ok(categories_response.categories)
    }

    /// Get version and build information from the backend.
    pub async fn get_version(&self) -> ApiResult<VersionInfo> {
        use tracing::info;

        let url = format!("{}/version", self.base_url);
        info!("Fetching version info from: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        let version_info: VersionInfo = response
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))?;

        info!("Successfully fetched version: v{}", version_info.version);
        Ok(version_info)
    }

    /// Check authentication status and whether auth is required.
    pub async fn get_auth_status(&self) -> ApiResult<AuthStatusResponse> {
        use tracing::info;

        let url = format!("{}/auth/status", self.base_url);
        info!("Fetching auth status from: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        let auth_status: AuthStatusResponse = response
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))?;

        info!(
            "Auth status: required={}, authenticated={}",
            auth_status.auth_required, auth_status.authenticated
        );
        Ok(auth_status)
    }

    /// Login with username and password.
    pub async fn login(&self, username: String, password: String) -> ApiResult<LoginResponse> {
        use tracing::info;

        let url = format!("{}/login", self.base_url);
        info!("Attempting login for user: {}", username);

        let request = LoginRequest { username, password };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Network request failed: {}", e);
                ApiError::Network(e.to_string())
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("HTTP error {}: {}", status, text);
            return Err(ApiError::Http(status, text));
        }

        let login_response: LoginResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse login response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!("Login response: success={}", login_response.success);
        Ok(login_response)
    }

    /// Logout the current session.
    pub async fn logout(&self) -> ApiResult<()> {
        use tracing::info;

        let url = format!("{}/logout", self.base_url);
        info!("Logging out");

        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status, text));
        }

        info!("Logged out successfully");
        Ok(())
    }
}

/// Login request payload
#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Login response
#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub success: bool,
    pub message: String,
}

/// Authentication status response
#[derive(Debug, Deserialize)]
pub struct AuthStatusResponse {
    pub authenticated: bool,
    pub auth_required: bool,
    pub methods: Vec<String>,
}
