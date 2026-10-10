#!/usr/bin/env python3
"""Carry the committed card fixtures forward when the save record grows.

Seven of the ten validation routes boot from a card under tools/cards, and every
one of those fixtures was written by an older guest. When a field joins
`game/src/save.rs` the record gets longer and differently tagged, so the guest
reads the old fixture as Corrupt and those seven routes stop resuming. That is
the correct behaviour, not a bug: a short record must never half-decode into a
save whose new fields would be invented.

It does mean the fixtures have to move with the format, and replaying them into
existence is circular. `bench-save` is the only route that writes a card and it
resumes from `town-continue.mcd`, so the fixture that needs regenerating is the
one the regenerating route needs first.

Rewriting them here is not a shortcut around that. The fields a pre-charm save
gained are genuinely zero for it: it owned no charms because none could be
obtained, and it had held no conversation because nobody was there to talk to.
Everything the old record did carry, the scene, the seat, the wallet, the Great
Door and the Shade, is copied across byte for byte.

The record layout is read out of game/src/save.rs rather than repeated here, so
this cannot drift from the guest the way a hardcoded copy would.
"""
import argparse, re, struct, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import rustsrc
SAVE_RS = ROOT / 'game/src/save.rs'
# A PSX card is 16 blocks of 8192 bytes; block 0 holds the 128-byte directory
# entries and blocks 1 to 15 hold files. 0x51 marks an entry in use.
BLOCK = 8192
ENTRY = 128
IN_USE = 0x51
# psx-mc writes a self-describing container ahead of every payload: "PMC1", a
# flags byte whose bit 0 means LZSS, three reserved, then raw_len and stored_len
# as little-endian u32. Both lengths have to move with the record. Missing them
# is silent in the worst way: the record itself is valid and correctly
# checksummed, the guest just reads back the old number of bytes and rejects it
# for being too short, so every card-booting route stops resuming and nothing
# says why.
CONTAINER = b'PMC1'
CONTAINER_LEN = 16
FLAG_COMPRESSED = 1


def fnv(data):
    h = 0x811c9dc5
    for b in data:
        h = ((h ^ b) * 16777619) & 0xffffffff
    return h


# Formats the current guest loads besides its own, as (magic, fixed length).
# HKS4 loads into HKS5 with an untouched world (game/src/save.rs says why), so
# a fixture on it is not stale and the power-cut check must find it.
LOADABLE_OLDER = ((b'HKS4', 156),)


def record_length(data, at):
    """A record's own length, from the psx-mc container ahead of it. HKS5 is
    variable (its SceneData list grows with play), so the length the guest
    defines is only the shortest one."""
    raw, stored = struct.unpack_from('<II', data, at - CONTAINER_LEN + 8)
    return raw if raw == stored else None


def loadable_records(data, start, end):
    """(at, length, magic) of every record the current guest would load that
    starts in data[start:end] and whose own checksum holds."""
    magic, _ = layout()
    out = []
    for tag in (magic,) + tuple(m for m, _ in LOADABLE_OLDER):
        at = data.find(tag, start, end)
        if at < 0:
            continue
        length = record_length(data, at)
        if length and at + length <= len(data) and \
                fnv(data[at:at + length - 4]) == struct.unpack_from('<I', data, at + length - 4)[0]:
            out.append((at, length, tag))
    return out


def layout():
    """(magic, length) as the guest currently defines them. For HKS5 onward the
    length is the shortest record, the one with an empty SceneData list, which
    is what a migrated fixture becomes."""
    text = rustsrc.source(SAVE_RS)
    magic = re.search(r'const MAGIC:\[u8;4\]=\*b"(\w{4})"', text)
    try:
        length = rustsrc.const_int(text, 'LEN')
    except KeyError:
        length = None
    if not magic or length is None:
        raise SystemExit('cannot read MAGIC/LEN out of game/src/save.rs')
    return magic.group(1).encode(), length


def container_of(data, at, block_start):
    """The psx-mc container header that describes the payload starting at `at`."""
    hdr = data.rfind(CONTAINER, block_start, at)
    if hdr < 0 or at - hdr != CONTAINER_LEN:
        raise SystemExit(f'no psx-mc container immediately ahead of the record at {at}')
    if data[hdr + 4] & FLAG_COMPRESSED:
        raise SystemExit('the payload is compressed; this rewrites plain records only')
    return hdr


