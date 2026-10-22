import { test, expect } from '@playwright/test';
import fs from 'node:fs/promises';
import path from 'node:path';
const root = path.resolve(import.meta.dirname, '../..');
const iso = process.env.DISCWORLD_TEST_ISO || path.join(root, "Terry Pratchett's Discworld (1995)(Psygnosis).iso");
const font = process.env.DISCWORLD_TEST_SOUNDFONT || path.join(root, 'assets/soundfonts/TimGM6mb.sf2');
const save = process.env.DISCWORLD_TEST_SAVE || path.join(root, 'evidence/save-relaunch-inventory/checkpoint.dws');
const frame = process.env.DISCWORLD_TEST_FRAME || path.join(root, 'evidence/save-relaunch-inventory/checkpoint.png');
async function open(page, initial = true, checkpoint = save) {
  await page.locator('#iso').setInputFiles(iso);
  await page.locator('#font').setInputFiles(font);
  if (initial) await page.locator('#initial-save').setInputFiles(checkpoint);
  await page.locator('#open').click();
  await expect(page.locator('#player')).toBeVisible({ timeout: 60000 });
}
async function exported(page) {
  const download = page.waitForEvent('download');
  await page.locator('#export').click();
  return fs.readFile(await (await download).path());
}
async function matchesOriginalFrame(page) {
  const encoded = (await fs.readFile(frame)).toString('base64');
  expect(await page.evaluate(async encoded => {
    const bitmap = await createImageBitmap(await (await fetch(`data:image/png;base64,${encoded}`)).blob());
    const expected = document.createElement('canvas'); expected.width = 320; expected.height = 200;
    expected.getContext('2d').drawImage(bitmap, 0, 0); bitmap.close();
    const pixels = expected.getContext('2d').getImageData(0, 0, 320, 200).data;
    const actual = document.querySelector('#game').getContext('2d').getImageData(0, 0, 320, 200).data;
    return actual.every((value, i) => value === pixels[i]);
  }, encoded)).toBe(true);
}
test('real WASM canvas, native save import/export, audio controls and device persistence', async ({ page }, testInfo) => {
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.addInitScript(() => {
    window.audioEvidence = { starts: 0, nonzero: 0, finite: true, rate: 0, channels: 0 };
    const start = AudioBufferSourceNode.prototype.start;
    AudioBufferSourceNode.prototype.start = function(...args) {
      const evidence = window.audioEvidence;
      evidence.starts++; evidence.rate = this.buffer.sampleRate; evidence.deviceRate = this.context.sampleRate; evidence.channels = this.buffer.numberOfChannels;
      for (let channel = 0; channel < this.buffer.numberOfChannels; channel++) {
        for (const sample of this.buffer.getChannelData(channel)) {
          if (sample !== 0) evidence.nonzero++;
          if (!Number.isFinite(sample)) evidence.finite = false;
        }
      }
      return start.apply(this, args);
    };
  });
  await page.goto('/');
  await open(page);
  await expect(page.locator('#game')).toHaveAttribute('data-tick', '33200');
  await matchesOriginalFrame(page);
  await page.screenshot({ path: path.join(root, `assets/previews/browser-${testInfo.project.name}-inventory-loaded.png`) });
  await page.locator('summary').click();
  const original = await fs.readFile(save);
  expect(await exported(page)).toEqual(original);
  await page.locator('#headroom').click();
  await expect(page.locator('#headroom')).toHaveAttribute('aria-pressed', 'true');
  expect(await exported(page)).toEqual(original);
  await expect(page.locator('#game')).toHaveAttribute('data-tick', '33200');
  await page.locator('#headroom').click();
  await expect(page.locator('#headroom')).toHaveAttribute('aria-pressed', 'false');
  await page.locator('#save-local').click();
  await expect(page.locator('#status')).toHaveText('Saved on this device.');
  await page.locator('#mute').click();
  await expect(page.locator('#mute')).toHaveText('Sound on');
  await page.locator('#pause').click();
  await expect.poll(async () => Number(await page.locator('#game').getAttribute('data-tick'))).toBeGreaterThan(33205);
  await expect.poll(() => page.evaluate(() => window.audioEvidence.starts)).toBeGreaterThan(0);
  const audio = await page.evaluate(() => window.audioEvidence);
  expect(audio).toMatchObject({ finite: true, rate: audio.deviceRate, channels: 2 });
  expect(audio.nonzero).toBeGreaterThan(0);
  await page.locator('#pause').click();
  await page.waitForTimeout(150); // allow the already-submitted tick to settle
  const pausedTick = await page.locator('#game').getAttribute('data-tick');
  await page.waitForTimeout(200);
  await expect(page.locator('#game')).toHaveAttribute('data-tick', pausedTick);
  await page.locator('#mute').click();
  await expect(page.locator('#mute')).toHaveText('Sound off');
  await page.locator('#load-local').click();
  await expect(page.locator('#game')).toHaveAttribute('data-tick', '33200');
  await matchesOriginalFrame(page);
  const corrupted = Buffer.from(original); corrupted[0] ^= 1;
  await page.locator('#import').setInputFiles({ name: 'corrupt.dws', mimeType: 'application/octet-stream', buffer: corrupted });
  await expect(page.locator('#status')).toContainText('Could not load save:');
  expect(await exported(page)).toEqual(original);
  await matchesOriginalFrame(page);
  // A new worker/page must recover the IndexedDB save after selecting assets again.
  await page.reload();
  await open(page, false);
  await page.locator('summary').click();
  await page.locator('#load-local').click();
  await expect(page.locator('#game')).toHaveAttribute('data-tick', '33200');
  await matchesOriginalFrame(page);
  expect(await exported(page)).toEqual(original);
  const viewport = page.viewportSize();
  const bounds = await page.locator('#game').boundingBox();
  expect(bounds.width / bounds.height).toBeCloseTo(1.6, 2);
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(viewport.width);
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(viewport.height);
  // Send actual touch input through the scaled canvas outside the open inventory.
  await page.locator('summary').click();
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Pause');
  await page.touchscreen.tap(bounds.x + bounds.width * 20 / 320, bounds.y + bounds.height * 180 / 200);
  await page.waitForTimeout(250);
  await page.locator('#pause').click();
  await page.waitForTimeout(150);
  await page.screenshot({ path: path.join(root, `assets/previews/browser-${testInfo.project.name}-touch-close-inventory.png`) });
  const touched = await exportedAfterOpeningDetails(page);
  expect(touched).not.toEqual(original);
  // Inner core v4 proves physical input reached the shared classifier. Preserve
  // its partial click history through the production import/export path.
  expect(touched.readUInt32LE(52 + 8)).toBe(4);
  await page.locator('#import').setInputFiles({ name: 'mouse.dws', mimeType: 'application/octet-stream', buffer: touched });
  await expect(page.locator('#status')).toContainText('Saved game loaded.');
  expect(await exported(page)).toEqual(touched);
  expect(errors).toEqual([]);
});

