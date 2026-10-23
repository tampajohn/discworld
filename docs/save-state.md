# Runtime snapshots

The core has a versioned snapshot codec. It captures the complete owned `Engine`
at a tick boundary, including suspended scripts, scheduler order, scene and
cutaway state, inventory, animation, random state, timers, and PCM/music timing.
External game archives and audio devices are not serialized. The core snapshot
and host synthesizer checkpoint have separate inner envelopes, combined in a player save.
The host `MusicOutput` supports exact in-memory cloning, including its sample
clock and all synthesizer DSP state. A real TimGM6mb SoundFont test verifies
bit-exact continuation through active voices, pitch bend, sustain release,
effect tails, and a partially consumed block; a separate branch remains
unaffected by new notes. Run it explicitly with
`cargo test -p discworld-audio -- --ignored`; supply `DISCWORLD_TEST_SOUNDFONT`
to use a different local font. The test requires the font and is not silently
treated as passing when assets are missing. The same test now serializes and
reconstructs a fresh synthesizer, verifies bit-exact continuation, and rejects
wrong fonts, truncated/corrupted data, and invalid DSP settings with a repaired
checksum. Its encoded checkpoint is 315,019 bytes.

The host now supports durable player saves. `Snapshot::restore` returns an
engine after script, audio, polygon, animation and preview checks; the caller
commits that replacement only after preparing host state. Native F5/F9 and startup `--load` use a combined file; browser manual import/export and IndexedDB saves use the same format. Decoding a snapshot never replaces a running engine.

## Core envelope versions 1–7

All envelope integers are little endian. The maximum complete file is 16 MiB.

| Offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 8 | `DWRSAVE` followed by a zero byte |
| 8 | 4 | Format version: 1; 2 for pending scripted clicks; 3 for pending ordinary drops; 4 for physical mouse history; 5 for active header dragging; 6 for active inventory scrollbar dragging; 7 for stored inventory geometry |
| 12 | 32 | Host-supplied SHA-256 identity of the original game assets |
| 44 | 4 | Payload byte length |
| 48 | 32 | SHA-256 checksum of the payload |
| 80 | remaining | Postcard encoding of the complete engine |

The decoder rejects incorrect sizes, signatures, versions, asset identities,
checksums, and trailing payload data. The checksum detects corruption; it does
not authenticate a file or establish semantic validity of decoded engine state.
Encoding and decoding also check stack/base-pointer bounds, halted/pending-call
consistency, saved-scene nesting, and idle scheduler state with unique process
identities and positive sleeps. These checks cover scripts in active processes,
saved cutaways, and pending scene restoration. They do not yet constitute full
state or resource validation.
`Snapshot::verify_scripts` additionally resolves active and saved script handles
inside original bytecode chunks, checks pending calls against their encoded
opcode/operand/next address, and verifies halted positions. Host waits must
correspond to the VM's pending call; multiple simultaneous waits are rejected.
`Snapshot::verify_audio` checks active sample IDs/positions against the sample
bank and compares saved sequencer events with the original music archive. Basic
decoding also checks the fractional PCM clock, sequencer cursor/loop/note bounds,
music clock agreement, and pending stop messages. Offset-zero music facts are
handled without requiring the still-playing sequence to have been stopped.
This validates core audio state, not the host synthesizer or audio device.
`Snapshot::preview` checks animation clocks, capacities, script cursors, film
columns, and current image resources before rendering. It also checks hidden
cursor/inventory animations and deferred text, which ordinary rendering can
skip. Preview leaves the running engine unchanged and does not activate a save.
Changes to serialized field order/types require a format version change or an
explicit migration. These files are not ScummVM saves.

## Evidence and remaining gates

### Host audio checkpoint version 1

`MusicOutput::checkpoint_bytes` serializes all mutable DSP state, including voice
envelopes/oscillators/filters, channel controllers, chorus/reverb delay buffers,
the partially consumed output block, and the absolute sample cursor. It excludes
the immutable SoundFont and its preset lookup, which are reconstructed.

