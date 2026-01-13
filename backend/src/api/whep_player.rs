//! WHEP Player - serves web pages and static assets for playing WHEP streams.
//!
//! URL Structure:
//! - `/player/whep` - HTML page for playing a single WHEP stream
//! - `/player/whep-streams` - HTML page listing all active WHEP streams
//! - `/static/whep.css` - Shared CSS styles
//! - `/static/whep.js` - Shared JavaScript for WebRTC connections
//! - `/whep/{endpoint_id}` - Proxy to internal WHEP servers
//! - `/api/whep-streams` - JSON API listing all active WHEP endpoints

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;

use crate::state::AppState;

// ============================================================================
// Shared Static Assets
// ============================================================================

/// Shared CSS styles for WHEP player pages (egui-inspired dark theme)
pub async fn whep_css() -> impl IntoResponse {
    let css = r#"* {
    box-sizing: border-box;
}
body {
    font-family: monospace, 'Courier New', Courier;
    background: #1b1b1b;
    color: #b4b4b4;
    min-height: 100vh;
    margin: 0;
    padding: 20px;
}
h1 {
    margin: 0 0 16px 0;
    font-size: 18px;
    font-weight: normal;
    color: #e0e0e0;
}
.container {
    max-width: 800px;
    width: 100%;
    background: #2b2b2b;
    border: 1px solid #3d3d3d;
    border-radius: 2px;
    padding: 24px;
    margin: 0 auto;
}
.form-group {
    margin-bottom: 16px;
}
label {
    display: block;
    margin-bottom: 6px;
    font-size: 13px;
    color: #888;
}
input {
    width: 100%;
    padding: 8px 10px;
    border: 1px solid #3d3d3d;
    border-radius: 2px;
    background: #1b1b1b;
    color: #e0e0e0;
    font-family: monospace;
    font-size: 13px;
}
input:focus {
    outline: none;
    border-color: #5588cc;
}
button {
    padding: 8px 16px;
    border: 1px solid #3d3d3d;
    border-radius: 2px;
    font-family: monospace;
    font-size: 13px;
    cursor: pointer;
    background: #3d3d3d;
    color: #e0e0e0;
}
button:hover:not(:disabled) {
    background: #4a4a4a;
}
button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
}
.connect-btn {
    background: #2d5a8a;
    border-color: #3d6a9a;
}
.connect-btn:hover:not(:disabled) {
    background: #3d6a9a;
}
.disconnect-btn {
    background: #8a3d3d;
    border-color: #9a4d4d;
}
.disconnect-btn:hover:not(:disabled) {
    background: #9a4d4d;
}
.open-btn {
    background: #4a5a4a;
    border-color: #5a6a5a;
}
.open-btn:hover:not(:disabled) {
    background: #5a6a5a;
}
.status {
    padding: 10px 12px;
    border-radius: 2px;
    background: #252525;
    border: 1px solid #3d3d3d;
    font-size: 13px;
}
.status.connected {
    border-left: 3px solid #5a8a5a;
}
.status.connecting {
    border-left: 3px solid #8a8a5a;
}
.status.error {
    border-left: 3px solid #8a5a5a;
}
.status.disconnected {
    border-left: 3px solid #5a5a5a;
}
.video-container {
    width: 100%;
    aspect-ratio: 16/9;
    background: #000;
    border: 1px solid #3d3d3d;
    border-radius: 2px;
    overflow: hidden;
    display: none;
}
.video-container.active {
    display: block;
}
.video-container video {
    width: 100%;
    height: 100%;
    object-fit: contain;
}
.audio-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
    height: 32px;
}
.audio-bar {
    width: 4px;
    height: 16px;
    background: #5588cc;
    border-radius: 1px;
    animation: audio-wave 0.5s ease-in-out infinite;
}
.audio-bar:nth-child(1) { animation-delay: 0s; }
.audio-bar:nth-child(2) { animation-delay: 0.1s; }
.audio-bar:nth-child(3) { animation-delay: 0.2s; }
.audio-bar:nth-child(4) { animation-delay: 0.3s; }
.audio-bar:nth-child(5) { animation-delay: 0.4s; }
@keyframes audio-wave {
    0%, 100% { height: 8px; }
    50% { height: 24px; }
}
.audio-indicator.inactive .audio-bar {
    animation: none;
    height: 8px;
    background: #4a4a4a;
}
.log {
    margin-top: 16px;
    padding: 10px 12px;
    border-radius: 2px;
    background: #1b1b1b;
    border: 1px solid #3d3d3d;
    font-family: monospace;
    font-size: 11px;
    max-height: 180px;
    overflow-y: auto;
}
.log-entry {
    margin: 3px 0;
    color: #777;
}
.log-entry.error {
    color: #cc7777;
}
.log-entry.success {
    color: #77aa77;
}

