# V1 script escape compatibility

Reference revision: 96b42d42bc8533953bdd701db059cc33c839a9ff.
Current engine support includes escape generations, waits, walking, films,
samples, speech/text, inventory opening/arbitration, scrolling and palette/color
guards. Fresh native/browser/tool startup activates detected DOS pre-title
policy. DWRSAVE v17 preserves that state, and user loads end intro mode while
exact runtime restoration retains it.

Native keyboard and browser keyboard/touch Escape now route through a shared
player event dispatcher. Complete user-facing skipping across every game
sequence remains unverified. The sections below
record implementation stages chronologically; earlier pending items may have
been resolved by later sections.

## Observed reference contract

- `events.cpp::PlayerEvent` increments the escape counter (initialized to 1)
  and left-event counter for PLR_ESCAPE before testing whether player control
  is enabled. Disabling control must not suppress cutscene escape delivery.
- `pcode.cpp::Interpret`, opcodes 40/41: ESCON captures the current escape
  counter and enables escaping; ESCOFF clears both. Repeated ESCON must capture
  again even when the Boolean was already true. The pinned implementation
  excludes the DW1 intro and implements intro skipping separately.
- Library ESCAPE is not callable: tinlib.cpp deliberately errors on it. Adding
  behavior to library call 30 would not implement the bytecode mechanism.
- WAITTIME skips at entry or ends its suspended wait when an enabled context's
  captured counter differs. This is independent of ordinary user-event waits.
- V1 WALK checks before starting and after its sleep while moving. Escape
  stands at the requested destination and releases the actor token. Do not
  import the V2 branch's different ordering or hold semantics.
- WALKTAG uses StandTag on escape, including the tag's arrival pose. SWALK
  delegates to Walk after standing, and restores control only if it took it.
- PLAYSAMPLE tests speech priority before its escape guard. On stale escape
  at entry it stops current samples; while blocked it stops its own playback
  handle. Missing samples still yield once in the ordinary non-escaped path.

## Required implementation and verification

Capture generation at execution of each ESCON, not lazily at a later library
call. Preserve the generation alongside per-script state (including suspended
and saved-scene contexts), global event state and active operation ownership.
Older saves contain only the Boolean; define an explicit migration that does
not fabricate an already-pending escape event. Unchanged legacy imports should
retain their exact export behavior until state actually advances.

Route host Escape through shared event handling while retaining original menu
and inventory priorities. Provide touch access for landscape iPad. Add focused
bytecode tests for repeated ESCON, ESCOFF, control-disabled delivery, nested
contexts, entry versus suspended operations, token/control release and exact
save/load continuation. Validate intro skipping separately against the pinned
intro exception. Audit film, speech, inventory and other escape-sensitive
library branches before claiming complete cutscene skipping. A fast-forward
button or generic process cancellation is not equivalent behavior.


## Interpreter transition boundary

Machine::step now returns `Step::Escape(bool)` for each committed ESCON or
ESCOFF instruction. The event occurs on repeated ESCON as well as Boolean
changes, so the engine can capture a fresh global generation at the correct
instruction rather than guessing from state changes after a library call.
The existing serialized Boolean is unchanged; this step result is transient
and is not added to the saved Machine layout.

A regression executes ESCON twice, suspends at a library boundary, resumes
through ESCOFF and halts. It verifies each event occurs once, repeated polls
of the pending library do not replay an event, and an intermediate serialized
Machine continues identically. Engine, declaration and probe callers currently
consume the new transition as an ordinary instruction; event-generation capture,
versioned persistence, cancellation handlers and host routing remain to be
integrated. This change alone does not enable cutscene skipping.

Interpreter-boundary validation: workspace tests, Clippy with warnings denied
and the browser WASM check pass (`assets/logs/escape-opcode-*`). No cancellation
behavior or new save-state generation fields are claimed by these checks.


## Escape snapshot extension (DWRSAVE v14)

