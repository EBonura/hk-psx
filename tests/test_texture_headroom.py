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
    # A row's `cluts` is its distinct-palette count, which is what the budget
    # gates. These fixtures give each view one palette per texture, the case
    # that held before frames tiled, so the arithmetic below is unchanged by the
    # split and the two columns stay legible side by side.
    return {'quality': {'texture_budget': cap},
            'regions': [{'scene_name': s, 'chunk_id': c, 'textures': t, 'cluts': t}
                        for s, c, t in rows]}


def cluts_of(rep):
    return {r['chunk_id']: r['cluts'] for r in rep['regions']}


class HeadroomTests(unittest.TestCase):
    def test_a_scene_is_measured_by_its_worst_view_not_its_average(self):
        rep = report([('A', 1, 100), ('A', 2, 400), ('A', 3, 100)])
        out = headroom(rep, cluts_of(rep))
        self.assertEqual(out['scenes']['A']['headroom'], 16)
        self.assertEqual(out['scenes']['A']['tightest_chunk'], 2)
        self.assertEqual(out['scenes']['A']['views'], 3)

    def test_the_global_figure_is_the_worst_view_anywhere(self):
        # Art resident everywhere fits only where the least room is, so a roomy
        # scene must not raise the answer.
        rep = report([('A', 1, 410), ('B', 2, 100)])
        out = headroom(rep, cluts_of(rep))
        self.assertEqual(out['global_headroom'], 6)
        self.assertEqual(out['scenes']['B']['headroom'], 316)

    def test_the_real_world_still_has_room_somewhere_and_almost_none_everywhere(self):
        # Pins the shape of the live answer: per-scene room is plentiful and the
        # global figure much smaller. A change that moves either is a budget
        # event. One was: with scenery cooked one texture per sprite per scene
        # (2026-09-23), King's Pass's tightest view went from 413 of 416 CLUT
        # slots to 249, so art resident in every view has 167 slots, not 3.
        import json
        from texture_headroom import view_cluts
        rep = json.loads((ROOT / 'data/regions.json').read_text())
        out = headroom(rep, view_cluts(rep))
        self.assertGreaterEqual(out['global_headroom'], 64,
                                'the per-sprite scenery cook left resident art real room; check the budget')
        self.assertLess(out['global_headroom'], max(s['headroom'] for s in out['scenes'].values()))
        self.assertGreater(max(s['headroom'] for s in out['scenes'].values()), 200)


if __name__ == '__main__':
    unittest.main()