/* Streams page specific styles */
.header {
    text-align: center;
    margin-bottom: 24px;
}
.subtitle {
    color: #777;
    font-size: 12px;
}
.streams-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 16px;
    max-width: 1400px;
    margin: 0 auto;
}
.stream-card {
    background: #2b2b2b;
    border: 1px solid #3d3d3d;
    border-radius: 2px;
    overflow: hidden;
}
.stream-card:hover {
    border-color: #4a4a4a;
}
.stream-header {
    padding: 12px;
    background: #252525;
    border-bottom: 1px solid #3d3d3d;
    display: flex;
    justify-content: space-between;
    align-items: center;
}
.stream-id {
    font-size: 12px;
    word-break: break-all;
    color: #e0e0e0;
}
.stream-mode {
    font-size: 11px;
    padding: 3px 6px;
    border-radius: 2px;
    background: #2d5a8a;
    color: #b4c8e0;
}
.stream-content {
    padding: 12px;
}
.stream-status {
    font-size: 11px;
    margin-bottom: 12px;
}
.stream-actions {
    display: flex;
    gap: 6px;
}
.stream-actions button {
    flex: 1;
    padding: 6px 10px;
    font-size: 12px;
}
.no-streams {
    text-align: center;
    padding: 48px 16px;
    color: #777;
}
.no-streams-icon {
    font-size: 32px;
    margin-bottom: 12px;
    opacity: 0.6;
}
.refresh-btn {
    position: fixed;
    bottom: 16px;
    right: 16px;
}
"#;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/css")
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::from(css))
        .unwrap()
}

