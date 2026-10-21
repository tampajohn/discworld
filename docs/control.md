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

Remaining integration includes inventory resizing,
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
of completed original mouse support: inventory resizing, menus,
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
Pending ordinary drops require core save version 3. Inventory resizing still needs integration before claiming original input parity.

## Physical input delivery and persistence

Native drains SDL2 left/right button events in order and browser sends pointer down/up/cancel
through its serialized worker. The default canvas action uses physical left
clicks (single selects/walks, double acts); right click looks. Explicit action
and look controls remain available for touch. Browser pointer capture ensures
release outside the canvas reaches the worker. Releases can update button
history while paused, without advancing simulation. Inventory title-bar dragging now moves the original frame, title and icons;
resizing remains unfinished.

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
by `invDragStart`; header movement and thumb dragging are implemented; border resizing remains unfinished.
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
ordinary and conversation windows; original resizing and hover-cursor changes still remain.

Active header drags use core save version 5 unless stored maximization geometry
requires version 7. Versions 1–4 retain their existing layouts, and a completed
drag without stored geometry returns to version 4. Invalid owners, absent mouse history,
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
and broader conversation-drag acceptance are still required.

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

### Inventory scrollbar dragging

Pinned `dialogs.cpp::adjustTop`, `nearestSlideY` and `slideSlider` define the
ordinary inventory drag. The row-position table includes the original
non-row-aligned first-display offset; nearest-position ties choose the first
entry. Cursor movement accumulates before clamping, so overshoot must be undone
before the thumb moves away from an endpoint. Horizontal motion has no effect.

During a drag, crossing a row threshold changes `first_display` and refreshes
icons on the next engine tick. The frame and continuous thumb position survive
that refresh; `adjustTop` is deferred until release, as in `fillInInventory`.
Release schedules the normal refresh and snaps the thumb to the selected row.
The state is shared by native mouse and browser mouse/touch input; active slider
saves use core version 6, or version 7 when maximization geometry is present. Configuration-menu sliders and border resizing are
still separate unfinished controls.

The real-asset slider fixture is the existing recorded inventory route stopped
at tick 434,050, immediately after opening luggage in `alley.scn`. It contains
12 items in a 3×2 window at [100,100], with thumb [187,124] and range 124–162.
To reproduce it, truncate `evidence/inventory-scroll-pickup-input.json` to events
with `tick < 434050`, then run `discworld-tools run-headless` to tick 434050 using
that trace and `--load evidence/save-relaunch-inventory/checkpoint.dws`, saving
with `--save` to a fresh path. The startup press argument is irrelevant when
resuming this tick-33200 save. Set `DISCWORLD_TEST_SLIDER_SAVE` to the result for
the opt-in Rust check and browser tests (their local default is
`assets/saves/slider-checkpoint.dws`). Asset bytes remain local.

The Rust regression checks mid-drag row changes, unchanged window position,
ignored horizontal motion, retained overshoot after loading, release/close,
and exact engine/DSP/frame/audio continuations. Unit cases cover non-row-aligned
pages, tied/duplicate row positions, extreme movement and malformed drag tables.

Chrome and WebKit mouse acceptance saves an active drag as version 6, reloads
it byte-for-byte with identical pixels, and releases it after reload to version
4. Independent native loads of both browser saves confirmed first-display 3,
unchanged item membership and no held item. Screenshots are
`assets/previews/browser-{chrome,webkit}-slider-active.png`; the complete suite
passes ten player checks plus two clock checks. This is desktop browser input
evidence, not physical iPad acceptance. All 253 enabled workspace tests, five
opt-in real-asset inventory tests, Clippy and native/WASM release builds pass.

Native HID acceptance moved the original thumb and changed the visible row.
At tick 434,103, `native-slider-active-retry.dws` is version 6 and independently
loads/advances in the native headless host. F9 followed by a normal mouse
press/release while paused produces version 4 at the same tick in
`native-slider-completed-retry.dws`. The down is ignored while paused; the up
ends the restored drag. Earlier synthetic attempts lost window focus (ending
the drag), and a lone synthetic up after OS button-state loss was not delivered.
Those attempts are not counted as active-drag acceptance; no engine change was
needed for them. Visible gameplay was refreshed separately with its room save
and mute state preserved.

### Title-bar maximization and restored positions

Pinned `dialogs.cpp::invAction` expands an inventory on a title-bar double click
only when both dimensions are resizable. It saves the normal dimensions, uses
the declared maximum, and swaps current/alternate positions. The alternate
position starts at [21,15]. Restoring swaps positions back; dragging the expanded
window therefore preserves its location for the next maximization. Fixed-size
pockets do not maximize. Original frame reels, fonts, title centering and icon
scripts are reconstructed immediately, without waiting for a simulation tick.

The current maximized flag is copied into the per-window flag on close and
restored on open, matching `killInventory` and inventory creation. Scene teardown
also retains it. `SETINVSIZE` clears the stored flag as in the reference, without
altering the active flag until the normal lifecycle does so. Full construction
on opening or toggling applies `rePosition`: background left <=315, right >=3,
and top between -13 and 195, retaining a reachable part of the header. Ordinary
icon refreshes and header movement do not clamp the window.

Core save version 7 persists both positions, normal dimensions, per-window flags
and the active flag, together with any current header/scrollbar drag. Restoring
the normal size still uses version 7 because the alternate expanded position
remains meaningful. Versions 1–6 retain their previous encodings when no such
geometry has been recorded.

