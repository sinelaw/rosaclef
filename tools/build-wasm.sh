#!/bin/sh
# Build the WebAssembly modules used by the browser:
#   web/engine/rosaclef.wasm        the audio engine (runs in the AudioWorklet)
#   web/local/rosaclef-local.wasm   the studio back end of the browser-only
#                                   build (runs in a worker; see docs/static.md)
set -e
cd "$(dirname "$0")/.."
cargo build -p rosaclef-wasm -p rosaclef-local --target wasm32-unknown-unknown --profile wasm
out=target/wasm32-unknown-unknown/wasm
mkdir -p web/local
cp $out/rosaclef_wasm.wasm web/engine/rosaclef.wasm
cp $out/rosaclef_local.wasm web/local/rosaclef-local.wasm
ls -la web/engine/rosaclef.wasm web/local/rosaclef-local.wasm
