"""Source-free inverse mask action validation and cross-region binding tests."""
import copy
import pathlib
import struct
import sys
import unittest
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]/'host'))
from reveal_masks import verify_states, verify_ordinary_states, bind_regions, validate_palette
import test_world
report=test_world.report


def value(v=None,name=''):
    return {'value':v,'name':name,'useVariable':bool(name)}


def fade(alpha,time,inverse=False,pause=False):
    return {'gameObject':{'ownerOption':int(inverse),'gameObject':value(None,'Inverse Mask')},
        'alpha':value(alpha),'time':value(time),'delay':value(0),'includeChildren':value(True),
        'namedValueColor':value('_Color'),'easeType':21,'loopType':0,'realTime':value(False),
        'stopOnExit':value(True),'loopDontFinish':value(not pause),'startEvent':'','finishEvent':''}


def states():
    out={}
    for name,alpha in [('Idle',0),('Fade Out',1),('Fade In',0)]:
        f=[('iTweenFadeTo',fade(alpha,.01 if name=='Idle'else .5)),
           ('iTweenFadeTo',fade(1-alpha,.01 if name=='Idle'else .5,True))]
        trigger=('Trigger2dEvent',{'trigger':2 if name=='Fade Out'else 1,
          'sendEvent':'COVER'if name=='Fade Out'else'UNCOVER','collideTag':value('Player'),'collideLayer':value('')})
        out[name]=[trigger]+f if name=='Idle'else f+[('SetBoolValue',{'boolVariable':value(None,'Activated'),'boolValue':value(True),'everyFrame':False}),trigger]
    out['Pause']=[('FindChild',{'gameObject':{'ownerOption':0},'childName':value('Inverse Mask'),'storeResult':value(None,'Inverse Mask')}),
        ('iTweenFadeTo',fade(0,0,True,True)),('WaitForHeroInPosition',{'sendEvent':'FINISHED','skipIfAlreadyPositioned':value(False)}),
        ('Wait',{'finishEvent':'FINISHED','time':value(2),'realTime':False})]
    out['Hero Leave']=[]
    return out


def room():
    # One texture, two source draws, one palette, no pages required by layout helper.
    raw=bytearray(40+16+88+32);raw[:8]=b'HKROOM02'
    struct.pack_into('<6I',raw,8,0,1,2,0,0,0)
    struct.pack_into('<6HI',raw,40,0,0,0,1,1,0,0)
    struct.pack_into('<16H',raw,144,0,1,*([0x8000]*14))
    return raw


def controller():
    return {'controller':0,'source_id':77,'renderer_sources':['level6:mask'],'trigger':[[1,1],[2,1],[2,2],[1,2]],'fade_ticks':30,'initial_opacity':0}


class RevealTests(unittest.TestCase):
    def test_action_semantics_and_reversal_are_strict(self):
        self.assertEqual(verify_states(states(),{'Fade Time':.5}),30)
        for key,val in [('easeType',0),('stopOnExit',value(False)),('alpha',value(0)),('time',value(.3))]:
            data=states();data['Fade Out'][0][1][key]=val
            with self.assertRaises(ValueError):verify_states(data,{'Fade Time':.5})
        data=states();data['Fade Out'][-1][1]['trigger']=1
        with self.assertRaisesRegex(ValueError,'trigger'):verify_states(data,{'Fade Time':.5})
        data=states();data['Idle'].append(('Unknown',{}))
        with self.assertRaisesRegex(ValueError,'sequence'):verify_states(data,{'Fade Time':.5})
        data=states();data['Fade In'][0][1]['time']=value(None,'Unknown')
        with self.assertRaisesRegex(ValueError,'unknown FSM variable'):verify_states(data,{'Fade Time':.5})

    def test_ordinary_keeps_initial_cover_and_reverses_opacity(self):
        data=states();data['Idle']=data['Idle'][:1]
        for name,alpha in [('Fade Out',0),('Fade In',1)]:
            data[name].pop(1);data[name][0][1]['alpha']=value(alpha)
        data['Pause']=data['Pause'][2:];data['Pause'][1][1]['time']=value(1)
        self.assertEqual(verify_ordinary_states(data,{'Fade Time':.5}),30)
        inverse=copy.deepcopy(data);inverse['Fade Out'][0][1]['alpha']=value(1)
        with self.assertRaisesRegex(ValueError,'ordinary fade'):verify_ordinary_states(inverse,{'Fade Time':.5})
        data['Pause'][1][1]['time']=value(2)
        with self.assertRaisesRegex(ValueError,'ordinary pause'):verify_ordinary_states(data,{'Fade Time':.5})
        fixture=report();record=controller();record['initial_opacity']=128
        fixture['reveal_mask_scenes']={'0':{'controllers':[record]}}
        self.assertIn('initial_opacity:128',test_world.WorldMetadata().generate(fixture))
        record['initial_opacity']=127
        with self.assertRaisesRegex(ValueError,'initial opacity'):test_world.WorldMetadata().generate(fixture)

    def test_palette_guard_rejects_color_and_opaque_zero(self):
        raw=room();validate_palette(raw,0);validate_palette(raw,1)
        for entry,value_ in [(0,1),(1,0),(1,0x7fff),(2,0)]:
            changed=raw.copy();struct.pack_into('<H',changed,144+entry*2,value_)
            with self.assertRaisesRegex(ValueError,'binary black'):validate_palette(changed,0)
        changed=raw.copy();struct.pack_into('<H',changed,56,1)
        with self.assertRaisesRegex(ValueError,'texture outside'):validate_palette(changed,0)

    def test_scene_specs_survive_remote_regions_and_generate(self):
        data=report();distant=copy.deepcopy(data['regions'][0]);distant.update(chunk_id=2,activation_bounds=[80,10,96,30]);data['regions'].append(distant)
        by_scene={0:{'controllers':[controller()],'unsupported':[]}}
        bind_regions(data,by_scene,{1:['floor','level6:mask'],2:['level6:mask','rock']},{1:room(),2:room()})
        self.assertEqual([r['reveal_mask_bindings'][0]['draw']for r in data['regions']],[1,0])
        rust=test_world.WorldMetadata().generate(data)
        self.assertIn('source_id:77,trigger:&[[65536,65536]',rust)
        self.assertIn('pub static SCENE_REVEAL_MASKS: &[&[crate::reveal_masks::RevealMaskSpec]] = &[REVEAL_MASKS_SCENE_0,',rust)
        # Bindings live in the bank as one interleaved (controller, draw) list per region.
        self.assertNotIn('RevealMaskBinding',rust)
        objects=test_world.WorldMetadata().bank(dict(data,scenes=[dict(data['scenes'][0],reveal_controllers=1)]))
        bindings=[o for o in objects if o['kind']==8]
        self.assertEqual([o['lists'][0] for o in bindings],[[0,1],[0,0]])
        distant['reveal_mask_bindings'][0]['controller']=1
        with self.assertRaisesRegex(ValueError,'reveal controller'):
            test_world.WorldMetadata().bank(dict(data,scenes=[dict(data['scenes'][0],reveal_controllers=1)]))

    def test_duplicate_owner_and_pool_fail_closed(self):
        data=report();one=controller();two=dict(one,controller=1,source_id=78)
        with self.assertRaisesRegex(ValueError,'multiple reveal'):
            bind_regions(data,{0:{'controllers':[one,two]}},{1:['floor','level6:mask']},{1:room()})
        with self.assertRaisesRegex(ValueError,'exceeds16'):
            bind_regions(data,{0:{'controllers':[one]*17}},{1:['floor','level6:mask']},{1:room()})

if __name__=='__main__':unittest.main()
