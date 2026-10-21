// Developer-only full-route runner; normal player does not load this module.
import init, { GameUpload } from '../replay-pkg/discworld_web.js';
onmessage = async ({ data }) => {
  let game, upload;
  let tick = 0, position = 0;
  try {
    const { iso, font, trace, end } = data;
    if (!Number.isSafeInteger(end) || end <= 500 || end >= 2**32) throw Error('invalid end tick');
    const inputs = JSON.parse(await trace.text());
    if (!Array.isArray(inputs) || !inputs.every((v,i) => Number.isInteger(v.tick) && v.tick >= 0 && v.tick < end && (!i || v.tick > inputs[i-1].tick))) throw Error('invalid route ticks');
    if (iso.size <= 0 || iso.size > 1024 ** 3) throw Error('invalid ISO size');
    await init();
    upload = new GameUpload(iso.size);
    for (let offset = 0; offset < iso.size; offset += 1024 ** 2) {
      upload.append(new Uint8Array(await iso.slice(offset, offset + 1024 ** 2).arrayBuffer()));
    }
    const complete = upload; upload = null;
    game = complete.finish(new Uint8Array(await font.arrayBuffer()));
    const started = performance.now(), encoder = new TextEncoder(), scenes = [];
    let nextProgress = 100000, audioSamples = 0, nonfinite = 0;
    while (tick < end) {
      if (tick === 500) game.input('start');
      if (inputs[position]?.tick === tick) {
        game.replay_input(encoder.encode(JSON.stringify([inputs[position]]))); position++;
      }
      const boundary = Math.min(end, inputs[position]?.tick ?? end, tick < 500 ? 500 : end, nextProgress);
      const count = Math.min(24, boundary - tick);
      if (count <= 0) throw Error('replay made no progress');
      const audio = game.advance(count);
      for (const sample of audio) if (!Number.isFinite(sample)) nonfinite++;
      audioSamples += audio.length;
      tick = Number(game.tick());
      const scene = game.scene();
      if (scenes.at(-1)?.scene !== scene) scenes.push({ tick, scene });
      if (tick === nextProgress) {
        const frame = game.frame();
        postMessage({ progress: { tick, end, scene, inputs: position }, frame }, [frame.buffer]);
        nextProgress += 100000;
      }
    }
    if (position !== inputs.length || nonfinite) throw Error('route or audio validation failed');
    const frame = game.frame(), save = game.save();
    postMessage({ report: { ticks:tick, scene:game.scene(), has_restarted:game.has_restarted(),
      accepted_inputs:position, input_count:inputs.length, audio_samples:audioSamples,
      nonfinite_audio_samples:nonfinite, elapsed_seconds:(performance.now()-started)/1000,
      scene_history_scope:'scene sampled after batches of up to 24 ticks; intermediate scenes may be omitted', scenes },
      frame, save }, [frame.buffer, save.buffer]);
  } catch (error) {
    postMessage({ error: String(error), tick, accepted_inputs: position });
  } finally { if (upload) upload.free(); if (game) game.free(); }
};
