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
    return {'regions':[row],'scenes':[{'scene_id':0,'scene_name':'Tutorial_01','file':'level6','gates':[],
                       'camera_tilemap':{'width':32,'height':18}}]}

class WorldMetadata(unittest.TestCase):
    def generate(self,data):
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory);(root/'data').mkdir()
            # The guest's camera table has a row for every catalogued scene;
            # these fixtures describe scene 0, so the rest get a stand-in tilemap.
            present={scene['scene_id'] for scene in data['scenes']}
            data=dict(data,scenes=data['scenes']+[{'scene_id':scene,'scene_name':f'Fixture_{scene}','file':f'level{scene}','gates':[],
                                                    'camera_tilemap':{'width':32,'height':18}} for scene in range(world.SCENES) if scene not in present])
            with patch.object(world,'ROOT',root), patch.object(world,'flat_floor_catalog',return_value=[[] for _ in data['regions']]):return world.generate(data)

    def bank(self,data):
        """Objects of the scene-0 HKWMTA01 bank as (kind, state, flags, extra, index lists)."""
        import struct
        from world_metadata import encode_scene
        payload,_=encode_scene(data['scenes'][0],[r for r in data['regions'] if r['scene_id']==0])
        offset,count,stride=struct.unpack_from('<3I',payload,76)
        ioffset,icount,_=struct.unpack_from('<3I',payload,112)
        indices=struct.unpack_from(f'<{icount}H',payload,ioffset)
        out=[]
        for i in range(count):
            fields=struct.unpack_from('<IIHH4i2I3i',payload,offset+i*stride)
            lists=[list(indices[w&0xFFFF:(w&0xFFFF)+(w>>16)]) for w in fields[10:]]
            out.append(dict(kind=fields[2],state=fields[1],flags=fields[3],extra=fields[10:],lists=lists))
        return out

    def test_actual_persistence_flags(self):
        for flags,expected in [([],False),([{'dont_save':False,'semi_persistent':False}],True),
                ([{'dont_save':True,'semi_persistent':False}],False),
                ([{'dont_save':False,'semi_persistent':True}],False)]:
            data=report();data['regions'][0]['breakables'][0]['persistence']=flags
            self.assertEqual(self.bank(data)[0]['flags']&1,int(expected))
        self.assertNotIn('Breakable {',self.generate(report()))

    def test_state_alias_and_owned_index_fail_closed(self):
        data=report();other=copy.deepcopy(data['regions'][0]);other['chunk_id']=2
        other['breakables'][0]['source']='level6:11';data['regions'].append(other)
        with self.assertRaisesRegex(ValueError,'aliases'):self.bank(data)
        data=report();data['regions'][0]['breakables'][0]['edge_indices']=[1]
        with self.assertRaisesRegex(ValueError,'edge index'):self.bank(data)
        data=report();data['regions'][0]['breakables'][0]['state_index']=128
        with self.assertRaisesRegex(ValueError,'breakable state'):self.bank(data)
        self.assertEqual(self.bank(report())[0]['lists'],[[0],[1],[0]])

    def test_mask_uses_authored_duration_and_bound_draws(self):
        data=report();prop=data['regions'][0]['breakables'][0]
        prop['mask_fades']=[{'ticks_60hz':60,'target_alpha':0,'ease':'linear',
                           'renderers':[{'initial_alpha':1}],'draw_indices':[1]}]
        objects=self.bank(data)
        self.assertEqual(objects[0]['flags']>>6,60)
        self.assertEqual((objects[1]['kind'],objects[1]['state'],objects[1]['lists'][0],objects[1]['extra'][1]),(6,3,[1],60))
        prop['mask_fades'][0]['draw_indices']=[2]
        with self.assertRaisesRegex(ValueError,'draw'):self.bank(data)

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
        remote=self.bank(data)[-1]
        self.assertEqual((remote['kind'],remote['state'],remote['lists'][0],remote['extra'][1:]),(7,3,[1],(60,60)))
        before=copy.deepcopy(data)
        world.postpack_masks(data,{1:['door','level6:11301'],2:['floor','level6:11301']})
        self.assertEqual(data,before)
        distant['remote_mask_bindings'][0]['state_index']=128
        with self.assertRaisesRegex(ValueError,'mask owner state'):self.bank(data)

    def test_collision_coverage_is_distinct_from_activation(self):
        import struct
        from world_metadata import encode_scene
        data=report();data['regions'][0]['collision_bounds']=[-3,-5,13,15]
        payload,_=encode_scene(data['scenes'][0],data['regions'])
        offset=struct.unpack_from('<I',payload,64)[0]
        self.assertEqual(struct.unpack_from('<4i',payload,offset+20),(-196608,-327680,851968,983040))
        self.assertNotIn('collision_bounds:',self.generate(data))

    def test_damagehero_integer_convention_is_not_hazard_enum(self):
        # Hazards are cooked only into the HKWMTA01 bank; the damage word carries
        # the DamageHero respawn convention in its upper half.
        import struct
        from world_metadata import encode_scene
        scene={'scene_id':0,'scene_name':'S','file':'level6'}
        row={'chunk_id':1,'activation_bounds':[0,0,10,10],'camera_bounds':[0,0,10,10]}
        hazard={'source':'level6:7','damage':1,'bounds':[1,1,2,2],'position':[1,1],
                'world_polygons':[[[1,1],[2,1],[2,2],[1,2]]]}
        for hit_type,expected in [(0,False),(1,False),(2,True),(3,True)]:
            payload,_=encode_scene(scene,[dict(row,hazards=[dict(hazard,hazard_type=hit_type)])])
            offset=struct.unpack_from('<I',payload,76)[0]
            self.assertEqual(struct.unpack_from('<i',payload,offset+40)[0],1|(int(expected)<<16))
        for hit_type in [4,5,6,-1,True]:
            with self.assertRaisesRegex(ValueError,'unsupported DamageHero'):
                encode_scene(scene,[dict(row,hazards=[dict(hazard,hazard_type=hit_type)])])

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
