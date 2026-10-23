# Tinsel 1 reel scheduling contract

Status: script-started and internal PLAY reels now share the scene-script
scheduler. The initial comparison against the recorded reference startup has
120/120 aligned RGB matches, eliminating the prior 17 phase mismatches. This
is startup evidence, not proof of complete scheduler equivalence.

Reference: ScummVM `96b42d42bc8533953bdd701db059cc33c839a9ff`,
`common/coroutines.cpp` and `engines/tinsel/play.cpp`. The reference source and
executable are unmodified.

## Empty films

Both `PlayFilm` and `PlayFilmc` return when `numreels` is zero, before converting
the frame rate or spawning anything (play.cpp lines 986 and 1048). The runtime
now accepts a complete eight-byte empty-film header and returns without a wait,
including blocking PLAY/SPLAY/TOPPLAY. Unused rate values do not cause a
division by zero or a validation failure. Nonempty films still require a
positive rate and a complete, bounded reel table.

The generic reader no longer imposes a first-reel requirement on every film.
Background/cursor initialization and mover reel restoration still require one
and reject empty films explicitly. Mover resource validation now uses checked
first-reel access. Other first-reel consumers were audited for checked access
before rate division.

The bytecode regression covers all three calls, blocking and nonblocking forms,
rates 0, -1 and 24, stack consumption, continuation in the same dispatch, exact
engine state except the script's subsequent global assignment, internal film
launch, and snapshot round-trip. It failed with `invalid film header` before
the fix. Reader tests cover truncation and nonempty invalid rates; the mover
fixture verifies rejection of an empty saved reel. These synthetic cases do
not establish whether the supplied game's recorded route encounters an empty
film or resolve the separate first-frame scheduling discrepancy.

Validation passes: workspace tests (335 core), all 20 original-asset web tests,
Clippy across all targets, WASM checking, and native/tools/browser release
builds. Logs are under `assets/logs/empty-film-*`. No new full-route,
live-reference, visible-session or physical-iPad result is claimed.

## Dispatch ordering

`CoroutineScheduler::createProcess` (lines 485–540) inserts a new process
immediately after the current process, with sleep 1. Without a current process,
it inserts at the head. `schedule` (lines 218–245) decrements sleep before
dispatch and obtains the next process from the updated linked list after the
caller yields or finishes. Therefore children run in the same scheduler pass,
after the caller's uninterrupted work. Successive children run in reverse
creation order. Their positions remain in that list on subsequent passes.

`PlayFilm` (play.cpp lines 973–1022) schedules columns in descending order and
calls `NewestFilm` before each spawn. Its explicit give-way branch applies only
to Tinsel 2 and later. In Tinsel 1 the caller can continue scripting before any
new reel dispatches. The reversed insertion restores ascending column order
when the caller yields.

For a caller that schedules films A then B, each with columns 0 and 1, the
dispatch order after yielding is B0, B1, A0, A1, then the previously next peer.
Arbitration can reject A when A and B share an actor. Scheduling A does not
entitle it to create a mover, alter the object's placement, or display a frame
before B is scheduled.

`PlayFilmc` (lines 1032–1101) schedules secondary columns in descending order,
then invokes column 0 inside the caller. Column 0 may take its first animation
step before the caller yields; the secondary columns dispatch afterward. If
column 0 finishes immediately, the caller continues executing before those
children run. The blocking primary must not be stepped again as a child.

`t1PlayReel` performs actor-alive, escape, latest-film, talking-film and duplicate
reel checks at dispatch, not at `PlayFilm` scheduling (lines 419–487). Object
creation and animation initialization follow those checks. These checks must
observe changes made by the caller before it yielded, and by earlier children.

## Implementation and remaining scope

PLAY/SPLAY/TOPPLAY enqueue requests containing the film, column, position,
layer and captured Escape generation. Only the scheduling-time actor lookup
and latest-film/step reset happen before the caller yields. Each queued job
then checks actor life and arbitration, resolves placement and depth, claims
the actor, creates a declared mover if needed, and takes its first step.
Rejected superseded reels do not initialize animation resources. The blocking
primary still runs inside its caller. Internal TALK film launches use the same
queue.

Active jobs retain their scheduler position. `Engine::films` remains drawable
storage, but its earlier animation phase skips reels with queue jobs. Completion,
replacement and Escape use the existing ownership/SPLAY cleanup. A new reel
checks Escape after its initial step and sleep, matching the reference loop.
The shared first-column depth and factor persist across dispatches rather than
being computed eagerly for all columns at library-call time.

Save version 21 wraps the existing snapshot with ordered process-to-reel-task
associations and shared depth state. Both pending and active jobs are restored;
script validation distinguishes these native tasks from bytecode contexts.
Malformed identities, duplicate associations, noncanonical order and invalid
state are checked. No new wrapper is emitted for a legacy state with no tasks
and zero dispatch-depth state. Legacy films without jobs retain their old
advance phase; source-specific `restore_actor_reel` rebuilding still uses that
path. That restoration scheduling, and ordering against the separate master,
background, cursor and mover phases, remain compatibility work.

Regressions cover superseded films with deliberately unusable animation data,
WAITFRAME before/after the first child dispatch, pending Escape, exact pending
and active snapshot continuation, multiple films/columns retaining their order
across passes, and malformed version-21 saves. Existing blocking-primary,
SPLAY, actor death, TALK and legacy envelope tests are retained with their
assertions updated where the reference requires same-pass first steps.

