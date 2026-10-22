# V1 script escape compatibility

Reference revision: 96b42d42bc8533953bdd701db059cc33c839a9ff.
Current implementation is incomplete: Machine stores only escape_enabled;
there is no global escape event generation or captured per-context generation.
Native/browser Escape currently handles configuration/inventory UI and does
not implement the reference's script cancellation mechanism.

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
