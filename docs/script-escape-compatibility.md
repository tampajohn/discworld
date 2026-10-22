# V1 script escape compatibility

Reference revision: 96b42d42bc8533953bdd701db059cc33c839a9ff.
Current implementation is incomplete: the engine captures global/per-context
escape generations and handles WAITTIME plus scripted-walking cancellation,
with DWRSAVE v14 state.
Native/browser Escape still handles configuration/inventory UI; the shared
script event API is not connected to hosts. Film/speech cancellation
and the special intro skip path remain pending.

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