/// Shared JavaScript for WHEP WebRTC connections
pub async fn whep_js() -> impl IntoResponse {
    let js = r#"// WHEP Connection Library

class WhepConnection {
    constructor(endpoint, callbacks = {}) {
        this.endpoint = endpoint;
        this.callbacks = callbacks;
        this.peerConnection = null;
        this.resourceUrl = null;
        this.hasAudio = false;
        this.hasVideo = false;
    }

    async connect() {
        this.hasAudio = false;
        this.hasVideo = false;

        try {
            this.peerConnection = new RTCPeerConnection({
                iceServers: [{ urls: 'stun:stun.l.google.com:19302' }]
            });

            this.peerConnection.ontrack = (event) => {
                if (event.track.kind === 'audio') {
                    this.hasAudio = true;
                    if (this.callbacks.onAudioTrack) {
                        this.callbacks.onAudioTrack(event.streams[0]);
                    }
                } else if (event.track.kind === 'video') {
                    this.hasVideo = true;
                    if (this.callbacks.onVideoTrack) {
                        this.callbacks.onVideoTrack(event.streams[0]);
                    }
                }
                this._updateStatus();
            };

            this.peerConnection.oniceconnectionstatechange = () => {
                const state = this.peerConnection.iceConnectionState;
                if (this.callbacks.onIceState) {
                    this.callbacks.onIceState(state);
                }
                if (state === 'connected') {
                    this._updateStatus();
                } else if (state === 'failed') {
                    if (this.callbacks.onError) {
                        this.callbacks.onError('Connection failed');
                    }
                } else if (state === 'disconnected') {
                    if (this.callbacks.onDisconnected) {
                        this.callbacks.onDisconnected();
                    }
                }
            };

            this.peerConnection.addTransceiver('audio', { direction: 'recvonly' });
            this.peerConnection.addTransceiver('video', { direction: 'recvonly' });

            const offer = await this.peerConnection.createOffer();
            await this.peerConnection.setLocalDescription(offer);

            if (this.callbacks.onLog) {
                this.callbacks.onLog('Created SDP offer');
            }

            // Wait for ICE gathering
            await new Promise((resolve) => {
                if (this.peerConnection.iceGatheringState === 'complete') {
                    resolve();
                } else {
                    const timeout = setTimeout(resolve, 2000);
                    this.peerConnection.onicegatheringstatechange = () => {
                        if (this.peerConnection.iceGatheringState === 'complete') {
                            clearTimeout(timeout);
                            resolve();
                        }
                    };
                }
            });

            if (this.callbacks.onLog) {
                this.callbacks.onLog('ICE gathering complete');
            }

            const response = await fetch(this.endpoint, {
                method: 'POST',
                headers: { 'Content-Type': 'application/sdp' },
                body: this.peerConnection.localDescription.sdp,
            });

            if (!response.ok) {
                const errorText = await response.text();
                throw new Error('WHEP request failed: ' + response.status + ' ' + (errorText || response.statusText));
            }

            this.resourceUrl = response.headers.get('Location');
            const answerSdp = await response.text();

            if (this.callbacks.onLog) {
                this.callbacks.onLog('Received SDP answer', 'success');
            }

            await this.peerConnection.setRemoteDescription({
                type: 'answer',
                sdp: answerSdp,
            });

            if (this.callbacks.onConnected) {
                this.callbacks.onConnected();
            }

            return true;
        } catch (error) {
            if (this.callbacks.onError) {
                this.callbacks.onError(error.message);
            }
            this.close();
            return false;
        }
    }

    _updateStatus() {
        if (this.peerConnection && this.peerConnection.iceConnectionState === 'connected') {
            if (this.callbacks.onMediaStatus) {
                this.callbacks.onMediaStatus(this.hasAudio, this.hasVideo);
            }
        }
    }

    async disconnect() {
        if (this.resourceUrl) {
            try {
                await fetch(this.resourceUrl, { method: 'DELETE' });
            } catch (e) {
                console.error('Failed to DELETE resource:', e);
            }
        }
        this.close();
        if (this.callbacks.onDisconnected) {
            this.callbacks.onDisconnected();
        }
    }

    close() {
        if (this.peerConnection) {
            this.peerConnection.close();
            this.peerConnection = null;
        }
        this.resourceUrl = null;
        this.hasAudio = false;
        this.hasVideo = false;
    }

    isConnected() {
        return this.peerConnection && this.peerConnection.iceConnectionState === 'connected';
    }
}

// UI Helper functions
function setElementClass(id, className, condition) {
    const el = document.getElementById(id);
    if (el) {
        if (condition) {
            el.classList.add(className);
        } else {
            el.classList.remove(className);
        }
    }
}

function setStatus(elementId, message, state) {
    const el = document.getElementById(elementId);
    if (el) {
        el.textContent = message;
        el.className = 'status ' + state;
    }
}

function createAudioIndicator() {
    return `
        <div class="audio-bar"></div>
        <div class="audio-bar"></div>
        <div class="audio-bar"></div>
        <div class="audio-bar"></div>
        <div class="audio-bar"></div>
    `;
}

function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}
"#;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/javascript")
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::from(js))
        .unwrap()
}

// ============================================================================
// Player Pages
// ============================================================================

#[derive(Deserialize)]
pub struct WhepPlayerQuery {
    /// The WHEP endpoint URL to connect to (e.g., /whep/my-stream)
    endpoint: Option<String>,
}

