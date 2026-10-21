# V1 control and cursor state

## Current host compatibility boundary

The paragraphs under “Initial implementation evidence” below describe an early
milestone. Polygon/actor interaction, text, speech and inventory are implemented
now and exercised by the complete recorded route; they are not still absent.
That route uses semantic actions, so it does not prove raw mouse/touch fidelity.

Native and browser hosts currently expose development walk/action/look controls.
Timed world/inventory click dispatch is now connected, but full V1 input compatibility remains an acceptance requirement. In pinned
ScummVM `96b42d42bc8533953bdd701db059cc33c839a9ff`, inspect
`engines/tinsel/tinsel.cpp`'s mouse event process and
`engines/tinsel/events.cpp`'s `AllowDclick`, `PlayerEvent` and
`PolyTinselProcess`:

- Single-left script dispatch waits `_dclickSpeed + 1` scheduler ticks while
  holding the left-button token. Double-left acquires that token to cancel the
  pending single-click process. This is more than mapping browser `dblclick` to
  an action after an immediate walk has already run.
- Polygon scripts check control again after that wait. Actor and inventory
  object processes do not have that second check in their V1 branches; a shared
  delay must preserve this distinction.
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

### Physical button classifier

`discworld_core::mouse::MouseInput` implements the first stage of the reference's
V1 `MouseProcess`. Native button edges and browser pointer edges now feed this classifier; explicit
keyboard/action controls remain available. Five tests cover strict threshold comparison,
timing from release rather than press, ending a double-click pair, independent
left/right timers, swapped buttons, drag-event order, the initial-click quirk,
32-bit clock wrap, a zero threshold, and serialization midway through a pair.

The reference initializes both prior-release times to the current tick, uses a
default threshold of 10 ticks, and classifies a down event as double when the
wrapping elapsed time is strictly less than the threshold. On a double's release
it subtracts the threshold from the prior release time rather than recording
the current time. Physical timers are updated before button swapping. Right
double-click maps to `PLR_NOEVENT`, not a second look action. Each press emits
drag-start before its click classification; release emits drag-end.

Remaining integration includes inventory resizing and slider dragging,
raw host button delivery, and persisting the physical button classifier. `AllowDclick` is called by polygon,
actor and inventory-object processes (`dialogs.cpp::ObjectProcess`). Bare-ground
`WalkProcess` does not call it, so delaying every single-click globally would
change the original movement behavior. A browser `dblclick` callback alone
cannot reproduce these rules. Raw native/browser button events now feed the shared classifier and timed dispatch.

`discworld_core::click::ClickToken` now implements the scheduler-owned part of
`AllowDclick`. A single-click script acquires ownership and yields for threshold
plus one passes; a double-click script cancels that owner and immediately frees
the token. Acquisition requires an active dispatch, repeated acquisition by the
owner and invalid release fail explicitly, and process teardown must release
ownership. Tests exercise the real scheduler, including cancellation of an
earlier queue entry without losing a neighboring dispatch, exact delay,
serialization/restoration of a pending wait, and stale ownership rejection.
`Engine::world_click` attaches this delay/cancellation to newly queued actor
and polygon scripts; ground walks and look events remain immediate. The existing
semantic interaction methods retain their timing for recorded routes. Native and browser physical input now reaches these paths. This is not a claim
of completed original mouse support: inventory resizing, slider dragging, menus,
configurable button swapping and timing, and broader gesture parity remain.

Pending click ownership and dispatch phases are persisted in the core save's
version-2 extension. States without pending clicks retain byte-compatible
version-1 encoding. Validation rejects missing/duplicate owners, inappropriate
script targets/events and invalid pending sleep values. The engine regression
covers single-click delay, complete-state equality after snapshot decoding,
double-click cancellation after reload, polygon control loss during the wait,
and version-1 round-trip compatibility. Actor control semantics and bare-ground
walking are retained by using the existing distinct dispatch paths; broader
reference comparisons remain needed.

## Initial implementation evidence

The control model follows the V1 branch of tinlib.cpp Control: mode 0 hides the cursor and saves its position, mode 1 restores control and the main cursor, mode 2 keeps a frozen visible cursor, mode 3 keeps a visible cursor without forcing a new freeze, and mode 4 disables control for scene startup. Restoring after mode 0 or 2 returns to the saved position; mode 3 or 4 preserves the current position. Every CONTROL call clears the startup menu permission.

Tests cover cursor restoration, frozen movement, idle-time reset, and idle queries while control is disabled. Host key/button events and effective cursor movement reset the idle clock. IDLETIME reports whole seconds at the deterministic 24-Hz engine clock. Master-script queries now use this state instead of stopping when control is enabled.

