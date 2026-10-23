#!/usr/bin/env python3
"""Capture a controlled hover diagnostic from the untouched pinned reference."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import signal
import subprocess

ROOT = Path(__file__).resolve().parent.parent
REVISION = '96b42d42bc8533953bdd701db059cc33c839a9ff'
BINARY_SHA256 = '1e8b098f0e9f6da44062aa19c195a5771a4a5dbc806cb3d3a9c3eb0e8fb6f524'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new diagnostic directory')
    parser.add_argument('--data', type=Path, default=ROOT / 'assets/game')
    args = parser.parse_args()
    binary = ROOT / f'.reference/scummvm-{REVISION}/scummvm'
    if digest(binary) != BINARY_SHA256:
        parser.error('reference binary differs from the validated pinned build')
    output = args.output.resolve()
    output.mkdir()  # Never overwrite an earlier attempt.
    config = output / 'reference.ini'
    # A CLI --gfx-mode transaction can call internUpdateScreen before the
    # initial SDL source surface exists. Select it before initBackend instead.
    config.write_text('[scummvm]\ngfx_mode=surfacesdl\nscaler=normal\n'
                      'scale_factor=1\naspect_ratio=false\n')
    probe = ROOT / 'tools/reference_hover_lldb.py'
    commands = [
        'settings set target.disable-aslr false',
        'command script import ' + shlex.quote(str(probe)),
        'breakpoint set --shlib scummvm --name error',
        'run', 'script reference_hover_lldb.drive(lldb.debugger)',
        'script reference_hover_lldb.finish(lldb.debugger)', 'process kill',
    ]
    command = ['lldb', '--batch']
    for item in commands:
        command.extend(['-o', item])
    command.extend(['--', str(binary), f'--config={config}',
                    f'--logfile={output}/reference.log', f'--path={args.data.resolve()}',
                    '--no-fullscreen', '--music-driver=null', '--debuglevel=5',
                    '--debugflags=actions', '--random-seed=1', 'tinsel:dw'])
    environment = dict(os.environ, SDL_VIDEODRIVER='dummy', SDL_RENDER_DRIVER='software',
                       SDL_AUDIODRIVER='dummy', DISCWORLD_REFERENCE_FRAMES=str(output / 'frames'),
                       DISCWORLD_REFERENCE_FRAME_LIMIT='72', DISCWORLD_REFERENCE_BACKEND='surfacesdl')
    metadata = dict(reference_revision=REVISION, binary_sha256=BINARY_SHA256,
                    probe_sha256=digest(probe), config_sha256=digest(config), command=command,
                    scope='Controlled scene and logical cursor injection; not ordinary input acceptance.')
    metadata_path = output / 'launch.json'
    with (output / 'console.log').open('w') as log:
        process = subprocess.Popen(command, cwd=ROOT, env=environment, stdout=log,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        metadata.update(status='running', pid=process.pid)
        metadata_path.write_text(json.dumps(metadata, indent=2) + '\n')
        print(json.dumps(dict(pid=process.pid, output=str(output))), flush=True)
        try:
            metadata.update(status='exited', exit_code=process.wait(timeout=300))
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            metadata.update(status='timed_out', exit_code=process.returncode)
    capture = output / 'frames/capture.json'
    report = json.loads(capture.read_text()) if capture.exists() else {}
    frames = report.get('frames', [])
    points = [[50, 120], [147, 110], [294, 56], [250, 160], [149, 60], [10, 10]]
    accepted = (metadata.get('exit_code') == 0 and report.get('complete') is True
                and report.get('errors') == [] and len(frames) == 72
                and len(report.get('injected_actions', [])) == 8
                and all(f['scene'] == 18 << 23 and f['driver_cursor'] == points[i // 12]
                        for i, f in enumerate(frames)))
    metadata['accepted'] = accepted
    metadata_path.write_text(json.dumps(metadata, indent=2) + '\n')
    print(json.dumps(dict(status=metadata['status'], accepted=accepted, frames=len(frames))))
    raise SystemExit(0 if accepted else 1)


if __name__ == '__main__':
    main()