async function exportedAfterOpeningDetails(page) {
  await page.locator('summary').click();
  return exported(page);
}

test('inventory header drag renders immediately and roundtrips an active drag', async ({ page }, testInfo) => {
  await page.goto('/');
  await open(page);
  const canvas = page.locator('#game');
  const bounds = await canvas.boundingBox();
  const move = (x, y) => page.mouse.move(bounds.x + bounds.width * x / 320, bounds.y + bounds.height * y / 200);
  // Export without moving/releasing the held mouse button to click a toolbar.
  const exportHeld = async () => {
    const download = page.waitForEvent('download');
    await page.evaluate(() => document.querySelector('#export').click());
    return fs.readFile(await (await download).path());
  };
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Pause');
  await move(140, 110);
  await page.mouse.down();
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  await move(170, 130);
  await page.keyboard.press('p');
  await expect(page.locator('#pause')).toHaveText('Play');
  await page.waitForTimeout(150);
  const pending = await exportHeld();
  expect(pending.readUInt32LE(60)).toBe(5);
  const pixels = await canvas.evaluate(c => c.toDataURL());
  await page.screenshot({ path: path.join(root, `assets/previews/browser-${testInfo.project.name}-inventory-drag.png`) });
  await move(250, 180);
  await page.mouse.up();
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  expect(await canvas.evaluate(c => c.toDataURL())).toBe(pixels);
  expect((await exportHeld()).readUInt32LE(60)).toBe(4);
  await page.locator('#import').setInputFiles({ name: 'drag.dws', mimeType: 'application/octet-stream', buffer: pending });
  await expect(page.locator('#status')).toContainText('Saved game loaded.');
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  expect(await exportHeld()).toEqual(pending);
  expect(await canvas.evaluate(c => c.toDataURL())).toBe(pixels);
  // A matching release ends a restored drag even without host pointer ownership.
  await page.mouse.up();
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  if (testInfo.project.name === 'chrome') {
    // Real browser touch dispatch (not DOM PointerEvent synthesis). WebKit's
    // public Playwright touch API only supplies taps; mouse coverage runs there.
    await page.locator('#import').setInputFiles(save);
    await expect(page.locator('#status')).toContainText('Saved game loaded.');
    await page.locator('#pause').click();
    await expect(page.locator('#pause')).toHaveText('Pause');
    const cdp = await page.context().newCDPSession(page);
    const touch = (x, y) => [{ x: bounds.x + bounds.width * x / 320, y: bounds.y + bounds.height * y / 200, id: 1 }];
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: touch(140, 110) });
    await expect(canvas).toHaveAttribute('data-dragging', 'true');
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: touch(170, 130) });
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    await expect(canvas).toHaveAttribute('data-dragging', 'false');
    await page.locator('#pause').click();
    await page.waitForTimeout(150);
    await fs.writeFile(path.join(root, 'assets/saves/browser-chrome-touch-drag.dws'), await exportHeld());
    await page.screenshot({ path: path.join(root, 'assets/previews/browser-chrome-touch-drag.png') });
    await cdp.detach();
  }
});

