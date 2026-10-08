"""Source-derived Tutorial grass/death particle subset, no generic replacement."""
import math
from pathlib import Path
from breakables import _components

def curve_value(curve,t):
    points=curve['m_Curve']
    if not points:raise ValueError('empty particle curve')
    if any(p['weightedMode'] for p in points):raise ValueError('weighted particle curve unsupported')
    if t<=points[0]['time']:return points[0]['value']
    if t>=points[-1]['time']:return points[-1]['value']
    for a,b in zip(points,points[1:]):
        if a['time']<=t<=b['time']:
            dt=b['time']-a['time'];u=(t-a['time'])/dt
            return (2*u**3-3*u*u+1)*a['value']+(u**3-2*u*u+u)*dt*a['outSlope']+(-2*u**3+3*u*u)*b['value']+(u**3-u*u)*dt*b['inSlope']
    raise ValueError('particle curve ordering')

def gradient_alpha(gradient,t):
    keys=[(gradient[f'atime{i}']/65535,gradient[f'key{i}']['a'])for i in range(gradient['m_NumAlphaKeys'])]
    if gradient['m_Mode']!=0 or not keys:raise ValueError('unsupported gradient interpolation')
    if t<=keys[0][0]:return keys[0][1]
    if t>=keys[-1][0]:return keys[-1][1]
    for (a,x),(b,y)in zip(keys,keys[1:]):
        if a<=t<=b:return x+(y-x)*(t-a)/(b-a)
    raise ValueError('gradient ordering')

def _read(source,obj):
    go=source.read(obj);components={}
    for ref in go['m_Component']:
        co=source.ref(obj.assets_file,ref['component']);components[source.typename(co)]=(co,source.read(co))
    ps=components['ParticleSystem'][1];renderer=components['ParticleSystemRenderer'][1]
    mo=source.ref(obj.assets_file,renderer['m_Materials'][0]);material=source.read(mo)
    so=source.ref(mo.assets_file,material['m_Shader']);shader=source.read(so)
    if shader['m_ParsedForm']['m_Name']!='Sprites/Default':raise ValueError('particle material shader')
    for sub in shader['m_ParsedForm']['m_SubShaders']:
        for p in sub['m_Passes']:
            blend=p['m_State']['rtBlend0']
            if [blend[k]['val']for k in ('srcBlend','destBlend','blendOp')]!=[1,10,0]:raise ValueError('particle source alpha blend')
    colors=dict(material['m_SavedProperties']['m_Colors'])
    if colors['_Color']!={'r':1.,'g':1.,'b':1.,'a':1.}:raise ValueError('particle material color')
    texture=source.ref(mo.assets_file,dict(material['m_SavedProperties']['m_TexEnvs'])['_MainTex']['m_Texture'])
    if ps['looping'] or ps['scalingMode']!=2 or ps['moveWithTransform']!=1 or abs(ps['lengthInSec']-.1)>1e-6:
        raise ValueError('particle simulation space/duration')
    supported={'InitialModule','ShapeModule','EmissionModule','SizeModule','ColorModule','UVModule','ForceModule','ClampVelocityModule','RotationBySpeedModule'}
    if any(k.endswith('Module')and isinstance(v,dict)and v.get('enabled')and k not in supported for k,v in ps.items()):raise ValueError('unimplemented enabled particle module')
    if renderer['m_RenderMode']!=0 or renderer['m_RenderAlignment']!=0 or renderer['m_Pivot']!={'x':0.,'y':0.,'z':0.}:raise ValueError('unsupported particle billboard')
    return ps,texture,{'system':source.sid(components['ParticleSystem'][0]),'renderer':source.sid(components['ParticleSystemRenderer'][0]),'material':source.sid(mo),'shader':source.sid(so),'texture':source.sid(texture)}

def _range(curve):
    if curve['minMaxState']==0:return [curve['scalar']]*2
    if curve['minMaxState']==3:return [curve['minScalar'],curve['scalar']]
    raise ValueError('particle initial range requires constants')

