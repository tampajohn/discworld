# Discworld runtime goal

Build a complete Rust replacement for Tinsel 1 that plays the supplied English DOS CD v2 edition of Discworld from start to credits. Preserve the original assets and scripts. Share a deterministic engine between a native macOS application and a browser WebAssembly application. Compatibility and maintainability take priority over delivery speed.

The long-term user-facing target is comfortable, complete play on an iPad in landscape orientation. This is an explicit acceptance target for the browser host, not an optional desktop-browser extrapolation. Native macOS remains a required host and useful development environment.

## iPad landscape acceptance

- Complete the game using touch alone, with no required keyboard, mouse, hover, or development hotkeys. Provide reachable controls for movement, action/look, inventory, conversation topics, dialogue advancement, and save/load.
- Preserve original art and fonts, with legible scaling and an intentional aspect-ratio policy. Fit landscape viewports and device safe areas without cropping the scene or hiding essential controls. Keep host control placement separate from original game-space coordinates.
- Support user-provided game assets and a clear audio-start interaction. Handle interruption, backgrounding, and return without runaway simulation, stuck input, or loss of progress.
- Persist saves across browser sessions and support export/import. Test portrait-to-landscape changes and interrupted input even though landscape is the primary play orientation.
- Validate on a real iPad with its browser and touch input. A desktop browser build, WebAssembly compilation, or pointer emulation alone does not satisfy this criterion. Record the tested device/browser versions when that validation occurs.

Touch gestures and presentation technology remain implementation decisions to validate with gameplay; the shared deterministic engine must not depend on a particular gesture or device API.

## Latest validation follow-up

Updated Finder/browser packages from exact source3df76c8 pass extracted manifests/source integrity, macOS deep signature verification, native startup/legacy-load, and Chrome/WebKit startup/load/save/audio-control smoke. Browser UI passes49 Playwright cases plus4 Node tests, with1 known WebKit multi-touch injection skip. Full-route audits are still running; visible Finder and physical iPad acceptance remain open. See [package evidence](validation/packages-3df76c8.json).

Paired replay proves the added cursor samples prevent the expected idle close-up before tick88000; they do not dismiss an already-started close-up in this case. Preserving that stationary interval passes through100000 with84 engine/DSP continuation checks. A4239-input candidate preserves all1657 original actions and stationary windows before all8 recorded close-up keys; all three full audits are running. See [close-up recovery](validation/idle-cursor-closeup-recovery.json).

Cursor activity is now representable in replay traces through the ordinary checked movement API. A revised route preserves all1657 scene-checked actions and adds2616 cursor samples; its65000-tick prefix passes with175 engine/DSP save checks. All three full audits reject a key at88000: the trace expects closeup.scn but execution is in square.scn; the cursor-input effect needs diagnosis; see [active-player replay](validation/idle-cursor-replay.json). Idle behavior remains covered separately.

IDLETIME now preserves the reference55-unit elapsed-clock scaling using deterministic virtual milliseconds. The original idle close-up triggers at tick6985 and enters at6992, compared with11043/11050 before correction. Numbered script timers and save layout are unchanged. Workspace, Clippy, native/WASM builds,25 original-asset checks and the idle diagnostic pass. All three full routes reject the old recorded input at tick24500 after7 inputs because an idle close-up interrupts a long pause; ordinary-input recovery and package follow-up remain pending. See [idle-clock correction](validation/idle-clock-scaling.json).

The late Archchancellor scene divergence is an idle close-up, not omitted dialogue: all19 subsequent voice IDs and PCM hashes match in order, and Rust reaches the same close-up at tick11050. The master script triggers it at IDLETIME >=300. Reference elapsed-time scaling and Rust idle units differ; timer compatibility remains to resolve, independently of debugger-induced audio timing drift. See [dialogue and idle-clock evidence](validation/arch-dialogue-idle-clock.json).

The extended reference inventory attempt fails its cycle6000 control precondition: ScummVM has entered closeup.scn while Rust is back in arch.scn. Later scene/dialogue state alignment needs investigation before claiming overlay fidelity; see [reference observation](validation/arch-inventory-reference-run.json).

The Rust archchancellor introduction returns control at tick3843 after a single first-line Escape. Pockets then open normally, and save/load preserves overlay pixels and next-tick state. The initial480-frame prefix still matches ScummVM; later dialogue/overlay reference comparison remains open. See [inventory completion evidence](validation/arch-inventory-completion.json).

Escape during Archchancellor entry speech matches all 480 original-reference frames at the same cycles, with save/load continuation throughout. Rust stops the active voice at the input tick; independent acoustic stop timing is outside this capture. No engine change was required; see [speech-skip evidence](validation/reference-arch-skip.json).

The bedroom → university → Archchancellor sequence matches all 480 original-reference frames on the same cycles, including entry speech with matching subtitle settings. All save/load continuations pass. See [two-transition evidence](validation/reference-arch-entry.json).

Changing destination during the university walk matches all 240 original-reference frames at the same cycles, all 73 traced mover positions/phases, and all save/load continuations. No additional engine change was required; see [retarget evidence](validation/reference-university-retarget.json).

Stationary mover rerouting now retries once in the same scheduled tick, matching original `DoMoveActor`. The university floor-walk capture improves from 182/240 to 240/240 exact same-cycle frames, with matching position/phase at 73 traced ticks and all 240 save continuations. Eleven earlier reference cases retain 72/72 matches. Workspace, 25 asset tests, Clippy and native/WASM builds pass; Browser UI checks pass (49 cases, one known WebKit multi-touch injection skip, four Node tests). All three frozen `bad0d45` routes reject recorded input at tick 1,109,020 after 544 accepted inputs; a normal discovery/return/retry sequence now completes the Brotherhood and dragon scenes. The revised 1,657-input route completes 2,635,725 ticks on all three hosts, returning to the title screen with identical final engine bytes and pixels. Native performs 5,065 engine and 5,065 DSP save/load checks. See [completed route evidence](validation/mover-retry-recovered-route.json). This does not establish physical iPad acceptance or acoustic equality. See [recovery evidence](validation/mover-retry-route-recovery.json). See [rejection evidence](validation/mover-retry-route-rejection.json). Updated packages pass extracted startup and legacy-save checks; see [package evidence](validation/packages-bad0d45.json). See [movement retry evidence](validation/mover-stationary-retry.json). The preceding `6061cc3` build completed all three full routes with equal final engine bytes/pixels; see [route evidence](validation/cursor-entry-route.json).

