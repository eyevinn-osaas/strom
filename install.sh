#!/usr/bin/env bash
#
# Strom Installer Script
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh | bash
#
# Options (set as environment variables):
#   INSTALL_DIR              - Installation directory (default: /usr/local/bin or ~/.local/bin)
#   SKIP_GSTREAMER           - Skip GStreamer installation (default: false, GStreamer installs by default)
#   GSTREAMER_INSTALL_TYPE   - GStreamer install type: "minimal" or "full" (default: full)
#   SKIP_GRAPHVIZ            - Skip Graphviz installation (default: false, Graphviz installs by default)
#   INSTALL_MCP_SERVER       - Install strom-mcp-server instead of strom (default: false)
#   VERSION                  - Specific version to install (default: latest)
#
# Examples:
#   # Install strom with all dependencies (default behavior)
#   curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh | bash
#
#   # Install with minimal GStreamer
#   curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh | GSTREAMER_INSTALL_TYPE=minimal bash
#
#   # Skip dependencies (binary only)
#   curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh | SKIP_GSTREAMER=true SKIP_GRAPHVIZ=true bash
#
#   # Install MCP server with dependencies
#   curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh | INSTALL_MCP_SERVER=true bash
#

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
REPO="Eyevinn/strom"
BINARY_NAME="${INSTALL_MCP_SERVER:+strom-mcp-server}"
BINARY_NAME="${BINARY_NAME:-strom}"
VERSION="${VERSION:-latest}"
GSTREAMER_INSTALL_TYPE="${GSTREAMER_INSTALL_TYPE:-full}"

# Helper functions
log_info() {
    echo -e "${BLUE}==>${NC} $1"
}

log_success() {
    echo -e "${GREEN}==>${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}==>${NC} $1"
}

log_error() {
    echo -e "${RED}Error:${NC} $1" >&2
}

detect_os() {
    local os
    case "$(uname -s)" in
        Linux*)     os=linux;;
        Darwin*)    os=macos;;
        CYGWIN*|MINGW*|MSYS*) os=windows;;
        *)
            log_error "Unsupported operating system: $(uname -s)"
            exit 1
            ;;
    esac
    echo "$os"
}

detect_arch() {
    local arch
    case "$(uname -m)" in
        x86_64|amd64)   arch=x86_64;;
        aarch64|arm64)  arch=aarch64;;
        *)
            log_error "Unsupported architecture: $(uname -m)"
            exit 1
            ;;
    esac
    echo "$arch"
}

get_latest_version() {
    log_info "Fetching latest version..."
    local version
    version=$(curl -sSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
    if [ -z "$version" ]; then
        log_error "Failed to fetch latest version"
        exit 1
    fi
    echo "$version"
}

download_binary() {
    local os=$1
    local arch=$2
    local version=$3
    local binary_name=$4

    local ext=""
    if [ "$os" = "windows" ]; then
        ext=".exe"
    fi

    local artifact="${binary_name}-${version}-${os}-${arch}${ext}"
    local url="https://github.com/${REPO}/releases/download/${version}/${artifact}"

    log_info "Downloading ${artifact}..."
    log_info "URL: $url"

    local tmp_file=$(mktemp)
    if ! curl -sSL -f "$url" -o "$tmp_file"; then
        log_error "Failed to download binary from $url"
        rm -f "$tmp_file"
        exit 1
    fi

    echo "$tmp_file"
}

determine_install_dir() {
    if [ -n "${INSTALL_DIR:-}" ]; then
        echo "$INSTALL_DIR"
        return
    fi

    # Try /usr/local/bin if we have write access or can sudo
    if [ -w "/usr/local/bin" ] || command -v sudo >/dev/null 2>&1; then
        echo "/usr/local/bin"
    else
        # Fallback to user's local bin
        local user_bin="$HOME/.local/bin"
        mkdir -p "$user_bin"
        echo "$user_bin"
    fi
}

install_binary() {
    local tmp_file=$1
    local install_dir=$2
    local binary_name=$3

    local dest="$install_dir/$binary_name"

    log_info "Installing to $dest..."

    if [ -w "$install_dir" ]; then
        mv "$tmp_file" "$dest"
        chmod +x "$dest"
    elif command -v sudo >/dev/null 2>&1; then
        sudo mv "$tmp_file" "$dest"
        sudo chmod +x "$dest"
    else
        log_error "No write access to $install_dir and sudo not available"
        rm -f "$tmp_file"
        exit 1
    fi

    log_success "Installed $binary_name to $dest"
}

install_gstreamer() {
    local os=$1
    local install_type=$2

    log_info "Installing GStreamer ($install_type)..."

    case "$os" in
        linux)
            if command -v apt-get >/dev/null 2>&1; then
                sudo apt-get update

                # Minimal: core libraries + basic plugins
                local packages=(
                    libgstreamer1.0-dev
                    libgstreamer-plugins-base1.0-dev
                    gstreamer1.0-plugins-base
                    gstreamer1.0-plugins-good
                    gstreamer1.0-tools
                )

                # Full: add advanced plugins and extras
                if [ "$install_type" = "full" ]; then
                    packages+=(
                        libgstreamer-plugins-bad1.0-dev
                        gstreamer1.0-plugins-bad
                        gstreamer1.0-plugins-ugly
                        gstreamer1.0-libav
                        gstreamer1.0-nice
                        gstreamer1.0-x
                        gstreamer1.0-alsa
                        gstreamer1.0-gl
                        gstreamer1.0-gtk3
                        gstreamer1.0-qt5
                        gstreamer1.0-pulseaudio
                    )
                fi

                sudo apt-get install -y "${packages[@]}"
                log_success "GStreamer installed successfully"
            elif command -v dnf >/dev/null 2>&1; then
                local packages=(
                    gstreamer1-devel
                    gstreamer1-plugins-base-devel
                    gstreamer1-plugins-good
                )

                if [ "$install_type" = "full" ]; then
                    packages+=(
                        gstreamer1-plugins-bad-free
                        gstreamer1-plugins-ugly-free
                        libnice-gstreamer
                    )
                fi

                sudo dnf install -y "${packages[@]}"
                log_success "GStreamer installed successfully"
            elif command -v pacman >/dev/null 2>&1; then
                local packages=(
                    gstreamer
                    gst-plugins-base
                    gst-plugins-good
                )

                if [ "$install_type" = "full" ]; then
                    packages+=(
                        gst-plugins-bad
                        gst-plugins-ugly
                        gst-libav
                        libnice
                    )
                fi

                sudo pacman -S --noconfirm "${packages[@]}"
                log_success "GStreamer installed successfully"
            else
                log_warning "Unsupported package manager. Please install GStreamer manually."
                log_info "See: https://gstreamer.freedesktop.org/documentation/installing/"
            fi
            ;;
        macos)
            if command -v brew >/dev/null 2>&1; then
                local packages=(gstreamer gst-plugins-base gst-plugins-good)

                if [ "$install_type" = "full" ]; then
                    packages+=(gst-plugins-bad gst-plugins-ugly gst-libav libnice)
                fi

                brew install "${packages[@]}"
                log_success "GStreamer installed successfully"
            else
                log_warning "Homebrew not found. Please install GStreamer manually."
                log_info "See: https://gstreamer.freedesktop.org/documentation/installing/on-mac-osx.html"
            fi
            ;;
        windows)
            log_warning "Automatic GStreamer installation not supported on Windows."
            log_info "Please download and install from: https://gstreamer.freedesktop.org/download/"
            ;;
    esac
}

