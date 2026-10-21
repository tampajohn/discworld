import { deviceSave } from './storage.js';
import { advanceClock } from './clock.js';
const $ = id => document.getElementById(id);
const canvas = $('game'), context = canvas.getContext('2d', { alpha: false });
const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
let serial = 0, ready = false, paused = true, muted = true, epoch = 0;
let dragging = false;
let busy = false, previous = performance.now(), elapsed = 0;
let audioContext, gain, audioEnd = 0;
const sources = new Set(), pending = new Map();
function status(text, error = false) {
  $('status').textContent = text;
  $('status').dataset.error = String(error);
}
function rpc(type, data = {}, transfer = []) {
  return new Promise((resolve, reject) => {
    const id = ++serial;
    pending.set(id, { resolve, reject });
    worker.postMessage({ id, type, ...data }, transfer);
  });
}
worker.onmessage = ({ data }) => {
  const request = pending.get(data.id);
  if (!request) return;
  pending.delete(data.id);
  if (data.error) request.reject(new Error(data.error)); else request.resolve(data.result);
};
worker.onerror = event => {
  pause();
  for (const request of pending.values()) request.reject(new Error(event.message));
  pending.clear();
  status(`Player stopped: ${event.message}`, true);
};
function draw(result) {
  if (result.dragging !== undefined) { dragging = result.dragging; canvas.dataset.dragging = String(dragging); }
  if (result.frame) context.putImageData(new ImageData(new Uint8ClampedArray(result.frame), 320, 200), 0, 0);
  if (result.tick !== undefined) { canvas.dataset.tick = result.tick; canvas.dataset.scene = result.scene; }
}
function clearAudio() {
  for (const source of sources) { source.stop(); source.disconnect(); }
  sources.clear(); audioEnd = 0;
}
function pause() {
  paused = true; epoch++; elapsed = 0;
  $('pause').textContent = 'Play';
  clearAudio();
}
async function enableAudio() {
  if (!audioContext) {
    audioContext = new AudioContext();
    gain = audioContext.createGain(); gain.connect(audioContext.destination);
    gain.gain.value = muted ? 0 : 1;
    audioContext.onstatechange = () => {
      if (ready && !paused && audioContext.state !== 'running') {
        pause(); status('Audio interrupted. Press Play to resume.');
      }
    };
  }
  await audioContext.resume();
}
function presentAudio(samples) {
  if (!samples?.length || !audioContext || audioContext.state !== 'running') return;
  const buffer = audioContext.createBuffer(2, samples.length / 2, 22050);
  for (let channel = 0; channel < 2; channel++) {
    const values = buffer.getChannelData(channel);
    for (let i = 0; i < values.length; i++) values[i] = samples[i * 2 + channel];
  }
  const now = audioContext.currentTime;
  // Bound presentation latency after a slow frame or device interruption.
  if (audioEnd > now + .3) clearAudio();
  const start = Math.max(audioEnd, now + .04);
  const source = audioContext.createBufferSource();
  source.buffer = buffer; source.connect(gain); sources.add(source);
  source.onended = () => { sources.delete(source); source.disconnect(); };
  source.start(start); audioEnd = start + buffer.duration;
}
async function tick(now) {
  const delta = now - previous; previous = now;
  if (ready && !paused) {
    const planned = advanceClock(elapsed, delta, busy);
    elapsed = planned.elapsed;
    const ticks = planned.ticks;
    if (ticks && !busy) {
      busy = true;
      const generation = epoch;
      try {
        const result = await rpc('advance', { ticks });
        draw(result);
        if (!paused && generation === epoch) presentAudio(result.audio);
      } catch (error) { pause(); status(`Player stopped: ${error.message}`, true); }
      finally { busy = false; }
    }
  } else elapsed = 0;
  requestAnimationFrame(tick);
}
requestAnimationFrame(tick);
$('open-game').onsubmit = async event => {
  event.preventDefault(); $('open').disabled = true; status('Opening your game files…');
  try {
    const save = $('initial-save').files[0];
    if (save && save.size > 24 * 1024 * 1024 + 52) throw new Error('Save exceeds the size limit');
    draw(await rpc('open', { iso: $('iso').files[0], font: $('font').files[0], save }));
    ready = true; $('setup').hidden = true; $('player').hidden = false;
    status('Ready. Press Play to begin; sound is off.');
  } catch (error) { status(error.message, true); $('open').disabled = false; }
};
$('pause').onclick = async () => {
  if (!paused) { pause(); status('Paused.'); return; }
  try { await enableAudio(); paused = false; previous = performance.now(); elapsed = 0; $('pause').textContent = 'Pause'; status('Playing.'); }
  catch (error) { status(`Could not start audio: ${error.message}`, true); }
};
$('mute').onclick = async () => {
  try {
    await enableAudio(); muted = !muted; gain.gain.value = muted ? 0 : 1;
    $('mute').textContent = muted ? 'Sound off' : 'Sound on'; $('mute').setAttribute('aria-pressed', String(muted));
  } catch (error) { status(error.message, true); }
};
async function input(command, point) {
  if (!ready || (paused && !command.endsWith('-up'))) return;
  // Release ownership while paused, without moving the cursor/window. Match
  // the native host, which also ignores motion until simulation resumes.
  if (paused) point = undefined;
  try { draw(await rpc('input', { command, point })); }
  catch (error) { pause(); status(error.message, true); }
}
function point(event) {
  const rect = canvas.getBoundingClientRect();
  return [Math.floor((event.clientX - rect.left) * 320 / rect.width), Math.floor((event.clientY - rect.top) * 200 / rect.height)];
}
const pressedPointers = new Map();
canvas.oncontextmenu = event => event.preventDefault();
canvas.onpointerdown = event => {
  event.preventDefault(); canvas.focus();
  if (!ready || paused || ![0, 2].includes(event.button)) return;
  const action = $('action').value;
  if (action !== 'click' && event.button === 0) { input(action, point(event)); return; }
  const button = event.button === 2 ? 'right' : 'left';
  pressedPointers.set(event.pointerId, button);
  canvas.setPointerCapture(event.pointerId);
  input(`${button}-down`, point(event));
};
function releasePointer(event) {
  const button = pressedPointers.get(event.pointerId) || (dragging && event.button === 0 ? 'left' : null);
  if (!button) return;
  pressedPointers.delete(event.pointerId);
  input(`${button}-up`, point(event));
}
canvas.onpointerup = releasePointer;
canvas.onpointercancel = releasePointer;
canvas.onlostpointercapture = releasePointer;
let pointerPending = false;
canvas.onpointermove = async event => {
  if (!ready || paused || pointerPending || (event.pointerType === 'touch' && !pressedPointers.has(event.pointerId))) return;
  pointerPending = true;
  try { draw(await rpc('pointer', { point: point(event) })); }
  catch (error) { status(error.message, true); }
  finally { pointerPending = false; }
};
canvas.onwheel = event => { event.preventDefault(); input(event.deltaY < 0 ? 'up' : 'down'); };
for (const button of document.querySelectorAll('[data-input]')) button.onclick = () => input(button.dataset.input);
document.onkeydown = event => {
  if (!ready || /^(INPUT|SELECT|TEXTAREA)$/.test(event.target.tagName)) return;
  const key = event.key.toLowerCase();
  if (key === 'p') { event.preventDefault(); $('pause').click(); return; }
  if (key === 'm') { event.preventDefault(); $('mute').click(); return; }
  const command = { w: 'walk', a: 'action', l: 'look', i: 'pockets', backspace: 'close', ' ': 'continue', arrowup: 'up', arrowdown: 'down' }[key];
  if (command) { event.preventDefault(); input(command); }
};
document.onvisibilitychange = () => { if (ready && document.hidden) { pause(); status('Paused while the page is hidden.'); } };
window.addEventListener('pagehide', pause);

async function restore(bytes) {
  pause();
  draw(await rpc('load', { bytes }));
  status('Saved game loaded. Press Play to resume.');
}
function operation(id, action) {
  $(id).onclick = async () => {
    $(id).disabled = true;
    try { await action(); } catch (error) { status(error.message, true); }
    finally { $(id).disabled = false; }
  };
}
operation('save-local', async () => {
  const { bytes } = await rpc('save'); await deviceSave(bytes); status('Saved on this device.');
});
operation('load-local', async () => {
  const bytes = await deviceSave(); if (!bytes) throw new Error('No device save yet'); await restore(bytes);
});
operation('export', async () => {
  const { bytes } = await rpc('save');
  const url = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
  const link = document.createElement('a'); link.href = url; link.download = 'discworld-save.dws'; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 60000); status('Save exported.');
});
$('import').onchange = async () => {
  try {
    const file = $('import').files[0]; if (!file) return;
    if (file.size > 24 * 1024 * 1024 + 52) throw new Error('Save exceeds the size limit');
    await restore(await file.arrayBuffer());
  } catch (error) { status(`Could not load save: ${error.message}`, true); }
  finally { $('import').value = ''; }
};
