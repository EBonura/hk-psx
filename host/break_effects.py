"""Original Breakable particle families, emitted into the existing bounded pool.

Fresh Windows source only. No room mutation and no replacement generic particles.
Source curves are sampled to a shared 33-entry fixed-point table; PS1 alpha uses
three prequantized levels. Unity collision/damping/RNG remain approximations.
"""
import hashlib,json,math,struct
from pathlib import Path
from PIL import Image
from source import Source,ROOT
from scene import Scene
from breakables import breakable_sources
from particles import curve_value,gradient_alpha
from materials import quantize_alpha_coverage
from geo import place_rectangles
VRAM_RECTS=((352,176,32,64),(360,32,24,32),(352,64,32,32))
ALPHAS=(43,85,128)

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
    if mode!=0:raise ValueError('unsupported particle scaling mode')
    if ps['moveWithTransform']!=1:raise ValueError('Hierarchy particles require world simulation')
    columns=[[matrix[i][j]for i in range(3)]for j in range(3)]
    lengths=[math.sqrt(sum(v*v for v in col))for col in columns]
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


def style(ps,gravity,scale=1.0):
    im=ps['InitialModule'];shape=ps['ShapeModule'];uv=ps['UVModule'];em=ps['EmissionModule']
    enabled={k for k,v in ps.items()if k.endswith('Module')and isinstance(v,dict)and v.get('enabled')}
    supported={'InitialModule','ShapeModule','EmissionModule','SizeModule','RotationModule','ColorModule','UVModule','VelocityModule','ForceModule','ClampVelocityModule','RotationBySpeedModule','CollisionModule'}
    if enabled-supported:raise ValueError('unsupported particle modules: '+str(enabled-supported))
    if not ps['playOnAwake'] or ps['looping']or ps['scalingMode']not in(0,2) or ps['moveWithTransform']not in(0,1):raise ValueError('particle simulation space')
    if im['size3D']or im['rotation3D']or im['gravitySource']!=0 or em['m_BurstCount']or values(em['rateOverDistance'])!=[0,0]:raise ValueError('particle emission mode')
    if shape['type']not in(5,10)or not shape['enabled']:raise ValueError('particle shape')
    if any(shape['m_Position'][k]or shape['m_Rotation'][k]for k in 'xyz'):raise ValueError('particle shape transform')
    if shape['randomDirectionAmount']or shape['sphericalDirectionAmount']or shape['randomPositionAmount']or shape['radius']['mode']or shape['arc']['mode']:raise ValueError('particle shape random mode')
    if uv['enabled']and(uv['tilesX']!=1 or uv['animationType']!=1 or uv['rowMode']!=1 or uv['tilesY']not in(3,4)):raise ValueError('particle random row')
    start=im['startColor']
    if start['minMaxState']!=0:raise ValueError('particle start color mode')
    color=[round(start['maxColor'][k]*128)for k in 'rgb'];initial_alpha=start['maxColor']['a']
    if any(not 0<=v<=255 for v in color):raise ValueError('particle color range')
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
    if gravityv[0]!=gravityv[1]:raise ValueError('particle random gravity')
    forcev[1]=[v+gravity*gravityv[0]for v in forcev[1]]
    fields={'life':[max(1,round(v*60))for v in initial('startLifetime')],'speed':[q(v*scale)for v in initial('startSpeed')],'size':[q(v*scale)for v in initial('startSize')],
        'rotation':[q(math.degrees(v))for v in initial('startRotation')],'color':color,'count':count,'rate':q(rate[0]),'shape':shape['type'],
        'radius':q(shape['radius']['value']),'arc':q(shape['arc']['value']),'shape_scale':[q(shape['m_Scale'][k])for k in 'xyz'],
        'force':[[q(v*scale)for v in pair]for pair in forcev],'velocity':[[q(v*scale)for v in values(vel[k])]if vel['enabled']else[0,0]for k in 'xyz'],
        'limit':q(values(limit['magnitude'])[0])if limit['enabled']else-1,'dampen':q(limit['dampen'])if limit['enabled']else 0,
        'spin_speed':list(map(lambda v:q(math.degrees(v)),omega)),'spin_range':[q(speedspin['range'][k])for k in 'xy'],
        'collision':bool(collision['enabled']),'bounce':q(values(collision['m_Bounce'])[0]),'collision_dampen':q(values(collision['m_Dampen'])[0]),
        'life_loss':q(values(collision['m_EnergyLossOnCollision'])[0]),'kill_speed':q(collision['minKillSpeed']),'radius_scale':q(collision['radiusScale']),'samples':samples,'cells':uv['tilesY']if uv['enabled']else 1}
    if not 0<count<=128 or max(fields['life'])>65535:raise ValueError('particle source capacity')
    return fields