The 92-byte envelope contains `DWRMIDI\0` (8 bytes), version (u32), SoundFont
SHA-256 (32 bytes), rate (u32), sample cursor (u64), payload length (u32), and
SHA-256 of the preceding 60 header bytes plus payload (32 bytes). Integers are
little endian; the payload is Postcard and total size is capped at 8 MiB.
Restore checks format, identity and integrity before decoding, then validates
DSP vector shapes, fixed settings, finite numeric state, delay cursors, voice
identities and sample bounds before installing into a separately constructed
synthesizer. Reverb cursors may equal the buffer length because wrapping occurs
on the next process call; a dedicated vendored unit test covers this boundary.
This is not a complete hostile-file/allocation audit or cross-platform floating
point equivalence claim. Native device handoff is visually verified at credits; broader recovery and browser persistence remain.

Core tests exercise exact encode/decode equality and deterministic continuation
of a suspended wait, inventory interaction after a reordered/scrolled window,
and active speech/animation through natural completion. The speech test compares
complete engine state after each resumed tick, including its PCM output.
Envelope tests reject every single-byte mutation and every truncation of a
fixture, wrong assets, and trailing data with a recomputed checksum.
Additional tests reject forged scheduler state with a valid checksum and invalid
VM stack/pending-call state.

Append `--snapshot-audit` after the interaction-trace argument to `run-headless`
to check exact snapshot round trips every 1000 ticks, at scene changes, after
recorded inputs, and at the final tick. Reports include `snapshot_checks` and
`snapshot_max_bytes`. The audit hashes the supplied ISO as the asset identity;
it restores the decoded engine at each checkpoint and compares a trial next tick
against uninterrupted execution, including full state, PCM, and rendered surface.
With a SoundFont, the newer audit also restores the synthesizer and compares synthesized output. Process-relaunch evidence is recorded separately below.
The audit also invokes `verify_scripts` and `verify_audio`. The 10,000-tick opening prefix of
`evidence/act4-ending-input.json` passed 40 checks with a maximum encoded snapshot
of 36,973 bytes (`evidence/headless-snapshot-scripts-room.json`); its final
`unseen.scn` frame was inspected. This is fixture and opening evidence, not full
game save/load acceptance.
The same prefix passed all 40 checks with audio validation enabled
(`evidence/headless-snapshot-audio-room.json`, same maximum snapshot size).
The preview-enabled opening audit also passed; at each checkpoint the decoded
snapshot preview exactly matched the uninterrupted engine's rendered surface
(`evidence/headless-snapshot-preview-room.json`).

The initial full-route codec/structure audit at commit `9680068` passed 5,048
checkpoints through all 1,652 recorded inputs, the ending, restart, and title at
tick 2,630,000. Maximum encoded size was 47,409 bytes. Evidence:
`evidence/headless-ending-snapshot-audit.json`, exit 0, final title frame inspected.
This run predates script-resource, audio-resource, and preview validation. A
second full run at `1bf9525` passed the same 5,048 checkpoints with script-reference
validation (`evidence/headless-ending-snapshot-scripts-audit.json`, exit 0, title
at 2,630,000). Combined validation is being tested separately. Neither of these
two earlier runs activates a decoded state or verifies host relaunch.

The full audit at `14e1037` also passed all 5,048 checkpoints with script and
audio resource checks plus exact preview comparisons. It reached the title at
2,630,000 with restart true, exit 0, and the same 47,409-byte maximum snapshot
(`evidence/headless-ending-snapshot-preview-audit.json`). Its final PNG is byte
identical to the inspected initial-audit title frame. Actual replacement and
continuation use the newer restore audit and remain a separate result.

The restore-enabled opening audit passed 40 replacements and continuation checks
through tick 10,000 (`evidence/headless-snapshot-restore-room.json`). Tests also
reject invalid inventory, fade, and transition indices before restoration.
The full engine restore audit at `8226489` then passed all 5,048 replacements,
trial next ticks and frame comparisons through title/restart at 2,630,000
(`evidence/headless-ending-snapshot-restore-audit.json`, exit 0). It retains the
live synthesizer and therefore does not prove synthesizer restoration.

The opening combined engine/DSP audit passed 40 replacements of both components,
with bit-exact synthesized output through the current and trial next tick
(`evidence/headless-snapshot-dsp-room-wrap.json`). Maximum audio checkpoint size
was 317,055 bytes. The headless audit now reports `audio_snapshot_checks` and
`audio_snapshot_max_bytes` and restores DSP state when a SoundFont is supplied.

