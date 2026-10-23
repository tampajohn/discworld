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
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--inventory', action='store_true', help='dispatch original lead WALKTO and close inventory halfway through')
    mode.add_argument('--inventory-drag', action='store_true', help='open inventory, drag its header, release and close')
    mode.add_argument('--inventory-resize', action='store_true', help='directly open Luggage inventory, resize its corner, release and close')
    mode.add_argument('--inventory-fixed-corner', action='store_true', help='attempt to resize fixed-size pockets, release and close')
    mode.add_argument('--look', action='store_true', help='dispatch original lead LOOK with speech and subtitles')
    mode.add_argument('--walk', action='store_true', help='send a player WALKTO on the room floor')
    mode.add_argument('--walk-from-actor', action='store_true', help='move off a cached actor and send WALKTO immediately')
    parser.add_argument('--walk-delay', type=int, choices=[0, 1], default=1, help='cycles between cursor injection and WALKTO')
    parser.add_argument('--resize-corner', choices=['bottom-right', 'top-left'], default='bottom-right', help='corner used by --inventory-resize')
    parser.add_argument('--resize-delta', type=int, choices=[-26, 26], default=26, help='signed size change for --inventory-resize')
    parser.add_argument('--frames', type=int, default=72, help='capture 72..240 frames')
    args = parser.parse_args()
    if (args.resize_corner != 'bottom-right' or args.resize_delta != 26) and not args.inventory_resize:
        parser.error('resize options require --inventory-resize')
    if args.inventory_fixed_corner or args.inventory_resize:
        args.inventory_drag = True
    if args.inventory_drag:
        args.inventory = True
    if args.walk_from_actor:
        args.walk = True
        args.walk_delay = 1
    if not 72 <= args.frames <= 240:
        parser.error('frames must be between 72 and 240')
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
    if args.look:
        with config.open('a') as file:
            file.write('subtitles=true\nspeech_mute=false\n')
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
                       DISCWORLD_REFERENCE_FRAME_LIMIT=str(args.frames), DISCWORLD_REFERENCE_BACKEND='surfacesdl')
    environment['DISCWORLD_REFERENCE_INVENTORY'] = '1' if args.inventory else '0'
    environment['DISCWORLD_REFERENCE_LOOK'] = '1' if args.look else '0'
    environment['DISCWORLD_REFERENCE_WALK'] = '1' if args.walk else '0'
    environment['DISCWORLD_REFERENCE_WALK_DELAY'] = str(args.walk_delay)
    environment['DISCWORLD_REFERENCE_WALK_FROM_ACTOR'] = '1' if args.walk_from_actor else '0'
    environment['DISCWORLD_REFERENCE_INVENTORY_DRAG'] = '1' if args.inventory_drag else '0'
    environment['DISCWORLD_REFERENCE_INVENTORY_FIXED_CORNER'] = '1' if args.inventory_fixed_corner else '0'
    environment['DISCWORLD_REFERENCE_INVENTORY_RESIZE'] = '1' if args.inventory_resize else '0'
    environment['DISCWORLD_REFERENCE_RESIZE_CORNER'] = args.resize_corner
    environment['DISCWORLD_REFERENCE_RESIZE_DELTA'] = str(args.resize_delta)
    metadata = dict(resize_delta=args.resize_delta, resize_corner=args.resize_corner, inventory_resize=args.inventory_resize, inventory_fixed_corner=args.inventory_fixed_corner, inventory_drag=args.inventory_drag, walk_from_actor=args.walk_from_actor, walk_delay=args.walk_delay, walk_diagnostic=args.walk, frame_limit=args.frames, look_diagnostic=args.look, inventory_diagnostic=args.inventory, reference_revision=REVISION, binary_sha256=BINARY_SHA256,
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
    if args.walk:
        points[0] = [250, 180]
    if args.walk_from_actor:
        points[0] = [200, 140]
    if args.inventory_drag:
        points = [[50, 120], [140, 110], [170, 130], [180, 135], [10, 10], [10, 10]]
    if args.inventory_fixed_corner:
        points = [[50, 120], [169, 147], [189, 167], [189, 167], [10, 10], [10, 10]]
    if args.inventory_resize:
        corner = [100, 100] if args.resize_corner == 'top-left' else [194, 172]
        direction = -1 if args.resize_corner == 'top-left' else 1
        moved = [p + direction * args.resize_delta for p in corner]
        points = [[50, 120], corner, moved, moved, [10, 10], [10, 10]]
    # After the six injected movement blocks, LOOK can restore the saved cursor
    # itself. Retain that position as observed output rather than requiring a
    # constant driver position after all controlled input has ended.
    accepted = (metadata.get('exit_code') == 0 and report.get('complete') is True
                and report.get('errors') == [] and len(frames) == args.frames
                and len(report.get('injected_actions', [])) == (13 if args.inventory_drag else 10 if args.inventory or args.walk_from_actor else 9 if args.look or args.walk else 8)
                and all(f['scene'] == 18 << 23
                        and ((args.look and i >= 72)
                             or f['driver_cursor'] == ([250, 180] if args.walk_from_actor and 1 <= i < 12
                                                      else points[min(i // 12, len(points) - 1)]))
                        for i, f in enumerate(frames)))
    if accepted and args.inventory:
        accepted = (frames[12]['inventory'] == [2, 1 if args.inventory_resize else 2]
                    and frames[48]['inventory'][0] == 1)
    if accepted and args.inventory_drag:
        expected_drag = (0 if args.inventory_fixed_corner else
                         (7 if args.resize_corner == 'top-left' else 10) if args.inventory_resize else 1)
        accepted = (frames[12].get('inventory_dragging') == expected_drag
                    and frames[24].get('inventory_dragging') == expected_drag
                    and frames[36].get('inventory_dragging') == 0)
    if accepted and args.look:
        accepted = any(e.get('kind') == 'sample_pcm' and e.get('type') == 3
                       and e['cycle'] >= report['hover_start_cycle']
                       for e in report.get('events', []))
    metadata['accepted'] = accepted
    metadata_path.write_text(json.dumps(metadata, indent=2) + '\n')
    print(json.dumps(dict(status=metadata['status'], accepted=accepted, frames=len(frames))))
    raise SystemExit(0 if accepted else 1)


if __name__ == '__main__':
    main()
