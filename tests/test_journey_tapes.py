"""The journey's tapes are their route strings, and the driver runs them in order.

A .pxtape is opaque, so each journey segment keeps the route string it was
encoded from beside it, the way tools/tapes/boss-fight.route does. This pins
that the two still agree, which is what lets a reader audit a segment (and
re-author one) from the text instead of trusting a binary.
"""
import re, struct, sys, tempfile, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
import sys
sys.path.insert(0, str(ROOT / 'host'))
sys.path.insert(0, str(ROOT / 'tools'))


def journey():
    """The journey segments in order: the pin files that say `journey = N`."""
    found = []
    for pins in (ROOT / 'tools/tapes').glob('*.pins'):
        m = re.search(r'^journey\s*=\s*(\d+)\s*$', pins.read_text(), re.M)
        if m:
            found.append((int(m[1]), pins.stem))
    return [name for _, name in sorted(found)]


class JourneyTapeTests(unittest.TestCase):
    def test_every_segment_is_its_route_string(self):
        from validate import poll_tape
        names = journey()
        self.assertEqual(names[0], 'journey-kings', 'the journey starts at Start Game')
        for name in names:
            with self.subTest(segment=name):
                tape = (ROOT / f'tools/tapes/{name}.pxtape').read_bytes()
                count, start = struct.unpack_from('<II', tape, 8)
                self.assertEqual(start, 0)
                route = (ROOT / f'tools/tapes/{name}.route').read_text().strip()
                with tempfile.TemporaryDirectory() as temp:
                    rebuilt = Path(temp) / 'tape.pxtape'
                    poll_tape(rebuilt, route, count)
                    self.assertEqual(rebuilt.read_bytes(), tape, f'{name}.route no longer encodes {name}.pxtape')

    def test_no_segment_boots_from_a_fixture(self):
        # The point of the journey is that nothing is seeded: the first segment
        # boots with no card and every later one with its predecessor's.
        for name in journey():
            pins = (ROOT / f'tools/tapes/{name}.pins').read_text()
            self.assertIsNone(re.search(r'^card\s*=', pins, re.M), f'{name} boots from a fixture')


if __name__ == '__main__':
    unittest.main()
