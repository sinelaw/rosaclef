#!/bin/sh
# Build the browser-only studio: a folder of static files (default: dist/)
# that any web server can host — GitHub Pages, Netlify, `python3 -m
# http.server` — at the domain's root or under a sub-path.
#
# The studio's back end (crates/local) runs in a worker, and projects are
# saved in the browser's storage. See docs/static.md.
#
#   tools/build-static.sh [OUT]          build (rebuilds the WebAssembly first)
#   SKIP_WASM=1 tools/build-static.sh    reuse web/engine and web/local as they are
set -e
cd "$(dirname "$0")/.."
out="${1:-dist}"
if [ -z "$SKIP_WASM" ]; then
  ./tools/build-wasm.sh
fi
rm -rf "$out"
mkdir -p "$out"
# The page, its modules and assets — not the type declarations and tests.
for d in engine fonts lib local locales soundfonts src styles tree vendor; do
  cp -R "web/$d" "$out/$d"
done
# Tell the page it has no server (skips probing for one).
awk '{ print } /<meta charset="utf-8">/ { print "  <meta name=\"rosaclef-backend\" content=\"local\">" }' web/index.html > "$out/index.html"
# GitHub Pages: serve files as they are.
touch "$out/.nojekyll"
echo "Built the browser-only studio in $out/ ($(du -sh "$out" | cut -f1))."
echo "Try it: python3 -m http.server -d $out 8080, then open http://localhost:8080"
