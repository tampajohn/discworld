# V1 mover reel compatibility gap

Status: reference-derived transition model implemented in `mover_reel.rs`;
transactional mover animation application and snapshot storage are implemented.
Walking and new stationary speech use explicit reel state; old active talk
waits migrate at the first tick. Custom WALK/WALKED/WALKTAG/SWALK films are implemented. Ambiguous legacy overlapping slots require further compatibility work. Reference revision:
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


## Custom WALKTAG walking films

Pinned `tinlib.cpp::WalkTag` passes its explicit film to SetActorDest, then
applies the polygon's separate final pose after walking. Call 91 now follows
that distinction through `begin_walk_film`; it no longer rejects a nonzero
walking film. The existing tag-wait payload continues to store the arrival
pose, while the mover snapshot stores the active custom animation.

The script regression now includes WALKTAG with zero/custom films on ordinary
and node paths, successful/short-distance arrival and interrupted walks. It
checks blocking completion, the polygon's final facing direction, custom film
selection, and exact snapshot/next-tick continuation throughout. This does not
add script escape cancellation or SWALK film overrides. The running full-route
audit remains pinned to 6a519c6, which predates this WALKTAG change.

Validation: workspace tests, Clippy with warnings denied, the web WASM check,
and all 17 opt-in original-asset tests pass (`assets/logs/walktag-*`).


## Custom SWALK walking films

