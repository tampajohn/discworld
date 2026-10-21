import { test } from 'node:test';
import assert from 'node:assert/strict';
import { advanceClock } from '../clock.js';
test('a pre-resume RAF timestamp never becomes a wrapped WASM tick count', () => {
  assert.deepEqual(advanceClock(0, -12), { ticks: 0, elapsed: 0 });
  assert.equal(advanceClock(0, 1000 / 24).ticks, 1);
});
test('a stalled frame caps catch-up and an outstanding request retains time', () => {
  assert.equal(advanceClock(0, 10000).ticks, 4);
  const pending = advanceClock(0, 100, true);
  assert.equal(pending.ticks, 0);
  assert.equal(pending.elapsed, 100);
  const resumed = advanceClock(pending.elapsed, 0);
  assert.equal(resumed.ticks, 2);
  assert.ok(resumed.elapsed >= 0 && resumed.elapsed < 1000 / 24);
});
