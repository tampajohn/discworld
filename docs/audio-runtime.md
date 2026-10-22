# Discworld sample audio

The original DOS CD sample bank is unsigned 8-bit mono PCM at 22050 Hz. ENGLISH.IDX contains little-endian 32-bit offsets. Each nonzero sample offset points to a 32-bit byte length followed by PCM bytes in ENGLISH.SMP. Index zero is reserved. The checked reader rejects truncated payloads and compressed MP3, OGG, or FLAC banks.

The supplied image contains 6850 index entries and 5741 present samples totaling 344695006 PCM bytes. Every present sample passes bounds validation. Per-sample hashes and lengths are recorded in evidence/audio-audit.json. Formats and single-channel behavior follow the pinned ScummVM sound.cpp.

## Playback state

The core sample player tracks sample ID, playback position, and fractional output timing. Each 24 Hz engine tick generates 918 or 919 signed 16-bit samples, totaling exactly 22050 samples per second. Conversion is `(unsigned_byte - 128) * 256`. Starting a sample replaces the existing V1 channel. Scene transitions and STOPSAMPLE stop it.

PLAYSAMPLE now starts real PCM output; blocking calls remain suspended until the sample is consumed. Missing samples defer the calling script for one tick, following tinlib.cpp. WAITFRAME uses animation step counts, including hidden frames, so the TWG sequence progresses beyond its sample call.

The early 150-tick smoke run emits 137812 PCM samples, including 81415 nonzero samples. The local WAV is assets/previews/runtime-sound150.wav. This is historical smoke evidence; the runtime now completes the recorded game route. No actual audio device is opened by the headless tool.

## Host presentation

Native and browser hosts mix the original sample channel with SoundFont MIDI
output. Both preserve continuous sample-rate conversion across engine blocks.
Native `audio_output::connect` exposes the stereo22050Hz queue as one span so
rodio does not restart interpolation at each buffer boundary. Browser
`web/audio.js` retains one stereo frame and an integer conversion phase across
worker replies, then schedules device-rate WebAudio buffers. Both use linear
interpolation. Pause/load clears pending presentation audio; mute is a downstream
gain and does not modify the game state or original sample data.

Regression tests compare a continuous tone delivered as one buffer and as24
engine blocks at44100Hz and48000Hz. Both old host paths produce different device
output for those equivalent inputs; the corrected paths are partition invariant
(native exact equality; browser OfflineAudioContext tolerance1e-6 in Chrome and
WebKit). Details and local test evidence are in [control.md](control.md).
This fixes a playback artifact, not the hiss or quantization in original8-bit
recordings. No denoising is applied.

## Limits

Live original-menu volume settings remain unfinished. Tick/audio ordering still
needs comparison with a running reference, and physical iPad listening and
device-interruption behavior require acceptance testing. Durable session files
now preserve engine PCM position and complete SoundFont DSP state; original-asset
tests verify continuation. Cross-platform DSP equality is not claimed. Continuous
conversion does not by itself prove the absence of mix clipping, device underruns
or noise in the source recordings.

## Commands

Audit the archive:

```sh
cargo run --release -p discworld-tools -- audit-audio "Terry Pratchett's Discworld (1995)(Psygnosis).iso"
```

Render initial gameplay output and save PCM as WAV using new output paths:

```sh
cargo run --release -p discworld-tools -- run-headless "Terry Pratchett's Discworld (1995)(Psygnosis).iso" 150 assets/previews/intro.png assets/previews/intro.wav
```
