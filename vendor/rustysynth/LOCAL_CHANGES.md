# Local safe decoding patch

Upstream crate rustysynth 1.3.7, Git revision 4a778db849dfe0798c7afb1bd3c6882a5d2baa61. Source copied from the crates.io package; MIT license copied from that revision's repository root. Upstream copyright and license are retained.

Local changes replace the two unsafe sample-buffer byte casts with explicit little-endian conversion and checked even-length PCM input. The crate forbids unsafe code. No synthesis algorithms were changed. These changes are local and have not been submitted upstream.

This removes unsafe casts; it does not establish complete robustness against malicious SoundFonts, allocation exhaustion, or all malformed-file panics. Runtime compatibility and native/browser numerical equivalence require separate validation.

The synthesizer and its owned DSP components also derive `Clone` for exact
in-memory host checkpoints. This includes channel/voice state, oscillators,
envelopes, filters, LFOs, reverb/chorus buffers, and the partially consumed output
block. Only the immutable `Arc<SoundFont>` is shared; mutable vectors are cloned.
No synthesis algorithms were changed. This does not add durable serialization.
The discworld-audio ignored integration test exercises bit-exact continuation
with a locally supplied SoundFont, including sustain release and effect tails.