The real-asset regression exercises 3×2 to 10×6 expansion, exact save/pixel
roundtrip, a saved active drag of the maximized window, independent normal and
expanded positions, close/save/load/reopen, and restoration. It also drags a
window off-screen and checks that reopening restores the original reachable
header limits. Item membership and held state remain unchanged. Unit tests
cover dimension eligibility, position swapping, malformed saved dimensions,
and exact reposition boundary pixels. Border resizing, configuration menus
and hover-cursor fidelity still need work.

Native HID double-click acceptance saved an expanded window at tick 33,222 in
`assets/saves/native-maximized-preserved.dws`; F9 and another double click saved
the restored window at 33,248 in `native-maximized-restored.dws`. Both are version
7. Chrome and WebKit independently exercised actual double-click input, exact
maximized import/export and identical pixels after loading. Native headless
loads of all six saves confirmed 10×6 at [21,15] and 3×2 at [100,100], with no
held item. Screenshots include `assets/previews/native-inventory-maximized.png`
and `browser-{chrome,webkit}-maximized.png` (plus restored variants).

All 256 enabled workspace tests, six opt-in original-asset inventory checks,
Clippy, native/WASM builds, twelve browser-player checks and two clock checks
pass. The slider regression additionally saves and resumes a scrollbar drag
after maximization history exists, verifying the combined version-7 payload.
Physical iPad input and full original menu/resize behavior remain unverified.

### Border and corner resize integration

Ordinary inventories now accept all eight edge/corner drags when both declared
size ranges vary. The implementation follows the pinned V1 `invDragStart`,
`xMovement`/`yMovement`, `changeingSize`, `constructInventory(EMPTY/FULL)` and
`invDragEnd`: X movement precedes Y, limit overshoot is retained, top/left
movement compensates the window origin, and partial cells round up only above
12 pixels on release. Release rebuilds the full original frame and icons and
clears active maximization, retaining the resized window as the normal size.

During the drag, original resize corner reels 21/22/17/23 and filler reels form
an outline at z=22 over the existing inventory. The outline has no additional
backdrop or title. The normal backdrop, title and animated contents remain
visible. The first motion creates the outline. Extreme coordinates cancel the
gesture atomically and restore its initial size and position.

The real-asset regression covers every edge/corner at the 12/13-pixel threshold,
exact mid-drag save/pixel restoration, identical engine/DSP/audio continuation,
release after restoring, unchanged item membership and held state, and resizing
an expanded window followed by maximizing/restoring its new normal dimensions.
The default workspace suite and Clippy pass. Browser and native gesture
acceptance are recorded separately below when complete. Closing or changing
inventory contents during a held resize, hover-cursor fidelity, original menus,
and physical iPad gesture acceptance still require further reference checks.

Chrome and WebKit both pass the actual pointer corner-drag test: save while held
(version 8), import/export exact bytes and pixels, release while paused after
reload, and resume. All 14 player checks plus the two clock checks pass. Saves
are `assets/saves/browser-{chrome,webkit}-resize-{active,released}.dws`; screenshots
are `assets/previews/browser-{chrome,webkit}-resize-active.png`. Independent
native loads verify the released window is 4×3 at [100,100], with no held item.

Native HID acceptance at tick 33,215 uses
`assets/saves/native-resize-active-retry.dws` (version 8),
`native-resize-released.dws` and `native-resize-reloaded-released.dws` (version 4).
Both release paths produce 4×3 at [100,100]; the second restores the active save
with F9 first. Independent headless loads confirm the geometry and empty hand.
The first synthetic attempt (`native-resize-active.dws`) received unintended
leftward motion from the physical cursor position. The retry explicitly warped
the cursor before each synthetic event and passed; the first attempt is retained
as diagnostic evidence, not counted as successful acceptance.

Validation totals: 258 enabled workspace tests; seven opt-in real-asset tests;
Clippy with warnings denied; release native/tools and WASM builds. A subsequent
bounds-validation tightening rejects malformed resize minimum/maximum values;
the real-asset resize/maximization regression and Clippy were rerun afterward.

The final focused browser rerun exposed a test readiness race in WebKit: the
script sent pointer-down while Play was still awaiting `AudioContext.resume()`.
The player correctly ignored input while paused. The resize test now waits for
the button to report `Pause` before beginning the gesture, rather than assuming
that completion of the DOM click means asynchronous startup has finished.
The corrected focused test passes in both Chrome and WebKit on the final WASM
build. Visible native gameplay was refreshed from tick 85,613, running and muted,
with its state preserved in `assets/saves/playable-inventory-maximized.dws` and
future saves directed to `playable-inventory-resize.dws`. The inspected screenshot
is `assets/previews/inventory-resize-current-native.png`.

### Item redraws during an active resize

V1 `redraw` calls `fillInInventory` when items change: it replaces only icon
objects and adjusts an existing scrollbar thumb. It does not reconstruct the
full frame, title or resize outline. Icons use the current declaration's origin
and cell counts even while the old full frame remains visible. Likewise,
`invItem` uses declaration geometry rather than the old frame's icon positions.

