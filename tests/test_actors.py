"""Synthetic action-data and guest-contract checks, without retail fixtures."""
import struct
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from actors import action_fields, generated_actor_region, generated_vital_params, generated_nail_response_params, pogo_sources


class ActorCookTests(unittest.TestCase):
    def test_action_parameters_use_typed_offsets_not_searching_raw_bytes(self):
        data = {'actionStartIndex': [0], 'paramName': ['enabled', 'speed', 'clip'],
                'paramDataType': [1, 2, 18], 'paramDataPos': [0, 1, 0],
                'paramByteDataSize': [1, 4, 0], 'byteData': [1] + list(struct.pack('<f', 3.5)),
                'fsmStringParams': [{'useVariable': False, 'value': 'fixture_walk'}]}
        self.assertEqual(action_fields(data, 0), {'enabled': True, 'speed': 3.5, 'clip': 'fixture_walk'})
        data['fsmStringParams'][0]['useVariable'] = True
        with self.assertRaisesRegex(ValueError, 'dynamic action'):
            action_fields(data, 0)

    def test_supported_actor_requires_cooked_clips(self):
        with self.assertRaisesRegex(ValueError, 'missing cooked clips'):
            generated_actor_region({'actors': [{'source': 'fixture:1', 'movement_supported': True}]})
        self.assertEqual(generated_actor_region({'actors': [{'movement_supported': False}]}), '&[]')

    def test_nail_response_resamples_inclusive_source_fixed_steps(self):
        prefix={'RECOIL_HOR_STEPS':8,'RECOIL_HOR_VELOCITY':3.75,'BOUNCE_TIME':.25,
                'BOUNCE_VELOCITY':12,'RECOIL_DOWN_VELOCITY':0}
        result=generated_nail_response_params(prefix,.0199999929)
        for value in ['recoil_ticks:11','recoil_speed:245760','bounce_ticks:15','bounce_speed:786432','down_speed:0']:
            self.assertIn(value,result)

    def test_static_pogo_is_layer_qualified_and_rejects_special_dynamic_targets(self):
        gos={i:{'m_Layer':layer,'m_Name':f'fixture{i}'} for i,layer in enumerate([17,19,11,8,17,17,17,17],1)}
        objects={}
        for gid in gos:
            objects[100+gid]=('BoxCollider2D',{'m_GameObject':{'m_PathID':gid},'m_Enabled':1,
                'm_Offset':{'x':0,'y':0},'m_Size':{'x':2,'y':1},'m_IsTrigger':False})
        objects[201]=('NonBouncer',{'m_GameObject':{'m_PathID':5},'active':True})
        objects[202]=('BigBouncer',{'m_GameObject':{'m_PathID':6}})
        objects[203]=('PlayMakerFSM',{'m_GameObject':{'m_PathID':7}})
        objects[204]=('HealthManager',{'m_GameObject':{'m_PathID':8}})
        # Scene.sid names the file an object was serialized in, which is not the
        # scene's own file once a room has an additive scene merged into it.
        scene=SimpleNamespace(gos=gos,objects=objects,file=SimpleNamespace(name='fixture'),
            sid=lambda i:f'fixture:{i}',
            active=lambda gid:True,point=lambda gid,x,y:(x+gid*3,y,0))
        result=pogo_sources(scene)
        self.assertEqual([t['layer'] for t in result['targets']],[17,19,11])
        self.assertEqual([t['horizontal_and_up'] for t in result['targets']],[False,False,True])
        self.assertEqual(len(result['unsupported']),2)

    def test_vital_duration_rounding_and_fixed_width_bounds(self):
        values = {'max_health': 5, 'max_soul': 99, 'nail_damage': 5, 'soul_per_hit': 11,
                  'INVUL_TIME': 1.3, 'DAMAGE_FREEZE_DOWN': .001, 'DAMAGE_FREEZE_WAIT': .25,
                  'DAMAGE_FREEZE_UP': .05, 'RECOIL_DURATION': .2, 'death_wait_seconds': 2.85,
                  'RECOIL_VELOCITY': 15, 'enemy_hit_evasion_seconds': .2}
        result = generated_vital_params(values)
        for value in ['invulnerable_ticks:79', 'hazard_invulnerable_ticks:40', 'freeze_ticks:19',
                      'recoil_ticks:12', 'death_ticks:171', 'recoil_speed:983040']:
            self.assertIn(value, result)
        values['max_health'] = 65536
        with self.assertRaisesRegex(ValueError, 'fixed-width'):
            generated_vital_params(values)


if __name__ == '__main__':
    unittest.main()
