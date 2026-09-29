#!/bin/sh
# Build the WebAssembly engine used by the browser (web/engine/rosaclef.wasm).
set -e
cd "$(dirname "$0")/.."
cargo build -p rosaclef-wasm --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/rosaclef_wasm.wasm web/engine/rosaclef.wasm
ls -la web/engine/rosaclef.wasm
