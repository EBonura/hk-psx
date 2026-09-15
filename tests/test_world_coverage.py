"""Synthetic whole-world coverage aggregation tests."""
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'tools'))
from world_coverage import aggregate_gaps, build, role_signals, transition_summary


def report():
    return {'fingerprint':'abc', 'inputs_unchanged':True,
        'run':{'status':'verified'},
        'coverage':{'catalog_scenes':2,'processed_scenes':2,'imported_scenes':0,
            'partial_scenes':2,'failed_scenes':0,'all_scenes_processed':True,
            'all_geometry_resolved':False,'ps1_packing':'not_run','gameplay_validation':'not_run',
            'counts':{'sprites':4}, 'systems':{
                'RestBench':{'instances':1,'scene_indices':[1],'gameplay_support':'not_evaluated'},
                'PlayMakerFSM':{'instances':3,'scene_indices':[0,1],'gameplay_support':'not_evaluated'}}},
        'serialized_fsm_actions':{'SendEvent':{'instances':5,'scene_indices':[0,1]}},
        'scenes':[
            {'index':0,'file':'level0','scene_name':'A','status':'partial',
             'counts':{'terrain_edges':2,'gates':1},'component_types':{'PlayMakerFSM':2},
             'errors':[{'source':'level0:1','type':'Mesh','error':'missing'}],
             'unsupported':[{'source':'level0:2','type':'Trail','reason':'unsupported'}]},
            {'index':1,'file':'level1','scene_name':'B','status':'partial',
             'counts':{'terrain_edges':1},'component_types':{'PlayMakerFSM':1,'RestBench':1},
             'errors':[{'source':'level1:1','type':'Mesh','error':'missing'}],
             'unsupported':[]}]}


def graph():
    return {'room_count':2,'edge_count':2,'rooms':[], 'unresolved':[], 'edges':[
        {'source_scene':'A','source_file':'level0','source_id':'level0:4','gate_name':'right1',
         'target_scene':'B','entry_point':'left1','resolution':'mapped_gate'},
        {'source_scene':'B','source_file':'level1','source_id':'level1:4','gate_name':'door1',
         'target_scene':'','entry_point':'','resolution':'empty_target'}]}


class CoverageTests(unittest.TestCase):
    def test_gaps_group_by_category_type_and_detail(self):
        gaps=aggregate_gaps(report())
        self.assertEqual((gaps[0]['type'],gaps[0]['instances'],gaps[0]['scene_count']),('Mesh',2,2))
        self.assertEqual(gaps[0]['example_sources'],['level0:1','level1:1'])
        self.assertEqual(gaps[1]['category'],'unsupported')

    def test_role_signals_are_evidence_not_final_classification(self):
        signals=role_signals(report()['scenes'][1])
        self.assertEqual([x['role'] for x in signals],['bench','gameplay_candidate'])

    def test_transition_summary_preserves_every_nonmapped_edge(self):
        value=transition_summary(graph())
        self.assertEqual(value['resolutions'],{'empty_target':1,'mapped_gate':1})
        self.assertEqual(value['mapped_gate_percent'],50.0)
        self.assertEqual(value['unresolved_edges'][0]['source_scene'],'B')

    def test_build_rejects_unverified_source_import(self):
        source=report();source['inputs_unchanged']=False
        with self.assertRaisesRegex(ValueError,'not complete'):
            build(source,graph())

    def test_build_keeps_support_and_classification_unproven(self):
        data=build(report(),graph(),{'complete':True,'source_hashes':{'level0':{}},
            'textures':{'t':{}},'sprites':{'s':{}},'animations':{'a':{}},'rooms':[]})
        self.assertEqual(data['scenes'][0]['final_classification'],'requires_source_or_reference_evidence')
        self.assertEqual(data['systems'][0]['gameplay_support'],'not_evaluated')
        self.assertEqual(data['fsm_actions'][0]['runtime_support'],'not_evaluated')
        self.assertEqual(data['dependency_inventory']['textures'],1)


if __name__ == '__main__':
    unittest.main()