def collect(source=None,metadata=None):
    source=source or Source();metadata=metadata or json.loads((ROOT/'data/regions.json').read_text())
    physics=next(o for o in source.file('globalgamemanagers').objects.values()if o.type.name=='PhysicsManager');gravity=source.read(physics)['m_Gravity']['y']
    wanted={r['source'] for region in metadata['regions']for r in region['breakables']};styles=[];emitters=[];textures={};ignored=[];source_records=[]
    for desc in metadata['scenes']:
        sc=Scene(source,desc['file'])
        for b in breakable_sources(sc):
            if b['source']not in wanted:continue
            for part in b['debris']:
                gid=int(part['game_object'].split(':')[1]);cs={}
                for ref in sc.gos[gid]['m_Component']:
                    o=source.ref(sc.file,ref['component']);cs[source.typename(o)]=(o,source.read(o))
                if 'ParticleSystem'not in cs:
                    ignored.append({'owner':b['source'],'part':part['game_object'],'reason':'existing rigid fragment'if 'SpinSelf'in cs else'unhandled nonparticle part'});continue
                po,ps=cs['ParticleSystem'];ro,renderer=cs['ParticleSystemRenderer'];mo=source.ref(sc.file,renderer['m_Materials'][0]);mat=source.read(mo)
                so=source.ref(mo.assets_file,mat['m_Shader']);shader=source.read(so)['m_ParsedForm']['m_Name']
                try:
                    if shader not in('Sprites/Lit','Sprites/Default') or renderer['m_RenderMode']or renderer['m_RenderAlignment']:raise ValueError('particle renderer material')
                    tint=dict(mat['m_SavedProperties']['m_Colors'])['_Color']
                    if any(not 0<=tint[c]<=1 for c in 'rgba'):raise ValueError('particle material tint')
                    texture=source.ref(mo.assets_file,dict(mat['m_SavedProperties']['m_TexEnvs'])['_MainTex']['m_Texture']);sid=source.sid(texture)
                    matrix=sc.world(sc.go_transform[gid]);scale=particle_scale(ps,matrix)
                    st=style(ps,gravity,scale);st['texture']=sid
                except ValueError as error:
                    # An emitter the bounded particle model cannot reproduce is an
                    # explicit omission for its breakable, not a whole-world failure.
                    ignored.append({'owner':b['source'],'part':part['game_object'],'reason':'unsupported particle style: '+str(error)});continue
                st['color']=[round(v*tint[c])for v,c in zip(st['color'],'rgb')]
                for sample in st['samples']:sample['alpha']=[round(v*tint['a'])for v in sample['alpha']]
                if st not in styles:styles.append(st)
                emitters.append({'scene':desc['scene_id'],'owner':desc['scene_id']*128+b['state_index'],'source':po.path_id,'style':styles.index(st),
                    'angle_offset':q(b['angle_offset']),'origin':[q(matrix[i][3])for i in range(3)],'basis':[[q(matrix[i][j])for j in range(3)]for i in range(3)]})
                textures[sid]=texture
                source_records.append({'owner':b['source'],'name':b['name'],'part':part['game_object'],'system':source.sid(po),'texture':sid,'shader':shader,'scaling_mode':ps['scalingMode'],'world_parameter_scale':scale,'ps_sha256':hashlib.sha256(json.dumps(ps,sort_keys=True).encode()).hexdigest()})
    if len(styles)>253:raise ValueError('particle style IDs')
    return source,styles,emitters,textures,source_records,ignored