The Rust engine now preserves that distinction. Scrolling or another item
change during resizing redraws icons at the current origin; an existing thumb
moves within its original range. The title and full frame remain in place.
Ordinary inventory hit-testing and cell-center queries use the current declared
grid, with checked coordinates and the original one-pixel gaps. No new save
version is needed: version 8 already retains both the old display and active
resize state.

The original-asset regression loads the scrollable Luggage at tick 434,050,
drags its left edge from x=100 to87, and scrolls down while held. It compares
header/title pixels before and after the redraw, verifies cell0 at105,132,
first-display3, and thumb y143 in the original124..162 range. Mid-drag save/load
preserves identical continuation; release produces4×2 at87,100. All eight
opt-in real-asset tests pass. Closing a held resize, menu transitions and hover
cursor behavior remain separate reference checks.

Chrome and WebKit exercise the same held left-edge drag followed by ArrowDown,
then exact active save import/export, release after reload, and continued play.
The test targets logical pixel centers so CSS coordinate truncation does not
turn a boundary click into a click outside the window. Both pass on the final
WASM build. Independent native loads of
`assets/saves/browser-{chrome,webkit}-resize-redraw-{active,released}.dws`
confirm 3×2 while held and 4×2 after release, both at [87,100] with
first-display3 and no held item. The screenshot
`assets/previews/browser-webkit-resize-redraw-active.png` shows the old title/frame
and the relocated icons beneath the outline.

Native acceptance saved the active resize/scroll at tick434,070 in
`assets/saves/native-resize-scroll-active.dws` (version8). F9 followed by a
focused physical press/release while paused and F5 produced
`native-resize-scroll-released-retry.dws` (version4). An independent native load
confirms4×2 at[87,100], first-display3 and no held item. The earlier
`native-resize-scroll-released.dws` remained an active version8 snapshot and is
retained as failed automation evidence; its screenshot was insufficient to
establish release. Final workspace tests, Clippy, native/tools builds and WASM
build pass.

### Original inventory edge cursors

The pinned V1 `invCursor` selects window-parts reel24 for horizontal resizing,
25 for vertical resizing,26 for the rising diagonal, and27 for the falling
diagonal. Titles, bodies and scroll controls keep the normal cursor. Borders of
fixed-size windows also keep the normal cursor. Any inventory drag freezes the
current shape until release.

The engine switches the existing cursor object's animation script rather than
creating another cursor. Temporary scripts run at period2; returning to normal
uses the declared cursor film's rate and steps immediately. Flip flags reset on
script changes, matching `InitStepAnimScript`. The current script identifies a
temporary cursor, so its existing animation serialization preserves hover and
drag shapes without another save version. The host-independent cursor update
runs after cursor animation in the simulation tick, matching the reference's
AnimateProcess order. Exact event-boundary timing (including close before the
next tick), cursor trails, and hover-label/POINTED arbitration still need work.

The real-asset regression compares the complete rendered image against the
original cursor reels for all eight borders/corners, then checks frozen shape
while dragging, restoration after release and close, and exact saved
engine/DSP/audio continuation. Normal title/body behavior and fixed pockets use
the declared cursor artwork. Workspace tests and all nine opt-in asset tests
pass, along with Clippy and release native/tools/WASM builds.

Native HID saves `assets/saves/native-hover-arrow.dws`,
`native-hover-corner.dws` and `native-hover-normal.dws` were independently loaded
by the asset regression and matched pixel-for-pixel against original reels24,
27 and the declared normal cursor film. Fixed pockets also retain the normal
cursor; its comparison advances the expected original animation28 ticks after
restoration, since the normal cross itself is animated. Comparing only its first
frame at that point incorrectly failed the initial test.

All18 Chrome/WebKit player checks and both clock checks pass. The new hover
check exports and reloads both arrow and normal cursor saves with identical
bytes and pixels. Evidence uses `assets/saves/browser-{chrome,webkit}-hover-{arrow,normal}.dws`
and matching screenshots under `assets/previews/`.
The same asset regression independently loads both browsers' exported hover
saves and matches their pixels against the original reels, then checks exact
continuation. Both browser/native cross-load checks and final Clippy pass.


Inventory hover-label lifecycle foundation (not yet dispatched): the pinned
`dialogs.cpp::ObjectProcess` increments its shared POINTED serial only after
interpretation completes, sleeps, then tests departure before serial mismatch.
Departure runs UNPOINT even for an older waiter. `tinlib.cpp::PrintObjPointed`
checks held-object identity before suppression; while suppressed it waits
without checking pointer or held state. On release it compares the original
absolute cell, redisplays at the original position, sleeps, and only then
compares object identity. POINTED clears an unmatched OFF. The new
`inventory_hover` state machine covers these distinctions, including serialized
continuation of suppression and waiter state. It is not yet connected to engine
processes, rendering or the save envelope; POINTED PRINTOBJ and ON/OFF remain
unsupported until that integration is complete.

A prerequisite runtime correction is connected: PRINTOBJ now uses V1 invItem's
current declared grid and text anchor, including empty cells and conversation
windows, rather than indexing the previous rendered frame. During resize those
geometries can differ. The lookup returns absolute FirstDisp-based cells, skips
one-pixel gaps, checks arithmetic, and intentionally has no visibility guard,
matching invItem rather than invItemId. Regression tests cover geometry,
empty/hidden cells, gaps, bounds, and the actual PRINTOBJ first-tick yield before
speech takes control. All265 enabled workspace tests and nine original-asset
tests pass; workspace Clippy and release native/tools builds pass. Logs:
`assets/logs/hover-label-foundation-{tests,assets,clippy,build}.log`.

