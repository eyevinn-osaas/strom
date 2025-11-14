# Strom - GStreamer Flow Engine

**Strom** (Swedish for "stream") is a full-stack Rust application that provides a visual, web-based interface for creating, managing, and executing GStreamer media pipelines. Think of it as a Swiss Army knife for GStreamer - a powerful engine to design and run complex media flows without writing code.

## Overview

Strom allows you to:

- **Visually design GStreamer pipelines** using a node-based flow editor in your browser
- **Configure element properties** with a user-friendly interface
- **Manage multiple flows** simultaneously, each running as an independent GStreamer pipeline
- **Persist configurations** so flows can be automatically restored on server restart
- **Monitor pipeline state** in real-time through Server-Sent Events (SSE)
- **Start/stop flows** on-demand or automatically at startup

## Architecture

Strom is built as a full-stack Rust application with three main components:

```
┌─────────────────────────────────────────────┐
│         Web Browser (Frontend)              │
│  ┌───────────────────────────────────────┐  │
│  │  egui (WebAssembly)                   │  │
│  │  - Visual flow editor                 │  │
│  │  - Element property inspector         │  │
│  │  - Pipeline state monitoring          │  │
│  └───────────────────────────────────────┘  │
└──────────────┬──────────────────────────────┘
               │
               │ REST API (CRUD operations)
               │ SSE (Real-time state updates)
               │
┌──────────────▼──────────────────────────────┐
│         Backend Server (Rust)               │
│  ┌───────────────────────────────────────┐  │
│  │  axum Web Framework                   │  │
│  │  - REST endpoints                     │  │
│  │  - SSE handler                        │  │
│  └───────────────────────────────────────┘  │
│  ┌───────────────────────────────────────┐  │
│  │  Flow Manager                         │  │
│  │  - Pipeline lifecycle management      │  │
│  │  - Element introspection              │  │
│  │  - State tracking                     │  │
│  └───────────────────────────────────────┘  │
│  ┌───────────────────────────────────────┐  │
│  │  GStreamer (gstreamer-rs)             │  │
│  │  - Pipeline execution                 │  │
│  │  - Element management                 │  │
│  └───────────────────────────────────────┘  │
│  ┌───────────────────────────────────────┐  │
│  │  Persistence Layer                    │  │
│  │  - JSON file storage (initial)        │  │
│  │  - Database support (future)          │  │
│  └───────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
                    │
                    │ depends on
                    ▼
         ┌──────────────────────┐
         │   Shared Types       │
         │   (Library Crate)    │
         │                      │
         │  - Domain models     │
         │  - API contracts     │
         │  - Serialization     │
         └──────────────────────┘
                    ▲
                    │ depends on
                    │
         (Both frontend and backend)
```

### Component Architecture

**Backend** - Server application that manages GStreamer pipelines
- Depends on: `strom-types` (shared types library)
- Responsibilities: API handling, pipeline execution, persistence

**Frontend** - WebAssembly UI compiled from Rust
- Depends on: `strom-types` (shared types library)
- Responsibilities: Visual editor, user interactions, API client

**Shared Types** - Library crate (`strom-types`)
- No dependencies on frontend or backend
- Provides: Domain models (Flow, Element, Link), API request/response types, common utilities
- Benefits: Type-safe API contracts, zero duplication, compile-time guarantees

### Technology Stack

#### Shared Types (`strom-types`)
- **serde**: Serialization/deserialization
- **uuid**: Unique identifiers for flows
- **chrono**: Timestamps (optional)

#### Backend (`strom-backend`)
- **gstreamer-rs**: Rust bindings for GStreamer
- **axum**: Modern, ergonomic web framework
- **tokio**: Async runtime for handling concurrent operations
- **serde_json**: JSON serialization for persistence
- **tower-http**: CORS, static file serving for frontend
- **utoipa**: OpenAPI documentation generation
- **strom-types**: Shared type definitions

#### Frontend (`strom-frontend`)
- **egui**: Immediate mode GUI framework compiled to WebAssembly
- **eframe**: Framework wrapper for egui with web support
- **Custom graph editor**: Node-based graph editor implementation
- **trunk**: Build tool for Rust WebAssembly applications
- **reqwest**: HTTP client for REST API (with WASM support)
- **gloo-net**: SSE client for real-time updates (WASM)
- **strom-types**: Shared type definitions

