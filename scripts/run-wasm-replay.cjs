// Execute the unchanged native route with actual WASM. No scene/global edits.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const [modulePath, iso, font, trace, output, endText = '2630000'] = process.argv.slice(2);
if (!output) throw new Error('usage: node run-wasm-replay.cjs MODULE ISO FONT TRACE OUTPUT_DIR [FINAL_TICK]');
const end = Number(endText);
assert.ok(Number.isSafeInteger(end) && end > 500 && end < 2**32);
fs.mkdirSync(output); // Never overwrite evidence from an earlier run.
const inputs = JSON.parse(fs.readFileSync(trace, 'utf8'));
assert.ok(inputs.every((v, i) => Number.isInteger(v.tick) && v.tick < end && (!i || v.tick > inputs[i-1].tick)));
const { BrowserGame } = require(path.resolve(modulePath));
const game = new BrowserGame(fs.readFileSync(iso), fs.readFileSync(font));
let position = 0, tick = 0, audioSamples = 0, nonfiniteSamples = 0;
const scenes = [], started = performance.now();
let nextProgress = 100000;
try {
  while (tick < end) {
    if (tick === 500) game.input('start');
    if (inputs[position]?.tick === tick) {
      game.replay_input(Buffer.from(JSON.stringify([inputs[position]])));
      position++;
    }
    const boundary = Math.min(end, inputs[position]?.tick ?? end, tick < 500 ? 500 : end, nextProgress);
    const count = Math.min(24, boundary - tick);
    assert.ok(count > 0);
    const audio = game.advance(count);
    for (const sample of audio) if (!Number.isFinite(sample)) nonfiniteSamples++;
    audioSamples += audio.length;
    tick = Number(game.tick());
    const scene = game.scene();
    if (scenes.at(-1)?.scene !== scene) scenes.push({ tick, scene });
    if (tick === nextProgress) {
      console.error(`WASM replay ${tick}/${end}: ${scene}; ${position} inputs`);
      nextProgress += 100000;
    }
  }
  const frame = Buffer.from(game.frame()), saved = Buffer.from(game.save());
  fs.writeFileSync(path.join(output, 'final.rgba'), frame);
  fs.writeFileSync(path.join(output, 'final.dws'), saved);
  const report = { ticks: tick, scene: game.scene(), has_restarted: game.has_restarted(),
    accepted_inputs: position, input_count: inputs.length, audio_samples: audioSamples,
    nonfinite_audio_samples: nonfiniteSamples, elapsed_seconds: (performance.now()-started)/1000,
    rgba_sha256: crypto.createHash('sha256').update(frame).digest('hex'),
    core_sha256: crypto.createHash('sha256').update(saved.subarray(52,52+saved.readUInt32LE(12))).digest('hex'),
    scene_history_scope: 'scene sampled after batches of up to 24 ticks; intermediate scenes may be omitted', scenes };
  fs.writeFileSync(path.join(output, 'report.json'), JSON.stringify(report, null, 2)+'\n');
  assert.equal(position, inputs.length);
  assert.equal(nonfiniteSamples, 0);
  console.log(JSON.stringify({ ...report, scenes: undefined }, null, 2));
} catch (error) {
  fs.writeFileSync(path.join(output, 'failure.json'), JSON.stringify({ tick: game.tick().toString(), scene: game.scene(), accepted_inputs: position, error: String(error) }, null, 2));
  throw error;
} finally { game.free(); }
