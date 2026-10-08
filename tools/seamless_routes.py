#!/usr/bin/env python3
"""Generate the asset-free poll tapes used for residency regression checks."""
import argparse
import hashlib
import json
import struct
from pathlib import Path

from validate import DOOR_ROUTE, poll_tape


def generate(output: Path):
    output.mkdir(parents=True, exist_ok=True)
    returning = DOOR_ROUTE + ',500:left:446,650:cross:24,780:cross:24,910:cross:24'
    jumping = DOOR_ROUTE + ',' + ','.join(
        f'{n}:{"left" if i % 2 == 0 else "right"}:44'
        for i, n in enumerate(range(510, 1214, 44)))
    jumping += ',' + ','.join(f'{n}:cross:20' for n in range(525, 1195, 45))
    recipes = {'forward': (DOOR_ROUTE, 512), 'return': (returning, 1024),
               'jumping': (jumping, 1280)}
    manifest = {}
    for name, (events, count) in recipes.items():
        path = output / f'{name}.pxtape'
        poll_tape(path, events, count)
        manifest[name] = {'events': events, 'samples': count}

    # Reach x≈86 with the forward route, then repeatedly jump left through
    # x=84 and walk right through it. Preserve the original first440 polls.
    prefix = (output / 'return.pxtape').read_bytes()
    rows = [bytearray(prefix[16+i*6:22+i*6]) if i < 440 else
            bytearray(struct.pack('<HBBBB', 0, 128, 128, 128, 128))
            for i in range(1280)]
    for i in range(440, 1160):
        mask = 128 if (i - 440) // 60 % 2 == 0 else 32
        if (i - 450) % 120 < 24:
            mask |= 16384
        if i % 26 < 2:
            mask |= 32768
        rows[i][:2] = struct.pack('<H', mask)
    (output / 'boundary-jumps.pxtape').write_bytes(
        b'PXITAPE2' + struct.pack('<II', 1280, 0) + b''.join(rows))
    manifest['boundary-jumps'] = {
        'samples': 1280,
        'recipe': 'return prefix440; alternate left/right60 polls through1159; '
                  'jump24 every120 from450; square2 every26; release afterward'}
    for name, row in manifest.items():
        row['sha256'] = hashlib.sha256((output / f'{name}.pxtape').read_bytes()).hexdigest()
    (output / 'routes.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('.hkpsx/seamless-routes'))
    generate(parser.parse_args().output)
