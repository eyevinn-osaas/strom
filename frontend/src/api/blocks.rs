use super::*;

impl ApiClient {
    /// Get a thumbnail from a block's video tap.
    ///
    /// Returns JPEG-encoded image bytes for the specified block and tap index.
    pub async fn get_block_thumbnail(
        &self,
        flow_id: &str,
        block_id: &str,
        index: usize,
    ) -> ApiResult<Vec<u8>> {
        let url = if index == 0 {
            format!(
                "{}/flows/{}/blocks/{}/thumbnail",
                self.base_url, flow_id, block_id
            )
        } else {
            format!(
                "{}/flows/{}/blocks/{}/thumbnail?index={}",
                self.base_url, flow_id, block_id, index
            )
        };

        let response = self
            .with_auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status_code = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http(status_code, text));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))?;
        Ok(bytes.to_vec())
    }

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
