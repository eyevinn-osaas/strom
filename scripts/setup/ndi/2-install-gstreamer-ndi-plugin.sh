#!/usr/bin/env bash
#
# Install GStreamer NDI Plugin
#
# This script builds and installs the gst-plugin-ndi GStreamer plugin
# from source. This plugin provides ndisrc and ndisink elements for
# GStreamer pipelines.
#

set -euo pipefail

# Use sudo only if not already root
if [ "$(id -u)" -eq 0 ]; then SUDO=""; else SUDO="sudo"; fi

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}==>${NC} $1"
}

log_success() {
    echo -e "${GREEN}==>${NC} $1"
}

log_error() {
    echo -e "${RED}Error:${NC} $1"
    exit 1
}

log_warning() {
    echo -e "${YELLOW}==>${NC} $1"
}

# Configuration
WORK_DIR="${WORK_DIR:-/tmp/gst-plugin-ndi-build}"
PLUGIN_REPO="${PLUGIN_REPO:-https://github.com/teltek/gst-plugin-ndi.git}"
PLUGIN_BRANCH="${PLUGIN_BRANCH:-master}"
BUILD_TYPE="${BUILD_TYPE:-release}"

# Detect architecture
ARCH=$(uname -m)
case "$ARCH" in
    x86_64)
        LIB_DIR="x86_64-linux-gnu"
        ;;
    aarch64|arm64)
        LIB_DIR="aarch64-linux-gnu"
        ;;
    *)
        log_error "Unsupported architecture: $ARCH"
        ;;
esac

log_info "GStreamer NDI Plugin Installer"
echo ""
log_info "Architecture: $ARCH"
log_info "Build type: $BUILD_TYPE"
log_info "Work directory: $WORK_DIR"
echo ""

# Verify NDI SDK is installed
log_info "Checking for NDI SDK..."
NDI_CHECK=$(ldconfig -p | grep libndi.so || true)
if [ -z "$NDI_CHECK" ]; then
    log_error "NDI SDK not found! Please run 1-install-ndi-sdk.sh first."
fi
log_success "NDI SDK found"

# Install build dependencies
log_info "Installing build dependencies..."
if command -v apt-get >/dev/null 2>&1; then
    $SUDO apt-get update
    $SUDO apt-get install -y \
        libgstreamer1.0-dev \
        libgstreamer-plugins-base1.0-dev \
        gstreamer1.0-plugins-base \
        cargo \
        rustc \
        git \
        pkg-config
else
    log_warning "apt-get not found. Please ensure GStreamer development packages and Rust are installed."
fi

# Verify Rust installation
if ! command -v cargo >/dev/null 2>&1; then
    log_error "Rust/Cargo not found. Install from: https://rustup.rs/"
fi
log_info "Rust version: $(rustc --version)"

# Create work directory
mkdir -p "$WORK_DIR"
cd "$WORK_DIR"

# Clone or update repository
if [ -d "gst-plugin-ndi" ]; then
    log_info "Repository already exists, updating..."
    cd gst-plugin-ndi
    git fetch origin
    git checkout "$PLUGIN_BRANCH"
    git pull origin "$PLUGIN_BRANCH"
else
    log_info "Cloning gst-plugin-ndi repository..."
    git clone "$PLUGIN_REPO" gst-plugin-ndi
    cd gst-plugin-ndi
    git checkout "$PLUGIN_BRANCH"
fi

log_info "Current directory: $(pwd)"
log_info "Repository info:"
git log -1 --oneline

# Build the plugin
echo ""
log_info "Building NDI plugin (this may take a few minutes)..."

if [ "$BUILD_TYPE" = "release" ]; then
    cargo build --release
    PLUGIN_PATH="target/release/libgstndi.so"
else
    cargo build
    PLUGIN_PATH="target/debug/libgstndi.so"
fi

# Verify build output
if [ ! -f "$PLUGIN_PATH" ]; then
    log_error "Build failed! Plugin library not found at: $PLUGIN_PATH"
fi

log_success "Build complete: $PLUGIN_PATH"

# Install the plugin
INSTALL_PATH="/usr/lib/$LIB_DIR/gstreamer-1.0/libgstndi.so"

log_info "Installing plugin to: $INSTALL_PATH"
$SUDO install -o root -g root -m 644 "$PLUGIN_PATH" "$INSTALL_PATH"

# Update library cache
log_info "Updating library cache..."
$SUDO ldconfig

# Clear GStreamer plugin cache
log_info "Clearing GStreamer plugin cache..."
rm -rf ~/.cache/gstreamer-1.0/

# Verify installation
echo ""
log_info "Verifying installation..."
if gst-inspect-1.0 ndi >/dev/null 2>&1; then
    log_success "GStreamer NDI plugin installed successfully!"
    echo ""
    echo "Available NDI elements:"
    gst-inspect-1.0 ndi | grep -E "^\s+(ndisrc|ndisink)" || true
else
    log_error "Plugin installation verification failed. GStreamer cannot find the NDI plugin."
fi

# Cleanup option
echo ""
read -p "Do you want to keep the build directory for debugging? [y/N] " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    log_info "Cleaning up build directory..."
    cd /tmp
    rm -rf "$WORK_DIR"
    log_success "Cleanup complete"
else
    log_info "Build directory preserved at: $WORK_DIR"
fi

echo ""
log_success "GStreamer NDI plugin installation complete!"
log_info "Next step: Run 3-verify-ndi-installation.sh to test the setup"
