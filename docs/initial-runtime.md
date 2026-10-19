# Initial Discworld runtime

The core now runs the original master initialization, preserves its continuing idle-monitor task, enters Psygnosis, and executes the scene's BACKGROUND, nonblocking PLAY, WAITTIME, and NEWSCENE calls. It transitions to TWG and now executes PLAYSAMPLE and WAITFRAME. The 24-tick framebuffer shows the Psygnosis owl; a 150-tick request runs through TWG with PCM output. The next scene needs actor/polygon scheduling and currently stops a longer run at tick 154. Latest evidence is in evidence/headless-queue.json; the earlier single-scene-VM run stopped at 155.

The standalone logo reel export and runtime output at ticks 24, 26, and 27 have identical PNG SHA1 `2ea171de12b5a6c34f5f50dbc0addba0e104cb61`. This checks integration of the same renderer, not independent equivalence to ScummVM.

## Animation and composition

The animation state stores the instruction index, frame, animation point, orientation, period, and countdown. Frame changes, relative jumps, flips, adjustments, NOSLEEP, HIDE, and END are implemented. The first tick advances immediately. Subsequent advances use the film rate and the reference's 24 ticks per second. An instruction budget rejects infinite non-yielding loops; errors preserve the prior state.

Frame capacity follows the initial multipart object's number of image slots. Composition applies image animation offsets, clipping, and transparent coverage to an RGBA framebuffer. It uses a local palette per multipart object; runtime palette relocation, palette changes, special drawing flags, and exact object ordering remain outstanding. This preview compositor must not be treated as complete rendering compatibility.

## Scheduling limits

The current runtime queues scene and entrance scripts, each with its own VM and waits. Actor and polygon initialization remain unsupported. It refuses broader scenes, nonzero transition modes, blocking PLAY, and playback position overrides. It retains the master VM after NEWSCENE and handles its IDLETIME query while control is disabled, plus WAITTIME. Enabling control requires the remaining input and idle tracking behavior.

WAITTIME preserves the reference's sleep-first, post-decrement loop: a duration of N waits N plus one ticks. Scene NEWSCENE cancels old scene peers and inserts new entrance/scene scripts into the current pass; the master continuation remains separate. Fixed-tick ordering is provisional until compared with the reference coroutine scheduler. The current implementation is not a general scheduler, and native/browser hosts remain pending.

## Next work

Use the live reference trace to compare ordering, extend sample playback to speech and device output, and replace the restricted task handling with the full cooperative scene scheduler. Add actor, polygon, and interaction-event processes without bypassing unsupported calls. Integrate native/browser presentation and audio, followed by movement, interaction, and durable save state.

## Actor startup and input-wait progress

The latest run (evidence/headless-waitkey.json) reaches title.scn at tick 361 and stops on ENABLEMENU. Basic actor lifetime and scene startup state are allocated from the V1 master actor-count chunk (0x3334000f). Live actors with non-null code receive queued startup processes carrying their actor ID; completion marks their code complete. Movement, actor playback ownership, masking, killing/revival, and interaction events remain incomplete. Polygon scenes are still refused.

WAITKEY now suspends one process while peers continue. The host can report a key/button event or logical cursor position; events predating the wait do not release it. Tests cover this behavior and waking on cursor movement. Escape counters, cursor availability, menu-active waits, and menu-close cooldown remain unimplemented. ENABLEMENU is rejected rather than accepting those missing behaviors.
