"""Original Breakable particle families, emitted into the existing bounded pool.

Fresh Windows source only. No room mutation and no replacement generic particles.
Source curves are sampled to a shared 33-entry fixed-point table; PS1 alpha uses
three prequantized levels. Unity collision/damping/RNG remain approximations.
"""
import hashlib,json,math,struct
from pathlib import Path
from source import Source,ROOT,rel
from particles import curve_value,gradient_alpha
# Slots in the guest particle pool, game/src/particles.rs CAPACITY. An emitter
# authored above this can never finish emitting even into an empty pool, so the
# host refuses it here rather than shipping a style the guest truncates. This
# bounds one emitter, not one break: a breakable whose emitters together author
# more than the pool holds still drops the overflow at spawn.
# 224 is the smallest multiple of 32 (the guest's pending-collision bitmap word)
# above the largest measured source emitter, Crossroads_09's 210.
POOL_CAPACITY=224
# Texture-sheet rows a random-row emitter may draw from. The catalogue's
# measured values are 3, 4 and 6; the bound exists so a malformed sheet cannot
# turn into hundreds of art cells, not because the guest counts rows.
MAX_SHEET_ROWS=8

def q(v):
    if not math.isfinite(v) or abs(v)>=32768:raise ValueError('break effect fixed point range')
    return round(v*65536)

def values(c,t=0):
    mode=c['minMaxState']
    if mode==0:return [c['scalar']]*2
    if mode==3:return [c['minScalar'],c['scalar']]
    if mode==1:return [curve_value(c['maxCurve'],t)*c['scalar']]*2
    if mode==2:return [curve_value(c['minCurve'],t)*c['minScalar'],curve_value(c['maxCurve'],t)*c['scalar']]
    raise ValueError('unknown particle curve')

def particle_scale(ps,matrix):
    """Reduce static uniform-XY planar Hierarchy scaling to world-space guest parameters.

    Unity6000 native ParticleProbe proves world launch/velocity/force scale, but
    the velocity clamp remains in world units. Shape mode scales only emission
    positions (already carried by Emitter.basis). Hierarchy also scales rendered
    particle size; GetCurrentSize returns the unscaled authoring value.
    Nonuniform-XY/sheared, Local scaling and local-space Hierarchy simulations need
    a different guest representation and remain explicit errors.
    """
    mode=ps['scalingMode']
    if mode==2:return 1.0
    # Local (1) scales by the emitter's own transform rather than the hierarchy.
    # The guest carries one scalar world scale that multiplies launch speed,
    # size, force and velocity, so only a uniform scale is expressible; the
    # measured Local emitters are nonuniform in XY, and no native ParticleProbe
    # case covers Local at all (the probe went with the Windows reference driver).
    columns=[[matrix[i][j]for i in range(3)]for j in range(3)]
    lengths=[math.sqrt(sum(v*v for v in col))for col in columns]
    if mode!=0:raise ValueError(f'unsupported particle scaling mode {mode} at scale '
                               f'[{lengths[0]:.4f},{lengths[1]:.4f},{lengths[2]:.4f}]')
    if ps['moveWithTransform']!=1:raise ValueError('Hierarchy particles require world simulation')
    scale=lengths[0]
    if not math.isfinite(scale) or scale<=0 or any(not math.isfinite(v)or v<=0 for v in lengths) or abs(lengths[1]-scale)>scale*1e-6:
        raise ValueError('Hierarchy particle XY scale must be positive and uniform')
    if any(abs(sum(a*b for a,b in zip(columns[i],columns[j])))>scale*scale*1e-6 for i in range(3)for j in range(i)):
        raise ValueError('Hierarchy particle transform must not shear')
    if any(abs(matrix[i][j])>1e-12 for i,j in ((0,2),(1,2),(2,0),(2,1))) or ps['ShapeModule']['type']!=10:
        raise ValueError('Hierarchy particle emission must stay in XY plane')
    if any(module['enabled'] and values(module['z'])!=[0,0]for module in (ps['ForceModule'],ps['VelocityModule'])):
        raise ValueError('Hierarchy particle motion must stay in XY plane')
    if values(ps['InitialModule']['gravityModifier'])!=[0,0]:
        raise ValueError('Hierarchy particle gravity scaling needs native validation')
    return scale


