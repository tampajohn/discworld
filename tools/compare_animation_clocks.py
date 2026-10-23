#!/usr/bin/env python3
"""Compare diagnostic clock offsets; this does not prove scheduler equivalence."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reference', type=Path)
    parser.add_argument('rust', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    reference_bytes = args.reference.read_bytes()
    rust_bytes = args.rust.read_bytes()
    reference, rust = json.loads(reference_bytes), json.loads(rust_bytes)
    if not reference['complete'] or reference['errors']:
        raise ValueError('incomplete reference capture')
    original = defaultdict(list)
    replacement = defaultdict(list)
    for event in reference['events']:
        if event['kind'] == 'animation_step':
            original[event['cycle'], event['script']].append(event)
    for frame in rust:
        for animation in frame['animations']:
            replacement[frame['before_tick'], animation['script']].append(animation)
    if not original or not replacement:
        raise ValueError('missing animation clocks')
    fields = ('index', 'period', 'remaining')
    rows = []
    for script in sorted({script for _, script in original}):
        scores = {}
        for offset in (-2, -1, 0, 1, 2):
            compared = equal = ambiguous = 0
            for (cycle, handle), events in original.items():
                if handle != script:
                    continue
                candidates = replacement.get((cycle + offset, script), [])
                if len(events) > 1 or len(candidates) > 1:
                    ambiguous += 1
                elif candidates:
                    compared += 1
                    equal += all(events[0][key] == candidates[0][key] for key in fields)
            scores[offset] = dict(equal=equal, compared=compared, ambiguous=ambiguous)
        rows.append(dict(script=script, rust_tick_minus_reference_cycle=scores))
    result = {
        'scope': 'Rust before-tick state versus original StepAnimScript entry. '
                 'Repeated script identities in one cycle are excluded. '
                 'Offsets are diagnostic candidates, not permission to shift all frames.',
        'reference_sha256': hashlib.sha256(reference_bytes).hexdigest(),
        'rust_sha256': hashlib.sha256(rust_bytes).hexdigest(),
        'scripts': rows,
    }
    with args.output.open('x') as file:
        json.dump(result, file, indent=2)
        file.write('\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
