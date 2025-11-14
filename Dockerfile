# Dockerfile for Strom - Multi-stage build with cargo-chef for optimal caching

# Stage 1: Planner - Analyze dependencies and create recipe
FROM rust:1.83-trixie as planner
WORKDIR /app
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Builder - Build dependencies using cargo-chef
FROM rust:1.83-trixie as builder
WORKDIR /app

# Install cargo-chef
RUN cargo install cargo-chef

# Install GStreamer development dependencies
RUN apt-get update && apt-get install -y \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    gstreamer1.0-tools \
    && rm -rf /var/lib/apt/lists/*

# Install trunk for building the WASM frontend
RUN cargo install trunk

# Add WASM target for frontend compilation
RUN rustup target add wasm32-unknown-unknown

# Copy recipe and build dependencies (this layer is cached)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy entire project source
COPY . .

# Build the frontend
RUN cd frontend && trunk build --release

# Build the backend (with embedded frontend)
RUN cargo build --release --package strom-backend

# Stage 3: Runtime - Minimal runtime image
FROM debian:trixie-slim as runtime
WORKDIR /app

# Install only GStreamer runtime dependencies
RUN apt-get update && apt-get install -y \
    libgstreamer1.0-0 \
    libgstreamer-plugins-base1.0-0 \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    gstreamer1.0-tools \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from builder
COPY --from=builder /app/target/release/strom-backend /usr/local/bin/strom-backend

# Set environment variables
ENV RUST_LOG=info
ENV STROM_PORT=8080
ENV STROM_FLOWS_PATH=/data/flows.json

# Create data directory for persistent storage
RUN mkdir -p /data

# Expose the server port
EXPOSE 8080

# Run the server
CMD ["strom-backend"]
