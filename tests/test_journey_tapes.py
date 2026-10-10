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
import rustsrc
sys.path.insert(0, str(ROOT / 'tools'))


def journey():
    body = rustsrc.const_expr(rustsrc.source(ROOT / 'host/hk-build/main.rs'), 'JOURNEY')
    return re.findall(r'"([^"]+)"', body)


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
        memcards = rustsrc.const_expr(rustsrc.source(ROOT / 'host/hk-build/main.rs'), 'MEMCARDS')
        for name in journey():
            self.assertNotIn(f'"{name}"', memcards)


if __name__ == '__main__':
    unittest.main()