The main cursor uses the declared cursor film's first reel. It is recreated on scene entry and CONTROL_ON, advances while visible, and is composited at logical cursor coordinates. Cursor trails, held-object auxiliary cursors, keyboard acceleration, exact coroutine timing, token contention, and double-click cancellation remain outstanding. Tag enablement is recorded but polygon interaction is still unsupported. This is not complete input compatibility.

With music enabled, the latest original-data run renders the title screen and reaches PRINT at tick 392 (evidence/headless-cursor.json). It emitted 106 MIDI messages. The WAV portion after title music began contains 28482 samples, of which 12427 are nonzero. Text, speech timing, and interaction behavior remain to be implemented; no independent frame/audio equivalence claim is made.

## Timed inventory input

`Engine::inventory_click` now delays both scripted pickup/drop and ordinary
putdown using the same left-button token as world clicks. V1 `ObjectProcess`
waits before executing pickup or DROPCODE bytecode. `InvPdProcess` also waits
before ordinary putdown; it preserves the absolute insertion index and checks
current inventory space when it wakes. An intervening double-click action can
cancel either pending operation. Look remains immediate. The existing semantic
`interact_inventory` method remains available for unchanged recorded routes.

Regression coverage includes delayed scripted PUTDOWN, item ownership remaining
unchanged throughout an ordinary wait, cancelling a saved pending drop with an
action, exact snapshot continuation, and capacity becoming available during the
wait. A synthetic hidden window avoids unrelated rendering resources in these
state tests; browser UI tests separately exercise original inventory rendering.
Pending ordinary drops require core save version 3. Inventory resizing and slider dragging still need integration before claiming original input parity.

## Physical input delivery and persistence

Native drains SDL2 left/right button events in order and browser sends pointer down/up/cancel
through its serialized worker. The default canvas action uses physical left
clicks (single selects/walks, double acts); right click looks. Explicit action
and look controls remain available for touch. Browser pointer capture ensures
release outside the canvas reaches the worker. Releases can update button
history while paused, without advancing simulation. Inventory title-bar dragging now moves the original frame, title and icons;
resizing and slider dragging remain unfinished.

