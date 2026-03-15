#!/bin/sh
# Build the WASM binary and place it alongside the static site.
# After running, serve musicbox-web/www/ with any HTTP server.
set -e

cd "$(dirname "$0")"
wasm-pack build --target web --out-dir www/pkg
echo ""
echo "Build complete. Serve www/ to play in browser."
