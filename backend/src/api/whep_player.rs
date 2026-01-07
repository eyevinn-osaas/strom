//! WHEP Player - serves a simple web page to connect to WHEP endpoints and play audio.
//! Also provides a proxy endpoint to avoid CORS issues when connecting to local WHEP servers.
//!
//! Two proxy modes:
//! 1. /api/whep-proxy?endpoint=... - legacy, direct URL proxy
//! 2. /api/whep/{endpoint_id}/... - new, uses WhepRegistry to look up internal port

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct WhepPlayerQuery {
    /// The WHEP endpoint URL to connect to
    endpoint: Option<String>,
}

#[derive(Deserialize)]
pub struct WhepProxyQuery {
    /// The target WHEP endpoint URL
    endpoint: String,
}

/// Proxy WHEP requests to avoid CORS issues.
/// POST /api/whep-proxy?endpoint=http://localhost:8190
pub async fn whep_proxy(
    Query(params): Query<WhepProxyQuery>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let client = reqwest::Client::new();

    // Forward the request to the WHEP endpoint
    let mut request = client.post(&params.endpoint);

    // Forward content-type header
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        if let Ok(ct) = content_type.to_str() {
            request = request.header(header::CONTENT_TYPE, ct);
        }
    }

    request = request.body(body);

    match request.send().await {
        Ok(response) => {
            let status = response.status();

            // Get Location header for resource URL
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            let body_bytes = response.bytes().await.unwrap_or_default();

            let mut builder = Response::builder()
                .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK))
                .header(header::CONTENT_TYPE, "application/sdp")
                // Add CORS headers
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .header(
                    header::ACCESS_CONTROL_ALLOW_METHODS,
                    "POST, DELETE, OPTIONS",
                )
                .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
                .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Location");

            if let Some(loc) = location {
                // Rewrite location to go through our proxy
                let proxy_location = format!(
                    "/api/whep-proxy?endpoint={}",
                    urlencoding::encode(&format!("{}{}", params.endpoint, loc))
                );
                builder = builder.header(header::LOCATION, proxy_location);
            }

            builder.body(Body::from(body_bytes)).unwrap()
        }
        Err(e) => Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(Body::from(format!("Proxy error: {}", e)))
            .unwrap(),
    }
}

/// Handle DELETE requests for WHEP resource cleanup
pub async fn whep_proxy_delete(Query(params): Query<WhepProxyQuery>) -> Response {
    let client = reqwest::Client::new();

    match client.delete(&params.endpoint).send().await {
        Ok(response) => {
            let status = response.status();
            Response::builder()
                .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK))
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Body::empty())
                .unwrap()
        }
        Err(e) => Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(Body::from(format!("Proxy error: {}", e)))
            .unwrap(),
    }
}

/// Handle OPTIONS preflight requests for CORS
pub async fn whep_proxy_options() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            "POST, DELETE, OPTIONS",
        )
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
        .header(header::ACCESS_CONTROL_MAX_AGE, "86400")
        .body(Body::empty())
        .unwrap()
}