/// Serve the WHEP player HTML page.
/// GET /player/whep?endpoint=/whep/my-stream
pub async fn whep_player(Query(params): Query<WhepPlayerQuery>) -> impl IntoResponse {
    let endpoint = params.endpoint.unwrap_or_default();

    let html = format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>WHEP Player - Strom</title>
    <link rel="stylesheet" href="/static/whep.css">
</head>
<body style="display: flex; flex-direction: column; align-items: center;">
    <div class="container">
        <h1 style="text-align: center;">WHEP Player</h1>

        <div class="form-group">
            <label for="endpoint">WHEP Endpoint URL</label>
            <input type="text" id="endpoint" placeholder="/whep/my-stream" value="{endpoint}">
        </div>

        <div style="display: flex; gap: 8px; margin-bottom: 16px;">
            <button class="connect-btn" id="connectBtn" style="flex: 1;" onclick="doConnect()">Connect</button>
            <button class="disconnect-btn" id="disconnectBtn" style="flex: 1;" onclick="doDisconnect()" disabled>Disconnect</button>
        </div>

        <div style="display: flex; flex-direction: column; align-items: center; gap: 12px;">
            <div class="video-container" id="videoContainer" style="max-width: 720px;">
                <video id="video" autoplay muted playsinline controls></video>
            </div>
            <div class="audio-indicator inactive" id="audioIndicator">
                <div class="audio-bar"></div>
                <div class="audio-bar"></div>
                <div class="audio-bar"></div>
                <div class="audio-bar"></div>
                <div class="audio-bar"></div>
            </div>
        </div>

        <div class="status disconnected" id="status" style="margin-top: 16px;">Not connected</div>

        <div class="log" id="log"></div>
    </div>

    <audio id="audio" autoplay></audio>

    <script src="/static/whep.js"></script>
    <script>
        let connection = null;

        function log(message, type = '') {{
            const logEl = document.getElementById('log');
            const entry = document.createElement('div');
            entry.className = 'log-entry ' + type;
            entry.textContent = new Date().toLocaleTimeString() + ' - ' + message;
            logEl.appendChild(entry);
            logEl.scrollTop = logEl.scrollHeight;
        }}

        function doConnect() {{
            const endpoint = document.getElementById('endpoint').value.trim();
            if (!endpoint) {{
                log('Please enter a WHEP endpoint URL', 'error');
                return;
            }}

            document.getElementById('connectBtn').disabled = true;
            setStatus('status', 'Connecting...', 'connecting');
            log('Connecting to ' + endpoint);

            connection = new WhepConnection(endpoint, {{
                onAudioTrack: (stream) => {{
                    document.getElementById('audio').srcObject = stream;
                    setElementClass('audioIndicator', 'inactive', false);
                }},
                onVideoTrack: (stream) => {{
                    document.getElementById('video').srcObject = stream;
                    document.getElementById('video').muted = false;
                    setElementClass('videoContainer', 'active', true);
                }},
                onConnected: () => {{
                    document.getElementById('disconnectBtn').disabled = false;
                    setStatus('status', 'Connected - Waiting for media...', 'connected');
                }},
                onMediaStatus: (hasAudio, hasVideo) => {{
                    let mediaTypes = [];
                    if (hasAudio) mediaTypes.push('audio');
                    if (hasVideo) mediaTypes.push('video');
                    if (mediaTypes.length > 0) {{
                        setStatus('status', 'Connected - Playing ' + mediaTypes.join(' + '), 'connected');
                    }}
                }},
                onError: (msg) => {{
                    log('Error: ' + msg, 'error');
                    setStatus('status', 'Connection failed: ' + msg, 'error');
                    document.getElementById('connectBtn').disabled = false;
                }},
                onDisconnected: () => {{
                    setStatus('status', 'Disconnected', 'disconnected');
                    setElementClass('audioIndicator', 'inactive', true);
                    setElementClass('videoContainer', 'active', false);
                    document.getElementById('audio').srcObject = null;
                    document.getElementById('video').srcObject = null;
                    document.getElementById('connectBtn').disabled = false;
                    document.getElementById('disconnectBtn').disabled = true;
                }},
                onIceState: (state) => {{
                    log('ICE state: ' + state);
                }},
                onLog: (msg, type) => {{
                    log(msg, type || '');
                }}
            }});

            connection.connect();
        }}

        function doDisconnect() {{
            log('Disconnecting...');
            if (connection) {{
                connection.disconnect();
                connection = null;
            }}
            log('Disconnected', 'success');
        }}

        // Auto-connect if endpoint is provided
        window.onload = () => {{
            const endpoint = document.getElementById('endpoint').value;
            if (endpoint) {{
                setTimeout(doConnect, 500);
            }}
        }};
    </script>
</body>
</html>
"##
    );

    Html(html)
}

