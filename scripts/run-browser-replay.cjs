// Full original-input route in an actual browser module worker, without UI bypasses
// inside the engine. This is accelerated runtime validation, not manual UI playback.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const browsers = require('../web/node_modules/playwright');
const [kind, iso, font, trace, output, endText='2630000', url='http://127.0.0.1:8765/'] = process.argv.slice(2);
if (!output || !['chrome','webkit'].includes(kind)) throw Error('usage: run-browser-replay.cjs chrome|webkit ISO FONT TRACE OUTPUT_DIR [FINAL_TICK] [URL]');
const end = Number(endText);
assert.ok(Number.isSafeInteger(end) && end > 500 && end < 2**32);
fs.mkdirSync(output);
(async () => {
  const browser = await (kind === 'chrome' ? browsers.chromium : browsers.webkit).launch(kind === 'chrome' ? { channel:'chrome', headless:true } : { headless:true });
  try {
    const page = await browser.newPage({ viewport:{width:1024,height:768} });
    page.on('console', message => console.error(message.text()));
    page.on('pageerror', error => console.error('page error:', error.message));
    await page.goto(url);
    await page.locator('#iso').setInputFiles(iso);
    await page.locator('#font').setInputFiles(font);
    await page.evaluate(() => {const input=document.createElement('input');input.type='file';input.id='route';document.body.appendChild(input);});
    await page.locator('#route').setInputFiles(trace);
    const result = await page.evaluate(end => new Promise((resolve, reject) => {
      const worker = new Worker('/tests/replay-worker.js', {type:'module'});
      document.querySelector('#setup').hidden=true; document.querySelector('#player').hidden=false;
      for (const control of document.querySelectorAll('#player button,#player select')) control.disabled=true;
      const canvas=document.querySelector('#game'), context=canvas.getContext('2d');
      worker.onerror = e => {worker.terminate();reject(Error(e.message));};
      worker.onmessage = ({data}) => {
        if (data.frame) context.putImageData(new ImageData(new Uint8ClampedArray(data.frame),320,200),0,0);
        if (data.progress) {
          const {tick,end,scene,inputs}=data.progress;
          console.log(`Browser replay ${tick}/${end}: ${scene}; ${inputs} inputs`);
          document.querySelector('#status').textContent=`Validation: ${tick}/${end} — ${scene}`;
        } else {
          worker.terminate();
          if (data.error) reject(Error(JSON.stringify(data)));
          else resolve({report:data.report,frame:[...data.frame],save:[...data.save]});
        }
      };
      worker.postMessage({iso:document.querySelector('#iso').files[0],font:document.querySelector('#font').files[0],trace:document.querySelector('#route').files[0],end});
    }), end);
    const frame=Buffer.from(result.frame), saved=Buffer.from(result.save);
    const report={browser:kind,browser_version:browser.version(),...result.report,
      rgba_sha256:crypto.createHash('sha256').update(frame).digest('hex'),
      core_sha256:crypto.createHash('sha256').update(saved.subarray(52,52+saved.readUInt32LE(12))).digest('hex'),
      scope:'accelerated browser worker runtime; audio generated but not sent through Web Audio'};
    fs.writeFileSync(path.join(output,'report.json'),JSON.stringify(report,null,2)+'\n');
    fs.writeFileSync(path.join(output,'final.rgba'),frame);
    fs.writeFileSync(path.join(output,'final.dws'),saved);
    await page.locator('#game').screenshot({path:path.join(output,'browser-final.png')});
    console.log(JSON.stringify({...report,scenes:undefined},null,2));
  } finally {await browser.close();}
})().catch(error => {
  fs.writeFileSync(path.join(output,'failure.json'),JSON.stringify({error:String(error)},null,2));
  console.error(error);process.exitCode=1;
});
