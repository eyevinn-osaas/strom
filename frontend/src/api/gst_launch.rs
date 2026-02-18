use serde::{Deserialize, Serialize};

use super::*;

/// Response from parsing a gst-launch pipeline
#[derive(Debug, Clone, Deserialize)]
pub struct ParseGstLaunchResponse {
    /// Elements extracted from the parsed pipeline
    pub elements: Vec<strom_types::Element>,
    /// Links between elements
    pub links: Vec<strom_types::element::Link>,
}

/// Response from exporting to gst-launch syntax
#[derive(Debug, Clone, Deserialize)]
pub struct ExportGstLaunchResponse {
    /// The generated gst-launch-1.0 pipeline string
    pub pipeline: String,
}

impl ApiClient {
    /// Parse a gst-launch-1.0 pipeline string and return elements and links.
    ///
    /// This uses the backend's GStreamer parser to ensure complete compatibility
    /// with the gst-launch-1.0 syntax.
    pub async fn parse_gst_launch(&self, pipeline: &str) -> ApiResult<ParseGstLaunchResponse> {
        use tracing::info;

        let url = format!("{}/gst-launch/parse", self.base_url);
        info!("Parsing gst-launch pipeline via API: POST {}", url);

        #[derive(Serialize)]
        struct ParseRequest<'a> {
            pipeline: &'a str,
        }

        let request = ParseRequest { pipeline };

        let response = self
            .with_auth(self.client.post(&url).json(&request))
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

        let parse_response: ParseGstLaunchResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!(
            "Successfully parsed pipeline: {} elements, {} links",
            parse_response.elements.len(),
            parse_response.links.len()
        );
        Ok(parse_response)
    }

    /// Export elements and links to gst-launch-1.0 syntax.
    pub async fn export_gst_launch(
        &self,
        elements: &[strom_types::Element],
        links: &[strom_types::element::Link],
    ) -> ApiResult<String> {
        use tracing::info;

        let url = format!("{}/gst-launch/export", self.base_url);
        info!("Exporting to gst-launch syntax via API: POST {}", url);

        #[derive(Serialize)]
        struct ExportRequest<'a> {
            elements: &'a [strom_types::Element],
            links: &'a [strom_types::element::Link],
        }

        let request = ExportRequest { elements, links };

        let response = self
            .with_auth(self.client.post(&url).json(&request))
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

        let export_response: ExportGstLaunchResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse response: {}", e);
            ApiError::Decode(e.to_string())
        })?;

        info!("Successfully exported pipeline");
        Ok(export_response.pipeline)
    }
}
