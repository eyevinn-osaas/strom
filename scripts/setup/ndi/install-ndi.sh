#!/usr/bin/env bash
#
# Install NDI SDK and GStreamer NDI plugin
#
# Convenience script that runs the install steps in order.
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

"$SCRIPT_DIR/1-install-ndi-sdk.sh"
"$SCRIPT_DIR/2-install-gstreamer-ndi-plugin.sh"
