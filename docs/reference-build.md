# Pinned ScummVM reference build

The reference source is commit 96b42d42bc8533953bdd701db059cc33c839a9ff, downloaded from GitHub codeload into the ignored .reference directory. The reference source is not modified. Its upstream AI contribution guidelines were read; this work builds a local comparison executable and does not submit a contribution to ScummVM.

Configuration:

```sh
./configure --disable-all-engines --enable-engine=tinsel --disable-detection-full --disable-libcurl --disable-sdlnet --disable-fluidsynth --disable-freetype2 --with-sdl-prefix=/opt/homebrew
make -j8
```

Configure and build logs are .reference/scummvm-configure.log and .reference/scummvm-build.log. Both configuration and build succeeded. The executable detects the extracted files as Discworld (CD/DOS/English).

The native Cocoa/OpenGL backend successfully executes the game. Reproduce a bounded live run with:

```sh
python3 tools/reference_smoke.py --seconds 20
```

This opens a game window, disables MIDI, and uses a dummy audio device. It terminates the child at the time limit, escalating to kill only if that child does not exit. Logs and preferences stay under .reference. Reports overwrite evidence/reference-smoke.json and evidence/reference-animations.json. The normalization removes process-specific addresses while retaining each animation instance's initial position/frame, script handle, period, and observed instruction offsets/opcodes.

The first successful run observed 18 animation instances and 603 operations, including the Psygnosis and TWG reels and later scenes. It also received input events, so it is not a deterministic input replay. The report records this explicitly. No frame, audio, timing, or full-game equivalence has been established. SDL dummy video failed during graphics initialization; Cocoa with the SDL surface backend also failed. Cocoa/OpenGL is the verified configuration for this machine.

Homebrew installed pkgconf and upgraded SDL2 compatibility support during setup. Homebrew also upgraded SDL3, FFmpeg, and linked dependencies through its dependency handling. Future dependency setup should suppress unrelated dependent upgrades where possible.

The extracted asset copy is assets/game, excluded from version control. Reproduce extraction into a new directory with:

```sh
cargo run --release -p discworld-tools -- extract-assets "Terry Pratchett's Discworld (1995)(Psygnosis).iso" assets/game
```

The command requires a new destination directory and refuses overwriting files. It extracts only the DISCWLD directory; the original ISO remains unchanged.


## Captured animation instruction comparison

`discworld-tools compare-animation-trace ISO TRACE.json` compares each
nonempty normalized reference animation's observed `(index, opcode)` prefix
against the production Rust animation interpreter. `Animation::tick_traced`
uses the same implementation as ordinary ticks, with no trace fields in save
state. Failed ticks commit neither state nor a trace. Operands are excluded,
matching the reference's `DoNextFrame` debug records.

The historical `evidence/reference-animations.json` capture matches all 603
observed operations across 13 animation instances. Five initialized instances
contain no observed operations and are explicitly excluded from pass counts.
The original console SHA-256 matches `evidence/reference-smoke.json`, and
rerunning its normalizer yields the tracked JSON exactly. A corrupted first
opcode is rejected. This uses the previously captured live reference run; it
is not a newly executed deterministic reference session. The capture metadata
identifies the pinned revision but does not include an executable hash.

Reproduce the opcode comparison from the repository root:

```sh
cargo run --locked --release -p discworld-tools -- compare-animation-trace "Terry Pratchett's Discworld (1995)(Psygnosis).iso" evidence/reference-animations.json
```

The capture logs initial image handles, not multipart frame lists. Comparison
therefore uses a neutral object: the opcode interpreter's index progression
is independent of its shape and position. It checks only recorded prefixes,
allowing capture to end during a tick. It does not compare image pixels, object
geometry, flip state, animation timing, cross-instance scheduling, input order
or audio. The original capture received live input, so it is not suitable as
a deterministic whole-engine trace. Broader reference acceptance remains open.

The tracing regression checks exact control/frame/jump opcode sequences,
waiting ticks, ordinary-versus-traced state equality, and atomic failure.
Local comparison and deliberate-corruption logs use the
`assets/logs/reference-animation-*` prefix.

Validation passes: workspace tests (333 core), Clippy, 20 original-asset web
tests and native/browser release builds. The comparison command rejects the
deliberately corrupted trace. These checks do not replace the broader live
reference acceptance criteria.

The [recorded comparison evidence](validation/reference-animation-prefixes.json)
contains capture and input hashes, matched operation counts, individual
instance coverage and deliberate-corruption rejection.


## Fresh reference framebuffer capture