def _style(ps,kind,scene_gravity=-9.8100004196167):
    initial=ps['InitialModule'];em=ps['EmissionModule'];shape=ps['ShapeModule'];uv=ps['UVModule'];force=ps['ForceModule'];limit=ps['ClampVelocityModule']
    lifetime=_range(initial['startLifetime']);speed=_range(initial['startSpeed']);size=_range(initial['startSize'])
    expected=([.7,1.3],[3,30],[.7,.9],250,4)if kind==0 else([.6,.6],[8,8],[.5,1.3],500,0)
    values=(lifetime,speed,size)
    if any(abs(x-y)>1e-5 for actual,want in zip(values,expected[:3])for x,y in zip(actual,want)) or em['rateOverTime']['scalar']!=expected[3] or shape['type']!=expected[4]:raise ValueError('particle source variant')
    if em['m_BurstCount']or em['rateOverDistance']['scalar']or initial['size3D']or initial['rotation3D']or initial['gravitySource']!=0:raise ValueError('particle emission/size variant')
    if uv['tilesX']!=(1 if kind==0 else 9)or uv['tilesY']!=(3 if kind==0 else 1)or uv['mode']!=0 or uv['timeMode']!=0:raise ValueError('particle texture sheet')
    if uv['frameOverTime']['minMaxState']!=1 or any(abs(curve_value(uv['frameOverTime']['maxCurve'],t)-t)>1e-6 for t in (0,.25,.5,.75,1)):raise ValueError('particle UV lifetime curve')
    if limit['separateAxis']or limit['drag']['scalar']or _range(limit['magnitude'])!=[1,1]:raise ValueError('particle limit velocity variant')
    start=initial['startColor'];gradient=ps['ColorModule']['gradient'];sizecurve=ps['SizeModule']['curve']
    if start['minMaxState']!=(0 if kind==0 else 2)or gradient['minMaxState']!=(1 if kind==0 else 3)or sizecurve['minMaxState']!=1:raise ValueError('particle color/size modes')
    for g in [gradient['minGradient'],gradient['maxGradient']]:
        if any(any(g[f'key{i}'][k]!=1 for k in 'rgb')for i in range(g['m_NumColorKeys'])):raise ValueError('particle color-over-life RGB variant')
    lut=[]
    for i in range(65):
        t=i/64
        alphas=[gradient_alpha(gradient['maxGradient'],t)]*2 if kind==0 else [gradient_alpha(gradient[k],t)for k in ('minGradient','maxGradient')]
        lut.append({'size':round(curve_value(sizecurve['maxCurve'],t)*sizecurve['scalar']*65536),'alpha':[max(0,min(255,round(v*255)))for v in alphas]})
    if kind==0:
        if not force['enabled']or not force['inWorldSpace']or force['randomizePerFrame'] or _range(force['y'])!=[-8,-4]:raise ValueError('grass force variant')
        rotation=ps['RotationBySpeedModule']
        if not rotation['enabled']or rotation['curve']['minMaxState']!=0:raise ValueError('grass rotation mode')
        gravity=[-8,-4];omega=math.degrees(rotation['curve']['scalar'])
    else:
        if force['enabled']or initial['gravityModifier']['scalar']!=-2:raise ValueError('death gravity variant')
        gravity=[scene_gravity*initial['gravityModifier']['scalar']]*2;omega=0
    return {'life':[round(v*60)for v in lifetime],'speed':[round(v*65536)for v in speed],'size':[round(v*65536)for v in size],
            'force':[round(v*65536)for v in gravity],'dampen':round(limit['dampen']*65536),'rotation':round(omega*65536),
            'colors':[[round(start[k][c]*128)for c in 'rgb']for k in ('minColor','maxColor')],
            'count':25 if kind==0 else 50,'duration':6,'uv_scale':round(uv['frameOverTime']['scalar']*65536),'curves':lut,'shape':shape['type']}