The rebuilt WASM also passes all18 Chrome/WebKit player checks and both clock
checks (`assets/logs/hover-label-foundation-browser.log`). Native bcc2ada was
reloaded from preserved current progress at tick122120, running/muted, with
actual colored pixels inspected in
`assets/previews/inventory-printobj-current-native.png`. Current native output
save is `assets/saves/playable-inventory-printobj.dws`; the previous current
state remains `assets/saves/playable-inventory-hover.dws`. These checks validate
this increment, not completed hover-label integration or a new full-route run.


POINTED/UNPOINT integration now dispatches the original inventory-object scripts
when the pointer enters a new item. Labels use the original tag font regardless
of subtitle preference, retain their original anchor while suppressed, yield to
non-POINTED PRINTOBJ, and execute UNPOINT after the original completion/departure
sequence. OFF/ON (-1/-2) update the shared arbitration flag. A held item is not a
new hover target. Window gestures freeze discovery. Cancelled scene/process
waiters are removed; suspended label resources are checked during save preview.
Hover discovery currently runs at the cursor-update tick boundary; complete
reference equivalence for every immediate event/redraw boundary remains open.

Version9 saves retain the shared monitor, labels, layouts, visibility and
post-POINTED waiter serials alongside all previous optional input/window state.
Versions1–8 still decode and are still emitted when hover state is default.
Structural validation rejects orphan/duplicate owners, mismatched pending calls,
conflicting host waits, invalid visibility/cell/serial values, and UNPOINT
waiters whose POINTED interpreter has not halted. The original-asset banana test
compares actual pixels with the archive's label string and font, checks subtitle
independence, save/DSP continuation, speech suppression and redisplay. The
scheduler regression verifies a fresh EVENT8 after leaving a completed EVENT0,
including save continuation and malformed waiter rejection.

User-reported right-click stop, tick133103 in unseen.scn: preserved original
state at `assets/saves/rightclick-node-exit-stopped.dws`. The failure was
`unsupported preserved node exit leg`, a deliberately unimplemented branch in
Mover::begin_node_walk. Pinned move.cpp::SetOffWithinNodePath explicitly keeps
its immediate target when already LEAVING through the selected nearest end;
SetActorDest replaces ultimate/intermediate targets and resets overshoot.
The runtime now retains that existing leg, then routes toward the new ultimate
destination normally. An exhausted leg is represented by the current position,
so the next stationary step routes without moving. Regression retargets an
active exit, checks leg/serial/overshoot, serializes the mover, then reaches the
destination identically. The exact user save advances to133800 Running with
control enabled: Rincewind[452,553], Luggage[452,573], both walks complete.
Evidence: `assets/logs/rightclick-node-exit-{before,fixed}.json`,
`assets/previews/rightclick-node-exit-fixed.png`. The opt-in original-asset
regression requires DISCWORLD_TEST_NODE_EXIT_SAVE and checks exact
engine/pixels/audio continuation across the formerly failing tick.


Full-route regression caught and corrected during hover integration:83d1928
stayed in Arch at tick612000 (ingredient count4), while an isolated cc20493
pre-hover build entered Detector (count5). New POINTED discovery on the tick
after an inventory click overwrote global glitterIndex with the object ID before
ADDOPENINV consumed the click's cell index, changing inventory order. Discovery
now also runs before raw mouse-button and resolved inventory dispatch, preserving
pointer-before-click ordering. A regression executes a LOOK script's ADDOPENINV
and checks insertion at the clicked cell even when movement/click share a frame.
The unchanged route prefix0–612000 now matches the isolated baseline for scene,
all globals, inventory contents, held item and control. Evidence:
`assets/logs/{before-hover,hover-order}-arch-diagnostic.json`; the failed hover
state is retained as `assets/logs/hover-arch-diagnostic.json`. This establishes
the cause and prefix recovery; the remaining recorded route is running from
`assets/saves/hover-order-arch-diagnostic.dws`, not yet claimed complete.

Verification for the hover/cutaway increment:266 enabled workspace tests,
11 original-asset tests and Clippy pass. All18 Chrome/WebKit player checks and
two clock tests passed on83d1928; both browser-exported POINTED saves were loaded
by the original-font pixel and speech-arbitration regression and passed. Final
pointer-before-click build has its own browser run and route continuation pending.
Native right-click recovery was visually verified in library.scn after loading
exact stopped133103 progress; subsequent refresh preserved current library state
and the user's sound-on preference. Native hover scratch input was deferred
because the user was actively playing; browser and shared-engine pixel checks
provide label evidence without interfering with that session.


8702043 verification completed: all18 Chrome/WebKit player tests and two clock
checks pass (`assets/logs/hover-order-browser.log`), along with final Clippy.
Both browsers' new label exports pass the original-font pixel, suppression,
redisplay and save/DSP continuation test when loaded natively
(`assets/logs/{chrome,webkit}-pointed-order-crossload.log`). WebKit's actual
Banana label screenshot was inspected. The complete unchanged1652-input route
passes in two continuous segments:283 inputs from0 to612000, followed by1369
from that exact save to2630000. Every input reports Ok(true), final scene is
Title, status Running, has_restarted=true, and the final title image was
inspected. Reports are `assets/logs/hover-order-arch-diagnostic.json` and
`assets/logs/hover-order-full-ending.json`; final save/image are
`assets/saves/hover-order-full-ending.dws` and
`assets/previews/hover-order-full-ending.png`. This revalidates the recorded
native route after hover/node changes, not a complete physical-input route,
new full-browser route or physical iPad acceptance.

