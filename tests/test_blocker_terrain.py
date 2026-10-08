"""The Blocker Terrain Block join: rows from the cook's own edge sources."""
import sys
import unittest
from pathlib import Path
from unittest import mock
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
import blocker_terrain


def report():
    blocker = {'name': 'Blocker', 'movement_supported': True, 'source': 'level50:4585',
               'game_object': 729, 'spec_source_id': 4585}
    return {'regions': [
        {'scene_id': 19, 'scene_file': 'level50', 'scene_name': 'X', 'actors': [blocker],
         'edge_sources': ['level50:1', 'level50:2936', 'level50:2936']},
        {'scene_id': 19, 'scene_file': 'level50', 'scene_name': 'X', 'actors': [blocker],
         'edge_sources': ['level50:2']},
        {'scene_id': 20, 'scene_file': 'level51', 'scene_name': 'Y', 'actors': [], 'edge_sources': []}]}


class BlockerTerrainTests(unittest.TestCase):
    def test_one_row_per_placement_and_every_block_edge_bound(self):
        r = report()
        rows = blocker_terrain.blockers(r)
        self.assertEqual(rows, [(19, 'level50', 'level50:4585', 729, 4585)])
        with mock.patch('battle_gates.neighbour_edges', return_value={}):
            bindings = blocker_terrain.bind(rows, ['level50:2936'], r)
        self.assertEqual(bindings, [(0, 0, [1, 2])])
        text = blocker_terrain.generate(rows, bindings)
        self.assertIn('pub static SOURCES:&[(u16,u32)]=&[(19,4585),];', text)
        self.assertIn('pub static REGIONS:&[(u16,u8,&[u16])]=&[(0,0,&[1,2]),];', text)

    def test_a_slot_over_the_scratch_budget_is_refused(self):
        r = report()
        rows = blocker_terrain.blockers(r)
        # Two block edges plus a slot one short of full: over by one, whatever
        # SCRIPT_EDGE_SLOTS is (the False Knight's floor raised it to 20).
        from battle_gates import edge_scratch_slots
        with mock.patch('battle_gates.neighbour_edges', return_value={0: edge_scratch_slots() - 1}):
            with self.assertRaisesRegex(ValueError, 'scripted edge exclusions'):
                blocker_terrain.bind(rows, ['level50:2936'], r)


if __name__ == '__main__':
    unittest.main()
