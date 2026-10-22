// Continuous stereo conversion from the engine's 22050 Hz clock to the device
// clock. Keep interpolation history across worker replies; independent WebAudio
// buffer sources must not each resample the beginning/end of a speech fragment.
export class AudioResampler {
  constructor(rate) {
    if (!Number.isSafeInteger(rate) || rate < 8000 || rate > 384000) throw new Error('Invalid audio device rate');
    this.rate = rate;
    this.reset();
  }
  reset() { this.tail = new Float32Array(0); this.position = 0; }
  convert(samples) {
    if (samples.length % 2 || samples.length > 22050 * 2 * 2) throw new Error('Invalid stereo audio block');
    if (!samples.every(Number.isFinite)) throw new Error('Non-finite audio sample');
    const data = new Float32Array(this.tail.length + samples.length);
    data.set(this.tail); data.set(samples, this.tail.length);
    const frames = data.length / 2;
    const end = Math.max(0, frames - 1) * this.rate;
    const count = Math.max(0, Math.ceil((end - this.position) / 22050));
    const output = new Float32Array(count * 2);
    let position = this.position;
    for (let frame = 0; frame < count; frame++, position += 22050) {
      const index = Math.floor(position / this.rate), fraction = (position % this.rate) / this.rate;
      for (let channel = 0; channel < 2; channel++) {
        const at = index * 2 + channel;
        output[frame * 2 + channel] = data[at] + (data[at + 2] - data[at]) * fraction;
      }
    }
    // Retain the final frame for interpolation into the next block. Downsampling
    // may skip beyond it; carry that fractional/integer offset instead.
    const consumed = Math.min(frames, Math.floor(position / this.rate));
    this.tail = data.slice(consumed * 2);
    this.position = position - consumed * this.rate;
    return output;
  }
}
