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
Changes to serialized field order/types require a format version change or an
explicit migration. These files are not ScummVM saves.

## Evidence and remaining gates

Core tests exercise exact encode/decode equality and deterministic continuation
of a suspended wait, inventory interaction after a reordered/scrolled window,
and active speech/animation through natural completion. The speech test compares
complete engine state after each resumed tick, including its PCM output.
Envelope tests reject every single-byte mutation and every truncation of a
fixture, wrong assets, and trailing data with a recomputed checksum.

Before exposing load, validate decoded invariants and resource references, with
atomic failure preserving the current session. Bound decoder allocations as
well as input size. Define the host asset identity consistently. Implement
atomic file replacement and host audio restoration, including synth state and
fractional clocks. Verify actual game checkpoints by save, process exit,
relaunch, load, and comparison of future inputs, frames, and audio. Exercise
inventory, dialogue, movement, transitions, and cutaways. Browser persistence
and real iPad interruption/relaunch checks remain required separately.
