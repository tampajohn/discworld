# Native development distribution

`scripts/package-native.py OUTPUT.tar.gz` builds the native host on Apple Silicon
macOS using `cargo build --locked --release`. A clean tracked checkout and stable
HEAD are required; unrelated untracked game files are excluded. The script
checks the Mach-O architecture, rejects non-system dynamic dependencies, and
verifies the existing code signature. SDL is statically linked by the current
Cargo configuration. Signature verification does not establish notarization.

The archive contains an executable with mode 0755, LICENSE, NOTICE, usage
instructions, exact tracked project source from `git archive`, and a manifest
covering every other file. Publication refuses existing output paths. The game
ISO and user SoundFont are not included. The source includes the vendored synth
and Cargo.lock; Cargo resolves the pinned third-party build dependencies.

## Local validation

The local archive `assets/packages/discworld-native-apple-silicon.tar.gz` is pinned
to cf7f8888a7e0f121d4a833f7741b2c3c99bd5533. It is 3,869,977 bytes, SHA-256
`fedcb5d026b89ed9f5cfe55ffe83145bc2682d82879bab1291e6c1532e4aaf2f`.

All five manifested files passed hash checks; the source archive matched a fresh
archive of the pinned revision byte-for-byte. Extraction into a fresh temporary
directory outside the checkout retained executable permissions, and signature
verification passed there. Reusing the package output path was rejected.

The extracted executable loaded the original tick-133103 unseen.scn checkpoint
using absolute ISO/SoundFont paths and a separate writable save path. With
`SDL_VIDEODRIVER=dummy`, `--mute --frames 30`, it rendered 30 frames and exited
successfully at tick 133116, unseen.scn, `stopped=false`. This checks an isolated
working directory, asset loading, session restoration and the native host loop;
it does not establish visible-window interaction, physical audio quality or
compatibility on a different Mac. Logs: `assets/logs/native-package-build.json`,
`native-package-validation.json`, `native-package-smoke.log`, and
`native-package-refusal.log`.

The inspected binary records minimum macOS 11.0, but that is linker metadata,
not evidence that all supported hardware or OS versions work. Finder `.app`
packaging is now available as described below. Distribution signing/notarization
and broader OS/device acceptance remain outstanding. This archive is a development deliverable, not completion
of the runtime goal.


## Current development package

`assets/packages/discworld-native-d39a279.tar.gz` contains runtime d39a279,
including current mover creation, SPLAY lifecycle, FRAMEGRAB/RUNMODE and
scripted subtitle handling. All manifested file hashes and its exact source
archive were verified after extraction outside the checkout; executable
permissions and strict signature verification pass. See
[package evidence](validation/packages-d39a279.json) for size and SHA-256.
The extracted executable accepted all 1,652 recorded inputs through tick
2,630,000, then ran 30 muted SDL dummy-video presentation frames and exited
with tick 2,630,013, title.scn, stopped=false. This validates accelerated native
route preparation and the headless host loop; it is not visible interaction,
device audio, or a snapshot audit. The later internal/restored-film creation
correction at 9ba83fb is not included in this package.

## Finder development bundle

`scripts/package-macos-app.py OUTPUT.zip` packages the verified native distribution
inside `Discworld.app`. It requires the same clean Apple Silicon checkout as the
CLI builder. Double-clicking the app asks for the English DOS CD v2 ISO and a
SoundFont, then runs the native host. Cancelling either picker exits without
starting a session. Assets are selected on each launch and are never bundled.

Quick saves, named slots and preferences live under
`~/Library/Application Support/Discworld Rust/`. The current session log is
`last-session.log` there. Use the original load menu to resume a saved game;
launch does not automatically load the quick save. Closing the game ends the
launcher. Paths are individually quoted, including spaces and shell characters.

The bundle contains the exact source archive and native manifest in
`Contents/Resources/runtime/`. It is ad-hoc signed, not Developer ID signed or
notarized. Finder/Gatekeeper behavior on another Mac and visible picker/window
interaction remain acceptance checks; successful packaging alone does not prove
them. The app uses the system AppleScript applet icon pending original artwork.

The final pinned-launcher bundle is
`assets/packages/discworld-finder-pinned.zip`, source `a8c9c20`. Extraction
outside the checkout preserves its strict deep signature. All five native
manifest hashes, the exact source archive and the decompiled launcher against
that source were verified. See [integrity evidence](validation/finder-pinned-package.json).
The earlier bundle's dummy-video startup/load checks are recorded separately in
[first-bundle evidence](validation/finder-first-package.json); they do not prove
visible Finder launch for either artifact.

The cached-click engine correction is packaged in
`assets/packages/discworld-finder-07e395f.zip`. Its extracted runtime passes
muted dummy-video startup and legacy-save loading outside the checkout. Deep
signature, all manifested hashes and exact source verification pass. This does
not establish visible Finder/file-picker behavior. See
[package evidence](validation/packages-07e395f.json).

`assets/packages/discworld-finder-9186705.zip` includes the inventory trail
movement correction. Its extracted runtime passes startup and legacy-save
loading with dummy video/audio. Signature, manifest and exact source checks
pass. Visible Finder launch remains unverified. See
[package evidence](validation/packages-9186705.json).


The `cc0e3db` Finder package contains runtime `30c2f69`, including synchronous
inventory cursor changes before drag and on release. Extracted outside the
checkout, all five manifest files and the exact source archive verify, and
deep/strict signature verification passes. Dummy-video startup and legacy save
load advance to ticks 13 and 33213 with no stopped runtime. These are executable
checks, not visible Finder/file-picker acceptance. See
[package evidence](validation/packages-cc0e3db.json).
