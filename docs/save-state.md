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
Native SDL menu activation and dispatch consumption still require wiring.
