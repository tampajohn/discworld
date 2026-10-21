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
commits that replacement only after preparing host state. Native F5/F9 and startup `--load` use a combined file; browser persistence remains unfinished. Decoding a snapshot never replaces a running engine.

## Core envelope versions 1, 2 and 3

All envelope integers are little endian. The maximum complete file is 16 MiB.

| Offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 8 | `DWRSAVE` followed by a zero byte |
| 8 | 4 | Format version: 1; 2 for pending scripted clicks; 3 for pending ordinary drops |
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
validated before activation. Physical mouse classifier persistence and raw host
input wiring remain unfinished.
