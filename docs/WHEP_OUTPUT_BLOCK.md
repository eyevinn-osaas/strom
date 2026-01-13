# WHEP Output Block - Design & Implementation

## Overview

The WHEP Output block (`builtin.whep_output`) serves audio and/or video streams via WebRTC using the WHEP (WebRTC-HTTP Egress Protocol) standard. It includes a built-in proxy system that provides stable external URLs and integrated web player pages for easy stream playback.

**Block ID**: `builtin.whep_output`
**Category**: Output
**Implementation**: `backend/src/blocks/builtin/whep.rs`

## Features

- **WebRTC Streaming**: Serves media via standard WHEP protocol
- **Multiple Stream Modes**: Audio-only, video-only, or audio+video
- **Stable External URLs**: Proxy system provides consistent `/whep/{endpoint_id}` URLs
- **Built-in Player Pages**: Web-based players at `/player/whep` and `/player/whep-streams`
- **Dynamic Port Allocation**: Internal whepserversink binds to ephemeral ports
- **H.264/H.265 Video Support**: With automatic caps normalization for WebRTC compatibility
- **Opus Audio**: Standard WebRTC audio codec

## Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         Strom Backend                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐     ┌──────────────┐     ┌────────────────┐  │
│  │  GStreamer   │     │    WHEP      │     │   Axum HTTP    │  │
│  │   Pipeline   │────▶│  Registry    │◀────│    Server      │  │
│  │              │     │              │     │                │  │
│  │ whepserversink     │  endpoint_id │     │ /whep/{id}     │  │
│  │ (127.0.0.1:N)│     │  → port map  │     │ /player/whep   │  │
│  └──────────────┘     └──────────────┘     └────────────────┘  │
│                                                    │            │
└────────────────────────────────────────────────────│────────────┘
                                                     │
                                              ┌──────▼──────┐
                                              │   Browser   │
                                              │  WebRTC     │
                                              │  Player     │
                                              └─────────────┘
```

### Block Structure

```
Audio Input ─────┐
                 ▼
         ┌───────────────┐     ┌──────────────┐     ┌────────────────┐
         │  audioconvert │────▶│  audioresample│────▶│   opusenc      │
         └───────────────┘     └──────────────┘     └───────┬────────┘
                                                            │
Video Input ─────┐                                          │
                 ▼                                          ▼
         ┌───────────────┐     ┌──────────────┐     ┌────────────────┐
         │   queue       │────▶│  capsfilter  │────▶│ whepserversink │
         └───────────────┘     │  (normalize) │     │ (127.0.0.1:N)  │
                               └──────────────┘     └────────────────┘
```

### WHEP Proxy System

The proxy system solves two problems:

1. **Stable URLs**: `whepserversink` binds to dynamic ports, but external clients need stable URLs
2. **CORS/Security**: External requests go through Axum, allowing proper CORS headers and future authentication

**Flow:**

```
Browser                    Axum                      whepserversink
   │                         │                            │
   │  POST /whep/my-stream   │                            │
   │────────────────────────▶│                            │
   │                         │  lookup("my-stream")       │
   │                         │───────────────────────────▶│
   │                         │  returns port 54321        │
   │                         │◀───────────────────────────│
   │                         │                            │
   │                         │  POST 127.0.0.1:54321/whep │
   │                         │───────────────────────────▶│
   │                         │  SDP answer                │
   │                         │◀───────────────────────────│
   │  SDP answer             │                            │
   │◀────────────────────────│                            │
   │                         │                            │
   │  ════════ WebRTC P2P Connection ══════════════════▶ │
```

### WhepRegistry

The `WhepRegistry` maintains a mapping of endpoint IDs to internal ports:

```rust
pub struct WhepRegistry {
    endpoints: Arc<RwLock<HashMap<String, WhepEndpointEntry>>>,
}

