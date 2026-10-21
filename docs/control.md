# V1 control and cursor state

## Current host compatibility boundary

The paragraphs under “Initial implementation evidence” below describe an early
milestone. Polygon/actor interaction, text, speech and inventory are implemented
now and exercised by the complete recorded route; they are not still absent.
That route uses semantic actions, so it does not prove raw mouse/touch fidelity.

Native and browser hosts currently expose development walk/action/look controls.
Original V1 click arbitration remains an implementation requirement. In pinned
ScummVM `96b42d42bc8533953bdd701db059cc33c839a9ff`, inspect
`engines/tinsel/tinsel.cpp`'s mouse event process and
`engines/tinsel/events.cpp`'s `AllowDclick`, `PlayerEvent` and
`PolyTinselProcess`:

- Single-left script dispatch waits `_dclickSpeed + 1` scheduler ticks while
  holding the left-button token. Double-left acquires that token to cancel the
  pending single-click process. This is more than mapping browser `dblclick` to
  an action after an immediate walk has already run.
- Control is checked again after that wait. Dispatch must not proceed if a
  script disabled control while the click was pending.
- `PlayerEvent` advances user/left-event counters before its control check, so
  input during disabled control can still interrupt waiting speech.
- V1 routes walk/action/look to inventory handling when inventory is active.
  Conversation transitions also need coverage. Both hosts now call the shared
  `Engine::primary_click` for their immediate click control. Hidden conversation
  inventory consumes the click as a left event, waking speech waits even when
  control is enabled; the browser previously attempted a rejected world action
  in that case. A core regression compares the complete resulting state with a
  left event for both enabled and disabled control. This does not add the
  original double-click timing.

Required acceptance cases include single versus double clicks at the timing
boundary, cancellation without duplicate script execution, control loss during
the pending click, speech interruption, inventory/conversation routing, and
equivalent native/browser behavior. These cases are not established by the
full semantic-input route. Original menus, cursor trails and keyboard cursor
behavior also need their own acceptance evidence.

## Initial implementation evidence

The control model follows the V1 branch of tinlib.cpp Control: mode 0 hides the cursor and saves its position, mode 1 restores control and the main cursor, mode 2 keeps a frozen visible cursor, mode 3 keeps a visible cursor without forcing a new freeze, and mode 4 disables control for scene startup. Restoring after mode 0 or 2 returns to the saved position; mode 3 or 4 preserves the current position. Every CONTROL call clears the startup menu permission.

Tests cover cursor restoration, frozen movement, idle-time reset, and idle queries while control is disabled. Host key/button events and effective cursor movement reset the idle clock. IDLETIME reports whole seconds at the deterministic 24-Hz engine clock. Master-script queries now use this state instead of stopping when control is enabled.

The main cursor uses the declared cursor film's first reel. It is recreated on scene entry and CONTROL_ON, advances while visible, and is composited at logical cursor coordinates. Cursor trails, held-object auxiliary cursors, keyboard acceleration, exact coroutine timing, token contention, and double-click cancellation remain outstanding. Tag enablement is recorded but polygon interaction is still unsupported. This is not complete input compatibility.

With music enabled, the latest original-data run renders the title screen and reaches PRINT at tick 392 (evidence/headless-cursor.json). It emitted 106 MIDI messages. The WAV portion after title music began contains 28482 samples, of which 12427 are nonzero. Text, speech timing, and interaction behavior remain to be implemented; no independent frame/audio equivalence claim is made.