The user's previous native window closed normally at139628; its last saved
checkpoint139277 was reopened on8702043 as PID16294 with output
`assets/saves/playable-inventory-pointed-order.dws`. The final351 unsaved ticks
were unavailable and were not claimed preserved. Colored library/pockets
pixels were inspected in `assets/previews/pointed-order-current-native.png`;
subsequent user mute selection was left unchanged.


Original configuration menus remain an implementation gap. Source mapping for
next work (pinned ScummVM 96b42d42, engines/tinsel): events.cpp::ProcessKeyEvent
allows PLR_MENU during startup when ENABLEMENU is set, turning control on;
otherwise normal control gating applies. dialogs.cpp::openMenu rejects opening
while another inventory is active in V1 and uses a nonmovable, nonresizable
INV_CONF window. The English V1 main menu is 6x5 at (72,23), with eight original
text buttons (Load, Save, Restart, Sound, Controls, Subtitles, Quit, Resume),
relative x15/y20 plus 15 per row, size145x13. Text comes from the twenty handles
registered by library call101, already captured as Startup.config_strings,
and uses the original tag font (dialogs.cpp::addBox), not host UI fonts.
Sound, controls and subtitles have separate V1 tables; do not substitute V2
system-string tables or Japanese geometry. Restart confirmation is 4x2 at
(98,53); its original tick/cross reels have relative positions (70,28)/(30,28).
Before implementation, trace configuration click/close/reopen behavior, frame
and highlight rendering, slider settings and save-slot host integration. This
mapping does not claim that F1 menus or original save slots currently work.


Full browser replay revalidated runtime8702043 after the node-exit and inventory
hover ordering fixes. Chrome156.0.8078.47 (504.338s) and Playwright WebKit27.2
(534.320s) each accepted all1652 unchanged inputs from startup through credits,
then restarted to title.scn at2630000. Both complete serialized engine states
and final RGBA match the corrected native route exactly. Both browser title
screenshots were inspected. Each generated4,832,625,000 finite audio samples;
combined DSP saves differ, so cross-platform audio equality is not established.
Evidence: evidence/{chrome,webkit}-hover-full-ending/{report,native-comparison}.json
and browser-final.png. The native comparison report combines the measured283
prefix and1369 continuation inputs in assets/logs/hover-order-native-comparison.json,
with explicit source/checkpoint provenance. Replay uses the separate
replay-validation WASM build and accelerated semantic inputs; it does not prove
a full physical-input route, Web Audio playback or physical iPad acceptance.
No additional engine changes were needed for these full browser checks.


Original main-menu presentation is now implemented in configuration_menu::MainMenu:
original configuration strings0..7, tag-font layouts, configuration frame,
background-palette rectangles228/231 and half-open button hit regions with
inactive two-pixel row gaps. Opaque rectangle writes retain palette indices,
including index0, so a later translucent overlay maps the actual destination.
Tests cover clipping/extreme bounds, atomic invalid dimensions, opaque black,
subsequent translucency and hit-region boundaries. No engine/controller field
or save layout changed; F1 activation, submenus and dispatch remain unfinished.

`discworld-tools preview-menu ISO FRESH_OUTPUT.png` advances startup with the
original sample bank, MIDI tracks and strings, then renders the main menu with
Load highlighted. The actual asset preview was inspected:
`assets/previews/original-main-menu-content.png`, with string report
`assets/logs/original-main-menu-content.json`. An initial preview incorrectly
used tick() without media/text and ignored Unsupported, leaving the placeholder
startup palette (green buttons); the tool now supplies dependencies and rejects
Unsupported. This was a preview harness problem, not evidence for changing the
reserved blue palette. No independently captured reference image comparison is
claimed. All267 enabled workspace tests pass; final Clippy and tool tests pass.
The live player's behavior is unchanged by this presentation-only increment.


Confirmation presentation now supports the original Restart/Quit headings
(config strings10/19), 4x2 configuration frame at98,53, and tick/cross reels
46..48/40..42 at168,81/128,81. Graphic hit regions exclude all four edges;
text-button regions remain half-open. ButtonPress models the inventory-process
coroutine: install normal, sleep1, depressed for2, normal for1, then dispatch
once; a second press is rejected. Full reset discards the pending action,
but menu closure does not (corrected by the lifecycle audit below).
The component is not yet attached to engine scheduling, so this verifies its
relative steps, not exact physical-input scheduling or live restart behavior.

Asset previews were inspected: assets/previews/original-{restart,quit}-menu.png
(highlighted accept), original-restart-menu-normal-verified.png and
original-restart-menu-down-verified.png. All three button images differ.
Commands: discworld-tools preview-restart-menu ISO FRESH_PNG [STATE], and
preview-quit-menu with the same arguments; STATE0(default)=highlight,
1=normal,2=depressed. Initial normal/down preview attempts accidentally omitted
the effect argument and produced identical images; corrected verified files
exercise the real state selection. Original menu activation/actions remain
unconnected. 269 enabled workspace tests pass, with final clean Clippy in
assets/logs/confirmation-menu-clippy-corrected.log.