SILENT=object()
"""A ParticleSystem whose authored emission is exactly zero particles.

Distinct from None, which `part_emitter` already means as "not a particle
system". A caller that receives this has an emitter it reproduces exactly by
drawing nothing, not an omission to record.
"""

def emits_nothing(ps):
    """True when the source emission is provably zero particles.

    A disabled EmissionModule, or constant-zero rateOverTime and rateOverDistance
    with no bursts, emits nothing in Unity. Only the constant mode (`minMaxState`
    0) answers this: a curve that happens to read zero at t=0 is a real emitter.
    """
    em=ps['EmissionModule']
    if not em['enabled']:return True
    zero=lambda c:c['minMaxState']==0 and not c['scalar']
    return not em['m_BurstCount'] and zero(em['rateOverTime']) and zero(em['rateOverDistance'])


def style(ps,gravity,scale=1.0,played=False,random_gravity=False):
    im=ps['InitialModule'];shape=ps['ShapeModule'];uv=ps['UVModule'];em=ps['EmissionModule']
    enabled={k for k,v in ps.items()if k.endswith('Module')and isinstance(v,dict)and v.get('enabled')}
    supported={'InitialModule','ShapeModule','EmissionModule','SizeModule','RotationModule','ColorModule','UVModule','VelocityModule','ForceModule','ClampVelocityModule','RotationBySpeedModule','CollisionModule'}
    # An enabled SubModule whose every entry has a null emitter reference spawns
    # nothing in Unity, so it is not a reason to refuse the parent emitter.
    if 'SubModule' in enabled and all(not e['emitter']['m_PathID'] for e in ps['SubModule']['subEmitters']):
        enabled.discard('SubModule')
    if enabled-supported:raise ValueError('unsupported particle modules: '+str(enabled-supported))
    # These four unrelated properties shared one 'particle simulation space'
    # message, and it named the wrong one: every Breakable-adjacent emitter that
    # hit it was a one-shot system with `playOnAwake` off, waiting for an FSM
    # `PlayParticleEmitter`, not a simulation-space or scaling variant. `played`
    # is the caller saying it resolved the action that starts this emitter.
    if ps['looping']:raise ValueError('looping particle system')
    if not ps['playOnAwake']and not played:raise ValueError('particle system does not play on awake and no resolved action plays it')
    if ps['scalingMode']not in(0,2):raise ValueError(f"unsupported particle scaling mode {ps['scalingMode']}")
    if ps['moveWithTransform']not in(0,1):raise ValueError(f"unsupported particle simulation space {ps['moveWithTransform']}")
    if im['size3D']or im['rotation3D']or im['gravitySource']!=0 or em['m_BurstCount']or values(em['rateOverDistance'])!=[0,0]:raise ValueError('particle emission mode')
    if shape['type']not in(5,10)or not shape['enabled']:raise ValueError('particle shape')
    if any(shape['m_Position'][k]or shape['m_Rotation'][k]for k in 'xyz'):raise ValueError('particle shape transform')
    if shape['randomDirectionAmount']or shape['sphericalDirectionAmount']or shape['randomPositionAmount']or shape['radius']['mode']or shape['arc']['mode']:raise ValueError('particle shape random mode')
    # One column, whole-sheet animation, random row: each particle draws one row
    # for its life. The row count is not a property the guest cares about, only
    # the art it produces, which art_bank measures against the VRAM reservation.
    if uv['enabled']and(uv['tilesX']!=1 or uv['animationType']!=1 or uv['rowMode']!=1 or not 1<=uv['tilesY']<=MAX_SHEET_ROWS):raise ValueError('particle random row')
    start=im['startColor']
    # 0 is one authored colour. 2 is Unity's random-between-two-colors, which
    # the guest draws one seed for and reproduces as a per-particle lerp between
    # the ends. The RGB is free, because Particle already carries its own; the
    # alpha is not, and took Particle from 48 to 52 bytes. The gradient modes 1
    # and 3 would need a colour curve evaluated per phase, which no sampled
    # table here carries, and stay refused.
    if start['minMaxState']not in(0,2):raise ValueError(f"particle start color mode {start['minMaxState']}")
    ends=[start['maxColor']]*2 if start['minMaxState']==0 else[start['minColor'],start['maxColor']]
    colors=[[round(end[k]*128)for k in 'rgb']for end in ends]
    # The sampled alpha table is shared by every particle of a style, so the
    # louder of the two start alphas is baked into it and each particle carries
    # its own fraction of that. One authored colour gives [255,255], which the
    # guest's lerp short-circuits back to exactly the table value.
    initial_alpha=max(end['a']for end in ends)
    start_alpha=[round(end['a']/initial_alpha*255)if initial_alpha else 255 for end in ends]
    if any(not 0<=v<=255 for v in start_alpha):raise ValueError(f'particle start color alpha {start_alpha}')
    if any(not 0<=v<=255 for color in colors for v in color):raise ValueError('particle color range')
    collision=ps['CollisionModule']
    if collision['enabled']and(collision['type']!=1 or collision['collisionMode']!=1 or collision['collidesWith']['m_Bits']!=256 or collision['colliderForce']):raise ValueError('particle terrain collision')
    force=ps['ForceModule'];vel=ps['VelocityModule'];limit=ps['ClampVelocityModule']
    if force['enabled']and(not force['inWorldSpace']or force['randomizePerFrame']):raise ValueError('particle force space')
    if vel['enabled']and not vel['inWorldSpace']:raise ValueError('particle velocity space')
    if vel['enabled']and(any(values(vel[k])!=[0,0]for k in ('orbitalX','orbitalY','orbitalZ','orbitalOffsetX','orbitalOffsetY','orbitalOffsetZ','radial'))or values(vel['speedModifier'])!=[1,1]):raise ValueError('particle orbital/speed modifier')
    if limit['enabled']and(limit['separateAxis']or values(limit['drag'])!=[0,0]):raise ValueError('particle velocity limit')
    samples=[]
    for i in range(33):
        t=i/32
        size=values(ps['SizeModule']['curve'],t)if ps['SizeModule']['enabled']else[1,1]
        cm=ps['ColorModule'];alphas=[1,1]
        if cm['enabled']:
            g=cm['gradient'];mode=g['minMaxState']
            if mode not in(1,3):raise ValueError('particle color curve mode')
            gs=[g['maxGradient']]*2 if mode==1 else[g['minGradient'],g['maxGradient']]
            for gradient in gs:
                if any(abs(gradient[f'key{k}'][c]-1)>1e-6 for k in range(gradient['m_NumColorKeys'])for c in 'rgb'):raise ValueError('particle nonwhite color curve')
            alphas=[gradient_alpha(g,t)for g in gs]
        rotation=values(ps['RotationModule']['curve'],t)if ps['RotationModule']['enabled']else[0,0]
        samples.append({'size':list(map(q,size)),'alpha':[max(0,min(255,round(a*initial_alpha*255)))for a in alphas],'spin':[q(math.degrees(v))for v in rotation]})
    initial=lambda key:sorted(values(im[key]))
    rate=values(em['rateOverTime'])
    if rate[0]!=rate[1] or rate[0]<=0:raise ValueError('particle emission rate')
    count=min(im['maxNumParticles'],math.ceil(rate[0]*ps['lengthInSec']-1e-4))
    speedspin=ps['RotationBySpeedModule'];omega=values(speedspin['curve'])if speedspin['enabled']else[0,0]
    forcev=[values(force[k])if force['enabled']else[0,0]for k in 'xyz']
    gravityv=values(im['gravityModifier'])
    if gravityv[0]!=gravityv[1]:
        # A random gravity multiplier is a per-particle force: the guest draws
        # each particle's force from the style's range with one seed.
        if not random_gravity:raise ValueError('particle random gravity')
        low,high=sorted(gravity*g for g in gravityv)
        forcev[1]=[forcev[1][0]+low,forcev[1][1]+high]
    else:forcev[1]=[v+gravity*gravityv[0]for v in forcev[1]]
    fields={'life':[max(1,round(v*60))for v in initial('startLifetime')],'speed':[q(v*scale)for v in initial('startSpeed')],'size':[q(v*scale)for v in initial('startSize')],
        'rotation':[q(math.degrees(v))for v in initial('startRotation')],'colors':colors,'start_alpha':start_alpha,'count':count,'rate':q(rate[0]),'shape':shape['type'],
        'radius':q(shape['radius']['value']),'arc':q(shape['arc']['value']),'shape_scale':[q(shape['m_Scale'][k])for k in 'xyz'],
        'force':[[q(v*scale)for v in pair]for pair in forcev],'velocity':[[q(v*scale)for v in values(vel[k])]if vel['enabled']else[0,0]for k in 'xyz'],
        'limit':q(values(limit['magnitude'])[0])if limit['enabled']else-1,'dampen':q(limit['dampen'])if limit['enabled']else 0,
        'spin_speed':list(map(lambda v:q(math.degrees(v)),omega)),'spin_range':[q(speedspin['range'][k])for k in 'xy'],
        'collision':bool(collision['enabled']),'bounce':q(values(collision['m_Bounce'])[0]),'collision_dampen':q(values(collision['m_Dampen'])[0]),
        'life_loss':q(values(collision['m_EnergyLossOnCollision'])[0]),'kill_speed':q(collision['minKillSpeed']),'radius_scale':q(collision['radiusScale']),'samples':samples,'cells':uv['tilesY']if uv['enabled']else 1}
    if not 0<count<=POOL_CAPACITY:raise ValueError(f'particle source capacity: {count} particles against a {POOL_CAPACITY}-slot pool')
    if max(fields['life'])>65535:raise ValueError(f"particle source capacity: lifetime {max(fields['life'])} ticks")
    return fields

