# Browser runtime boundary

`discworld-web` exports `BrowserGame` through wasm-bindgen 0.2.129. It uses the
same core engine, original indexed renderer/fonts, speech and MIDI synthesis,
and combined save format as native macOS. The `web/` player now provides worker, canvas, audio and persistence hosts.
This is not yet an iPad acceptance result.

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

## Browser host

Build with `scripts/build-web.sh` and serve only `web/` using a local HTTP server.
Generated bindings are ignored; game assets are never served or uploaded by the
player. The file picker passes local File objects to a module worker, which
serializes all requests, including asynchronous initialization. The main thread
presents transferred RGBA frames and schedules stereo Web Audio buffers. Playback
starts from a user gesture, defaults muted, and keeps synthesizing while muted.
Pausing stops queued sources; an epoch prevents in-flight audio from playing
after pause/load. Hidden pages and interrupted audio pause the simulation.

The player supports explicit walk/select, action, look, pockets, conversation
continuation, close and inventory scrolling through touch or keyboard. This is
the same development input vocabulary as native, not original double-click
arbitration. Pointer coordinates are mapped from the displayed canvas rectangle.
Canvas scaling preserves 8:5 aspect and the original indexed image/font output.

Manual saves use one IndexedDB slot per origin, committed as a transaction.
Export/import uses the same `.dws` format as native. Loading pauses presentation;
failed decoding preserves the worker's live state. Assets must be selected again
after a page reload; automatic asset persistence and autosave are not implemented.

### Browser evidence

`npm --prefix web ci` then `npm --prefix web test` runs Playwright against locally
installed Chrome, using the real ISO, SoundFont and inventory checkpoint. The
fixture defaults are documented in `web/tests/player.spec.js`; missing assets
fail the test rather than silently skipping it. The server serves only `web/`.

The real browser test at 1024×768 passed exact canvas-to-native PNG comparison,
byte-identical native save import/export, pause stability, nonzero finite stereo
22,050-Hz buffer scheduling, sound toggles, IndexedDB save/load across page reload,
and corrupt-file rejection with the original live save unchanged. It checked
canvas bounds/aspect and delivered an actual touch event outside the inventory.
Screenshots `assets/previews/browser-inventory-loaded.png` and
`assets/previews/browser-touch-close-inventory.png` were inspected: the inventory
background/border is present initially and the window closes after the touch.
Buffer scheduling is not an acoustic or uninterrupted-playback quality check.

A second real-browser test starts without a save, reaches the original title,
uses Continue, and enters the opening sequence. Its screenshot
`assets/previews/browser-cold-start.png` was inspected. This test caught and fixed
a resume-clock bug: a queued RAF timestamp can precede a click's performance.now()
timestamp. Negative elapsed time had become a negative tick count and wrapped
at the WASM u32 boundary. The clock now clamps negative deltas and bounds catch-up
to four ticks; two Node regression tests cover that boundary and stalled frames.
Both browser tests and both clock tests pass. No game scripts changed.


Remaining gates include real Safari/iPad memory/performance, audio interruption
and long-run scheduling, full-route WASM/browser verification, broader touch
interactions, offline packaging, and deeper decoder robustness. Node and desktop
Chrome results do not establish iPad acceptance.
