# Physical iPad acceptance

Status: **not executed**. Desktop Chrome/WebKit and simulator rendering do not
establish this result. See [current package checks](validation/packages-3df76c8.json)
and the [full acceptance scope](goal.md#ipad-landscape-acceptance).

## Preparation

Use the extracted `discworld-web-3df76c8.zip` development package. Serve only
that extracted directory, not the repository or the game-data directory:

```sh
cd /path/to/extracted/browser-package
python3 -m http.server 8765 --bind 0.0.0.0
```

On the same local network, open `http://MAC-LAN-ADDRESS:8765/` in iPad Safari.
The server supplies the runtime; the player selects their own ISO and SoundFont
from Files. Neither asset is served by this package. Stop the server with Ctrl-C
when finished. Use the same address and port when checking persisted device
saves, because browser storage belongs to that origin.

Record the device model, iPadOS/Safari version, package revision and SHA-256,
viewport size, orientation, test date, and any attached keyboard or pointer.
Run the touch checks without an attached keyboard or pointer.

## Device checks

1. In landscape, choose **Game ISO** and **Music SoundFont**, then **Open game**.
   Confirm upload completes without a tab reload or memory failure. Start with
   **Play**. Verify the whole scene and all controls remain reachable.
2. Use **Walk / select**, **Action**, and **Look** through the **Tap action**
   selector. Walk through a doorway and downstairs, inspect a target, and
   advance dialogue using **Continue** and **Escape**. Verify touch releases
   do not leave walking, dragging, or selection stuck.
3. Open **Pockets** and the Luggage through ordinary game interactions. Check
   backgrounds, borders, original-font labels, item pickup/storage, scrolling,
   title-bar dragging, and supported resizing. Close and reopen each window.
   The local checkpoint `evidence/save-relaunch-inventory/checkpoint.dws` can
   support a separate repeatable load test; record use of that checkpoint
   rather than counting it as completion of the preceding gameplay.
4. Use **Sound off** to enable audio through a touch gesture, then mute and
   unmute. Check speech, music, and effects; record audible crackle, clipping,
   or delayed playback. Exercise **Reduce distortion** separately. Muting must
   not stop the simulation or reset gameplay progress.
5. Use **Options** and the original Save/Load menus. Use **Type save name** to
   open the software keyboard, type and erase characters, save, and reload.
   Verify the name is rendered in the original game font and the keyboard
   does not make essential controls inaccessible.
6. Exercise **Save on device**, **Load device save**, **Export save**, and
   **Import save**. Close Safari, reopen the same origin, select the assets
   again, and load the saved progress. Confirm inventory contents and scene
   state survive; test an exported file independently of device storage.
7. Rotate to portrait and back. Background Safari, lock/unlock the device,
   and return during play and during a held touch/drag. Check that input is
   released, audio resumes only through supported user interaction, and no
   burst of simulation or loss of progress occurs. Include a sustained play
   session long enough to observe memory pressure and audio continuity.
8. Complete a separate start-to-credits run using touch alone, with save/load
   in every act. Record puzzle milestones, ending and credits, and restart.
   The accelerated resolved-input audit does not satisfy this gesture test.

## Evidence and result

For each check, record pass/fail, the observed result, and the checkpoint or
reproduction steps for any failure. Retain screenshots or recordings where
they demonstrate layout or input behavior; log audio observations explicitly
rather than inferring them from generated PCM. Record interrupted or untested
checks as incomplete. Keep game data and personal device details out of public
artifacts.

No physical-device result should be marked passed until the corresponding
device interaction has actually been observed.
