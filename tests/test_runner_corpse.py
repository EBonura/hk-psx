"""Synthetic validated corpse contracts; no original assets needed by these tests."""
import copy
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from effects import corpse_source,append_corpse_art,generated_corpse


def fixture(runner=True):
    family='ZombieSwipeWalker' if runner else 'WalkLeftRight'
    actor={'game_object':1,'movement_supported':True,'movement_control':{'kind':family,'library_source':'sharedassets37.assets:149'}}
    scene=SimpleNamespace(file='scene',go_transform={1:1,2:2},world=lambda _:[[1,0,0],[0,1,0],[0,0,1]])
    death=dict(isCorpseRecyclable=False,corpseFacesRight=False,lowCorpseArc=False,rotateCorpse=False,
        corpseFlingSpeed=15,corpseSpawnPoint={'x':0.,'y':0. if runner else .5,'z':0.},
        corpsePrefab={'m_PathID':1},m_Enabled=True,playerDataName='ZombieRunner')
    special=('breaker','bigBreaker','chunker','deathStun','fungusExplode','goopExplode','hatcher','instantChunker','massless','resetRotation','spineBurst','zomHive')
    parts={
        'Corpse':dict.fromkeys(special,False)|{'landEffects':{'m_PathID':0},'m_Enabled':True},
        'Rigidbody2D':dict(m_BodyType=0,m_LinearDamping=0,m_GravityScale=.8,m_Constraints=4,m_Simulated=True,m_CollisionDetection=0),
        'ObjectBounce':dict(bounceFactor=.2 if runner else .3,speedThreshold=1,playSound=False,playAnimationOnBounce=False,sendFSMEvent=False,m_Enabled=True),
        'tk2dSprite':{'_color':{'r':1.,'g':1.,'b':1.,'a':1.},'_scale':{'x':1.,'y':1.,'z':1.}},
        'Transform':{'m_LocalScale':{'x':1.,'y':1.,'z':1.}},
        'tk2dSpriteAnimator':dict(library={'m_PathID':100},m_Enabled=True,playAutomatically=True,isRealtime=False,defaultClipId=0),
        'BoxCollider2D':dict(m_Offset={'x':.03125,'y':-.7265625},m_Size={'x':1.21875,'y':1.265625},m_Enabled=True,m_IsTrigger=False,m_EdgeRadius=0,m_Material={'m_PathID':101}),
    }
    timings=[(30,6,1),(12,2,8)]if runner else[(12,2,3),(12,2,2)]
    clips=[dict(name=name,fps=fps,wrapMode=wrap,loopStart=0,frames=[{'spriteId':i,'spriteCollection':{'m_PathID':102},'triggerEvent':False}for i in range(count)])
        for name,(fps,wrap,count)in zip(('Death Air','Death Land'),timings)]
    objects={}
    def obj(pid,kind,data,sid=None):
        value=SimpleNamespace(path_id=pid,kind=kind,data=data,sid=sid or f'fake:{pid}',assets_file='prefab')
        objects[pid]=value;return value
    go=obj(1,'GameObject',{'m_Name':'Corpse Zombie Basic One','m_Component':[]},'sharedassets37.assets:61'if runner else'sharedassets6.assets:467')
    for i,(kind,data)in enumerate(parts.items(),10):
        obj(i,kind,data);go.data['m_Component'].append({'component':{'m_PathID':i}})
    obj(100,'Animation',{'clips':clips},'sharedassets37.assets:149'if runner else'sharedassets6.assets:1113')
    obj(101,'PhysicsMaterial2D',dict(friction=.2,bounciness=0),'resources.assets:1073')
    obj(102,'Collection',{},'fake:collection')
    source=SimpleNamespace(ref=lambda file,ref:objects[ref['m_PathID']],read=lambda o:o.data,typename=lambda o:o.kind,sid=lambda o:o.sid)
    return source,scene,actor,death,parts,objects

