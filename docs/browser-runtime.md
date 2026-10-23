# Browser runtime boundary

`discworld-web` exports `BrowserGame` through wasm-bindgen 0.2.129. It uses the
same core engine, original indexed renderer/fonts, speech and MIDI synthesis,
and combined save format as native macOS. The `web/` player now provides worker, canvas, audio and persistence hosts.
This is not yet an iPad acceptance result.

The direct constructor takes ISO and SoundFont bytes. The ISO is owned once in
Rust; resource ranges borrow it without copying the archives. The browser uses
`GameUpload` to copy File slices of at most 1 MiB into a reserved Rust buffer,
avoiding a second complete ISO ArrayBuffer in JavaScript. Upload size is bounded
to 1 GiB, incomplete/overflowing uploads are rejected, and parsing starts only
after all original bytes arrive. Actual peak memory on an iPad still needs measurement.
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
continuation, close and inventory scrolling through touch or keyboard. Default canvas input now delivers physical press/release edges to the shared
V1 classifier and timed world/inventory dispatch; explicit action controls
remain available. Title-bar dragging forwards held pointer/touch motion to the shared engine;
border/corner resizing is implemented; interrupted resize cases and complete original input parity remain unfinished. Pointer coordinates are mapped from the displayed canvas rectangle.
Canvas scaling preserves 8:5 aspect and the original indexed image/font output.

Manual saves use one IndexedDB slot per origin, committed as a transaction.
Export/import uses the same `.dws` format as native. Loading pauses presentation;
failed decoding preserves the worker's live state. Assets must be selected again
after a page reload; automatic asset persistence and autosave are not implemented.

### Browser evidence