install_graphviz() {
    local os=$1

    log_info "Installing Graphviz..."

    case "$os" in
        linux)
            if command -v apt-get >/dev/null 2>&1; then
                sudo apt-get update
                sudo apt-get install -y graphviz
                log_success "Graphviz installed successfully"
            elif command -v dnf >/dev/null 2>&1; then
                sudo dnf install -y graphviz
                log_success "Graphviz installed successfully"
            elif command -v pacman >/dev/null 2>&1; then
                sudo pacman -S --noconfirm graphviz
                log_success "Graphviz installed successfully"
            else
                log_warning "Unsupported package manager. Please install Graphviz manually."
            fi
            ;;
        macos)
            if command -v brew >/dev/null 2>&1; then
                brew install graphviz
                log_success "Graphviz installed successfully"
            else
                log_warning "Homebrew not found. Please install Graphviz manually."
            fi
            ;;
        windows)
            log_warning "Automatic Graphviz installation not supported on Windows."
            log_info "Please download and install from: https://graphviz.org/download/"
            ;;
    esac
}

check_path() {
    local install_dir=$1

    if [[ ":$PATH:" != *":$install_dir:"* ]]; then
        log_warning "$install_dir is not in your PATH"
        log_info "Add it to your PATH by adding this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
        echo ""
        echo "    export PATH=\"$install_dir:\$PATH\""
        echo ""
    fi
}

main() {
    log_info "Strom Installer"
    echo ""

    # Detect system
    local os=$(detect_os)
    local arch=$(detect_arch)
    log_info "Detected: $os-$arch"

    # Get version
    if [ "$VERSION" = "latest" ]; then
        VERSION=$(get_latest_version)
    fi
    log_info "Version: $VERSION"

    # Install dependencies (default: install both)
    if [ "${SKIP_GSTREAMER:-false}" != "true" ]; then
        install_gstreamer "$os" "$GSTREAMER_INSTALL_TYPE"
    else
        log_warning "Skipping GStreamer installation (Strom requires GStreamer to run)"
    fi

    if [ "${SKIP_GRAPHVIZ:-false}" != "true" ]; then
        install_graphviz "$os"
    else
        log_info "Skipping Graphviz installation"
    fi

    # Download and install binary
    local tmp_file=$(download_binary "$os" "$arch" "$VERSION" "$BINARY_NAME")
    local install_dir=$(determine_install_dir)
    install_binary "$tmp_file" "$install_dir" "$BINARY_NAME"

    # Check PATH
    check_path "$install_dir"

    echo ""
    log_success "Installation complete! 🎉"

    if [ "${SKIP_GSTREAMER:-false}" = "true" ]; then
        log_warning "GStreamer was not installed. Strom requires GStreamer to function."
        log_info "Install it manually: https://gstreamer.freedesktop.org/documentation/installing/"
    fi

    log_info "Run '$BINARY_NAME --help' to get started"
}

main "$@"