test('cold start reaches the original title and accepts Continue', async ({ page }, testInfo) => {
  await page.goto('/');
  await open(page, false);
  await expect(page.locator('#game')).toHaveAttribute('data-tick', '0');
  await page.locator('#pause').click();
  await expect(page.locator('#game')).toHaveAttribute('data-scene', 'title.scn', { timeout: 30000 });
  await expect.poll(async () => Number(await page.locator('#game').getAttribute('data-tick')), { timeout: 30000 }).toBeGreaterThan(390);
  await page.locator('[data-input="continue"]').click();
  await expect(page.locator('#game')).not.toHaveAttribute('data-scene', 'title.scn', { timeout: 15000 });
  await page.locator('#pause').click();
  await page.screenshot({ path: path.join(root, `assets/previews/browser-${testInfo.project.name}-cold-start.png`) });
  await expect(page.locator('#status')).not.toHaveAttribute('data-error', 'true');
});

test('audio interruption pauses simulation and Play resumes it', async ({ page }) => {
  await page.addInitScript(() => {
    const Original = window.AudioContext;
    window.AudioContext = new Proxy(Original, {
      construct(target, args) {
        const context = Reflect.construct(target, args);
        window.audioContextUnderTest = context;
        return context;
      },
    });
  });
  await page.goto('/');
  await open(page);
  await page.locator('#pause').click();
  await expect.poll(async () => Number(await page.locator('#game').getAttribute('data-tick'))).toBeGreaterThan(33203);
  // Suspend the real context, exercising the player's actual statechange handler.
  // This does not simulate an OS-level interruption or prove device recovery.
  await page.evaluate(() => window.audioContextUnderTest.suspend());
  await expect(page.locator('#status')).toHaveText('Audio interrupted. Press Play to resume.');
  await expect(page.locator('#pause')).toHaveText('Play');
  await page.waitForTimeout(150);
  const tick = await page.locator('#game').getAttribute('data-tick');
  await page.waitForTimeout(200);
  await expect(page.locator('#game')).toHaveAttribute('data-tick', tick);
  await page.locator('#pause').click();
  await expect.poll(async () => Number(await page.locator('#game').getAttribute('data-tick'))).toBeGreaterThan(Number(tick));
  expect(await page.evaluate(() => window.audioContextUnderTest.state)).toBe('running');
  await page.locator('#pause').click();
});


