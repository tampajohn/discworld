# Local safe decoding patch

Upstream crate rustysynth 1.3.7, Git revision 4a778db849dfe0798c7afb1bd3c6882a5d2baa61. Source copied from the crates.io package; MIT license copied from that revision's repository root. Upstream copyright and license are retained.

Local changes replace the two unsafe sample-buffer byte casts with explicit little-endian conversion and checked even-length PCM input. The crate forbids unsafe code. No synthesis algorithms were changed. These changes are local and have not been submitted upstream.

This removes unsafe casts; it does not establish complete robustness against malicious SoundFonts, allocation exhaustion, or all malformed-file panics. Runtime compatibility and native/browser numerical equivalence require separate validation.