/// Serve the WHEP player HTML page.
///
/// Query parameters:
/// - `endpoint`: The WHEP endpoint URL (e.g., `http://localhost:8190`)
pub async fn whep_player(Query(params): Query<WhepPlayerQuery>) -> impl IntoResponse {
    let endpoint = params.endpoint.unwrap_or_default();

    let html = format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>WHEP Audio Player - Strom</title>
    <style>
        * {{
            box-sizing: border-box;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
            color: #eee;
            min-height: 100vh;
            margin: 0;
            padding: 20px;
            display: flex;
            flex-direction: column;
            align-items: center;
        }}
        .container {{
            max-width: 500px;
            width: 100%;
            background: rgba(255,255,255,0.05);
            border-radius: 16px;
            padding: 30px;
            box-shadow: 0 8px 32px rgba(0,0,0,0.3);
        }}
        h1 {{
            margin: 0 0 20px 0;
            font-size: 24px;
            text-align: center;
        }}
        .form-group {{
            margin-bottom: 20px;
        }}
        label {{
            display: block;
            margin-bottom: 8px;
            font-size: 14px;
            color: #aaa;
        }}
        input {{
            width: 100%;
            padding: 12px;
            border: 1px solid #444;
            border-radius: 8px;
            background: rgba(0,0,0,0.3);
            color: #fff;
            font-size: 14px;
        }}
        input:focus {{
            outline: none;
            border-color: #4a9eff;
        }}
        .buttons {{
            display: flex;
            gap: 10px;
        }}
        button {{
            flex: 1;
            padding: 14px 20px;
            border: none;
            border-radius: 8px;
            font-size: 16px;
            font-weight: 600;
            cursor: pointer;
            transition: all 0.2s;
        }}
        button:disabled {{
            opacity: 0.5;
            cursor: not-allowed;
        }}
        .connect-btn {{
            background: linear-gradient(135deg, #4a9eff 0%, #2d7dd2 100%);
            color: white;
        }}
        .connect-btn:hover:not(:disabled) {{
            transform: translateY(-2px);
            box-shadow: 0 4px 12px rgba(74, 158, 255, 0.4);
        }}
        .disconnect-btn {{
            background: linear-gradient(135deg, #ff4a4a 0%, #d22d2d 100%);
            color: white;
        }}
        .disconnect-btn:hover:not(:disabled) {{
            transform: translateY(-2px);
            box-shadow: 0 4px 12px rgba(255, 74, 74, 0.4);
        }}
        .status {{
            margin-top: 20px;
            padding: 15px;
            border-radius: 8px;
            background: rgba(0,0,0,0.2);
            font-size: 14px;
        }}
        .status.connected {{
            border-left: 4px solid #4ade80;
        }}
        .status.connecting {{
            border-left: 4px solid #facc15;
        }}
        .status.error {{
            border-left: 4px solid #f87171;
        }}
        .status.disconnected {{
            border-left: 4px solid #6b7280;
        }}
        .audio-indicator {{
            display: flex;
            align-items: center;
            justify-content: center;
            gap: 4px;
            margin-top: 20px;
            height: 40px;
        }}
        .audio-bar {{
            width: 6px;
            height: 20px;
            background: #4a9eff;
            border-radius: 3px;
            animation: audio-wave 0.5s ease-in-out infinite;
        }}
        .audio-bar:nth-child(1) {{ animation-delay: 0s; }}
        .audio-bar:nth-child(2) {{ animation-delay: 0.1s; }}
        .audio-bar:nth-child(3) {{ animation-delay: 0.2s; }}
        .audio-bar:nth-child(4) {{ animation-delay: 0.3s; }}
        .audio-bar:nth-child(5) {{ animation-delay: 0.4s; }}
        @keyframes audio-wave {{
            0%, 100% {{ height: 10px; }}
            50% {{ height: 30px; }}
        }}
        .audio-indicator.inactive .audio-bar {{
            animation: none;
            height: 10px;
            background: #6b7280;
        }}
        .log {{
            margin-top: 20px;
            padding: 15px;
            border-radius: 8px;
            background: rgba(0,0,0,0.3);
            font-family: monospace;
            font-size: 12px;
            max-height: 200px;
            overflow-y: auto;
        }}
        .log-entry {{
            margin: 4px 0;
            color: #888;
        }}
        .log-entry.error {{
            color: #f87171;
        }}
        .log-entry.success {{
            color: #4ade80;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>WHEP Audio Player</h1>

        <div class="form-group">
            <label for="endpoint">WHEP Endpoint URL</label>
            <input type="text" id="endpoint" placeholder="http://localhost:8190" value="{endpoint}">
        </div>

        <div class="buttons">
            <button class="connect-btn" id="connectBtn" onclick="connect()">Connect</button>
            <button class="disconnect-btn" id="disconnectBtn" onclick="disconnect()" disabled>Disconnect</button>
        </div>

        <div class="audio-indicator inactive" id="audioIndicator">
            <div class="audio-bar"></div>
            <div class="audio-bar"></div>
            <div class="audio-bar"></div>
            <div class="audio-bar"></div>
            <div class="audio-bar"></div>
        </div>

        <div class="status disconnected" id="status">Not connected</div>

        <div class="log" id="log"></div>
    </div>

    <audio id="audio" autoplay></audio>

    <script>
        let peerConnection = null;
        let resourceUrl = null;

        function log(message, type = '') {{
            const logEl = document.getElementById('log');
            const entry = document.createElement('div');
            entry.className = 'log-entry ' + type;
            entry.textContent = new Date().toLocaleTimeString() + ' - ' + message;
            logEl.appendChild(entry);
            logEl.scrollTop = logEl.scrollHeight;
        }}

        function setStatus(message, state) {{
            const statusEl = document.getElementById('status');
            statusEl.textContent = message;
            statusEl.className = 'status ' + state;
        }}

        function setAudioActive(active) {{
            const indicator = document.getElementById('audioIndicator');
            if (active) {{
                indicator.classList.remove('inactive');
            }} else {{
                indicator.classList.add('inactive');
            }}
        }}

        // Check if endpoint is a local (same-origin) or external URL
        function isLocalEndpoint(endpoint) {{
            return endpoint.startsWith('/api/whep/');
        }}

        // Get the URL to use for WHEP requests
        // Local endpoints (from strom's WHEP Output blocks) can be used directly
        // External endpoints need to go through the proxy to avoid CORS issues
        function getWhepUrl(whepEndpoint) {{
            if (isLocalEndpoint(whepEndpoint)) {{
                return whepEndpoint; // Same-origin, no proxy needed
            }}
            return '/api/whep-proxy?endpoint=' + encodeURIComponent(whepEndpoint);
        }}

        async function connect() {{
            const endpoint = document.getElementById('endpoint').value.trim();
            if (!endpoint) {{
                log('Please enter a WHEP endpoint URL', 'error');
                return;
            }}

            document.getElementById('connectBtn').disabled = true;
            setStatus('Connecting...', 'connecting');
            log('Connecting to ' + endpoint);

            try {{
                // Create peer connection
                peerConnection = new RTCPeerConnection({{
                    iceServers: [{{ urls: 'stun:stun.l.google.com:19302' }}]
                }});

                // Handle incoming tracks
                peerConnection.ontrack = (event) => {{
                    log('Received track: ' + event.track.kind, 'success');
                    if (event.track.kind === 'audio') {{
                        const audio = document.getElementById('audio');
                        audio.srcObject = event.streams[0];
                        setAudioActive(true);
                    }}
                }};

                // Log ICE connection state changes
                peerConnection.oniceconnectionstatechange = () => {{
                    log('ICE state: ' + peerConnection.iceConnectionState);
                    if (peerConnection.iceConnectionState === 'connected') {{
                        setStatus('Connected - Playing audio', 'connected');
                    }} else if (peerConnection.iceConnectionState === 'failed') {{
                        setStatus('Connection failed', 'error');
                        setAudioActive(false);
                    }} else if (peerConnection.iceConnectionState === 'disconnected') {{
                        setStatus('Disconnected', 'disconnected');
                        setAudioActive(false);
                    }}
                }};

                // Add transceiver for receiving audio
                peerConnection.addTransceiver('audio', {{ direction: 'recvonly' }});

                // Create offer
                const offer = await peerConnection.createOffer();
                await peerConnection.setLocalDescription(offer);
                log('Created SDP offer');

                // Wait for ICE gathering to complete (or timeout)
                await new Promise((resolve) => {{
                    if (peerConnection.iceGatheringState === 'complete') {{
                        resolve();
                    }} else {{
                        const timeout = setTimeout(resolve, 2000);
                        peerConnection.onicegatheringstatechange = () => {{
                            if (peerConnection.iceGatheringState === 'complete') {{
                                clearTimeout(timeout);
                                resolve();
                            }}
                        }};
                    }}
                }});
                log('ICE gathering complete');

                // Send offer (via proxy for external endpoints, directly for local)
                const whepUrl = getWhepUrl(endpoint);
                log('Sending offer to ' + (isLocalEndpoint(endpoint) ? 'local' : 'proxied') + ' endpoint...');
                const response = await fetch(whepUrl, {{
                    method: 'POST',
                    headers: {{
                        'Content-Type': 'application/sdp',
                    }},
                    body: peerConnection.localDescription.sdp,
                }});

                if (!response.ok) {{
                    const errorText = await response.text();
                    throw new Error('WHEP request failed: ' + response.status + ' ' + (errorText || response.statusText));
                }}

                // Store resource URL for DELETE on disconnect (already proxied)
                resourceUrl = response.headers.get('Location');
                log('Resource URL: ' + (resourceUrl || 'none'));

                // Get answer
                const answerSdp = await response.text();
                log('Received SDP answer', 'success');

                // Set remote description
                await peerConnection.setRemoteDescription({{
                    type: 'answer',
                    sdp: answerSdp,
                }});
                log('Set remote description');

                document.getElementById('disconnectBtn').disabled = false;
                setStatus('Connected - Waiting for audio...', 'connected');

            }} catch (error) {{
                log('Error: ' + error.message, 'error');
                setStatus('Connection failed: ' + error.message, 'error');
                document.getElementById('connectBtn').disabled = false;
                if (peerConnection) {{
                    peerConnection.close();
                    peerConnection = null;
                }}
            }}
        }}

        async function disconnect() {{
            log('Disconnecting...');

            // Send DELETE to resource URL if we have one (already goes through proxy)
            if (resourceUrl) {{
                try {{
                    await fetch(resourceUrl, {{ method: 'DELETE' }});
                    log('Sent DELETE to resource URL');
                }} catch (e) {{
                    log('Failed to DELETE resource: ' + e.message, 'error');
                }}
            }}

            if (peerConnection) {{
                peerConnection.close();
                peerConnection = null;
            }}

            resourceUrl = null;
            setAudioActive(false);
            setStatus('Disconnected', 'disconnected');
            document.getElementById('connectBtn').disabled = false;
            document.getElementById('disconnectBtn').disabled = true;

            const audio = document.getElementById('audio');
            audio.srcObject = null;

            log('Disconnected', 'success');
        }}

        // Auto-connect if endpoint is provided
        window.onload = () => {{
            const endpoint = document.getElementById('endpoint').value;
            if (endpoint) {{
                // Small delay to let page render
                setTimeout(connect, 500);
            }}
        }};
    </script>
</body>
</html>
"##
    );

    Html(html)
}

// ============================================================================
// New endpoint_id-based WHEP proxy (uses WhepRegistry)
// ============================================================================

/// Proxy POST requests to /api/whep/{endpoint_id}/endpoint
/// Looks up the internal port from WhepRegistry and forwards to localhost:{port}/whep/endpoint
pub async fn whep_endpoint_proxy(
    State(state): State<AppState>,
    Path(endpoint_id): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Response {
    // Look up internal port from registry
    let port = match state.whep_registry().get_port(&endpoint_id).await {
        Some(p) => p,
        None => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Body::from(format!(
                    "WHEP endpoint '{}' not found",
                    endpoint_id
                )))
                .unwrap();
        }
    };

    let target_url = format!("http://127.0.0.1:{}/whep/endpoint", port);
    let client = reqwest::Client::new();

    let mut request = client.post(&target_url);

    // Forward content-type header
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        if let Ok(ct) = content_type.to_str() {
            request = request.header(header::CONTENT_TYPE, ct);
        }
    }

    request = request.body(body);

    match request.send().await {
        Ok(response) => {
            let status = response.status();

            // Get Location header for resource URL and rewrite it
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            let body_bytes = response.bytes().await.unwrap_or_default();

            let mut builder = Response::builder()
                .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK))
                .header(header::CONTENT_TYPE, "application/sdp")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .header(
                    header::ACCESS_CONTROL_ALLOW_METHODS,
                    "POST, DELETE, OPTIONS",
                )
                .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
                .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Location");

            if let Some(loc) = location {
                // Rewrite location from /whep/resource/{id} to /api/whep/{endpoint_id}/resource/{id}
                // The original location is like "/whep/resource/abc123"
                let proxy_location = if loc.starts_with("/whep/resource/") {
                    let resource_id = loc.trim_start_matches("/whep/resource/");
                    format!("/api/whep/{}/resource/{}", endpoint_id, resource_id)
                } else {
                    // Fallback: just prefix with our endpoint path
                    format!("/api/whep/{}{}", endpoint_id, loc)
                };
                builder = builder.header(header::LOCATION, proxy_location);
            }

            builder.body(Body::from(body_bytes)).unwrap()
        }
        Err(e) => Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(Body::from(format!("Proxy error: {}", e)))
            .unwrap(),
    }
}

