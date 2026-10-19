# V1 control and cursor state

The control model follows the V1 branch of tinlib.cpp Control: mode 0 hides the cursor and saves its position, mode 1 restores control and the main cursor, mode 2 keeps a frozen visible cursor, mode 3 keeps a visible cursor without forcing a new freeze, and mode 4 disables control for scene startup. Restoring after mode 0 or 2 returns to the saved position; mode 3 or 4 preserves the current position. Every CONTROL call clears the startup menu permission.

Tests cover cursor restoration, frozen movement, idle-time reset, and idle queries while control is disabled. Host key/button events and effective cursor movement reset the idle clock. IDLETIME reports whole seconds at the deterministic 24-Hz engine clock. Master-script queries now use this state instead of stopping when control is enabled.

The main cursor uses the declared cursor film's first reel. It is recreated on scene entry and CONTROL_ON, advances while visible, and is composited at logical cursor coordinates. Cursor trails, held-object auxiliary cursors, keyboard acceleration, exact coroutine timing, token contention, and double-click cancellation remain outstanding. Tag enablement is recorded but polygon interaction is still unsupported. This is not complete input compatibility.

With music enabled, the latest original-data run renders the title screen and reaches PRINT at tick 392 (evidence/headless-cursor.json). It emitted 106 MIDI messages. The WAV portion after title music began contains 28482 samples, of which 12427 are nonzero. Text, speech timing, and interaction behavior remain to be implemented; no independent frame/audio equivalence claim is made.
