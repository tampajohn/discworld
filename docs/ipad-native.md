# Native iPad development build

The requested iPad test deployment is a native application with the supplied
ISO and music SoundFont bundled. One installation must be sufficient to start
playing offline; no browser, file import, or separate sound download is required
for this internal test configuration. The browser host remains supported.

## Initial application prototype

`platform/ipad/App.swift` is a native UIKit host linked to the Rust session
through `discworld-ipad`. It copies the original framebuffer for presentation
and supplies generated stereo PCM to AVAudioEngine. No web view is used.

```sh
python3 scripts/build-ipad.py --output assets/apps/Discworld-iPad.app
xcrun simctl install booted assets/apps/Discworld-iPad.app
xcrun simctl launch booted org.tampajohn.discworld.dev
```

The script currently builds **only for the Apple Silicon simulator**. It
includes the staged ISO and SoundFont in one ad-hoc-signed app. The observed
prototype is `assets/apps/Discworld-iPad-8.app`. Startup reaches the bedroom;
quick-save/load preserves pixels, and a separate app launch loads the same
save from the application container. See [application evidence](validation/ipad-app-prototype.json).

Tap modes, mute, pause, and quick-save/load controls are implemented. XCTest
now activates Save, Resume/Pause, Sound off/on and Load through synthesized
taps. Native audio-player completion callbacks occur; muting preserves ongoing
simulation. Both immediate load and a separate app launch restore the saved
tick and exact framebuffer. See [control evidence](validation/ipad-native-controls.json).
This does not establish acoustic quality or physical-device input/audio behavior.
One landscape game-canvas tap now moves Rincewind from `[200,178]` to
`[160,185]`, with the actual cursor and completed mover state checked through
read-only native inspection. Pockets opens inventory 2 and Close removes it;
the original-font title, border and backing were visually inspected. See
[canvas evidence](validation/ipad-native-canvas.json). This does not yet cover
item handling, dragging, resizing or scene transitions through touch.
The host adapts to
both landscape orientations and portrait, with the full game scene and controls
visible in inspected full-screen simulator captures. XCTest verifies that all
nine host buttons are hittable and wholly inside the window after each rotation.
See [layout evidence](validation/ipad-native-layout.json). Inventory dragging,
the original save menus, save export/import, physical-device signing and full
touch play remain pending.

The layout follows the available window size rather than relying on the
deprecated full-screen compatibility setting. See
[Apple's migration guidance](https://developer.apple.com/documentation/technotes/tn3192-migrating-your-app-from-the-deprecated-uirequiresfullscreen-key).
To repeat the simulator rotation test (use a new output directory):

```sh
python3 scripts/test-ipad-ui.py SIMULATOR-UDID assets/validation/ipad-ui-test
```

The test builds a local Xcode project with the same Swift host, bundled test
resources and Rust library. It launches the Simulator window, runs XCTest,
and retains its result bundle. Full-screen captures are used because app-only
XCTest screenshots clipped rotated content in this environment.

`--smoke-room` is a diagnostic launch argument: it runs the opening to tick
8,500, saves, advances, reloads and compares the frame, then pauses. A subsequent
`--smoke-load` launch loads that saved session and pauses. These arguments
write results to the app's Documents directory; ordinary launch plays normally.

The C ABI exposes uniquely prefixed symbol attributes with a local lint
exception for Rust 2024 `unsafe(no_mangle)`. It contains no unsafe blocks or
pointer dereferences. Shared engine crates retain `forbid(unsafe_code)`.
All calls occur on the main thread; Swift immediately copies borrowed frame,
audio and response buffers before the next call can replace them. This narrow
boundary does not provide a general thread-safe foreign API.

## Verified foundation

With Rust 1.95.0, Xcode 26.6 and the installed iOS 26.5 SDKs, the shared engine
and synthesis libraries build without source changes for device and simulator:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo build --locked --release --target aarch64-apple-ios -p discworld-core -p discworld-audio
cargo build --locked --release --target aarch64-apple-ios-sim -p discworld-core -p discworld-audio
```

These are Rust library builds, not linked or running iPad applications. The
existing SDL/rodio macOS executable has not been validated on iPad.

The existing Rust session interface (`discworld-web::BrowserGame`) also runs
as native code without JavaScript or a browser. The `ipad_session_probe` example
reaches Rincewind's room after 8,500 ticks in the iPad Pro 11-inch (M5), iOS 26.5
simulator. Its final frame matches the macOS probe, and save restoration
preserves next-frame pixels, generated PCM and session bytes on each host.
See [probe evidence](validation/ipad-native-session-probe.json).

To repeat with an already booted simulator:

```sh
cargo build --locked --release --target aarch64-apple-ios-sim -p discworld-web --example ipad_session_probe
xcrun simctl spawn booted "$PWD/target/aarch64-apple-ios-sim/release/examples/ipad_session_probe" "$PWD/assets/private/ipad/GameData.bundle"
```

This standalone simulator executable does not exercise an app sandbox, visible
window, touch, or audio-device output. It is not the installable application.

## Bundled test resources

```sh
python3 scripts/stage-ipad-test-assets.py
```

The staging command copies and verifies the original files into
`assets/private/ipad/GameData.bundle`, with stable names and a SHA-256 manifest:

- `discworld.iso`: game resources, speech, and sound effects.
- `music.sf2`: the same TimGM6mb SoundFont used by current host validation.
- `SoundFont-COPYRIGHT.txt`: the accompanying SoundFont notice.

Staging refuses to replace an existing bundle. Use `--output` with another
location under the ignored `assets` directory to prepare a new one. This
directory is intended to become an application resource, not a separate user
installation. It is not an installable app today. Public source and runtime
packages must not acquire these private game resources.

## Remaining integration and acceptance

Build a native iPad host around the shared engine, with a narrowly scoped
platform bridge. Keep game logic in safe Rust and preserve the workspace's
safety rule; review any required foreign-function boundary separately.
Present the existing framebuffer with native display code and send existing
generated audio to native playback. Add landscape touch controls and lifecycle
handling, retaining original game fonts and artwork.

The internal application must locate and validate the bundled assets at
startup, store saves in its writable application container, and support save
export/import. Package the resources into the signed app before installation.
Validate startup, speech/music/effects, mute, inventory, save/relaunch/load,
background/foreground, and full touch-only play on a real iPad. Simulator or
library-build success does not establish these results.

No device-signed IPA, TestFlight upload, or physical-device result is claimed.
The simulator prototype does not close the remaining acceptance items above.