The engine now has an optional escape-event generation and each Machine has
an optional captured generation. These fields are excluded from the legacy
postcard layout. Version 14 wraps an ordinary v1-v13 envelope with the global
generation and captured values in deterministic context order: master, scheduler
order, saved-scene stack/context order, then pending scene restoration. Decoding
requires exactly one value per context, a nonzero global generation, and only
nonzero captured generations no greater than the global value. A disabled
context cannot have a capture. Enabled contexts with no capture remain explicitly
unmigrated; legacy saves did not record the missing value.

The inner version is checked before decoding, so v14 cannot nest itself.
Existing mover/menu/speech extensions remain inside the older envelope. An
engine without escape state continues to emit its previous save format; old
imports are not rewritten merely by decoding them. Repeated ESCON/ESCOFF clears
the old capture before emitting the transition, ready for the caller to record
the current generation.

The regression covers master/live/saved/restoring contexts, exact byte round
trips, next-tick continuation, unchanged inner legacy bytes, invalid counts,
zero/future generations, captures on disabled contexts, nested-v14 rejection
and atomic failure of explicit restoration. These are storage prerequisites:
the engine does not yet initialize/capture the global counter or route Escape
input, and no cancellation behavior is enabled by this extension alone.

Storage validation: workspace tests, Clippy with warnings denied, WASM checking
and all 18 original-asset integration tests pass (`assets/logs/escape-save-*`).
The original-asset cases exercise legacy/current imports; synthetic tests above
exercise populated v14 escape state.


## Counter capture and timed waits

ESCON now captures the global generation at that instruction in master and
scene interpreters; ESCOFF clears its capture. The first generation is 1.
Legacy enabled contexts without a capture adopt the current generation when
resumed. When the first new escape is delivered before such a context resumes,
all live/master/saved/restoring enabled contexts are armed against the previous
generation before the counter advances, so that event is not lost.

`Engine::script_escape_event` atomically increments escape, user and left-event
counters and updates idle activity without requiring player control. Overflow
rejects before any mutation. Hosts do not yet call this API: menu priority and
the separate pre-title intro mechanism still require integration.

WAITTIME now skips an entry call with a stale enabled capture or completes a
suspended wait on escape. The context stays stale until ESCON captures again;
ESCOFF disables cancellation. Other one-tick library yields are not treated as
WAITTIME. Tests cover repeated ESCON, entry/suspended waits, ESCOFF, disabled
control, master waits, legacy migration, counter overflow and exact save/load
continuation at each tick. This does not claim the other library cancellation
handlers, intro exception or host routing are implemented.

Counter validation: workspace tests, Clippy, WASM checking and all 18
original-asset tests pass (`assets/logs/escape-counter-*`). The additional
master/legacy migration case passes in the focused escape regression run.


## Scripted walking cancellation

WALK now uses the original requested coordinates on stale escape at entry or
while suspended, then completes its call. It does not resolve them as a new
mouse click. WALKED returns true on its escape path; an event during its initial
24-tick V1 sleep is checked after that sleep and movement startup, preserving
the reference's ordering. A newer walk serial still supersedes an old waiter.

WALKTAG uses the polygon node and V1 StandTag pose on escape. Only the lead actor
uses the polygon film in this escape path; ordinary completion retains its
separate arrival-pose behavior. SWALK that is already escaped at entry returns
without standing or taking control in V1. A suspended SWALK uses Walk's escape
standing behavior and restores control if that invocation took it. Existing
STAND and cancellation share the same checked mover-placement implementation.

The walking bytecode regression now includes escape before entry, escape during
movement and escape during WALKED's initial delay, in addition to normal/stop
cases. It covers custom/zero films, ordinary/node paths, outside-path SWALK
entrances, control initially enabled/disabled, WALK hold modes, return values,
arrival poses and exact save/load continuation each tick. Host Escape remains
disconnected while film/speech and pre-title intro cancellation are unfinished.

Walking cancellation validation: workspace tests, Clippy, WASM checking and
all 18 original-asset tests pass (`assets/logs/escape-walk-*`). The original
assets run without host escape input; explicit cancellation is covered by the
synthetic script cases above. The running fb49b7e route predates this change.

## V1 film cancellation

PLAY/TOPPLAY skip already-escaped calls before reading film resources. Live
primary and secondary reels capture the calling context's escape generation
alongside their playback identity. Escape releases only the matching owner;
a replacement reel survives. V1 removes escaped reels without the V2
last-frame fast-forward. Duplicate requests update escape flags before
arbitration without restarting the reel, matching the reference ordering.