Scene-entry cursor initialization now follows the original restart order: entry scripts run before the replacement cursor is created, and the first animation step occurs on the following tick. The 240-frame door transition improves from 177 to 240 exact same-cycle matches; all eleven earlier captures retain 72/72 matches each. Workspace, 25 asset tests, Clippy and native/WASM builds pass. Runtime `6061cc3` completed all three full routes with identical final engine bytes and pixels. Its UI suite passed 49 cases plus four Node tests, with one known WebKit multi-touch injection skip; see [route evidence](validation/cursor-entry-route.json). Updated extracted Finder/browser packages pass startup and legacy-save checks; see [cursor-entry evidence](validation/cursor-scene-entry.json) and [package evidence](validation/packages-6061cc3.json). Visible Finder and physical iPad acceptance remain open.

Hidden-cursor trail lifetime is corrected: CONTROL_OFF/STARTOFF delete trail objects immediately, preserving ring position and suppression. The door capture improves from 62/72 to 72/72 exact same-cycle frames; ten prior scenarios also match 72/72, with full save continuation per frame. Workspace (361 core tests), 25 asset tests, Clippy and native/WASM builds pass. Browser UI validation passes (49 cases, one known WebKit multi-touch injection skip, four Node tests). Frozen f5b9a91 native/Chrome/WebKit full routes complete with 1,652 accepted inputs, 5,048 engine/DSP save checks and equal final engine bytes/pixels; see [route evidence](validation/hidden-trails-route.json); updated Finder/browser packages pass extracted startup and legacy-load checks; see [package evidence](validation/packages-ea6d560.json). Visible Finder and physical iPad acceptance remain open. See [trail deletion evidence](validation/hidden-cursor-trail-deletion.json).

A starting-room door ACTION capture exposes a hidden-cursor trail discrepancy: 62/72 full frames match; ten frames after control restoration contain 1–5 extra trail pixels in Rust. The reference deletes trails in `DwHideCursor`, while Rust retains hidden animation clocks. Door script execution, control take/restore and all 72 save continuations pass; this is a failing fidelity observation, not acceptance. This failing capture is retained; the trail-deletion correction above resolves it. See [door trail evidence](validation/reference-door-trail-observation.json).

Shrinking the Luggage from either bottom-right or top-left now matches all 72 reference frames per case, with full save continuation at every frame. Both shrink from 3×2 to 2×1 slots; top-left also moves the origin to (126,126). Both enlargement regressions retain 72/72 matches. No engine change was required; see [shrink evidence](validation/reference-luggage-shrink.json).

Top-left Luggage resizing also matches all 72 reference frames at the same cycles, including moving the origin from (100,100) to (74,74) while growing from 3×2 to 4×3 slots. All 72 save continuations pass; the bottom-right regression remains 72/72. No further engine change was needed. See [top-left resize evidence](validation/reference-luggage-top-left.json). The full-route audit of runtime 30c2f69 is complete; see [route evidence](validation/resize-cursor-route.json).

The resize cursor discrepancy is corrected in the engine and both host movement paths. Cursor shape now updates before a same-tick button edge starts dragging, and refreshes immediately on release. The Luggage sequence improves from 47/72 to 72/72 exact same-cycle frames; six prior scenarios retain 72/72 each with full save continuations. Workspace, 25 original-asset tests, Clippy and native/WASM builds pass. Browser UI validation passes 49 cases plus 4 Node tests, with one unsupported WebKit multi-touch injection skipped. Frozen 30c2f69 full-route audits pass in native, Chrome and WebKit: all 2,630,000 ticks and 1,652 inputs, 5,048 engine and 5,048 audio save checks, and identical final engine state/pixels; see [route evidence](validation/resize-cursor-route.json); see [cursor-ordering evidence](validation/inventory-resize-cursor-ordering.json). Updated Finder/browser packages containing the correction pass extracted startup and legacy-load checks; see [package evidence](validation/packages-cc0e3db.json). Visible Finder and physical iPad validation remain outstanding.

A controlled Luggage resize now exposes a cursor-ordering discrepancy: 47 of 72 frames match; 25 frames around the held resize gesture differ near the cursor. Both runtimes start/end the drag and the resulting enlarged window matches. All 72 Rust save continuations pass. This is a failing fidelity observation, not resize acceptance; see [resize observation](validation/reference-luggage-resize-observation.json). The probe directly opens inventory 1 through the library/API and does not validate its puzzle unlock. This failing capture is preserved; the subsequent cursor-ordering correction above resolves it.

The fixed-size pockets window correctly refuses a bottom-right resize attempt: all 72 full RGB frames match the pinned reference at the same cycles, with 72 save/restore continuation checks. The reference remains in drag state 0; Rust asserts equal minimum/maximum dimensions and no resize state. Earlier attempts incorrectly expected this window to resize and remain recorded as rejected probes. The initial Luggage capture exposed the subsequently corrected cursor-ordering discrepancy. No engine change was required; see [fixed-corner evidence](validation/reference-inventory-fixed-corner.json).

A controlled inventory header drag, held movement, release and close matches the pinned reference in all 72 full RGB frames at the same cycles. All frames pass complete save/restore continuation checks; no engine change was needed. See [inventory drag evidence](validation/reference-inventory-drag.json).

Inventory trail suppression now follows movement, preserving trails when a window opens under a stationary cursor. The failing cached-actor capture improves from13/72 to72/72 exact same-cycle frames; earlier inventory open/close, floor walking and voiced LOOK retain72/72 each. Workspace,25 asset tests,Clippy,native/WASM builds and23 targeted browser cases pass (one unsupported WebKit multi-touch injection case skipped). See [trail correction](validation/inventory-trail-movement.json). Updated Finder/browser packages at9186705 pass extracted startup and legacy-load checks; see [package evidence](validation/packages-9186705.json). The frozen 9186705 native/Chrome/WebKit full-route audits completed all 2,630,000 ticks and 1,652 inputs, with 5,048 engine and 5,048 DSP save checks and identical final engine state/pixels. All 17 frozen browser files, binary and trace hashes were verified; see [route evidence](validation/inventory-trails-route.json). Combined DSP saves differ across hosts; this does not establish acoustic equivalence, full reference compatibility or physical iPad acceptance.

