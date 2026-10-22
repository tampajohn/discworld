# V1 mover reel compatibility gap

Status: reference-derived transition model implemented in `mover_reel.rs`;
transactional mover animation application and snapshot storage are implemented.
Walking and new stationary speech use explicit reel state; old active talk
waits migrate at the first tick. Custom WALK/WALKED films are implemented; SWALK/WALKTAG overrides remain
guarded. Ambiguous legacy overlapping slots require further compatibility work. Reference revision:
`96b42d42bc8533953bdd701db059cc33c839a9ff`. This applies to V1; later-version
branches in the same functions must not be imported implicitly.

## Reference behavior

`engines/tinsel/movers.cpp`, `AlterMover` (around lines 546–604), owns three
separate concepts: last selected film, whether a special stationary/talking
reel is active (`bSpecReel`), and whether an explicit walking reel is active
(`bWalkReel`). `hPushedFilm` is a single saved film, not an unbounded stack.

- PUSH saves last film only if a special reel is currently active; otherwise
  it saves zero. Thus pushing speech during normal walking saves zero, not
  the actor's standing film or current walk film.
- A nonzero normal/PUSH/POP film starts its first reel at the film rate, resets
  the V1 walk phase, marks special mode, clears custom walking mode, and takes
  the first animation step immediately.
- A nonzero WALKREEL film resets the script/phase but does not take that first
  step. It marks custom walking mode and clears special mode.
- Zero restores normal walking selection only when special mode was active.
  POP first substitutes the saved film and then follows the same rules.

`MoverProcess` (around lines 788 and 863) advances only animation while special
mode is active. It does not call `DoMoveActor`. This suspends displacement and
route progression without completing the walk or changing its walk number.
Hidden special actors do not advance their animation in that branch.

`SetMoverWalkReel` (around line 640) restores normal actor ownership/visibility
before checking custom walking mode. In custom walking mode it then returns
without changing direction, scale or the selected script. Normal scale changes
and phase alignment still apply when custom mode is absent.

`engines/tinsel/tinlib.cpp`, `TalkOrSay` (around line 3366), pushes the talk film
and yields one tick. `FinishTalkingReel` (around line 3240) first calls
`SetMoverStanding`, then POP. Neither operation explicitly ends the walk.
For speech interrupting normal walking, POP therefore forces the normal walk
reel, after the standing selection has reset phase, and movement can resume
on the next mover update. Speech ending before its text timeout must resume
movement at reel completion, not at final text dismissal.

`engines/tinsel/move.cpp`, `SetActorDest` (around line 1356), installs the
supplied WALKREEL, or clears special mode for a zero film, before routing.
A new walk request can therefore replace a talk animation while its speech
coroutine is still alive. Deriving the animation mode only from the existence
of a pending talk coroutine would be wrong.

## Gaps identified before integration

`engine.rs` call 77 explicitly rejects a mover with `walk_target`, and
`Mover::push_talk` independently rejects it. `pop_talk` calls `finish_walk`,
which clears route state. The mover tick loop chooses displacement solely
from `walk_target.is_some()`. Removing either guard cannot reproduce the
reference behavior.

`Mover` records `last_film`, animation, step phase and route state, but has no
independent special/custom-walk mode. Existing standing-film bookkeeping is
not equivalent to the reference's single pushed-film slot. WALK/WALKED film
arguments also remain guarded. These gaps should share one reel-state model;
separate ad hoc talk and custom-walk exceptions would diverge on replacement.

## Implementation and acceptance gates

1. Introduce explicit mover reel mode and single pushed-film state. Implement
   PUSH, POP, normal selection and custom walking transitions transactionally;
   invalid films must not partially alter animation, phase or routing.
2. Make the engine mover phase choose special animation versus displacement
   from that state. Preserve route, walk number, pending stop request and
   coordinate ownership during speech. Handle new walk requests and scene
   replacement independently of the suspended speech coroutine.
3. Preserve the added state in a versioned snapshot extension. Do not append
   serialized fields to the legacy mover payload: versions 1–12 must remain
   readable and existing fixtures must retain their bytes. Validate actor IDs,
   reel handles and mode combinations before replacing a running engine.
   Legacy snapshots require a documented reconstruction rule for ordinary
   walking versus standing/talking; do not infer solely from `last_film`.
4. Exercise normal walk → TALK → resume with both direct and routed/node walks,
   stationary talk, explicit talk film/direction constants, new walk during
   speech, stop during speech, hidden actor and scene transition. Check the
   single-slot behavior for overlapping PUSH/POP against the reference.
5. Save/reload before talk initialization, during speech, after early reel
   completion with text remaining, and after resumption. Compare every
   subsequent tick's position, route, phase, animation and engine snapshot;
   verify atomic rejection of corrupt extension data.
6. Only after these pass, remove the call-77/mover guards. Extend WALK/WALKED
   and SWALK film support using the same model, verify original assets in both
   hosts, and rerun the recorded route to catch stationary-reel regressions.

