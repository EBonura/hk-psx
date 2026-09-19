"""Carrying the committed card fixtures forward when the save record grows.

This rewrites saves the guest will trust, so the properties that matter are that
nothing the old record carried changes, that the fields it never had come out
zero rather than inherited from padding, and that a record whose own checksum
does not hold is refused rather than laundered into a valid one.
"""
import struct, sys, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))

from migrate_cards import BLOCK, ENTRY, IN_USE, fnv, layout, migrate

OLD_MAGIC, OLD_LEN = b'HKS1', 56


def card(records):
    """A 16-block card image holding one old record per named block."""
    data = bytearray(BLOCK * 16)
    for block, body in records.items():
        data[block * ENTRY] = IN_USE
        struct.pack_into('<I', data, block * ENTRY + 4, BLOCK)
        data[block * BLOCK + 272:block * BLOCK + 272 + len(body)] = body
    return bytes(data)


def old_record(geo=6, sequence=3, door_hits=13):
    body = bytearray(OLD_LEN)
    body[0:4] = OLD_MAGIC
    struct.pack_into('<i', body, 24, geo)
    body[28] = door_hits
    struct.pack_into('<i', body, 48, sequence)
    struct.pack_into('<I', body, OLD_LEN - 4, fnv(body[:OLD_LEN - 4]))
    return bytes(body)


class MigrationTests(unittest.TestCase):
    def setUp(self):
        self.magic, self.length = layout()

    def run_one(self, image):
        return migrate(image, OLD_MAGIC, OLD_LEN, self.magic, self.length)

    def test_everything_the_old_record_carried_survives_byte_for_byte(self):
        out, moved = self.run_one(card({1: old_record()}))
        self.assertEqual(moved, 1)
        at = out.find(self.magic)
        # Bytes 4 to 52 are the fields the old format already had; only the tag
        # ahead of them and the fields after them are new.
        self.assertEqual(out[at + 4:at + OLD_LEN - 4], old_record()[4:OLD_LEN - 4])
        self.assertEqual(struct.unpack_from('<i', out, at + 24)[0], 6)
        self.assertEqual(out[at + 28], 13)
        self.assertEqual(struct.unpack_from('<i', out, at + 48)[0], 3)

    def test_the_fields_it_never_had_are_zero_and_the_new_checksum_holds(self):
        out, _ = self.run_one(card({1: old_record()}))
        at = out.find(self.magic)
        # A save made before charms existed owned none and had held no
        # conversation, so zero is the true value rather than a placeholder.
        self.assertEqual(out[at + OLD_LEN - 4:at + self.length - 4], bytes(self.length - OLD_LEN))
        self.assertEqual(fnv(out[at:at + self.length - 4]),
                         struct.unpack_from('<I', out, at + self.length - 4)[0])

    def test_a_record_that_fails_its_own_checksum_is_refused(self):
        # Rewriting it would turn a card the guest correctly rejects into one it
        # trusts, which is worse than leaving it broken.
        broken = bytearray(old_record())
        broken[24] ^= 0xff
        with self.assertRaises(SystemExit):
            self.run_one(card({1: bytes(broken)}))

    def test_padding_that_is_not_padding_is_refused(self):
        # The new fields land on bytes the old file left zero. If something is
        # there, this is not the layout we think it is.
        image = bytearray(card({1: old_record()}))
        at = image.find(OLD_MAGIC)
        image[at + OLD_LEN + 2] = 1
        with self.assertRaises(SystemExit):
            self.run_one(bytes(image))

    def test_every_in_use_block_is_carried_not_just_the_first(self):
        # power-cut.mcd holds two copies; missing one leaves a card half old.
        out, moved = self.run_one(card({1: old_record(sequence=3), 2: old_record(sequence=4)}))
        self.assertEqual(moved, 2)
        self.assertEqual(out.count(self.magic), 2)
        self.assertEqual(out.count(OLD_MAGIC), 0)

    def test_the_committed_fixtures_are_on_the_current_format(self):
        # The thing that actually breaks seven routes. If this fails, run
        # tools/migrate_cards.py.
        for path in sorted((ROOT / 'tools/cards').glob('*.mcd')):
            with self.subTest(card=path.name):
                # assertIn would print the whole 131 KB image on failure.
                self.assertTrue(self.magic in path.read_bytes(),
                                f'{path.name} predates the current save record; '
                                'run tools/migrate_cards.py')


if __name__ == '__main__':
    unittest.main()
