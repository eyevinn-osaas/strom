//! API client for communicating with the Strom backend.

mod auth;
mod blocks;
mod compositor;
mod discovery;
mod elements;
mod flows;
mod gst_launch;
mod media;
mod player;
mod stats;

pub use auth::AuthStatusResponse;
pub use flows::LatencyInfo;

use serde::Deserialize;

/// Version information from the backend
#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    pub version: String,
    pub git_hash: String,
    pub git_tag: String,
    pub git_branch: String,
    pub git_dirty: bool,
    pub build_timestamp: String,
    /// Unique build ID (UUID) generated at compile time
    #[serde(default)]
    pub build_id: String,
    #[serde(default)]
    pub gstreamer_version: String,
    #[serde(default)]
    pub os_info: String,
    #[serde(default)]
    pub in_docker: bool,
    /// When the Strom server process was started (ISO 8601 format with timezone)
    /// This is the process uptime, not the system uptime
    #[serde(default)]
    pub process_started_at: String,
    /// When the system was booted (ISO 8601 format with timezone)
    #[serde(default)]
    pub system_boot_time: String,
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
    /// Optional auth token for Bearer authentication (used by native GUI)
    auth_token: Option<String>,
}

impl ApiClient {
    /// Create a new API client with the given base URL (WASM only, no auth).
    #[cfg(target_arch = "wasm32")]
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
            auth_token: None,
        }
    }

    /// Create a new API client with authentication token (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_with_auth(base_url: impl Into<String>, auth_token: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
            auth_token,
        }
    }

    /// Helper to add auth header to a request builder
    pub(super) fn with_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref token) = self.auth_token {
            builder.header("Authorization", format!("Bearer {}", token))
        } else {
            builder
        }
    }

    /// Get the base URL for the API.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}
