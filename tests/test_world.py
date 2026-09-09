"""Metadata rejection and source checkpoint residency regressions."""
import copy
import pathlib
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]/'host'))
import world


def report():
    prop={'source':'level6:10','state_index':3,'hit_points':1,'box':[1,1,2,2],
          'hit_polygons':[[[1,1],[2,1],[2,2],[1,2]]],'off_draws':[0],'on_draws':[1],
          'edge_indices':[0],'persistence':[{'dont_save':False,'semi_persistent':False}]}
    row={'chunk_id':1,'scene_id':0,'activation_bounds':[0,0,10,10],
         'camera_bounds':[0,0,10,10],'draws':2,'edges':1,'grass':[],
         'breakables':[prop],'hazards':[],'actors':[]}
    return {'regions':[row],'scenes':[{'scene_id':0,'scene_name':'Tutorial_01','file':'level6','gates':[]}]}

class WorldMetadata(unittest.TestCase):
    def generate(self,data):
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory);(root/'data').mkdir()
            with patch.object(world,'ROOT',root), patch.object(world,'flat_floor_catalog',return_value=[[] for _ in data['regions']]):return world.generate(data)

    def test_actual_persistence_flags(self):
        for flags,expected in [([],False),([{'dont_save':False,'semi_persistent':False}],True),
                ([{'dont_save':True,'semi_persistent':False}],False),
                ([{'dont_save':False,'semi_persistent':True}],False)]:
            data=report();data['regions'][0]['breakables'][0]['persistence']=flags
            self.assertIn(f'persistent:{str(expected).lower()}',self.generate(data))

    def test_state_alias_and_owned_index_fail_closed(self):
        data=report();other=copy.deepcopy(data['regions'][0]);other['chunk_id']=2
        other['breakables'][0]['source']='level6:11';data['regions'].append(other)
        with self.assertRaisesRegex(ValueError,'aliases'):self.generate(data)
        data=report();data['regions'][0]['breakables'][0]['edge_indices']=[1]
        with self.assertRaisesRegex(ValueError,'edge index'):self.generate(data)
        data=report();data['regions'][0]['breakables'][0]['state_index']=128
        with self.assertRaisesRegex(ValueError,'breakable state'):self.generate(data)

    def test_mask_uses_authored_duration_and_bound_draws(self):
        data=report();prop=data['regions'][0]['breakables'][0]
        prop['mask_fades']=[{'ticks_60hz':60,'target_alpha':0,'ease':'linear',
                           'renderers':[{'initial_alpha':1}],'draw_indices':[1]}]
        self.assertIn('MaskFade {draws:&[1],ticks:60}',self.generate(data))
        prop['mask_fades'][0]['draw_indices']=[2]
        with self.assertRaisesRegex(ValueError,'draw index'):self.generate(data)

    def test_remote_mask_follows_renderer_without_copying_hit_shapes(self):
        data=report();owner=data['regions'][0]['breakables'][0]
        owner['mask_fades']=[{'source_fsm':'level6:12055','ticks_60hz':60,
            'target_alpha':0,'ease':'linear','renderers':[{'initial_alpha':1}],
            'renderer_sources':['level6:11301'],'draw_indices':[1]}]
        distant=copy.deepcopy(data['regions'][0]);distant.update(chunk_id=2,
            activation_bounds=[80,10,96,30],breakables=[])
        data['regions'].append(distant)
        world.postpack_masks(data,{1:['door','level6:11301'],2:['floor','level6:11301']})
        self.assertEqual(data['regions'][0]['remote_mask_bindings'],[])
        self.assertEqual(distant['breakables'],[])
        self.assertEqual(distant['remote_mask_bindings'][0]['state_index'],3)
        self.assertEqual(distant['remote_mask_bindings'][0]['fade']['draw_indices'],[1])
        self.assertIn('MaskBinding {owner:3,fade_ticks:60,fade:MaskFade {draws:&[1],ticks:60}}',self.generate(data))
        before=copy.deepcopy(data)
        world.postpack_masks(data,{1:['door','level6:11301'],2:['floor','level6:11301']})
        self.assertEqual(data,before)
        distant['remote_mask_bindings'][0]['state_index']=128
        with self.assertRaisesRegex(ValueError,'mask owner state'):self.generate(data)

    def test_collision_coverage_is_distinct_from_activation(self):
        data=report();data['regions'][0]['collision_bounds']=[-3,-5,13,15]
        self.assertIn('collision_bounds:[-196608,-327680,851968,983040]',self.generate(data))

    def test_damagehero_integer_convention_is_not_hazard_enum(self):
        data=report()
        hazard={'damage':1,'bounds':[1,1,2,2],'position':[1,1],
                'world_polygons':[[[1,1],[2,1],[2,2],[1,2]]]}
        for hit_type,expected in [(0,False),(1,False),(2,True),(3,True)]:
            data['regions'][0]['hazards']=[dict(hazard,hazard_type=hit_type)]
            self.assertIn(f'damage:1,hazard:{str(expected).lower()}',self.generate(data))
        for hit_type in [4,5,6,-1,True]:
            data['regions'][0]['hazards']=[dict(hazard,hazard_type=hit_type)]
            with self.assertRaisesRegex(ValueError,'unsupported DamageHero'):
                self.generate(data)

    def test_checkpoint_trigger_overlap_keeps_remote_spawn(self):
        data=report();other=copy.deepcopy(data['regions'][0]);other.update(chunk_id=2,activation_bounds=[10,0,20,10]);data['regions'].append(other)
        checkpoints=[{'source':'level6:100','bounds':[9,2,11,4],'spawn':[100,31]},
                     {'source':'level6:101','bounds':[40,2,41,4],'spawn':[3,2]}]
        with patch.object(world,'checkpoint_sources',return_value=checkpoints):
            world.postpack_checkpoints(data,None,{0:object()})
        self.assertEqual(data['checkpoint_policy']['unique_triggers'],2)
        for row in data['regions']:
            self.assertEqual(len(row['checkpoints']),1)
            self.assertEqual(row['checkpoints'][0]['spawn'],[100,31])

if __name__=='__main__':unittest.main()
