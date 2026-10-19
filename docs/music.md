# DOS music archive

MIDI.DAT starts with a little-endian maximum sequence length, followed by repeated little-endian lengths and sequence payloads. PLAYMIDI addresses a length word by file offset. The checked reader follows pinned ScummVM music.cpp and audio/midiparser_xmidi.cpp.

This ISO contains 154 XMIDI sequences in IFF FORM/CAT containers. The reader checks container lengths, nesting, padding, and exactly one EVNT stream per sequence. Several streams include a single zero alignment byte in their declared EVNT length after the end marker.

XMIDI delays add bytes below 128 before an event status; they are not standard MIDI variable-length quantities. Note-on durations and metadata/SysEx lengths use standard VLQs. The decoder preserves channel events, meta events, SysEx, note durations, and loop-controller commands. It does not expand loops, schedule note-offs, interpret Miles controllers, or synthesize sound.

```sh
cargo run --release -p discworld-tools -- audit-music "Terry Pratchett's Discworld (1995)(Psygnosis).iso"
```

evidence/music-audit.json records 154 tracks and 198410 encoded events with per-track hashes and controller inventories. Controllers 116 and 117 occur, requiring XMIDI loop semantics. PLAYMIDI remains an unsupported runtime boundary until sequencing and a sound backend exist. Asset decoding is not playback equivalence.

## Sequencing and offline synthesis

The sequencer now implements a fixed 120-Hz music clock, note duration releases, finite/infinite nested FOR/NEXT loops, whole-track repeat, end-of-track note cleanup, and snapshot cloning. Events retain musical timestamps across render blocks. Tempo metadata follows the reference's fixed 500000 microseconds per quarter at 60 PPQN. Tests check chunked versus continuous execution, loop counts, note release times, and transactional rejection of zero-time infinite loops. Same-note retrigger and driver-callback timing still need independent reference comparison.

`sequence-music <iso>` runs each of the 154 tracks for 60 seconds without forcing repeat; their own loop controllers remain active. The saved trace contains 296063 emitted messages with per-track hashes. This does not establish end-to-end audio equivalence.

The new discworld-audio crate presents timestamped messages through a pinned local copy of RustySynth 1.3.7. Two unsafe PCM casts have been replaced with checked little-endian decoding, and the vendored crate forbids unsafe code. See vendor/rustysynth/LOCAL_CHANGES.md and its MIT license. This is not a complete malformed-SoundFont audit.

For a ten-second offline render of the first track:

```sh
cargo run --release -p discworld-tools -- render-music "Terry Pratchett's Discworld (1995)(Psygnosis).iso" 4 10 assets/soundfonts/TimGM6mb.sf2 assets/previews/music-first.wav
```

The WAV command refuses existing output paths. Stereo synthesis is downmixed to mono 22050-Hz PCM for this diagnostic. The verified render has 220500 samples, 210492 nonzero samples, and peak magnitude 22924. MusicOutput uses absolute sample positions derived from musical ticks and rejects unordered messages and unsupported system messages.

The local development SoundFont comes from Debian timgm6mb-soundfont 1.3-5; it is GPL-2 licensed and remains excluded from version control with its copyright notice in assets/soundfonts/COPYRIGHT. Its origin and hashes are recorded in evidence/music-backend.json. Distribution of an instrument bank is a separate packaging decision. SoundFont synthesis is a General MIDI presentation choice, not a claim to reproduce a particular original sound card.

The core and audio crate compile for WebAssembly. PLAYMIDI integration, audible host output, synth-state save/restore, and independent reference comparisons are still outstanding. The current engine continues to reject PLAYMIDI instead of silently consuming it.

## Engine integration

`Engine::tick_media` accepts the checked MIDI track bank and emits absolute-clock messages in `music_frame`. PLAYMIDI supports replacement, loop selection, and waiting for completion; STOPMIDI releases notes and sustain. The music clock runs independently of scene scripts. The legacy tick methods keep rejecting PLAYMIDI if no track bank is supplied. Core snapshots include sequence state; synthesizer voices are not yet serializable.

The headless command accepts an optional SoundFont path after the WAV path and mixes synthesized music with sample PCM on the same 22050-Hz sample clock:

```sh
cargo run --release -p discworld-tools -- run-headless "Terry Pratchett's Discworld (1995)(Psygnosis).iso" 1000 assets/previews/runtime-midi.png assets/previews/runtime-midi.wav assets/soundfonts/TimGM6mb.sf2
```

Current execution requests track offset 40512, then stops on CONTROL in the same tick (361); evidence/headless-midi2.json correctly reports zero emitted MIDI messages. The selected track is independently renderable with render-music. The full title run has not yet produced integrated music because subsequent script behavior is still unsupported. WHICHINVENTORY queries the current closed inventory state; opening inventories remains unsupported.
