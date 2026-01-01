#!/usr/bin/env bash
#
# Test NDI Input (Receiver)
#
# This script discovers and receives NDI streams using GStreamer.
# It can list available sources or receive a specific stream.
#

set -euo pipefail

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
NDI_SOURCE="${NDI_SOURCE:-}"
SHOW_VIDEO="${SHOW_VIDEO:-true}"
SHOW_AUDIO="${SHOW_AUDIO:-true}"

echo "═══════════════════════════════════════════════════════════"
echo "  NDI Input Test"
echo "═══════════════════════════════════════════════════════════"
echo ""

# Verify NDI plugin is available
if ! gst-inspect-1.0 ndisrc >/dev/null 2>&1; then
    log_error "ndisrc element not found. Run 3-verify-ndi-installation.sh first."
fi

log_success "NDI plugin found"
echo ""

# Function to list NDI sources
list_sources() {
    log_warning "NDI source discovery is not available via command line"
    echo ""
    echo "To find NDI sources, you can:"
    echo "  1. Use NDI Studio Monitor or NDI Screen Capture (from NDI Tools)"
    echo "  2. Check your NDI-enabled devices/applications"
    echo "  3. Start the test sender: ./4-test-ndi-output.sh"
    echo ""
    echo "Common NDI source name format:"
    echo "  HOSTNAME (Source Name)"
    echo ""
    HOSTNAME=$(hostname)
    echo "  Your hostname: $HOSTNAME"
    echo "  Full NDI name: $HOSTNAME (Strom-Test-Output)"
    echo ""
    log_info "Use the full name including hostname when connecting"
}

# Function to receive NDI stream
receive_stream() {
    local source_name="$1"

    log_info "Receiving NDI stream: $source_name"
    log_warning "Press Ctrl+C to stop receiving"
    echo ""

    # Build pipeline based on what to show
    if [ "$SHOW_VIDEO" = "true" ] && [ "$SHOW_AUDIO" = "true" ]; then
        # Both video and audio - use demuxer
        log_info "Receiving video and audio..."
        gst-launch-1.0 -v \
            ndisrc ndi-name="$source_name" ! \
                ndisrcdemux name=demux \
            demux.video ! queue ! videoconvert ! autovideosink \
            demux.audio ! queue ! audioconvert ! audioresample ! autoaudiosink

    elif [ "$SHOW_VIDEO" = "true" ]; then
        # Video only - use demuxer and select video
        log_info "Receiving video only..."
        gst-launch-1.0 -v \
            ndisrc ndi-name="$source_name" ! \
                ndisrcdemux name=demux \
            demux.video ! queue ! videoconvert ! autovideosink

    elif [ "$SHOW_AUDIO" = "true" ]; then
        # Audio only - use demuxer and select audio
        log_info "Receiving audio only..."
        gst-launch-1.0 -v \
            ndisrc ndi-name="$source_name" ! \
                ndisrcdemux name=demux \
            demux.audio ! queue ! audioconvert ! audioresample ! autoaudiosink

    else
        log_error "Must enable at least one of video or audio"
    fi
}

# Menu function
show_menu() {
    echo "═══════════════════════════════════════════════════════════"
    echo "  NDI Input Test Options"
    echo "═══════════════════════════════════════════════════════════"
    echo ""
    HOSTNAME=$(hostname)
    echo "  1) List available NDI sources"
    echo "  2) Receive from specific NDI source"
    echo "  3) Receive from test source: $HOSTNAME (Strom-Test-Output)"
    echo "  q) Quit"
    echo ""
    read -p "Choose an option: " choice

    case "$choice" in
        1)
            list_sources
            echo ""
            read -p "Press Enter to return to menu..."
            show_menu
            ;;
        2)
            echo ""
            read -p "Enter NDI source name: " source_name
            if [ -z "$source_name" ]; then
                log_warning "No source name provided"
                show_menu
            else
                receive_stream "$source_name"
            fi
            ;;
        3)
            # Get hostname and construct full NDI name
            HOSTNAME=$(hostname)
            FULL_NDI_NAME="$HOSTNAME (Strom-Test-Output)"
            log_info "Using full NDI name: $FULL_NDI_NAME"
            receive_stream "$FULL_NDI_NAME"
            ;;
        q|Q)
            log_info "Exiting"
            exit 0
            ;;
        *)
            log_warning "Invalid option"
            show_menu
            ;;
    esac
}

# Main execution
if [ -n "$NDI_SOURCE" ]; then
    # Source provided via environment variable, receive directly
    receive_stream "$NDI_SOURCE"
else
    # No source provided, show menu
    cat << EOF
This test will receive an NDI stream.

You can:
  - List available NDI sources on your network
  - Receive a specific NDI source
  - Receive from the default test source (requires running 4-test-ndi-output.sh)

To receive a specific source directly:
  NDI_SOURCE="Source Name" $0

To receive video only:
  SHOW_AUDIO=false $0

To receive audio only:
  SHOW_VIDEO=false $0

EOF
    show_menu
fi

log_success "NDI input test completed"
