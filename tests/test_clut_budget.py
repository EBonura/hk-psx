"""A CLUT slot is a distinct palette, not a texture record.

Synthetic fixtures only; no retail data in committed tests.
"""
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from PIL import Image
from cook import Atlas, MAX_ROOM_TEXTURES
from texture_dedup import clut_count


def palette(word):
    return struct.pack('<16H', 0, word, *([0] * 14))


class ClutBudgetTests(unittest.TestCase):
    def test_repeated_palettes_spend_one_slot_and_the_table_is_bounded_separately(self):
        """The two counts have their own limits, and only one of them is VRAM.

        A region may carry more texture records than it has palettes, which is
        exactly what a tiled frame produces, so a budget that spends a slot per
        record refuses art that fits.
        """
        a = Atlas(max_pages=0, max_textures=MAX_ROOM_TEXTURES, max_cluts=4)
        for i in range(200):
            a.add_quantized(1, 1, palette(1 + i % 4), b'\x01', streamed=True, unique=True)
        a.pack()
        self.assertEqual(len(a.entries), 200)
        self.assertEqual(a.cluts, 4)
        # The block still holds one entry per texture; sharing is a fact about
        # the words, not about how the reader sizes the section.
        self.assertEqual(len(a.palettes), 200)

    def test_distinct_palettes_still_exhaust_the_budget(self):
        a = Atlas(max_pages=0, max_cluts=8)
        for i in range(9):
            a.add_quantized(1, 1, palette(1 + i), b'\x01', streamed=True)
        with self.assertRaisesRegex(ValueError, 'CLUT budget'):
            a.pack()

    def test_texture_table_limit_is_reported_as_itself(self):
        """Past the table limit the failure names the table, not the budget.

        The two used to be one check, so a region over the record limit reported
        a CLUT overrun it did not have.
        """
        a = Atlas(max_pages=0, max_cluts=MAX_ROOM_TEXTURES)
        for i in range(MAX_ROOM_TEXTURES + 1):
            a.add_quantized(1, 1, palette(1 + i % 2), b'\x01', streamed=True, unique=True)
        with self.assertRaisesRegex(ValueError, 'texture table'):
            a.pack()

    def test_one_tiled_frame_costs_one_slot(self):
        """The measurement the False Knight route turns on.

        `add_tiled` quantizes a frame once, so its tiles carry byte-identical
        palette words however many slots the frame binds.
        """
        image = Image.new('RGBA', (198, 169))
        for y in range(169):
            for x in range(198):
                image.putpixel((x, y), ((x * 5) % 256, (y * 3) % 256, 40, 255))
        a = Atlas(max_pages=0)
        a.add_tiled(image, 198, 169)
        a.pack()
        self.assertEqual(len(a.entries), 12)
        self.assertEqual(a.cluts, 1)

    def test_clut_count_reads_a_cooked_pack(self):
        """The pack reader and the cooker agree on the same region."""
        a = Atlas(max_pages=0)
        for i in range(6):
            a.add_quantized(2, 1, palette(1 + i % 3), b'\x21', streamed=True, unique=True)
        a.pack()
        pack = bytearray(b'HKROOM02')
        pack.extend(struct.pack('<6I', 0, len(a.entries), 0, 0, 0, 0))
        pack.extend(struct.pack('<2I', len(a.stream), a.flags))
        for e in a.entries:
            pack.extend(struct.pack('<6HI', *e))
        pack.extend(b''.join(a.palettes))
        pack.extend(a.stream)
        self.assertEqual(clut_count(bytes(pack)), a.cluts)
        self.assertEqual(a.cluts, 3)


if __name__ == '__main__':
    unittest.main()