The full combined engine/DSP audit at `8f53854` passed all 5,048 replacements
and continuation checks through tick 2,630,000, restart true, exit 0. Maximum
core size was 47,409 bytes and maximum audio size was 317,221 bytes. Evidence:
`evidence/headless-ending-snapshot-dsp-audit.json`. The final PNG matches the
previously inspected title frame byte-for-byte. This establishes restoration
along the recorded route, not general save compatibility or native UI acceptance.

## Combined player save version 1

`discworld_audio::session` prepares engine and synthesizer together and rejects
mismatched clocks before returning a replacement. Its 52-byte header contains
`DWRPLAY\0`, version (u32), engine length (u32), audio length (u32), and SHA-256
of the first 20 header bytes plus both payloads. The inner envelopes bind the
save to the exact ISO and SoundFont. Total size is capped at 24 MiB + 52 bytes.
Native writes use a synced temporary sibling file followed by rename and parent
directory sync. Failed decoding leaves the running session intact.

F5 saves; F9 loads. `--save-file PATH` selects the file, defaulting to the
`--load PATH` source or `discworld-save.dws`. Startup `--load` cannot combine with
`--room` or `--from-tick`. Mute, speed and pause are host preferences preserved
when loading. Native hotkeys and device/window handoff are visually verified at credits; broader interaction and failure cases remain.

`scripts/check-save-relaunch.py ISO SOUNDFONT TRACE OUTPUTDIR` runs separate
baseline, save, and resumed processes. The default 10,000-to-12,000 opening test
passed with a 335,749-byte save. A second run with `--checkpoint 33200 --end 33400`
passed with a 332,019-byte save and open Luggage inventory. Both produce exactly
identical final engine/DSP files, rendered PNGs, and mixed speech/music PCM continuation;
both reject a clock mismatch despite repaired checksums. The inventory frame
was inspected: its original dark patterned background and gold border are present.
Evidence: `evidence/save-relaunch-opening/verification.json` and
`evidence/save-relaunch-inventory/verification.json`. The WAV includes speech and synthesized music mixed to mono. Earlier report
files call this `speech_pcm_equal`; that label understated what was compared.
New reports use `mixed_pcm_equal`.

Remaining gates include decoder allocation limits and deeper hostile-state
validation, broader native controls and failure recovery, more dialogue/movement/cutaway
relaunch checkpoints, browser persistence, and real iPad interruption/relaunch.

### Native credits checkpoint and restart continuation

The recovered native window successfully saved at tick 2,604,072 using F5;
the 329,430-byte file was produced while paused on visibly rendered credits.
A fresh headless process loaded that native file and reached tick 2,630,000,
Running with restart true and a PNG byte-identical to the full-route title frame
(`evidence/native-credits-save-relaunch.json`).

The relaunch harness now accepts `--initial-save PATH --start-tick N` to verify
late-game continuations without rebuilding the entire route. WAV capture limits
apply to elapsed capture duration, not the absolute final tick. Using the native
credits save, saving again at 2,604,200, and finishing at 2,605,000 passed exact
engine/DSP bytes, PNG, and mixed PCM comparisons, including script-driven restart.
The final frame is the title's credits attract scene, inspected visually.
The deliberately mismatched clock was rejected again. Evidence:
`evidence/save-relaunch-credits-restart/verification.json` (329,431-byte save).

Native GUI validation also passed: a fresh process loaded the 2,604,072 save
with `--paused --mute`, immediately displayed the same credits, saved with F5
to a separate path, and reloaded with F9. The saved files are byte-identical
(SHA-256 `0f1902f3c97ca2de27e56d367dc6e73ddf2aff5b9ae01b53db435ad9ae209012`).
Screenshots `assets/previews/credits-fresh-load-native.png` and
`assets/previews/credits-f9-reload-native.png` show actual game pixels; the latter
was independently inspected. `assets/logs/credits-load-validation-visible.log`
records the startup load, F5 save and F9 reload at the same tick. The visible
session remains paused and muted. This does not yet verify failed native loads
or every interactive save location.

## Pending world-click extension

