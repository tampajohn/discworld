#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --locked --release -p discworld-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/discworld_web.wasm
