"""Global scripted-transition inventory keeps literal and variable evidence distinct."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'tools'))
from world_transitions import (bind_dynamic_producers, binding, scan_components,
                               target_resolution, variable_reference_name, variable_table)


class WorldTransitionTests(unittest.TestCase):
    def test_variable_binding_retains_declaration_without_claiming_runtime_value(self):
        fsm = {'variables': {'stringVariables': [
            {'name':'Destination', 'value':'Town', 'useVariable':1}]}}
        variables = variable_table(fsm)
        target = binding({'useVariable':1, 'name':'Destination', 'value':''}, variables)
        self.assertEqual(target['binding'], 'fsm_variable')
        self.assertEqual(target['declarations'][0]['initial_value'], 'Town')
        result = target_resolution(target, {'Town':[{'file':'level7','index':7}]})
        self.assertEqual(result['resolution'], 'dynamic_variable')
        self.assertEqual(result['initial_target_candidates'], ['level7'])

    def test_literal_resolution_requires_an_exact_catalog_name(self):
        scenes = {'Town':[{'file':'level7','index':7}]}
        self.assertEqual(target_resolution({'binding':'literal','value':''}, scenes)['resolution'],
                         'empty_target')
        self.assertEqual(target_resolution({'binding':'literal','value':'Town'}, scenes),
                         {'resolution':'mapped_scene','target_file':'level7','target_index':7})
        self.assertEqual(target_resolution({'binding':'literal','value':'Missing'}, scenes)['resolution'],
                         'unresolved_scene')
        folded = target_resolution({'binding':'literal','value':'town'}, scenes)
        self.assertEqual(folded['resolution'], 'unresolved_scene')
        self.assertEqual(folded['casefold_candidates'], ['level7'])

    def test_scan_preserves_action_and_activation_metadata(self):
        # Compact PlayMaker type 18 points at fsmStringParams[0/1].
        action = {'actionNames':['HutongGames.PlayMaker.Actions.BeginSceneTransition'],
            'actionEnabled':[1], 'actionStartIndex':[0],
            'paramName':['sceneName','entryGateName'], 'paramDataType':[18,18],
            'paramDataPos':[0,1], 'paramByteDataSize':[0,0], 'byteData':[],
            'fsmStringParams':[{'useVariable':0,'name':'','value':'Town'},
                               {'useVariable':0,'name':'','value':'left1'}]}
        document = {'format':'HKWORLDCOMP01',
            'scene':{'scene_name':'Tutorial_01','file':'level6'}, 'objects':[
                {'source':'level6:1','type':'GameObject',
                 'data':{'m_Name':'Great Door','m_IsActive':False}},
                {'source':'level6:2','type':'PlayMakerFSM','data':{'m_GameObject':{'m_PathID':1},
                    'm_Enabled':True,'fsm':{'name':'Control','variables':{},
                    'states':[{'name':'Move','actionData':action}]}}}]}
        rows, errors = scan_components(document,
            {'Town':[{'file':'level7','index':7}]}, {('Town','left1'):['level7:9']})
        self.assertEqual(errors, [])
        self.assertEqual(rows[0]['resolution'], 'mapped_gate')
        self.assertEqual(rows[0]['target_gate_source_ids'], ['level7:9'])
        self.assertFalse(rows[0]['game_object_active_self'])

    def test_generic_fsm_var_names_are_recognized(self):
        self.assertEqual(variable_reference_name({'useVariable':1,
                         'variableName':'Return Scene','type':4}), 'Return Scene')
        self.assertIsNone(variable_reference_name({'useVariable':0,
                                                   'variableName':'Return Scene'}))

    def test_dynamic_producer_binding_preserves_runtime_storage_chain(self):
        rows = [{'resolution':'dynamic_variable','source_file':'level1',
            'fsm_source':'level1:2','fsm_name':'Dream Return',
            'game_object_source':'level1:1',
            'target':{'name':'Return Scene','declarations':[{
                'category':'stringVariables','initial_value':'Crossroads_10'}]}}]
        writer = {'kind':'player_data_read','source_key_or_value':'dreamReturnScene'}
        upstream = {'kind':'player_data_write','key':'dreamReturnScene'}
        counts,gate_counts = bind_dynamic_producers(rows,
            ({('level1','level1:2','Return Scene'):[writer]}, {},
             {'dreamReturnScene':[upstream]}, {}),
            {'Crossroads_10':[{'file':'level46','index':46}]})
        self.assertEqual(counts, {'runtime_storage_or_constant_read':1})
        self.assertEqual(gate_counts,{})
        flow = rows[0]['target_dataflow']
        self.assertEqual(flow['upstream_serialized_writers'][0]['count'], 1)
        self.assertEqual(flow['serialized_target_candidates'][0]['target_files'], ['level46'])

    def test_unresolved_runtime_target_gets_an_owner(self):
        rows = [{'resolution':'dynamic_variable','source_file':'level1',
            'fsm_source':'level1:2','fsm_name':'Boss','game_object_source':'level1:1',
            'target':{'name':'To Scene','declarations':[{
                'category':'stringVariables','initial_value':''}]}}]
        counts,gate_counts = bind_dynamic_producers(rows, ({},{},{},{}), {})
        self.assertEqual(counts, {'runtime_or_external_unresolved':1})
        self.assertEqual(gate_counts,{})
        self.assertEqual(rows[0]['target_dataflow']['owner_task'],
                         'P09/P22 scene-transition runtime')


if __name__ == '__main__':
    unittest.main()