Controller source findings for the next increment: dialogs.cpp::eventToInventory
routes right-click through invWalkTo for menus, and handles provisional left
click immediately. PLR_ESCAPE from a non-main submenu sets _reOpenMenu, so
killInventory reopens Main and writes configuration. Clicking a cancel graphic
instead calls CLOSEWIN/killInventory without setting _reOpenMenu, closing the
menu entirely. Do not make these two cancellation paths equivalent. AATBUT
text actions dispatch on the next InventoryProcess pass without ButtonPress;
AAGBUT/ARSGBUT wait for ButtonPress first. Closing the menu removes HL1 but retains the queued action; see the lifecycle
audit below. Host menu wiring and save-slot/settings integration remain
required; current live player behavior and save format are unchanged.


V1 navigation component (menu_navigation::Navigation) now models idle-inventory
exclusion, main-menu destinations, delayed text-button dispatch, graphic-button
requests, and Escape versus explicit close. Escape from any ordinary submenu
reopens Main and requests a configuration write; CLOSEWIN/outside-click closes
entirely. Restart closes before emitting a host request; Quit leaves the window
until the host handles the request. This is a controller component, not yet
Engine/F1 integration: settings widgets, save-slot handlers, actual event/control
counters, cursor relocation, rendering lifecycle and snapshot extension remain.

Important correction to the previous cancellation assumption: pinned
 dialogs.cpp::killInventory does not clear _buttonEffect. ButtonPress checks
HL1 after the first sleep and the two-tick depressed sleep, returning early
when the overlay disappeared. InventoryProcess still invokes callFunction
unconditionally afterward. Therefore pending text/confirmation functions survive
closing, and confirmation dispatch uses its original page, even if Escape opened
Main meanwhile. ButtonPress::step now takes actual overlay existence; navigation
retains the queued origin. Tests cover closure before text dispatch, Escape
while pressing Restart, global busy-button exclusion, every overlay-removal sleep
phase, cancel versus Escape, and one-shot request dispatch. Only full component
reset drops a pending effect. This source-derived edge behavior has not been
independently exercised in a reference binary; do not substitute an assumed
cancel-on-close policy during engine integration.

Final checks:273 enabled workspace tests pass and Clippy is clean in
assets/logs/menu-navigation-{workspace,clippy}-final.log. Early tests caught a
missing queued-origin assignment; the final tests require a Restart request
after returning to Main and pass. No live engine, save-format or host behavior
changed in this increment.


SoundMenu now constructs the English V1 10x5 configuration frame at20,16,
with groove reel30 and thumb reel34, three rows at162,41/81/121, and original
config strings11..13 right-aligned at158,rowY+6 using the tag font. Font now
supports reference TXT_RIGHT justification; centered text retains its existing
calculation. Thumb hit testing uses actual multipart bounds and strict edges.
Slider values are0..255, travel81pixels with minimum offset8. Drag arithmetic
retains raw overshoot, changes values only when the visible thumb moves, and
reconstructs thumb position on release (floor conversion can shift it back a
pixel). Checked overflow fails without changing the drag. The shared slider
model also accepts the other nonzero setting ranges used by later menus.

Pinned dialogs.cpp::whichMenuBox has V1 MD_YBUTTOP=9 and MD_YBUTBOT=0, making
its arrow hit interval empty; no invented arrow-click behavior was added.
SoundMenu emits changed channel/value pairs; live MIDI volume, effects/voice
gain, settings persistence and engine menu attachment are still required.
The source keeps MUSICVOL immediate and other channels as mixer settings.

Asset-backed drag audit: preview-sound-menu ISO FRESH_PNG [STATE] constructs
values[255,128,0], grabs effects thumb at218,93, applies deltas0,1,100,-60,-1,
checks changes129/255/(no change)/251, releases, and rejects movement afterward.
STATE0 draws the initial preview; STATE1 draws the audited released state.
Inspected assets/previews/original-sound-menu.png and
original-sound-menu-drag.png; detailed audit is
assets/logs/original-sound-menu-drag.json. These are component previews, not a
live host input/audio test or independent reference screenshot comparison.
275 enabled workspace tests and final Clippy pass, recorded in
assets/logs/sound-menu-workspace.log and sound-menu-clippy-final.log.
Controls/subtitles widgets and save/load slots remain alongside host integration.


Controls/Subtitles components now render the English CD V1 tables using the
original configuration strings14..18, tag font, circle reels38/39, and all six
cross/tick button states. Controls uses10x5 at20,16; Subtitles uses10x3 there.
Pinned detection_tables.h English CD v2 has GF_SCNFILES and enhanced audio,
without GF_USE_3/4/5FLAGS, so the compact no-language-flags table applies.
Double-click-speed range is0..30 (3*DOUBLE_CLICK_TIME10); text speed is0..100.
The double-click test circle flips only on a double action. Constructors accept
its existing value so the controller can preserve the original global bFlipped.
Both sliders use the shared retained-overshoot arithmetic and actual thumb bounds.

ButtonToggle is separate from ButtonPress: remove highlight, old depressed,
flip setting/new depressed, remove, new highlighted, each separated by one
inventory step. After waits it checks inventory activity at phases1/4 and HL1
existence at2/3. Loss before the flip aborts without changing the setting; loss
afterward preserves the change. The tests exercise each abort point. Hover owns
the same overlay and can replace/remove it during the effect, as invBoxes does.
No per-channel mixer, live mouse swap/timing, subtitle text speed or host settings
persistence is wired by these components yet.