Timed world clicks now use the original scheduled cached tag before coordinate fallback. The immediate move/click discrepancy is resolved: both immediate and delayed scenarios match all 72 reference frames at the same cycles, with pre-click and per-frame save continuation checks. Workspace, 25 original-asset cases, Clippy, native/WASM builds and 49 browser UI cases pass; one unsupported WebKit multi-touch injection case is skipped. Full-route reruns for this engine change passed in native, Chrome and WebKit, with identical final engine state and pixels; see [route evidence](validation/cached-click-route.json). Updated Finder and browser packages pass extracted startup/legacy-load checks; see [package evidence](validation/packages-07e395f.json). See [cached-click evidence](validation/cached-world-click.json).

A controlled player WALKTO on the starting-room floor now matches the pinned reference in all 72 full RGB frames at the same cycles, including walking and the standing transition. This uses one hover-update cycle before clicking; immediate move/click cached-tag behavior remains separate. No engine change was needed. See [walking evidence](validation/reference-player-walk.json).

Browser package `a4020ae` includes focus/audio interruption cleanup and primary-touch ownership. Its extracted site passes 17 Chrome/WebKit original-asset checks; WebKit simultaneous-touch injection is explicitly skipped. See [browser package](validation/browser-package-a4020ae.json). The pinned Finder bundle passes extracted signature, manifest, source and compiled-launcher integrity checks; [visible Finder interaction remains unverified](validation/finder-pinned-package.json).

The inventory-close runtime (`0a87557`) completed the unchanged 2,630,000-tick route in native, Chrome, and WebKit: all 1,652 inputs accepted, 5,048 engine and 5,048 audio snapshot checks, and identical final engine state and pixels. See [route evidence](validation/inventory-close-route.json). This does not establish full-game original-engine equivalence or physical iPad acceptance.

The extended voiced LOOK diagnostic records all 144 frames and reference mixer sample counts. The reference consumed the full original recording but ended speech at different game cycles with different debugger instrumentation. The instrumented capture failed the fixed cursor-position acceptance gate when speech restored the cursor at frame 70. This is recorded as an observation, not a fidelity pass; no runtime speech duration was changed. See [speech completion evidence](validation/reference-look-completion-observation.json). The subsequent [emitted PCM check](validation/look-emitted-pcm.json) verifies all 101,171 original samples through the integrated runtime, a silent final-block tail, and control/cursor restoration on completion; all 144 captured frames remain unchanged. Same-cycle reference completion under an unperturbed mixer remains open.

## Architecture

The engine core forbids unsafe Rust and has no host filesystem, window, audio device, or wall-clock dependencies. Asset readers validate lengths, offsets, identifiers, and resource handles. A deterministic cooperative scheduler owns explicit, serializable script stacks and suspended operations. Save state includes scripts, actors, inventory, timers, scene state, and random generator state.

CPU composition preserves the original framebuffer and palette behavior. The current native host presents through SDL2 and the browser through canvas; the initial wgpu/Metal presentation proposal has not been implemented. Hosts supply input events, asset access, audio output, persistence, and lifecycle handling. Browser execution must yield to the host and cannot assume native threads or blocking filesystem access.

ScummVM is a pinned behavioral reference. Its source informs format and behavior investigation; it is not a linked runtime dependency. Source translations require appropriate attribution and licensing. Do not describe this work as a clean-room implementation. Analyze DWB.EXE when an observed discrepancy requires additional evidence.

## Milestones

1. Inventory and hash the ISO, pin the reference, document resource formats, and establish a reproducible reference run.
2. Build checked resource readers and tools for scenes, sprites, palettes, fonts, text, audio, and bytecode.
3. Render the same real scene on macOS and in the browser.
4. Execute original scripts with observable engine calls and deterministic scheduling.
5. Complete a playable puzzle sequence with movement, dialogue, inventory, audio, scene changes, and save/reload.
6. Implement all behavior required by all acts, cutscenes, and ending.
7. Validate alternate interaction orders, regressions, persistent saves, and packaged native and browser hosts.

## Completion criteria

A start-to-credits playthrough completes without required behavior stubbed out. Save/reload works across every act and suspended operations. Repeatable input traces compare relevant game state, frames, and audio events with the reference. Known script fixes have explicit compatibility decisions and regression coverage. The same core runs on macOS and in a browser using user-supplied assets.

Initial scope is this exact ISO and Apple Silicon. Other releases, Intel Macs, and ScummVM save compatibility are later scope decisions. Original assets remain local and excluded from version control; public hosting of game assets is not part of the goal.

## Visible playthrough policy

Keep the user's visible native session current with newly verified engine builds and replay milestones. After validation, reconstruct the latest useful watch segment, verify its window is advancing, then close the superseded instance. Preserve the user's playback speed and mute settings, and honor deliberate manual pauses. An automatic recorded-route endpoint pause may be replaced with the newly verified segment. Report preparation time and the new checkpoint; do not present unverified routes as working. This is ongoing user authorization to update the visible session without asking each time.

## Current progress (2026-10-23)

Earlier engine milestone `10b3a2c` schedules movers at their original process insertion
positions and preserves that order in version 26 saves. Together with hidden
standing-reel suspension and destination path-entry completion, this matches
all 72 controlled reference-room frames at the same rendered cycles. The
workspace suite, 25 original-asset checks, Clippy and WASM checks pass. Its
native/Chrome/WebKit full-route runs pass with all 1,652 inputs accepted and
identical final engine state and pixels; all 44 browser UI cases and four Node
audio/clock tests pass. See [scheduled-mover route](validation/scheduled-movers-route.json).
The preceding hidden-standing build completed all three routes with identical
final engine state and pixels; the path-entry build completed the native route.
See [scheduled movers](validation/scheduled-movers.json),
[hidden-standing route](validation/hidden-standing-route.json), and
[path-entry route](validation/path-reentry-route.json).

Legacy player-load migration passes original-asset and targeted browser UI
checks. Old saves did not retain
mover creation positions, so migration preserves their known ascending-actor
order before existing script peers; it cannot reconstruct missing historical
order. Exact snapshot decoding/re-export remains unchanged. Packages at `70fa619` pass extracted startup and legacy-load checks in native,
Chrome and WebKit; see [package evidence](validation/packages-70fa619.json).
The visible session, broader original-reference comparisons, and real iPad
acceptance remain unfinished. The goal is not complete.

The bytecode audit identified missing world hover (843 PRINTTAG sites). New
scenes now schedule PointProcess/TagProcess, dispatch original POINTED scripts,
register PRINTTAG, render cached original-font labels with actor priority and
save the state in DWRSAVE v24. The original Rincewind-room integration checks
five polygon labels and exact save/frame continuation. See
[world-hover.json](validation/world-hover.json).

