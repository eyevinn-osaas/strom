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

    /// Get a specific block definition by ID.
    pub async fn get_block(&self, id: &str) -> ApiResult<strom_types::BlockDefinition> {
        use strom_types::block::BlockResponse;
        use tracing::info;

        let url = format!("{}/blocks/{}", self.base_url, id);
        info!("Fetching block from: {}", url);

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
            .with_auth(self.client.post(&url).json(block))
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
            .with_auth(self.client.put(&url).json(block))
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
            .with_auth(self.client.delete(&url))
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
            .with_auth(self.client.get(&url))
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
}
