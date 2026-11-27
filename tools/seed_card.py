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

It edits HKS5 records, the format every fixture carries: the guest loads them
through its migration (`save::migrate`), so a seeded HKS5 card is as good a
fixture as an earned one of the same format. HKS6 holds PlayerData as named
fields rather than at these offsets, so this deliberately stays on HKS5. The
psx-mc container header ahead of the payload carries the length and is rewritten
with it; missing that produces a valid record the guest silently rejects for
being short, which cost two builds to find.
"""
import argparse, struct, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from migrate_cards import APPEND_ONLY_LAST, BLOCK, CONTAINER, CONTAINER_LEN, ENTRY, IN_USE, fnv, record_length

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
        if fields.get('player_bits'):
            # PlayerData bools (persist.rs bit numbers) ORed into the saved word.
            old, = struct.unpack_from('<I', record, PLAYER_BOOLS_AT)
            struct.pack_into('<I', record, PLAYER_BOOLS_AT, old | fields['player_bits'])
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


# Offsets from HKS5's `Save::encode` (game/src/save.rs before HKS6; the HKS4 and
# HKS5 offsets `save::migrate` reads are the same ones). The PlayerData bool word sits
# four bytes ahead of the small integers `fireball` writes.
PLAYER_BOOLS_AT = 152
FIELDS = {
    'scene': (4, '<i'), 'seat_x': (8, '<i'), 'seat_y': (12, '<i'),
    'facing': (16, '<i'), 'region': (20, '<i'), 'geo': (24, '<i'),
    # PlayerData `fireballLevel`, the first of the small integers after the bools.
    'fireball': (156, '<B'),
}


# HKS6 (game/src/save.rs): a 52-byte header at HKS5's offsets, the PlayerData section (a u16 count, then a u16 id and
# an i32 value per field that differs from a new game's, ids increasing), the SceneData item count and items, then
# the checksum. The ids are indexes into `NAMES` in game/src/player_data.rs, read from there.
HKS6_HEADER = 52


def player_data_ids():
    import re
    text = (ROOT / 'game/src/player_data.rs').read_text()
    body = text.split('pub const NAMES: [&str; COUNT] = [', 1)[1].split('];', 1)[0]
    return {name: i for i, name in enumerate(re.findall(r'"([^"]*)"', body))}


def to_hks6(image, fields):
    """Rewrite every HKS4 or HKS5 record in a card as an HKS6 one: the header and the SceneData items as they were,
    PlayerData a new game's plus `fields` ({id: value}). The old record's own PlayerData is not carried: use this
    only on a record whose PlayerData is a new game's (every offset `save::migrate` reads is zero), which it checks."""
    out = bytearray(image)
    written = 0
    for block in range(1, 16):
        if image[block * ENTRY] != IN_USE:
            continue
        start = block * BLOCK
        for magic, shortest in ((b'HKS4', 156), (b'HKS5', 166)):
            at = image.find(magic, start, start + BLOCK)
            if at >= 0:
                break
        else:
            continue
        length = record_length(image, at) or shortest
        record = image[at:at + length]
        if fnv(record[:-4]) != struct.unpack_from('<I', record, length - 4)[0]:
            raise SystemExit(f'block {block}: the record fails its checksum')
        items_at = ITEMS_AT if magic == b'HKS5' else None
        player_end = items_at - 2 if items_at else length - 4
        if any(record[HKS6_HEADER:player_end]):
            raise SystemExit(f'block {block}: the record carries PlayerData; to_hks6 would drop it')
        count = struct.unpack_from('<H', record, items_at - 2)[0] if items_at else 0
        items = record[items_at:items_at + 4 * count] if items_at else b''
        entries = sorted(fields.items())
        rec = bytearray(b'HKS6' + record[4:HKS6_HEADER])
        rec += struct.pack('<H', len(entries)) + b''.join(struct.pack('<Hi', i, v) for i, v in entries)
        rec += struct.pack('<H', count) + items
        rec += struct.pack('<I', fnv(rec))
        if at + len(rec) > (block + 1) * BLOCK:
            raise SystemExit(f'block {block}: the record leaves its block')
        struct.pack_into('<II', out, at - CONTAINER_LEN + 8, len(rec), len(rec))
        out[at:at + length] = b'\0' * length
        out[at:at + len(rec)] = rec
        written += 1
    return bytes(out), written


def main():
    if len(sys.argv) > 1 and sys.argv[1] == 'hks6':
        return main_hks6(sys.argv[2:])
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
    p.add_argument('--player-bit', type=int, action='append', default=[], metavar='BIT',
                   help='a PlayerData bool to set (game/src/persist.rs bit number), repeatable')
    p.add_argument('--item', action='append', default=[], metavar='KIND:SCENE:LOCAL:VALUE',
                   help='a SceneData item to seed (persist.rs Kind number), repeatable')
    a = p.parse_args()
    magic, length = APPEND_ONLY_LAST
    fields = {'scene': a.scene, 'region': a.region, 'facing': a.facing,
              'seat_x': round(a.x * Q), 'seat_y': round(a.y * Q)}
    if a.geo is not None:
        fields['geo'] = a.geo
    if a.fireball is not None:
        fields['fireball'] = a.fireball
    if a.player_bit:
        fields['player_bits'] = sum(1 << bit for bit in a.player_bit)
    image = a.from_card.read_bytes()
    if magic not in image:
        raise SystemExit(f'{a.from_card} holds no {magic.decode()} record; '
                         'run tools/migrate_cards.py on it first')
    items = [item_word(*map(int, text.split(':'))) for text in a.item]
    out, written = seed(image, magic, length, items, **fields)
    a.output.write_bytes(out)
    print(f'{a.output.name}: {written} record(s) seeded at scene {a.scene} region {a.region} '
          f'({a.x}, {a.y})')
    return 0


def main_hks6(argv):
    """`seed_card.py hks6 --from-card A --output B --player-field slyRescued=1`: the record of a new-game-PlayerData
    card as HKS6, with the named PlayerData fields set. For a field HKS5 has no place for."""
    p = argparse.ArgumentParser(prog='seed_card.py hks6', description=main_hks6.__doc__)
    p.add_argument('--from-card', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--player-field', action='append', default=[], metavar='NAME=VALUE')
    a = p.parse_args(argv)
    ids = player_data_ids()
    fields = {}
    for text in a.player_field:
        name, value = text.split('=')
        if name not in ids:
            raise SystemExit(f'{name} is not a player_data field')
        fields[ids[name]] = int(value)
    out, written = to_hks6(a.from_card.read_bytes(), fields)
    if not written:
        raise SystemExit(f'{a.from_card} holds no HKS4 or HKS5 record')
    a.output.write_bytes(out)
    print(f'{a.output.name}: {written} record(s) as HKS6 with {a.player_field}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
