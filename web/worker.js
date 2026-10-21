import init, { BrowserGame } from './pkg/discworld_web.js';
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
        const iso = new Uint8Array(await data.iso.arrayBuffer());
        const font = new Uint8Array(await data.font.arrayBuffer());
        const prepared = new BrowserGame(iso, font);
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