def particle_sources(source,scene):
    if hasattr(scene,'_particle_sources'):return scene._particle_sources
    if Path(scene.file.name).name!='level6':return None
    emitters=[];representative=None;style=None
    grasses=[(i,d)for i,(typ,d)in scene.objects.items()if typ=='GrassCut']
    for state,(sid,d)in enumerate(grasses):
        if not d['m_Enabled']or not d['particles']['m_PathID']:continue
        obj=source.ref(scene.file,d['particles']);ps,texture,provenance=_read(source,obj)
        candidate=_style(ps,0)
        if provenance['texture']!='resources.assets:290' or style is not None and candidate!=style:raise ValueError('mixed grass particle family')
        style=candidate;representative=texture
        gid=obj.path_id;matrix=scene.world(scene.go_transform[gid]);direction=[matrix[i][2]for i in range(3)];norm=math.sqrt(sum(v*v for v in direction))
        emitters.append({'state':state,'source':sid,'origin':[round(matrix[i][3]*65536)for i in range(3)],
                         'basis':[[round(matrix[i][j]*65536)for j in range(3)]for i in range(3)],'direction':[round(v/norm*65536)for v in direction],
                         'provenance':provenance})
    actor=next(d for typ,d in scene.objects.values()if typ=='EnemyDeathEffects'and d.get('deathPuffMedPrefab',{}).get('m_PathID'))
    obj=source.ref(scene.file,actor['deathPuffMedPrefab']);ps,death_texture,provenance=_read(source,obj)
    if source.sid(obj)!='resources.assets:6520'or provenance['texture']!='resources.assets:292':raise ValueError('unverified death puff')
    # The supported Crawler family has identical effect origin/type/ref.
    for sid in (12546,12547,12548):
        gid=scene.objects[sid][1]['m_GameObject']['m_PathID']
        death=next(d for _,typ,d in _components(scene,gid)if typ=='EnemyDeathEffects')
        if death['enemyDeathType']!=0 or death['effectOrigin']!={'x':0.,'y':-0.20000000298023224,'z':0.}or source.sid(source.ref(scene.file,death['deathPuffMedPrefab']))!='resources.assets:6520':raise ValueError('Crawler death effect variant')
    physics_o=next(o for o in source.file('globalgamemanagers').objects.values()if o.type.name=='PhysicsManager')
    physics=source.read(physics_o)
    if physics['m_Gravity']['x']!=0 or physics['m_Gravity']['z']!=0:raise ValueError('unsupported particle gravity direction')
    result={'styles':[style,_style(ps,1,physics['m_Gravity']['y'])],'physics_source':source.sid(physics_o),'physics_gravity':physics['m_Gravity'],'emitters':emitters,'texture_objects':[representative,death_texture],'death_source':provenance,
            'death_offset':[0,round(-.20000000298023224*65536),0],
            'limitations':['Fixed60Hz particle integration/damping and deterministic RNG approximate nativeUnity update scheduling',
                            'Source colored alpha uses4baked opacity levels and ordered0/.5/1 coverage onPS1',
                            'Finite128particle pool counts rejected spawns explicitly; no generic substitute art']}
    scene._particle_sources=result;return result

def append_particle_art(source,scene,atlas,frames):
    from cook import FOCAL,CAM_Z
    from materials import quantize_alpha_coverage
    from PIL import Image
    data=particle_sources(source,scene)
    if data is None:return None
    banks=[]
    for kind,texture in enumerate(data['texture_objects']):
        image=texture.read().image.convert('RGBA');cols,rows=(1,3)if kind==0 else(9,1);w,h=image.width//cols,image.height//rows
        target=math.ceil(max(data['styles'][kind]['size'])/65536*FOCAL/-CAM_Z)
        cells=[]
        for row in range(rows):
            for col in range(cols):
                # Unity texture-sheet row0 is at bottom (sourceUV origin).
                cell=image.crop((col*w,(rows-1-row)*h,(col+1)*w,(rows-row)*h)).resize((target,target),Image.Resampling.LANCZOS)
                variants=[]
                for alpha in (32,64,96,128):
                    tex=atlas.add_quantized(*quantize_alpha_coverage(cell,alpha),streamed=False)
                    variants.append(len(frames));frames.append({'texture':tex,'box':[-.5,-.5,.5,.5],'sprite':f'{source.sid(texture)}:cell:{col}:{row}:alpha:{alpha}','event':{}})
                cells.append(variants)
        banks.append(cells)
    return {k:v for k,v in data.items()if k!='texture_objects'}|{'frames':banks}

def generated_style(style):
    def ar(v):return '['+','.join(map(str,v))+']'
    curves='&['+','.join('particles::Sample{size:'+str(s['size'])+',alpha:'+ar(s['alpha'])+'}'for s in style['curves'])+']'
    fields={k:ar(style[k])for k in ('life','speed','size','force')}
    fields.update({k:style[k]for k in ('dampen','rotation','count','duration','uv_scale')})
    fields['colors']='['+','.join(ar(c)for c in style['colors'])+']';fields['curves']=curves
    return 'particles::Style{'+','.join(f'{k}:{v}'for k,v in fields.items())+'}'

def generated_emitter(p):
    def ar(v):return '['+','.join(map(str,v))+']'
    return 'particles::EmitterSpec{state:'+str(p['state'])+',source:'+str(p['source'])+',origin:'+ar(p['origin'])+',basis:['+','.join(ar(v)for v in p['basis'])+'],direction:'+ar(p['direction'])+'}'
