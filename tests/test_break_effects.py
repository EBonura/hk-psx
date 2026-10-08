"""Source-free tests of break-particle curves, art mapping and guarded allocation."""
import math,sys,unittest
from pathlib import Path
from types import SimpleNamespace
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
import break_effects as b
class BreakEffectsTests(unittest.TestCase):
    def curve(self,points):return {'m_Curve':[dict(time=t,value=v,inSlope=d,outSlope=d,weightedMode=0)for t,v,d in points]}
    def test_two_curves_preserve_independent_source_slopes_and_reverse_ranges(self):
        c={'minMaxState':2,'minScalar':2,'scalar':4,'minCurve':self.curve([(0,-1,0),(1,0,0)]),'maxCurve':self.curve([(0,1,0),(1,0,0)])}
        self.assertEqual(b.values(c,0),[-2,4]);self.assertEqual(b.values(c,1),[0,0]);self.assertEqual(b.values(c,.5),[-1,2])
        self.assertEqual(b.values({'minMaxState':3,'minScalar':2,'scalar':1}),[2,1])
        c['maxCurve']['m_Curve'][0]['weightedMode']=1
        with self.assertRaisesRegex(ValueError,'weighted'):b.values(c,.5)
    def test_fixed_point_rejects_unbounded_or_nonfinite_source(self):
        for x in [math.inf,math.nan,32768,-32768]:
            with self.assertRaises(ValueError):b.q(x)
        self.assertEqual(b.q(-.5),-32768)


