# Discworld script execution

The initial implementation notes below are historical. Current runtime and
validation status is maintained in [goal.md](goal.md), the focused compatibility
documents, and `docs/validation/`.

The safe Rust bytecode machine implements the V1 instructions handled by the pinned ScummVM interpreter. Its state includes a 128-word stack, stack and base pointers, an instruction pointer, and a pending library call. Invalid operations leave the machine and globals unchanged.

Operands are signed 8, 16, or 32 bit values. The 8-bit flag takes precedence if both width flags are set. Jumps and procedure targets are offsets from the current script start. Calls write static and dynamic links and a return address without raising the stack pointer; the callee claims this storage with ALLOC. These semantics follow the pinned [pcode.cpp](https://github.com/scummvm/scummvm/blob/96b42d42bc8533953bdd701db059cc33c839a9ff/engines/tinsel/pcode.cpp).

Arithmetic explicitly wraps at 32 bits, except division by zero and signed division overflow produce errors. CIMM is declared but has no interpreter case in the reference; this reader rejects it. Noir NOOP is also rejected.

## Engine boundary

Library calls suspend with their operands intact. The host must complete the pending call with an operand count and optional result before execution continues. A cloned suspended machine resumes identically; durable saves and asynchronous engine state remain outstanding. The 125 library names follow DW1_CODES in the pinned tinlib.cpp.

## Real data evidence

The probe starts each script independently with fresh zero globals, stopping at the first library call or balanced halt with a 10000-instruction budget. The supplied release has 388 globals, 147 scene records, and 2071 discovered entry points, including 886 polygon scripts. All prefixes execute without error: 2043 stop at library calls and 28 halt. The master starts with DECCURSOR. Results are in evidence/script-probe.json.

This is a parser and VM smoke test, not a gameplay trace. It excludes inventory entry points, later paths, script fixes, actual scene state, and library-operation behavior.

## Scene records

Scene metadata contains entrance, polygon, and actor counts; referral direction; scene script; and table handles. Entrance records are eight bytes: ID and script handle. Actor records are twelve bytes: masking, ID, and script handle. A regression test covers this distinction. Script handles must point inside a bytecode chunk; the reader bounds code by the remaining chunk pending more precise control-flow analysis.

## Remaining runtime work

Implement scene transitions, events, background films, PLAY, the cooperative scheduler, and other library operations without placeholder success. Add durable serialization and script workarounds. ESCON and ESCOFF currently track only an enabled bit; escape counters, pause effects, and intro-specific behavior need host integration. A live ScummVM comparison remains necessary.

## Master initialization

The startup command now executes the real master script through 891 VM steps and applies 117 supported engine calls. It stops with NEWSCENE pending, requesting resource handle 117440512 (index 14), entrance 1, transition 0. It does not acknowledge the scene change or run scene scripts.

Startup state includes the six-reel cursor film, lead actor definition, two mover reel tables with auxiliary scales, scale transition reels, tag and talk fonts, window artwork, inventory and conversation dimensions, configuration strings, permanent topics, and actor speech colors. Data is stored in the core; rendering and input consumers are still pending. Inventory maximums follow the V1 reference limits of 10 by 6 cells and 150 contents. The lead alias -2 resolves to the registered lead actor for auxiliary-scale declarations.

Film readers resolve reel object and animation-script handles; multipart object and frame readers are present. Animation stepping and a restricted headless scene runtime are now implemented; see initial-runtime.md. Actor resource limits, complete mover state, scene lifecycle effects, and permanent topics originating from inventory metadata still need integration. Startup evidence is in evidence/startup.json and is not a live reference comparison.


## FRAMEGRAB and RUNMODE compatibility

The continuing master and scene dispatchers now handle V1 FRAMEGRAB (116)
and RUNMODE (122). In pinned `tinlib.cpp`, FRAMEGRAB returns -1 without
performing work; it consumes exactly one argument. RunMode returns constant
zero and its dispatch writes into the caller's existing result slot, leaving
stack depth unchanged. Neither call yields or checks Escape. This is the
reference behavior, not a placeholder for a capture feature. Startup bootstrap
retains its separate declaration-oriented dispatch.

The bytecode regression covers both process contexts, sentinel preservation,
replacement of a nonzero result slot, completion within one tick despite stale
Escape, and atomic failure when the required stack slot is missing. Workspace
tests (329 core tests), all 20 original-asset web integration tests, and Clippy
pass; native and browser release hosts rebuild.
Logs use `assets/logs/framegrab-runmode-*`. The full-route 0ccdf9f audit
predates these two calls and is not evidence of their coverage on the route.


## Scripted subtitles

SUBTITLES (123) is implemented in the continuing master and scene dispatchers.
Pinned `tinlib.cpp::Subtitles` and `pcode.h` define ST_ON as zero and ST_OFF
as one. The English DOS runtime consumes one argument immediately, independent
of Escape; other values fail atomically. Existing speech timing and text
selection continue to use the shared subtitle setting. This does not implement
the separate Japanese-version override or startup-bootstrap dispatch.

The operation updates both engine state and the configuration controller,
including a live subtitle widget and a toggle retained across page changes.
Script changes do not reset slider geometry or pending button timing. A pending
toggle subsequently inverts the newly assigned value, matching its reference
pointer to the shared configuration setting. The existing snapshot format
preserves these fields; no new save version is needed.

A bytecode matrix covers both process contexts, with and without a controller,
valid/invalid modes, stale Escape, stack preservation, immediate completion and
exact snapshot encoding. The opt-in core original-menu regression checks real
Subtitles/Controls pages and a closed menu, resource-validated restoration, the
next menu cycle, and active/retained toggle continuation through checkpoints.
Workspace tests (330 core tests), Clippy, the original-menu regression and all
20 original-asset web tests pass. Native and browser release hosts rebuild.
Logs use `assets/logs/script-subtitles-*`. This targeted validation does not
claim full-route coverage of the newly implemented operation.

## Scripted quit

QUITGAME (115) now stops the sample channel and MIDI playback, consumes no
operands, and lets the current interpreter slice and scheduler pass finish.
The cycle returns `Status::Quit`. The request is carried in a local cycle result,
not an engine field, so save layouts are unchanged. Continuing master and scene
interpreters both support it; bootstrap before the initial NEWSCENE remains a
separate restricted declaration path.

This follows pinned `tinlib.cpp::QuitGame` and its zero-argument dispatcher,
plus `tinsel.cpp`'s outer `shouldQuit()` loop and `NextGameCycle` scheduler call.
A same-cycle restart completes before the quit result is delivered. Unsupported
operations still report their compatibility stop rather than being masked by a
prior quit request.

`tick_with_configuration` supplies a Quit dispatch when no menu request is
already present. Native finishes an already-issued menu operation and exits.
Browser completes the DSP block, stops advancing the batch, and delivers the
existing worker/player quit lifecycle without latching a runtime fault. If a
menu operation was already issued, a host-only pending quit follows it and
survives the load handler's clearing of ordinary menu requests. Browser quit
pauses presentation; it does not attempt to close the user's tab. The host must
consume the cycle's request before continuing simulation.

A bytecode regression covers master and scene callers, preservation of a lower
stack value, execution after QUITGAME, a peer's same-pass execution, stopping
active MIDI/sample state, pre-quit snapshot continuation and post-cycle saves.
A browser-core integration injects QUITGAME into private copies of the initial
scene scripts while keeping original files and original resource bytes intact.
It verifies one cycle from a 24-cycle request, no further ticks before draining
Quit, no fault, and exact combined engine/DSP save export and reload. This is
host wiring evidence, not an original-asset route reaching QUITGAME or a new
native-window/physical-device acceptance run. Simultaneous menu-operation/quit
arbitration has source-informed handling but no dedicated integration fixture.

Validation: workspace (339 core tests), all 21 local-asset integration checks,
Clippy, WASM checking and native/tools/browser release builds pass. See
[script-quit.json](validation/script-quit.json).
