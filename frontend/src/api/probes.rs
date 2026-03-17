//! Buffer age probe API client methods.

use super::{ApiClient, ApiError, ApiResult};
use strom_types::api::{ActivateProbeRequest, ProbeResponse};
use strom_types::FlowId;

impl ApiClient {
    /// Activate a buffer age probe on a pad.
    pub async fn activate_probe(
        &self,
        flow_id: &FlowId,
        element_id: &str,
        pad_name: &str,
        sample_interval: Option<u32>,
        timeout_secs: Option<u32>,
    ) -> ApiResult<ProbeResponse> {
        let url = format!("{}/flows/{}/probes", self.base_url, flow_id);
        let req = ActivateProbeRequest {
            element_id: element_id.to_string(),
            pad_name: pad_name.to_string(),
            sample_interval,
            timeout_secs,
        };

        let response = self
            .with_auth(self.client.post(&url))
            .json(&req)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if response.status().is_success() {
            response
                .json::<ProbeResponse>()
                .await
                .map_err(|e| ApiError::Decode(e.to_string()))
        } else {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            Err(ApiError::Http(status, text))
        }
    }

    /// Deactivate a probe.
    pub async fn deactivate_probe(&self, flow_id: &FlowId, probe_id: &str) -> ApiResult<()> {
        let url = format!("{}/flows/{}/probes/{}", self.base_url, flow_id, probe_id);

        let response = self
            .with_auth(self.client.delete(&url))
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            Err(ApiError::Http(status, text))
        }
    }
}