Core version 2 retains the 80-byte envelope and asset/checksum validation. Its
Postcard payload is `(EngineV1Fields, ClickState)`: pending process IDs/phases
and the left-click token owner. The extension is explicitly skipped in the
legacy Engine serialization and encoded only through this versioned envelope.
No fields were inserted into existing positional script-process payloads.
Version 1 remains readable and is still emitted when there are no pending clicks,
so existing checkpoints preserve their encoded bytes. The outer combined-player
and DSP envelopes are unchanged.

The decoder checks that pending IDs exist and are unique, that exactly the
waiting phase owns the token, and that sleep values and script targets/events
are appropriate. A pending single click survives decoding and can be cancelled
by a double click before executing its original script. Version 3 adds pending ordinary inventory-drop phases and their absolute insertion
index. Version 2 files remain readable; states containing these new phases must
identify themselves as version 3. Drop owner processes and saved indices are
validated before activation. Physical mouse classifier persistence and raw host input wiring are covered
by version 4 below; complete input compatibility remains unfinished.

Core version 4 stores `(EngineV1Fields, ClickState, MouseInput)` so a partial
physical click pair survives save/reload. Versions 1–3 remain readable and keep
their original field layouts. A session without physical input still emits its
previous compatible version. Raw button history resets at script-driven restart.

## Save immediately after a ground click

A real-asset regression exposed a load failure between physical input and the
next simulation tick: the queued player-walk task has no bytecode, but script
validation tried to resolve its zero handle. The validator now recognizes a
pristine walk task for the existing lead mover, while rejecting an unknown
actor/mover or extra script/wait state. No save-format change is required.

`cargo run --release -p discworld-web --example check_pending_walk -- ISO SF2 SAVE`
loads the original inventory checkpoint, closes it, clicks ground at (300,180),
saves before advancing, reloads, and compares two 24-tick continuations. With
`evidence/save-relaunch-inventory/checkpoint.dws`, this initially failed with
`script handle outside bytecode chunk`; it now passes at tick 33,224 in
`rinceroo.scn`. Complete combined saves, RGBA and stereo PCM are identical
between original and restored continuation.

`scripts/check-pending-walk.cjs MODULE ISO SF2 SAVE` performs the same check in
actual Node/WASM, using chunked ISO upload. It also passes. These comparisons
are exact within each runtime; they do not assert native/WASM DSP equality.
The workspace tests and Clippy pass, including malformed walk-owner rejection.

## Save during inventory pickup and putdown

The opt-in `discworld-web` test
`inventory_pickup_and_drop_resume_pending_physical_clicks` loads the original
tick-33,200 inventory checkpoint and clicks the banana (object 190) in luggage
inventory 1 at (118,132), using physical down/up events. It verifies the held
object changes only after the pending click completes, then puts it back and
checks membership and an empty hand. Both operations save/reload before their
first tick and again during the double-click wait. Each continuation compares
the complete combined engine/DSP save, RGBA pixels, and stereo PCM exactly.

Run with absolute paths in `DISCWORLD_TEST_ISO`, `DISCWORLD_TEST_SOUNDFONT`, and
`DISCWORLD_TEST_SAVE` (the latter is
`evidence/save-relaunch-inventory/checkpoint.dws`):

```sh
cargo test --release -p discworld-web inventory_pickup_and_drop_resume_pending_physical_clicks -- --ignored --nocapture
node scripts/check-pending-inventory.cjs MODULE ISO SF2 SAVE
```

Both checks pass at tick 33,250 in `rinceroo.scn`. The Node script runs the same
physical input and save/continuation comparisons in actual WASM; held-object
and membership assertions run in the Rust test. The reference's
`dialogs.cpp::InvPdProcess` delays ordinary putdown by `_dclickSpeed + 1`
scheduler ticks before calling `invPutDown`; these checks cover restoration
across that suspended native task as well as the scripted pickup. They do not
establish all inventory interactions, cross-platform PCM equality, or physical
iPad input compatibility. No engine change was required for these checks.

## Active inventory drags

Core version 5 stores `(EngineV1Fields, ClickState, Option<MouseInput>, slot)`
while a title-bar drag owns an active inventory. Validation requires mouse
history and a visible, declared window matching that slot. The existing engine
payload already includes the translated geometry and live animation states.
Versions 1–4 remain readable and keep their original encoding; after release,
a mouse-driven session uses version 4 again. The real-asset regression
`inventory_header_drag_moves_immediately_and_survives_save` validates an exact
save/reload during movement and subsequent engine/DSP, frame and PCM continuity.

