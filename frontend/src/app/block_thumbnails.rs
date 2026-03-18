//! Thumbnail polling for thumbnail blocks.
//!
//! Fetches JPEG thumbnails from the REST API for any `builtin.thumbnail`
//! block in a running flow, decodes them to egui textures, and stores
//! them for rendering in both the block node and the properties panel.

use super::{get_local_storage, remove_local_storage, set_local_storage, spawn_task};
use egui::Context;

impl super::StromApp {
    /// Fetch thumbnails for thumbnail blocks in the selected flow.
    pub(super) fn poll_block_thumbnails(&mut self, ctx: &Context) {
        let refresh_interval = std::time::Duration::from_millis(1000);

        // Evict thumbnails for flows that are no longer running
        let running_flows: std::collections::HashSet<_> = self
            .flows
            .iter()
            .filter(|f| f.state == Some(strom_types::PipelineState::Playing))
            .map(|f| f.id)
            .collect();
        self.block_thumbnails
            .retain(|(flow_id, _), _| running_flows.contains(flow_id));

        // Only poll the currently selected flow
        let flow = match self
            .selected_flow_id
            .and_then(|id| self.flows.iter().find(|f| f.id == id))
        {
            Some(f) => f,
            None => return,
        };

        // Only poll if running
        if flow.state != Some(strom_types::PipelineState::Playing) {
            return;
        }

        for block in &flow.blocks {
            if block.block_definition_id != "builtin.thumbnail" {
                continue;
            }

            let key = (flow.id, block.id.clone());

            // Skip if already loading
            if self.block_thumbnail_loading.contains(&key) {
                continue;
            }

            // Skip if recently fetched
            if let Some(last) = self.block_thumbnail_fetch_times.get(&key) {
                if last.elapsed() < refresh_interval {
                    continue;
                }
            }

            self.block_thumbnail_loading.insert(key.clone());
            self.block_thumbnail_fetch_times
                .insert(key.clone(), instant::Instant::now());

            let flow_id_str = flow.id.to_string();
            let block_id = block.id.clone();
            let api = self.api.clone();
            let ctx = ctx.clone();
            let storage_key = format!("block_thumb_{}_{}", flow.id, block.id);

            spawn_task(async move {
                match api.get_block_thumbnail(&flow_id_str, &block_id, 0).await {
                    Ok(jpeg_bytes) => {
                        use base64::Engine;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);
                        set_local_storage(&storage_key, &b64);
                        ctx.request_repaint();
                    }
                    Err(_) => {
                        set_local_storage(&format!("{}_err", storage_key), "1");
                        ctx.request_repaint();
                    }
                }
            });
        }
    }

    /// Check for loaded thumbnail data and convert to egui textures.
    pub(super) fn check_block_thumbnails(&mut self, ctx: &Context) {
        // Collect keys to check (avoid borrow issues)
        let keys: Vec<_> = self.block_thumbnail_loading.iter().cloned().collect();

        for key in keys {
            let (flow_id, ref block_id) = key;
            let storage_key = format!("block_thumb_{}_{}", flow_id, block_id);
            let err_key = format!("{}_err", storage_key);

            // Check for error
            if get_local_storage(&err_key).is_some() {
                remove_local_storage(&err_key);
                self.block_thumbnail_loading.remove(&key);
                continue;
            }

            // Check for data
            if let Some(b64) = get_local_storage(&storage_key) {
                remove_local_storage(&storage_key);
                self.block_thumbnail_loading.remove(&key);

                use base64::Engine;
                if let Ok(jpeg_bytes) = base64::engine::general_purpose::STANDARD.decode(&b64) {
                    if let Ok(img) =
                        image::load_from_memory_with_format(&jpeg_bytes, image::ImageFormat::Jpeg)
                    {
                        let rgba = img.to_rgba8();
                        let size = [rgba.width() as usize, rgba.height() as usize];
                        let color_image =
                            egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                        let texture_name = format!("block_thumb_{}_{}", flow_id, block_id);
                        let texture = ctx.load_texture(
                            texture_name,
                            color_image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.block_thumbnails.insert(key, texture);
                    }
                }
            }
        }
    }

    /// Get a thumbnail texture for a block, if available.
    pub fn get_block_thumbnail(
        &self,
        flow_id: strom_types::FlowId,
        block_id: &str,
    ) -> Option<&egui::TextureHandle> {
        self.block_thumbnails.get(&(flow_id, block_id.to_string()))
    }
}