class RunnerCorpseTests(unittest.TestCase):
    def extract(self,f):
        s,sc,a,death,_,_=f
        with patch('effects._components',return_value=[(1,'EnemyDeathEffects',death)]):return corpse_source(s,sc,a)
    def test_runner_exact_zero_offset_bounce_and_clip_contract(self):
        r=self.extract(fixture())
        self.assertEqual(r['source'],'sharedassets37.assets:61')
        self.assertEqual(r['bounds'],[-37888,-89088,41984,-6144])
        self.assertEqual(r['spawn_offset'],[0,0]);self.assertEqual(r['bounce_factor'],13107)
        self.assertEqual([(c['fps'],c['wrapMode'],len(c['frames']))for c in r['clips']],[(30,6,1),(12,2,8)])
    def test_crawler_contract_remains_separate(self):
        r=self.extract(fixture(False));self.assertEqual(r['bounce_factor'],19661);self.assertEqual(r['spawn_offset'],[0,32768])
        for mut in [lambda f:f[4]['ObjectBounce'].update(bounceFactor=.2),
                    lambda f:f[3].update(corpseSpawnPoint={'x':0.,'y':0.,'z':0.}),
                    lambda f:f[5][100].data['clips'][0].update(fps=30,wrapMode=6)]:
            f=fixture(False);mut(f)
            with self.assertRaises(ValueError):self.extract(f)
    def test_crawler_semantic_admission_does_not_depend_on_bundle_source_ids(self):
        f=fixture(False)
        f[5][1].sid='another_shared_bundle.assets:900'
        f[5][100].sid='another_shared_bundle.assets:901'
        r=self.extract(f)
        self.assertEqual(r['source'],'another_shared_bundle.assets:900')
        self.assertEqual(r['library'],'another_shared_bundle.assets:901')
        self.assertEqual(r['bounce_factor'],19661)
        f[4]['ObjectBounce']['bounceFactor']=.2
        with self.assertRaisesRegex(ValueError,'unsupported corpse bounce'):self.extract(f)
    def test_runner_rejects_changed_variant_without_weakening_source_guards(self):
        changes=[
            lambda f:f[2]['movement_control'].update(kind='Climber'),
            lambda f:f[5][1].data.update(m_Name='Corpse Other'),
            lambda f:setattr(f[5][100],'sid','sharedassets37.assets:146'),
            lambda f:f[3].update(corpseFlingSpeed=20),
            lambda f:f[3].update(corpseSpawnPoint={'x':0.,'y':.5,'z':0.}),
            lambda f:f[4]['ObjectBounce'].update(bounceFactor=.3),
            lambda f:f[4]['Corpse'].update(resetRotation=True),
            lambda f:f[4]['Corpse'].update(landEffects={'m_PathID':5}),
            lambda f:f[4]['BoxCollider2D'].update(m_IsTrigger=True),
            lambda f:f[4]['BoxCollider2D']['m_Size'].update(x=5.),
            lambda f:f[4]['Rigidbody2D'].update(m_Simulated=False),
            lambda f:f[4]['tk2dSpriteAnimator'].update(defaultClipId=1),
            lambda f:f[5][100].data['clips'][1].update(fps=15),
            lambda f:f[5][100].data['clips'][0]['frames'][0].update(triggerEvent=True),
            lambda f:f[5][101].data.update(friction=.5),
            lambda f:setattr(f[1],'world',lambda _:[[1,.1,0],[0,1,0],[0,0,1]]),
        ]
        for change in changes:
            f=fixture();change(f)
            with self.subTest(change=change),self.assertRaises(ValueError):self.extract(f)
    def test_two_admitted_instances_share_corpse_frames_and_default_pending_is_skipped(self):
        from PIL import Image
        from cook import Atlas
        s,sc,a,death,_,_=fixture();b=copy.deepcopy(a);b['game_object']=2
        atlas=Atlas();frames=[];clips=[]
        with patch('effects._components',return_value=[(1,'EnemyDeathEffects',death)]),patch('cook.tk_sprite',side_effect=lambda s,f,c,i,t:(Image.new('RGBA',(2,2),(i*24,255-i*24,32,255)),(0,0,1,1)))as sprite:
            append_corpse_art(s,sc,[a,b],atlas,frames,clips)
        self.assertEqual(len(frames),9);self.assertEqual(len(clips),2);self.assertEqual(sprite.call_count,8)
        self.assertEqual(a['corpse']['air_clip'],b['corpse']['air_clip']);self.assertEqual(a['corpse']['land_clip'],b['corpse']['land_clip'])
        # tk2d Single (6) on the one-frame Air clip cooks as the guest's Once (2).
        self.assertEqual(clips[0]['wrap'],2);self.assertEqual(clips[1]['count'],8)
        pending=copy.deepcopy(a);pending['movement_supported']=False;pending.pop('corpse')
        with patch('effects.corpse_source',side_effect=AssertionError('must not enable pending actor')):
            append_corpse_art(s,sc,[pending],atlas,frames,clips)
        self.assertNotIn('corpse',pending)
    def test_generated_bounce_is_required_and_bounded(self):
        r=dict(air_clip=1,land_clip=2,bounds=[-1,-1,1,1],spawn_offset=[0,0],bounce_factor=13107)
        self.assertIn('bounce_factor:13107',generated_corpse(r))
        for invalid in [-1,65537,True,.2]:
            with self.assertRaisesRegex(ValueError,'Q16'):generated_corpse(r|{'bounce_factor':invalid})
        del r['bounce_factor']
        with self.assertRaisesRegex(ValueError,'missing cooked'):generated_corpse(r)
