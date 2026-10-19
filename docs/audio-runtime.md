# Discworld sample audio

The original DOS CD sample bank is unsigned 8-bit mono PCM at 22050 Hz. ENGLISH.IDX contains little-endian 32-bit offsets. Each nonzero sample offset points to a 32-bit byte length followed by PCM bytes in ENGLISH.SMP. Index zero is reserved. The checked reader rejects truncated payloads and compressed MP3, OGG, or FLAC banks.

The supplied image contains 6850 index entries and 5741 present samples totaling 344695006 PCM bytes. Every present sample passes bounds validation. Per-sample hashes and lengths are recorded in evidence/audio-audit.json. Formats and single-channel behavior follow the pinned ScummVM sound.cpp.

## Playback state

The core sample player tracks sample ID, playback position, and fractional output timing. Each 24 Hz engine tick generates 918 or 919 signed 16-bit samples, totaling exactly 22050 samples per second. Conversion is `(unsigned_byte - 128) * 256`. Starting a sample replaces the existing V1 channel. Scene transitions and STOPSAMPLE stop it.

PLAYSAMPLE now starts real PCM output; blocking calls remain suspended until the sample is consumed. Missing samples defer the calling script for one tick, following tinlib.cpp. WAITFRAME uses animation step counts, including hidden frames, so the TWG sequence progresses beyond its sample call.

The 150-tick run emits 137812 PCM samples, including 81415 nonzero samples. The local WAV is assets/previews/runtime-sound150.wav. A longer run reaches tick 155 before the next scene requires actor and polygon processes. No actual audio device is opened by the headless tool.

## Limits

Speech priority, escape interruption, volume settings, MIDI music, output-device buffering, and browser audio activation remain unimplemented. Tick/audio ordering still needs comparison with a running reference. Snapshot tests cover deterministic PCM continuation in memory, not durable save-file compatibility.

## Commands

Audit the archive:

```sh
cargo run --release -p discworld-tools -- audit-audio "Terry Pratchett's Discworld (1995)(Psygnosis).iso"
```

Render initial gameplay output and save PCM as WAV using new output paths:

```sh
cargo run --release -p discworld-tools -- run-headless "Terry Pratchett's Discworld (1995)(Psygnosis).iso" 150 assets/previews/intro.png assets/previews/intro.wav
```
