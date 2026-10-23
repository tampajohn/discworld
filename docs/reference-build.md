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
