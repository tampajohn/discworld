# Browser runtime boundary

`discworld-web` exports `BrowserGame` through wasm-bindgen 0.2.129. It uses the
same core engine, original indexed renderer/fonts, speech and MIDI synthesis,
and combined save format as native macOS. This is an executable WASM boundary,
not yet a browser player or an iPad acceptance result.

The constructor takes ISO and SoundFont bytes. The ISO is owned once in Rust;
resource ranges borrow it without copying the archives. JavaScript-to-WASM
transfer still copies the input, so peak memory on an iPad must be measured.
Resource, text and music views are rebuilt per advance call; their cost needs
profiling on the device. No unsafe application Rust is introduced.

`advance(n)` runs at most 24 ticks and returns interleaved stereo f32 PCM at
22,050 Hz. Simulation runs at 24 ticks/second. `frame()` returns the original
320×200 RGBA image. Muting should discard presentation audio, not stop advancing
the synthesizer. `move_pointer(x,y)` clamps coordinates to the game viewport.
`input(command)` supports start, continue, click, walk, action, look, pockets,
close, up and down, with development semantics matching the native controls.
`save()` returns a combined file; `load(bytes)` prepares both engine and DSP
before replacing them. Failed loads preserve the live session. Runtime tick
failures stop advancement until a successful load.

## Build and exercise actual WASM

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo build --release -p discworld-web --target wasm32-unknown-unknown
wasm-bindgen --target nodejs --out-dir target/web-node \
  target/wasm32-unknown-unknown/release/discworld_web.wasm
cargo run --release -p discworld-web --example bridge_fixture -- \
  /path/to/discworld.iso /path/to/gm.sf2 /path/to/checkpoint.dws \
  evidence/new-native-reference
node scripts/check-wasm-bridge.cjs target/web-node/discworld_web.js \
  /path/to/discworld.iso /path/to/gm.sf2 /path/to/checkpoint.dws \
  evidence/new-native-reference
```

The reference directory must not exist. Native and Node/WASM each load the same
save, advance 24 ticks, and compare complete engine state, pixels and audio.
The script requires exact engine/pixel equivalence and exact reload continuation
within WASM. It also checks corrupt-file rejection and invalid command/batch
rejection without changing the live state. Audio differences are measured and
reported; add `--strict-dsp` to require exact cross-platform DSP/audio equality.
A normal exit without that flag does not establish audio equivalence.

Actual results on this Mac and Node 26.12.0:

| Checkpoint | Engine / RGBA | Audio samples differing | Maximum absolute error | Full combined save |
| --- | --- | --- | --- | --- |
| Opening, 10000→10024 | Exact | 66 / 44100 | 1.4901161e-8 | Exact |
| Open Luggage, 33200→33224 | Exact | 32651 / 44100 | 6.3329935e-7 | DSP differs |

Both passed exact within-WASM save/reload continuation and failed-load isolation.
Evidence is in `evidence/wasm-native-opening-reference-v2/wasm-verification.json`
and `evidence/wasm-native-inventory-reference/wasm-verification.json`.
The first strict audio comparison failed. These small floating-point differences
have not been traced to a specific operation; no cross-platform bit-exact DSP
claim or audible-fidelity acceptance follows from these measurements.

Next gates: browser worker and canvas presentation, gesture-enabled audio,
local asset selection and persistence, suspend/resume, landscape touch controls,
full route verification in WASM, profiling and real Safari/iPad testing. The
current Node checks do not exercise DOM, Web Audio, browser storage or touch.