### Active inventory scrollbar dragging

Core version 6 stores `(EngineV1Fields, ClickState, Option<MouseInput>,
Option<owner>, SliderDrag)`. The slider payload preserves its unclamped movement
accumulator, absolute range, original row-position table and last selected row.
The rendered thumb stays continuous during icon refreshes. Release selects the
nearest row and schedules the usual inventory refresh, then saves use version 4.
Header drags still use version 5; versions 1–5 keep their prior encodings.

Validation requires an ordinary inventory owner, its visible scrollbar and
mouse history. The table is bounded to 151 entries with ordered item indices
and positions, matching endpoints, a valid selected entry and a thumb matching
the clamped accumulator. Close, scene teardown and hidden/replaced inventory
windows discard the active gesture. Native and browser hosts retain left-button
ownership for either drag type when loading a save.

### Maximization geometry

Core version 7 stores `(EngineV1Fields, ClickState, Option<MouseInput>,
Option<owner>, Option<SliderDrag>, InventoryGeometry)`. Geometry contains the
active maximized flag and, for each of the three window slots, the saved normal
dimensions, alternate position and stored maximized flag. Closed windows retain
this state; restoring the normal size does not discard the expanded position.
Normal dimensions must be within supported bounds when needed, and a changed
window state requires a declared window. Active drag ownership and slider
validation remain mandatory.

Version 7 is emitted only when geometry differs from its initial state. Otherwise
the previous version-selection rules apply, retaining byte-compatible encodings
for versions 1–6. Legacy saves start with the original default alternate position
[21,15] and no maximized flags.

### Active border/corner resizing

Core version 8 stores `(EngineV1Fields, ClickState, Option<MouseInput>,
Option<owner>, InventoryGeometry, ResizeDrag)` only while a resize is active.
The gesture includes its direction, retained axis deltas, partial-cell offsets,
initial geometry, original corner dimensions, prepared reel animations and
current outline. The pre-existing full inventory frame remains in Engine fields.
Validation requires a visible ordinary inventory owner, no simultaneous slider,
bounded geometry and partial offsets, the complete required reel set and an
outline matching its saved geometry. Animation resources undergo the existing
snapshot validation. Version 1–7 encodings remain unchanged when no resize is
active. Release returns to the applicable earlier version.


Version9 extends the previous tuples with inventory hover state. The payload is
(engine, clicks, optional mouse, optional drag owner, optional slider, geometry,
optional resize, hover). The legacy engine serialization is unchanged. Hover
contains the remembered object, completed-POINTED serial, suppression flag, and
process-keyed label or departure waits. Label waits carry the original layout,
absolute cell, pending PRINTOBJ call and visibility; departure waits require a
halted POINTED interpreter and a valid completed serial. Older versions restore
an empty hover state. This is the runtime's save schema, not ScummVM save format.

## Menu checkpoints (version 10)

When `Engine::configuration` is present, version 10 extends the version 9 tuple
with the controller checkpoint bytes (maximum 64 KiB). The outer asset identity
and checksum cover this extension. Without a controller, the encoder continues
to select the previous applicable format. Original resources and strings rebuild
menu graphics; drawable objects are not serialized in the extension.

Use `Snapshot::restore_with_strings` and `preview_with_strings` for these saves.
The older methods reject a decoded menu checkpoint rather than omit its window.
Combined-session decoding supplies strings in native, browser and headless hosts.
Restoration validates the reconstructed controller and rejects an active menu
overlapping a conversation or inventory window. The engine renders the menu
before the cursor and suppresses the held-item cursor while a menu page is open.

Original-asset tests compare restored engine state, rendered pixels and encoded
session bytes across menu pages and effects. An opt-in browser test loads a native
menu checkpoint in Chrome and WebKit, compares all canvas pixels, and exports
identical session bytes. Generate its fixture from the repository root (the
output and companion `.rgba` path must not already exist):

