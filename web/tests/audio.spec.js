import { test, expect } from '@playwright/test';

test('WebAudio device output is independent of engine block boundaries', async ({ page }) => {
  await page.goto('/');
  const evidence = await page.evaluate(async () => {
    const { AudioResampler } = await import('/audio.js');
    const wave = Float32Array.from({length: 22050 * 2}, (_, i) => Math.sin(Math.floor(i / 2) * 2 * Math.PI * 997 / 22050) * (i % 2 ? -.5 : .5));
    async function render(rate, split, convert) {
      const context = new OfflineAudioContext(2, rate, rate), resampler = new AudioResampler(rate);
      let end = 0;
      for (let part = 0; part < (split ? 24 : 1); part++) {
        const count = split ? 24 : 1;
        let samples = wave.subarray(Math.floor(part * 22050 / count) * 2, Math.floor((part + 1) * 22050 / count) * 2);
        if (convert) samples = resampler.convert(samples);
        const buffer = context.createBuffer(2, samples.length / 2, convert ? rate : 22050);
        for (let channel = 0; channel < 2; channel++) {
          const output = buffer.getChannelData(channel);
          for (let i = 0; i < output.length; i++) output[i] = samples[i * 2 + channel];
        }
        const source = context.createBufferSource();
        source.buffer = buffer; source.connect(context.destination); source.start(end);
        end += buffer.duration;
      }
      return (await context.startRendering()).getChannelData(0);
    }
    const results = [];
    for (const rate of [44100, 48000]) {
      const oldWhole = await render(rate, false, false), oldSplit = await render(rate, true, false);
      const whole = await render(rate, false, true), split = await render(rate, true, true);
      const different = (a, b) => a.reduce((n, value, i) => n + (Math.abs(value - b[i]) > 1e-6 ? 1 : 0), 0);
      results.push({rate, oldDifferences: different(oldWhole, oldSplit), differences: different(whole, split)});
    }
    return results;
  });
  console.log(JSON.stringify(evidence));
  for (const result of evidence) {
    expect(result.oldDifferences).toBeGreaterThan(0);
    expect(result.differences).toBe(0);
  }
});