Pinned `tinlib.cpp::Swalk` establishes the start position (using the destination
path's characteristics when starting outside paths), then calls Walk with its
explicit film and ignore-paths enabled. Call 75 now installs a nonzero film on
the cloned mover after standing and before `begin_direct_walk`. Invalid film
resources therefore fail before the actor/mover/control state is committed.
Zero-film calls keep their existing behavior. No snapshot format change is
needed: custom animation state and the SWALK control-restoration wait are
already represented.

The shared bytecode regression includes SWALK with control initially enabled
or disabled, zero/custom films, ordinary/node paths, finite-film completion,
short walks and interruptions. It checks control suppression while moving,
restoration on successful completion, blocking semantics and exact next-tick
snapshot continuation. As in the existing token-cancellation path, an
interrupted coroutine does not execute its completion/control-restoration code.
Disabled-axis destinations and script escape cancellation remain compatibility
gaps; this change does not claim to resolve them.

Validation: workspace tests, Clippy with warnings denied, web WASM checking
and all 17 opt-in original-asset tests pass (`assets/logs/swalk-film-*`).
The ongoing full-route run is pinned to 6a519c6 and predates both WALKTAG and
SWALK custom-film integration.


SWALK entrance coverage now also starts at x=110 outside the synthetic path
(x=0..100), with targets inside it. Ordinary and node-path variants exercise
the destination-first standing setup, zero/custom films, control initially on
or off, successful arrival and interruption. Every tick still checks exact
snapshot continuation. These cases pass without a runtime change; the focused
regression and core Clippy pass (`assets/logs/swalk-entry-*`).


The 6a519c6 native audit completed the 2,630,000-tick recorded route: all
1,652 inputs accepted, 5,048 engine and 5,048 DSP snapshot audits, and restart
to the title scene. Final engine bytes exactly match 3856346. See
[recorded results](validation/custom-walk-6a519c6.json). This run includes the
short-distance WALKED correction but predates WALKTAG/SWALK integration.
Fresh native, Chrome and WebKit audits for f776192 are running separately under
`assets/validation/custom-reels-f776192`; no result is claimed until completion.

Remaining direct-axis investigation: pinned `GetDirection` and `NewCoOrdinates`
treat -1 as a disabled axis, but SetActorDest/SetNextDest short-distance checks
use raw destination coordinates and MoveActor returns immediately when both
active targets equal -1. Replacing -1 with the starting coordinate would change
those decisions. The existing guard remains until the complete destination,
completion and pending-coroutine behavior is modeled and tested; direction or
single-step support alone is insufficient evidence to remove it.


The f776192 native/Chrome/WebKit full-route audits are complete. All hosts
accepted 1,652 inputs and reached restart/title at tick 2,630,000. Native
performed 5,048 engine and 5,048 DSP save/load audits. Both browsers produced
finite audio throughout and match native final engine bytes and RGBA exactly;
combined DSP saves differ across hosts. See
[cross-host results](validation/custom-reels-f776192.json). These runs use the
original mixer and predate optional audio headroom.

## Disabled-axis direct walks: destination and completion contract

Source audit against pinned ScummVM
`96b42d42bc8533953bdd701db059cc33c839a9ff`, `engines/tinsel/move.cpp`:
`SetActorDest` (1356), `SetNextDest` (727), `SetMoverDest` (683),
`NewCoOrdinates` (1052), `NotMoving` (1484), `Move` (1609),
`MoveActor` (1633), and `DoMoveActor` (1761).
`movers.cpp::MoverMoving` (275) reads **bMoving** for V1, not whether either
ultimate target coordinate is enabled. `tinlib.cpp::Walk` (3909) waits on
that flag in V1; `Swalk` (3131) calls it with hold zero and ignore-path true.
These findings are source-derived, not observations from a running ScummVM.

The destination must retain its raw sentinel coordinates throughout the walk:

- `SetActorDest` sets bMoving, increments the walk number and clears stop before
  applying the raw short-distance check: `abs(x - target_x) < 4` and
  `abs(y - target_y) < 2`. It does not substitute the current position for -1.
- `SetNextDest` repeats the raw short-distance check. For ignore-path movement
  it copies the intermediate target directly to the active target.
- `NewCoOrdinates` makes no displacement on a disabled axis and marks that
  axis reached. `Move` only updates position, depth, animation and step count;
  reaching the active coordinates does not itself clear bMoving.
- On a subsequent zero-displacement attempt, `MoveActor` retries with the
  minor-axis adjustment, then calls `NotMoving`. That function clears the
  active target, tests raw distance to the ultimate target, and otherwise
  re-arms a nonempty intermediate/ultimate target through `SetNextDest`.
- With both active coordinates -1, `MoveActor` returns **before** checking
  bStop. A pending stop is therefore not equivalent to an immediate Stand.
  `DoMoveActor` may retry a stalled active target once in the same update;
  the both-disabled early return remains inert.

Expected collision-free cases, with a valid current path and no path transition:

| Start | Raw destination | Source-derived result |
| --- | --- | --- |
| [100, 50] | [-1, 54] | Positions advance to [100, 52], then [100, 54]; later stalled attempts re-arm the destination and bMoving remains true. |
| [0, 50] | [-1, 54] | Same vertical displacement; the subsequent stalled attempt meets the raw short-distance check and completes. |
| [100, 50] | [108, -1] | Positions advance to [104, 50], then [108, 50]; raw vertical distance prevents ordinary completion. |
| [100, 0] | [108, -1] | Horizontal arrival is followed by completion on the stalled attempt. |
| [100, 50] | [-1, -1] | No displacement; bMoving remains true, and MoveActor does not consume a stop request. |
| [0, 0] | [-1, -1] | Initial raw short-distance check completes without setting off. |

A blocking SWALK in a persistent case must remain suspended until an appropriate
script event resolves it; silently completing it would change original script
execution. Escape has its separate V1 Stand/owned-control restoration path.

Implementation follow-through must retain both the raw ultimate destination
and active-leg target, preserve the inert-target ordering relative to stop,
and check the existing save validator before allowing these states. Extend
mover and script tests for the cases above, both stop and Escape, custom/normal
reels, control ownership, outside-path entry, collision detours, and exact
snapshot continuation. Existing `walk::Leg` tests only establish coordinate
stepping and overshoot behavior, not completion or coroutine semantics. The
runtime guard remains until that integration and validation are complete.

## Disabled-axis integration

Direct walks and SWALK now retain raw -1 destinations instead of rejecting
them. Inert active targets return before stop handling, while enabled-axis
arrival still uses the existing raw-coordinate NotMoving checks. No save
format change is needed: ultimate and active-leg targets already serialize.
The structural save validator does not prohibit these coordinates.

The mover regression exercises all six source-derived cases above across
eight updates, including per-update serialized continuation and stop handling.
The script regression adds persistent disabled-horizontal SWALK and Escape
cases for normal/custom films, ordinary/node paths, and control initially on
or off, with complete engine snapshot decode/re-encode and continuation at
every tick. V1 Stand with a sentinel coordinate and film zero is a no-op;
Walk still returns after invoking it on Escape. The engine now completes that
call and restores SWALK-owned control even if the mover remains active.

These tests do not establish every disabled-axis collision detour or
outside-path entrance combination. The existing generic Stop/superseded-walk
coroutine behavior also warrants a separate V1 reference audit: the reference
Walk loop waits on bMoving, whereas the current engine checks its saved walk
serial before returning from a pending call. That pre-existing behavior was
not changed by disabled-axis support. Logs use `assets/logs/disabled-axis-*`.

Validation: workspace tests, all 20 original-asset web integration tests,
workspace Clippy and wasm32 checking pass. The cbb7d6b full-route evidence
predates this change; no new full-route or physical-device result is claimed.


### STOPWALK caller audit

The preceding raw-stop concern must not be interpreted as a defect in normal
STOPWALK cancellation. Pinned `tinlib.cpp::StopWalk` acquires the actor token
before setting bStop and releases it immediately. `token.cpp::GetToken` kills
the previous holder; that script never reaches its post-Walk continuation.
The Rust call-74 handler already cancels pending WALK/SWALK/WALKTAG owners
immediately, before movement processes the stop flag. WALKED has released its
token and instead observes the changed walk number.

The disabled-axis script matrix now invokes actual STOPWALK bytecode from a
second process. It checks immediate cancellation while the mover still has
its destination, subsequent movement stop, absence of the killed script's
post-call write, and control remaining disabled when the killed SWALK had
owned it. Normal/custom walking and Escape scenarios retain their separate
coverage. Directly setting stop_requested in a fixture bypasses token
acquisition and cannot establish STOPWALK fidelity by itself.

Further ownership work should audit acquisition by competing WALK/WALKTAG and
WALKED, not simply remove walk-number checks: the current checks also prevent
superseded scripts from resuming, while the reference kills the previous token
owner at acquisition. Token release and process cancellation need validation
across save/load and scene reset before replacing that approximation.

### Immediate competing-walk acquisition

WALK, SWALK and WALKTAG now use the same immediate owner cancellation as
STOPWALK when starting a new walk. WALKED invokes it only after its 24-tick
startup delay, then waits without owning the token. Pending blocking walk
fields identify owners; no new serialized token table is introduced. The
currently executing scheduler entry is excluded, and click/inventory-hover
process references are cleaned after cancellation.

Regression coverage starts a real suspended owner, snapshots it, and issues a
replacement before the owner gets another scheduler turn. Both the live and
restored engines must remove that owner immediately. Cases span normal/custom
films, ordinary/node paths and WALK/SWALK/WALKTAG. A delayed WALKED competitor
must preserve the owner during the first 23 resumptions and cancel it on the
24th, when it begins walking. Existing serial checks remain for historical
waits and other interruption paths; this change does not claim a complete
implementation of every Tinsel token type or raw-stop behavior.

Logs: `assets/logs/walk-token-*`. The a1eaead native full-route audit predates
this change and remains separate evidence.
Validation passes: workspace tests, all 20 original-asset web integration
tests, workspace Clippy, wasm32 checking, and native/browser release builds.
The existing visible native process has not been restarted.

## WALKPOLY implementation contract (pending)

Static dispatch audit found V1 library ID 90, WALKPOLY, has no scene-runtime
handler and currently reaches the explicit unsupported-call stop. This is
not covered by the successful recorded route.

Pinned `tinlib.cpp::WalkPoly` (4098) and its dispatcher (6806), plus
`movers.cpp::MoverIsInPolygon` (534), establish the following requirements:

- The V1 call consumes **three** stack arguments. Actor and walking film are
  the first two; the third is consumed even though the dispatcher does not
  pass it to WalkPoly. V2 consumes two. Preserve lower stack contents.
- A calling polygon context and mover are required. An already-escaped call
  invokes StandTag and returns without acquiring the actor token.
- Ordinary V1 entry acquires the actor token, starts a path walk toward the
  calling polygon's node with the supplied film, and enables cursor following.
- Wait only while the actor is outside that polygon **and** remains moving.
  The containment test uses the actor animation object's position. Being
  inside on entry permits immediate script continuation after starting the
  walk. Entering later releases the caller while movement continues.
- The coroutine yields before checking Escape. Escape calls StandTag, frees
  the token, and returns. A normal return only frees the token: unlike
  WALKTAG, it does not adopt the polygon's pose/film at completion.
- V1 does not compare walk numbers in this loop; competing actor-token
  acquisition kills its owner. Once WALKPOLY has returned, it no longer owns
  the token despite any remaining movement.

Implementation should reuse existing routing, film, token cancellation and
polygon geometry without equating polygon entry with final arrival. Required
regressions include initially inside/outside, crossing before final arrival,
movement ending before entry, stop/replacement, both Escape timings, custom
and normal films, non-lead/lead actors, lower-stack preservation, and exact
snapshot continuation of the suspended call. Saved pending calls must retain
polygon context and remain structurally validated. This is a source-derived
contract; no WALKPOLY runtime support or reference-process comparison is yet
claimed.

### WALKPOLY runtime integration

Library call 90 is now handled with the V1 three-argument stack contract.
It shares routing and saved tag-walk state, but returns on polygon entry or
movement completion without adopting the tag pose or stopping the remaining
walk. Its containment check uses animation-object coordinates. After a yielded
tick it checks Escape before polygon entry, invokes StandTag and returns.
Immediate competing-walk/STOPWALK cancellation also recognizes its pending
owner. Unlike WALKTAG's historical fallback, WALKPOLY does not kill its caller
merely because a raw mover stop changed the walk number.

A new matrix covers initially inside, crossing before final arrival, movement
ending without entry, Escape before/after startup, raw stop, scripted STOPWALK
and competing WALK, with normal/custom films and lead/non-lead actors. It
checks lower-stack preservation, absence of normal tag-pose adoption, no
post-call execution by killed owners, and exact snapshot continuation each
tick. A malformed pending WALKPOLY without polygon context is rejected;
structural validation also requires a valid polygon index and an existing
mover. No new snapshot format is required.

The recorded full route does not exercise this newly supported call. These
are focused reference-derived cases, not proof of all polygon/path shapes or
live ScummVM equivalence. Logs: `assets/logs/walkpoly-*`.
Validation passes: workspace tests (321 core tests passed, one ignored), all
20 original-asset web integration tests, workspace Clippy and wasm32 checking.

## HIDEACTOR audit: separate V1 visibility state (pending)

V1 library ID 34 is still unsupported in the scene dispatcher. Pinned
`tinlib.cpp::HideActorFn`/HIDEACTOR consumes one actor argument;
`actors.cpp::HideActor` resolves a mover and calls `HideMover(mover, 0)`, or
sets the non-mover actor's hidden flag through ToggleActor. Lead alias handling
and actor-range validation must remain checked.

Important version distinctions from the pinned source:

- `movers.cpp::HideMover` sets **mover** bHidden and SlowFactor and places its
  object at Z=-1. It deliberately avoids MultiHideObject because animation
  stepping can undo that object's hidden flag. This is independent of
  Actor::bHidden and of whether the actor is alive.
- `T1MoverProcess` freezes a hidden special reel but continues DoMoveActor for
  normal movement. HIDEACTOR passes SlowFactor zero, so the hidden-step gate
  in MoveActor does not add delay. Hiding is not a stop or cancellation.
- SetMoverZ leaves hidden depth alone; AlterMover restores Z=-1 after its
  initial animation step. PositionMover/UnHideMover and normal completion of
  an unreplaced t1PlayReel can make the mover visible again. A replaced reel
  must not unhide the mover on behalf of its successor.
- The loop responding to ActorHidden in `play.cpp` is **t2PlayReel**, not
  t1PlayReel. V1 non-mover HIDEACTOR sets actor state, but copying V2's per-frame
  reel hiding would add behavior absent from that V1 loop.
- SaveMovers copies bHidden only for V2+. V1 scene-stack restoration therefore
  must not acquire V2 hidden-state semantics. An exact runtime snapshot still
  needs enough state to resume the current hidden mover deterministically.

Current Rust Actor.hidden already affects pointing and special-reel stepping,
while Mover has no separate persistent hidden state. The renderer draws the
background before actor/mover animations, so assigning Z=-1 alone would still
draw a hidden mover over it. Integration must handle the background depth
ordering, separate mover/actor visibility, save compatibility, and unhide
ownership together. Simply filtering every hidden actor's films would conflate
V1 and V2 behavior and hide replacement reels incorrectly.

Required tests: existing/uncreated declared mover, ordinary actor, lead alias,
normal walking vs special-reel ticking while hidden, hiding during PLAY/TALK,
replacement/unhide ownership, pointing, draw order, exact save continuation,
and V1 scene-stack restoration. The audit is source-derived; no HIDEACTOR
runtime support is claimed yet.