```sh
DISCWORLD_TEST_ISO="$PWD/Terry Pratchett's Discworld (1995)(Psygnosis).iso" \
DISCWORLD_TEST_SOUNDFONT="$PWD/assets/soundfonts/TimGM6mb.sf2" \
DISCWORLD_TEST_MENU_SAVE_OUTPUT="$PWD/assets/saves/menu-engine-v10.dws" \
cargo test --release -p discworld-web original_menu_controller_transitions_render_and_resolve_live_selection -- --ignored
```

Then, from `web`, run:

```sh
DISCWORLD_TEST_MENU_SAVE="$PWD/../assets/saves/menu-engine-v10.dws" \
npx playwright test tests/menu-save.spec.js
```

This verifies persistence and rendering, not live menu operation. F1 opening,
input routing, menu scheduling, lifecycle transitions and settings application
still require integration. Normal hosts do not yet create this controller.

## Native menu session storage

`discworld_tools::menu_sessions` bridges a menu `Dispatch` to the atomic slot
store. `save` encodes a synchronized engine/DSP session only for a Save request
with a resolved target. `prepare_load` validates and reconstructs both engine
and synthesizer without mutating the running instance. The caller commits both
together, then resets queued device audio. Empty selections are no-ops; a Load
request cannot invent a new slot.

The catalog validates complete sessions against the original resources, asset
identity and SoundFont before offering them. Corrupt or incompatible IDs are
reported separately and remain reserved by the allocator. A selected load is
validated again, since files may have changed after the catalog was built.

The opt-in test `menu_slots_validate_assets_and_prepare_exact_session_replacement`
uses `DISCWORLD_TEST_ISO`, `DISCWORLD_TEST_SOUNDFONT` and `DISCWORLD_TEST_SAVE`.
It verifies exact session bytes after preparation, rejection of corrupt and
wrong-ISO slots, unchanged current state, and allocation around rejected IDs.
Run with `cargo test --release -p discworld-tools menu_slots_validate_assets_and_prepare_exact_session_replacement -- --ignored`.
Native SDL menu activation and dispatch consumption are now wired; see
`docs/control.md` for validation and remaining settings limitations.

## Speech animation phase (v11)

Slower subtitles can outlast their speech sample. The actor returns to its normal
reel when speech ends, while the script continues waiting for text dismissal.
Version 11 stores the process IDs whose talking reels have already finished,
plus the existing input/inventory state and optional controller checkpoint.
This prevents a restored line from attempting to finish the same talking reel
twice. The decoder rejects missing processes, duplicate markers and markers
inconsistent with initialized text, sample or mover state. Versions 1–10 remain
readable; saves without this intermediate phase retain their prior encoding.

The synthetic animated-TALK regression checks natural completion, cancellation,
byte-stable v11 encoding, full resource-validated restoration and identical
continuation through the remaining subtitle duration. It supplies an empty
precomputed text layout to isolate scheduling from font rasterization.

## Active speech channel (v12)

Version 12 uses the v11 extension layout but identifies the active sample as
speech; its finished-talking-process list may be empty. Decoding requires an
active sample and retains the existing process/controller validation. Other
new saves continue selecting the smallest applicable previous format. The
SamplePlayer base serialization is unchanged, so versions 1–11 remain readable
and byte-stable before stepping. Old live speech waits recover channel identity
on the next tick. Explicit v12 ownership also survives after the script that
started the sample has disappeared.


DWRSAVE v14 reserves and validates escape-event state without changing legacy
Machine/Engine postcard layouts. Its bounded wrapper contains a v1-v13 envelope,
a nonzero global generation and captured values for every live/master/saved/
restoring script context. No global escape state means unchanged legacy output.
See [escape integration status](script-escape-compatibility.md); host input and
cancellation integration remain pending.


## Live film escape ownership (v15)

Version 15 wraps a v1-v14 envelope and an ordered, nonempty list of actor,
playback identity and captured escape generation. Each entry must match a
live reel owner and a nonzero generation no newer than the global event.
The decoder rejects duplicate/unordered actors, unknown owners, invalid
generations, trailing data and nested v15 wrappers. Saves without live reel
captures retain the previous encoding.


## Blocking sample ownership (v16)

Version 16 wraps a v1-v15 envelope with one optional sample serial per active
scheduler process, in scheduler order. At least one entry must be present;
present serials must be nonzero, no newer than the sample player's serial and
attached to a pending sample wait. Older serials are valid: replacement must
release their wait without stopping the new sound. The decoder checks shape,
trailing data and nesting before installing the extension. Saves without
recorded sample wait ownership retain their previous encoding.

