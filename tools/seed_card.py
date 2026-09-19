#!/usr/bin/env python3
"""Write a save record into a card fixture, to reach a room no route can walk to.

Most fixtures come from a route that earned them, which is always better: the
card `kings-return` boots from was written by `kings-death` actually dying. But
some rooms are three scene transitions past anywhere a route reaches, and
authoring a blind traversal through three unfamiliar rooms costs far more than
the coverage is worth. `town-shade.mcd` is the precedent: its record carries a
Hollow Shade six units from the Dirtmouth bench because no route could produce
one there.

A seeded record is a weaker fixture than an earned one and the difference matters
when reading a result: it proves the room and what happens in it, not the journey
to it. Say which you have.

The record layout is read out of `game/src/save.rs`, the same way
`tools/migrate_cards.py` reads it, so this cannot drift from the guest. The
psx-mc container header ahead of the payload carries the length and is rewritten
with it; missing that produces a valid record the guest silently rejects for
being short, which cost two builds to find.
"""
import argparse, struct, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from migrate_cards import BLOCK, CONTAINER, CONTAINER_LEN, ENTRY, IN_USE, fnv, layout

Q = 65536


def seed(image, magic, length, **fields):
    """Rewrite every save record in a card with the given field values."""
    out = bytearray(image)
    written = 0
    for block in range(1, 16):
        if image[block * ENTRY] != IN_USE:
            continue
        start = block * BLOCK
        at = image.find(magic, start, start + BLOCK)
        if at < 0:
            continue
        record = bytearray(image[at:at + length])
        for name, (offset, fmt) in FIELDS.items():
            if name in fields:
                struct.pack_into(fmt, record, offset, fields[name])
        struct.pack_into('<I', record, length - 4, fnv(record[:length - 4]))
        out[at:at + length] = record
        # The container's lengths do not move here, but assert they already
        # describe this record rather than assuming it.
        raw, stored = struct.unpack_from('<II', out, at - CONTAINER_LEN + 8)
        if (raw, stored) != (length, length):
            raise SystemExit(f'block {block}: container says {raw}/{stored}, not {length}')
        written += 1
    return bytes(out), written


# Offsets from `Save::encode` in game/src/save.rs.
FIELDS = {
    'scene': (4, '<i'), 'seat_x': (8, '<i'), 'seat_y': (12, '<i'),
    'facing': (16, '<i'), 'region': (20, '<i'), 'geo': (24, '<i'),
}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--from-card', type=Path, required=True, help='a valid card to base the record on')
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--scene', type=int, required=True)
    p.add_argument('--region', type=int, required=True)
    p.add_argument('--x', type=float, required=True, help='world units')
    p.add_argument('--y', type=float, required=True)
    p.add_argument('--facing', type=int, default=1)
    p.add_argument('--geo', type=int)
    a = p.parse_args()
    magic, length = layout()
    fields = {'scene': a.scene, 'region': a.region, 'facing': a.facing,
              'seat_x': round(a.x * Q), 'seat_y': round(a.y * Q)}
    if a.geo is not None:
        fields['geo'] = a.geo
    image = a.from_card.read_bytes()
    if magic not in image:
        raise SystemExit(f'{a.from_card} is not on the current {magic.decode()} format; '
                         'run tools/migrate_cards.py first')
    out, written = seed(image, magic, length, **fields)
    a.output.write_bytes(out)
    print(f'{a.output.name}: {written} record(s) seeded at scene {a.scene} region {a.region} '
          f'({a.x}, {a.y})')
    return 0


if __name__ == '__main__':
    sys.exit(main())
