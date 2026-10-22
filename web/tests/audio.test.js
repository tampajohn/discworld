import { test } from 'node:test';
import assert from 'node:assert/strict';
import { AudioResampler } from '../audio.js';

test('device conversion preserves samples and duration regardless of worker block sizes', () => {
  const samples = Float32Array.from({length: 22050 * 2}, (_, i) => Math.sin(Math.floor(i / 2) * .31) * (i % 2 ? -.5 : .5));
  for (const rate of [8000, 22050, 44100, 48000, 96000]) {
    const whole = new AudioResampler(rate).convert(samples);
    const split = new AudioResampler(rate), output = [];
    for (let tick = 0; tick < 24; tick++) {
      output.push(...split.convert(samples.subarray(Math.floor(tick * 22050 / 24) * 2, Math.floor((tick + 1) * 22050 / 24) * 2)));
      assert.ok(split.tail.length <= 2);
    }
    assert.deepEqual(Float32Array.from(output), whole);
    assert.equal(whole.length / 2, Math.ceil(22049 * rate / 22050));
    split.reset();
    assert.deepEqual(split.convert(samples), whole);
  }
});

test('single-frame packets interpolate across boundaries and invalid packets preserve history', () => {
  const converter = new AudioResampler(44100);
  assert.deepEqual(converter.convert(new Float32Array([0, 1])), new Float32Array());
  assert.throws(() => converter.convert(new Float32Array([NaN, 0])));
  assert.throws(() => converter.convert(new Float32Array([0])));
  assert.deepEqual(converter.convert(new Float32Array([1, -1])), new Float32Array([0, 1, .5, 0]));
});
