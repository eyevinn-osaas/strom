//! Event broadcasting system for real-time updates.

use axum::response::sse::{Event, KeepAlive};
use axum::response::Sse;
use futures::Stream;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use strom_types::StromEvent;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::debug;

/// Event broadcaster for SSE (Server-Sent Events).
#[derive(Clone)]
pub struct EventBroadcaster {
    /// Broadcast channel for events
    sender: Arc<broadcast::Sender<StromEvent>>,
}

impl EventBroadcaster {
    /// Create a new event broadcaster with a buffer size.
    pub fn new(buffer_size: usize) -> Self {
        let (sender, _) = broadcast::channel(buffer_size);
        Self {
            sender: Arc::new(sender),
        }
    }

    /// Broadcast an event to all connected clients.
    pub fn broadcast(&self, event: StromEvent) {
        debug!("Broadcasting event: {}", event.description());
        // broadcast::send returns the number of receivers
        // We don't care about the result since clients may or may not be connected
        let _ = self.sender.send(event);
    }

    /// Subscribe to events and get a SSE stream.
    pub fn subscribe(&self) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
        let rx = self.sender.subscribe();
        let stream = BroadcastStream::new(rx);

        let event_stream = stream.filter_map(|result| match result {
            Ok(event) => {
                debug!("Sending SSE event: {}", event.description());
                // Convert StromEvent to SSE Event
                match serde_json::to_string(&event) {
                    Ok(json) => Some(Ok(Event::default().data(json))),
                    Err(e) => {
                        tracing::error!("Failed to serialize event: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                // BroadcastStream returns RecvError when lagging
                tracing::warn!("Client lagging, skipping events: {}", e);
                None
            }
        });

        Sse::new(event_stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
    }

    /// Get the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBroadcaster {
    fn default() -> Self {
        Self::new(100) // Default buffer of 100 events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strom_types::FlowId;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_broadcaster_creation() {
        let broadcaster = EventBroadcaster::new(10);
        assert_eq!(broadcaster.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn test_broadcast_event() {
        let broadcaster = EventBroadcaster::new(10);
        let flow_id = FlowId::from(Uuid::new_v4());

        // Subscribe before broadcasting
        let _subscription = broadcaster.subscribe();
        assert_eq!(broadcaster.subscriber_count(), 1);

        // Broadcast an event
        broadcaster.broadcast(StromEvent::FlowCreated { flow_id });
    }
}