class ParticleScalingTests(unittest.TestCase):
    def matrix(self,scale=.5):
        # A uniform scale and90-degree world rotation, with nonzero origin.
        return [[0,-scale,0,13],[scale,0,0,7],[0,0,scale,2],[0,0,0,1]]
    def source(self,mode=0):
        c=lambda x:dict(minMaxState=0,scalar=x)
        disabled=dict(enabled=False)
        ps={key:dict(disabled)for key in ('SizeModule','RotationModule','ColorModule','UVModule','ForceModule','VelocityModule','ClampVelocityModule','RotationBySpeedModule','CollisionModule')}
        ps.update(playOnAwake=True,looping=False,scalingMode=mode,moveWithTransform=1,lengthInSec=.1,
            InitialModule=dict(enabled=True,size3D=False,rotation3D=False,gravitySource=0,gravityModifier=c(0),
                startColor=dict(minMaxState=0,maxColor=dict(r=1,g=1,b=1,a=1)),startLifetime=c(2),
                startSpeed=c(10),startSize=c(.6),startRotation=c(0),maxNumParticles=128),
            ShapeModule=dict(enabled=True,type=10,m_Position=dict(x=0,y=0,z=0),m_Rotation=dict(x=0,y=0,z=0),
                m_Scale=dict(x=1,y=1,z=1),randomDirectionAmount=0,sphericalDirectionAmount=0,randomPositionAmount=0,
                radius=dict(mode=0,value=1),arc=dict(mode=0,value=180)),
            EmissionModule=dict(enabled=True,m_BurstCount=0,rateOverDistance=c(0),rateOverTime=c(10)))
        ps['ForceModule'].update(enabled=True,inWorldSpace=True,randomizePerFrame=False,x=c(0),y=c(-10),z=c(0))
        ps['VelocityModule'].update(enabled=True,inWorldSpace=True,x=c(0),y=c(5),z=c(0),speedModifier=c(1),
            **{k:c(0)for k in ('orbitalX','orbitalY','orbitalZ','orbitalOffsetX','orbitalOffsetY','orbitalOffsetZ','radial')})
        ps['ClampVelocityModule'].update(enabled=True,separateAxis=False,drag=c(0),magnitude=c(3),dampen=1)
        ps['RotationBySpeedModule']['range']=dict(x=3,y=12)
        ps['CollisionModule'].update(m_Bounce=c(.5),m_Dampen=c(.1),m_EnergyLossOnCollision=c(.2),minKillSpeed=2,radiusScale=.63)
        return ps
    def test_uniform_hierarchy_native_world_parameter_contract(self):
        ps=self.source();scale=b.particle_scale(ps,self.matrix())
        fields=b.style(ps,-9.81,scale)
        self.assertEqual(scale,.5)
        self.assertEqual(fields['speed'],[5*65536]*2)
        self.assertEqual(fields['size'],[round(.3*65536)]*2)
        self.assertEqual(fields['force'][1],[-5*65536]*2)
        self.assertEqual(fields['velocity'][1],[round(2.5*65536)]*2)
        # Native scaled force/velocity do not rotate with a world emitter; the
        # original source magnitude3 clamp is demonstrably not scaled to1.5.
        self.assertEqual(fields['force'][0],[0,0])
        self.assertEqual(fields['limit'],3*65536)
        self.assertEqual(fields['kill_speed'],2*65536)
        self.assertEqual(fields['radius_scale'],round(.63*65536))
        self.assertEqual(fields['life'],[120,120])
        self.assertEqual(fields['radius'],65536) # already scaled by basis once
    def test_shape_mode_preserves_existing_unscaled_parameters(self):
        ps=self.source(2)
        scale=b.particle_scale(ps,self.matrix())
        self.assertEqual(scale,1)
        self.assertEqual(b.style(ps,-9.81,scale),b.style(ps,-9.81))
    def test_unrepresented_hierarchy_shapes_fail_closed(self):
        ps=self.source()
        for value in (0,float('nan'),float('inf')):
            with self.assertRaisesRegex(ValueError,'uniform'):b.particle_scale(ps,self.matrix(value))
        bad=self.matrix();bad[0][1]=-.6
        with self.assertRaisesRegex(ValueError,'uniform'):b.particle_scale(ps,bad)
        planar=self.matrix();planar[2][2]=.49
        self.assertEqual(b.particle_scale(ps,planar),.5)
        tilted=self.matrix();tilted[0][2]=.1
        with self.assertRaises(ValueError):b.particle_scale(ps,tilted)
        ps['ForceModule']['z']['scalar']=1
        with self.assertRaisesRegex(ValueError,'XY plane'):b.particle_scale(ps,self.matrix())
        ps['ForceModule']['z']['scalar']=0
        bad=[[.5,.3,0,0],[0,.4,0,0],[0,0,.5,0],[0,0,0,1]]
        with self.assertRaisesRegex(ValueError,'shear'):b.particle_scale(ps,bad)
        ps['moveWithTransform']=0
        with self.assertRaisesRegex(ValueError,'world simulation'):b.particle_scale(ps,self.matrix())
        ps['moveWithTransform']=1;ps['InitialModule']['gravityModifier']['scalar']=1
        with self.assertRaisesRegex(ValueError,'gravity'):b.particle_scale(ps,self.matrix())
        ps['scalingMode']=1
        with self.assertRaisesRegex(ValueError,'scaling mode'):b.particle_scale(ps,self.matrix())
    def test_each_refused_simulation_property_says_which_one_it_was(self):
        # One message used to cover four unrelated properties, and it named the
        # wrong one: every hidden-wall and cracked-floor emitter it refused was a
        # one-shot system waiting for an FSM PlayParticleEmitter.
        for field,value,reason in (('looping',True,'looping'),('playOnAwake',False,'play on awake'),
                                   ('scalingMode',3,'scaling mode 3'),('moveWithTransform',2,'simulation space 2')):
            ps=self.source(2);ps[field]=value
            with self.assertRaisesRegex(ValueError,reason):b.style(ps,-9.81)
    def test_an_emitter_an_action_plays_does_not_need_play_on_awake(self):
        ps=self.source(2);ps['playOnAwake']=False
        with self.assertRaisesRegex(ValueError,'no resolved action plays it'):b.style(ps,-9.81)
        self.assertEqual(b.style(ps,-9.81,played=True),b.style(self.source(2),-9.81))
        # `played` opens that one gate and nothing else.
        ps['looping']=True
        with self.assertRaisesRegex(ValueError,'looping'):b.style(ps,-9.81,played=True)
    def test_a_source_emitter_that_emits_nothing_is_reproduced_by_drawing_nothing(self):
        ps=self.source(2)
        self.assertFalse(b.emits_nothing(ps))
        for change in ({'enabled':False},{'rateOverTime':dict(minMaxState=0,scalar=0)}):
            probe=self.source(2);probe['EmissionModule'].update(change)
            self.assertTrue(b.emits_nothing(probe))
        # A burst, a distance rate, or a rate given as a curve is a real emitter:
        # silence has to be provable from the constant form alone.
        for change in ({'m_BurstCount':1},{'rateOverDistance':dict(minMaxState=0,scalar=5)},
                       {'rateOverTime':dict(minMaxState=1,scalar=1)}):
            probe=self.source(2)
            probe['EmissionModule'].update(dict({'rateOverTime':dict(minMaxState=0,scalar=0)},**change))
            self.assertFalse(b.emits_nothing(probe))
    def test_unity_euler_basis_is_the_z_then_x_then_y_order(self):
        rows=b.euler_basis((-72.5,-180.0,-180.0))
        for row,want in zip(rows,([1,0,0],[0,-.30071,.95372],[0,-.95372,-.30071])):
            for got,expected in zip(row,want):self.assertAlmostEqual(got,expected,places=5)
        for row,want in zip(b.euler_basis((0,0,0)),([1,0,0],[0,1,0],[0,0,1])):
            for got,expected in zip(row,want):self.assertAlmostEqual(got,expected,places=6)
    def prefab(self,scale=(1,1,1)):
        """A one-object prefab carrying a stale authoring position, as the source does."""
        trees={1:dict(m_Name='Dust Break Wall',m_Component=[dict(component=dict(m_PathID=2,m_FileID=0))],m_IsActive=True),
               2:dict(m_GameObject=dict(m_PathID=1,m_FileID=0),m_Father=dict(m_PathID=0,m_FileID=0),m_Children=[],
                      m_LocalRotation=dict(x=0,y=0,z=0,w=1),m_LocalPosition=dict(x=81.6,y=50.1,z=0),
                      m_LocalScale=dict(x=scale[0],y=scale[1],z=scale[2]))}
        objects={i:SimpleNamespace(path_id=i,type=SimpleNamespace(name=name))
                 for i,name in ((1,'GameObject'),(2,'Transform'))}
        file=SimpleNamespace(name='sharedassets31.assets',objects=objects,externals=[])
        return SimpleNamespace(typename=lambda o:o.type.name,read=lambda o:trees[o.path_id],
                               ref=lambda f,r:f.objects[r['m_PathID']]),file
    def test_an_instantiated_prefab_takes_the_spawn_transform_not_its_authoring_one(self):
        source,file=self.prefab()
        view=b.PrefabView(source,file,1,(54.57,47.92,-.12),(-72.5,-180.0,-180.0))
        world=view.world(view.go_transform[1])
        # Instantiate replaces the prefab root's serialized position, so the
        # emitter sits where the break is and never at (81.6, 50.1).
        for got,expected in zip([world[i][3]for i in range(3)],(54.57,47.92,-.12)):
            self.assertAlmostEqual(got,expected,places=5)
        for row,want in zip(world,([1,0,0],[0,-.30071,.95372],[0,-.95372,-.30071])):
            for got,expected in zip(row[:3],want):self.assertAlmostEqual(got,expected,places=5)
        self.assertEqual(view.subtree(),[1])
        # Instantiate keeps the prefab's own scale, which a Shape-mode system
        # spends on emission positions through this basis.
        source,file=self.prefab(scale=(1,.1,1))
        columns=b.PrefabView(source,file,1,(0,0,0),(0,0,0)).world(2)
        self.assertAlmostEqual(columns[1][1],.1,places=6)

if __name__=='__main__':unittest.main()
