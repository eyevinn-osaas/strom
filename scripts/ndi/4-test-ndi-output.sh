#!/usr/bin/env bash
#
# Test NDI Output (Sender)
#
# This script creates a test NDI sender using GStreamer directly.
# It sends a test pattern video stream via NDI that can be received
# by NDI-compatible receivers.
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
NDI_NAME="${NDI_NAME:-Strom-Test-Output}"
VIDEO_PATTERN="${VIDEO_PATTERN:-0}"  # 0=smpte, 1=snow, 18=ball, etc.
AUDIO_WAVE="${AUDIO_WAVE:-0}"        # 0=sine, 4=pink-noise, etc.

echo "═══════════════════════════════════════════════════════════"
echo "  NDI Output Test"
echo "═══════════════════════════════════════════════════════════"
echo ""
log_info "NDI Stream Name: $NDI_NAME"
log_info "Video Pattern: $VIDEO_PATTERN (0=smpte, 1=snow, 18=ball)"
log_info "Audio Wave: $AUDIO_WAVE (0=sine, 4=pink-noise)"
echo ""

# Verify NDI plugin is available
if ! gst-inspect-1.0 ndisink >/dev/null 2>&1; then
    log_error "ndisink element not found. Run 3-verify-ndi-installation.sh first."
fi

log_success "NDI plugin found"
echo ""

# Display test options
cat << EOF
This test will create an NDI stream with:
  - 1920x1080 @ 30fps test pattern video
  - 48kHz stereo test tone audio
  - NDI stream name: $NDI_NAME

The stream will be discoverable by NDI receivers on your network.

Available test patterns (change with VIDEO_PATTERN env var):
  0  = SMPTE color bars
  1  = Snow (random)
  18 = Bouncing ball
  20 = Circular gradient

Available audio waves (change with AUDIO_WAVE env var):
  0 = Sine wave (440 Hz)
  4 = Pink noise
  8 = Sine wave (multiple frequencies)

To customize: VIDEO_PATTERN=18 AUDIO_WAVE=4 NDI_NAME="My Stream" $0

EOF

read -p "Press Enter to start the test stream (Ctrl+C to stop)..."
echo ""

log_info "Starting NDI output stream..."
log_warning "Press Ctrl+C to stop the stream"
echo ""

# Create GStreamer pipeline
# Video: test pattern -> convert -> combiner
# Audio: test tone -> convert/resample -> combiner
# Combiner -> NDI sink
gst-launch-1.0 -v \
    ndisinkcombiner name=combiner ! ndisink ndi-name="$NDI_NAME" \
    videotestsrc pattern=$VIDEO_PATTERN is-live=true ! \
        video/x-raw,width=1920,height=1080,framerate=30/1 ! \
        videoconvert ! \
        combiner.video \
    audiotestsrc wave=$AUDIO_WAVE is-live=true ! \
        audio/x-raw,rate=48000,channels=2 ! \
        audioconvert ! \
        audioresample ! \
        combiner.audio

log_success "NDI output test completed"