Core save version 4 persists physical button timing with pending click state.
The regression saves after a first click, restores, then applies the second
click: it produces the same action and cancels the single in both engines.
Legacy saves remain loadable and do not acquire mouse history until a physical
edge arrives. Restart initializes physical timing at the restart tick.
The native host previously sampled minifb's current button state, which could
lose a complete press/release between polls. SDL2's event queue preserves both
edges and their cursor positions even when they arrive before one host frame.
The host uses each event's position rather than the final pointer position.
SDL's logical 320×200 viewport maps window coordinates to original pixels;
the display uploads the same RGBA frame with nearest-neighbor scaling.
See the [SDL2 event API](https://docs.rs/sdl2/0.38.0/sdl2/event/enum.Event.html).

Only buttons pressed during active manual play are owned by the host. Their
releases still reach the engine while paused, and focus loss releases owned
buttons once. Pointer capture retains releases outside the window. Loading a
save resets host ownership; physical timing remains part of the restored core.
SDL's OS double-click count is ignored: the original engine classifier still
decides singles/doubles. A queued-event regression injects down/up before
polling, checks order and distinct coordinates, and verifies both edges survive.
Full original interaction and physical-device acceptance remain incomplete.

Native SDL acceptance used the original inventory checkpoint at tick 33,200.
A macOS HID-stream click picked up banana object 190; an independent headless
load of the native F5 save confirmed `held_object=190` at tick 33,254. A second
HID click with a measured 2.300 ms press/release interval put it down; the native
save at tick 33,286 restored with `held_object=-1` and luggage contents
`[190,200]`. The first attempt used PID-targeted synthetic mouse events, which
did not reach SDL's button handler (save remained core version 1); only the
subsequent HID-stream results count as interaction evidence. Evidence remains
local in `assets/logs/sdl-{pickup,putdown}-hid-state.json` and the corresponding
scratch saves and screenshots. The four native tests and all 247 enabled
workspace tests pass; two asset-dependent tests are opt-in. Native Clippy and
release build pass. SDL is statically linked; `otool -L` shows only macOS system
libraries/frameworks, without a Homebrew SDL dependency. The bundled SDL CMake
project needs CMake 4's policy-floor compatibility setting, supplied in
`.cargo/config.toml`.

## Inventory frame hit regions

The shared window layout now classifies the English DOS V1 regions from pinned
`dialogs.cpp::invArea`: outside, eight border/corner regions, title header,
scroll arrows, page regions, slider thumb, and body. This is the geometry needed
by `invDragStart`; header movement is implemented, while resizing and thumb
dragging are still unfinished.
The declarations `idec_convw`, `idec_inv1`, and `idec_inv2` make these windows
movable. The reference starts movement only on a left drag in the header,
translates frame/background/title/icons by cursor deltas, and ends it on left
release or an action event. Header movement preserves animation progress, updates hit regions immediately,
and persists through save/load as described below.

The region audit found a concrete outside-click bug: V1 `EXTRA=1` includes one
additional pixel at the right and bottom, beyond the background rectangle's
exclusive bounds. Clicking those pixels previously closed the window. Native
and browser now use the same corrected region test. Borders also take priority
over overlapping scrollbar arrows, as in the reference. Arithmetic uses i64
for bounds calculations so extreme input coordinates cannot overflow.

The unit regression checks each border/corner, exact header boundaries, scroll
thumb/page boundaries, the extra pixels and extreme coordinates. The opt-in
real-asset test `inventory_frame_edge_does_not_close_window` clicks both extra
edges of the original luggage checkpoint, advances 24 ticks, and verifies that
the inventory stays open with an empty hand. One pixel farther right still
closes it. Run with the same asset environment variables documented in
[save-state.md](save-state.md#save-during-inventory-pickup-and-putdown).
Validation passes: 248 enabled workspace tests, both opt-in real-asset inventory
tests, all-workspace Clippy, native/WASM release builds, and all six Chrome/
WebKit player tests plus two browser clock tests. The visible native credits
session was refreshed with the same paused/muted checkpoint and unchanged save.

## Header dragging and saves

Left-button drag start in the original header area records the active inventory
owner. Each cursor delta translates the existing frame sprites, backdrop, title
glyphs, icon animations, and all hit regions, without rebuilding their animation
state. Left release or a double-click action ends the drag. Closing, hiding,
replacing a window or changing scenes clears its ownership. This applies to
ordinary and conversation windows; original resizing, slider dragging,
maximize/unmaximize and hover-cursor changes still remain.

Active drags use core save version 5. Versions 1–4 retain their existing layouts,
and a completed drag returns to version 4. Invalid owners, absent mouse history,
hidden/missing windows and mismatched active slots are rejected before loading.
Coordinate translation checks every addition and commits only after all succeed;
unrepresentable input cancels the drag without partially moving the window.

The real-asset regression drags the luggage header by (30,20), checks its window
position and shifted icon cell, then moves back before a tick and compares every
RGBA pixel. It saves an active drag, reloads it exactly, moves again and releases,
then compares 24-tick continuations including complete engine/DSP state, pixels
and audio. Closing a restored drag clears it. Native restores host button
ownership for a saved drag so its release is retained. The browser forwards
held touch motion and renders pointer updates immediately; releases can end
a restored drag even without a preexisting browser pointer ID.

Native acceptance dragged luggage from [100,100] to [130,120] using macOS HID
motion/down/drag events. The active checkpoint at tick 33,503 is core version 5;
F9 while paused followed by physical release produced version 4 at the same
tick and position. Independent headless loads verified both files and the
translated geometry. The first synthetic attempt omitted a preceding mouse
motion event and closed the window; only the subsequent normal motion/down/drag
sequence counts as native drag evidence. Scratch files are
`assets/saves/sdl-header-drag-{active,retry}.dws`.

Chrome and WebKit tests exercise actual mouse dragging, exact active-drag
import/export, unchanged pixels on reload, and release while paused. Chrome
also uses CDP touch start/move/end; its exported save loaded in the native engine
with luggage position [130,120] and no held item. This is browser touch coverage,
not physical iPad acceptance. The core tests, three opt-in real-asset checks,
Clippy, native/WASM builds and the eight browser-player checks pass. Resizing,
slider dragging, window maximization and broader conversation-drag acceptance
are still required.

### Inventory cell gaps

Pinned `dialogs.cpp::invWalkTo` first dispatches on `invArea`, then resolves an
icon in the body. While holding an ordinary inventory item, a single click in
the one-pixel gap between cells tries one pixel right, then one pixel down,
then one pixel down-right. Empty-hand clicks, double-click actions and looks
use only the exact cell. Physical input now follows this rule; header, border
and scrollbar regions cannot be treated as icon targets by this fallback.
The immediate semantic replay API retains its explicit-cell behavior.

The opt-in `held_inventory_item_drops_through_one_pixel_cell_gaps` regression
uses the original tick-33200 inventory checkpoint and actual mouse edges. It
checks vertical, horizontal and intersecting gaps, empty-hand and look
non-actions, the delayed held-item release, and the resulting item order. Both
queued and suspended drop saves resume with identical engine/DSP bytes, RGBA
and audio over the following ticks. A core regression also checks that window
regions take priority even when icon coordinates overlap them.
