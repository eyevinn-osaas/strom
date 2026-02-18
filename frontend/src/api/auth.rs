use serde::{Deserialize, Serialize};

use super::*;

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
#[derive(Debug, Clone, Deserialize)]
pub struct AuthStatusResponse {
    pub authenticated: bool,
    pub auth_required: bool,
    pub methods: Vec<String>,
}

impl ApiClient {
    /// Get version and build information from the backend.
    pub async fn get_version(&self) -> ApiResult<VersionInfo> {
        use tracing::info;

        let url = format!("{}/version", self.base_url);
        info!("Fetching version info from: {}", url);

        let response = self
            .with_auth(self.client.get(&url))
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
            .with_auth(self.client.get(&url))
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
            .with_auth(self.client.post(&url).json(&request))
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
            .with_auth(self.client.post(&url))
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
