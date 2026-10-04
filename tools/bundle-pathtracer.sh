#!/bin/sh
# Bundle the film's path tracer (three.js, three-mesh-bvh, three-gpu-pathtracer)
# into one ES module: web/vendor/pathtracer/pathtracer.mjs. Only what
# tools/pathtracer-entry.js exports is kept (no WebGPU renderer).
set -e
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
cp tools/pathtracer-entry.js "$tmp/entry.js"
cd "$tmp"
echo '{"name":"pt","private":true,"type":"module"}' > package.json
npm install --no-save --no-package-lock three@0.186.1 three-mesh-bvh@0.9.15 three-gpu-pathtracer@0.0.26 esbuild@0.25.10 > /dev/null
npx esbuild entry.js --bundle --format=esm --minify --legal-comments=eof --outfile=pathtracer.mjs
cd - > /dev/null
cp "$tmp/pathtracer.mjs" web/vendor/pathtracer/pathtracer.mjs
rm -rf "$tmp"
ls -la web/vendor/pathtracer/pathtracer.mjs
