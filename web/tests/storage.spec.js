import { test, expect } from '@playwright/test';
import fs from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
const root = path.resolve(import.meta.dirname, '../..');
const checkpoint = path.join(root, 'evidence/save-relaunch-inventory/checkpoint.dws');
test.beforeEach(async ({ page }) => {
  await page.route('**/slot-test-session.dws', async route => route.fulfill({ body: await fs.readFile(checkpoint) }));
  await page.goto('/');
});
test('manual v1 save survives upgrade; slots persist complete records across reload', async ({ page }, info) => {
  await page.evaluate(async () => {
    await new Promise((resolve, reject) => {
      const r = indexedDB.open('discworld-player', 1);
      r.onupgradeneeded = () => r.result.createObjectStore('saves');
      r.onerror = () => reject(r.error);
      r.onsuccess = () => {
        const db = r.result, tx = db.transaction('saves', 'readwrite');
        tx.objectStore('saves').put(new Uint8Array([4, 2, 7]), 'manual');
        tx.oncomplete = () => { db.close(); resolve(); };
      };
    });
    const storage = await import('/storage.js');
    const session = new Uint8Array(await (await fetch('/slot-test-session.dws')).arrayBuffer());
    await storage.writeSlot({ timestampMs: 1234n, description: new TextEncoder().encode('Library'), session });
  });
  await page.reload();
  const result = await page.evaluate(async () => {
    const storage = await import('/storage.js');
    const slot = await storage.readSlot(0);
    const digest = await crypto.subtle.digest('SHA-256', slot.session);
    return { manual: [...await storage.deviceSave()], description: [...slot.description], timestamp: String(slot.timestampMs),
      digest: [...new Uint8Array(digest)].map(n => n.toString(16).padStart(2, '0')).join(''), bytes: [...slot.bytes] };
  });
  expect(result.manual).toEqual([4, 2, 7]);
  expect(Buffer.from(result.description).toString()).toBe('Library');
  expect(result.timestamp).toBe('1234');
  expect(result.digest).toBe(createHash('sha256').update(await fs.readFile(checkpoint)).digest('hex'));
  await fs.writeFile(path.join(root, `assets/saves/browser-${info.project.name}-slot.dwslot`), Buffer.from(result.bytes));
});
test('transactions allocate unique slots and rejected writes preserve previous data', async ({ page, context }) => {
  const other = await context.newPage();
  await other.route('**/slot-test-session.dws', async route => route.fulfill({ body: await fs.readFile(checkpoint) }));
  await other.goto('/');
  const allocate = p => p.evaluate(async () => {
    const storage = await import('/storage.js');
    const session = new Uint8Array(await (await fetch('/slot-test-session.dws')).arrayBuffer());
    return Promise.all(Array.from({ length: 4 }, (_, i) => storage.writeSlot({ timestampMs: BigInt(i), description: new Uint8Array([65+i]), session })));
  });
  const ids = (await Promise.all([allocate(page), allocate(other)])).flat().sort((a,b) => a-b);
  expect(ids).toEqual([0,1,2,3,4,5,6,7]);
  const result = await page.evaluate(async () => {
    const storage = await import('/storage.js');
    const before = await storage.readSlot(0);
    let rejected = false;
    try { await storage.writeSlot({ id: 0, timestampMs: 999n, description: new Uint8Array([0]), session: before.session }); }
    catch { rejected = true; }
    const after = await storage.readSlot(0);
    await new Promise((resolve, reject) => {
      const r = indexedDB.open('discworld-player', 2);
      r.onerror = () => reject(r.error);
      r.onsuccess = () => {
        const db = r.result, tx = db.transaction('slots', 'readwrite');
        tx.objectStore('slots').put(new Uint8Array([1]), 8);
        tx.oncomplete = () => { db.close(); resolve(); };
      };
    });
    const allocated = await storage.writeSlot({ timestampMs: 1000n, description: new Uint8Array([90]), session: before.session });
    const catalog = await storage.listSlots();
    return { rejected, preserved: before.bytes.every((b,i) => b === after.bytes[i]), allocated,
      bad: catalog.rejected, order: catalog.entries.map(e => e.id), times: catalog.entries.map(e => Number(e.timestampMs)) };
  });
  expect(result.rejected && result.preserved).toBe(true);
  expect(result.allocated).toBe(9);
  expect(result.bad).toEqual([8]);
  expect(result.order[0]).toBe(9);
  expect(result.times).toEqual([...result.times].sort((a,b) => b-a));
});
