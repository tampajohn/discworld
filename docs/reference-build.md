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