Player loads now migrate legacy rooms without re-entry while preserving script
order/sleeps and active scrolling. Exact replay restoration stays unchanged.
Validation: 354 core tests, 25 original-asset tests, and all 44 Chrome/WebKit
UI cases pass (40 in the full rerun, four corrected cases in a targeted rerun); see
[hover-migration.json](validation/hover-migration.json).
Deferred POINTED save continuation passes, and a regression exposed/fixed
missing hover geometry for non-moving actors in blocking PLAY. Current validation:
356 core tests, 25 original-asset tests, Clippy, WASM and rebuilt hosts pass. See
[blocking-actor-hover.json](validation/blocking-actor-hover.json).

The frozen 61b9d73 native/Chrome/WebKit full-route audits completed 2,630,000
ticks, accepted all 1,652 inputs, restarted and returned to title.scn. Final core
bytes and RGBA match exactly; native completed 5,048 engine and 5,048 DSP save
checks. Browser audio is finite, but DSP saves are not byte-identical across
platforms. This audit includes the blocking-PLAY hover fix and cursor trails.
See [cursor-trails-route.json](validation/cursor-trails-route.json). Remaining:
independent interactive hover/reference comparison, updated visible session,
physical iPad acceptance, Finder application delivery and broader fidelity gates.
Current development packages are refreshed at 61b9d73 with original cursor
trails and version-25 saves. Extracted native and Chrome/WebKit bundles pass
startup and legacy-load checks outside the checkout; browser post-advance
exports use v25. See [packages-61b9d73.json](validation/packages-61b9d73.json).
The integrated 72-frame controlled reference comparison matches 58 images
exactly (54 at the same capture cycle); remaining animation-phase differences
are unresolved. All 44 browser UI cases are covered across the full run and
corrected drag-test rerun. Frozen native/Chrome/WebKit full-route audits on this
runtime pass; see [cursor-trails-runtime.json](validation/cursor-trails-runtime.json).
No visible native process was running at the last process check; a current
manual session still needs to be launched and verified advancing.

PLAY/TOPPLAY completion arguments now match the pinned V1 integer dispatch:
`1` blocks, `2` errors, other values play asynchronously. A 42-case bytecode
regression covers caller precedence and queued save/load continuation. All 342
core tests, 24 original-asset tests, Clippy, WASM and host builds pass. The latest
full-route audit below predates this small correction; see
[play-completion.json](validation/play-completion.json).

Scrolling now runs as a scene scheduler task, fixing all 351 remaining aligned
startup-frame mismatches: all 1,000 reference frames match at aligned ticks.
Save version 23 retains task order and scroll initialization; older formats
remain byte-exact until scene entry. Workspace checks (341 core tests), all 24
original-asset integration tests, Clippy, WASM and host builds pass. All 44 browser
UI checks pass in Chrome/WebKit. The native full-route audit passes 2,630,000
ticks, all 1,652 inputs and 5,048 engine plus 5,048 audio save/load checks.
Chrome and WebKit also pass all 2,630,000 ticks and 1,652 inputs, with exact final
engine bytes and pixels matching native. Both generate finite audio; combined
DSP saves differ, so cross-platform audio equality is not established. See
[scheduled-scroll.json](validation/scheduled-scroll.json).

The independent reference probe also verifies the two startup SFX sample starts
and raw PCM payloads against Rust, while retaining all 1,000 aligned image
matches. Rincewind speech, mixed audio output and device listening remain
separate acceptance work; see [reference-startup-audio.json](validation/reference-startup-audio.json).

The preceding second-credits investigation traced the mismatch to camera scheduling:
the reference scroll process applies the first movement in the requesting
entrance script's cycle; Rust's earlier camera phase defers it one tick.
A new independent capture reproduces all 1,000 prior reference images and
confirms the offset across 362 camera updates. Read-only probes and Rust frame
capture diagnostics now expose that ordering. The correction above follows that evidence;
see [scroll-phase.json](validation/scroll-phase.json).

A new independent 1,000-frame reference capture exposed and drove a font
palette correction. All 1,000 captured images now have exact RGB matches;
351 second-credits frames still match one Rust tick late. The source and
pixel evidence, failed capture attempts, and validation scope are in
[longer reference comparison](validation/reference-software-frames.json).

Continuing master/scene scripts now implement QUITGAME with a cycle-boundary
host handoff and the reference V1 CDLOAD behavior. Same-cycle Save/Load plus
Quit preserves the selected slot and delivers Quit afterward without additional
simulation. Workspace tests (340 core), 22 local-asset integration checks,
Clippy, WASM checking and both release hosts pass. See
[CDLOAD and quit collision evidence](validation/cdload.json). These additions
are newer than the route results below.

Runtime `fad89dc` defers restored actor animations until after saved movers
are installed and preserves reel jobs across interpreter restoration. Pending
restored jobs have version-22 save support; explicit saved depths leave shared
column depth unchanged. The subsequent `c9fdccf` regression covers four-actor
queue order, signed 16-bit saved fields, and exact save/load continuation.
All 338 core tests and core Clippy pass. The production change also passed the
workspace, 20 original-asset checks, WASM checking, and native/browser builds.
See [restoration evidence](validation/deferred-restored-reels.json) and
[reel scheduling](reel-scheduling.md).

The earlier `85b728a` recorded route completed on native, Chrome and WebKit:
1,652 accepted inputs, restart/title at 2,630,000 ticks, identical final engine
bytes and RGBA. Native passed 5,048 engine and 5,048 DSP continuation checks;
browser audio was finite, but cross-platform audio equality is not established.
See [cross-host evidence](validation/reel-dispatch-browsers.json).
The `fad89dc` native, Chrome and WebKit routes have completed with matching
final engine bytes and RGBA. WebKit completed on a preserved-build retry; its
first attempt failed after 2.4 million ticks when its browser closed. The first
failure is retained and its cause remains unestablished. See
[restoration route evidence](validation/deferred-restore-route.json).
The production host passed four Node tests and all 44 Chrome/WebKit interaction
cases in one run; see [UI evidence](validation/deferred-restore-browser-ui.json).
These results predate scripted QUITGAME support, described in
[script runtime](script-runtime.md).

The startup scheduling correction resolves the 17 scene-aligned mismatches
reported in [historical phase evidence](validation/reference-startup-phase.json).
A fresh capture from `fad89dc` matches all 120 existing independent reference
draws across Psygnosis and TWG, with zero scene-aligned mismatches. See
[startup recheck](validation/deferred-restore-startup.json). This is startup-only
evidence, not proof of general scheduling or wall-clock equivalence.