Saved-scene contexts contain interpreter state rather than host waits, so
this extension covers active scheduler processes only. Historical active waits
without serials remain byte-stable on load/export and retain their previous
channel-busy completion behavior; Escape releases them without stopping an
unproven owner. Ownership is captured normally on subsequent sample calls.


## Intro state (v17)

Version 17 wraps a v1-v16 envelope and the DOS intro policy: detected title
handle, active flag and optional initial escape generation. It rejects nested
v17 envelopes and trailing bytes, and validates generations against the saved
global escape counter. Resource activation validates title scene metadata and
entrance 1. Snapshots without configured intro policy retain prior encoding.

Runtime snapshot restoration retains intro state exactly. A host's explicit
user-load operation must separately call `Engine::end_intro`, matching the
reference's distinction between game-load and live interpreter continuation.
Browser worker loads and native user-load actions now apply this rule. The
exact browser `load` API and native recorded-replay startup loads preserve
policy state for deterministic continuation.

## Version 18: independent mover visibility

A hidden mover now has a separate boolean from Actor.hidden. The field is
excluded from the legacy serialized Mover layout. When any current mover is
hidden, DWRSAVE v18 wraps the existing v1–v17 save plus a sorted, nonempty list
of hidden mover IDs. Otherwise encoding follows the previous path unchanged.
Legacy files default mover visibility to false and retain exact re-encoding.

The wrapper uses the same length, asset identity, checksum and size checks as
previous extensions. Decode rejects nested v18 payloads, trailing bytes,
empty/duplicate/unsorted ID lists, missing movers and invalid hidden-mover
identity. Restored flags must match existing mover keys, placement actor IDs
and the actor table. The extension does not modify V1 scene-stack semantics.

Focused tests verify exact bytes and state on round-trip, independence from
Actor.hidden, identical inner legacy bytes, malformed extensions and return
to the original bytes when visibility is cleared. Logs:
`assets/logs/mover-visibility-codec-*`.

HIDEACTOR now uses this persistence extension together with behind-background
composition, independent mover visibility and ownership-aware release. See
[mover lifecycle](mover-reel-compatibility.md) for implementation and evidence.


## Version 19: pending SPLAY reel cleanup

DWRSAVE v19 wraps a v1–v18 snapshot and a sorted, nonempty list of SPLAY
playback generations. The runtime field is excluded from the legacy Engine
layout. With no SPLAY generations, encoding follows the previous path exactly.
Flags identify live nonblocking films or blocking primary reels, including
replaced reels that no longer own their actor but still owe restoration. They
do not identify mover animations or arbitrary render objects.

Each generation must be nonzero, no newer than the playback counter, and
resolve to exactly one live film/primary reel with a reel identity. Decoding
checks canonical ordering, empty/duplicate lists, missing identities, nested
v19 wrappers, trailing bytes, asset identity, length, size and checksum.
Scene transitions clear this scene-local cleanup set. V1 scene-stack reel
restoration remains separate from exact runtime snapshots.

Tests cover blocking/nonblocking holders, legacy visibility state, multiple
superseded generations, exact state and byte round-trips, identical inner
legacy bytes, malformed wrappers, duplicate live reel identities, and returning
to the original bytes after clearing metadata. SPLAY dispatch and exit-side-effect handling now populate and consume these
identities; see the mover lifecycle document for integration coverage.

Validation passes: workspace suite (324 core tests), 20 original-asset web
tests, Clippy and WASM checks. Logs use assets/logs/splay-codec-*. Subsequent SPLAY lifecycle integration rebuilt native and browser release
hosts and passed the same checks; logs use assets/logs/splay-lifecycle-*.


## Version 20: movers initialized during a film

The initial movie-created-mover extension wraps an exact v1–v19 snapshot and
a sorted, nonempty list of (actor ID, playback generation) pairs. Empty metadata
uses the prior encoding unchanged. The marker distinguishes initialization
visibility from hiding an existing mover; HIDEACTOR's current hidden flag remains
independent.

