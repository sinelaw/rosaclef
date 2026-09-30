#!/bin/sh
# Type-check the frontend with inty (https://sinelaw.github.io/inty/).
# Install: cargo install --git https://github.com/sinelaw/inty inty-cli
# With arguments, checks just those files; otherwise every module, in one run.
set -e
cd "$(dirname "$0")"
LIBS="--lib types/globals.d.js"
if [ "$#" -gt 0 ]; then
  exec inty --no-color $LIBS "$@"
fi
exec inty --no-color $LIBS src/*.js src/ui/*.js test/*.js
