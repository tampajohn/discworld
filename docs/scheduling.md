# Cooperative scheduling evidence

Source revision: 96b42d42bc8533953bdd701db059cc33c839a9ff. This is an implementation specification in progress, not a claim that the current Rust engine follows the complete scheduler.

`common/coroutines.cpp`, `CoroutineScheduler::createProcess` and `schedule`:

- Creating a process during dispatch inserts it immediately after the current process. Repeated creation therefore reverses creation order. These newly inserted processes can run in the same scheduling pass.
- Creating a process outside dispatch inserts it at the head of the active list.
- A new process has sleep time 1. Each visit decrements its sleep time before testing readiness. A yielded sleep of 1 resumes on the next pass; sleep 2 skips one pass.
- After dispatch, iteration follows the current process's updated successor, accounting for insertions and removals. Taking a fixed snapshot of runnable processes would change behavior.
- A completed coroutine is removed. Explicit rescheduling and process cancellation also affect traversal; these must be modeled before claiming full scheduler equivalence.

`engines/tinsel/scene.cpp`, `LoadScene` initializes polygons, then actors, then creates the entrance script if non-null, then the scene script if non-null. With immediate insertion after the current process, scene and entrance startup dispatch precede the actor processes queued earlier. The caller's scene-transition phase and other active processes still matter for exact tick timing.

`engines/tinsel/actors.cpp`, `StartActor` resets scene-local actor fields and records masking/code. STARTUP runs only for an actor that is alive and has non-null code. Actor interpreter contexts carry the event and actor ID; treating these scripts as independent zero-context VM instances is insufficient.

`engines/tinsel/polygons.cpp`, `InitPolygons` does **not** dispatch polygon STARTUP events in V1. The tag-polygon startup loop belongs to the V2+ branch. V1 initializes path adjacency and persistent tag/exit state instead. Avoid importing V2 behavior into the DOS V1 runtime.

The engine now dispatches scene and entrance VMs through the queue, with per-process waits, V1 startup event context, shared globals, and scene-generation cancellation. Tests cover shared-global visibility during waits and cancellation of old entrance scripts on scene change. The master VM and animations remain outside this queue with provisional fixed ordering. Actor and polygon scene support must remain rejected until their required initialization is implemented.

`scheduler.rs` now provides the basic owned process queue: stable identities, same-pass insertion, pre-decrement sleeps, peer cancellation, and scene-generation cancellation that preserves global processes. Tests cover insertion order, sleep/snapshot behavior, and cancellation on either side of the dispatch cursor. It is integrated into Engine for scene and entrance scripts. Explicit rescheduling, event objects, actor/polygon contexts, and master/animation integration remain outstanding.