test('inventory scrollbar drag preserves active save and releases after reload', async ({ page }, testInfo) => {
  const checkpoint = process.env.DISCWORLD_TEST_SLIDER_SAVE || path.join(root, 'assets/saves/slider-checkpoint.dws');
  await page.goto('/');
  await open(page, true, checkpoint);
  const canvas = page.locator('#game');
  const bounds = await canvas.boundingBox();
  const move = (x, y) => page.mouse.move(bounds.x + bounds.width * x / 320, bounds.y + bounds.height * y / 200);
  const exportHeld = async () => {
    const download = page.waitForEvent('download');
    await page.evaluate(() => document.querySelector('#export').click());
    return fs.readFile(await (await download).path());
  };
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Pause');
  await move(189, 126);
  await page.mouse.down();
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  await move(189, 146);
  const movedTick = Number(await canvas.getAttribute('data-tick'));
  await expect.poll(async () => Number(await canvas.getAttribute('data-tick'))).toBeGreaterThan(movedTick + 1);
  await page.keyboard.press('p');
  await expect(page.locator('#pause')).toHaveText('Play');
  await page.waitForTimeout(150);
  const pending = await exportHeld();
  // A pending hover can select a newer outer format; verify exact state below.
  const pixels = await canvas.evaluate(c => c.toDataURL());
  await fs.writeFile(path.join(root, `assets/saves/browser-${testInfo.project.name}-slider-active.dws`), pending);
  await page.screenshot({ path: path.join(root, `assets/previews/browser-${testInfo.project.name}-slider-active.png`) });
  await page.mouse.up();
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  await page.locator('#import').setInputFiles({ name: 'slider.dws', mimeType: 'application/octet-stream', buffer: pending });
  await expect(page.locator('#status')).toContainText('Saved game loaded.');
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  expect(await exportHeld()).toEqual(pending);
  expect(await canvas.evaluate(c => c.toDataURL())).toBe(pixels);
  await page.mouse.up();
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  expect(await exportHeld()).not.toEqual(pending);
  await page.locator('#pause').click();
  const releasedTick = Number(await canvas.getAttribute('data-tick'));
  await expect.poll(async () => Number(await canvas.getAttribute('data-tick'))).toBeGreaterThan(releasedTick + 1);
  await page.keyboard.press('p');
  await expect(page.locator('#pause')).toHaveText('Play');
  await page.waitForTimeout(150);
  await fs.writeFile(path.join(root, `assets/saves/browser-${testInfo.project.name}-slider-released.dws`), await exportHeld());
});

test('inventory title double click maximizes and restores across a save', async ({ page }, testInfo) => {
  await page.goto('/');
  await open(page);
  const canvas = page.locator('#game');
  const bounds = await canvas.boundingBox();
  const double = (x,y) => page.mouse.dblclick(bounds.x + bounds.width*x/320, bounds.y + bounds.height*y/200, { delay: 60 });
  const exportPaused = async () => {
    const download = page.waitForEvent('download');
    await page.evaluate(() => document.querySelector('#export').click());
    return fs.readFile(await (await download).path());
  };
  await page.locator('#pause').click();
  await double(140,110);
  await expect(canvas).toHaveAttribute('data-dragging','false');
  await page.keyboard.press('p');
  await expect(page.locator('#pause')).toHaveText('Play');
  await page.waitForTimeout(150);
  const maximized = await exportPaused();
  expect(maximized.readUInt32LE(60)).toBe(7);
  const pixels = await canvas.evaluate(c => c.toDataURL());
  await fs.writeFile(path.join(root, `assets/saves/browser-${testInfo.project.name}-maximized.dws`),maximized);
  await page.screenshot({ path: path.join(root, `assets/previews/browser-${testInfo.project.name}-maximized.png`) });
  await page.locator('#import').setInputFiles({ name:'maximized.dws', mimeType:'application/octet-stream', buffer:maximized });
  await expect(page.locator('#status')).toContainText('Saved game loaded.');
  expect(await exportPaused()).toEqual(maximized);
  expect(await canvas.evaluate(c => c.toDataURL())).toBe(pixels);
  await page.locator('#pause').click();
  await double(41,25);
  await page.keyboard.press('p');
  await expect(page.locator('#pause')).toHaveText('Play');
  await page.waitForTimeout(150);
  const restored = await exportPaused();
  expect(restored.readUInt32LE(60)).toBe(7);
  await fs.writeFile(path.join(root, `assets/saves/browser-${testInfo.project.name}-maximized-restored.dws`),restored);
  await page.screenshot({ path: path.join(root, `assets/previews/browser-${testInfo.project.name}-maximized-restored.png`) });
});