/// Serve the WHEP streams page HTML.
/// GET /player/whep-streams
pub async fn whep_streams_page() -> impl IntoResponse {
    let html = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>WHEP Streams - Strom</title>
    <link rel="stylesheet" href="/static/whep.css">
</head>
<body>
    <div class="header">
        <h1>WHEP Streams</h1>
        <div class="subtitle">Active streams from Strom flows</div>
    </div>

    <div class="streams-grid" id="streamsGrid">
        <div class="no-streams" id="noStreams">
            <div class="no-streams-icon">--</div>
            <div>No active WHEP streams</div>
            <div style="margin-top: 8px; font-size: 11px;">Start a flow with a WHEP Output block to see streams here</div>
        </div>
    </div>

    <button class="refresh-btn" onclick="loadStreams()">Refresh</button>

    <script src="/static/whep.js"></script>
    <script>
        const streamConnections = new Map();

        async function loadStreams() {
            try {
                const response = await fetch('/api/whep-streams');
                const data = await response.json();
                renderStreams(data.streams);
            } catch (error) {
                console.error('Failed to load streams:', error);
            }
        }

        function renderStreams(streams) {
            const grid = document.getElementById('streamsGrid');
            const noStreams = document.getElementById('noStreams');

            // Clean up connections for streams that no longer exist
            for (const [id, conn] of streamConnections) {
                if (!streams.find(s => s.endpoint_id === id)) {
                    conn.close();
                    streamConnections.delete(id);
                }
            }

            if (streams.length === 0) {
                noStreams.style.display = 'block';
                const cards = grid.querySelectorAll('.stream-card');
                cards.forEach(card => card.remove());
                return;
            }

            noStreams.style.display = 'none';

            streams.forEach(stream => {
                let card = document.getElementById('card-' + stream.endpoint_id);
                if (!card) {
                    card = createStreamCard(stream);
                    grid.appendChild(card);
                }
            });

            const cards = grid.querySelectorAll('.stream-card');
            cards.forEach(card => {
                const id = card.id.replace('card-', '');
                if (!streams.find(s => s.endpoint_id === id)) {
                    card.remove();
                }
            });
        }

        function createStreamCard(stream) {
            const card = document.createElement('div');
            card.className = 'stream-card';
            card.id = 'card-' + stream.endpoint_id;

            const modeLabel = stream.mode === 'audio_video' ? 'Audio + Video' :
                             stream.mode === 'video' ? 'Video' : 'Audio';

            card.innerHTML = `
                <div class="stream-header">
                    <div class="stream-id">${escapeHtml(stream.endpoint_id)}</div>
                    <div class="stream-mode">${modeLabel}</div>
                </div>
                <div class="stream-content">
                    <div class="video-container" id="video-${stream.endpoint_id}">
                        <video autoplay muted playsinline controls></video>
                    </div>
                    <div class="audio-indicator inactive" id="audio-${stream.endpoint_id}" style="margin-bottom: 12px;">
                        ${createAudioIndicator()}
                    </div>
                    <div class="status disconnected stream-status" id="status-${stream.endpoint_id}">Not connected</div>
                    <div class="stream-actions">
                        <button class="connect-btn" id="connect-${stream.endpoint_id}" onclick="connectStream('${stream.endpoint_id}')">Play</button>
                        <button class="disconnect-btn" id="disconnect-${stream.endpoint_id}" onclick="disconnectStream('${stream.endpoint_id}')" disabled>Stop</button>
                        <button class="open-btn" onclick="openPlayer('${stream.endpoint_id}')">Open</button>
                    </div>
                </div>
                <audio id="audio-elem-${stream.endpoint_id}" autoplay></audio>
            `;

            return card;
        }

        function openPlayer(endpointId) {
            const url = '/player/whep?endpoint=' + encodeURIComponent('/whep/' + endpointId);
            window.open(url, '_blank');
        }

        function connectStream(endpointId) {
            const connectBtn = document.getElementById('connect-' + endpointId);
            const disconnectBtn = document.getElementById('disconnect-' + endpointId);

            connectBtn.disabled = true;
            setStatus('status-' + endpointId, 'Connecting...', 'connecting');

            const connection = new WhepConnection('/whep/' + endpointId, {
                onAudioTrack: (stream) => {
                    document.getElementById('audio-elem-' + endpointId).srcObject = stream;
                    setElementClass('audio-' + endpointId, 'inactive', false);
                },
                onVideoTrack: (stream) => {
                    const container = document.getElementById('video-' + endpointId);
                    container.querySelector('video').srcObject = stream;
                    setElementClass('video-' + endpointId, 'active', true);
                },
                onConnected: () => {
                    disconnectBtn.disabled = false;
                    setStatus('status-' + endpointId, 'Connected - Waiting...', 'connected');
                },
                onMediaStatus: (hasAudio, hasVideo) => {
                    let mediaTypes = [];
                    if (hasAudio) mediaTypes.push('audio');
                    if (hasVideo) mediaTypes.push('video');
                    if (mediaTypes.length > 0) {
                        setStatus('status-' + endpointId, 'Playing ' + mediaTypes.join(' + '), 'connected');
                    }
                },
                onError: (msg) => {
                    setStatus('status-' + endpointId, 'Error: ' + msg, 'error');
                    connectBtn.disabled = false;
                },
                onDisconnected: () => {
                    setStatus('status-' + endpointId, 'Disconnected', 'disconnected');
                    setElementClass('audio-' + endpointId, 'inactive', true);
                    setElementClass('video-' + endpointId, 'active', false);
                    document.getElementById('audio-elem-' + endpointId).srcObject = null;
                    const container = document.getElementById('video-' + endpointId);
                    if (container) container.querySelector('video').srcObject = null;
                    connectBtn.disabled = false;
                    disconnectBtn.disabled = true;
                }
            });

            streamConnections.set(endpointId, connection);
            connection.connect();
        }

        function disconnectStream(endpointId) {
            const conn = streamConnections.get(endpointId);
            if (conn) {
                conn.disconnect();
                streamConnections.delete(endpointId);
            }
        }

        // Initial load
        loadStreams();

        // Auto-refresh every 5 seconds
        setInterval(loadStreams, 5000);
    </script>
</body>
</html>
"##;

    Html(html)
}

