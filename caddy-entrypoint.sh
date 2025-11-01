#!/bin/sh
# Entrypoint script for Caddy that sets CADDY_PROTOCOL based on CADDY_TLS

# Default CADDY_TLS to true if not set
CADDY_TLS=${CADDY_TLS:-true}

# Set CADDY_PROTOCOL based on CADDY_TLS
if [ "$CADDY_TLS" = "false" ]; then
    export CADDY_PROTOCOL="http://"
else
    export CADDY_PROTOCOL=""
fi

# Start Caddy with the default command
exec caddy run --config /etc/caddy/Caddyfile --adapter caddyfile