`preview-controls-menu` and `preview-subtitles-menu` take ISO and fresh PNG path.
They assert busy-toggle rejection, the exact [none,none,true,none,none] change
sequence, actual thumb pickup/one-pixel drag/release, and (Controls) rejection
of single-click circle flipping followed by successful double-click flipping.
Inspected original-controls-menu.png and original-subtitles-menu.png under
assets/previews; full audits and updated renders are original-*-menu-verified
under assets/logs and assets/previews respectively. Audits end with slider10
(Controls) and1(Subtitles), both enabled, and no retained drag. These are original
asset component checks, not physical host input or an independent reference run.
276 enabled workspace tests pass, final Clippy is clean in
assets/logs/settings-menu-clippy-final.log. Save/load widgets, engine lifecycle,
snapshot state and native/browser menu wiring remain required.


save_menu::List now models the V1 nine-row,100-entry save/load catalog with
stable host IDs. The host supplies newest-first validated entries (reference
saveload.cpp::getList sorts by header timestamp). Save page0 has a blank new
row only when count<100; other pages have no modifier. Blank save rows return
Target::New, blank load rows return no target. Row/page/Home/End navigation uses
capacity91 as the final first index, including empty catalog tails. Wheel
scrolling adjusts selection by one row; page movement selects row8/0. Text
rows use half-open bounds48..214,47+row*15..+13 with two-pixel inactive gaps.
Selecting another row or scrolling reloads the draft; selecting the same row
does not erase current edits.

Name editing uses original-font bytes,40-byte maximum, underscore cursor,
left-aligned layout at50,47+row*15, and rejects unsupported glyphs. Space is
accepted after existing content (or if the font defines a space image).
InvKeyIn's inclusive right boundary213 is measured including the cursor;
when exceeded, one trailing byte is removed, including the reference's unusual
second deletion if backspacing an already oversized imported name. Host Unicode
mapping remains pending. Catalog/header decoding must mirror getList's
strlcpy(desc,40) truncation to39 bytes separately from the40-byte editor limit;
this component accepts already-decoded catalog descriptions. It performs no I/O
and does not claim ScummVM save-format compatibility.

Tests cover modifier disappearance, full capacity, stable target IDs, blank load
rows, paging/click gaps, duplicate IDs, width versus byte limits, glyph filtering,
backspace and draft replacement. Font left justification shares the existing
checked layout using a private alignment enum; centered/right behavior tests pass.
279 enabled workspace tests passed in assets/logs/save-menu-list-workspace.log;
expanded editor and text tests passed afterward, with final Clippy in
assets/logs/save-menu-list-clippy-typed.log. Save-menu frame/scrollbar rendering,
scroll-thumb interaction, confirmation/Return dispatch, catalog persistence,
snapshot integration and live menu activation remain unfinished.


Save/load View now draws the original10x6 configuration frame, heading strings8/9,
extra list-window parts from addExtraWindow, palette-colored row selection,
original-font descriptions/edit cursor and three-state accept/cancel graphics.
The extra frame uses reels21/49/17/18, horizontal11 and vertical4; slider reel0
is positioned at223,49+first*125/91 after addBoxes(true). Mode mismatches are
rejected before drawing. Save selected-row text is replaced by the live draft;
Load retains the catalog description. The component accepts button-effect state
separately from hover, as with the confirmation menus.

The save-list slider now models slideCSlider independently of mixing-desk sliders:
raw drag overshoot is retained, logical Y is clamped49..174, first index is
floor((Y-49)*91/125), selected row changes by oldFirst-newFirst clamped0..8.
V1 changes the rendered thumb only when first index changes; release does not
snap/rebuild it. Keyboard/wheel scrolling rebuilds from first index. The drag
regression distinguishes logical Y50/rendered49, then first1/rendered51, retains
that through release, verifies overshoot at the bottom and checks scrollbar hit
regions. whichMenuBox's absolute half-open scroll region is221..229,42..184;
arrow/track/thumb precedence and logical (not last drawn) Y determine the hit.

Original-asset previews inspected: assets/previews/original-save-menu.png
(12 synthetic catalog entries, blank new-save row) and original-load-menu-end.png
(100 synthetic entries, last page91..99). These are synthetic save-name fixtures,
not stored progress. Commands preview-save-menu/preview-load-menu take ISO,
fresh PNG, optional state0=first page with12 entries,1=page down with100,
2=end with100. They prove original artwork/font composition, not storage I/O,
manual host controls or an independent reference image match. All280 enabled
workspace tests pass; final save-menu tests and Clippy pass in
assets/logs/save-menu-view-{tests,clippy}-final.log. Catalog persistence, slot
metadata/snapshot writes, action/Return dispatch, engine snapshot state and
native/browser F1 integration remain required. Live game behavior is unchanged.


Portable slot record and native persistence are now implemented. audio::slot
wraps an unchanged DWRPLAY engine/DSP session with DWRSLOT version1 metadata:
ID0..99, host timestamp in milliseconds, up to40 original-font description
bytes (no NUL), payload lengths and SHA256. Metadata and snapshot are one record,
so there is no separate manifest that can refer to a different generation.
The codec bounds lengths, validates outer and nested session envelopes and
borrows the decoded payload. This verifies envelope integrity only; resource,
asset identity, core/DSP restoration and clock checks still run through
session::decode before activation. Existing DWRPLAY files remain unchanged.

