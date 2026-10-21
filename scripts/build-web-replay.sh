#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release -p discworld-web --features replay-validation --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir web/replay-pkg target/wasm32-unknown-unknown/release/discworld_web.wasm
