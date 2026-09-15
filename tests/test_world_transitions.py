"""Global scripted-transition inventory keeps literal and variable evidence distinct."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'tools'))
from world_transitions import binding, scan_components, target_resolution, variable_table


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


if __name__ == '__main__':
    unittest.main()
