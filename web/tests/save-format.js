import assert from 'node:assert/strict';

// Combined sessions carry the core snapshot after the fixed 52-byte header.
// Versions 13–25 wrap an older snapshot as the first postcard Vec<u8> field;
// versions 1–12 directly encode the runtime. Keep the inventory schema checks
// while allowing the current world-monitor wrapper around that state.
export function coreVersions(session) {
  let core = session.subarray(52, 52 + session.readUInt32LE(12));
  const versions = [];
  for (let depth = 0; depth < 25; depth++) {
    assert.equal(core.subarray(0, 8).toString('binary'), 'DWRSAVE\0');
    assert.equal(core.readUInt32LE(44), core.length - 80);
    const version = core.readUInt32LE(8);
    versions.push(version);
    if (version <= 12) return versions;
    assert.ok(version <= 25, `unknown snapshot wrapper ${version}`);
    let length = 0, factor = 1, offset = 80;
    for (;;) {
      assert.ok(offset < core.length && factor <= 2 ** 28);
      const byte = core[offset++];
      length += (byte & 127) * factor;
      if (!(byte & 128)) break;
      factor *= 128;
    }
    assert.ok(length >= 80 && length <= core.length - offset);
    core = core.subarray(offset, offset + length);
  }
  throw new Error('excessive snapshot nesting');
}
