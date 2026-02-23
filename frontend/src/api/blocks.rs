use super::*;

impl ApiClient {
    /// List all block definitions (built-in + user-defined).
    pub async fn list_blocks(&self) -> ApiResult<Vec<strom_types::BlockDefinition>> {
        use strom_types::block::BlockListResponse;
        use tracing::info;

        let url = format!("{}/blocks", self.base_url);
        info!("Fetching blocks from: {}", url);

        let response = self
            .with_auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| {
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
}
