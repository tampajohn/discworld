import { test, expect } from '@playwright/test';
import fs from 'node:fs/promises';
import path from 'node:path';
const root = path.resolve(import.meta.dirname, '../..');
const iso = process.env.DISCWORLD_TEST_ISO || path.join(root, "Terry Pratchett's Discworld (1995)(Psygnosis).iso");
const font = process.env.DISCWORLD_TEST_SOUNDFONT || path.join(root, 'assets/soundfonts/TimGM6mb.sf2');
const save = process.env.DISCWORLD_TEST_SAVE || path.join(root, 'evidence/save-relaunch-inventory/checkpoint.dws');
const frame = process.env.DISCWORLD_TEST_FRAME || path.join(root, 'evidence/save-relaunch-inventory/checkpoint.png');
async function open(page, initial = true) {
  await page.locator('#iso').setInputFiles(iso);
  await page.locator('#font').setInputFiles(font);
  if (initial) await page.locator('#initial-save').setInputFiles(save);
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
      evidence.starts++; evidence.rate = this.buffer.sampleRate; evidence.channels = this.buffer.numberOfChannels;
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
  await page.locator('#save-local').click();
  await expect(page.locator('#status')).toHaveText('Saved on this device.');
  await page.locator('#mute').click();
  await expect(page.locator('#mute')).toHaveText('Sound on');
  await page.locator('#pause').click();
  await expect.poll(async () => Number(await page.locator('#game').getAttribute('data-tick'))).toBeGreaterThan(33205);
  await expect.poll(() => page.evaluate(() => window.audioEvidence.starts)).toBeGreaterThan(0);
  const audio = await page.evaluate(() => window.audioEvidence);
  expect(audio).toMatchObject({ finite: true, rate: 22050, channels: 2 });
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
  await page.touchscreen.tap(bounds.x + bounds.width * 20 / 320, bounds.y + bounds.height * 180 / 200);
  await page.waitForTimeout(250);
  await page.locator('#pause').click();
  await page.waitForTimeout(150);
  await page.screenshot({ path: path.join(root, `assets/previews/browser-${testInfo.project.name}-touch-close-inventory.png`) });
  expect(await exportedAfterOpeningDetails(page)).not.toEqual(original);
  expect(errors).toEqual([]);
});

async function exportedAfterOpeningDetails(page) {
  await page.locator('summary').click();
  return exported(page);
}

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
