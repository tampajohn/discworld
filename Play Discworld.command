#!/bin/zsh
set -euo pipefail
cd -- "$(dirname -- "$0")"
mkdir -p assets/logs
run_log="assets/logs/native-$(date +%Y%m%d-%H%M%S)-$$.log"
print -r -- "Session log: $PWD/$run_log"
cargo run --release -p discworld-native -- "Terry Pratchett's Discworld (1995)(Psygnosis).iso" assets/soundfonts/TimGM6mb.sf2 --room 2>&1 | tee "$run_log"