The goal remains incomplete. Live-reference reconstruction ordering, broader
scene/state/audio comparisons, alternate interactions and remaining library
operations, updated packaged hosts, visible-session acceptance, and physical
iPad landscape/touch/lifecycle acceptance remain outstanding. Historical
results below retain their own revision and validation scope.

A fresh, read-only LLDB capture from the pinned ScummVM executable provides
an independent pixel comparison: the complete 320x200 Psygnosis logo at Rust
tick 24 matches reference draw ordinals 7–50 exactly. All 156 reference asset
files match the supplied ISO. See [reference pixel evidence](validation/reference-logo-pixels.json).
This validates that one fully visible image; animated frames, fades, scheduler
timing and later scenes remain outside its scope. The orientation-corrected
full native route at 29be6d6 completed separately.

Runtime `9ba83fb` completed the recorded route in native, Chrome and WebKit,
accepting all 1,652 inputs. Final engine save bytes and pixels match. Native
passed 5,048 engine and 5,048 DSP continuation checks; browser audio was finite.
See [cross-host evidence](validation/internal-film-9ba83fb.json). This run
predates the animation orientation correction at `d702cd1`.

A new diagnostic compares the production animation interpreter with the
historical live ScummVM capture: all 603 observed opcode/index pairs match
across 13 instances; five unobserved instances are excluded. Raw-log hashing,
normalization and deliberate-corruption rejection were checked. See
[reference comparison](validation/reference-animation-prefixes.json). This is
instruction-prefix evidence, not frame, timing, scheduling or audio equivalence.


Automatic mover creation runtime `0ccdf9f` completed the native recorded route:
all 1,652 inputs accepted, 5,048 engine and 5,048 audio continuation checks,
and restart/title at tick 2,630,000. Final engine bytes match `6167511`;
see [mover creation route evidence](validation/created-mover-0ccdf9f.json).
This includes implicit film-coordinate correction and automatic creation of
declared movers during PLAY/SPLAY/TOPPLAY. Dedicated tests cover creation
arbitration, explicit hiding and independent SPLAY secondary cleanup. Broader
mixed-column and live-reference comparisons remain outstanding.

Subsequent runtime `d39a279` implements FRAMEGRAB, RUNMODE and scripted
SUBTITLES in continuing master and scene processes. Subtitle changes share
live/retained menu state and survive resource-validated save/load. Workspace
checks (330 core tests), Clippy, the original-menu regression and all 20
original-asset web tests pass; both release hosts build. The completed route
above predates these library additions. See [script runtime](script-runtime.md).

The audits below retain their revision-specific scope; newer builds do not
inherit a claim of full-route or device acceptance from an older audit.

SPLAY runtime `6167511` completed the native recorded route with all 1,652
inputs accepted, 5,048 engine and 5,048 audio continuation checks, and final
restart/title at tick 2,630,000. Its production browser host passed four Node
clock/audio tests and 44 Chrome/WebKit UI cases. See
[native evidence](validation/splay-6167511.json) and
[browser UI evidence](validation/browser-splay-6167511.json). Both audits
predate the implicit mover film-coordinate correction at `34da839`.

Continued movement under PLAY at `a768ad9` completed the recorded native
route: all 1,652 inputs accepted, 5,048 engine and 5,048 audio continuation
checks, and restart/title at tick 2,630,000. See
[PLAY movement route evidence](validation/play-movement-a768ad9.json). This
run predates the STAND visibility correction and SPLAY infrastructure. SPLAY
now connects dispatch, restoration and v19 cleanup state. Focused blocking,
nonblocking, Escape and replacement tests pass. Subsequent secondary-actor
cleanup tests are described in [mover compatibility](mover-reel-compatibility.md);
broader alternate-route coverage remains outstanding.

WALKPOLY runtime `b692a3b` completed the recorded native route through title
restart at tick 2,630,000, accepting all 1,652 inputs and passing 5,048 engine
and 5,048 audio snapshot checks. See [route evidence](validation/walkpoly-b692a3b.json).
HIDEACTOR integration at `fdbdae2` subsequently added separate mover visibility,
background ordering, ownership-aware unhide, and exact save continuation. Its
focused and original-asset tests pass; its recorded native full-route audit
also completed: all 1,652 inputs accepted, 5,048 engine and 5,048 audio
continuation checks, and restart/title at tick 2,630,000. See
[HIDEACTOR route evidence](validation/hideactor-fdbdae2.json). This run predates
PAUSE, FADEMIDI, and continued movement under PLAY.
The visible manual session has not yet been refreshed.

The disabled-axis runtime at `a1eaead` completed the native recorded route:
all 1,652 inputs accepted, 5,048 engine and 5,048 DSP snapshot checks, and
restart/title at tick 2,630,000. See [native evidence](validation/disabled-axis-a1eaead.json).
It predates immediate competing-walk cancellation at `27d9924`; that revision's
same-build native/Chrome/WebKit audit is complete with identical final engine
bytes and RGBA on all hosts. All accepted 1,652 inputs and reached restart/title
at tick 2,630,000; native passed 5,048 engine and 5,048 DSP checks. See
[cross-host evidence](validation/walk-token-27d9924.json). No additional
disabled-axis inputs were added to the recorded route. This evidence predates
WALKPOLY support, which has separate focused tests.

The intro-aware native runtime at `1ac8f90` completed the recorded route:
2,630,000 ticks, all 1,652 inputs accepted, restart committed and final
`title.scn`. It passed 5,048 engine and 5,048 DSP snapshot audits; the final
image was inspected. See [native audit](validation/intro-startup-1ac8f90.json).
This route contains no Escape inputs. The subsequent `cbb7d6b` host Escape
routing has focused keyboard/touch tests. Its separate full cross-host audit
is now complete: native, Chrome and WebKit each reached restart/title at
2,630,000 ticks with all 1,652 inputs accepted. Both browsers match native
final core bytes and RGBA exactly. Native passed 5,048 engine and 5,048 DSP
snapshot checks; browser generated audio remained finite. Combined DSP saves
differ across platforms. See [cross-host evidence](validation/escape-host-cbb7d6b.json).
This audit predates the later null-film validation correction and contains
no Escape inputs; the focused Escape tests remain separate evidence.

Current development archives at `dfa0589` passed extracted-package startup
checks on native macOS, Chrome and WebKit, including the optional audio
headroom control. These include scheduled scrolling and the font palette fix.
See [package evidence](validation/packages-dfa0589.json).
These checks do not establish full-route or physical-device acceptance.