#### API Design

**REST API** (JSON over HTTP):
- `GET /api/flows` - List all flows
- `GET /api/flows/:id` - Get flow details
- `POST /api/flows` - Create new flow
- `POST /api/flows/:id` - Update flow configuration
- `DELETE /api/flows/:id` - Delete flow
- `POST /api/flows/:id/start` - Start a flow
- `POST /api/flows/:id/stop` - Stop a flow
- `GET /api/flows/:id/debug-graph` - Generate SVG visualization of pipeline
- `GET /api/elements` - List available GStreamer elements
- `GET /api/elements/:name` - Get element properties and capabilities

**Server-Sent Events (SSE)**:
- `GET /api/events` - Real-time event stream
  - Pipeline state changes (PLAYING, PAUSED, STOPPED, etc.)
  - Flow updates
  - System notifications

## Key Features

### Flow Management

Each **flow** represents a complete GStreamer pipeline with:
- **Unique ID** and name
- **Node graph** of GStreamer elements
- **Connections** between element pads
- **Property configurations** for each element
- **Auto-start flag** to launch on server startup
- **Current state** (NULL, READY, PAUSED, PLAYING, etc.)

### Visual Flow Editor

The web UI provides:
- **Drag-and-drop elements** from a palette
- **Visual linking** between compatible pads
- **Property inspector** with type-appropriate input widgets
- **Live state indication** (pipeline and element states)
- **Error display** with helpful messages
- **Debug graph visualization** - View running pipeline structure as interactive SVG

### Pipeline Debugging

Click the "🔍 Debug Graph" button in the UI to generate a visual representation of your running GStreamer pipeline. This feature:
- Generates a GraphViz DOT graph of the pipeline structure
- Converts to SVG for interactive viewing in browser
- Shows element connections, pad negotiations, and properties
- Requires Graphviz installed: `sudo apt install graphviz`

### Persistence

Flows are persisted to `flows.json` with the following structure:

```json
{
  "flows": [
    {
      "id": "uuid-v4",
      "name": "RTSP Camera Recorder",
      "auto_start": true,
      "elements": [
        {
          "id": "src1",
          "type": "rtspsrc",
          "properties": {
            "location": "rtsp://camera.local/stream"
          }
        },
        {
          "id": "sink1",
          "type": "filesink",
          "properties": {
            "location": "/recordings/output.mp4"
          }
        }
      ],
      "links": [
        {
          "from": "src1:src",
          "to": "sink1:sink"
        }
      ]
    }
  ]
}
```

On startup, the server:
1. Loads `flows.json`
2. Reconstructs GStreamer pipelines
3. Auto-starts flows with `auto_start: true`

## Use Cases

- **Video transcoding farms**: Create multiple encoding pipelines
- **Live streaming**: RTSP/RTMP ingest and restream
- **Recording systems**: Multi-camera recording with various formats
- **Media testing**: Quickly prototype and test GStreamer pipelines
- **Broadcast automation**: Pre-configured workflows that start automatically
- **Development tool**: Visual GStreamer learning and experimentation

## Project Structure

