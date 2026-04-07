#!/bin/bash
# Entrypoint for strom Docker image
#
# Starts dbus and avahi-daemon for NDI network discovery.
# NDI uses mDNS (Avahi) to discover streams on the local network.
# Without Avahi, NDI sources/sinks work but discovery does not.

# Start dbus (required by avahi-daemon)
rm -f /run/dbus/pid
mkdir -p /run/dbus
dbus-daemon --system 2>/dev/null

# Start avahi-daemon for mDNS/NDI discovery
rm -f /run/avahi-daemon/pid
avahi-daemon -D 2>/dev/null

# Execute the command (defaults to /app/strom via CMD)
exec "$@"