def migrate(data, old_magic, old_len, magic, length):
    """Every record in one card image, carried forward. Returns (bytes, count).

    Idempotent: a record already on the new format still has its container
    header checked and repaired, because the two are rewritten separately and a
    run that did one without the other leaves a card that looks migrated and
    does not load.
    """
    out = bytearray(data)
    moved = 0
    for block in range(1, 16):
        entry = data[block * ENTRY:(block + 1) * ENTRY]
        if entry[0] != IN_USE:
            continue
        touched = False
        start = block * BLOCK
        at = data.find(old_magic, start, start + BLOCK)
        if at >= 0:
            # Refuse a record whose own checksum does not hold: rewriting a
            # corrupt save would turn a card the guest correctly rejects into
            # one it trusts.
            body = data[at:at + old_len - 4]
            if fnv(body) != struct.unpack_from('<I', data, at + old_len - 4)[0]:
                raise SystemExit(f'block {block}: {old_magic.decode()} record fails its own checksum')
            # HKS4 lent the script reserve's top two slots to the False Knight;
            # HKS5 keeps that in its SceneData list, which zero-filling cannot
            # write. The guest moves it on load, so boot such a card and save.
            if old_magic == b'HKS4' and magic == b'HKS5' and any(body[134:142]):
                raise SystemExit(f'block {block}: carries a False Knight flag in the script reserve; '
                                 'load it on the guest and save at a bench instead')
            if at + length > start + BLOCK:
                raise SystemExit(f'block {block}: the longer record would leave its block')
            # The old checksum sits where the first new field now goes, and the
            # bytes beyond it are the file's zero padding, so both are overwritten.
            if any(data[at + old_len:at + length]):
                raise SystemExit(f'block {block}: the space the new fields need is not padding')
            record = bytearray(length)
            record[:old_len - 4] = body
            record[:4] = magic
            struct.pack_into('<I', record, length - 4, fnv(record[:length - 4]))
            out[at:at + length] = record
            touched = True
        else:
            at = data.find(magic, start, start + BLOCK)
            if at < 0:
                continue
        hdr = container_of(data, at, start)
        raw, stored = struct.unpack_from('<II', out, hdr + 8)
        if (raw, stored) != (length, length):
            struct.pack_into('<II', out, hdr + 8, length, length)
            touched = True
        moved += touched
    return bytes(out), moved


def main():
    p = argparse.ArgumentParser(description=__doc__)
    # These two are the format the committed fixtures are on right now, so a
    # bare run carries them to whatever game/src/save.rs currently defines.
    # They move with each growth: HKS1/56, then HKS2/78, then HKS3/146, and
    # HKS4/156 since Sly's half of PlayerData joined the record. Leaving them
    # behind is how they came to say HKS1 while every fixture carried HKS3,
    # which makes a bare run report nothing to do and find nothing wrong.
    # The fixtures stay on HKS4 on purpose while the guest still loads it:
    # every card route then proves an old save loads.
    p.add_argument('--from-magic', default='HKS4', help='the tag the fixtures carry now')
    p.add_argument('--from-len', type=int, default=156, help='that format\'s record length')
    p.add_argument('--dry-run', action='store_true')
    p.add_argument('cards', nargs='*', type=Path,
                   help='card images; defaults to every fixture under tools/cards')
    a = p.parse_args()
    magic, length = layout()
    old_magic = a.from_magic.encode()
    if old_magic == magic:
        print(f'fixtures already carry {magic.decode()}; nothing to do')
        return 0
    cards = a.cards or sorted((ROOT / 'tools/cards').glob('*.mcd'))
    total = 0
    for path in cards:
        data = path.read_bytes()
        out, moved = migrate(data, old_magic, a.from_len, magic, length)
        total += moved
        state = 'up to date' if out == data else (
            f'{moved} record(s) to {magic.decode()} {length}B'
            + (' (dry run)' if a.dry_run else ''))
        print(f'{path.name}: {state}')
        if not a.dry_run and out != data:
            path.write_bytes(out)
    print(f'{total} record(s) across {len(cards)} card(s)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