/// Proxy DELETE requests to /api/whep/{endpoint_id}/resource/{resource_id}
/// Forwards to localhost:{port}/whep/resource/{resource_id}
pub async fn whep_resource_proxy_delete(
    State(state): State<AppState>,
    Path((endpoint_id, resource_id)): Path<(String, String)>,
) -> Response {
    // Look up internal port from registry
    let port = match state.whep_registry().get_port(&endpoint_id).await {
        Some(p) => p,
        None => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Body::from(format!(
                    "WHEP endpoint '{}' not found",
                    endpoint_id
                )))
                .unwrap();
        }
    };

    let target_url = format!("http://127.0.0.1:{}/whep/resource/{}", port, resource_id);
    let client = reqwest::Client::new();

    match client.delete(&target_url).send().await {
        Ok(response) => {
            let status = response.status();
            Response::builder()
                .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK))
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Body::empty())
                .unwrap()
        }
        Err(e) => Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(Body::from(format!("Proxy error: {}", e)))
            .unwrap(),
    }
}

/// Handle OPTIONS preflight for /api/whep/{endpoint_id}/endpoint
pub async fn whep_endpoint_proxy_options() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "POST, OPTIONS")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
        .header(header::ACCESS_CONTROL_MAX_AGE, "86400")
        .body(Body::empty())
        .unwrap()
}

/// Handle OPTIONS preflight for /api/whep/{endpoint_id}/resource/{resource_id}
pub async fn whep_resource_proxy_options() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "DELETE, OPTIONS")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
        .header(header::ACCESS_CONTROL_MAX_AGE, "86400")
        .body(Body::empty())
        .unwrap()
}
