"""Tests for serialized transition progression evidence."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from world_progression import condition_status, one_way_sources, state_distances


class WorldProgressionTests(unittest.TestCase):
    def test_state_distances_are_bounded_across_cycles(self):
        self.assertEqual(state_distances('A',{'A':{'B'},'B':{'A','C'}}),
                         {'A':0,'B':1,'C':2})

    def test_same_state_condition_is_stronger_than_reachable_candidate(self):
        evidence={'same_state_conditions':[{'action':'GetPlayerDataBool'}],
                  'reachable_condition_candidates':[{'action':'GetPlayerDataBool'}]}
        self.assertEqual(condition_status(evidence),'same_state_serialized_condition')

    def test_one_way_scene_pair_is_explicit(self):
        graph={'edges':[{'source_id':'a','source_scene':'A','target_scene':'B',
                         'target_file':'level2'},
                        {'source_id':'b','source_scene':'B','target_scene':'C',
                         'target_file':'level3'},
                        {'source_id':'c','source_scene':'C','target_scene':'B',
                         'target_file':'level2'}]}
        self.assertEqual(one_way_sources(graph),{'a'})


if __name__=='__main__':
    unittest.main()