def scene_gravity(source):
    physics=next(o for o in source.file('globalgamemanagers').objects.values()if o.type.name=='PhysicsManager')
    return source.read(physics)['m_Gravity']['y']

def part_emitter(source,sc,part_gid,gravity,played=False,relaxed=False,emit=None):
    """One debrisPart's emitter, or None when the part is not a particle system.

    The single place a Breakable debris emitter is validated, so the destruction
    contract in breakables.py and the art cook here refuse exactly the same set.
    Raises ValueError with the source reason when the part is an emitter the
    bounded particle model cannot reproduce.

    `played` is the caller stating that it resolved the action that starts this
    emitter, which a Breakable's debrisPart does not need (it plays on awake when
    the break activates it) and an FSM-driven one does. It admits `playOnAwake`
    off, and it lets a provably silent system answer SILENT rather than raise:
    only a caller that knows the emitter is reached can say that drawing nothing
    reproduces it. Left off, the answer is exactly what it was before.
    """
    cs={}
    for ref in sc.gos[part_gid]['m_Component']:
        o=source.ref(sc.file,ref['component']);cs[source.typename(o)]=(o,source.read(o))
    if 'ParticleSystem' not in cs:return None
    po,ps=cs['ParticleSystem'];ro,renderer=cs['ParticleSystemRenderer']
    if played and emits_nothing(ps) and not emit:return SILENT
    mo=source.ref(sc.file,renderer['m_Materials'][0]);mat=source.read(mo)
    so=source.ref(mo.assets_file,mat['m_Shader']);shader=source.read(so)['m_ParsedForm']['m_Name']
    if shader not in('Sprites/Lit','Sprites/Default') or renderer['m_RenderMode']or renderer['m_RenderAlignment']:
        raise ValueError(f"particle renderer material {shader} mode {renderer['m_RenderMode']}/{renderer['m_RenderAlignment']}")
    tint=dict(mat['m_SavedProperties']['m_Colors'])['_Color']
    if any(not 0<=tint[c]<=1 for c in 'rgba'):raise ValueError('particle material tint')
    texture=source.ref(mo.assets_file,dict(mat['m_SavedProperties']['m_TexEnvs'])['_MainTex']['m_Texture']);sid=source.sid(texture)
    matrix=sc.world(sc.go_transform[part_gid])
    if relaxed:ps,matrix=secret_relax(ps,matrix,emit)
    scale=particle_scale(ps,matrix)
    st=style(ps,gravity,scale,played,random_gravity=relaxed);st['texture']=sid
    st['colors']=[[round(v*tint[c])for v,c in zip(color,'rgb')]for color in st['colors']]
    for sample in st['samples']:sample['alpha']=[round(v*tint['a'])for v in sample['alpha']]
    return {'style':st,'system':po,'texture':texture,'texture_id':sid,'matrix':matrix,
            'shader':shader,'scaling_mode':ps['scalingMode'],'world_parameter_scale':scale,
            'ps_sha256':hashlib.sha256(json.dumps(ps,sort_keys=True).encode()).hexdigest()}

