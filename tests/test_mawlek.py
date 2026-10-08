"""Keep host/mawlek_art.py's source contract and the Mawlek the guest runs in step.

`CONTRACT` is what host/mawlek_art.py `check_contract` asserts against the
installed source at cook time; this holds it against the constants in
shared/hk-sim/src/mawlek.rs without needing the source, so a change to either
side alone fails here.
"""
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import mawlek_art


class MawlekContract(unittest.TestCase):
    def test_every_rust_constant_is_the_source_number(self):
        mawlek_art.check_rust()

    def test_clip_order_is_the_rust_enum(self):
        text = (ROOT / 'shared/hk-sim/src/mawlek.rs').read_text()
        body = text[text.index('pub enum Clip {'):]
        body = body[:body.index('}')]
        names = [line.strip().rstrip(',') for line in body.splitlines()[1:] if line.strip() and not line.strip().startswith('//')]
        self.assertEqual([n.replace(' ', '') for n in mawlek_art.CLIPS], names)

    def test_a_spray_is_the_source_count(self):
        self.assertEqual(mawlek_art.CONTRACT['SPIT_SHOTS'], 25)
        self.assertEqual(mawlek_art.expected_rust()['ARENA_END_TICKS'], 630)


if __name__ == '__main__':
    unittest.main()
