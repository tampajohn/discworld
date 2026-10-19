#!/usr/bin/env python3
"""Run the unmodified pinned reference and retain a normalized animation trace.

This is a live macOS smoke run, not a deterministic input replay or pixel test.
The window can receive user input. Audio uses a dummy device and MIDI is disabled.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
REVISION = "96b42d42bc8533953bdd701db059cc33c839a9ff"


def normalize(log):
    records, active = [], {}
    for line in log.splitlines():
        match = re.search(
            r"InitStepAnimScript Object=\((-?\d+),(-?\d+),([0-9a-f]+)h\) "
            r"script=([0-9a-f]+)h aniSpeed=(\d+) rec=(0x[0-9a-f]+)h", line
        )
        if match:
            x, y, frame, script, period, address = match.groups()
            record = dict(x=int(x), y=int(y), initial_frame=int(frame, 16),
                          script=int(script, 16), period=int(period), operations=[])
            active[address] = record
            records.append(record)
        match = re.search(r"DoNextFrame (0x[0-9a-f]+)h index=(\d+), op=([0-9a-f]+)h", line)
        if match:
            address, index, opcode = match.groups()
            if address in active:
                active[address]["operations"].append([int(index), int(opcode, 16)])
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=int, default=20)
    parser.add_argument("--normalize-only", action="store_true")
    args = parser.parse_args()
    if not 1 <= args.seconds <= 300:
        parser.error("seconds must be between 1 and 300")
    os.chdir(ROOT)
    console = Path(".reference/reference-console.log")
    report_path = Path("evidence/reference-smoke.json")
    if args.normalize_only:
        report = json.loads(report_path.read_text())
    else:
        command = [f".reference/scummvm-{REVISION}/scummvm",
                   "--config=.reference/reference.ini", "--logfile=.reference/reference-run.log",
                   "--path=assets/game", "--gfx-mode=opengl", "--no-fullscreen",
                   "--music-driver=null", "--debuglevel=5",
                   "--debugflags=animations,actions,sound,music", "--random-seed=1", "tinsel:dw"]
        env = dict(os.environ, SDL_VIDEODRIVER="cocoa", SDL_AUDIODRIVER="dummy")
        env.pop("SDL_RENDER_DRIVER", None)
        with console.open("w") as output:
            process = subprocess.Popen(command, env=env, stdout=output, stderr=subprocess.STDOUT)
            try:
                code = process.wait(timeout=args.seconds)
                outcome = "exited"
            except subprocess.TimeoutExpired:
                process.terminate()
                try:
                    code = process.wait(timeout=3)
                    outcome = "terminated_at_time_limit"
                except subprocess.TimeoutExpired:
                    process.kill()
                    code = process.wait()
                    outcome = "killed_after_termination_timeout"
        report = dict(command=command, seconds=args.seconds, outcome=outcome, exit_code=code,
                      video_driver="cocoa", audio_driver="dummy", renderer="opengl")
    raw = console.read_bytes()
    log = raw.decode(errors="replace")
    animations = normalize(log)
    report.update(revision=REVISION, console_sha256=hashlib.sha256(raw).hexdigest(),
                  animation_instances=len(animations),
                  animation_operations=sum(len(a["operations"]) for a in animations),
                  game_execution_observed=any(a["operations"] for a in animations),
                  input_events_observed=bool(re.search(r"PLR_\w+", log)),
                  scope="Live startup smoke run; no deterministic replay, frame, audio, or timing comparison")
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    Path("evidence/reference-animations.json").write_text(json.dumps(animations, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if not report["game_execution_observed"]:
        raise SystemExit("Reference did not produce an animation trace")


if __name__ == "__main__":
    main()
