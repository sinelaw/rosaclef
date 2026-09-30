#!/bin/sh
# Type-check the frontend with inty (https://sinelaw.github.io/inty/).
# Install: cargo install --git https://github.com/sinelaw/inty inty-cli
set -e
cd "$(dirname "$0")"
LIBS="--lib types/newtypes.d.js --lib types/globals.d.js"
if [ "$#" -gt 0 ]; then
  exec inty --no-color $LIBS "$@"
fi
status=0
for entry in src/main.js test/tree.test.js; do
  if out=$(inty --no-color $LIBS "$entry" 2>&1); then
    echo "$out" | tail -n 1
  else
    echo "$out" | grep -v '^/\*\*' | head -n 60
    status=1
  fi
done
exit $status
