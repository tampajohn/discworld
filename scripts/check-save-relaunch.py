#!/usr/bin/env python3
"""Verify a real save/relaunch against uninterrupted play using separate processes."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import wave

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("iso")
parser.add_argument("soundfont")
parser.add_argument("trace")
parser.add_argument("output", type=Path)
parser.add_argument("--checkpoint", type=int, default=10000)
parser.add_argument("--end", type=int, default=12000)
args = parser.parse_args()
if not 0 < args.checkpoint < args.end <= 100000:
    parser.error("require 0 < checkpoint < end <= 100000 (speech WAV capture limit)")
args.output.mkdir(parents=True, exist_ok=False)
inputs = json.loads(Path(args.trace).read_text())
for name, end in [("checkpoint", args.checkpoint), ("final", args.end)]:
    (args.output / f"{name}-input.json").write_text(json.dumps([i for i in inputs if i["tick"] < end], indent=2) + "\n")

def run(name, end, trace, extra, audio=True, expected=0):
    command = ["target/release/discworld-tools", "run-headless", args.iso, str(end),
               str(args.output / f"{name}.png"), str(args.output / f"{name}.wav") if audio else "-",
               args.soundfont, "500", str(args.output / f"{trace}-input.json"), *map(str, extra)]
    with (args.output / f"{name}.json").open("x") as out, (args.output / f"{name}.log").open("x") as err:
        result = subprocess.run(command, stdout=out, stderr=err, check=False)
    if result.returncode != expected:
        raise RuntimeError(f"{name}: exit {result.returncode}; see its log")

run("baseline", args.end, "final", ["--save", args.output / "baseline.dws"])
run("checkpoint", args.checkpoint, "checkpoint", ["--save", args.output / "checkpoint.dws"], audio=False)
run("resumed", args.end, "final", ["--load", args.output / "checkpoint.dws", "--save", args.output / "resumed.dws"])
assert (args.output / "baseline.dws").read_bytes() == (args.output / "resumed.dws").read_bytes(), "engine/DSP state differs"
assert (args.output / "baseline.png").read_bytes() == (args.output / "resumed.png").read_bytes(), "frame differs"
with wave.open(str(args.output / "baseline.wav"), "rb") as baseline, wave.open(str(args.output / "resumed.wav"), "rb") as resumed:
    assert baseline.getnchannels() == resumed.getnchannels() == 1
    baseline.setpos(args.checkpoint * baseline.getframerate() // 24)
    assert baseline.readframes(baseline.getnframes()) == resumed.readframes(resumed.getnframes()), "speech PCM differs"

# Repair both checksums after changing only the DSP cursor: clock validation must reject it.
bad = bytearray((args.output / "checkpoint.dws").read_bytes())
engine_len = struct.unpack_from("<I", bad, 12)[0]
audio_start = 52 + engine_len
sample = struct.unpack_from("<Q", bad, audio_start + 48)[0]
struct.pack_into("<Q", bad, audio_start + 48, sample + 1)
bad[audio_start + 60:audio_start + 92] = hashlib.sha256(bad[audio_start:audio_start + 60] + bad[audio_start + 92:]).digest()
bad[20:52] = hashlib.sha256(bad[:20] + bad[52:]).digest()
(args.output / "mismatched.dws").write_bytes(bad)
run("mismatched", args.end, "final", ["--load", args.output / "mismatched.dws"], audio=False, expected=1)
assert "saved engine and audio clocks disagree" in (args.output / "mismatched.log").read_text()
report = {"checkpoint_tick": args.checkpoint, "final_tick": args.end,
          "separate_processes": True, "engine_and_dsp_bytes_equal": True,
          "frame_bytes_equal": True, "speech_pcm_equal": True, "mismatched_clocks_rejected": True,
          "save_bytes": (args.output / "checkpoint.dws").stat().st_size}
(args.output / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