Run `npm --prefix web ci` and
`npm --prefix web exec -- playwright install webkit` first.
Then `npm --prefix web test` runs Playwright projects against locally
installed Chrome and the downloaded Playwright WebKit, using the real ISO, SoundFont, inventory checkpoint and scrollable-inventory
checkpoint (see [Inventory scrollbar dragging](control.md#inventory-scrollbar-dragging)). The
fixture defaults are documented in `web/tests/player.spec.js`; missing assets
fail the test rather than silently skipping it. The server serves only `web/`.

The real browser tests in Chrome and Playwright WebKit at 1024×768 passed exact canvas-to-native PNG comparison,
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
and long-run scheduling, full-route interactive browser playback, broader touch
interactions, offline packaging, and deeper decoder robustness. Node and desktop
Chrome results do not establish iPad acceptance.

## Safari iPad simulator evidence

Safari 26.5 on the iPad Pro 11-inch (M5), iPadOS 26.5 simulator, successfully
loaded the same ISO/SoundFont and native inventory checkpoint. Its RGBA pixels
at tick 33,200 in `rinceroo.scn` exactly match the native image. Evidence:
`evidence/ipad-safari-simulator-render.json`, session capabilities in
`assets/logs/ipad-safari-session.json`, and screenshot
`assets/previews/ipad-safari-render-verified.png`. This run used an 834×1124
portrait viewport; landscape and physical-device performance remain unverified.

The installed Safari WebDriver exited when asked to populate a file input.
A replacement session used `scripts/serve-browser-fixtures.py`, which serves
only the explicitly supplied ISO, font and save on loopback fixture endpoints,
and `scripts/check-safari-simulator.py` to construct File inputs and submit the
form. This is a separate test harness, not the production file-selection flow.
The render checker accepts an existing Safari WebDriver session ID, the expected
PNG path, and the report destination. Start the fixture server with ISO, font
and save paths; its default address is `http://127.0.0.1:8767`.

WebDriver click/touch requests in this simulator returned without delivering
DOM pointer/click events; scripted Play did not unlock AudioContext. Therefore
this result proves loading and rendering only, not trusted gestures, file-picker
usability, audio playback or an iPad playthrough. Button/canvas text selection
and touch callouts are disabled to keep browser selection out of the game UI;
this does not resolve the simulator's missing input events.

Safari simulator sessions use `platformName: iOS`, `safari:useSimulator: true`
and a specific `safari:deviceUDID`, as described in
[WebKit's Safari WebDriver documentation](https://webkit.org/blog/9395/webdriver-is-coming-to-safari-in-ios-13/).
Playwright WebKit tests are a distinct result and passed their gesture/audio
checks; they are not substituted for simulator or physical-iPad acceptance.

## Full-route WASM validation harness

`discworld-replay` now owns the existing strict ordinary-input decoder and
application logic. Native tools re-export that same module. The web crate's
optional `replay-validation` feature exposes one-input-at-the-current-tick
validation and restart status. These exports are absent from the regular player.
Accepted inputs reset the diagnostic dispatch ring exactly as the headless host
does; this changes no original scripts or puzzle state.

```sh
cargo build --release -p discworld-web --features replay-validation --target wasm32-unknown-unknown
wasm-bindgen --target nodejs --out-dir target/web-replay-node \
  target/wasm32-unknown-unknown/release/discworld_web.wasm
node scripts/run-wasm-replay.cjs target/web-replay-node/discworld_web.js \
  /path/to/discworld.iso /path/to/gm.sf2 evidence/act4-ending-input.json \
  evidence/new-wasm-ending
```

The runner uses the unchanged 1,652-event route, the same start event at tick 500,
and the same 2,630,000 final tick as native. It verifies each event at its exact
tick and scene, advances the real engine and synthesizer, checks every returned
sample for finiteness, and writes the final RGBA, combined save and report. Scene
history is sampled after batches of at most 24 ticks and may miss brief scenes.
It is Node/WASM runtime validation, not browser input, audio-device or iPad testing.

`scripts/compare-wasm-replay.cjs` compares those outputs against a fresh native
headless run with `--save`: complete core-envelope bytes, PNG-decoded RGBA,
accepted-input counts, tick, scene and restart. It reports combined DSP save
identity separately; cross-platform audio equality is not assumed. It also
writes a viewable PNG of the WASM framebuffer. The first full run at `708771b` completed and compared successfully. It accepted
all 1,652 inputs, reached title/restart at 2,630,000, and matched the fresh native
run's complete serialized engine envelope and final RGBA exactly. All
4,832,625,000 returned stereo samples were finite. The run took 526.56 seconds on
this Mac. Evidence: `evidence/wasm-full-ending/report.json` and
`evidence/wasm-full-ending/native-comparison.json`; native report
`evidence/wasm-comparison-native.json`. The resulting
`evidence/wasm-full-ending/final.png` was inspected and shows the original title.
The engine-envelope SHA-256 is
`62570b31e8aca0793e461792be622810dbcb8c3d980db53513cb2f474d374d64`.
The combined saves differ in DSP state, consistent with the previously measured
cross-platform audio differences. This verifies the recorded route in Node/WASM,
not every interaction or a full browser/iPad playthrough.

The browser AudioContext interruption test passes in Chrome and Playwright
WebKit: suspending the actual context pauses simulation, ticks remain stable,
and Play resumes the context and engine. This tests the statechange path;
real OS/device interruptions and background-tab recovery remain separate gates.

## Full-route browser worker harness

The same route can run in an actual Chrome or Playwright WebKit module worker.
Build its separate validation bindings, serve `web/` on loopback port 8765, and
run (the output directory must not already exist):

```sh
scripts/build-web-replay.sh
node scripts/run-browser-replay.cjs webkit \
  /path/to/discworld.iso /path/to/gm.sf2 evidence/act4-ending-input.json \
  evidence/new-webkit-ending
node scripts/compare-wasm-replay.cjs evidence/new-webkit-ending \
  /path/to/native-final.dws /path/to/native-final.png \
  /path/to/native-report.json
```

Use `chrome` in place of `webkit` for installed Chrome. Both use the same
original input decoder and exact event ticks as the Node runner. Local File
objects go to the worker, the ISO is loaded in 1 MiB chunks, and progress frames
are drawn on the browser canvas. The runner writes the final save, RGBA,
browser canvas screenshot, browser version and execution report. The comparison
requires complete engine state and final pixels to match the native baseline.

The validation module is separate from the normal player and its generated
bindings live in ignored `web/replay-pkg/`. It advances faster than real time;
PCM is generated and checked for finite values but is not sent through Web Audio.
A successful route therefore proves browser-worker runtime execution, not
manual touch operation, continuous audio output, or physical iPad acceptance.

The first Playwright WebKit 27.2 run completed successfully: all 1,652 original
inputs were accepted, credits completed and the game restarted to `title.scn`
at tick 2,630,000. Complete serialized engine state and final RGBA match the
fresh native baseline exactly. All 4,832,625,000 returned samples were finite;
combined saves differ in DSP state, so audio equality is not established.
Elapsed simulation time was 537.094 seconds on this Mac. Evidence:
`evidence/webkit-full-ending/report.json` and `native-comparison.json`;
`browser-final.png` was inspected and shows the original title. This used the
validation harness committed as `3704361`, with unchanged original scripts and
route. Chrome 156.0.8078.47 also passed the same complete route in 511.916 seconds:
all 1,652 inputs accepted, exact native engine state and RGBA, and all samples
finite, with combined DSP save differences. Its corresponding evidence is in
`evidence/chrome-full-ending/`; its browser screenshot was also inspected.
Neither result is physical Safari/iPad acceptance. Both full runs used the
pre-refactor replay build; the subsequent shared primary-click change affects
interactive clicks, not the semantic replay events.

After sharing primary-click routing between native and browser, all 232 enabled
workspace tests, Clippy, six browser scenarios and two clock regressions passed.
The unrelated asset-dependent DSP test remains ignored in the default workspace
suite. Native was rebuilt and the visible credits checkpoint reloaded, paused
and muted, with its original save unchanged. Screenshot:
`assets/previews/credits-primary-click-current-native.png`.

## Upload memory evidence

`scripts/check-upload-memory.cjs` runs bulk and chunked uploads in separate Node
processes, loads the same native inventory save, and requires a byte-identical
re-encoded save. Both passed. On this Mac with the 396,863,488-byte ISO, peak RSS
was 862,960 KiB for bulk loading and 476,784 KiB for chunked loading (about
843 MiB versus 466 MiB). Elapsed times were 6.39 and 6.46 seconds respectively.
Evidence: `evidence/wasm-upload-bulk.json` and `evidence/wasm-upload-chunks.json`.
These are host-process measurements, not an iPad memory limit or performance claim.
The Rust upload regression rejects incomplete, oversized and overflowing input
without partially appending a rejected chunk. The direct constructor remains
available for callers that already own an in-memory ISO.

After switching the browser worker to chunked loading, all six browser tests
(three scenarios each in Chrome and Playwright WebKit) and both clock regressions
passed. The web crate's upload regression and Clippy checks also passed. The
normal browser build's generated interface contains no replay-validation exports.

## Physical pointer evidence

After connecting physical down/up events, the six browser scenarios and two
clock tests passed. The strengthened inventory scenario additionally requires
core save version 4 after a real touch and exact import/export of that save,
including mouse history; this passes in both Chrome and WebKit. The WebKit
screenshot was inspected and shows the inventory closed. The test now waits for
the Play button to become Pause before tapping: previously a tap could race
AudioContext resume and be ignored while input was still paused, and merely
checking changed save bytes did not prove touch delivery. This is one inventory
gesture, not complete touch or physical-iPad acceptance.


Full browser replay revalidated runtime8702043 after the node-exit and inventory
hover ordering fixes. Chrome156.0.8078.47 (504.338s) and Playwright WebKit27.2
(534.320s) each accepted all1652 unchanged inputs from startup through credits,
then restarted to title.scn at2630000. Both complete serialized engine states
and final RGBA match the corrected native route exactly. Both browser title
screenshots were inspected. Each generated4,832,625,000 finite audio samples;
combined DSP saves differ, so cross-platform audio equality is not established.
Evidence: evidence/{chrome,webkit}-hover-full-ending/{report,native-comparison}.json
and browser-final.png. The native comparison report combines the measured283
prefix and1369 continuation inputs in assets/logs/hover-order-native-comparison.json,
with explicit source/checkpoint provenance. Replay uses the separate
replay-validation WASM build and accelerated semantic inputs; it does not prove
a full physical-input route, Web Audio playback or physical iPad acceptance.
No additional engine changes were needed for these full browser checks.


Browser slot persistence now uses the same Rust DWRSLOT codec through WASM
encode_slot/decode_slot. storage.js upgrades discworld-player IndexedDB to
version2, preserving the existing saves/manual record and adding a slots store.
New-slot selection and full-record publication share one readwrite transaction,
so concurrent tabs cannot select the same free ID. Explicit overwrites are
transactional; corrupt occupied keys remain reserved and are reported separately.
Catalog reads decode one cursor record at a time and retain only metadata, rather
than retaining all complete sessions. Timestamp input is a checked unsigned
64-bit BigInt; game-font descriptions remain bytes. Rust/IndexedDB storage
functions are available, but live menu actions are not connected yet.

All22 Chrome/WebKit tests plus two clock tests pass in
assets/logs/browser-slots-tests.log. New tests exercise v1 manual-save migration,
reload persistence, two-tab concurrent allocation, failed overwrite preservation,
corrupt slot reporting/reservation and timestamp ordering. The existing18 player
checks also passed after manual deviceSave moved into the shared storage module.
282 enabled workspace tests and Clippy pass. Browser artifacts
assets/saves/browser-{chrome,webkit}-slot.dwslot decode with native inspect-slot;
both envelopes are identical and their extracted sessions exactly match the
original inventory checkpoint. Full resource/SoundFont restoration and24 ticks
of native continuation yield identical engine/DSP saves and PNGs at33224,
Running/rinceroo.scn. The loaded inventory image was inspected. Evidence:
assets/logs/browser-{chrome,webkit}-slot-{native,loaded}.json and matching loaded
images under assets/previews. inspect-slot SLOT [FRESH_SESSION_OUTPUT] validates
the record and optionally exports the unchanged session with no-replace atomic
publication. Browser manual-save behavior remains available; original F1/menu
integration, settings persistence, catalog asset filtering and physical iPad
acceptance are still unfinished.

Original menus are now connected to the browser worker. F1 or the Options button
opens Main; F5/F7 open Save/Load; Escape returns from submenus. Physical mouse
edges, drag motion, touch actions, wheel/page scrolling and ASCII keyboard text
entry use the shared controller and original fonts. The worker advances menu
coroutines with the game and handles requests before accepting the next queued
command. Save/load requests resolve stable IndexedDB slot IDs; the catalog checks
complete sessions against the current ISO/SoundFont before displaying them and
loads validate again before replacing engine/DSP. Invalid slots remain reserved.
Loading/restarting clears queued presentation audio. Quit pauses the browser
player rather than attempting to close the tab.

Escape-generated settings writes use IndexedDB preferences. Startup validates
preferences before applying them to an engine without a saved controller; saved
controllers keep their own settings. Storage initialization failures leave play
and file export available, with a visible notice. Existing manual device saves
and file import/export remain separate from the named menu slots.

`web/tests/menu-live.spec.js` drives the actual UI in Chrome and WebKit: it opens
Save with F5, types a name, activates the original confirmation button, checks
the stored description, and reloads through F7. Captures are
assets/previews/browser-{chrome,webkit}-save-name.png. This verifies desktop
keyboard/mouse menu operation; virtual-keyboard name entry and physical landscape
iPad acceptance are still pending. No new full start-to-credits run is claimed.

Validation for this integration: workspace tests, Clippy, WASM build and all 14
original-asset Rust web tests pass. The full browser run passed all 24 existing
cases and skipped two opt-in native-menu-checkpoint cases. Its new WebKit menu
case exposed an unnecessary test click on the already-selected new-save row,
which could trigger the preserved double-click save shortcut before typing.
After removing that extra click, the new case passed three repetitions in each
browser (six passes). Logs: assets/logs/browser-menu-{workspace,clippy,build,
assets,browser-suite,live}.log. The test also waits for audio startup before
sending menu keys, so a key cannot be dropped while the host is still paused.

Touch save-name entry: Save exposes a `Type save name` button that synchronously
focuses a text-input endpoint from the user's gesture. The endpoint is visually
transparent; the engine continues drawing the actual name with its original
font. Input events forward ASCII insertion/backspace to the shared editor;
IME composition waits for commitment, duplicate composition events are consumed,
and paste/input fallback is bounded. Autocorrection, capitalization and spelling
suggestions are disabled. Leaving Save or pausing blurs the endpoint. Enter or
Escape dismisses keyboard focus without submitting a save.

This work also found and fixed a native/browser host bug: both passed byte zero
for backspace, but the reference-derived editor ignores zero before inspecting
the backspace flag. Both hosts now pass ASCII backspace (8). Desktop and touch
menu tests insert a deliberately wrong character, delete it, finish the name,
save and reload. Touch cases navigate Options -> Save and Options -> Load without
function keys. All four Chrome/WebKit menu cases pass. The text endpoint's DOM
event test separately exercises composition, deletion and paste fallback.
These are emulated touch/input tests: a physical iPad must still verify keyboard
appearance, visual-viewport resizing, focus and landscape usability.

Restored drag ownership now reports the required physical release button from
WASM. Browser release fallback previously assumed left, so a drag saved with
swapped buttons could remain active after the user's right-button release.
The host now matches the configured physical button, clears old pointer ownership
when loading/restarting, and permits unowned fallback only on pointerup (not the
following lostpointercapture notification).

A Chrome/WebKit regression seeds swapped-button preferences, begins an actual
right-button inventory drag, exports/reloads the active drag byte-for-byte,
checks that a left release leaves it active, then checks that a right release
ends it. Existing ordinary header-drag save/load cases also pass, including
Chrome's real browser touch dispatch. Four targeted browser cases, workspace
tests, Clippy and WASM build pass; logs: assets/logs/swapped-drag-*.log.
This is browser input lifecycle evidence, not physical iPad acceptance.


## Standalone browser distribution

`scripts/package-web.py OUTPUT.zip` builds from a clean tracked checkout with
locked Cargo dependencies and wasm-bindgen-cli 0.2.129. It includes only the
static host files, generated WASM/glue, LICENSE/NOTICE, instructions, a SHA-256
manifest and a `git archive` of the exact source revision. Existing destinations
are rejected. Game data and the user music SoundFont are excluded; the source
archive retains the vendored synthesizer's tiny empty SoundFont test fixture.
Untracked files are not included in the source archive.

The local development bundle `assets/packages/discworld-web-d8e6f1e.zip` is pinned
to d8e6f1e235e0f713496cd08196f7f99e2375c9f8 (2,826,559 bytes; SHA-256
`0cfb529673b03f1fa67f9c350921425f0d8e16603a770d8291b28e1d2d0738ce`).
All 14 manifested files passed hash checks; the source archive matched a fresh
archive of that revision byte-for-byte. An attempted second publication to the
same path was rejected.

The extracted bundle was served from `assets/packages/verified-d8e6f1e`, outside
`web/`, with no source-directory HTTP fallback. The existing production-player
integration test passed in both Chrome and Playwright WebKit (2/2). It checks
native save import and exact export, native inventory pixels, simulation and
finite nonzero audio scheduling, pause/mute controls, local save persistence
across page reload, corrupt-save rejection without state replacement, landscape
canvas bounds and actual touch input with save roundtrip. Evidence is in
`assets/logs/browser-package-test.log`; the isolated test configuration is
`assets/packages/package.config.mjs`. This does not establish physical iPad
acceptance, full manual gameplay coverage or native application packaging.

## Page suspension during an inventory drag

The host now pauses and releases browser-owned pointers on `pagehide` or a
hidden-document `visibilitychange`. Releases are sent while paused, so they
cannot move the inventory window or cursor. Pointer ownership is cleared
before releasing capture; a later lost-capture or physical-up event does not
repeat the release. Manual Pause continues to preserve an active drag for
save/export, as before. Imported drags with no host-owned pointer are not
implicitly changed by suspension.

A new original-asset Playwright regression reproduced a stuck inventory drag
in Chrome and WebKit before the fix. It dispatches pagehide and hidden-page
events while a slider is held, checks drag release and paused tick/pixel
stability after a late physical release, then resumes simulation. These are
synthetic lifecycle events in desktop browsers, not physical iPad background,
orientation, operating-system suspension or device-audio acceptance.

The manual-pause regression waits for Play's asynchronous audio startup and
continues checking exact save import/export bytes, pixels and drag release.
Its obsolete fixed outer-version assertion was removed: a pending hover can
legitimately encode the same slider state using version 9 rather than 6.
Save-format versions and malformed input remain covered by core tests.
Logs: `assets/logs/lifecycle-drag-*`.
All six targeted Chrome/WebKit lifecycle and manual-pause/save cases pass
(`lifecycle-drag-final.log`); JavaScript syntax and whitespace checks pass.


## Desktop tablet-viewport rotation regression

Chrome and WebKit pass the paused original-inventory viewport sequence
1024x768 → 768x1024 → 1133x744 → 1180x820 → 1024x600. Every size preserves
original framebuffer pixels, tick 33200, and the 8:5 canvas aspect ratio.
Landscape checks place the complete game-control navigation inside the
viewport and verify all visible control heights are at least 44 CSS pixels.
No layout change was needed. Logs: `assets/logs/tablet-rotation.log` (two
browser cases). This does not simulate physical safe-area insets, browser
chrome, keyboard appearance or operating-system rotation events; those remain
part of real-iPad acceptance.

The complete current browser suite passed: four Node clock/audio tests and all
44 Playwright cases across Chrome/WebKit, including lifecycle, tablet viewport,
menus, original saves, audio interruption and storage. Log:
`assets/logs/browser-full-current.log`. The run used the 27d9924 engine with
a2b6513 lifecycle changes and cc0fcfa viewport tests; it predates WALKPOLY
runtime integration. Physical-device acceptance remains outstanding.


## Current extracted browser package

`assets/packages/discworld-web-d39a279.zip` contains runtime d39a279. All
manifested files and the exact source archive pass hash checks after extraction
outside the checkout. See [package evidence](validation/packages-d39a279.json).
All 44 Chrome/WebKit UI cases pass against an HTTP server rooted in that
extracted package. Hashes were rechecked after the tests. This covers existing
menu, save/load, inventory, lifecycle, audio and tablet-viewport regressions,
not a browser full-route replay or physical iPad acceptance.

## Focus and audio interruption during a drag

Window `blur` now uses the same owned-pointer cleanup as hidden-page/pagehide
suspension. An actual AudioContext state change away from running also uses
that cleanup; previously it paused simulation but retained the held slider.
Both paths leave the game paused until Play is pressed. Manual Pause still
preserves active drags for save/export, and an imported drag with no physical
pointer owner is not implicitly released.

The added original-asset regressions reproduce both defects in Chrome and
WebKit before correction: blur left Play labelled Pause, and audio interruption
left the inventory drag active. The checks hold the original inventory slider,
interrupt, verify release and stable paused ticks/pixels after a late pointer-up,
then resume. Blur is a synthetic window event; audio uses the real browser
AudioContext suspend operation. These do not prove physical iPad interruption
or touch delivery behavior.

Validation: all 12 targeted interruption/viewport cases, two manual-pause active-save
cases and four audio/clock Node tests pass. See [evidence](validation/browser-focus-audio-interruption.json).

## Multiple simultaneous touches

The canvas accepts only the primary touch pointer for the game's single cursor.
Secondary fingers cannot press, move or release the game pointer, including the
release fallback used for a restored drag. Mouse and pen handling is unchanged.
When the primary finger lifts, a remaining secondary finger is not promoted by
the host; lift all fingers before starting another touch gesture.

A Chromium CDP test uses real browser-dispatched simultaneous touches on the
original inventory slider. Before the fix, a second finger added a second
`left-down` and a `left-up`. The regression checks that the second contact and
release produce no extra button commands, the primary drag stays active, and
lifting the primary finger releases it exactly once. Playwright WebKit has no
multi-contact touch injection API, so that case is explicitly skipped there;
physical Safari/iPad multi-touch validation remains open.

The final multi-touch regression and all 12 existing targeted drag/interruption
cases pass. The test verifies the actual secondary pointer-up event before
checking game commands; see [evidence](validation/browser-primary-touch.json).

## Browser package with interruption and primary-touch fixes

`assets/packages/discworld-web-a4020ae.zip` includes focus/audio interruption
cleanup and primary-touch ownership. All 14 manifested files and the exact
tracked source archive were verified after extraction outside the checkout.
The extracted site passed 17 original-asset Chrome/WebKit checks covering cold
startup, save import/export, local persistence, audio controls, inventory drags,
interruption/resume and Chromium multi-touch. One WebKit multi-touch case is
explicitly skipped because its automation API cannot inject multiple contacts.
See [package evidence](validation/browser-package-a4020ae.json).

This is a static development bundle; serve its extracted directory over local
HTTP or HTTPS as described in its README. Physical iPad landscape acceptance,
visible native/Finder interaction and broader original-engine fidelity remain
open. The browser-host fixes do not change the deterministic engine validated
by the `0a87557` full-route audit.

The cached-click correction is now included in
`assets/packages/discworld-web-07e395f.zip`. After extraction outside the
checkout, all 14 manifest hashes and exact source were verified. Chrome and
WebKit both pass fresh startup, legacy-save loading, export and audio-headroom
controls without page errors. Full-route audits of frozen modules from this
revision passed; see [route evidence](validation/cached-click-route.json). See
[package evidence](validation/packages-07e395f.json).

`assets/packages/discworld-web-9186705.zip` includes the inventory trail
movement correction. The extracted package passes manifest/source checks and
Chrome/WebKit startup, legacy-load, save-export and audio-headroom smoke checks.
Full-route audits of separately frozen modules from 9186705 passed; see
[route evidence](validation/inventory-trails-route.json).
See [package evidence](validation/packages-9186705.json).


The `cc0e3db` browser package includes runtime `30c2f69` with synchronous inventory
cursor updates. All 14 extracted manifest files and bundled source verify.
Chrome 156.0.8078.47 and WebKit 27.2 pass startup, legacy save load, save export
and audio-headroom checks without page errors. The host UI suite passes 49 cases
and four Node tests; one unsupported WebKit multi-touch injection is skipped.
Full-route audits pass in native, Chrome and WebKit with identical final engine
state/pixels, all 1,652 inputs accepted and 5,048 engine plus 5,048 DSP save/load
checks. See [route evidence](validation/resize-cursor-route.json). Physical iPad
acceptance remains open. See [package evidence](validation/packages-cc0e3db.json).


Package `ea6d560` contains runtime `f5b9a91` with corrected hidden-cursor trail
lifetime. An isolated checkout kept the ongoing UI suite's files unchanged.
All 14 extracted manifest files and exact source verify. Chrome and WebKit pass
startup, legacy loading, save export and audio-headroom checks with no page
errors. Full-route audits now pass with identical final engine state and pixels
in native, Chrome and WebKit; see [route evidence](validation/hidden-trails-route.json).
The UI suite passes 49 cases with one known WebKit multi-touch injection skip,
plus four Node tests. Physical iPad acceptance remains open. See [package evidence](validation/packages-ea6d560.json).