DWRSAVE v15 preserves live reel captures. Tests cover both calls, blocking
and asynchronous playback, entry and suspended escape, duplicate requests
that disable escape, replacement ownership and exact restored continuation.
Malformed capture lists, unknown owners, future generations and nested v15
envelopes are rejected. Saved-scene reconstruction starts restored reels
without escape authority, matching RestoreReel; full snapshots retain it.

Validation: workspace tests (305 core tests), all 18 original-asset web tests,
Clippy and WASM checking; logs use `assets/logs/escape-film-*`. These focused
checks do not establish complete host cutscene skipping. The recorded route
on the earlier escape-counter revision is documented separately in
[its validation report](validation/escape-counter-fb49b7e.json).

## Already-escaped sample calls

PLAYSAMPLE now stops current effects and completes immediately when its
escape capture is stale. The speech guard precedes this branch, so active
speech is preserved. TRYPLAYSAMPLE's busy-channel guard precedes both and
retains its one-tick yield without stopping or retrying the existing sample.
Escape is checked before the effects-volume and resource lookup branches: an
escaped call does not yield merely because effects are muted or read a bank.

The regression covers both calls, blocking/nonblocking arguments, idle/effect/
speech channels and enabled/muted effects. It deliberately supplies no bank
and verifies exact channel preservation or stopping, yield behavior and
continuation without retry. No snapshot format changes are needed for entry.

Suspended sample calls now capture SamplePlayer's monotonically increasing
serial. A replaced or ended playback releases its waiter immediately; an
escaped waiter stops audio only while that same serial owns the active channel.
The serial is cleared on completion. This corrects ordinary replacement timing
as well as escape behavior: the reference waits on its original SoundHandle,
not whatever sample is currently playing.

DWRSAVE v16 preserves these serials in scheduler order. Older snapshots cannot
recover their unrecorded handle. Their waits retain the previous channel-busy
behavior; Escape completes such a wait without stopping unproven audio. New
PLAYSAMPLE/TRYPLAYSAMPLE calls always capture ownership. This legacy limitation is deliberate and
does not claim to reconstruct information absent from historical saves.
Speech/text cancellation still requires a separate reference audit.

Validation: workspace tests (306 core tests), all 18 original-asset web tests,
Clippy, WASM checking and native/browser release rebuilds passed. Logs use
`assets/logs/escape-sample-entry-*`. The visible session was not restarted.

The ownership regression covers both sample calls, ordinary and escaped waits,
original/replaced effect/replaced speech/ended channels, and historical waits.
It checks exact snapshot re-encoding and subsequent state equality, plus v16
rejection of empty/all-None captures, zero/future serials and nested wrappers.

Ownership validation: workspace tests (307 core tests), all 18 original-asset
web tests, Clippy and WASM checking passed; logs use `assets/logs/sample-owner-*`.

## PRINT and positioned speech escape

PRINT (53), TALKAT (98) and sustained TALKATS (104) now skip stale escape
contexts before reading strings, samples, actor colors or font resources.
TALKATS still validates its required sustain argument before this guard,
matching the reference assertion. Skipped calls leave existing audio intact.

During a wait, escape dismisses these calls independently of the left-click
sentinel, duration or sustain setting. Existing sample serial ownership limits
audio stopping to the line's own playback; replacement speech survives. This
uses the existing saved print wait and escape generation, without a new save
format. Inventory PRINTOBJ and animated TALK are separate paths and remain
outside this change.

The regression covers entry and suspended escape for all three calls, timed
sustained PRINT, speech and text-only waits, replacement speech, disabled
escape contexts and exact snapshot continuation. Empty precomputed glyph lists
isolate text wait scheduling from font rasterization in the text-only case.

Validation: workspace tests (308 core tests), all 18 original-asset web tests,
Clippy, WASM checking and native/browser release rebuilds passed. Logs use
`assets/logs/escape-print-*`. The recorded sample-ownership route remains a
separate run on revision 99fe70e, without escape inputs.

## Animated TALK escape

