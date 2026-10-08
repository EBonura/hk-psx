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
from migrate_cards import BLOCK, CONTAINER, CONTAINER_LEN, ENTRY, IN_USE, fnv, layout, record_length

Q = 65536


# SceneData items (game/src/persist.rs): kind in the top four bits, scene in
# the next ten, local id in the next ten, value in the low eight; the list is
# sorted and its count sits in the two bytes before it.
ITEMS_AT = 162


def item_word(kind, scene, local, value):
    if not (0 < kind < 16 and 0 <= scene < 1024 and 0 <= local < 1024 and 0 <= value < 256):
        raise SystemExit(f'item outside the persist layout: {kind}:{scene}:{local}:{value}')
    return kind << 28 | scene << 18 | local << 8 | value


def with_items(record, items):
    """The record with `items` merged into its SceneData list (an item for the
    same kind, scene and local id replaces the old one), checksum redone."""
    count = struct.unpack_from('<H', record, ITEMS_AT - 2)[0]
    old = list(struct.unpack_from(f'<{count}I', record, ITEMS_AT))
    merged = {word >> 8: word for word in old}
    merged.update({word >> 8: word for word in items})
    words = sorted(merged.values())
    out = bytearray(record[:ITEMS_AT]) + struct.pack(f'<{len(words)}I', *words) + b'\0' * 4
    struct.pack_into('<H', out, ITEMS_AT - 2, len(words))
    struct.pack_into('<I', out, len(out) - 4, fnv(out[:-4]))
    return bytes(out)


def seed(image, magic, length, items=(), **fields):
    """Rewrite every save record in a card with the given field values and
    SceneData items."""
    out = bytearray(image)
    written = 0
    for block in range(1, 16):
        if image[block * ENTRY] != IN_USE:
            continue
        start = block * BLOCK
        at = image.find(magic, start, start + BLOCK)
        if at < 0:
            continue
        # HKS5 records are as long as their SceneData list; `length` is the
        # shortest, so the container says how long this one is.
        length = record_length(image, at) or length
        record = bytearray(image[at:at + length])
        for name, (offset, fmt) in FIELDS.items():
            if name in fields:
                struct.pack_into(fmt, record, offset, fields[name])
        struct.pack_into('<I', record, length - 4, fnv(record[:length - 4]))
        # The container's lengths describe this record before anything grows it.
        raw, stored = struct.unpack_from('<II', out, at - CONTAINER_LEN + 8)
        if (raw, stored) != (length, length):
            raise SystemExit(f'block {block}: container says {raw}/{stored}, not {length}')
        if items:
            record = with_items(bytes(record), items)
            if at + len(record) > (block + 1) * BLOCK:
                raise SystemExit(f'block {block}: the grown record leaves its block')
            struct.pack_into('<II', out, at - CONTAINER_LEN + 8, len(record), len(record))
        out[at:at + len(record)] = record
        written += 1
    return bytes(out), written


# Offsets from `Save::encode` in game/src/save.rs.
FIELDS = {
    'scene': (4, '<i'), 'seat_x': (8, '<i'), 'seat_y': (12, '<i'),
    'facing': (16, '<i'), 'region': (20, '<i'), 'geo': (24, '<i'),
    # PlayerData `fireballLevel`, the first of the small integers after the bools.
    'fireball': (156, '<B'),
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
    p.add_argument('--fireball', type=int, help='PlayerData fireballLevel (Vengeful Spirit is 1)')
    p.add_argument('--item', action='append', default=[], metavar='KIND:SCENE:LOCAL:VALUE',
                   help='a SceneData item to seed (persist.rs Kind number), repeatable')
    a = p.parse_args()
    magic, length = layout()
    fields = {'scene': a.scene, 'region': a.region, 'facing': a.facing,
              'seat_x': round(a.x * Q), 'seat_y': round(a.y * Q)}
    if a.geo is not None:
        fields['geo'] = a.geo
    if a.fireball is not None:
        fields['fireball'] = a.fireball
    image = a.from_card.read_bytes()
    if magic not in image:
        raise SystemExit(f'{a.from_card} is not on the current {magic.decode()} format; '
                         'run tools/migrate_cards.py first')
    items = [item_word(*map(int, text.split(':'))) for text in a.item]
    out, written = seed(image, magic, length, items, **fields)
    a.output.write_bytes(out)
    print(f'{a.output.name}: {written} record(s) seeded at scene {a.scene} region {a.region} '
          f'({a.x}, {a.y})')
    return 0


if __name__ == '__main__':
    sys.exit(main())