The existing start-to-credits route does not establish these alternate-order
semantics. This is a remaining compatibility requirement, not a reason to
relax the complete-runtime goal or silently accept unsupported calls.


## Transition model evidence

The pure `mover_reel::State` model returns an animation operation and candidate
state without mutating its caller. Resource installation must succeed before
committing either. It distinguishes normal movement, special animation, and
custom walking, and retains the reference's single pushed-film slot.

Six regression sequences cover normal walking speech/standing-before-pop,
stationary restoration and repeated POP, overlapping PUSH operations, a new
walk while speech is pending, custom walking followed by zero-film requests
and speech, and zero PUSH with state serialization/continuation. These test
reel decisions only; displacement, hidden actor scheduling, resource validation
and full engine snapshots remain acceptance gates. No unsupported runtime
call has been enabled by this foundation alone.


## Animation application and snapshot storage

`Mover::alter_reel` applies the transition to a clone, validates the selected
film/script, and commits animation and explicit reel state together. It retains
the existing object, sprite capacity, position, route and walk serial. The
shared reel installation routine is also used by legacy standing changes.
Tests verify talk's immediate first step, custom walking's deferred first step,
walking restoration, and rejection of a bad film without losing the pushed
slot or altering the mover.

DWRSAVE v13 stores a bounded v1–v12 envelope plus a strictly ordered, nonempty
list of `(actor, reel state)` entries. The old mover payload is unchanged;
without explicit reel state the previous format selection is unchanged. Inner
v13 envelopes are rejected before decoding, so nesting cannot recurse. Unknown,
duplicate or unordered actor entries and trailing data are rejected. Restore
validates retained/pushed film references against the supplied resources before
activation. The extension preserves the earlier menu, input, speech-channel
and finished-talk extensions through its legacy envelope.

A synthetic engine regression checks exact encode/decode/re-encode, full
resource-validated restoration, subsequent PUSH/standing/POP/custom-walk state
continuation and corruption rejection with valid outer checksums. It does not
claim that live movement has been connected to the mode yet: ordinary gameplay
still leaves the optional mode absent, and walking TALK remains guarded.
The next change must migrate the mover tick, walking/standing replacements and
speech completion together, then exercise interrupted routes and full saves.

Validation: 296 core tests, workspace tests, Clippy, the web WASM build check,
and all 15 opt-in original-asset web tests pass. Logs are under
`assets/logs/mover-reel-integration-*.log`. The legacy fixture tests include
native save import/export and exact original frame restoration.


## Live walking speech

Call 77 now accepts a moving speaker. The first walking PUSH migrates that mover
to explicit reel state; already-migrated movers continue using it. Stationary
legacy movers retain their old bookkeeping, including restored old stationary
speech waits. New explicit states use DWRSAVE v13; imported legacy saves are not
rewritten merely by loading them.

The engine mover phase advances special animation without changing route or
position, skips hidden special actors, and leaves explicit normal movers with
no destination idle. A zero-film new walk clears special mode before selecting
its route. Speech completion selects standing then POP without clearing the
existing destination, leg or walk number. Stop requests remain queued during
special animation. Existing walking-film completion clears custom mode before
normal reel selection, matching MoveActor; script custom-film guards remain.

The core regression dispatches real TALK bytecode during ordinary and node-path
walks. Both paths cover natural completion, click cancellation, queued STOP,
replacement walk, hidden actor, and early reel completion while slow subtitle
text remains. Each tick encodes/decodes/re-encodes and compares subsequent engine
state; synthetic route geometry is tested independently of asset validation.
Its closed preferences controller is rebuilt separately because the synthetic
polygon does not exist in the fixture archive.

`original_walking_talk_reel_preserves_route_and_full_session` separately uses
the original unseen.scn checkpoint, actual walking/talk films, engine mover loop,
rendering and production engine/DSP session restoration. It verifies the held
position during speech, exact continuation after reload, retained walk serial,
and arrival at the original destination after POP. It invokes the mover reel
API directly; TALK dispatch is covered by the synthetic regression above.

The synthetic route test also replaces the scene during active walking speech,
verifies removal of the old mover/route, voice and TALK coroutine, then performs
resource-validated save/load in the replacement scene and compares continuation.
The cancelled script must not write its post-TALK global. Additional coverage
remains for full stationary-state migration, overlapping live speech coroutines,
and script custom walking films. The earlier transition-model tests cover single-slot
PUSH/POP decisions but do not establish all those engine interactions.


The complete 2,630,000-tick route passes for runtime fa77914 in native, Chrome
and WebKit: all 1,652 inputs accepted, ending/credits/restart reached, final
engine bytes and RGBA equal across hosts. Native passed 5,048 engine snapshot
and 5,048 DSP audits. Both browsers generated 4,832,625,000 finite samples with
zero nonfinite values; combined DSP saves still differ. See the tracked
[validation summary](validation/walking-talk-fa77914.json). Local artifacts are
under `assets/validation/walking-talk-fa77914`. Browser worker audio is not sent
through Web Audio during this run; it does not establish physical playback or
complete touch-only interaction.


