// Compare full-route results; decode native PNG with the browser's image decoder.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { chromium } = require('../web/node_modules/playwright');
const [wasmDir, nativeSavePath, nativePngPath, nativeReportPath] = process.argv.slice(2);
if (!nativeReportPath) throw new Error('usage: compare-wasm-replay.cjs WASM_DIR NATIVE_SAVE NATIVE_PNG NATIVE_REPORT');
(async () => {
  const wasm = JSON.parse(fs.readFileSync(path.join(wasmDir, 'report.json')));
  const native = JSON.parse(fs.readFileSync(nativeReportPath));
  const wasmSave = fs.readFileSync(path.join(wasmDir, 'final.dws'));
  const nativeSave = fs.readFileSync(nativeSavePath);
  const frame = fs.readFileSync(path.join(wasmDir, 'final.rgba'));
  const core = bytes => bytes.subarray(52, 52 + bytes.readUInt32LE(12));
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 320, height: 200 }, deviceScaleFactor: 1 });
    const nativeRgba = await page.evaluate(async encoded => {
      const bitmap = await createImageBitmap(await (await fetch(`data:image/png;base64,${encoded}`)).blob());
      const canvas = document.createElement('canvas'); canvas.width = 320; canvas.height = 200;
      canvas.getContext('2d').drawImage(bitmap, 0, 0); bitmap.close();
      return [...canvas.getContext('2d').getImageData(0, 0, 320, 200).data];
    }, fs.readFileSync(nativePngPath).toString('base64'));
    await page.setContent('<style>body{margin:0}canvas{display:block}</style><canvas width="320" height="200"></canvas>');
    await page.evaluate(values => document.querySelector('canvas').getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(values), 320, 200), 0, 0), [...frame]);
    await page.locator('canvas').screenshot({ path: path.join(wasmDir, 'final.png') });
    const report = {
      ticks: wasm.ticks, ticks_equal: wasm.ticks === native.ticks,
      native_status: native.status, scene: wasm.scene, scene_equal: wasm.scene === native.scene_name,
      has_restarted: wasm.has_restarted, restart_equal: wasm.has_restarted === native.has_restarted,
      accepted_inputs: wasm.accepted_inputs, native_inputs: native.interactions.length,
      native_inputs_accepted: native.interactions.every(i => i.result === 'Ok(true)'),
      engine_bytes_equal: core(wasmSave).equals(core(nativeSave)),
      rgba_equal: frame.equals(Buffer.from(nativeRgba)),
      combined_save_equal: wasmSave.equals(nativeSave),
      core_sha256: crypto.createHash('sha256').update(core(wasmSave)).digest('hex'),
      audio_scope: 'WASM checked for finite samples; cross-platform audio equality not established',
    };
    fs.writeFileSync(path.join(wasmDir, 'native-comparison.json'), JSON.stringify(report, null, 2)+'\n');
    console.log(JSON.stringify(report, null, 2));
    assert.ok(report.ticks_equal && report.scene_equal && report.restart_equal && report.native_inputs_accepted);
    assert.equal(report.accepted_inputs, report.native_inputs);
    assert.equal(report.native_status, 'Running');
    assert.equal(report.scene, 'title.scn', 'full route must return to title');
    assert.equal(report.has_restarted, true, 'full route must execute the scripted restart after credits');
    assert.ok(report.engine_bytes_equal && report.rgba_equal);
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