test('inventory corner resize preserves outline and releases after reload', async ({ page }, testInfo) => {
  await page.goto('/');
  await open(page);
  const canvas = page.locator('#game');
  const bounds = await canvas.boundingBox();
  const move = (x, y) => page.mouse.move(bounds.x + bounds.width * x / 320, bounds.y + bounds.height * y / 200);
  const exportHeld = async () => {
    const download = page.waitForEvent('download');
    await page.evaluate(() => document.querySelector('#export').click());
    return fs.readFile(await (await download).path());
  };
  await page.locator('#pause').click();
  // Play awaits AudioContext.resume(); a completed DOM click is not readiness.
  await expect(page.locator('#pause')).toHaveText('Pause');
  await move(194, 172);
  await page.mouse.down();
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  await move(207, 185);
  const movedTick = Number(await canvas.getAttribute('data-tick'));
  await expect.poll(async () => Number(await canvas.getAttribute('data-tick'))).toBeGreaterThan(movedTick + 1);
  await page.keyboard.press('p');
  await expect(page.locator('#pause')).toHaveText('Play');
  await page.waitForTimeout(150);
  const pending = await exportHeld();
  expect(pending.readUInt32LE(60)).toBe(8);
  const pixels = await canvas.evaluate(c => c.toDataURL());
  await fs.writeFile(path.join(root, `assets/saves/browser-${testInfo.project.name}-resize-active.dws`), pending);
  await page.screenshot({ path: path.join(root, `assets/previews/browser-${testInfo.project.name}-resize-active.png`) });
  await page.mouse.up();
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  await page.locator('#import').setInputFiles({ name: 'resize.dws', mimeType: 'application/octet-stream', buffer: pending });
  await expect(page.locator('#status')).toContainText('Saved game loaded.');
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  expect(await exportHeld()).toEqual(pending);
  expect(await canvas.evaluate(c => c.toDataURL())).toBe(pixels);
  await page.mouse.up();
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  expect((await exportHeld()).readUInt32LE(60)).toBe(4);
  await page.locator('#pause').click();
  const releasedTick = Number(await canvas.getAttribute('data-tick'));
  await expect.poll(async () => Number(await canvas.getAttribute('data-tick'))).toBeGreaterThan(releasedTick + 1);
  await page.keyboard.press('p');
  await expect(page.locator('#pause')).toHaveText('Play');
  await page.waitForTimeout(150);
  await fs.writeFile(path.join(root, `assets/saves/browser-${testInfo.project.name}-resize-released.dws`), await exportHeld());
});

test('inventory scrolling during resize keeps an active restorable outline', async ({ page }, testInfo) => {
  await page.goto('/');
  await open(page, true, path.join(root, 'assets/saves/slider-checkpoint.dws'));
  const canvas = page.locator('#game');
  const bounds = await canvas.boundingBox();
  const move = (x, y) => page.mouse.move(bounds.x + bounds.width * (x + 0.5) / 320, bounds.y + bounds.height * (y + 0.5) / 200);
  const exportHeld = async () => {
    const download = page.waitForEvent('download');
    await page.evaluate(() => document.querySelector('#export').click());
    return fs.readFile(await (await download).path());
  };
  await page.locator('#pause').click();
  // Play awaits AudioContext.resume(); a completed DOM click is not readiness.
  await expect(page.locator('#pause')).toHaveText('Pause');
  await move(100, 135);
  await page.mouse.down();
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  await move(87, 135);
  await page.keyboard.press('ArrowDown');
  const movedTick = Number(await canvas.getAttribute('data-tick'));
  await expect.poll(async () => Number(await canvas.getAttribute('data-tick'))).toBeGreaterThan(movedTick + 1);
  await page.keyboard.press('p');
  await expect(page.locator('#pause')).toHaveText('Play');
  await page.waitForTimeout(150);
  const pending = await exportHeld();
  expect(pending.readUInt32LE(60)).toBe(8);
  const pixels = await canvas.evaluate(c => c.toDataURL());
  await fs.writeFile(path.join(root, `assets/saves/browser-${testInfo.project.name}-resize-redraw-active.dws`), pending);
  await page.screenshot({ path: path.join(root, `assets/previews/browser-${testInfo.project.name}-resize-redraw-active.png`) });
  await page.mouse.up();
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  await page.locator('#import').setInputFiles({ name: 'resize.dws', mimeType: 'application/octet-stream', buffer: pending });
  await expect(page.locator('#status')).toContainText('Saved game loaded.');
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  expect(await exportHeld()).toEqual(pending);
  expect(await canvas.evaluate(c => c.toDataURL())).toBe(pixels);
  await page.mouse.up();
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  expect((await exportHeld()).readUInt32LE(60)).toBe(4);
  await page.locator('#pause').click();
  const releasedTick = Number(await canvas.getAttribute('data-tick'));
  await expect.poll(async () => Number(await canvas.getAttribute('data-tick'))).toBeGreaterThan(releasedTick + 1);
  await page.keyboard.press('p');
  await expect(page.locator('#pause')).toHaveText('Play');
  await page.waitForTimeout(150);
  await fs.writeFile(path.join(root, `assets/saves/browser-${testInfo.project.name}-resize-redraw-released.dws`), await exportHeld());
});