def secret_relax(ps,matrix,emit=None):
    """A hidden wall's or cracked floor's emitter in the guest's two shapes.

    Their dust, rock and bit emitters use four things the bounded model does
    not run, and a secret carries them into it as documented approximations
    rather than refusing its whole break:

      Local scaling (1): the emitter scales by its own transform only. The
      parents here are within 11% of unit scale, so the whole world matrix
      scales the emission shape (as Shape, 2) and speeds stay unscaled.
      Cone (4): a circle sector of twice the cone angle centred on the cone's
      world axis, from the cone's base radius.
      SingleSidedEdge (12): a box of the edge's length, turned so the box's
      +Z launch direction is the edge's +Y normal.
      Local-space force and velocity: taken in world axes.
    A random gravity multiplier becomes a per-particle force range (`style`).
    """
    import copy
    ps=copy.deepcopy(ps);shape=ps['ShapeModule']
    # A burst the pool cannot hold is cut before the secret budget cuts it
    # again (collect_secret); the emission rate, and so its time, stays.
    ps['InitialModule']['maxNumParticles']=min(ps['InitialModule']['maxNumParticles'],POOL_CAPACITY)
    if emit:
        # `ParticleSystem.Emit(n)` releases n now, whatever the emission
        # module says (the wall's per-hit rocks author a zero rate).
        em=ps['EmissionModule'];em['enabled']=1;em['m_BurstCount']=0
        em['rateOverTime']={'minMaxState':0,'scalar':emit/ps['lengthInSec']}
        em['rateOverDistance']={'minMaxState':0,'scalar':0}
        ps['InitialModule']['maxNumParticles']=emit
    if ps['scalingMode']==1:ps['scalingMode']=2
    for module in ('ForceModule','VelocityModule'):
        if ps[module]['enabled']:ps[module]['inWorldSpace']=1
    m=[row[:] for row in matrix]
    if shape['type']==4:
        axis=[matrix[0][2],matrix[1][2]]
        if not any(abs(v)>1e-9 for v in axis):axis=[0.0,1.0]
        angle=shape['angle']['value'] if isinstance(shape['angle'],dict) else shape['angle']
        start=math.degrees(math.atan2(axis[1],axis[0]))-angle
        c,s_=math.cos(math.radians(start)),math.sin(math.radians(start))
        sx=math.hypot(matrix[0][0],matrix[1][0]) or 1.0
        m=[[c*sx,-s_*sx,0,matrix[0][3]],[s_*sx,c*sx,0,matrix[1][3]],[0,0,1,matrix[2][3]],[0,0,0,1]]
        shape['type']=10;shape['arc']['value']=2*angle;shape['arc']['mode']=0
        if isinstance(shape['angle'],dict):shape['angle']['value']=0
    elif shape['type']==12:
        # (x, y, z) -> (x, z, -y): the box's local +Z is the edge's +Y.
        m=[[matrix[i][0],-matrix[i][2],matrix[i][1],matrix[i][3]] for i in range(3)]+[[0,0,0,1]]
        radius=shape['radius']['value']
        shape['type']=5;shape['m_Scale']={'x':2*radius,'y':0.0,'z':0.0}
        shape['radius']['mode']=0
    return ps,m

