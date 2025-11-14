# Changelog

All notable changes to the Strom GStreamer Flow Engine project.

## [Unreleased]

### Added
- Multi-port support with media type classification
  - Visual color coding for audio (green), video (orange), and generic (blue) ports
  - A/V labels inside audio and video ports for better visual distinction
  - Support for elements with multiple input/output pads
  - PadPresence (Always/Sometimes/Request) and MediaType enums
- Dynamic pad linking for runtime-created pads
  - Automatic pad-added signal handlers
  - Support for elements like decodebin, demuxers with dynamic outputs
  - Pending link queue for pads not yet available
- Automatic tee insertion for multiple outputs
  - Detects when one source connects to multiple destinations
  - Automatically inserts and configures tee elements
  - No manual configuration needed
- Server-Sent Events (SSE) for real-time updates
  - Pipeline state changes, errors, warnings, and info messages
  - Flow lifecycle events (created, updated, deleted, started, stopped)
  - End-of-stream notifications
  - Keep-alive pings
  - Backend endpoint: `GET /api/events`
- Pipeline debug visualization feature
  - DOT graph generation from running GStreamer pipelines
  - Automatic SVG conversion using Graphviz
  - "Debug Graph" button in UI toolbar
  - Opens interactive SVG in new browser tab
  - Backend endpoint: `GET /api/flows/:id/debug-graph`

### Changed
- Upgraded Rust version requirement to 1.82+
- Improved property type handling for i32/i64 and u32/u64 conversions
- Enhanced element introspection to skip write-only properties

### Fixed
- Property type mismatches causing runtime errors
- Element introspection panics on write-only properties

## [0.1.0] - 2025-01-13

### Added - Backend
- Complete Cargo workspace structure (types, backend, frontend)
- Axum web server with CORS and static file serving
- Full REST API for flow management (CRUD operations)
- GStreamer pipeline integration:
  - Pipeline creation from flow definitions
  - Element instantiation and property configuration
  - Pad linking with validation
  - Start/stop/pause pipeline control
  - State management and tracking
- Element discovery and introspection API
- JSON file storage backend with async I/O
- OpenAPI documentation with Swagger UI at `/swagger-ui`
- Structured logging with tracing
- Configuration system (environment variables + config file)
- Auto-start flows on server boot
- Health check endpoint

### Added - Frontend
- egui-based WebAssembly UI
- Custom node-based graph editor:
  - Drag nodes to reposition
  - Click-and-drag to create links between pads
  - Pan canvas (drag on background)
  - Zoom canvas (mouse wheel)
  - Grid background for alignment
  - Visual feedback for selected nodes
- Element palette panel:
  - Search functionality
  - Category filtering
  - Pre-loaded with 17 common GStreamer elements
  - Element descriptions and tooltips
- Property inspector:
  - Type-appropriate input widgets (text, number, slider, checkbox)
  - Common properties for well-known elements
  - Custom property support
- Flow management:
  - Create new flow dialog
  - Flow list sidebar
  - Delete flow functionality
- Pipeline controls:
  - Start/Stop buttons
  - State visualization with color-coding
  - Auto-start toggle
- API client with full CRUD support
- LocalStorage integration for async state handling
- Trunk build configuration
- Dark theme UI

### Added - Shared Types
- Domain models: Flow, Element, Link, PipelineState
- API request/response types
- OpenAPI schema support with utoipa
- Serde serialization
- UUID support with WASM compatibility (js feature)

### Technical
- Full Rust implementation (backend + frontend)
- WebAssembly compilation for frontend
- Comprehensive error handling
- Unit and integration tests
- Development and production build configurations

## [0.0.1] - Initial Architecture

### Added
- Project architecture design
- Technology stack selection
- Development roadmap (TODO.md)
- README with project overview

---

## Version Numbering

This project follows [Semantic Versioning](https://semver.org/):
- MAJOR version for incompatible API changes
- MINOR version for new functionality in a backwards compatible manner
- PATCH version for backwards compatible bug fixes

## Categories

- **Added**: New features
- **Changed**: Changes to existing functionality
- **Deprecated**: Soon-to-be removed features
- **Removed**: Removed features
- **Fixed**: Bug fixes
- **Security**: Security-related changes