test('original inventory hover cursor survives save and restores over body', async ({ page }, testInfo) => {
  await page.goto('/');
  await open(page);
  const canvas = page.locator('#game');
  const bounds = await canvas.boundingBox();
  const move = (x,y) => page.mouse.move(bounds.x + bounds.width*(x+0.5)/320, bounds.y + bounds.height*(y+0.5)/200);
  const exportPaused = async () => {
    const download = page.waitForEvent('download');
    await page.evaluate(() => document.querySelector('#export').click());
    return fs.readFile(await (await download).path());
  };
  for (const [label, point] of [['arrow',[195,135]],['normal',[145,135]],['label',[118,132]]]) {
    await page.locator('#pause').click();
    await expect(page.locator('#pause')).toHaveText('Pause');
    await move(...point);
    const movedTick = Number(await canvas.getAttribute('data-tick'));
    await expect.poll(async () => Number(await canvas.getAttribute('data-tick'))).toBeGreaterThan(movedTick+2);
    await page.keyboard.press('p');
    await expect(page.locator('#pause')).toHaveText('Play');
    await page.waitForTimeout(150);
    const saved = await exportPaused();
    const pixels = await canvas.evaluate(c => c.toDataURL());
    await fs.writeFile(path.join(root, `assets/saves/browser-${testInfo.project.name}-hover-${label}.dws`),saved);
    await page.screenshot({path:path.join(root, `assets/previews/browser-${testInfo.project.name}-hover-${label}.png`)});
    await page.locator('#import').setInputFiles({name:'hover.dws',mimeType:'application/octet-stream',buffer:saved});
    await expect(page.locator('#status')).toContainText('Saved game loaded.');
    expect(await exportPaused()).toEqual(saved);
    expect(await canvas.evaluate(c => c.toDataURL())).toBe(pixels);
  }
});

test('swapped-button inventory drag restores its physical release button', async ({ page }) => {
  await page.goto('/');
  await page.evaluate(async () => {
    const { devicePreferences } = await import('/storage.js');
    await devicePreferences({ volumes: [255, 255, 255], double_click: 10,
      swap_buttons: true, text_speed: 0, subtitles: true, flipped: false });
  });
  await open(page);
  const canvas = page.locator('#game');
  const bounds = await canvas.boundingBox();
  const move = (x, y) => page.mouse.move(bounds.x + bounds.width * x / 320, bounds.y + bounds.height * y / 200);
  const exportHeld = async () => {
    const download = page.waitForEvent('download');
    await page.evaluate(() => document.querySelector('#export').click());
    return fs.readFile(await (await download).path());
  };
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Pause');
  await move(140, 110);
  await page.mouse.down({ button: 'right' });
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  await expect(canvas).toHaveAttribute('data-drag-button', 'right');
  await move(165, 125);
  await page.evaluate(() => document.querySelector('#pause').click());
  await expect(page.locator('#pause')).toHaveText('Play');
  const pending = await exportHeld();
  await page.mouse.up({ button: 'right' });
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  await page.locator('#import').setInputFiles({ name: 'swapped-drag.dws', mimeType: 'application/octet-stream', buffer: pending });
  await expect(page.locator('#status')).toContainText('Saved game loaded.');
  await expect(canvas).toHaveAttribute('data-drag-button', 'right');
  expect(await exportHeld()).toEqual(pending);
  await page.mouse.up({ button: 'left' });
  await exportHeld(); // Barrier behind the input's worker response.
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  await page.mouse.up({ button: 'right' });
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  await expect(canvas).toHaveAttribute('data-drag-button', '');
});

