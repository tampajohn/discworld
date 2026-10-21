// Actual WASM save/load between a physical ground click and its first tick.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const [modulePath, iso, font, save] = process.argv.slice(2);
if (!save) throw Error('usage: check-pending-walk.cjs MODULE ISO SOUNDFONT INVENTORY_SAVE');
const { GameUpload } = require(path.resolve(modulePath));
const fd = fs.openSync(iso, 'r');
let upload, game;
try {
  const size = fs.fstatSync(fd).size;
  upload = new GameUpload(size);
  const chunk = Buffer.alloc(1024 ** 2);
  for (let offset = 0; offset < size;) {
    const n = fs.readSync(fd, chunk, 0, Math.min(chunk.length, size - offset), offset);
    if (!n) throw Error('unexpected ISO EOF');
    upload.append(chunk.subarray(0, n)); offset += n;
  }
  const ready = upload; upload = null;
  game = ready.finish(fs.readFileSync(font));
  game.load(fs.readFileSync(save));
  game.input('close');
  game.move_pointer(300, 180);
  game.input('left-down');
  const pending = game.save();
  game.load(pending);
  assert.deepEqual(game.save(), pending);
  const audio = game.advance(24), frame = game.frame(), continued = game.save();
  game.load(pending);
  assert.deepEqual(game.advance(24), audio);
  assert.deepEqual(game.frame(), frame);
  assert.deepEqual(game.save(), continued);
  console.log(JSON.stringify({tick:game.tick().toString(),scene:game.scene(),pending_reload_exact:true,continued_engine_dsp_frame_audio_exact:true}));
} finally {
  fs.closeSync(fd);
  if (upload) upload.free();
  if (game) game.free();
}