Each pair must identify an existing matching mover and actor, a nonzero playback
generation within the current counter, and exactly one live owned film or
blocking primary. Decode rejects duplicate/unsorted actors, missing movers,
wrong generations, trailing bytes, and nested v20 wrappers, with the standard
asset, length, checksum and size checks. Scene transitions clear the map.
Tests cover v19 nesting, blocking/nonblocking holders, legacy visibility, exact
round-trip, malformed metadata, and unchanged bytes after clearing markers.

Automatic mover creation, visibility, render/scroll behavior and lifecycle
cleanup now use this extension. See the mover compatibility document for
integration coverage.

Validation: workspace suite and final 327-core-test run, all 20 original-asset
web tests, Clippy and WASM checks pass. Both release hosts rebuilt after the
6167511 browser audit finished, including the implicit film-position correction.
Logs use assets/logs/created-mover-codec-*. This does not establish automatic
mover creation behavior; the new metadata is not yet populated by dispatch.

## Version 21: reel dispatch order

Newly scheduled PLAY/SPLAY/TOPPLAY and internal film launches now have native
jobs in the script scheduler. Version 21 wraps a v1–v20 snapshot, an ordered
list of `(process ID, reel task)` pairs, and the shared first-column depth and
factor. A pending task stores its film, column, requested position, status-layer
flag, SPLAY flag and captured Escape generation. An active task identifies the
playback generation in the existing film storage. Scheduler positions, sleeps
and process identities stay in the inner snapshot; the extension identifies
which otherwise pristine native process slots are reel jobs rather than VMs.

Decoding checks process existence and pristine state, uniqueness/canonical
queue order, pending column bounds and Escape generation, active generation
bounds and uniqueness, and depth bounds. Standard checksum, asset, length and
size validation applies. A nested v21 wrapper, trailing bytes, and an entirely
empty extension are rejected. Resource validation checks pending film/reel
references; it does not interpret native task slots as script bytecode.

A stale active task may legitimately remain for its next cleanup dispatch after
a replacement removes its animation. The task need not own a current film, but
must identify a nonzero generation no newer than the playback counter. Existing
SPLAY cleanup and created-mover ownership validation still apply separately.

States with no reel tasks and zero shared dispatch depth retain their previous
encoding. Legacy imported films without jobs keep their prior animation phase;
newly scheduled films use the shared queue. Exact pending/active continuation,
WAITFRAME ordering, pending Escape, multi-column order, malformed associations
and extreme depth values have regression coverage. The complete-game reference
and scene-reconstruction scheduling audit remain separate acceptance work.


## Version 23: scheduled scrolling

New scene entry creates a native scroll task before actor and scene scripts.
Version 23 uses the same `(inner, tasks, depth)` wrapper as versions 21–22,
with a new appended task variant carrying the scroll initialization flag.
The wrapper also records that scrolling uses scheduler order, even after the
scroll task exits for a screen-sized background. Its inner snapshot is v1–v20;
the wrapper cannot nest another task wrapper. Duplicate scroll tasks and scroll
variants under versions 21–22 are rejected.

Historical saves retain their fixed camera phase and byte-exact representation
until their next scene entry. Existing isolated legacy-format tests explicitly
construct that older state. New tests exercise same-cycle movement, delayed
blocking-script resumption, v23 continuation and independent original credits
frames before and after save/load. No game assets or scripts are rewritten.

## Version 22: pending restored columns

A snapshot with a queued restored actor column uses version 22. It retains the
version-21 wrapper shape (inner v1–v20 snapshot, ordered task associations, shared
depth), adding the `Restored` task variant after the existing Pending and Active
variants. A restored request contains the expected actor and its film, column,
position and explicit depth. Zero depth requests ordinary dispatch-time depth
calculation; nonzero depth leaves shared first-column depth/factor unchanged.

Version 21 rejects restored requests, and version 22 requires at least one.
Neither permits nested task wrappers. Structural checks validate actor existence
and column bounds; resource verification also checks the column's actor identity
and script address. Once all restored requests dispatch, snapshots use version
21 or the appropriate older encoding again. Existing version-21 bytes retain
their interpretation. A regression saves before dispatch, checks exact re-encoding
and resource validation, rejects a mislabeled version-21 header, and compares
exact engine state after both copies dispatch the restored animation.
