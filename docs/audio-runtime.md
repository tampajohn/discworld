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

Menu music volume is applied by `MusicOutput::render_engine_music` in the native,
WASM and headless rendering paths. The controller's music value (0–255) becomes
linear output gain after SoundFont synthesis and before sample PCM is mixed.
Without a controller, gain remains unity. Even at zero gain every MIDI event and
DSP sample is consumed; gain does not change the synth checkpoint or script clock.
The full-volume branch preserves existing output without additional arithmetic.
This supplies host mixer control; it does not claim an exact gain curve match to
every ScummVM MIDI driver or original sound card. The pinned music.cpp volume
setter updates both the music mixer category and the MIDI player.

The SoundFont regression checks zero, intermediate and full gain against the
same active notes, identical DSP checkpoints at each gain, uninterrupted unmute
after checkpoint restoration, and invalid-volume rejection before mutation.
Workspace tests, the opt-in SoundFont regression, Clippy and the web WASM check
pass (assets/logs/music-volume-*.log). Separate speech/effects gain still needs
persistent channel classification; the music slider never scales sample PCM.

Speech/effects gains now apply in the shared core. TALK, PRINT, TALKAT and
inventory speech mark the single sample channel as speech; PLAYSAMPLE uses the
effects category. A replacement sample changes category, a missing sample leaves
the current channel unchanged, and stop/natural completion clear the marker.
The marker belongs to playback rather than to a waiting script, so terminating
a script cannot misclassify a still-playing sample. Each engine tick applies
the selected controller volume (effects index 1, speech index 2) with integer
linear scaling by volume/255. With no controller, output remains full volume.
Gain does not change an already-playing sample's clock or cursor. Starting new
speech with voice volume zero has separate reference behavior described below.

DWRSAVE v12 preserves active speech classification. For older snapshots, a live
TALK/PRINT wait with the same sample serial recovers the speech category before
the next audio block; otherwise legacy unmarked playback is treated as effects.
Older snapshots cannot recover orphaned speech identity that they never stored.
The gain is applied before PCM reaches native, WASM or headless presentation;
the music gain remains separate. Tests cover replacement/missing IDs, stop and
natural completion, invalid gain atomicity, 0/128/255 scaling, speech versus
effects routing, legacy-wait recovery and exact resource-validated save/load
continuation. Workspace tests, all 14 original-asset web integration tests,
Clippy, web WASM checking and native release build pass. Logs use the
assets/logs/sample-volume-* prefix. No new physical-device listening or original
hardware gain-curve equivalence is claimed.


Reference correction for voice volume zero: pinned tinlib.cpp guards speech
startup in PRINT, inventory descriptions and TALK/TALKAT with `_voiceVolume != 0`.
The engine now follows that guard. New dialogue does not start a sample at zero
voice volume; text is displayed even when the subtitle preference is off, and
text duration controls completion. Skipping speech leaves an existing effect
sample untouched. This differs intentionally from the host M/Sound-off control,
which only silences presentation and keeps ordinary voiced timing.

A synthetic regression verifies forced text, exact text-vs-sample completion and
preservation of an existing effect. The opt-in original-asset browser-core test
`disabled_voice_plays_original_intro_with_text_and_restorable_timing` runs 8,500
intro ticks with zero voice volume and subtitles off, renders frames and checks
periodic save/load continuation. Workspace tests, the original-asset regression,
Clippy, web WASM checks and rebuilt native/browser packages pass; logs use
assets/logs/voice-disable-*.log. Full-route checks launched earlier remain pinned
to runtime 00a6b38 and use default volume; they do not test this zero-volume path.