Native save_slots uses fixed slot-NNN.dwslot names, stable IDs, newest-first
catalog ordering with ID tie-breaks, and reports corrupt IDs separately from
valid entries. New saves do not reuse occupied corrupt names. Complete sibling
files are synced before publication. New slots use atomic hard-link publication
that cannot replace an existing name, retrying the next ID on a concurrent
collision; explicit existing-slot writes use atomic rename. Directory entries
are synced. Tests run eight concurrent allocations, preserve a corrupt slot,
verify failed-metadata overwrite leaves prior bytes intact and check temp cleanup.
The existing single-file save API still uses its original size bound/rename path.

`audit-save-slots INPUT_DWRPLAY FRESH_DIRECTORY` exercises new-slot creation,
metadata replacement, catalog and byte-identical payload export. Actual user
checkpoint rightclick-node-exit-stopped.dws passed through
assets/saves/native-slot-roundtrip; the exported roundtrip.dws was fully decoded
with original resources/SoundFont and advanced to133800 Running/unseen.scn.
Its recovered.dws and assets/previews/native-slot-roundtrip-recovered.png exactly
match the earlier rightclick-node-exit-fixed session bytes and PNG. The image was
inspected. Evidence: assets/logs/native-slot-roundtrip.json and
native-slot-roundtrip-recovered.json. No active user save was replaced.
282 enabled workspace tests, Clippy, and wasm32 web-crate checking pass in
assets/logs/save-slots-{workspace,clippy-final,wasm-check}.log. Browser IndexedDB
slot persistence, host catalog asset validation/description conversion, settings
persistence and engine/native/browser menu wiring remain unfinished.


Browser slot persistence now uses the same Rust DWRSLOT codec through WASM
encode_slot/decode_slot. storage.js upgrades discworld-player IndexedDB to
version2, preserving the existing saves/manual record and adding a slots store.
New-slot selection and full-record publication share one readwrite transaction,
so concurrent tabs cannot select the same free ID. Explicit overwrites are
transactional; corrupt occupied keys remain reserved and are reported separately.
Catalog reads decode one cursor record at a time and retain only metadata, rather
than retaining all complete sessions. Timestamp input is a checked unsigned
64-bit BigInt; game-font descriptions remain bytes. Rust/IndexedDB storage
functions are available, but live menu actions are not connected yet.

All22 Chrome/WebKit tests plus two clock tests pass in
assets/logs/browser-slots-tests.log. New tests exercise v1 manual-save migration,
reload persistence, two-tab concurrent allocation, failed overwrite preservation,
corrupt slot reporting/reservation and timestamp ordering. The existing18 player
checks also passed after manual deviceSave moved into the shared storage module.
282 enabled workspace tests and Clippy pass. Browser artifacts
assets/saves/browser-{chrome,webkit}-slot.dwslot decode with native inspect-slot;
both envelopes are identical and their extracted sessions exactly match the
original inventory checkpoint. Full resource/SoundFont restoration and24 ticks
of native continuation yield identical engine/DSP saves and PNGs at33224,
Running/rinceroo.scn. The loaded inventory image was inspected. Evidence:
assets/logs/browser-{chrome,webkit}-slot-{native,loaded}.json and matching loaded
images under assets/previews. inspect-slot SLOT [FRESH_SESSION_OUTPUT] validates
the record and optionally exports the unchanged session with no-replace atomic
publication. Browser manual-save behavior remains available; original F1/menu
integration, settings persistence, catalog asset filtering and physical iPad
acceptance are still unfinished.


Navigation now emits typed Save/Load requests as well as Restart/Quit. Save/load
accept/cancel graphics use their original coordinates and ButtonPress timing.
RGROUP row input is handled separately: a single action changes selection even
while a graphic effect runs; a double action immediately closes and emits the
request using the existing selection, not the row argument. Targets/descriptions
must be resolved at dispatch time. Tests select New, press accept, change to an
existing row during the animation, then require the existing target/description;
they also cover immediate double Load, cancel without request, row gaps, mode
mismatch rejection and blank Load closing without a target. No storage operation
is performed by Navigation itself. The combined controller must still model the
shared selection state across window changes; a delayed SAVEGAME after switching
to Main can find NOBOX, so retaining an old save-list target blindly is incorrect.

Keyboard mapping audit remains open for Return: pinned metaengine.cpp puts
RETURN/KP_ENTER Action in game-shortcuts, which openMenu disables for Save/Load;
InvKeyIn lets Return pass, but KeyboardProcess's raw fallback is PLR_NOEVENT.
Consequently a direct Enter-to-confirm behavior is not established by this pin;
do not infer it from the comment alone. F5 Save and F7 Load mapping is explicit.
Mouse/list/graphic dispatch is established by dialogs.cpp::menuAction and
callFunction. Host keyboard behavior needs reference validation during wiring.
284 enabled workspace tests and Clippy pass in
assets/logs/save-menu-dispatch-{workspace,clippy}.log. Engine scene lifecycle,
input routing, menu snapshot state, live settings and native/browser integration
remain required; this increment does not change the running session.
