# V1 mover reel compatibility gap

Status: reference-derived transition model implemented in `mover_reel.rs`;
live mover/engine and snapshot integration remain pending. Reference revision:
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

## Current implementation gaps

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
