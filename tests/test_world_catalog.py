"""Tests for exhaustive source-world classification and PlayerData inventory."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'tools'))
from world_catalog import (aggregate_player_data, classify_component_type,
                           classify_scene, player_data_keys, transition_points)


def scene(name, path='Assets/Scenes/A.unity', components=None, counts=None):
    return {'scene_name':name, 'path':path, 'component_types':components or {},
            'counts':counts or {}}


class WorldCatalogTests(unittest.TestCase):
    def test_boss_and_mixed_cinematic_roles_keep_source_evidence(self):
        result=classify_scene(scene('GG_Test',components={
            'BossSceneController':1,'CinematicPlayer':1},counts={'terrain_edges':2}))
        self.assertEqual(result['primary_role'],'boss_arena')
        self.assertEqual([x['feature'] for x in result['features']],
                         ['boss_content','cinematic_content'])
        self.assertTrue(result['source_included'])

    def test_unrecognized_scene_is_retained_explicitly(self):
        result=classify_scene(scene('Mystery'))
        self.assertEqual(result['primary_role'],'support_scene_unresolved')
        self.assertIsNone(result['exclusion'])

    def test_player_data_keys_ignore_variable_bound_names(self):
        fields={'boolName':{'useVariable':0,'value':'hasDash'},
                'otherName':{'useVariable':1,'name':'Key','value':'hasWalljump'},
                'fsmName':{'useVariable':0,'value':'Control'}}
        self.assertEqual(player_data_keys('GetPlayerDataBool',fields),
                         [{'field':'boolName','key':'hasDash','binding':'literal'}])

    def test_player_data_key_can_resolve_from_fsm_initial_value(self):
        fields={'boolName':{'useVariable':1,'name':'PD Bool Name','value':''}}
        fsm={'variables':{'stringVariables':[{'name':'PD Bool Name',
                                              'value':'gotCharm_1'}]}}
        self.assertEqual(player_data_keys('PlayerDataBoolTest',fields,fsm),
            [{'field':'boolName','key':'gotCharm_1','binding':'fsm_variable_initial',
              'variable':'PD Bool Name'}])

    def test_player_data_aggregation_keeps_producers_and_consumers(self):
        base={'source_scene':'A','source_file':'level1','fsm_source':'level1:1',
              'fsm_name':'Control','state':'S','action_index':0}
        rows=[{**base,'action':'SetPlayerDataBool','direction':'producer',
               'keys':[{'field':'boolName','key':'hasDash'}]},
              {**base,'action':'GetPlayerDataBool','direction':'consumer',
               'keys':[{'field':'boolName','key':'hasDash'}]}]
        result,unresolved=aggregate_player_data(rows)
        self.assertEqual((result[0]['producers'],result[0]['consumers']),(1,1))
        self.assertEqual(unresolved,[])

    def test_every_component_type_has_a_category_and_owner_class(self):
        self.assertIn('bosses',classify_component_type('BossSceneController'))
        self.assertEqual(classify_component_type('UnknownCustomThing'),['unclassified'])

    def test_transition_point_retains_serialized_flags_and_trigger_shape(self):
        geometry={'scene':{'scene_name':'A','file':'level1'},
            'colliders':[{'source':'level1:3','type':'BoxCollider2D','trigger':True,
                          'world_shape':{'kind':'box'}}],
            'gates':[{'source':'level1:2','game_object':'level1:1','name':'right1',
                'position':[1,2,0],'enabled':True,'active_self':True,
                'active_hierarchy':True,'target_scene':'B','entry_point':'left1',
                'entry_offset':{'x':-1,'y':0},'colliders':['level1:3'],
                'serialized':{'entryDelay':.5,'alwaysEnterLeft':1,'customFade':1}}]}
        row=transition_points(geometry)[0]
        self.assertEqual(row['serialized']['entryDelay'],.5)
        self.assertEqual(row['trigger_shapes'][0]['world_shape'],{'kind':'box'})


if __name__ == '__main__':
    unittest.main()
