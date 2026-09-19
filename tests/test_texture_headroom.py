"""The headroom report, which decides whether new art can exist at all.

The distinction it draws is the whole point: a scene's headroom is measured
against that scene's worst view, and the global figure against the worst view
anywhere. Collapsing the two would say a pause-screen icon has 212 slots in Town
when what it actually has is whatever Tutorial_01's tightest view left over.
"""
import sys, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))

from texture_headroom import headroom


def report(rows, cap=416):
    return {'quality': {'texture_budget': cap},
            'regions': [{'scene_name': s, 'chunk_id': c, 'textures': t} for s, c, t in rows]}


class HeadroomTests(unittest.TestCase):
    def test_a_scene_is_measured_by_its_worst_view_not_its_average(self):
        out = headroom(report([('A', 1, 100), ('A', 2, 400), ('A', 3, 100)]))
        self.assertEqual(out['scenes']['A']['headroom'], 16)
        self.assertEqual(out['scenes']['A']['tightest_chunk'], 2)
        self.assertEqual(out['scenes']['A']['views'], 3)

    def test_the_global_figure_is_the_worst_view_anywhere(self):
        # Art resident everywhere fits only where the least room is, so a roomy
        # scene must not raise the answer.
        out = headroom(report([('A', 1, 410), ('B', 2, 100)]))
        self.assertEqual(out['global_headroom'], 6)
        self.assertEqual(out['scenes']['B']['headroom'], 316)

    def test_the_real_world_still_has_room_somewhere_and_almost_none_everywhere(self):
        # Pins the shape of the live answer: per-scene room is plentiful and the
        # global figure is not. A change that inverts this is a budget event.
        import json
        out = headroom(json.loads((ROOT / 'data/regions.json').read_text()))
        self.assertLess(out['global_headroom'], 64,
                        'art resident in every view has never had real room; check the budget')
        self.assertGreater(max(s['headroom'] for s in out['scenes'].values()), 200)


if __name__ == '__main__':
    unittest.main()