pub struct WhepEndpointEntry {
    pub port: u16,           // Internal port (e.g., 54321)
    pub mode: WhepStreamMode, // audio, video, or audio_video
}
```

**Lifecycle:**
1. Block registers endpoint when flow starts (`register()`)
2. Proxy looks up port for incoming requests (`get_port()`)
3. Block unregisters endpoint when flow stops (`unregister()`)

## Block Properties

### `endpoint_id` (String, Required)

Unique identifier for the WHEP endpoint. Used in the URL path.

**Example:** `my-stream` → accessible at `/whep/my-stream`

**Constraints:**
- Must be unique across all running flows
- URL-safe characters recommended

### `stun_server` (String, Optional)

STUN server URL for NAT traversal.

**Default:** `stun://stun.l.google.com:19302`

**Format:** `stun://hostname:port` or `turn://hostname:port`

### `auth_token` (String, Optional)

Bearer token for WHEP authentication (passed to whepserversink).

**Note:** Currently the external proxy endpoints are not authenticated. This token is for the internal whepserversink.

## Stream Modes

The block automatically detects the stream mode based on connected inputs:

| Mode | Audio Input | Video Input | Description |
|------|-------------|-------------|-------------|
| `audio` | Connected | Not connected | Audio-only stream |
| `video` | Not connected | Connected | Video-only stream |
| `audio_video` | Connected | Connected | Both audio and video |

## GStreamer Elements

### whepserversink

The core element from `gst-plugins-rs` that implements WHEP server functionality.

**Key Properties:**
- `stun-server`: STUN/TURN server URL
- `host`: Bind address (always `127.0.0.1` for security)
- `port`: HTTP port for WHEP signaling (dynamically allocated)

**Internal Elements:**
- `webrtcbin`: Handles WebRTC peer connections
- `rtpopuspay`: RTP payloader for Opus audio
- `rtph264pay`/`rtph265pay`: RTP payloader for video

### Caps Normalization

H.264/H.265 parsers add fields that can cause false renegotiation in webrtcsink. A pad probe normalizes caps:

```rust
// Fields removed from H.264 caps:
// - coded-picture-structure
// - chroma-format
// - bit-depth-luma
// - bit-depth-chroma
// - colorimetry
// - chroma-site
```

This prevents unnecessary renegotiation when upstream caps change slightly.

## Web Player Pages

### Player Page (`/player/whep`)

Single-stream player with:
- Endpoint URL input field
- Full WHEP URL display for external players
- Connect/Disconnect buttons
- Video display (for video streams)
- Audio indicator with volume/mute controls (for audio-only streams)
- Connection status and log

**Query Parameter:**
- `endpoint`: Pre-fill the WHEP endpoint (e.g., `/player/whep?endpoint=/whep/my-stream`)

### Streams Page (`/player/whep-streams`)

Multi-stream gallery showing all active WHEP endpoints:
- Auto-refreshes every 5 seconds
- Per-stream Play/Stop/Open buttons
- Inline video preview
- Volume controls for audio-only streams
- Copy URL button for each stream

### Links Page (Frontend)

The frontend includes a Links page accessible from the navigation menu that provides:
- Quick links to active WHEP player pages
- Direct URLs for integration with external players

## API Endpoints

### WHEP Proxy

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/whep/{endpoint_id}` | POST | WHEP signaling (create session) |
| `/whep/{endpoint_id}` | OPTIONS | CORS preflight |
| `/whep/{endpoint_id}/resource/{resource_id}` | DELETE | End session |
| `/whep/{endpoint_id}/resource/{resource_id}` | OPTIONS | CORS preflight |

### Player Pages

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/player/whep` | GET | Single stream player page |
| `/player/whep-streams` | GET | All streams gallery page |

### Static Assets

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/static/whep.css` | GET | Shared CSS styles |
| `/static/whep.js` | GET | WHEP connection library |

### JSON API

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/whep-streams` | GET | List active WHEP endpoints |

**Response:**
```json
{
  "streams": [
    {
      "endpoint_id": "my-stream",
      "mode": "audio_video",
      "has_audio": true,
      "has_video": true
    }
  ]
}
```

