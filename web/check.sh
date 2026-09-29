#!/bin/sh
# Type-check the frontend with inty (https://sinelaw.github.io/inty/).
# Install: cargo install --git https://github.com/sinelaw/inty inty-cli
set -e
cd "$(dirname "$0")"
exec inty --no-color --lib types/globals.d.js src/main.js "$@"
