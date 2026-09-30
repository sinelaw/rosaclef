#!/bin/sh
# Refresh the vendored xterm.js build in web/vendor/xterm (no bundler needed).
set -e
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
(cd "$tmp" && npm pack @xterm/xterm @xterm/addon-fit @xterm/addon-web-links >/dev/null && for f in *.tgz; do mkdir "${f%.tgz}" && tar xzf "$f" -C "${f%.tgz}"; done)
d=web/vendor/xterm
cp "$tmp"/xterm-xterm-*/package/lib/xterm.mjs "$tmp"/xterm-xterm-*/package/css/xterm.css "$tmp"/xterm-xterm-*/package/LICENSE "$d"/
cp "$tmp"/xterm-addon-fit-*/package/lib/addon-fit.mjs "$tmp"/xterm-addon-web-links-*/package/lib/addon-web-links.mjs "$d"/
sed -i '/sourceMappingURL/d' "$d"/*.mjs
rm -rf "$tmp"
ls -la "$d"
