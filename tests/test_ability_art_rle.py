"""The ability art's per-frame run-length code (host/ability_art.py `packbits`) round-trips.

The guest decodes it straight into the GPU port (game/src/ability_art.rs `Unpack`), a
token at a time, so an off-by-one at a run or literal boundary would corrupt a frame
on the console and nowhere else.
"""
import random
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

from ability_art import packbits, unpackbits


class RunLengthTest(unittest.TestCase):
    def test_boundaries(self):
        for n in (1, 2, 3, 127, 128, 129, 130, 131, 257, 258, 259):
            for data in (bytes(n), bytes([7]) * n, bytes(range(n % 256)) * (1 + n // 256), b'\x00\x01' * n):
                self.assertEqual(unpackbits(packbits(data)), data, (n, data[:8]))

    def test_random(self):
        rng = random.Random(1)
        for _ in range(300):
            data = bytes(rng.choice((0, 0, 0, 1, 2, rng.randrange(256))) for _ in range(rng.randrange(1, 600)))
            self.assertEqual(unpackbits(packbits(data)), data)

    def test_tokens_stay_in_range(self):
        coded = packbits(bytes(1000) + bytes(range(200)) + bytes([9]) * 500)
        i = 0
        while i < len(coded):
            token = coded[i]
            i += 2 + token if token < 0x80 else 2
        self.assertEqual(i, len(coded))


if __name__ == '__main__':
    unittest.main()