```
strom/
├── Cargo.toml                 # Workspace definition
├── README.md
├── docs/                      # Documentation
│   ├── TODO.md                # Development roadmap
│   ├── PROGRESS.md            # Current status
│   ├── CONTRIBUTING.md        # Contribution guidelines
│   └── INTEGRATION.md         # Integration options
├── types/                     # Shared types library
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs             # Library entry point
│       ├── flow.rs            # Flow domain models
│       ├── element.rs         # Element and property types
│       ├── api.rs             # API request/response types
│       ├── state.rs           # Pipeline state enums
│       └── events.rs          # SSE event types
├── backend/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs            # Server entry point
│       ├── lib.rs             # Library entry point
│       ├── api/               # REST and SSE handlers
│       │   ├── flows.rs       # Flow CRUD operations
│       │   ├── elements.rs    # Element discovery
│       │   └── sse.rs         # Server-Sent Events
│       ├── gst/               # GStreamer integration
│       │   ├── pipeline.rs    # Pipeline management
│       │   └── discovery.rs   # Element discovery
│       ├── storage/           # Persistence layer
│       │   └── json_storage.rs
│       ├── state.rs           # Application state
│       ├── config.rs          # Configuration
│       ├── events.rs          # Event broadcasting
│       ├── openapi.rs         # OpenAPI documentation
│       └── assets.rs          # Static asset serving
└── frontend/
    ├── Cargo.toml
    ├── Trunk.toml             # Build configuration
    ├── index.html
    └── src/
        ├── main.rs            # Frontend entry point
        ├── app.rs             # Main egui application
        ├── graph.rs           # Node graph editor
        ├── palette.rs         # Element palette
        ├── properties.rs      # Property inspector
        ├── api.rs             # API client
        └── sse.rs             # SSE client

```

## Getting Started

### Prerequisites

- Rust 1.75+ with `cargo`
- GStreamer 1.20+ development libraries
- Graphviz (for debug graph feature): `sudo apt install graphviz`
- WebAssembly target: `rustup target add wasm32-unknown-unknown`
- Trunk: `cargo install trunk`

### Building

```bash
# Build all components
cargo build --release

# Build specific components
cargo build --release -p strom-types
cargo build --release -p strom-backend
cd frontend && trunk build --release

# Development mode with hot reload
# Terminal 1: Backend
cargo run -p strom-backend

# Terminal 2: Frontend
cd frontend && trunk serve
```

### Running

```bash
# Production: backend serves frontend
cargo run --release -p strom-backend

# Open browser to http://localhost:3000
```

## Configuration

Configuration is handled via environment variables or `config.toml`:

```toml
[server]
host = "127.0.0.1"
port = 3000
static_dir = "./frontend/dist"

[storage]
type = "json"
path = "./flows.json"

[gstreamer]
debug_level = 2
```

## Development Roadmap

See [docs/TODO.md](docs/TODO.md) for detailed development tasks.

**Phase 1**: Core infrastructure (backend framework, basic frontend, persistence)
**Phase 2**: GStreamer integration (element discovery, pipeline management)
**Phase 3**: Visual editor (node graph, property inspector)
**Phase 4**: Polish (error handling, validation, documentation)
**Phase 5**: Advanced features (templates, monitoring, database support)

## Docker

Strom can be run as a Docker container with optimal build caching using cargo-chef.

### Quick Start with Docker

```bash
# Build the image
docker build -t strom:latest .

# Run with Docker
docker run -p 8080:8080 -v $(pwd)/data:/data strom:latest

# Or use docker-compose
docker-compose up
```

### Docker Configuration

The Docker image:
- Uses multi-stage builds with cargo-chef for fast rebuilds
- Includes all GStreamer runtime dependencies
- Exposes port 8080 by default
- Persists flows to `/data/flows.json`

Environment variables:
- `RUST_LOG` - Log level (default: info)
- `STROM_PORT` - Server port (default: 8080)
- `STROM_FLOWS_PATH` - Flow storage path (default: /data/flows.json)

## Continuous Integration

The project includes comprehensive CI/CD via GitHub Actions:

- **Format Check** - Ensures code follows rustfmt standards
- **Clippy** - Linting with no warnings allowed
- **Tests** - Full test suite must pass
- **Build** - Verifies complete build including frontend
- **Docker** - Builds and caches Docker images

All checks run automatically on pull requests and must pass before merging.

## Contributing

We welcome contributions! Please see [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md) for detailed guidelines.

### Quick Start for Contributors

1. Fork and clone the repository
2. Install development dependencies (see docs/CONTRIBUTING.md)
3. Install Git hooks: `./scripts/install-hooks.sh`
4. Make your changes
5. Ensure all checks pass:
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace
   ```
6. Submit a pull request

The pre-commit hook will automatically check formatting and linting before each commit.

## License

MIT OR Apache-2.0 (standard Rust dual license)

## Name Origin

**Strom** is Swedish, translated as "ström" meaning "stream" - fitting for a GStreamer-based application with Scandinavian roots.