The rebuilt runtime comparison and validation summary are in
[reel-dispatch-startup.json](validation/reel-dispatch-startup.json), against the
unchanged 120-frame reference capture. Workspace tests (337 core), all 20
original-asset checks (19 plus a menu-version assertion recheck), Clippy, WASM
and both host builds pass. Full recorded-route results remain pending.

## Required regression evidence

- Compare the 120 captured startup draws after scene alignment. The 17 known
  phase mismatches must disappear without changing the matching RGB content.
- Exercise nonblocking PLAY followed by WAITFRAME in the same caller: the caller
  sees no child step before yielding; the child starts in that same pass, and
  the following caller resumption observes its step count.
- Schedule two different films for one actor without a yield between them.
  Verify the later film wins without transient mover creation or geometry
  changes from the rejected film. Include duplicates, dead actors and TALK.
- Interleave multi-column films with script spawns and older peers. Verify
  first-pass and subsequent-pass order, including an immediately finished
  blocking primary and a primary that sleeps.
- Preserve SPLAY cleanup, Escape, actor removal and scene cancellation for
  pending and running reels. A scene transition must discard old-scene work.
- Save and restore between passes with sleeping scripts and active reels, then
  compare exact continuation. Legacy save import must retain its documented
  compatibility behavior; new dispatch ordering must not be silently lost.
- Rerun original-asset tests and the native/browser recorded route after the
  change. Those checks complement the reference comparisons; they do not
  replace ordering evidence or prove all alternative game interactions.

## Scene reconstruction follow-up

The pinned `savescn.cpp::sortActors` (lines 182–204) establishes actor life and
queues saved reels, then restores auxiliary scales and calls STAND for active
saved movers. `play.cpp::RestoreActorReels` (lines 1109–1135) only records
parameters, calls `NewestFilm`, and creates a `PlayProcess`; it does not claim
the actor or create a mover synchronously. Thus the restored mover state is
present when those queued reels eventually resolve their placement and hide
their moving actor. The order of queued restored columns also follows scheduler
insertion, rather than the order in which an eager reconstruction loop appends
drawable objects.

The current `rebuild_scene_objects` calls the synchronous `restore_actor_reel`
before `restore_scene_movers`. This can initialize/hide a mover before the mover
map is replaced. Moving restored reels into queued jobs must defer those side
effects until after mover restoration, not just defer their first frame.

There is a second integration requirement: the reference
`DoRestoreSceneFrame` reconstructs objects at `RS_COUNT` and calls
`ResumeInterprets` at count 1. `ResumeInterprets` creates interpreter processes
in the existing scheduler; it does not discard the running reel jobs. The Rust
`restore_scene_scripts` now clones the existing queue, removes only interpreter
and other non-reel tasks, and inserts the restored scripts ahead of retained
reels. Reel identities, sleep state and relative order survive. Validation and
queue construction occur before committing the replacement, so invalid saved
contexts leave the original queue unchanged. Cancelled click state and departed
POINTED label waits are removed after successful restoration. This also avoids
resetting the process identity counter and accidentally reusing departed IDs.

The regression retains four active reel jobs with sleep 3 and two pending
duplicate jobs with sleep 1 across interpreter restoration. It checks identities,
ordering, waits, unchanged state on invalid restoration, and exact version-21
save/load continuation through wakeup. Moving `restore_actor_reel` itself to a
deferred request remains outstanding; this change establishes the queue
preservation that integration requires, including for atomic `rebuild_scene`.

Validation: 337 core tests, all 20 original-asset checks, Clippy, WASM checking
and native/tools/browser release builds pass. See
[restore-reel-queue.json](validation/restore-reel-queue.json). The ongoing
full-route audits remain pinned to the earlier `85b728a` runtime.

The restore request must also retain the saved depth. A nonzero `ppi.z` bypasses
normal depth calculation without updating the shared first-column depth/factor;
zero uses normal dispatch-time depth calculation (`t1PlayReel`, lines 541–596).
Reusing PLAY's default request and overwriting Z after its ordinary startup
would incorrectly alter that shared state.

Required targeted cases include a restored film on an already restored mover,
multiple saved actors, saved explicit and zero depths, dead actors, duplicate
requests, pending save/reload before the reel runs, and the later interpreter
resume retaining the live reel job. These are source-derived requirements;
the current startup comparison and recorded-route runs do not establish them.

## Additional trace instrumentation

`tools/reference_frames_lldb.py` now records a shared ordered event stream for
nonblocking reel scheduling (`play.cpp:1008`), `t1PlayReel` entry, animation init
and animation steps. Reel entry includes resumptions, not just first starts.
Animation and reel fields are checked for readable debugger values; failures
stop capture rather than silently recording zero. The schedule breakpoint does
not cover blocking secondary creation or restored/internal scheduling sites.

The attempted capture at `assets/validation/reference-reel-order` timed out
without a frame or completed report. All seven breakpoint locations resolved
uniquely, but no new runtime ordering evidence is claimed. The log includes
`CVDisplayLinkCreateWithCGDisplays` reporting zero active displays. The cause
of the startup stall is not established. The launcher exited on timeout and
its remaining owned reference process was explicitly terminated. The earlier
completed first-step capture remains the live-reference timing evidence.