TALK (77) now skips an already-escaped context before accessing speech/film
resources or taking control/tags. Suspended TALK passes its captured-generation
comparison into the existing completion path, restoring the normal film or
mover reel, stopping only its own sample and releasing owned control/tags.
The initial subtitle setup retains its existing yield; cancellation uses the
reference wait boundary. No save layout changes are needed.

Tests isolate the escape-generation path by aligning the wait's left-event
value after injecting Escape. They cover non-mover film restoration, the
post-speech subtitle phase without duplicate reel cleanup, replacement speech,
exact saved continuation, and moving TALK on ordinary/node paths followed by
completion of the original route. Entry tests use invalid film resources and
no strings/bank while preserving existing audio and enabled/disabled control.

Validation: workspace tests (309 core tests), all 18 original-asset web tests,
Clippy, WASM checking and native/browser release rebuilds passed. Logs use
`assets/logs/escape-talk-*`. Host Escape remains disconnected pending the
remaining library audit and intro routing.

## Inventory/conversation entry and sample polling

INVENTORY skips stale contexts before validating the slot or accessing window
assets, matching the reference guard before its assertion. CONVERSATION keeps
its polygon-context requirement, skips only open modes (default/bottom/top),
and still executes END after escape. Existing windows and tags remain intact
when opening is skipped. Tests cover no window, a conversation, and a personal
inventory, including END and an invalid INVENTORY slot in the skipped branch.

SAMPLEPLAYING returns false for a stale enabled context without stopping or
consuming audio. This lets the original sample-polling loops terminate after
escape. Its regression covers idle, live, partially consumed and ended samples,
disabled/enabled escape, return-slot behavior and preserved audio state.

Remaining reference audit: ScrollScreen checks escape before replacement
identity on wake and jumps to Offset, unlike ordinary superseded-scroll
cancellation. SetPalette skips before resources. PRINTOBJ has a V2-only early
guard but a shared substring-loop guard after POINT/non-POINT arbitration;
do not add a blanket V1 entry skip without preserving that ordering.

Validation: workspace tests (310 core tests), all 18 original-asset web tests,
Clippy, WASM checking and native/browser release rebuilds passed. Logs use
`assets/logs/escape-dialog-*`. No host input integration is claimed.

## Scroll and palette escape

SCROLL increments its request identity even when already escaped, then applies
OFFSET immediately without yielding. A suspended blocking SCROLL checks escape
before supersession, stops current motion and jumps to its original raw target.
Ordinary replaced scrolls still terminate their caller. Nonblocking V1 scrolls
do not acquire V2's asynchronous escape monitor. Cursor position is unchanged;
OFFSET's raw coordinates are not clamped to the scroll range.

SETPALETTE skips resource access in a stale context. Regressions cover blocking
and nonblocking scroll entry, suspended escape with and without replacement,
raw targets outside scroll bounds, request identity and exact save continuation,
plus skipped invalid palette resources.

The native recorded route at sample-ownership revision 99fe70e completed with
all 1,652 inputs accepted and 5,048 engine plus 5,048 DSP snapshot checks. See
[validation report](validation/sample-owner-99fe70e.json). It contains no Escape
inputs and predates these scroll changes.

Validation: workspace tests (312 core tests), all 18 original-asset web tests,
Clippy, WASM checking and native/browser release rebuilds passed. Logs use
`assets/logs/escape-scroll-*`. Final route image was inspected at title.scn.

## Inventory text arbitration

PRINTOBJ retains its V1 ordering. ON/OFF commands execute independently of
escape. After finding the inventory cell, a POINTED call clears suppression
before checking its captured generation. A non-POINTED description suppresses
hover text and yields once, then checks escape before accessing text, speech
or fonts. Skipping releases suppression and leaves current audio/control
unchanged. The saved object_print_position already preserves this boundary.

Once displayed, non-POINTED V1 text uses the existing left-event cancellation
path (Escape increments that event); it does not import a V2-specific early
guard or add generic cancellation to hover labels. The regression covers
already-escaped entry, escape during the arbitration delay, POINTED versus
description behavior, unchanged existing speech/control and exact saved
continuation. It supplies no strings, bank or font resources during skipping.

