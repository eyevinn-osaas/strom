//! WHIP session manager for per-client whipserversrc elements.
//!
//! gst-plugin-webrtc 0.15 removed multi-session support from whipserversrc.
//! This manager creates one whipserversrc per WHIP client session, each with
//! its own internal HTTP port. Downstream elements (liveadder, videoconvert)
//! remain persistent and shared.

use crate::blocks::{DynamicWebrtcbinStore, WhepStreamMode};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

/// Configuration for a WHIP endpoint, registered at pipeline start.
///
/// Stores everything needed to create a new whipserversrc for each session.
pub struct WhipEndpointConfig {
    pub instance_id: String,
    pub endpoint_id: String,
    pub mode: WhepStreamMode,
    pub stun_server: Option<String>,
    pub turn_server: Option<String>,
    pub ice_transport_policy: String,
    /// Weak ref to liveadder (audio mixing element), if audio mode
    pub liveadder_weak: Option<gst::glib::WeakRef<gst::Element>>,
    /// Weak ref to output videoconvert, if video mode
    pub videoconvert_weak: Option<gst::glib::WeakRef<gst::Element>>,
    /// Weak ref to the pipeline
    pub pipeline_weak: gst::glib::WeakRef<gst::Pipeline>,
    /// Whether to decode RTP to raw media (true) or pass through RTP (false)
    pub decode: bool,
    /// Shared dynamic webrtcbin store for ICE policy tracking
    pub dynamic_webrtcbin_store: DynamicWebrtcbinStore,
}

/// An active WHIP session (one whipserversrc element per client).
/// Each session runs in its own GStreamer pipeline to isolate NiceAgent instances.
struct WhipSession {
    /// Internal port where this session's whipserversrc is listening
    port: u16,
    /// The whipserversrc element for this session
    element: gst::Element,
    /// The isolated pipeline for this session's whipserversrc
    session_pipeline: gst::Pipeline,
    /// The endpoint this session belongs to
    endpoint_id: String,
}

/// Manages WHIP sessions across all endpoints.
///
/// Thread-safe: uses RwLock for the sessions map and read-only Arc for endpoint configs.
pub struct WhipSessionManager {
    /// endpoint_id -> config (registered at pipeline start, immutable after that)
    endpoints: RwLock<HashMap<String, Arc<WhipEndpointConfig>>>,
    /// resource_id -> session (created/removed dynamically as clients connect/disconnect)
    sessions: RwLock<HashMap<String, WhipSession>>,
}

impl WhipSessionManager {
    pub fn new() -> Self {
        Self {
            endpoints: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Register an endpoint configuration (called once per WHIP Input block at pipeline start).
    pub fn register_endpoint(&self, endpoint_id: String, config: WhipEndpointConfig) {
        info!(
            "WhipSessionManager: Registering endpoint '{}' (instance: {}, mode: {:?})",
            endpoint_id, config.instance_id, config.mode
        );
        let mut endpoints = self.endpoints.write().unwrap();
        endpoints.insert(endpoint_id, Arc::new(config));
    }

    /// Get the endpoint configuration for creating new sessions.
    pub fn get_endpoint_config(&self, endpoint_id: &str) -> Option<Arc<WhipEndpointConfig>> {
        let endpoints = self.endpoints.read().unwrap();
        endpoints.get(endpoint_id).cloned()
    }

    /// Register a new session after a whipserversrc has been created.
    pub fn register_session(
        &self,
        resource_id: String,
        port: u16,
        element: gst::Element,
        session_pipeline: gst::Pipeline,
        endpoint_id: String,
    ) {
        info!(
            "WhipSessionManager: Registering session '{}' on port {} for endpoint '{}'",
            resource_id, port, endpoint_id
        );
        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(
            resource_id,
            WhipSession {
                port,
                element,
                session_pipeline,
                endpoint_id,
            },
        );
    }

    /// Look up the port for a session by resource_id.
    pub fn get_session_port(&self, resource_id: &str) -> Option<u16> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(resource_id).map(|s| s.port)
    }

    /// Look up the port for a session, also returning the endpoint_id.
    pub fn get_session_info(&self, resource_id: &str) -> Option<(u16, String)> {
        let sessions = self.sessions.read().unwrap();
        sessions
            .get(resource_id)
            .map(|s| (s.port, s.endpoint_id.clone()))
    }

    /// Remove a session and return (element, session_pipeline, endpoint_id, port) for teardown.
    pub fn remove_session(
        &self,
        resource_id: &str,
    ) -> Option<(gst::Element, gst::Pipeline, String, u16)> {
        let mut sessions = self.sessions.write().unwrap();
        sessions
            .remove(resource_id)
            .map(|s| (s.element, s.session_pipeline, s.endpoint_id, s.port))
    }

    /// Remove all sessions for a given endpoint (called during pipeline stop).
    /// Returns the session pipelines for teardown.
    pub fn remove_all_sessions(&self, endpoint_id: &str) -> Vec<gst::Pipeline> {
        let mut sessions = self.sessions.write().unwrap();
        let resource_ids: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| s.endpoint_id == endpoint_id)
            .map(|(k, _)| k.clone())
            .collect();

        let mut pipelines = Vec::new();
        for resource_id in &resource_ids {
            if let Some(session) = sessions.remove(resource_id) {
                info!(
                    "WhipSessionManager: Removing session '{}' for endpoint '{}'",
                    resource_id, endpoint_id
                );
                pipelines.push(session.session_pipeline);
            }
        }
        pipelines
    }

    /// Unregister an endpoint (called during pipeline stop).
    pub fn unregister_endpoint(&self, endpoint_id: &str) {
        info!(
            "WhipSessionManager: Unregistering endpoint '{}'",
            endpoint_id
        );
        let mut endpoints = self.endpoints.write().unwrap();
        endpoints.remove(endpoint_id);
    }

    /// List all registered endpoint IDs.
    pub fn list_endpoints(&self) -> Vec<String> {
        let endpoints = self.endpoints.read().unwrap();
        endpoints.keys().cloned().collect()
    }

    /// Teardown a session's isolated pipeline.
    pub fn teardown_session_pipeline(session_pipeline: &gst::Pipeline) {
        let name = session_pipeline.name().to_string();
        debug!(
            "WhipSessionManager: Tearing down session pipeline '{}'",
            name
        );

        if let Err(e) = session_pipeline.set_state(gst::State::Null) {
            warn!(
                "WhipSessionManager: Failed to set session pipeline {} to Null: {:?}",
                name, e
            );
        }
    }
}

impl Default for WhipSessionManager {
    fn default() -> Self {
        Self::new()
    }
}
