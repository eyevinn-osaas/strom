//! Server-Sent Events endpoint for real-time updates.

use axum::extract::State;
use axum::response::sse::Sse;
use futures::Stream;
use std::convert::Infallible;
use tracing::info;

use crate::state::AppState;

/// Subscribe to Server-Sent Events for real-time updates.
///
/// This endpoint establishes a long-lived connection and streams events
/// to the client whenever flows are created, updated, deleted, started, or stopped.
///
/// Example usage from JavaScript:
/// ```javascript
/// const eventSource = new EventSource('http://localhost:3000/api/events');
/// eventSource.onmessage = (event) => {
///     const data = JSON.parse(event.data);
///     console.log('Received event:', data);
/// };
/// ```
pub async fn events_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    info!(
        "New SSE client connected (total subscribers: {})",
        state.events().subscriber_count() + 1
    );
    state.events().subscribe()
}
