import { listSlots, readSlot, writeSlot, devicePreferences } from './storage.js';
import init, { GameUpload } from './pkg/discworld_web.js';
let game;
let queue = Promise.resolve();
const snapshot = () => ({ frame: game.frame(), tick: game.tick().toString(), scene: game.scene(), dragging: game.inventory_dragging(), dragButton: game.drag_button() || null, menu: game.menu_active(), editing: game.menu_editing(), menuPage: game.menu_page() || '' });
async function refreshCatalog() {
  const { entries } = await listSlots();
  game.clear_menu_catalog();
  for (const entry of entries) {
    try { game.add_menu_slot((await readSlot(entry.id)).bytes); }
    catch { /* Incompatible/corrupt slots remain reserved in storage. */ }
  }
}
async function consumeMenu() {
  const effects = {};
  for (let raw; (raw = game.take_menu_request()) !== undefined;) {
    const { request, selection, settings } = JSON.parse(raw);
    if (settings) await devicePreferences(settings);
    if (request === 'save' && selection) {
      await writeSlot({ id: selection.id, timestampMs: BigInt(Date.now()), description: new Uint8Array(selection.description), session: game.save() });
      await refreshCatalog();
      effects.notice = 'Game saved on this device.';
    } else if (request === 'load' && selection) {
      if (selection.id === null) throw new Error('Load requires an existing slot');
      game.load_game((await readSlot(selection.id)).session);
      effects.resetAudio = true;
      effects.resetPointers = true;
      effects.changed = true;
      effects.notice = 'Saved game loaded.';
    } else if (request === 'quit') effects.quit = true;
    else if (request === 'restart') { effects.resetAudio = true; effects.resetPointers = true; }
  }
  return effects;
}
async function handle({ id, type, ...data }) {
  try {
    let result;
    switch (type) {
      case 'open': {
        await init();
        if (game) throw new Error('Game is already open');
        if (!Number.isSafeInteger(data.iso.size) || data.iso.size <= 0 || data.iso.size > 1024 ** 3) {
          throw new Error('ISO upload must be between 1 byte and 1 GiB');
        }
        let upload = new GameUpload(data.iso.size);
        let prepared;
        try {
          for (let offset = 0; offset < data.iso.size; offset += 1024 ** 2) {
            upload.append(new Uint8Array(await data.iso.slice(offset, offset + 1024 ** 2).arrayBuffer()));
          }
          const font = new Uint8Array(await data.font.arrayBuffer());
          // finish consumes its Rust receiver even when validation fails.
          const complete = upload; upload = null;
          prepared = complete.finish(font);
        } finally { if (upload) upload.free(); }
        try {
          if (data.save) prepared.load_game(new Uint8Array(await data.save.arrayBuffer()));
          game = prepared;
          let notice;
          try {
            const preferences = await devicePreferences();
            if (preferences) {
              try { prepared.menu_preferences(JSON.stringify(preferences)); }
              catch { notice = 'Stored preferences are invalid; using the current game settings.'; }
            }
            await refreshCatalog();
          } catch (error) {
            game.clear_menu_catalog();
            notice = `Device storage unavailable: ${error}. Play and file export are still available.`;
          }
          result = { ...snapshot(), notice };
        } catch (error) { game = undefined; prepared.free(); throw error; }
        break;
      }
      case 'menu': await refreshCatalog(); game.open_menu(data.page); result = snapshot(); break;
      case 'edit-menu': game.edit_menu(data.text || '', !!data.backspace); result = snapshot(); break;
      case 'audio-headroom': game.set_audio_headroom(Boolean(data.enabled)); result = {}; break;
      case 'advance': result = { audio: game.advance(data.ticks), ...snapshot() }; break;
      case 'input':
        if (data.point) game.move_pointer_checked(...data.point);
        game.input(data.command);
        result = snapshot();
        break;
      case 'pointer': game.move_pointer_checked(...data.point); result = snapshot(); break;
      case 'save': result = { bytes: game.save(), tick: game.tick().toString() }; break;
      case 'load': game.load_game(new Uint8Array(data.bytes)); result = { ...snapshot(), resetPointers: true }; break;
      default: throw new Error('Unknown worker request');
    }
    const effects = await consumeMenu();
    result = { ...result, ...(effects.changed ? snapshot() : {}), ...effects };
    if (effects.resetAudio) result.audio = new Float32Array();
    const transfer = [result.frame?.buffer, result.audio?.buffer, result.bytes?.buffer].filter(Boolean);
    postMessage({ id, result }, transfer);
  } catch (error) { postMessage({ id, error: String(error) }); }
}
// Initialization awaits file reads; serialize all later messages behind it.
onmessage = ({ data }) => { queue = queue.then(() => handle(data)); };
