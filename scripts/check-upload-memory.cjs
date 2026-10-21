// Compare loader memory in separate Node processes; not an iPad memory result.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const [modulePath, mode, iso, font, saved] = process.argv.slice(2);
if (!saved || !['bulk', 'chunks'].includes(mode)) throw new Error('usage: check-upload-memory.cjs MODULE bulk|chunks ISO FONT SAVE');
const { BrowserGame, GameUpload } = require(path.resolve(modulePath));
let game;
const start = performance.now();
if (mode === 'bulk') game = new BrowserGame(fs.readFileSync(iso), fs.readFileSync(font));
else {
  const size = fs.statSync(iso).size;
  const upload = new GameUpload(size), chunk = Buffer.alloc(1024 * 1024), fd = fs.openSync(iso);
  try {
    for (let offset = 0; offset < size;) {
      const count = fs.readSync(fd, chunk, 0, Math.min(chunk.length, size-offset), offset);
      assert.ok(count > 0);
      upload.append(chunk.subarray(0, count)); offset += count;
    }
    assert.equal(upload.received(), size);
  } catch (error) { upload.free(); throw error; }
  finally { fs.closeSync(fd); }
  game = upload.finish(fs.readFileSync(font));
}
try {
  const original = fs.readFileSync(saved);
  game.load(original);
  assert.ok(Buffer.from(game.save()).equals(original), 'loader changed save state');
  console.log(JSON.stringify({mode, tick:game.tick().toString(), scene:game.scene(), save_equal:true,
    elapsed_seconds:(performance.now()-start)/1000, rss_bytes:process.memoryUsage().rss,
    max_rss_kib:process.resourceUsage().maxRSS}, null, 2));
} finally { game.free(); }
