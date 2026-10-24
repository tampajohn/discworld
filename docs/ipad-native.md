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

By default the script builds for the Apple Silicon simulator. It includes the
staged ISO and SoundFont in one ad-hoc-signed app. `--platform device` instead
links for physical iPad hardware and leaves provisioning/signing outstanding. The observed
prototype is `assets/apps/Discworld-iPad-17.app`. Startup reaches the bedroom;
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
[canvas evidence](validation/ipad-native-canvas.json).
A second touch sequence opens the wardrobe, picks up pouch 284, stores it in
Pockets, retrieves it, and loads a save restoring the stored item and exact
framebuffer. It uses screen taps and the original object scripts, with read-only
observations for assertions. See [pouch evidence](validation/ipad-native-pouch.json).
This covers one item; broader item use and resizing remain unverified through
native touch. Another touch test opens the bedroom door, walks down the university
stairs, saves, returns to the bedroom, reloads the stairs save and returns again.
The loaded tick and framebuffer match the save exactly; both return walks finish
with control restored. See [stairs evidence](validation/ipad-native-stairs.json).
This is one stair segment, not full hallway traversal or continuous reference
comparison of doorway animation.
One-finger pans now use the engine's existing inventory hit regions and mouse
press/move/release path. The native host preserves the initial touch before
UIKit recognizes the pan and forwards its final position before releasing it.
Pause, load and application suspension also release an owned drag. Canvas taps
and new drags are ignored while paused.
The simulator test verifies an exact `[30,20]` window displacement, release,
subsequent input and paused-drag rejection. See [drag evidence](validation/ipad-native-drag.json).
Cancellation and suspension during an active drag still need direct acceptance tests.
A separate simulator test presses Home and reactivates the app, first while
paused/muted and then during unmuted playback. Paused ticks and pixels are
preserved; the last inactive tick equals the first resumed tick, and audio
completion callbacks resume. See [lifecycle evidence](validation/ipad-native-lifecycle.json).
The native audio-interruption handler now distinguishes begin/end notifications,
freezes simulation while interrupted, and honors Apple's `shouldResume` option.
Without that option, the player must tap Resume. Real system interruptions,
lock/unlock and audio route changes still require device acceptance; the Home
button test does not establish those results.
See [Apple's interruption guidance](https://developer.apple.com/documentation/avfaudio/handling-audio-interruptions).
The host adapts to
both landscape orientations and portrait, with the full game scene and controls
visible in inspected full-screen simulator captures. XCTest verifies that all
nine host buttons are hittable and wholly inside the window after each rotation.
See [layout evidence](validation/ipad-native-layout.json). Broader inventory item use,
menu overwrite/settings/restart/quit acceptance, save export/import, physical-device signing and full
touch play remain pending.

The layout follows the available window size rather than relying on the
deprecated full-screen compatibility setting. See
[Apple's migration guidance](https://developer.apple.com/documentation/technotes/tn3192-migrating-your-app-from-the-deprecated-uirequiresfullscreen-key).
To repeat the simulator rotation test (use a new output directory):

```sh
python3 scripts/test-ipad-ui.py SIMULATOR-UDID assets/validation/ipad-ui-test
```

The runner refuses UI execution if any other simulator is booted, before any
configuration change, build or install. Use `--build-only` while a player session
is active. `--no-window` skips opening Simulator and changing shared preferences;
it does not override this guard. A separate-device attempt compiled the current
nine-test suite, including Files-picker cancellation, but was interrupted and
produced no usable completed result bundle. The saved pretest listing establishes
that the player's device was already shut down before that attempt. The player
simulator was reopened on build 18 and the automation device shut down. See
[attempt evidence](validation/ipad-isolated-ui-attempt.json). New gesture and
Files UI acceptance remains pending; the earlier eight-test pass is unchanged.

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

## Staged touch controls and original artwork

Build `assets/apps/Discworld-iPad-18.app` is compiled and signed for the simulator
and installed at the player’s explicit restart request. Existing saves were
preserved, and a startup screenshot shows the styled controls in landscape.
In default Walk mode, single
taps walk/select and double taps act. A single tap waits for the double-tap
recognizer to fail, preventing an unintended walk before an action. Explicit
Action and Look modes remain available; pans still take priority for dragging.
The pouch UI test now exercises double-tap wardrobe opening and pickup followed
by single-tap storage. It has compiled but has not yet run against this change.
The earlier eight-test pass covers build 17, not this staged revision.

The private build extracts host labels with `ipad_controls`, using the ISO's
original tag-font glyphs and MainMenu palette indices 228/231. New host labels
are composed from those glyphs; they are not claimed to be original button
objects. Normal, selected and highlighted states use these raster assets, with
nearest-neighbor display and accessibility names retained. Both standalone and
XCTest builds generate and bundle `Controls`; generated artwork stays under
ignored `assets`. Prefer original game assets/fonts for project game controls.
No generated substitute artwork is needed for this implementation.

To compile UI tests without altering or driving a player's Simulator session:

```sh
python3 scripts/test-ipad-ui.py SIMULATOR-UDID assets/validation/ipad-ui-build --build-only
```

Quick saves now use the same bounded atomic-file helper as the desktop host,
including file synchronization, unique temporary names and cleanup on failure.
An isolated macOS headless bridge test passed with the private bundled assets:
exact session/frame restoration, truncated and oversized file rejection without
changing the session, and failed-write cleanup. A fresh host also restores the
same checkpoint; after the failed I/O attempts, 240 continuation ticks match
exactly in frames, combined engine/DSP bytes and all 441,000 PCM sample bit
patterns (all nonzero). This is one checkpoint in a headless macOS process,
not a UIKit relaunch or a physical-device audio check. See
[fresh-host continuity evidence](validation/ipad-quick-continuity.json).
This is host persistence work;
the shared engine and original game scripts are unchanged. See
[staged control evidence](validation/ipad-staged-controls.json).

## Staged Files transfer

`assets/apps/Discworld-iPad-20.app` builds and signs successfully but is not
installed over the player's build 18. Its original-font **Files** control pauses
play and offers export/import through Apple's native document picker. Export
writes the current combined engine/DSP session to a temporary `Discworld.dws`
and asks Files to copy it to the chosen location. Import copies a selected file,
performs a bounded read and full session validation before replacing live state.
It does not overwrite quick saves or named slots. After importing, use Save to
keep that state on the device. Dialog completion/cancellation leaves gameplay
paused until Resume. Close an original game menu before transferring files.

The headless test verifies exact export bytes, restoration from an external
file, preservation of a different existing quick save, and unchanged live state
after an invalid import. The restored host then matches 240 frames, combined
state snapshots and 441,000 PCM samples against its baseline. See
[transfer evidence](validation/ipad-save-transfer.json). Native document-provider
interaction, cancellation, layout and physical-device transfer still require UI
acceptance; compilation is not evidence that those flows work end to end.

The picker uses Apple's
[copy-based export/import API](https://developer.apple.com/documentation/uikit/uidocumentpickerviewcontroller/init%28forexporting%3Aascopy%3A%29).
Generated Files artwork remains private with the other extracted control assets.

## Optional settings recovery

Build `assets/apps/Discworld-iPad-21.app` includes the staged Files transfer and
recovers from unreadable optional settings. Previously a damaged settings file
aborted startup. The host now uses defaults and reports that recovery, matching
the desktop host's optional-preference behavior. It preserves the bad file;
only a later explicit settings change writes new preferences. Missing settings
remain a normal first launch, and valid preferences are applied unchanged.
The headless test compares complete startup session bytes against the missing-file
baseline for malformed, oversized and out-of-range cases. See
[recovery evidence](validation/ipad-settings-recovery.json). Compilation/signing
passed; the build has not replaced the user's build 18 session, and visual
startup-notice acceptance remains pending.

## Original sound controls

Native pans now reach configuration menus as well as inventory windows, while
the engine retains original hit testing and slider arithmetic. The simulator
test drags Music Volume to zero, then 40 pixels right to 125/255, releases it,
closes the menu and verifies that the same setting survives a fresh launch.
Other channels remain at 255. The test leaves music at 125/255. See
[sound evidence](validation/ipad-native-sound.json). This checks controls and
persistence, not subjective sound quality or physical-device audio behavior.

The post-rotation software-keyboard failure was resolved by disabling Simulator's
`ConnectHardwareKeyboard` preference at both shared and per-device levels and
restarting its GUI. Changing only the per-device setting was insufficient.
The test runner now performs this setup, backs up changed preferences under
ignored `assets/validation`, and refuses to restart the GUI if another simulator
is booted. App and save data are preserved. No game keyboard behavior change was
needed. A targeted rotation/save/relaunch/load run passed with a visible keyboard
frame `[0,444,1210,390]`; the earlier failing frame was `[0,834,1210,0]`.
`GCKeyboard.coalesced` still reports a connection in the passing run, so that
flag alone does not establish software-keyboard visibility.
The subsequent full suite passed all eight tests, including rotation followed by
named save/relaunch/load. See [keyboard and suite evidence](validation/ipad-native-keyboard.json).

## Original menus and named saves

The Menu button opens the runtime's original Main menu and resumes the host
clock so original button animations can run. Original script control gates still
apply: menus cannot open during a scene's uninterruptible sequence. Existing
Save/Load host buttons remain quick-save shortcuts.

The native bridge consumes original Save/Load requests using the same checked,
atomic slot-file implementation as the macOS tools. Records live in the app's
Documents/slots directory, include engine/DSP state, and populate the original
catalog on startup. Invalid/incompatible records stay reserved on disk and are
omitted from the menu. Settings requests use the existing validated settings-file
format; settings, overwrite, restart and quit need further UI acceptance.

The software keyboard appears on the original Save page. Printable ASCII and
Delete feed the shared editor; the game renders the name with its original font.
The scene scales above the keyboard rather than being covered by it. One named
save, software-key correction and relaunch/load sequence passed in the simulator;
the stored name and file checksum were independently inspected. See
[menu evidence](validation/ipad-native-menu.json). Save import/export and physical
keyboard/device acceptance remain open.
A headless native-bridge test additionally creates two named saves through the
original menu, overwrites the older row while retaining its stable ID and the
other slot's exact bytes, cancels an edited new row without writing, and loads
the replacement through a newly constructed host's original Load page. The
loaded engine/DSP session bytes match exactly. See
[overwrite evidence](validation/ipad-menu-overwrite.json). This closes the bridge
persistence check, not the outstanding UIKit touch overwrite acceptance.

## Physical-device build

The complete UIKit app and Rust bridge now link for `aarch64-apple-ios` using
`--platform device`:

```sh
python3 scripts/build-ipad.py --platform device --output assets/apps/Discworld-iPad-device.app
```

The verified artifact is `assets/apps/Discworld-iPad-device-2.app`. Mach-O build
metadata identifies IOS (not Simulator), arm64, minimum OS 17.0 and SDK 26.5.
Linked dependencies are system iOS/Swift libraries, and the bundle declares
only the iPad device family. All three private resource hashes match the staged
manifest; the original-font Files artwork is included. See
[device build evidence](validation/ipad-device-build.json).

This app is **unprovisioned** and is not an installable device deployment yet.
The build script intentionally does not apply the simulator's ad-hoc signing
step to device output. Apple Development identities exist locally, but the
latest device discovery found no iPad. Provisioning for the intended iPad,
installation and hardware acceptance remain outstanding. The user's running
simulator app was not replaced or driven during this check.

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
directory is copied into the simulator application by `build-ipad.py`, so no
separate resource installation is needed. The staging directory alone is not
an installable app. Public source and runtime
packages must not acquire these private game resources.

## Remaining integration and acceptance

The native host, platform bridge, framebuffer presentation, audio playback and
basic landscape touch controls are implemented as described above. Complete
inventory resizing and broader item handling, remaining menu and lifecycle acceptance,
retaining original game fonts and artwork.

The internal application must locate and validate the bundled assets at
startup, store saves in its writable application container, and support save
export/import. Package the resources into the signed app before installation.
Validate startup, speech/music/effects, mute, inventory, save/relaunch/load,
background/foreground, and full touch-only play on a real iPad. Simulator or
library-build success does not establish these results.

No device-signed IPA, TestFlight upload, or physical-device result is claimed.
The simulator prototype does not close the remaining acceptance items above.

## iPhone prototype

The host now supports iPhone in landscape, with original-font controls in two side columns. iPad now uses the same paired side columns, with larger controls. The first physical iPhone deployment includes the private ISO and SoundFont and uses runtime `8ac0135` from source `23ebe72`, the last complete cross-host playthrough pass. The newer cursor runtime is withheld pending a full-route regression fix. Wireless installation, launch and process liveness passed; physical touch/layout acceptance remains open. See [deployment evidence](validation/iphone-first-deployment.json).

## Native touch and lifecycle follow-up

Held-item drags now dispatch the drop/use action at release, rather than only
moving the cursor. A cancelled drag does not drop. Native touch Walk gestures
with a held item may snap up to six original-game pixels to a nearby inventory
cell inside the open inventory; empty-hand gestures and Action/Look retain exact
hit testing. Core mouse behavior and original scripts are unchanged.

The Load control follows the host's `can_load` response and stays disabled when
`quick.dws` is absent. Save-name keyboard focus is allowed only while the game
reports an active name editor, preventing an unsolicited keyboard after return
from Home. Foreground activation clears a stale audio-interruption latch and
retries audio. Audio resumption advice no longer changes the user's pause state.
Apple documents that interruption begin/end notifications need not be paired:
[Responding to interruptions](https://developer.apple.com/library/archive/documentation/Audio/Conceptual/AudioSessionProgrammingGuide/HandlingAudioInterruptions/HandlingAudioInterruptions.html).

The private control extractor now exports original inventory-film sprites with
transparent bounds and records object/film/frame handles in `icon-sources.json`.
Walk uses object231, Action330, Look294, Pockets284, Menu223, Save188, Load189 and
Files273. Labels retain the original tag font; missing raster labels no longer
fall back to system-font button text. These are new host controls assembled from
game assets, not claims that the original game had these toolbar buttons.

The native toolbar omits Start, Continue and Close. Escape is the dismissal control. The iPhone canvas walking/inventory test passes using Escape to close the inventory (`assets/validation/iphone-controls-trim1-tests/results.xcresult`).

Empty-handed Walk taps outside an ordinary inventory now close it and dispatch
movement to that point. Held objects and active drags are protected. The broader modal behavior below
supersedes the initial ordinary-inventory-only adaptation. Conversation/action-window topic taps (including the mouth
icon) now route to conversation selection in every touch mode. Before this fix,
direct touch gestures fell through to world interaction and were rejected while
the raw mouse-edge path used by pans could select a topic. Menu single taps also
select normally regardless of the selected world verb.

Phone controls use matching six-row columns: navigation and saves on the left,
world verbs above sound/pause on the right. Save and Load open original-font
confirmation panels. Cancel preserves the current game and previous pause state;
only confirmation performs quick-save overwrite or quick-load. See
[validation evidence](validation/ios-topic-confirm.json).


## Native UX interaction rules

Keep these rules when extending the iPhone/iPad host:

- Empty-handed Walk taps outside game windows dismiss them and continue to the
  tapped floor position once the original scripts return control. This covers
  inventory, conversation, tutorial windows and configuration menus. Selecting
  Walk also dismisses these overlays without choosing a destination.
- Never interpret an inventory transfer or held-item gesture as close-and-walk.
  New input, changing modes, pausing, suspension, loading, or a scene/origin change
  cancels pending movement; queued movement expires after 240 game ticks.
- Menu/topic selections use a single tap in every world action mode. During
  startup and speech waits, a canvas tap advances the existing wait.
- Quick Save/Load require explicit confirmation. Outside taps cancel rather than
  confirm, and an empty-handed floor tap in Walk mode continues walking. Cancel
  restores the previous pause state. Load remains disabled without a quick save.
- The Files panel has explicit Import, Export and Cancel buttons with the same
  outside dismissal/walking behavior. Cancelling a transfer restores the previous
  pause state. Apple's file picker is an OS modal: use its Cancel to return.
- Keep matching six-row side columns: Pockets/Escape/Menu/Files/Save/Load on the
  left; Walk/Action/Look above Sound/Pause on the right. Preserve safe areas and
  at least 44-point targets. iPhone is landscape; iPad also supports portrait.
- Use the original game's font for host game controls and prompts, with original
  asset icons where appropriate. The OS file picker retains its native controls.

The touch host uses normal game Escape/postamble behavior instead of rewriting
scripts. The only core API change in this pass exposes existing read-only menu
hit testing. The physical build retains the validated 8ac0135 engine. This is a
focused UX regression pass, not a new full-game acceptance run. See
[UX validation](validation/ios-ux-pass.json).

## Home Screen icon

Native builds now include an `AppIcon` asset catalog for iPhone and iPad, including
Home Screen, Spotlight, Settings and the 1024-pixel marketing image. The default
uses [ScummVM's Discworld artwork icon](https://github.com/scummvm/scummvm-icons/blob/9343606fa428e90755dda28cb9b37e18c7232e20/icons/tinsel-dw.png):
Rincewind, Death, the Luggage and the original gold logo. This is original game
artwork assembled for a launcher, not a claim that the supplied DOS disc had a
Windows/Mac launcher icon. No standalone icon was present on the supplied disc.

`scripts/stage-ios-app-icon.py OUTPUT.xcassets` stages a pinned, SHA-256-verified
source under ignored `assets/private` and generates opaque sRGB PNGs. It fits the
complete artwork over black; iOS applies its own rounded corners. `--source PATH`
can use another historical icon. Original artwork remains with its rights holders
and is not committed to this repository. Source provenance is written alongside
the generated catalog. Both `build-ipad.py` and `test-ipad-ui.py` include the icon;
custom Xcode projects must add that catalog to Resources and set
`ASSETCATALOG_COMPILER_APPICON_NAME=AppIcon`.
