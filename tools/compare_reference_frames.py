#!/usr/bin/env python3
"""Compare verified full RGB frames; report content coverage separately from timing."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reference', type=Path)
    parser.add_argument('rust', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    ref_bytes = (args.reference / 'capture.json').read_bytes()
    rust_bytes = (args.rust / 'capture.json').read_bytes()
    ref, rust = json.loads(ref_bytes), json.loads(rust_bytes)
    if not ref['complete'] or ref['errors'] or not ref['frames'] or not rust['frames']:
        raise ValueError('incomplete capture')
    by_image = defaultdict(list)
    rust_by_tick = {}
    starts = {}
    for frame in rust['frames']:
        data = (args.rust / f"tick-{frame['tick']:06d}.rgb").read_bytes()
        if len(data) != 320 * 200 * 3 or digest(data) != frame['rgb_sha256']:
            raise ValueError('Rust frame hash/size mismatch')
        by_image[(frame['scene'], frame['rgb_sha256'])].append(frame['tick'])
        rust_by_tick[frame['tick']] = (frame['scene'], frame['rgb_sha256'])
        starts.setdefault(frame['scene'], frame['tick'])
    reference_starts = {}
    rows = []
    for frame in ref['frames']:
        stem = args.reference / f"frame-{frame['frame']:04d}"
        indices, palette = stem.with_suffix('.idx').read_bytes(), stem.with_suffix('.pal').read_bytes()
        if len(indices) != 320 * 200 or len(palette) != 768:
            raise ValueError('reference frame size mismatch')
        rgb = b''.join(palette[i*3:i*3+3] for i in indices)
        if (digest(indices), digest(palette), digest(rgb)) != (
                frame['indices_sha256'], frame['palette_sha256'], frame['rgb_sha256']):
            raise ValueError('reference frame hash mismatch')
        key = (frame['scene'], frame['rgb_sha256'])
        row = {'frame': frame['frame'], 'scene': frame['scene'],
               'matching_rust_ticks': by_image[key]}
        if 'cycle' in frame and frame['scene'] in starts:
            reference_starts.setdefault(frame['scene'], frame['cycle'])
            tick = starts[frame['scene']] + frame['cycle'] - reference_starts[frame['scene']]
            row.update(cycle=frame['cycle'], scene_aligned_rust_tick=tick,
                       scene_aligned_equal=rust_by_tick.get(tick) == key)
            if tick in rust_by_tick and rust_by_tick[tick][0] == frame['scene']:
                other = (args.rust / f'tick-{tick:06d}.rgb').read_bytes()
                differences = [i // 3 for i in range(0, len(rgb), 3)
                               if rgb[i:i+3] != other[i:i+3]]
                row['scene_aligned_different_pixels'] = len(differences)
                row['scene_aligned_difference_bounds'] = (
                    [min(i % 320 for i in differences), min(i // 320 for i in differences),
                     max(i % 320 for i in differences), max(i // 320 for i in differences)]
                    if differences else None)
        rows.append(row)
    result = {'reference_capture_sha256': digest(ref_bytes),
              'rust_capture_sha256': digest(rust_bytes),
              'reference_frames': len(rows),
              'pixel_matched_frames': sum(bool(row['matching_rust_ticks']) for row in rows),
              'distinct_scene_images': len({(f['scene'], f['rgb_sha256']) for f in ref['frames']}),
              'scene_aligned_mismatches': [r['frame'] for r in rows if r.get('scene_aligned_equal') is False],
              'frames': rows,
              'scope': 'Exact full RGB content matches within the same scene. Scene-start alignment is diagnostic, not an assumed proof of scheduler or wall-clock equivalence.'}
    with args.output.open('x') as file:
        json.dump(result, file, indent=2)
        file.write('\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'frames'}, indent=2))


if __name__ == '__main__':
    main()