Earlier completed cross-host route validation: runtime `f776192` passes 2,630,000 ticks in
native, Chrome and WebKit with exact final engine state and pixels. Native
passes 5,048 engine snapshot and 5,048 DSP continuation audits. The new walking
speech behavior has separate interruption, scene-reset and original-asset
save/load tests; explicit mover reel state uses DWRSAVE v13. See
[recorded evidence](validation/custom-reels-f776192.json). Browser and native
command-line development archives have been tested outside their source
folders; native Finder app packaging and physical iPad acceptance remain.


Next engine compatibility work: [mover reel modes and walking speech](mover-reel-compatibility.md). Walking TALK now pauses movement and resumes the existing route, with versioned reel state and interruption/save-continuation tests. New stationary speech also uses the shared reel slot; old active waits migrate at the next tick without rewriting imported bytes. WALK, WALKED, WALKTAG and SWALK accept custom films; disabled-axis direct walks now retain sentinel destinations and have mover/script continuation tests; ambiguous historical overlapping slots remain. [Script escape cancellation](script-escape-compatibility.md), intro routing and host Escape input now have focused coverage; broader reference comparisons remain.

Runtime revision `00a6b38` has runnable native and browser hosts, original menus,
named save/load, original-font name editing, independently controlled music/
speech/effects, and persistent preferences. DWRSAVE v12 preserves active speech
ownership; earlier versions remain readable. Native storage uses atomic slot
files; browser storage uses IndexedDB and the same slot/session codecs. Menu
restart uses the shared engine's cycle boundary. The browser provides a touch
keyboard endpoint while the original game font draws the name.

The recorded 1,652-input route previously passed Acts I–IV, ending, credits and
restart in native, Chrome and WebKit, with matching final core state and RGBA.
A fresh full-route run of `00a6b38` now passes in native, Chrome and WebKit:
all 1,652 inputs accepted, credits and committed restart reached, and final
`title.scn` at tick 2,630,000. Final engine bytes and RGBA match across all three.
Native passed 5,048 engine snapshot/preview/continuation checks and 5,048 DSP
checkpoint checks. Combined DSP saves still differ across platforms. See the
[tracked validation summary](validation/menu-runtime-00a6b38.json); bulk local
outputs are under `assets/validation/menu-runtime-00a6b38`.
These are accelerated semantic-input runs, not complete manual UI or physical
touch playthroughs. The later `a6fc4a3` zero-voice-volume correction has separate
original-intro validation and was not part of the full-route binaries.

Historical menu-runtime evidence includes workspace tests, Clippy, all 14 original-asset
Rust web integration tests, desktop and emulated-touch menu save/name-edit/load
in Chrome/WebKit, audio continuation tests, and active-drag restoration with
ordinary and swapped mouse buttons. Browser test captures show original menu
backgrounds, buttons and fonts. See the scoped logs and limitations in
`docs/control.md`, `docs/browser-runtime.md`, `docs/audio-runtime.md` and
`docs/save-state.md`.

The full objective remains incomplete. Remaining acceptance work includes:

- Physical iPad landscape play, keyboard appearance, viewport/orientation changes,
  interruption recovery, and a touch-only complete game playthrough.
- End-to-end native menu interaction and current-build visible-session acceptance;
  programmatic tests do not substitute for SDL click/typing/device validation.
- Broader alternate interaction orders and reference comparisons. Custom walking
  films and moving-actor TALK are implemented with focused tests. Disabled-axis
  direct-walk collision/entrance combinations and ambiguous historical overlapping
  speech slots need further coverage;
  one recorded route cannot establish every supported-game path.
- Full-route checks of packaged hosts, native Finder application delivery, and
  a final requirement-by-requirement audit, including reference
  state/frame/audio-event comparisons.

The existing visible native session is kept running to preserve unsaved progress;
its binary is older than the latest rebuilt executable. A process being alive is
not evidence that it is advancing or displaying the latest engine.

## Historical milestone log

The entries below retain earlier debugging evidence. Their statements about
then-unimplemented behavior are historical, not the current status above.

Latest checkpoint:2630000 Running in title.scn after the original ending, scrolling Credits4 and committedrestart (has_restarted=true). All1652 inputs in act4-ending-input.json accepted; finaltitleframe inspected. Credits4 begins2603567 and restart2604319; originalstartup returnsTitle2604708. This verifies the recorded Acts I–IV start-to-credits route, not the full compatibility objective. Native ending visual validation is queued; durable saves, browser host, landscape iPad validation and broader reference/interaction coverage remain unfinished. All217 workspace tests pass with the restart audio-clock correction.

Latest puzzle validation: Acts I and II are complete in the cumulative original-input replay. Act III spell, scissors, knife, rubber belt, waiting-room dialogue, paper and autograph are verified. evidence/headless-act3-autograph-short-walk.json reaches 1761000 Running in square3.scn with control enabled and autograph 189 held after the psychiatrist scene; every input passes and the final frame was inspected. The visible session is being updated to this verified build/route. Vendor bag 271 is verified through 1765500; the paper-bag exchange opens the palace and reaches dungeon initialization. Dungeon sword 306 is verified through tick 1784500 after using crank 216; every cumulative input passes, control is restored and the final frame was inspected. Palace supplies, toyshop exchange, closet item 304, kitchen item 301 and Shades hero item 299 are verified through 1840500 with all inputs accepted and control restored. Act III hero counter is 6 of 6; dining components and the inn whistle are verified through tick1901000 with all1106 inputs accepted, control restored and the final frame inspected. Worldedge parrot capture is verified through tick1928500 with all1132 inputs accepted and the final frame inspected. Hat/descent/whistle recovery is verified through tick1966500 with all1158 inputs accepted and control restored. Sailor exchange is verified through tick1989000 with all1169 inputs accepted and clue220=1. Birthmark/barber-return and SETPALETTE rendering are verified through tick2043000, all1198 inputs accepted, birthmark192 held and control restored. Prepared brush205 is verified through tick2078000 with all1226 inputs accepted and control restored. Cart inscription, rooftop knife/rope interaction, beggar return and moustache264 are verified through2150500 with all1285 inputs accepted and control restored. Mine discovery, sword discussion, barman clue and bogeyman introduction are verified through2217000 with all1329 inputs accepted and control restored. Freeing the bogeyman and setting the cellar-lighting flag are verified through2240000 with all1351 inputs accepted and control restored. Filled-tankard delivery and upgraded sword307 are verified through2273000 with all1384 inputs accepted and control restored. Imp recovery and loaded camera247 are verified through2339500 with all1439 inputs accepted and control restored. Ramkin supplies, fish-shop picture, custard tart and truth potion321 are verified through2433000 with all1506 inputs accepted and control restored. Sheep photography and completed picture279 are verified through2451500 with all1529 inputs accepted and control restored. Picture mounting, truth-treated drink and gorge discovery are verified through2485500 with all1554 inputs accepted and control restored (198=1). Temple eye228 and the full escape back toGorge are verified through2530000 with all1601 inputs accepted and control restored. The original Climax and ActIV transition complete, restoring control onAnkhmap at2537148 (281=4). ActIV cage key252 is verified through2543500 with all1608 inputs accepted and control restored inFinale. Dragon-cage approach is under test. Original scripts and assets remain unchanged. Remaining Act III and later gameplay, durable save/load and browser/iPad hosts remain incomplete.

