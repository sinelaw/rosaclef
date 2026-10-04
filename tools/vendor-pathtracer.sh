#!/usr/bin/env bash
# Rebuild web/vendor/pathtracer/pathtracer.js: three-gpu-pathtracer's
# WebGLPathTracer (with three-mesh-bvh), bundled and minified against the
# three.js vendored in web/vendor/three (not a copy of its own).
#
#   tools/vendor-pathtracer.sh
set -euo pipefail
cd "$(dirname "$0")/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
(
  cd "$work"
  npm init -y >/dev/null
  npm install --no-audit --no-fund three@0.186.1 three-mesh-bvh@0.9.15 three-gpu-pathtracer@0.0.26 esbuild@0.24.0 >/dev/null
  echo 'export { WebGLPathTracer } from "three-gpu-pathtracer";' >entry.js
  echo 'export default {};' >xatlas-stub.js
  cat >build.mjs <<'EOF'
import { build } from "esbuild";
await build({
  entryPoints: ["entry.js"],
  bundle: true,
  format: "esm",
  minify: true,
  outfile: "pathtracer.js",
  legalComments: "none",
  plugins: [
    {
      name: "shared-three",
      setup(b) {
        // three.js itself is the one vendored beside this bundle.
        b.onResolve({ filter: /^three$/ }, () => ({ path: "../three/three.module.js", external: true }));
        // Lightmap baking (not used) needs xatlas: a stub.
        b.onResolve({ filter: /^xatlas-web/ }, () => ({ path: new URL("./xatlas-stub.js", import.meta.url).pathname }));
      },
    },
  ],
});
EOF
  node build.mjs
)
cp "$work/pathtracer.js" web/vendor/pathtracer/pathtracer.js
echo "web/vendor/pathtracer/pathtracer.js rebuilt"
