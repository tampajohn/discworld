# Tinsel 1 reel scheduling contract

Status: implementation work remains. The startup first-step discrepancy in
[reference-startup-phase.json](validation/reference-startup-phase.json) is
confirmed. This document records the source-derived ordering needed to fix it;
it does not claim that the current engine satisfies that ordering.

Reference: ScummVM `96b42d42bc8533953bdd701db059cc33c839a9ff`,
`common/coroutines.cpp` and `engines/tinsel/play.cpp`. The reference source and
executable are unmodified.

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

## Current implementation mismatch

The Rust script scheduler already implements insertion after the current job
and reverse creation order. Its existing tests cover same-pass children, parent
termination, cancellation and scene replacement. Films, however, live outside
that queue in `Engine::films` and advance before scene script dispatch.

The scene PLAY/SPLAY handler currently prepares all columns, claims ownership
and creates movers during the library call. It appends nonblocking columns to
the film vector. Their first step waits for the next engine tick. Internal
`launch_film` and restored reels also bypass the script queue. This is a
scheduling integration problem; changing the generic queue's insertion rule
would not fix it.

Stepping every new film immediately inside PLAY would incorrectly run children
before their caller yields. Stepping all new films at the end of a tick would
also lose ordering with newly spawned scripts and existing peers. A correct
integration must represent reel dispatch relative to those jobs, retain that
order on later ticks, and defer dispatch-time side effects. Resource preparation
must distinguish scheduling data from actor/mover state read during dispatch.

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
