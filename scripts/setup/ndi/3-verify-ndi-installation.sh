#!/usr/bin/env bash
#
# Verify NDI Installation
#
# This script checks that both the NDI SDK and GStreamer NDI plugin
# are properly installed and configured.
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
    echo -e "${GREEN}✓${NC} $1"
}

log_error() {
    echo -e "${RED}✗${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}!${NC} $1"
}

ERRORS=0
WARNINGS=0

check_pass() {
    log_success "$1"
}

check_fail() {
    log_error "$1"
    ((ERRORS++))
}

check_warn() {
    log_warning "$1"
    ((WARNINGS++))
}

echo "═══════════════════════════════════════════════════════════"
echo "  NDI Installation Verification"
echo "═══════════════════════════════════════════════════════════"
echo ""

# 1. Check NDI SDK library
log_info "Checking NDI SDK library..."
NDI_CHECK=$(ldconfig -p | grep libndi.so || true)
if [ -n "$NDI_CHECK" ]; then
    NDI_LIB=$(echo "$NDI_CHECK" | head -n 1 | awk '{print $NF}')
    check_pass "NDI library found: $NDI_LIB"

    # Check if it's a valid ELF file (follow symlinks with -L)
    if file -L "$NDI_LIB" | grep -q "ELF"; then
        check_pass "NDI library is valid ELF binary"
    else
        check_fail "NDI library exists but is not a valid binary"
    fi
else
    check_fail "NDI library (libndi.so) not found in ldconfig cache"
fi

echo ""

# 2. Check NDI runtime directories
log_info "Checking NDI runtime environment..."
if [ -n "${NDI_RUNTIME_DIR_V6:-}" ]; then
    check_pass "NDI_RUNTIME_DIR_V6 is set: $NDI_RUNTIME_DIR_V6"
elif [ -n "${NDI_RUNTIME_DIR_V5:-}" ]; then
    check_pass "NDI_RUNTIME_DIR_V5 is set: $NDI_RUNTIME_DIR_V5"
else
    check_warn "NDI_RUNTIME_DIR not set (this is usually OK if library is in system path)"
fi

echo ""

# 3. Check GStreamer installation
log_info "Checking GStreamer installation..."
if command -v gst-launch-1.0 >/dev/null 2>&1; then
    GST_VERSION=$(gst-launch-1.0 --version | grep "GStreamer" | awk '{print $2}')
    check_pass "GStreamer found: version $GST_VERSION"
else
    check_fail "gst-launch-1.0 not found"
fi

if command -v gst-inspect-1.0 >/dev/null 2>&1; then
    check_pass "gst-inspect-1.0 found"
else
    check_fail "gst-inspect-1.0 not found"
fi

echo ""

# 4. Check GStreamer NDI plugin
log_info "Checking GStreamer NDI plugin..."

# Clear plugin cache first
rm -rf ~/.cache/gstreamer-1.0/ 2>/dev/null || true

if gst-inspect-1.0 ndi >/dev/null 2>&1; then
    check_pass "NDI plugin registered with GStreamer"

    # Check for ndisrc
    if gst-inspect-1.0 ndisrc >/dev/null 2>&1; then
        check_pass "ndisrc element available"
    else
        check_fail "ndisrc element not found"
    fi

    # Check for ndisink
    if gst-inspect-1.0 ndisink >/dev/null 2>&1; then
        check_pass "ndisink element available"
    else
        check_fail "ndisink element not found"
    fi
else
    check_fail "NDI plugin not found by GStreamer"

    # Try to locate the plugin file
    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64) LIB_DIR="x86_64-linux-gnu" ;;
        aarch64|arm64) LIB_DIR="aarch64-linux-gnu" ;;
    esac

    PLUGIN_PATH="/usr/lib/$LIB_DIR/gstreamer-1.0/libgstndi.so"
    if [ -f "$PLUGIN_PATH" ]; then
        check_warn "Plugin file exists at $PLUGIN_PATH but GStreamer can't load it"
        echo "         Try: export GST_DEBUG=2 and run gst-inspect-1.0 ndi for details"
    else
        check_fail "Plugin file not found at expected location: $PLUGIN_PATH"
    fi
fi

echo ""

# 5. Check dependencies
log_info "Checking dependencies..."

AVAHI_COMMON=$(ldconfig -p | grep libavahi-common.so || true)
if [ -n "$AVAHI_COMMON" ]; then
    check_pass "libavahi-common found"
else
    check_warn "libavahi-common not found (NDI may not work properly)"
fi

AVAHI_CLIENT=$(ldconfig -p | grep libavahi-client.so || true)
if [ -n "$AVAHI_CLIENT" ]; then
    check_pass "libavahi-client found"
else
    check_warn "libavahi-client not found (NDI may not work properly)"
fi

echo ""

# 6. Display detailed NDI element information
if gst-inspect-1.0 ndisrc >/dev/null 2>&1; then
    log_info "NDI Source (ndisrc) properties:"
    gst-inspect-1.0 ndisrc | grep -A 50 "Element Properties:" | head -n 30

    echo ""
    log_info "NDI Sink (ndisink) properties:"
    gst-inspect-1.0 ndisink | grep -A 50 "Element Properties:" | head -n 30
fi

echo ""
echo "═══════════════════════════════════════════════════════════"

# Summary
if [ $ERRORS -eq 0 ] && [ $WARNINGS -eq 0 ]; then
    echo -e "${GREEN}✓ All checks passed!${NC}"
    echo ""
    echo "NDI is ready to use with Strom."
    echo "Next step: Run 4-test-ndi-output.sh and 5-test-ndi-input.sh"
    exit 0
elif [ $ERRORS -eq 0 ]; then
    echo -e "${YELLOW}✓ Installation complete with warnings${NC}"
    echo "  Errors: $ERRORS"
    echo "  Warnings: $WARNINGS"
    echo ""
    echo "NDI should work, but review warnings above."
    exit 0
else
    echo -e "${RED}✗ Installation verification failed${NC}"
    echo "  Errors: $ERRORS"
    echo "  Warnings: $WARNINGS"
    echo ""
    echo "Please fix the errors above before using NDI."
    exit 1
fi
