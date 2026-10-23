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
packaging, distribution signing/notarization, and broader OS/device acceptance
remain outstanding. This archive is a development deliverable, not completion
of the runtime goal.


## Current development package

`assets/packages/discworld-native-d39a279.tar.gz` contains runtime d39a279,
including current mover creation, SPLAY lifecycle, FRAMEGRAB/RUNMODE and
scripted subtitle handling. All manifested file hashes and its exact source
archive were verified after extraction outside the checkout; executable
permissions and strict signature verification pass. See
[package evidence](validation/packages-d39a279.json) for size and SHA-256.
The full recorded-route run of this extracted executable is pending; integrity
checks alone do not establish gameplay acceptance.