Remaining audit items include WAITFRAME's escape check inside its step loop,
WAITKEY's entry guard (suspended Escape is already an ordinary user event),
and TALKATTR's color guard. Intro detection and host routing remain separate.

Validation: workspace tests (313 core tests), all 18 original-asset web tests,
Clippy, WASM checking and native/browser release rebuilds passed. Logs use
`assets/logs/escape-invtext-*`.

## Remaining waits and speech color

WAITFRAME (100) now breaks its frame-step polling loop for a stale enabled
escape context. It retains argument validation and does not change actor step
counters. WAITKEY (85) skips at entry for a stale context; a suspended wait
already observes Escape through the ordinary input event, including ESCOFF.
The regression covers entry/suspended waits, enabled/disabled capture and
exact save/load continuation.

TALKATTR (78), previously unsupported, now updates the shared text color unless
the context has escaped. This targets the supplied English V1 edition. It
matches the reference upper clamp at 255 and TINSEL_RGB conversions, including
the historical 16-bit green cast and its overlap into blue for negative input.
Tests cover ordinary values, upper clamp, signed conversion, skipped updates,
stack consumption and saved color. Existing actor-specific color declarations
and font rasterization are separate paths.

Validation: workspace tests (315 core tests), all 18 original-asset web tests,
Clippy, WASM checking and native/browser release rebuilds passed. Logs use
`assets/logs/escape-waits-*`. Intro state and host routing remain incomplete.

## Intro policy and resource detection (integration pending)

`intro.rs` now isolates the DOS reference policy. It detects the first
case-insensitive `title.` resource prefix, disables detection for handle zero
or a missing name, and rejects handle overflow. It does not use PSX's different
`turtle.` rule. `inspect-resources` reports `dos_intro_title_handle`.

The policy captures one initial escape generation at the first scene script
and supplies the current generation to subsequent non-title scene scripts.
A changed initial generation redirects the next scene request to the detected
title, entrance 1, transition 0, after hook interception. Reaching the title
ends intro mode; game restart clears its baseline. This is a scene-boundary
redirect, not an immediate jump from the input handler.

Policy tests cover detection/fallback, title code offsets, generations across
multiple scenes, serialized continuation, restart/end and invalid checkpoints.
The module is not yet installed in Engine or its snapshot envelope. Next steps:
pass detected metadata at bootstrap, preserve policy across restart and runtime
snapshots, arm scene interpreters, suppress both ESCON and ESCOFF mutations
during intro, and apply redirect after hook processing. User game-load must
end intro mode, while deterministic snapshot audit restoration must preserve
it; these are distinct operations. No host intro-skipping behavior is claimed.

Validation: workspace tests (318 core tests), Clippy and WASM checking passed.
The original ISO resource inspection identifies `title.scn` at index
11, handle `0x05800000`, among 149 resources. Logs use
`assets/logs/intro-policy-*`. This is metadata/policy validation, not a
playthrough or host Escape test.

## Opt-in engine intro integration

`Engine::bootstrap_with_title` installs the detected policy before master
execution. The original `bootstrap` path and a missing detected title retain
legacy behavior. Title metadata is checked for a scene script and entrance 1;
resource-validated snapshot restoration checks it again. Intro scene/entrance
interpreters receive the current generation, while master and scene dispatch
restore the previous escape flags when ignoring ESCON/ESCOFF. Scene requests
apply intro redirection after hook interception. Reaching the title ends intro
mode; restart preserves detection but resets its baseline and event counter.

DWRSAVE v17 wraps the earlier envelope with intro policy, preserving exact
runtime continuation. `end_intro` is a separate user-load operation. It must
not be called by deterministic snapshot audits. The hosts still use legacy
bootstrap and do not yet route keyboard/touch Escape to the shared event API.

Synthetic integration tests cover armed intro scripts through ESCOFF/ESCON,
hook override, exact snapshots at every tick, title arrival, restart, explicit
end, nested-wrapper rejection and missing-title fallback. An original-asset
web-core test enables the opt-in bootstrap, injects Escape at tick 1 (before the title), and
checks full session/audio/frame continuation in 24-tick blocks until title.

