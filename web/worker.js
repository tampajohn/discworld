import init, { GameUpload } from './pkg/discworld_web.js';
let game;
let queue = Promise.resolve();
const snapshot = () => ({ frame: game.frame(), tick: game.tick().toString(), scene: game.scene() });
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
          if (data.save) prepared.load(new Uint8Array(await data.save.arrayBuffer()));
          game = prepared;
        } catch (error) { prepared.free(); throw error; }
        result = snapshot();
        break;
      }
      case 'advance': result = { audio: game.advance(data.ticks), ...snapshot() }; break;
      case 'input':
        if (data.point) game.move_pointer(...data.point);
        game.input(data.command);
        result = snapshot();
        break;
      case 'pointer': game.move_pointer(...data.point); result = {}; break;
      case 'save': result = { bytes: game.save(), tick: game.tick().toString() }; break;
      case 'load': game.load(new Uint8Array(data.bytes)); result = snapshot(); break;
      default: throw new Error('Unknown worker request');
    }
    const transfer = [result.frame?.buffer, result.audio?.buffer, result.bytes?.buffer].filter(Boolean);
    postMessage({ id, result }, transfer);
  } catch (error) { postMessage({ id, error: String(error) }); }
}
// Initialization awaits file reads; serialize all later messages behind it.
onmessage = ({ data }) => { queue = queue.then(() => handle(data)); };
