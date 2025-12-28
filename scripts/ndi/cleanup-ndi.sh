#!/usr/bin/env bash
#
# Cleanup NDI Installation
#
# Removes all NDI SDK files to start fresh
#

set -euo pipefail

RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}==>${NC} $1"; }
log_warning() { echo -e "${YELLOW}==>${NC} $1"; }
log_error() { echo -e "${RED}Error:${NC} $1"; exit 1; }

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

log_warning "This will remove ALL NDI SDK files and installations"
echo ""
log_info "Architecture: $ARCH"
echo ""
echo "Will remove:"
echo "  - /tmp/NDI SDK for Linux/"
echo "  - /tmp/Install_NDI_SDK_v6_Linux.*"
echo "  - /usr/lib/$LIB_DIR/libndi.so*"
echo "  - /lib/$LIB_DIR/libndi.so*"
echo "  - /usr/lib/$LIB_DIR/gstreamer-1.0/libgstndi.so"
echo "  - /usr/include/ndi/"
echo ""
read -p "Continue? [y/N] " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    log_info "Cancelled"
    exit 0
fi

log_info "Removing NDI SDK files from /tmp..."
sudo rm -rf "/tmp/NDI SDK for Linux"
rm -f /tmp/Install_NDI_SDK_v6_Linux.*

log_info "Removing installed libraries..."
sudo rm -f /usr/lib/$LIB_DIR/libndi.so*
sudo rm -f /lib/$LIB_DIR/libndi.so*

log_info "Removing GStreamer NDI plugin..."
sudo rm -f /usr/lib/$LIB_DIR/gstreamer-1.0/libgstndi.so

log_info "Removing headers..."
sudo rm -rf /usr/include/ndi

log_info "Updating library cache..."
sudo ldconfig

log_info "Cleanup complete! You can now run: ./1-install-ndi-sdk.sh"