Latest engine validation: INVDEPICT now applies persistent inventory-film replacement with the pinned held-cursor/redraw behavior. All212 workspace tests, workspace Clippy all targets, core WASM check, formatting and release builds pass; cumulative camera gameplay validation passes through2339500 with all1439 inputs accepted. Previous validated change: SETPALETTE now applies validated scene-local palette replacement to shared images, game fonts and translucency, with retained high colors and reset on scene/background replacement. All211 workspace tests, workspace Clippy all-targets with warnings denied, core WASM, formatting and native/tools release builds passed. The full birthmark replay exercises palette alternation/restoration through tick2043000. Native post-replay manual controls have also been visually checked. ACTORSON, short-WALK, cursor-query, adjacent-path and short-SWALK fixes remain covered. Hardware palette-slot relocation and script escape cancellation remain open compatibility gaps.

Latest verified extension: the witch cottage and Edge of World routes run successfully. At tick 930000, worldedg.scn is Running with control enabled and lamp 255 held; the original pickup enables the fork interaction. The next replay reaches 940000 with coconut 212 held, lamp stored and net retained after shaking the tree and collecting the fallen coconut. No additional engine fix was needed for these routes. Native replay titles now show time until the next recorded input and explicitly identify the recorded endpoint.

Latest status: the cumulative original-input replay completes Act I, delivers all five ingredients, walks the detector map to its destination, runs the barn cutscene and Act II title, and returns to ankhmap.scn. evidence/headless-act2-timers.json reaches tick 620000 Running with control enabled and empty hands. Original scripts add Act II items 316–319 to Luggage. The new V1 timer bank supports master and scene calls, persists through scene entry and advances deterministically after scheduling. All 206 workspace tests, Clippy, core WASM check and native/tools release builds pass. Native rendering, speech/music, pause/speed/mute replay controls and original inventory/tutorial backing are implemented. A native relaunch reconstructed from replay has been checked; this is not persistent engine save/load. Act II library dialogue, the four-treasure exchange, golden-banana handoff and L-space traversal now run through original inputs to pastlibr.scn at tick 651000. The bookcase door and exit now reach pastankh.scn at tick 659000 with control enabled, after adding scene-level WALKINGACTOR dispatch. The next original-input route reaches the past Broken Drum exterior, its interior and the barman conversation at tick 666000 (pastbar.scn); the overlay and five topics are visually verified. Further recorded inputs complete the barman topics, patron dialogue, past inn exterior and door opening, reaching pastinnr.scn at tick 691000 with control enabled; bedroom rendering is visually verified. The return route through L-space reaches present library.scn at tick 700000, and subsequent university/map/inn inputs reach innroom.scn at tick 713000, both with control enabled and visually inspected final frames. Sheet pickup, ordinary storage in Luggage and bubble-bath pickup now reach ensuite.scn at 724000 with control enabled, held item 206 and stored sheet 293. The corrected bathroom return route then reaches barn.scn at 738000 with screwdriver 292 held and bubble bath/sheet stored in Luggage. Return to the present Broken Drum, scared-patron dialogue, wine order and glass pickup now reach bar.scn at 758000 with control enabled and item 238 held. Cornflour pickup and storage followed by another L-space traversal reach pastlibr.scn at 780000 with control enabled and all collected supplies retained. The original sheet disguise and first jewelry-box attempt reach pastinnr.scn at 797000 with control enabled, ghost sprites active and global 192 set; no gate pass is acquired yet. The subsequent present-day hammer dialogue and second past-inn attempt acquire gate pass 273 at 849000 with control restored; both disguise removal and reuse are verified. Returning the pass to the present guard consumes it and opens the city gate at 872000, with control restored; City Gate Action then reaches discmap.scn at 876000 with control enabled and a visually inspected world-map frame. Woods entry, well operation and screwdriver removal of crank 216 are verified through 888000 in lwood.scn with control restored. The Dark Wood route reaches mountpas.scn. A constrained-walk occupied-destination fix, matching pinned SetNextDest, removes Luggage’s collision stop at 896744; the full replay now runs to 900000 with control enabled, and a second exit click after the original encounter reaches discmap.scn at 904000. Selecting Dark Wood again reaches mystwood.scn at 908000 with control enabled and a visually inspected frame. All 206 workspace tests, Clippy, WASM check and release builds pass. Later Act II and subsequent gameplay, palette lifecycle/rendering parity, durable saves, browser hosting and real-iPad validation remain incomplete. The watch launcher uses the L-space route, and a native replay endpoint re-pause bug is fixed. Detailed chronological evidence and known limits are in docs/text.md; entries below retain milestone history.

The workspace contains safe Rust ISO, resource, image, and palette readers. Offline PNG export renders the original title image and individual scene images. All 66621 image records pass indexed tile decoding, with per-resource pixel and coverage hashes recorded in evidence/image-audit.json. The bytecode VM and scene readers now probe 2071 original script entry points, stopping at engine calls. The master startup executor applies 117 declaration calls and stops at the first NEWSCENE request after 891 steps. Film/reel readers are present. A restricted headless engine now animates and renders the first logo, transitions to TWG, produces PCM audio, and reaches the next scene request at tick 155. Full scheduling, rendering compatibility, speech/MIDI/device audio, and most engine-library behavior remain unimplemented. The core compiles for WebAssembly; no browser host exists yet.

The pinned reference now builds and executes the original game using Cocoa/OpenGL. A bounded launch harness records normalized animation operations; the first successful run captured 18 animation instances and 603 operations. Live input occurred during that run, so deterministic replay and pixel comparison remain outstanding. Next work is cooperative scheduling, actor/polygon scene initialization, and the shared presentation spike. Asset audit success and a reference smoke run do not establish gameplay compatibility.