def euler_basis(degrees):
    """Unity's Quaternion.Euler as a 3x3 basis: intrinsic Z, then X, then Y."""
    x,y,z=(math.radians(v) for v in degrees)
    def rot(axis,a):
        c,s=math.cos(a),math.sin(a)
        if axis=='x':return [[1,0,0],[0,c,-s],[0,s,c]]
        if axis=='y':return [[c,0,s],[0,1,0],[-s,0,c]]
        return [[c,-s,0],[s,c,0],[0,0,1]]
    def mul(a,b):return [[sum(a[i][k]*b[k][j]for k in range(3))for j in range(3)]for i in range(3)]
    return mul(mul(rot('y',y),rot('x',x)),rot('z',z))


class PrefabView:
    """A CreateObject prefab presented the way `part_emitter` reads a scene.

    Only the four members `part_emitter` touches are provided: `gos`, `file`,
    `go_transform` and `world`. The point is that a prefab emitter then goes
    through the identical validator and cook as an authored one rather than a
    parallel path that could drift from it.

    `Instantiate(prefab, position, rotation)` replaces the prefab root's
    serialized position and rotation and keeps its scale, so `world` composes the
    spawn transform with each object's own local chain *below* the root. The
    root's stale authoring position, which is a leftover of wherever the prefab
    was built, is never used.
    """
    def __init__(self,source,file,root_gid,origin,rotation=(0,0,0)):
        # Walk the prefab's own subtree rather than the assets file, which for a
        # pooled prefab is resources.assets and thousands of unrelated objects.
        self.source=source;self.file=file;self.gos={};self.transforms={};self.go_transform={}
        pending=[root_gid]
        while pending:
            gid=pending.pop()
            if gid in self.gos:continue
            self.gos[gid]=go=source.read(file.objects[gid])
            tid=next((o.path_id for o in (source.ref(file,c['component']) for c in go['m_Component'])
                      if o.type.name=='Transform'),None)
            if tid is None:raise ValueError('prefab object carries no Transform')
            self.go_transform[gid]=tid
            self.transforms[tid]=transform=source.read(file.objects[tid])
            for child in transform['m_Children']:
                self.transforms[child['m_PathID']]=kid=source.read(file.objects[child['m_PathID']])
                pending.append(kid['m_GameObject']['m_PathID'])
        self.root=root_gid;self.root_tid=self.go_transform[root_gid]
        scale=self.transforms[self.root_tid]['m_LocalScale']
        basis=euler_basis(rotation)
        self.spawn=[[basis[i][j]*scale['xyz'[j]] for j in range(3)]+[origin[i]] for i in range(3)]+[[0,0,0,1]]
        self._cache={}
    def _local(self,tid):
        """Local transform of one object relative to the prefab root."""
        if tid==self.root_tid:return [[float(i==j) for j in range(4)] for i in range(4)]
        t=self.transforms[tid];q=t['m_LocalRotation'];x,y,z,w=[q[k] for k in 'xyzw']
        s=t['m_LocalScale'];p=t['m_LocalPosition']
        r=[[1-2*(y*y+z*z),2*(x*y-z*w),2*(x*z+y*w),p['x']],[2*(x*y+z*w),1-2*(x*x+z*z),2*(y*z-x*w),p['y']],
           [2*(x*z-y*w),2*(y*z+x*w),1-2*(x*x+y*y),p['z']],[0,0,0,1]]
        for row in range(3):
            for col,k in enumerate('xyz'):r[row][col]*=s[k]
        father=t['m_Father']['m_PathID']
        if not father or father not in self.transforms:
            raise ValueError('prefab object is not under the instantiated root')
        a=self._local(father)
        return [[sum(a[i][k]*r[k][j] for k in range(4)) for j in range(4)] for i in range(4)]
    def world(self,tid):
        if tid not in self._cache:
            a,r=self.spawn,self._local(tid)
            self._cache[tid]=[[sum(a[i][k]*r[k][j] for k in range(4)) for j in range(4)] for i in range(4)]
        return self._cache[tid]
    def subtree(self,gid=None):
        out=[gid if gid is not None else self.root];i=0
        while i<len(out):
            tid=self.go_transform.get(out[i]);i+=1
            for child in (self.transforms[tid]['m_Children'] if tid is not None else []):
                if child['m_PathID'] in self.transforms:
                    out.append(self.transforms[child['m_PathID']]['m_GameObject']['m_PathID'])
        return out


def prefab_emitters(source,file,ref,gravity,origin,rotation=(0,0,0),relaxed=False):
    """Every emitter a fixed CreateObject prefab reference instantiates.

    The prefab is resolved at cook time, so what it spawns is as fixed as an
    authored child: nothing about it is decided at runtime. Each ParticleSystem
    in it is validated and cooked by `part_emitter` unchanged, with the spawn
    transform the action supplies standing in for the scene transform an authored
    emitter would have. `played=True` because Instantiate is what starts it.
    """
    prefab=source.ref(file,ref);tree=source.read(prefab)
    if 'm_Component' not in tree:raise ValueError('CreateObject target is not a GameObject')
    view=PrefabView(source,prefab.assets_file,prefab.path_id,origin,rotation)
    found=[]
    for gid in view.subtree():
        emitter=part_emitter(source,view,gid,gravity,played=True,relaxed=relaxed)
        if emitter is None or emitter is SILENT:continue
        found.append(dict(emitter,prefab=source.sid(prefab),prefab_name=tree['m_Name'],
                          part=f'{Path(view.file.name).name}:{gid}'))
    return found
