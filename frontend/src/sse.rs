//! Server-Sent Events (SSE) client for real-time updates.

use strom_types::StromEvent;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{EventSource, MessageEvent};

/// SSE client for connecting to the backend event stream.
pub struct SseClient {
    event_source: Option<EventSource>,
    url: String,
}

impl SseClient {
    /// Create a new SSE client with the given event stream URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            event_source: None,
            url: url.into(),
        }
    }

    /// Connect to the SSE stream and set up event handlers.
    ///
    /// The `on_event` callback will be called for each event received.
    /// Returns true if connection was successful, false otherwise.
    pub fn connect<F>(&mut self, on_event: F) -> bool
    where
        F: Fn(StromEvent) + 'static,
    {
        tracing::info!("Connecting to SSE stream: {}", self.url);

        // Create EventSource
        let event_source = match EventSource::new(&self.url) {
            Ok(es) => es,
            Err(e) => {
                tracing::error!("Failed to create EventSource: {:?}", e);
                return false;
            }
        };

        // Set up message handler
        let onmessage_callback = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Some(data) = e.data().as_string() {
                tracing::debug!("Received SSE message: {}", data);

                // Parse the event
                match serde_json::from_str::<StromEvent>(&data) {
                    Ok(event) => {
                        tracing::info!("Parsed SSE event: {}", event.description());
                        on_event(event);
                    }
                    Err(err) => {
                        tracing::error!("Failed to parse SSE event: {}", err);
                    }
                }
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        event_source.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
        onmessage_callback.forget(); // Keep the closure alive

        // Set up error handler
        let onerror_callback = Closure::wrap(Box::new(move |e: web_sys::Event| {
            tracing::error!("SSE error: {:?}", e);
        }) as Box<dyn FnMut(web_sys::Event)>);

        event_source.set_onerror(Some(onerror_callback.as_ref().unchecked_ref()));
        onerror_callback.forget(); // Keep the closure alive

        // Set up open handler
        let onopen_callback = Closure::wrap(Box::new(move |_: web_sys::Event| {
            tracing::info!("SSE connection opened");
        }) as Box<dyn FnMut(web_sys::Event)>);

        event_source.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
        onopen_callback.forget(); // Keep the closure alive

        self.event_source = Some(event_source);
        true
    }

    /// Disconnect from the SSE stream.
    pub fn disconnect(&mut self) {
        if let Some(es) = self.event_source.take() {
            tracing::info!("Disconnecting from SSE stream");
            es.close();
        }
    }

    /// Check if the client is currently connected.
    pub fn is_connected(&self) -> bool {
        self.event_source
            .as_ref()
            .map(|es| es.ready_state() == EventSource::OPEN)
            .unwrap_or(false)
    }
}

impl Drop for SseClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}
