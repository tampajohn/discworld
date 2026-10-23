# Native iPad development build

The requested iPad test deployment is a native application with the supplied
ISO and music SoundFont bundled. One installation must be sufficient to start
playing offline; no browser, file import, or separate sound download is required
for this internal test configuration. The browser host remains supported.

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

No native iPad app, signed IPA, TestFlight upload, or physical-device result is
claimed by the current library and resource-staging checks.