`tools/reference_frames_lldb.py` reads the pinned executable's original
320x200 indexed framebuffer and the OpenGL backend's current 256-color palette.
It sets debugger breakpoints at palette updates and immediately before
`Background::DrawBackgnd` calls `updateScreen`. It reads memory through LLDB
without invoking target expressions or modifying reference source. A separate
reference process is stopped after the requested draw count and killed by its
owning batch command. This does not control the user's existing native session.

A fresh 120-frame run captured the Psygnosis and TWG scenes without probe
errors. All 156 extracted game files used by the reference matched their
SHA-256 entries in the supplied ISO. The reference binary, probe, raw log,
capture report and Rust renderer hashes are recorded in
[pixel-comparison evidence](validation/reference-logo-pixels.json).

The Rust tick-24 Psygnosis image matches every RGB pixel of reference draw
ordinals 7–50: 320x200, no resizing, cropping or masked regions. The reference
frame was visually inspected. This proves the fully visible logo composition
and resolved palette for that image. Captured TWG animation and fade frames
have not yet been compared; draw ordinals are not engine ticks. Debugger pauses
alter wall-clock timing, and a live window can receive input. No PLR event was
observed in this capture's action log, but that is not proof of input isolation.

To reproduce a bounded frame capture on the same macOS/arm64 debug-symbol build,
create a fresh parent directory and set `DISCWORLD_REFERENCE_FRAMES` to a
nonexistent child directory within it. Set `DISCWORLD_REFERENCE_FRAME_LIMIT`
to 120 (accepted range 1–1000), `SDL_VIDEODRIVER=cocoa` and
`SDL_AUDIODRIVER=dummy`. From the repository root, invoke LLDB with:

```sh
lldb --batch \
  -o 'settings set target.disable-aslr false' \
  -o 'command script import tools/reference_frames_lldb.py' \
  -o run \
  -o 'script reference_frames_lldb.finish(lldb.debugger)' \
  -o 'process kill' \
  -- .reference/scummvm-96b42d42bc8533953bdd701db059cc33c839a9ff/scummvm \
  --config=/absolute/fresh-parent/reference.ini --path=assets/game \
  --gfx-mode=opengl --no-fullscreen --music-driver=null \
  --debuglevel=5 --debugflags=actions --random-seed=1 tinsel:dw
```

Use a bounded process runner when automating this command; the successful run
used a 60-second outer timeout. Check `capture.json.complete` and `errors`,
not just LLDB's exit code. The hook writes `.idx` and `.pal` files plus hashes;
it stops on an invalid framebuffer, unavailable symbols or failed memory read.
The source-line breakpoint and debug member names are specific to the pinned
reference build.


## Startup sequence and first-step phase

The read-only probe now counts `NextGameCycle` entries and records
`InitStepAnimScript` plus `DoNextFrame` entry events. Four independent
120-draw captures produced identical scene/RGB sequences. The first rendered
frame occurs in reference cycle 2; draw ordinals therefore are not cycle
numbers. The two startup scenes contain 25 distinct scene/image pairs.

`discworld-tools capture-startup-frames ISO TICKS NEW_DIRECTORY` captures
unscaled RGB after each Rust startup tick, advancing original script, PCM
and MIDI state without user input or device audio. It also records active
film state and scene calls. The new directory is required and tick count is
bounded to 1–10,000. Compare verified capture files with:

```sh
python3 tools/compare_reference_frames.py REFERENCE_FRAMES RUST_CAPTURE NEW_REPORT.json
```

The comparator verifies every raw-frame/palette hash and distinguishes image
coverage from scene-start-aligned equality. All 120 reference images have
exact matches in their corresponding Rust scenes. After aligning each scene's
first draw, 103 frames match at the corresponding tick and 17 TWG frames do
not. Those mismatches are not timing passes. The detailed
[phase evidence](validation/reference-startup-phase.json) records both outcomes.

The independent event capture confirms the cause to investigate: TWG script
125903660 is initialized and first stepped in reference cycle 58. Rust creates
it at tick 57 with zero steps and first steps it at tick 58. Relative to reel
creation, this is reference offset zero versus Rust offset one. The ordinary
nonblocking film pass currently precedes scene-script dispatch, so a newly
created film misses that pass. Correcting this requires scheduling its first
step in the creation cycle while preserving film arbitration, caller/peer
ordering, WAITFRAME counts and blocking-primary behavior. No engine scheduling
change has been applied in this diagnostic commit.

The tools build, workspace suite and Clippy pass. Original captures run under
Cocoa/OpenGL with dummy audio; neither these diagnostics nor the matching
images establish wall-clock, audio or full-game reference equivalence.
