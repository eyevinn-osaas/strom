#!/usr/bin/env bash
#
# Strom Installer Script
#
# Usage:
#   # Interactive mode (default, for humans):
#   bash <(curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh)
#   # Shows configuration menu before installation
#
#   # Automated mode (for CI/CD):
#   curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh | AUTO_INSTALL=true bash
#   # Skips menu, uses defaults or environment variables
#
# Options (set as environment variables):
#   AUTO_INSTALL             - Skip interactive menu (default: false, menu shown by default)
#   INSTALL_DIR              - Installation directory (default: /usr/local/bin or ~/.local/bin)
#   SKIP_GSTREAMER           - Skip GStreamer installation (default: false, GStreamer installs by default)
#   GSTREAMER_INSTALL_TYPE   - GStreamer install type: "minimal" or "full" (default: full)
#   SKIP_GRAPHVIZ            - Skip Graphviz installation (default: false, Graphviz installs by default)
#   INSTALL_MCP_SERVER       - Install strom-mcp-server instead of strom (default: false)
#   VERSION                  - Specific version to install (default: latest)
#
# Examples:
#   # Interactive install (shows menu) - DEFAULT
#   bash <(curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh)
#
#   # Automated install (CI/CD, skips menu)
#   curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh | AUTO_INSTALL=true bash
#
#   # Automated with minimal GStreamer
#   curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh | AUTO_INSTALL=true GSTREAMER_INSTALL_TYPE=minimal bash
#
#   # Automated binary only (skip dependencies)
#   curl -sSL https://raw.githubusercontent.com/Eyevinn/strom/main/install.sh | AUTO_INSTALL=true SKIP_GSTREAMER=true SKIP_GRAPHVIZ=true bash
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
SKIP_GSTREAMER="${SKIP_GSTREAMER:-false}"
SKIP_GRAPHVIZ="${SKIP_GRAPHVIZ:-false}"
INSTALL_DIR="${INSTALL_DIR:-}"
AUTO_INSTALL="${AUTO_INSTALL:-false}"

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

# Run command with sudo only if not root
run_elevated() {
    if [ "$(id -u)" -eq 0 ]; then
        # Already root, run directly
        "$@"
    elif command -v sudo >/dev/null 2>&1; then
        # Not root, use sudo
        sudo "$@"
    else
        log_error "Not running as root and sudo not available"
        exit 1
    fi
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
    else
        run_elevated mv "$tmp_file" "$dest"
        run_elevated chmod +x "$dest"
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
                run_elevated apt-get update

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

                run_elevated apt-get install -y "${packages[@]}"
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

                run_elevated dnf install -y "${packages[@]}"
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

                run_elevated pacman -S --noconfirm "${packages[@]}"
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
                run_elevated apt-get update
                run_elevated apt-get install -y graphviz
                log_success "Graphviz installed successfully"
            elif command -v dnf >/dev/null 2>&1; then
                run_elevated dnf install -y graphviz
                log_success "Graphviz installed successfully"
            elif command -v pacman >/dev/null 2>&1; then
                run_elevated pacman -S --noconfirm graphviz
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

show_config_menu() {
    # Skip menu if AUTO_INSTALL is set (for automation)
    if [ "$AUTO_INSTALL" = "true" ]; then
        log_info "Auto-install mode enabled, skipping configuration menu..."
        return
    fi

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  Strom Installation Configuration"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Current settings:"
    echo ""
    echo "  1. Binary:           ${GREEN}${BINARY_NAME}${NC}"
    echo "  2. Version:          ${GREEN}${VERSION}${NC}"
    echo "  3. Install GStreamer: ${GREEN}$([ "$SKIP_GSTREAMER" = "false" ] && echo "Yes" || echo "No")${NC}"
    echo "  4. GStreamer Type:   ${GREEN}${GSTREAMER_INSTALL_TYPE}${NC} (minimal/full)"
    echo "  5. Install Graphviz: ${GREEN}$([ "$SKIP_GRAPHVIZ" = "false" ] && echo "Yes" || echo "No")${NC}"
    if [ -n "$INSTALL_DIR" ]; then
        echo "  6. Install Directory: ${GREEN}${INSTALL_DIR}${NC}"
    else
        echo "  6. Install Directory: ${GREEN}auto (/usr/local/bin or ~/.local/bin)${NC}"
    fi
    echo ""
    echo "  ${GREEN}c${NC}. Continue with these settings"
    echo "  ${GREEN}q${NC}. Quit"
    echo ""
    echo -n "Enter option to change (or 'c' to continue): "

    read -r choice

    case "$choice" in
        1)
            echo ""
            echo "Select binary to install:"
            echo "  1. strom (main application)"
            echo "  2. strom-mcp-server (MCP server)"
            echo -n "Choice [1-2]: "
            read -r bin_choice
            case "$bin_choice" in
                1) BINARY_NAME="strom" ;;
                2) BINARY_NAME="strom-mcp-server" ;;
                *) log_warning "Invalid choice, keeping current setting" ;;
            esac
            show_config_menu
            ;;
        2)
            echo ""
            echo -n "Enter version (or 'latest'): "
            read -r ver
            VERSION="${ver:-latest}"
            show_config_menu
            ;;
        3)
            echo ""
            echo "Install GStreamer? (Required for Strom to work)"
            echo -n "Choice [y/N]: "
            read -r gst_choice
            case "$gst_choice" in
                [Yy]*) SKIP_GSTREAMER="false" ;;
                [Nn]*) SKIP_GSTREAMER="true" ;;
                *) log_warning "Invalid choice, keeping current setting" ;;
            esac
            show_config_menu
            ;;
        4)
            echo ""
            echo "Select GStreamer installation type:"
            echo "  1. minimal - Core + base/good plugins (~200MB)"
            echo "  2. full - All plugins + WebRTC support (~500MB)"
            echo -n "Choice [1-2]: "
            read -r type_choice
            case "$type_choice" in
                1) GSTREAMER_INSTALL_TYPE="minimal" ;;
                2) GSTREAMER_INSTALL_TYPE="full" ;;
                *) log_warning "Invalid choice, keeping current setting" ;;
            esac
            show_config_menu
            ;;
        5)
            echo ""
            echo "Install Graphviz? (Required for debug graphs)"
            echo -n "Choice [y/N]: "
            read -r gv_choice
            case "$gv_choice" in
                [Yy]*) SKIP_GRAPHVIZ="false" ;;
                [Nn]*) SKIP_GRAPHVIZ="true" ;;
                *) log_warning "Invalid choice, keeping current setting" ;;
            esac
            show_config_menu
            ;;
        6)
            echo ""
            echo -n "Enter install directory (or leave empty for auto): "
            read -r dir
            INSTALL_DIR="$dir"
            show_config_menu
            ;;
        [Cc]*)
            echo ""
            log_info "Proceeding with installation..."
            ;;
        [Qq]*)
            echo ""
            log_info "Installation cancelled."
            exit 0
            ;;
        *)
            log_warning "Invalid option"
            show_config_menu
            ;;
    esac
}

main() {
    log_info "Strom Installer"
    echo ""

    # Detect system
    local os=$(detect_os)
    local arch=$(detect_arch)
    log_info "Detected: $os-$arch"

    # Show interactive configuration menu if running in a terminal
    show_config_menu

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