for (const source of ['keyboard', 'touch']) {
  test(`Escape from ${source} skips pre-title sequence`, async ({ page }) => {
    await page.goto('/');
    await open(page, false);
    await expect(page.locator('#game')).toHaveAttribute('data-tick', '0');
    await page.locator('#pause').click();
    await expect(page.locator('#pause')).toHaveText('Pause');
    if (source === 'keyboard') await page.keyboard.press('Escape');
    else await page.locator('[data-input="escape"]').tap();
    await expect(page.locator('#game')).toHaveAttribute('data-scene', 'title.scn', { timeout: 15000 });
    await page.locator('#pause').click();
    expect(Number(await page.locator('#game').getAttribute('data-tick'))).toBeLessThan(390);
    await expect(page.locator('#status')).not.toHaveAttribute('data-error', 'true');
  });
}

for (const lifecycle of ['pagehide', 'visibilitychange']) test(`${lifecycle} releases an owned inventory drag and resumes without stuck input`, async ({ page }) => {
  await page.goto('/');
  await open(page, true, process.env.DISCWORLD_TEST_SLIDER_SAVE || path.join(root, 'assets/saves/slider-checkpoint.dws'));
  const canvas = page.locator('#game');
  const bounds = await canvas.boundingBox();
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Pause');
  await page.mouse.move(bounds.x + bounds.width * 189 / 320, bounds.y + bounds.height * 126 / 200);
  await page.mouse.down();
  await expect(canvas).toHaveAttribute('data-dragging', 'true');
  await page.evaluate(lifecycle => {
    if (lifecycle === 'pagehide') window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
    else {
      Object.defineProperty(document, 'hidden', { configurable: true, value: true });
      document.dispatchEvent(new Event('visibilitychange'));
    }
  }, lifecycle);
  await expect(page.locator('#pause')).toHaveText('Play');
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
  await page.waitForTimeout(150); // let an already submitted advance finish
  const tick = await canvas.getAttribute('data-tick');
  const pixels = await canvas.evaluate(c => c.toDataURL());
  await page.mouse.up();
  await page.waitForTimeout(150);
  await expect(canvas).toHaveAttribute('data-tick', tick);
  expect(await canvas.evaluate(c => c.toDataURL())).toBe(pixels);
  await page.evaluate(() => { delete document.hidden; });
  await page.locator('#pause').click();
  await expect.poll(async () => Number(await canvas.getAttribute('data-tick'))).toBeGreaterThan(Number(tick));
  await expect(canvas).toHaveAttribute('data-dragging', 'false');
});

test('tablet rotation preserves the paused frame and keeps landscape controls visible', async ({ page }) => {
  await page.goto('/');
  await open(page);
  for (const [width, height] of [[1024, 768], [768, 1024], [1133, 744], [1180, 820], [1024, 600]]) {
    await page.setViewportSize({ width, height });
    await expect(page.locator('#game')).toHaveAttribute('data-tick', '33200');
    await matchesOriginalFrame(page);
    const bounds = await page.locator('#game').boundingBox();
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
    expect(bounds.width / bounds.height).toBeCloseTo(1.6, 2);
    if (width > height) {
      const controls = page.locator('nav.controls');
      const box = await controls.boundingBox();
      expect(box.y + box.height).toBeLessThanOrEqual(height);
      for (const control of await controls.locator('button:visible, select:visible').all()) {
        const target = await control.boundingBox();
        expect(target.height).toBeGreaterThanOrEqual(44);
        expect(target.x).toBeGreaterThanOrEqual(0);
        expect(target.x + target.width).toBeLessThanOrEqual(width);
      }
    }
  }
});