## Usage Examples

### Example 1: Audio-Only Stream

```json
{
  "block_type": "builtin.whep_output",
  "properties": {
    "endpoint_id": "radio-stream"
  }
}
```

Connect audio source to `audio_in` pad. Stream available at `/whep/radio-stream`.

### Example 2: Video Stream with Custom STUN

```json
{
  "block_type": "builtin.whep_output",
  "properties": {
    "endpoint_id": "camera-feed",
    "stun_server": "stun://stun.example.com:3478"
  }
}
```

### Example 3: Full Audio+Video Stream

Connect both `audio_in` and `video_in` pads for a complete media stream.

## Playing Streams

### Browser (Built-in Player)

Navigate to: `http://localhost:8080/player/whep?endpoint=/whep/my-stream`

### VLC

```bash
# Note: VLC's WHEP support may be limited
vlc "http://localhost:8080/whep/my-stream"
```

### GStreamer (whepsrc)

```bash
gst-launch-1.0 whepsrc uri="http://localhost:8080/whep/my-stream" ! decodebin ! autovideosink
```

### ffplay (via SDP)

WHEP doesn't directly support ffplay, but you can use the built-in web player or GStreamer.

## Security Considerations

### Current State

- `whepserversink` binds to `127.0.0.1` only (not externally accessible)
- All external access goes through Axum proxy
- CORS headers allow browser access from any origin

### Not Yet Implemented

- Authentication for WHEP proxy endpoints
- Rate limiting
- Per-endpoint access control

### Recommendations

For production use:
1. Run behind a reverse proxy (nginx, Caddy) with TLS
2. Implement network-level access control
3. Use non-guessable endpoint IDs (UUIDs)

## Troubleshooting

### "WHEP endpoint not found"

**Cause:** The flow with the WHEP Output block is not running.

**Solution:** Start the flow containing the WHEP Output block.

### Video not playing in browser

**Possible causes:**
1. Codec not supported (use H.264 constrained-baseline for maximum compatibility)
2. Caps negotiation issue

**Debug:**
```bash
# Check GStreamer logs
GST_DEBUG=webrtc*:5 cargo run
```

### Audio controls not showing

**Cause:** Audio controls only appear for audio-only streams. If video is present, the video element provides its own controls.

### Connection fails immediately

**Possible causes:**
1. STUN server unreachable
2. Firewall blocking UDP
3. NAT traversal failure

**Solution:** Try a different STUN server or check network configuration.

## Implementation Details

### Port Allocation

Dynamic port allocation uses TCP socket binding:

```rust
let listener = TcpListener::bind("127.0.0.1:0").await?;
let port = listener.local_addr()?.port();
drop(listener); // Free port for whepserversink
```

### BlockBuildContext

The WHEP block uses `BlockBuildContext` to register endpoints:

```rust
impl BlockBuilder for WhepOutputBuilder {
    fn build(&self, ctx: &mut BlockBuildContext, ...) -> Result<...> {
        // Register endpoint for proxy
        ctx.register_whep_endpoint(endpoint_id, port, mode);
        // ...
    }
}
```

The context collects registrations during build, and `AppState` processes them when the flow starts.

## Related Documentation

- [WHEP Protocol Spec](https://datatracker.ietf.org/doc/draft-ietf-wish-whep/)
- [GStreamer webrtcsink](https://gstreamer.freedesktop.org/documentation/rswebrtc/index.html)
- [Blocks Implementation Guide](BLOCKS_IMPLEMENTATION.md)
- [Video Encoder Block](VIDEO_ENCODER_BLOCK.md) - For encoding video before WHEP output

## Changelog

### v0.3.7

**Initial Implementation**
- WHEP Output block with audio/video support
- WHEP proxy system with WhepRegistry
- Built-in player pages (`/player/whep`, `/player/whep-streams`)
- Links page in frontend
- Volume/mute controls for audio-only streams
- H.264/H.265 caps normalization for WebRTC compatibility