The checked polygon reader audits all 2033 records, 1687 path nodes, and 886 polygon script handles. The cooperative queue now dispatches scene and entrance scripts with separate waits and scene-generation cancellation. Master and animation scheduling remain provisional. The same actor/polygon boundary is now reached at tick 154 because new scene scripts can dispatch in the same pass.

Latest execution advances through Perfect Entertainment and the initial credits screen into title.scn at tick 361. The next unsupported operation is ENABLEMENU. Basic actor startup contexts and input-wait yielding are implemented; full actor operations, menu state, and polygon scene behavior remain outstanding. See evidence/headless-waitkey.json.

ENABLEMENU now records the startup permission flag; opening and rendering the configuration menu remain unimplemented. Latest execution stops at PLAYMIDI in title.scn at tick 361 (evidence/headless-menupermission.json). The new checked music reader audits all 154 XMIDI tracks and 198410 events; sequencing, synthesis, and playback integration remain outstanding.

XMIDI sequencing and offline SoundFont synthesis are now implemented. All 154 tracks sequence for 60 seconds, producing 296063 messages; the first track renders to a non-silent ten-second WAV. The new audio crate compiles for WebAssembly and uses a pinned RustySynth copy with checked PCM decoding and unsafe code forbidden. Engine PLAYMIDI integration, live playback, and independent audio fidelity checks remain outstanding.

PLAYMIDI/STOPMIDI and blocking music waits are now integrated with the deterministic core. The headless host can synthesize and mix music when supplied a SoundFont. The title script selects offset 40512 and stops later in the same tick on CONTROL; no MIDI events have yet been emitted by that stopped game run. See evidence/headless-midi2.json.

CONTROL modes, logical cursor state, main cursor composition, and IDLETIME now advance execution to PRINT at tick 392. The original title screen renders and the integrated host emits music (106 messages; nonzero PCM after title music starts). Latest report: evidence/headless-cursor.json. Full cursor/input semantics, text display and speech handling, menus, movement, and reference comparisons remain outstanding.

PRINT, checked English string/font readers, centered glyph composition, and per-process text/speech waits now render the original start prompt at tick 400. TALKAT/TALKATS now support actor-colored positioned speech and sustain semantics. Injecting a key event at tick 500 renders the turtle narration at tick 800 and advances through deferred scene transitions into rinceroo.scn at tick 8548 before SWALK; without input, the title idle path stops on SAVESCENE at tick 602. All 6850 strings and both 199-glyph fonts pass the structural/layout audit. Primary-reel blocking playback now lives in its caller and resumes on that reel ending; looping secondaries do not delay it. Tests cover peer-script progress, coordinate rules, and cloned-state replay. RANDOM now follows the pinned generator and V1 bounded no-repeat behavior with explicit cloned state and default seed 1. Polygon initialization now stores V1 geometry, inclusive point tests with blocking-corner exclusion, path adjacency, and default refer state; V1 correctly sends no tag startup events. Per-actor reel ownership and playback generations now prevent duplicate current reels and release replaced blocking callers safely. The brethren replay has three independent reels instead of seventeen. Deferred cut/default/fade transitions now use the reference countdown and fade tables, with fade-in triggered by BACKGROUND. SETACTOR now re-enables dead/completed actors and queues their current-scene startup scripts; old scene script handles are cleared on entry. OFFSET now translates world background/reels while preserving screen-space cursor/text; scene entry resets the world origin. Scripted SCROLL now supports default/explicit speed, bounded movement, blocking completion, superseding requests, and OFFSET cancellation. Normal/talk film arbitration now records the latest requested film and restores the latest deferred normal film; multipart bounds support actor-relative subtitle placement. Non-mover TALK now connects speech, deferred subtitle creation above actor bounds, camera-relative text, left-click cancellation, and normal-film restoration. KILLTAG/SETTAG and KILLEXIT/SETEXIT now preserve enabled state by polygon table and ID across visits. Non-mover KILLACTOR now clears position and reel ownership, releases blocked reel callers, and prevents new dead-actor playback; SETACTOR can revive it. Initial mover placement now resolves lead aliases, direction constants, path scales, node snapping, and standing/base-walk reels. The real room request resolves to actor 7, position (200,178), left, path 1, scale 1, standing film 351060. STAND now creates a persistent mover object from its base walking reel, selects the standing animation, and renders Rincewind in his room. V1 direct-step and facing calculations are now implemented and tested, including diagonal integer rounding, no-overshoot steps, path reel constraints, and small-turn suppression. SWALK remains unsupported until movement state/completion and collisions are integrated. Next runtime work includes SWALK/path movement and full mover lifecycle, mover/direction TALK and mover lifecycle, palette-faithful fades, full reel scheduler order, polygon interaction/persistence, and scene save/restore; native/browser hosts, full interaction, and reference equivalence remain unfinished.

Latest movement follow-up: constrained re-entry onto a node path now uses normal node routing after the first off-path step, including re-entry with an unchanged stored path handle. Regression and all 206 workspace tests pass, as do Clippy, core WASM and release builds. The full brotherhood-entry replay moves Rincewind past the old 1113001 stop to [87,155], then exposes a separate Luggage ordinary-path collision at tick 1113264. Meeting entry is still unverified; see docs/text.md.

Brotherhood meeting now verified: ordinary-path re-entry resumes graph routing, and occupied-target route probes shorten the destination to the reachable position as pinned SetNextDest does. The full unchanged replay reaches 1120000 Running in pasthide.scn after brethren.scn and dragon.scn, with control restored and every input accepted. Regression coverage, all 206 workspace tests, Clippy, core WASM and release builds pass. Later Act II, subsequent acts, durable saves and browser/iPad hosts remain incomplete.

Latest supplies milestone: headless-act3-truth-potion-timing.json reaches2438000 Running in witch.scn, control enabled and all1506 cumulative inputs accepted; final frame inspected. Ramkin access, rosette289,256,nail266,picture275 and tart218 are verified. Potion321 remains unacquired because the original ten-second invitation expired before the scripted self-use. A corrected ordinary-input replay is running; watcher is reconstructing the verified supplies/tart segment. Hero count remains5of6.

Latest verified milestone: truth potion321 acquired by2433000 in witch.scn with control restored and all1506 inputs accepted (headless-act3-truth-potion-invitation.json; final frame inspected). Native speed changes replace the audio sink without waiting for a suspended device callback; dedicated regression/native Clippy/release build pass. The visible potion handoff is being prepared. Completed-picture replay remains under test; hero count remains5of6.