def art_bank(styles,textures):
    from cook import FOCAL,CAM_Z
    cells=[];families=[]
    for sid,obj in textures.items():
        group=[s for s in styles if s['texture']==sid];rows={s['cells']for s in group}
        if len(rows)!=1:raise ValueError('mixed sheet grid')
        count=rows.pop();target=math.ceil(max(max(s['size'])for s in group)/65536*FOCAL/-CAM_Z)
        im=obj.read().image.convert('RGBA');w,h=im.width,im.height//count
        start=len(cells)
        for i in range(count):cells.append(im.crop((0,(count-1-i)*h,w,(count-i)*h)).resize((target,target),Image.Resampling.LANCZOS))
        families.append((sid,start,count))
    # Joint palette per opacity level, preserving the 4px alpha-coverage phase.
    sheet=Image.new('RGBA',(128,math.ceil(len(cells)/4)*32))
    if any(im.width>32 or im.height>32 for im in cells):raise ValueError('particle art exceeds source-projection bank')
    for i,im in enumerate(cells):sheet.paste(im,((i%4)*32,(i//4)*32))
    palettes=[];planes=[];mapping=[]
    for alpha in ALPHAS:
        sw,sh,palette,pixels=quantize_alpha_coverage(sheet,alpha)
        pal=bytes(palette)
        if pal not in palettes:palettes.append(pal)
        for i,im in enumerate(cells):
            stride=(im.width+3)//4*2;data=bytearray(stride*im.height)
            for y in range(im.height):
                for x in range(im.width):
                    sx=(i%4)*32+x;sy=(i//4)*32+y;index=(pixels[sy*((sw+1)//2)+sx//2]>>(4*(sx&1)))&15
                    data[y*stride+x//2]|=index<<(4*(x&1))
            item=(stride//2,im.height,bytes(data))
            if item not in planes:planes.append(item)
            mapping.append((planes.index(item),palettes.index(pal),im.width,im.height))
    items=[('p'+str(i),16,1,16)for i in range(len(palettes))]+[('t'+str(i),w,h,1)for i,(w,h,_)in enumerate(planes)]
    allocated=place_rectangles(items,VRAM_RECTS);blob=bytearray();uploads=[]
    for key,_,_,_ in items:
        x,y,w,h=allocated[key];payload=palettes[int(key[1:])]if key[0]=='p'else planes[int(key[1:])][2]
        uploads.append({'offset':len(blob),'x':x,'y':y,'w':w,'h':h});blob.extend(payload)
    art=[]
    for t,p,w,h in mapping:
        x,y,_,_=allocated['t'+str(t)];px,py,_,_=allocated['p'+str(p)]
        art.append({'u':(x%64)*4,'v':y%256,'w':w,'h':h,'clut':(py<<6)|(px>>4),'tpage':(x//64)|((y//256)<<4)})
    frames={sid:[[level*len(cells)+start+i for level in range(3)]for i in range(count)]for sid,start,count in families}
    return bytes(blob),uploads,art,frames,sheet

def rust(v):
    if isinstance(v,bool):return str(v).lower()
    if isinstance(v,(list,tuple)):return '['+','.join(rust(x)for x in v)+']'
    return str(v)

TRACK_PHASES=33
NO_TRACK=65535
# Baked per-particle tracks are only a cache of the guest's scalar shape path
# (the retired tools/test_particle_tracks.py proved them equal). They cost about 4.9 KiB of
# permanent RAM per emitter, which blocked the third scene; every emitter now
# uses the scalar path and the tables stay empty.
BAKE_TRACKS=False

def particle_tracks(styles,emitters):
    """Exact source-seeded immutable values; motion/contact remains guest work.

    Each emitter restarts the LCG, so pool truncation only shortens a fixed
    prefix. Facing/strike direction change geometry after size/gradient draws,
    with identical RNG consumption. Unpackable future emitters use scalar code.
    """
    def trunc(n,d):return (-1 if n<0 else 1)*(abs(n)//d)
    def lerp(pair,g):return pair[0]+trunc((pair[1]-pair[0])*g,255)
    def mulq(a,b):return trunc(a*b,65536)
    tracks=[];bases=[];fallback=[]
    for ei,e in enumerate(emitters):
        st=styles[e['style']];rng=e['source']^0xd1b54a35
        def random():
            nonlocal rng
            rng=(rng*1664525+1013904223)&0xffffffff;return rng
        def sample(pair):return pair[0]+((random()*(pair[1]-pair[0]+1))>>32)
        rows=[];supported=(st['shape']in(5,10)and len(st['samples'])==TRACK_PHASES
            and 0<st['count']<=128 and st['cells']>0)
        if supported:
            for i in range(st['count']):
                sample(st['life']);sample(st['speed']);size=sample(st['size']);sample(st['rotation']);gradient=random()>>24
                for _ in range(2 if st['shape']==10 else 3):random()
                for _ in range(3):random()
                random() # texture cell, including a one-cell family
                frames=[]
                for curve in st['samples']:
                    half=trunc(mulq(size,lerp(curve['size'],gradient)),2)
                    radius=mulq(half,st['radius_scale']);spin=lerp(curve['spin'],gradient)
                    level=max(0,min(3,(lerp(curve['alpha'],gradient)*3+127)//255))
                    if not (0<=half<=65535 and 0<=radius<=65535 and -(1<<25)<=spin<(1<<25)):
                        supported=False;break
                    frames.append([half|(radius<<16),(spin&0x3ffffff)|(level<<26)])
                if not supported:break
                rows.append(frames)
        if BAKE_TRACKS and supported and len(tracks)+len(rows)<=NO_TRACK:
            bases.append(len(tracks));tracks.extend(rows)
        else:
            bases.append(NO_TRACK);fallback.append(ei)
    return tracks,bases,fallback

def generate(scenes,manifest):
    """Per-scene style, emitter, art and upload tables plus the effect-art chunk manifest.

    `scenes` maps scene_id -> dict(styles, emitters, uploads, art, frames); a scene
    without emitters has empty tables and a placeholder chunk.
    """
    out=['// Generated from Windows source; source IDs/hashes in ignored break-effects report.']
    tables={}
    count=len(manifest)
    for scene in range(count):
        data=scenes.get(scene,{'styles':[],'emitters':[],'uploads':[],'art':[],'frames':{}})
        for st in data['styles']:
            key=json.dumps(st['samples'],sort_keys=True)
            if key not in tables:
                name='CURVE_'+str(len(tables));tables[key]=name
                out.append('static '+name+':&[Sample]=&['+','.join('Sample{'+','.join(k+':'+rust(s[k])for k in('size','alpha','spin'))+'}'for s in st['samples'])+'];')
        out.append(f'static STYLES_{scene}:&[Style]=&[')
        for st in data['styles']:
            fields={k:rust(v)for k,v in st.items()if k not in('samples','texture','cells')}
            fields['samples']=tables[json.dumps(st['samples'],sort_keys=True)];fields['frames']='&'+rust(data['frames'][st['texture']])
            out.append('Style{'+','.join(k+':'+v for k,v in fields.items())+'},')
        out.append(f'];\nstatic EMITTERS_{scene}:&[Emitter]=&[')
        for e in data['emitters']:out.append('Emitter{'+','.join(k+':'+rust(v)for k,v in e.items())+'},')
        out.append(f'];\nstatic ART_{scene}:&[Art]=&[')
        for a in data['art']:out.append('Art{'+','.join(k+':'+rust(v)for k,v in a.items())+'},')
        out.append(f'];\nstatic UPLOADS_{scene}:&[Upload]=&[')
        for u in data['uploads']:out.append('Upload{'+','.join(k+':'+rust(v)for k,v in u.items())+'},')
        out.append('];')
    for name in ('STYLES','EMITTERS','ART','UPLOADS'):
        kind={'STYLES':'Style','EMITTERS':'Emitter','ART':'Art','UPLOADS':'Upload'}[name]
        out.append(f'pub static SCENE_{name}:&[&[{kind}]]=&['+','.join(f'{name}_{scene}' for scene in range(count))+'];')
    # One HLZC chunk per catalog scene, after the world metadata chunks; the
    # guest derives chunk ids from its manifest order like the scene banks.
    out.append('pub static EFFECT_ART_MANIFEST:&[EffectArtDesc]=&['+','.join(
        'EffectArtDesc{raw_len:%d,raw_fnv:%d,stored_len:%d,stored_fnv:%d}'%(m['raw_len'],m['raw_fnv'],m['stored_len'],m['stored_fnv']) for m in manifest)+'];')
    out.append('pub static EMITTER_TRACK_BASES:&[u16]=&[];')
    out.append('pub static PARTICLE_TRACKS:&[[[u32;2];33]]=&[];')
    return '\n'.join(out)+'\n'

def scene_art(scene,styles,emitters,textures,ignored):
    """One scene's trimmed styles, emitters and art bank inside the fixed VRAM reservation.

    Emitters of a scene whose art cannot fit are recorded as omissions for that
    scene alone; other scenes keep their own sheets.
    """
    emitters=[e for e in emitters if e['scene']==scene]
    used={e['style'] for e in emitters};kept=[i for i in range(len(styles)) if i in used]
    remap={old:new for new,old in enumerate(kept)};trimmed=[styles[i] for i in kept]
    trimmed_emitters=[dict(e,style=remap[e['style']]) for e in emitters]
    trimmed_textures={s['texture']:textures[s['texture']] for s in trimmed}
    if not trimmed_emitters:
        return {'styles':[],'emitters':[],'uploads':[],'art':[],'frames':{}},b''
    try:
        blob,uploads,art,frames,sheet=art_bank(trimmed,trimmed_textures)
    except ValueError as error:
        if 'VRAM' not in str(error):raise
        for e in trimmed_emitters:
            ignored.append({'owner':e['owner'],'part':e['source'],'reason':f'particle art VRAM budget: scene {scene} omitted ({error})'})
        return {'styles':[],'emitters':[],'uploads':[],'art':[],'frames':{}},b''
    return {'styles':trimmed,'emitters':trimmed_emitters,'uploads':uploads,'art':art,'frames':frames,'sheet':sheet},bytes(blob)

def cook():
    from scene_bank import compressed,fnv
    from quality import SCENE_COUNT
    source,styles,emitters,textures,records,ignored=collect()
    dest=ROOT/'.hkpsx/break-effects';dest.mkdir(exist_ok=True)
    scenes={};manifest=[];chunks=[];total_vram=0;kept_emitters=[]
    for scene in range(SCENE_COUNT):
        data,blob=scene_art(scene,styles,emitters,textures,ignored)
        scenes[scene]=data;kept_emitters+=data['emitters'];total_vram=max(total_vram,len(blob))
        # The pack needs a nonempty chunk per scene; a scene without art ships four zero bytes.
        raw=blob or b'\0\0\0\0';raw=raw+b'\0'*(-len(raw)%4)
        stored=compressed(raw);path=dest/f'scene_{scene}.hkfx.z';path.write_bytes(stored)
        manifest.append({'scene_id':scene,'raw_len':len(raw),'raw_fnv':fnv(raw),'stored_len':len(stored),'stored_fnv':fnv(stored),'path':str(path),'raw_sha256':hashlib.sha256(raw).hexdigest()})
        if 'sheet' in data:data.pop('sheet').save(dest/f'art_{scene}.png')
    code=generate(scenes,manifest)
    (ROOT/'data/break_effects.rs').write_text(code)
    report={'format':'HKBREAK02','scenes':{str(k):{n:v for n,v in d.items()} for k,d in scenes.items()},'source_records':records,'other_parts':ignored,
        'largest_scene_vram_bytes':total_vram,'vram_rects':VRAM_RECTS,'chunks':manifest,'rust_sha256':hashlib.sha256(code.encode()).hexdigest(),
        'particle_tracks':{'bytes':0,'fallback_emitters':len(kept_emitters),'format':'scalar path only'},
        'source_files':{name:hashlib.sha256((source.directory/name).read_bytes()).hexdigest()for name in [*__import__('quality').SCENE_FILES,'globalgamemanagers','Managed/Assembly-CSharp.dll']},
        'limitations':['Deterministic RNG,60Hz integration/damping,33-point curves,radius/edge collision and initial-lifetime collision-loss approximation differ from nativeUnity particle physics.',
            'Three baked opacity levels and sourceRGB palette quantization approximate Sprites/Lit lighting/blending.',
            'Existing128-particle pool reports capacity drops; authored maximum-per-emitter counts and lifetimes preserved.',
            'Town graves use source uniform-XY planar Hierarchy scaling; nonuniform-XY/sheared/out-of-plane and local-space Hierarchy emitters fail closed.',
            'Geo-rock hit jitter/gleam/chips,containingParticles probability events and mainLifeblood cocoon are separate systems.']}
    (dest/'report.json').write_text(json.dumps(report,indent=2))
    print(f'Break effects: {len(kept_emitters)} source emitters across {sum(1 for d in scenes.values() if d["emitters"])} scenes, largest sheet {total_vram} VRAM bytes')
    return report
if __name__=='__main__':cook()
