"""The Husk Guard's cooked clip order and prefab constants against husk_guard.rs.

`ActorController::HuskGuard` carries its clips in one array that
`husk_guard::Clip::slot` indexes, and the four sense boxes and the stomp wave
are constants in the controller that host/husk_guard.py proves per placement.
Either side drifting alone would run a placement against the wrong numbers
without failing anywhere else.
"""
import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import husk_guard

RUST = (ROOT / 'shared/hk-sim/src/husk_guard.rs').read_text()


def rust_array(name):
    found = re.search(rf'pub const {name}: \[i32; 4\] = \[([^\]]*)\];', RUST)
    return [int(v) for v in found.group(1).split(',')]


class HuskGuardContractTests(unittest.TestCase):
    def test_clip_slots_follow_the_clip_enum(self):
        body = re.search(r'pub enum Clip \{(.*?)\}', RUST, re.S).group(1)
        variants = [v.strip().rstrip(',') for v in body.split('\n') if v.strip() and not v.strip().startswith('//')]
        snake = [re.sub(r'(?<!^)([A-Z])', r'_\1', v).lower() for v in variants[2:]]
        self.assertEqual(variants[:2], ['Walk', 'Turn'])
        self.assertEqual(list(husk_guard.CLIP_SLOTS), snake)
        count = int(re.search(r'pub const COUNT: usize = (\d+);', RUST).group(1))
        self.assertEqual(count, len(husk_guard.CLIP_SLOTS))
        for slot in husk_guard.CLIP_SLOTS:
            self.assertIn(husk_guard.SLOT_CLIPS[slot], husk_guard.CLIPS)

    def test_the_boxes_are_the_ones_the_controller_carries(self):
        for name, value in (('ALERT', husk_guard.ALERT_Q16), ('ATTACK', husk_guard.ATTACK_Q16),
                            ('OVERHEAD', husk_guard.OVERHEAD_Q16), ('SWIPE', husk_guard.SWIPE_Q16)):
            self.assertEqual(rust_array(name), value, name)


if __name__ == '__main__':
    unittest.main()