## Custom walking film readiness

The mover regression now checks that a forced direction/scale selection cannot
replace an explicit walking film. It advances a finite custom film through its
12 Hz timing, checks that the route and walk serial survive every displacement,
and verifies that film completion clears custom mode, resets phase and selects
the ordinary walking reel without an extra frame. This matches SetMoverWalkReel
and MoveActor in the pinned movers.cpp/move.cpp. The test passes; evidence is in
`assets/logs/custom-walk-reel-completion.log`.

Script WALK (87) and WALKED (88) still reject nonzero film arguments. Their next
integration must account for legacy stationary speech: a new custom walk must
retain the reference's existing pushed-film slot, even when the current old
snapshot stored that slot in a TALK wait rather than explicit mover state.
WALKED's one-second delay precedes SetActorDest; the film argument remains on
its suspended VM stack and need not be duplicated in a new wait payload.
Retain the existing exact-arrival result, token interruption and hold==2 rules.
Removing these guards before legacy reel migration would enable a path with
incorrect later POP behavior.


## Stationary speech and legacy continuation

New stationary TALK calls now save the actual last film into the same shared
slot used for walking speech. A newer TALK overwrites that single slot; ending
an older coroutine does not restore its private historical film. The live core
regression uses distinct standing and talking films, overlaps two TALK calls,
and checks the final restored film and snapshot continuation at every tick.

For an old active TALK wait with no explicit mover state, the engine recovers
the saved film at the beginning of the next mover phase without restarting the
animation. Loading and re-exporting the old snapshot remains byte-identical.
Old runtimes rejected TALK starting during movement: if an old pending TALK
coexists with a live route, a newer walk replaced the talk animation. Migration
therefore keeps normal movement active while recovering the pushed slot.
The regression saves both before and after that replacement, checks the legacy
v12 roundtrip, and compares continuation with the new explicit state on ordinary
and node paths.

If multiple legacy waits for one mover contain conflicting saved films, the
old representation cannot unambiguously identify a shared pushed slot. Migration
returns an explicit error before committing any recovered mover state instead
of guessing. A regression verifies this rejection is atomic. Newly generated
overlapping speech saves retain the shared slot and do not have this ambiguity.
This remains a limitation for ambiguous old saves, not a claim of universal
migration of every historical state.

The raw-mover unit fixture now carries the companion reel state when testing
serialization; production persistence continues to use the validated DWRSAVE
v13 extension. No legacy mover payload fields were added or reordered.


Stationary migration validation: workspace tests (297 core tests), Clippy and
web WASM checking pass, as do all 16 opt-in original-asset web tests. Native and
normal browser hosts are rebuilt. The browser UI suite passes 32 cases; its two
optional native-v10-menu import cases were then run with the saved fixture and
both passed, for 34 browser cases exercised across Chrome/WebKit. Those cases
verify exact imported pixels and unchanged save export. Logs use the
`assets/logs/stationary-reel-*` prefix.

The full native route with engine/DSP snapshot audits passes for runtime
3856346: 2,630,000 ticks, all 1,652 inputs accepted, ending/credits/restart reached,
and 5,048 engine plus 5,048 DSP audits. The final engine state matches fa77914.
See [native validation](validation/stationary-reel-3856346.json). The completed cross-host route evidence above remains
pinned to fa77914.


## Scripted custom WALK and WALKED

WALK (87) and WALKED (88) now accept explicit walking films. The mover installs
reel zero before route selection, retaining its persistent object and deferring
the first animation step. Destination/resource failures leave the mover intact.
Zero-film calls retain the existing normal walking path. WALKED recovers its
film from the suspended VM stack after the original 24-tick delay; the wait
payload and older snapshot encodings need no new field.

The new regression exposed an independent V1 return-value quirk: SetActorDest
increments the walk number but returns zero when it calls GotThere before
starting a walk (distance less than four horizontal and two vertical pixels).
WALKED compares that returned value, so it returns false even at an exact
already-reached destination. The engine now preserves that zero result before
node projection instead of reporting success merely from coordinate equality.
A zero-film and a custom-film call share this correction.

The synthetic bytecode regression covers ordinary/node paths, WALK holds 0/1/2,
WALKED's delayed start, interrupted versus successful arrival, finite custom
film completion and no-motion return, with snapshot/re-encode and next-tick
continuation at every step. The original-asset regression starts an actual
custom-film walk in unseen.scn, rejects a bad film atomically, preserves the
selected film/direction while moving, restores standing at arrival, and compares
full engine/DSP saves, frames and audio through the production session codec.
Workspace tests (298 core), Clippy, WASM checking and all 17 original-asset web
tests pass; logs use `assets/logs/custom-walk-*`. The completed 3856346 replay
predates these changes, so it is not full-route evidence for this later build.
SWALK custom films and WALKTAG film overrides remain separate compatibility work.
