// IndexedDB transactions publish complete native-compatible DWRSLOT records.
// Keep the original manual save while upgrading the database schema.
let codecPromise;
async function codec() {
  return codecPromise ??= import('./pkg/discworld_web.js').then(async module => {
    await module.default();
    return module;
  });
}
function database() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('discworld-player', 2);
    request.onupgradeneeded = () => {
      for (const name of ['saves', 'slots']) {
        if (!request.result.objectStoreNames.contains(name)) request.result.createObjectStore(name);
      }
    };
    request.onerror = () => reject(request.error);
    request.onsuccess = () => {
      request.result.onversionchange = () => request.result.close();
      resolve(request.result);
    };
  });
}
async function transaction(storeName, mode, work) {
  const db = await database();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(storeName, mode);
      let result, failure;
      const fail = error => { failure = error; tx.abort(); };
      tx.oncomplete = () => resolve(result);
      tx.onerror = tx.onabort = () => reject(failure || tx.error || new Error('Save transaction failed'));
      try { work(tx.objectStore(storeName), value => { result = value; }, fail); }
      catch (error) { fail(error); }
    });
  } finally { db.close(); }
}
export function deviceSave(bytes) {
  return transaction('saves', bytes ? 'readwrite' : 'readonly', (store, result) => {
    const request = bytes ? store.put(bytes, 'manual') : store.get('manual');
    request.onsuccess = () => result(request.result);
  });
}
function validId(id) {
  if (!Number.isInteger(id) || id < 0 || id >= 100) throw new Error('Slot ID outside capacity');
}
export async function writeSlot({ id = null, timestampMs, description, session }) {
  if (id !== null) validId(id);
  if (typeof timestampMs !== 'bigint' || timestampMs < 0n || timestampMs > 0xffffffffffffffffn) throw new Error('Invalid slot timestamp');
  const { encode_slot } = await codec(); // No async work inside the transaction.
  return transaction('slots', 'readwrite', (store, result, fail) => {
    const keys = store.getAllKeys();
    keys.onsuccess = () => {
      try {
        let selected = id;
        if (selected === null) {
          const occupied = new Set(keys.result);
          selected = Array.from({ length: 100 }, (_, i) => i).find(i => !occupied.has(i));
          if (selected === undefined) throw new Error('All 100 save slots are occupied');
        }
        const bytes = encode_slot(selected, timestampMs, description, session);
        store.put(bytes, selected);
        result(selected);
      } catch (error) { fail(error); }
    };
  });
}
export async function readSlot(id) {
  validId(id);
  const { decode_slot } = await codec();
  const bytes = await transaction('slots', 'readonly', (store, result) => {
    const request = store.get(id); request.onsuccess = () => result(request.result);
  });
  if (!bytes) throw new Error('Save slot does not exist');
  const slot = decode_slot(bytes);
  try {
    if (slot.id !== id) throw new Error('Slot ID does not match database key');
    return { id, timestampMs: slot.timestamp_ms, description: slot.description, session: slot.session, bytes };
  } finally { slot.free(); }
}
export async function listSlots() {
  const { decode_slot } = await codec();
  const { entries, rejected } = await transaction('slots', 'readonly', (store, result) => {
    // Decode one record per cursor callback; keep only metadata in the catalog.
    // Retaining all full snapshots here would multiply memory by slot count.
    const entries = [], rejected = [];
    const request = store.openCursor();
    request.onsuccess = () => {
      const cursor = request.result;
      if (!cursor) { result({ entries, rejected }); return; }
      const id = cursor.key;
      try {
        validId(id);
        const slot = decode_slot(cursor.value);
        try {
          if (slot.id !== id) throw new Error('Slot ID does not match database key');
          entries.push({ id, timestampMs: slot.timestamp_ms, description: slot.description });
        } finally { slot.free(); }
      } catch { rejected.push(id); }
      cursor.continue();
    };
  });
  entries.sort((a, b) => a.timestampMs === b.timestampMs ? a.id - b.id : a.timestampMs > b.timestampMs ? -1 : 1);
  return { entries, rejected };
}
