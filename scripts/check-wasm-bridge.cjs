// Run generated Node-target WASM against the native bridge_fixture outputs.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const [modulePath, iso, font, save, reference] = process.argv.slice(2);
if (!reference) throw new Error('usage: node check-wasm-bridge.cjs MODULE ISO FONT SAVE REFERENCE');
const { BrowserGame } = require(path.resolve(modulePath));
const game = new BrowserGame(fs.readFileSync(iso), fs.readFileSync(font));
try {
  const initial = fs.readFileSync(save);
  game.load(initial);
  assert.deepEqual(Buffer.from(game.save()), fs.readFileSync(path.join(reference, 'before.dws')));
  const corrupt = Buffer.from(initial);
  corrupt[corrupt.length - 1] ^= 1;
  assert.throws(() => game.load(corrupt));
  assert.deepEqual(Buffer.from(game.save()), initial, 'rejected load changed live state');
  assert.throws(() => game.advance(25));
  assert.throws(() => game.input('not-a-command'));
  assert.deepEqual(Buffer.from(game.save()), initial, 'rejected command changed live state');
  const audio = game.advance(24);
  const encoded = Buffer.alloc(audio.length * 4);
  audio.forEach((sample, i) => encoded.writeFloatLE(sample, i * 4));
  const nativeAudio = fs.readFileSync(path.join(reference, 'audio.f32le'));
  assert.equal(encoded.length, nativeAudio.length);
  let maxError = 0, squaredError = 0, differentSamples = 0;
  audio.forEach((sample, i) => {
    const expected = nativeAudio.readFloatLE(i * 4);
    assert.ok(Number.isFinite(sample) && Number.isFinite(expected));
    const delta = Math.abs(sample - expected);
    maxError = Math.max(maxError, delta);
    squaredError += delta * delta;
    if (delta !== 0) differentSamples++;
  });
  assert.deepEqual(Buffer.from(game.frame()), fs.readFileSync(path.join(reference, 'frame.rgba')), 'native/WASM frame differs');
  const saved = Buffer.from(game.save());
  const nativeSaved = fs.readFileSync(path.join(reference, 'after.dws'));
  const core = bytes => bytes.subarray(52, 52 + bytes.readUInt32LE(12));
  assert.deepEqual(core(saved), core(nativeSaved), 'native/WASM engine state differs');
  assert.equal(game.tick().toString(), fs.readFileSync(path.join(reference, 'tick.txt'), 'utf8'));
  const tick = game.tick().toString(), scene = game.scene();
  // Within WASM, continuation across a reload must remain exactly reproducible.
  const continuedAudio = game.advance(24);
  const continuedFrame = game.frame();
  const continuedSave = game.save();
  game.load(saved);
  assert.deepEqual(game.advance(24), continuedAudio, 'WASM reload audio differs');
  assert.deepEqual(game.frame(), continuedFrame, 'WASM reload frame differs');
  assert.deepEqual(game.save(), continuedSave, 'WASM reload state differs');
  const report = {tick, scene, native_wasm_engine_equal: true,
    native_wasm_rgba_equal: true, native_wasm_full_save_equal: saved.equals(nativeSaved),
    native_wasm_stereo_f32_equal: encoded.equals(nativeAudio),
    audio_samples: audio.length, differing_audio_samples: differentSamples,
    max_audio_absolute_error: maxError, rms_audio_error: Math.sqrt(squaredError / audio.length),
    wasm_reload_exact: true, rejected_load_preserves_state: true};
  fs.writeFileSync(path.join(reference, 'wasm-verification.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
  // Audio differences are reported, never silently promoted to exact equivalence.
  if (process.argv.includes('--strict-dsp')) {
    assert.ok(report.native_wasm_stereo_f32_equal && report.native_wasm_full_save_equal, 'strict cross-platform DSP comparison failed');
  }

} finally { game.free(); }
