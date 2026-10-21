# Runtime snapshots

The core has a versioned snapshot codec. It captures the complete owned `Engine`
at a tick boundary, including suspended scripts, scheduler order, scene and
cutaway state, inventory, animation, random state, timers, and PCM/music timing.
External game archives and the host synthesizer/device are not serialized.

This is groundwork for durable player saves. There is no public activation API,
native save/load control, filesystem persistence, or browser persistence yet.
Decoding a snapshot never replaces a running engine.

## Envelope version 1

All envelope integers are little endian. The maximum complete file is 16 MiB.

| Offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 8 | `DWRSAVE` followed by a zero byte |
| 8 | 4 | Format version, currently 1 |
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
it does not activate the decoded state or establish process-relaunch equivalence.
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
second full run checks script references; combined validation is being tested
separately. Neither run activates a decoded state or verifies host relaunch.

Before exposing load, validate decoded invariants and resource references, with
atomic failure preserving the current session. Bound decoder allocations as
well as input size. Define the host asset identity consistently. Implement
atomic file replacement and host audio restoration, including synth state and
fractional clocks. Verify actual game checkpoints by save, process exit,
relaunch, load, and comparison of future inputs, frames, and audio. Exercise
inventory, dialogue, movement, transitions, and cutaways. Browser persistence
and real iPad interruption/relaunch checks remain required separately.