Original-asset timing check: this edition is already at title.scn by tick 500.
The special reference intro policy concerns the pre-title sequence, not the
post-title game introduction. The integration test therefore injects at tick 1
and asserts that it has not reached the title yet. An initial tick-500 test
correctly exposed the distinction by starting the game instead of skipping
the pre-title sequence.

Validation: workspace tests (319 core tests), all 19 original-asset web tests,
Clippy and WASM checking passed. Native/browser release builds include the
opt-in API. Logs use `assets/logs/intro-engine-*`. Host activation remains
pending; no visible-session update is claimed.

## User loads versus exact restoration

BrowserGame now exposes `load_game` for user-facing loads while `load` remains
exact session restoration. The worker uses `load_game` for imported files and
original menu slots. Both decode/validate the complete replacement before
committing engine and DSP; only the user-load path calls `end_intro`. Native
quick-load and menu-load actions also end intro mode, as does a startup load
without a replay trace. Startup loads with recorded replay retain exact state.

The original-asset regression imports an active pre-title v17 snapshot through
both APIs. It verifies that only user loading ends intro mode, frame/tick/DSP
remain unchanged, invalid input is atomic, and an exact restore can recover
the original active policy. Historical saves without intro metadata retain
their existing representation. Default host startup activation and Escape
routing are still pending.

User-load validation: workspace tests, all 20 original-asset web tests,
Clippy, WASM checking and native/browser release builds passed. Logs use
`assets/logs/user-load-*`. The original save-import/export player cases
exercise historical saves without intro metadata; the new Rust original-asset
regression separately covers active v17 policy.

All 20 Chrome/WebKit player cases passed (10 per browser), covering import/
export, cold startup, audio pause/resume and active inventory interactions.
See `assets/logs/user-load-browser.log`. This is not physical iPad acceptance.

## Host startup activation

Fresh native/browser players and native headless/window/startup tools now use
resource-index title detection and `bootstrap_with_title`. Missing title
detection retains the legacy fallback. The original-asset skip test now uses
the real browser constructor, rather than replacing its engine with an opt-in
instance. New sessions preserve the policy in v17 saves; the menu roundtrip
test now expects that envelope while still validating identical menu rendering
and continuation. Imported historical saves retain their existing state and
encoding; they do not acquire fabricated intro history.

Keyboard/touch Escape routing remains pending. This startup change enables
reference intro opcode behavior and policy state, not a new user input path.

Startup validation: workspace tests, all 20 original-asset web tests, Clippy
and WASM checking passed. Both hosts rebuilt; Chrome/WebKit cold-start cases
passed, and a 30-frame native dummy-video smoke run exited normally at tick 15
in psygnosi.scn. Logs use `assets/logs/intro-startup-*`. Dummy-video smoke is
not visible-window or physical-device acceptance.

## Host Escape dispatch

`Engine::escape_input` atomically increments escape, user and left-event
counters before the control gate, matching PlayerEvent. With control enabled,
it delegates active configuration menus to their existing escape navigation,
closes visible personal inventories, and schedules a visible conversation's
close/postamble. Hidden inventories and control-disabled windows remain intact.

Native manual Escape and BrowserGame's `escape` command use this method. The
browser adds an Escape touch button; repeated keyboard keydown events are
ignored. The separate Continue/Close controls retain their existing meanings.
A core regression checks one increment per event, control-disabled delivery,
hidden-inventory preservation and overflow atomicity. The original intro test
now injects via BrowserGame.input rather than the internal script API.

Host validation: workspace tests (320 core tests), all 20 original-asset web
tests, Clippy, WASM checking and release rebuilds passed. Added conversation
postamble/atomic-failure coverage passed, and the original-menu gate/warp/
atomicity regression passed through escape_input. Chrome and WebKit each
passed keyboard and touch pre-title skipping (four browser cases). Logs use
`assets/logs/escape-host-*`. This does not establish physical iPad acceptance
or every post-title cutscene/order. The full-route job remains pinned to
1ac8f90 with no Escape inputs, predating this dispatcher.

Both Chrome/WebKit native-menu-v10 import/export cases also passed, preserving
exact pixels and unchanged historical save bytes. The visible manual session
was not restarted.
