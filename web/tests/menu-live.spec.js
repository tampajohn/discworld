import { test, expect } from '@playwright/test';
import path from 'node:path';
const root = path.resolve(import.meta.dirname, '../..');

for (const touch of [false, true]) test(`original menu ${touch ? 'touch keyboard' : 'desktop keyboard'} saves and reloads through IndexedDB`, async ({ page }, info) => {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('/');
  await page.locator('#iso').setInputFiles(process.env.DISCWORLD_TEST_ISO || path.join(root, "Terry Pratchett's Discworld (1995)(Psygnosis).iso"));
  await page.locator('#font').setInputFiles(process.env.DISCWORLD_TEST_SOUNDFONT || path.join(root, 'assets/soundfonts/TimGM6mb.sf2'));
  await page.locator('#initial-save').setInputFiles(path.join(root, 'assets/saves/rightclick-node-exit-stopped.dws'));
  await page.locator('#open').click();
  await expect(page.locator('#player')).toBeVisible({ timeout: 60000 });
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Pause');
  await page.locator('#game').focus();
  if (touch) await page.locator('#options').tap();
  else await page.keyboard.press('F5');
  await expect(page.locator('#game')).toHaveAttribute('data-menu', 'true');
  const click = async (x, y) => {
    const bounds = await page.locator('#game').boundingBox();
    const point = [bounds.x + x * bounds.width / 320, bounds.y + y * bounds.height / 200];
    if (touch) await page.touchscreen.tap(...point);
    else await page.mouse.click(...point);
  };
  if (touch) {
    await click(90, 59); // Main -> Save
    await expect(page.locator('#game')).toHaveAttribute('data-editing', 'true');
    await page.locator('#name-keyboard').tap();
    await expect(page.locator('#name-input')).toBeFocused();
    // insertText exercises the input path used by virtual keyboards without keydown.
    await page.keyboard.insertText('Browser tesX');
    await page.keyboard.press('Backspace');
    await page.keyboard.insertText('t');
  } else {
    await page.keyboard.type('Browser tesX');
    await page.keyboard.press('Backspace');
    await page.keyboard.type('t');
  }
  await page.locator('#game').screenshot({ path: path.join(root, `assets/previews/browser-${info.project.name}-${touch ? 'touch' : 'desktop'}-save-name.png`) });
  await click(262, 69);
  await expect(page.locator('#status')).toHaveText('Game saved on this device.');
  const saved = await page.evaluate(async () => {
    const storage = await import('/storage.js');
    const catalog = await storage.listSlots();
    const slot = await storage.readSlot(catalog.entries[0].id);
    return { id: slot.id, description: [...slot.description] };
  });
  expect(new TextDecoder().decode(new Uint8Array(saved.description))).toContain('Browser test');
  await expect(page.locator('#game')).toHaveAttribute('data-menu', 'false');
  if (touch) await page.locator('#options').tap();
  else await page.keyboard.press('F7');
  await expect(page.locator('#game')).toHaveAttribute('data-menu', 'true');
  if (touch) {
    await click(90, 44); // Main -> Load
    await expect(page.locator('#game')).toHaveAttribute('data-menu-page', 'Load');
  }
  await click(262, 69);
  await expect(page.locator('#status')).toHaveText('Saved game loaded.');
  await expect(page.locator('#game')).toHaveAttribute('data-menu', 'false');
  expect(errors).toEqual([]);
});