// ============================================================================
// WHEP Proxy (endpoint_id-based routing via WhepRegistry)
// ============================================================================

/// Proxy POST requests to /whep/{endpoint_id}
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
                // Rewrite location from /whep/resource/{id} to /whep/{endpoint_id}/resource/{id}
                let proxy_location = if loc.starts_with("/whep/resource/") {
                    let resource_id = loc.trim_start_matches("/whep/resource/");
                    format!("/whep/{}/resource/{}", endpoint_id, resource_id)
                } else {
                    format!("/whep/{}{}", endpoint_id, loc)
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

/// Proxy DELETE requests to /whep/{endpoint_id}/resource/{resource_id}
pub async fn whep_resource_proxy_delete(
    State(state): State<AppState>,
    Path((endpoint_id, resource_id)): Path<(String, String)>,
) -> Response {
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

/// Handle OPTIONS preflight for /whep/{endpoint_id}
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

/// Handle OPTIONS preflight for /whep/{endpoint_id}/resource/{resource_id}
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

// ============================================================================
// WHEP Streams API (JSON)
// ============================================================================

/// Response structure for a WHEP stream.
#[derive(serde::Serialize)]
pub struct WhepStreamInfo {
    pub endpoint_id: String,
    pub mode: String,
    pub has_audio: bool,
    pub has_video: bool,
}

/// Response structure for the streams list endpoint.
#[derive(serde::Serialize)]
pub struct WhepStreamsResponse {
    pub streams: Vec<WhepStreamInfo>,
}

/// GET /api/whep-streams - List all active WHEP streams (JSON API).
pub async fn list_whep_streams(State(state): State<AppState>) -> axum::Json<WhepStreamsResponse> {
    let endpoints = state.whep_registry().list_all().await;

    let streams = endpoints
        .into_iter()
        .map(|(endpoint_id, entry)| WhepStreamInfo {
            endpoint_id,
            mode: entry.mode.as_str().to_string(),
            has_audio: entry.mode.has_audio(),
            has_video: entry.mode.has_video(),
        })
        .collect();

    axum::Json(WhepStreamsResponse { streams })
}
