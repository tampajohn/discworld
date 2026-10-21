#!/bin/zsh
set -euo pipefail
cd "${0:A:h}"
mkdir -p assets/logs
cargo run --release -p discworld-native -- "Terry Pratchett's Discworld (1995)(Psygnosis).iso" assets/soundfonts/TimGM6mb.sf2 --replay evidence/act3-completed-picture-input.json --from-tick 8500 --mute 2>&1 | tee "assets/logs/replay-$(date +%Y%m%d-%H%M%S)-$$.log"
