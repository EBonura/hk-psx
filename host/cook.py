"""Cook bounded original scene regions and animation banks from Windows assets."""
import math,struct,hashlib,json,sys
from pathlib import Path
from PIL import Image
from source import Source,ROOT,dump
sys.path.insert(0,str(ROOT/'tools'))
from audit_resident_bank import dense_pack,aligned
from scene import Scene
from combat import grass_sources,nail_sources,transformed_box,generated_params
from scenery_geometry import packet_bound, CHILD_CAPACITY
from breakables import breakable_sources,bind_breakables
from reveal_masks import reveal_mask_sources
from materials import scenery_material
from tilemap_fill import append_tilemap_fills
from quality import SCENERY_MAX_AXIS, STATIC_PAGE_BUDGET, ROOM_BYTE_BUDGET, TEXTURE_BUDGET, SCENERY_TEXEL_CAP, SCENERY_SCENE_CAPS
from focus import FOCUS_CLIPS,source_focus_values,generated_focus_params
from superdash import source_superdash_values
from dream_nail import source_dream_nail_values,generated_dream_nail_params
from spells import source_spell_values,generated_spell_params
from actors import actor_sources,hazard_sources,shroom_sources,source_vital_values,generated_vital_params,generated_nail_response_params
from UnityPy.helpers.MeshHelper import MeshHandler

# Camera limits for this isolated development chamber. Source locks are reported.
CAM_X=(30.0,58.0);CAM_Y=(14.11,20.0)
FOCAL=120/math.tan(math.radians(12));CAM_Z=-38.1
CLIPS=('Idle','Run','Airborne','Fall','Land','Slash','SlashAlt','UpSlash','DownSlash')
# The one-frame clip name the False Knight's barrel rides; the prefab has no
# tk2d library of its own, so the clip is named after the pooled object.
BARREL_CLIP='Falling Barrel'

def scenery_dimensions(width,height,cap):
    """One stable integer sampling size; floating roundoff cannot exceed cap."""
    if not isinstance(cap,int) or not 1<=cap<=252 or not all(math.isfinite(v) and v>0 for v in (width,height)):
        raise ValueError('Invalid scenery dimensions/cap')
    reduction=min(1.0,cap/max(width,height))
    return tuple(max(1,min(cap,math.ceil(v*reduction))) for v in (width,height))

_SCENE_TEXELS={}

def scene_sprite_texels(s,sc,cap,shared_geometry):
    """(sprite id, renderer alpha) -> the one texel size its scenery texture has in this scene.

    The largest size any instance projects to, over every SpriteRenderer of the
    scene rather than one view, so every view cooks the same texels and the scene
    bank stores them once. Never above the sprite's own pixels (upsampling adds
    nothing) and never above `cap` on the long axis. Instances drawn smaller map
    the same texture onto their smaller quad.
    """
    key=(Path(sc.file.name).name,cap)
    if key in _SCENE_TEXELS:return _SCENE_TEXELS[key]
    out={}
    for i,(typ,t) in sc.objects.items():
        if typ!='SpriteRenderer' or not t['m_Sprite']['m_PathID'] or t['m_DrawMode']!=0:continue
        gid=t['m_GameObject']['m_PathID']
        # A renderer whose transform chain leaves the scene file (Room_shop has
        # one) has no world position to project; no view draws it either.
        try:z=sc.point(gid)[2]
        except KeyError:continue
        if z<=CAM_Z+2:continue
        try:
            o=s.ref(sc.file,t['m_Sprite']);sid=s.sid(o)
            if sid not in shared_geometry:shared_geometry[sid]=native_sprite_geometry(o)
        except Exception:continue
        sp,(x0,y0,x1,y1)=shared_geometry[sid]
        points=[sc.point(gid,x,y) for x,y in [(x0,y1),(x1,y1),(x0,y0)]];scale=FOCAL/(z-CAM_Z)
        w=math.dist(points[0],points[1])*scale;h=math.dist(points[0],points[2])*scale
        if w<0.5 or h<0.5:continue
        full=scenery_dimensions(w,h,cap);native=(sp.m_Rect.width,sp.m_Rect.height)
        k=min(1.0,native[0]/full[0],native[1]/full[1])
        full=(max(1,math.ceil(full[0]*k)),max(1,math.ceil(full[1]*k)))
        a=t['m_Color']['a'];old=out.get((sid,a),(0,0));out[(sid,a)]=(max(old[0],full[0]),max(old[1],full[1]))
    _SCENE_TEXELS[key]=out
    return out

def scenery_texture(atlas,images,shared_pixels,sid,image_factory,width,height,alpha,cap,target=None):
    target=target or scenery_dimensions(width,height,cap)
    key=(sid,*target,alpha,cap)
    if key not in images:
        if key not in shared_pixels:
            im=image_factory().convert('RGBA');im.putalpha(im.getchannel('A').point(lambda a:round(a*alpha)))
            shared_pixels[key]=im.resize(target,Image.Resampling.LANCZOS)
        images[key]=atlas.add(shared_pixels[key],*target)
    return images[key],shared_pixels[key]

def native_draw_range(scale,points):
    """Reason a draw would fail hk-format's Geometry check, or None when admissible."""
    q12=round(scale*4096)
    if not 1<=q12<=262144:return f'draw scale {q12}/4096 outside native range 1..262144'
    if any(abs(round(p[k]*scale*256))>8_000_000 for p in points for k in (0,1)):return 'draw coordinate exceeds native Q24.8 range'
    return None

EDGE_SNAP=0.005

def cooked_edge(a,b,region):
    """One source terrain segment as the cook stores it, or why it is refused.

    Returns `(segment,None)` for an admitted `([ax,ay],[bx,by])`, or
    `(None,reason)`. The three steps are in the order the cook applies them: the
    region's collision_bounds clip, the slope refusal, then the near-axis snap
    that gives the fixed-point guest stable horizontal/vertical support.

    This is the whole policy. tools/compare_world_regions.py reads source
    geometry back and has to reproduce the cook exactly to compare it, so the
    rule lives here once rather than in both.
    """
    bounds=region['collision_bounds']
    if max(a[0],b[0])<bounds[0] or min(a[0],b[0])>bounds[2] or max(a[1],b[1])<bounds[1] or min(a[1],b[1])>bounds[3]:
        return None,'outside collision bounds'
    if abs(a[0]-b[0])>EDGE_SNAP and abs(a[1]-b[1])>EDGE_SNAP and not region.get('allow_slopes'):
        return None,'sloped terrain edge'
    a=list(a);b=list(b)
    if abs(a[0]-b[0])<=EDGE_SNAP:b[0]=a[0]
    elif abs(a[1]-b[1])<=EDGE_SNAP:b[1]=a[1]
    return (a[:2],b[:2]),None

def unsupported_sprite_behavior(name,parent_name,component_types):
    # This named scene helper is driven by a PlayMaker fade/cover state machine.
    # Its ordinary Sprites/Lit material is also used by valid room scenery, so
    # material/shader identity alone must never be used as an exclusion rule.
    if name=='Inverse Remasker' and parent_name=='mask_container' and 'PlayMakerFSM' in component_types:
        return 'FSM-controlled inverse remasker'
    return None

BINARY_BLACK_WORDS=frozenset({0x0001,0x8000})

def canonical_black(palette,pixels,w,h):
    """A binary black palette with its opaque sentinel moved to index 1.

    The reveal fade (game/src/render.rs FADE_CLUT) maps index 1 to the
    subtractive white that takes the opaque black texels down and every other
    index to subtract-nothing, so it can only fade a black mask whose palette is
    exactly (0, 1, 0x8000...). The quantizer orders entries by use, and a mostly
    soft-edged mask (`black_fader`, 92% half-coverage texels) lands its opaque
    word at index 2. Swapping the two indices changes no drawn texel.
    """
    words=[int.from_bytes(bytes(palette[i*2:i*2+2]),'little') for i in range(16)]
    if words[0]!=0 or not set(words[1:])<=BINARY_BLACK_WORDS or 1 not in words[1:] or words[1]==1:return palette,pixels
    k=words.index(1,1)
    words[1],words[k]=words[k],words[1]
    swap={1:k,k:1}
    out=bytearray(pixels)
    for i,b in enumerate(out):
        lo,hi=b&15,b>>4
        out[i]=swap.get(lo,lo)|(swap.get(hi,hi)<<4)
    return b''.join(v.to_bytes(2,'little') for v in words),bytes(out)

def black_member_image(image,renderer):
    """A reveal member drawn black, as the texels it really shows.

    A SpriteRenderer coloured (0,0,0) draws its sprite as a black silhouette
    (Crossroads_04, _08 and _21 tint `cd_FG_rock_20` that way into their
    masks), and a sprite whose every visible texel is below one PS1 colour step
    is black on this hardware (Tutorial_01's `Tut_msk_01`: all rgb <= 4, its
    colours are resize bleed from transparent texels). Either is cooked as pure
    black so the binary black palette, and so the reveal fade, admits it.
    Returns None for a member that keeps its colours.
    """
    color=renderer['m_Color']
    rgba=image.convert('RGBA')
    if not (color['r']==color['g']==color['b']==0):
        visible=[p for p in rgba.getdata() if p[3]>=16]
        if not visible or max(max(p[:3]) for p in visible)>=8:return None
    black=Image.new('RGBA',rgba.size,(0,0,0,0));black.putalpha(rgba.getchannel('A'))
    return black

# MonoBehaviours that switch their own GameObject off shortly after it is
# enabled, so the renderer they sit beside is never a lasting part of the room.
SELF_DISABLING_EFFECTS=frozenset({'WaveEffectControl'})

def self_disabling_effect(sc,gid):
    return any(sc.objects.get(c['component']['m_PathID'],(None,))[0] in SELF_DISABLING_EFFECTS
               for c in sc.gos[gid]['m_Component'])

def unsupported_remasker(s,sc,gid):
    g=sc.gos[gid];t=sc.transforms[sc.go_transform[gid]];father=t['m_Father']['m_PathID']
    parent=sc.gos[sc.transforms[father]['m_GameObject']['m_PathID']]['m_Name'] if father else ''
    components=[s.ref(sc.file,c['component']) for c in g['m_Component']]
    kind=unsupported_sprite_behavior(g['m_Name'],parent,[s.typename(c) for c in components])
    if not kind:return None
    render=next(c for c in components if c.type.name=='SpriteRenderer');r=s.read(render);materials=[]
    for ref in r['m_Materials']:
        mo=s.ref(sc.file,ref);m=s.read(mo);so=s.ref(mo.assets_file,m['m_Shader']);shader=s.read(so)
        materials.append({'id':s.sid(mo),'name':m['m_Name'],'shader_id':s.sid(so),'shader_name':shader.get('m_ParsedForm',{}).get('m_Name',shader.get('m_Name',''))})
    return {'id':s.sid(render),'type':kind,'game_object':f'{Path(sc.file.name).name}:{gid}','name':g['m_Name'],'parent':parent,
        'sprite':s.sid(s.ref(sc.file,r['m_Sprite'])),'materials':materials,'mask_interaction':r['m_MaskInteraction'],
        'controllers':[s.sid(c) for c in components if s.typename(c) in ('PlayMakerFSM','PersistentBoolItem')],
        'error':'Omitted: authored cover/fade state and persistent activation are unsupported; rendering this helper as static scenery produces an opaque screen cover.'}

def native_sprite_geometry(o):
    sp=o.read()
    mesh=MeshHandler(sp.m_RD,o.version);mesh.process()
    if not mesh.m_Vertices:raise ValueError('sprite has no mesh vertices')
    # UnityPy crops the tight sprite to mesh minima; preserve that origin.
    xs=[v[0] for v in mesh.m_Vertices];ys=[v[1] for v in mesh.m_Vertices]
    return sp,(min(xs),min(ys),max(xs),max(ys))

def native_sprite(o):
    sp,box=native_sprite_geometry(o)
    return sp.image.convert('RGBA'),box

def tk_sprite(s,file,collection,index,textures):
    d=collection['spriteDefinitions'][index];mat=s.ref(file,d['material']);m=s.read(mat)
    refs=dict(m['m_SavedProperties']['m_TexEnvs']);to=s.ref(mat.assets_file,refs['_MainTex']['m_Texture']);key=s.sid(to)
    if key not in textures:textures[key]=to.read().image.convert('RGBA')
    tex=textures[key];pos=d['positions'];uv=d['uvs']
    if len(pos)!=4 or d['complexGeometry']:raise ValueError('non-quad tk2d sprite')
    xmin=min(v['x'] for v in pos);xmax=max(v['x'] for v in pos);ymin=min(v['y'] for v in pos);ymax=max(v['y'] for v in pos)
    # Recover authored vertex-to-UV mapping, including 90-degree atlas packing.
    p00=min(range(4),key=lambda i:abs(pos[i]['x']-xmin)+abs(pos[i]['y']-ymax))
    p10=min(range(4),key=lambda i:abs(pos[i]['x']-xmax)+abs(pos[i]['y']-ymax))
    p01=min(range(4),key=lambda i:abs(pos[i]['x']-xmin)+abs(pos[i]['y']-ymin))
    a,b,c=[(uv[i]['x']*tex.width,(1-uv[i]['y'])*tex.height) for i in [p00,p10,p01]]
    w=max(1,round((xmax-xmin)/d['texelSize']['x']));h=max(1,round((ymax-ymin)/d['texelSize']['y']))
    im=tex.transform((w,h),Image.Transform.AFFINE,((b[0]-a[0])/w,(c[0]-a[0])/h,a[0],(b[1]-a[1])/w,(c[1]-a[1])/h,a[1]),Image.Resampling.BILINEAR)
    return im,(xmin,ymin,xmax,ymax)

def tk2d_wall_draw(s,sc,record,atlas,images,shared_pixels,cap,cull=None):
    """`Break Wall 2`'s tk2dSprite as one scenery draw, like a SpriteRenderer's.

    tk2d applies the sprite's own `_scale` to its mesh before the transform, and
    the MeshRenderer carries the sorting. Crumble's frames are all transparent,
    so the idle sprite is the only art the wall ever shows.
    """
    rid=int(record['renderer_source'].split(':')[1]);typ,sprite=sc.objects[rid]
    if typ!='tk2dSprite':raise ValueError('tk2d wall renderer is not a tk2dSprite')
    gid=sprite['m_GameObject']['m_PathID']
    renderer=next((t for i,k,t in ((c['component']['m_PathID'],)+sc.objects.get(c['component']['m_PathID'],(None,None)) for c in sc.gos[gid]['m_Component']) if k=='MeshRenderer'),None)
    if renderer is None or not renderer['m_Enabled']:raise ValueError('tk2d wall has no enabled MeshRenderer')
    collection_o=s.ref(sc.file,sprite['collection']);collection=s.read(collection_o)
    image,box=tk_sprite(s,collection_o.assets_file,collection,sprite['_spriteId'],{})
    k=sprite['_scale'];x0,y0,x1,y1=box[0]*k['x'],box[1]*k['y'],box[2]*k['x'],box[3]*k['y']
    points=[sc.point(gid,x,y) for x,y in [(x0,y1),(x1,y1),(x0,y0),(x1,y0)]]
    z=points[0][2];scale=FOCAL/(z-CAM_Z)
    if cull is not None:
        # The camera-range test every SpriteRenderer of a view passes.
        (cx0,cx1),(cy0,cy1)=cull;xs=[p[0] for p in points];ys=[p[1] for p in points]
        if max(xs)<cx0-160/scale or min(xs)>cx1+160/scale or max(ys)<cy0-120/scale or min(ys)>cy1+120/scale:return None
    w=math.dist(points[0],points[1])*scale;h=math.dist(points[0],points[2])*scale
    reason=native_draw_range(scale,points)
    if reason:raise ValueError(reason)
    color=sprite['_color']
    texture,_=scenery_texture(atlas,images,shared_pixels,s.sid(collection_o)+f":{sprite['_spriteId']}",lambda:image,w,h,color['a'],cap)
    return {'source':record['renderer_source'],'sprite':s.sid(collection_o)+f":{sprite['_spriteId']}",'name':sc.gos[gid]['m_Name'],
            'texture':texture,'points':points,'scale':scale,'tint':[min(255,round(color[c]*128)) for c in 'rgb'],'z':z,
            'order':renderer['m_SortingOrder'],'layer':renderer['m_SortingLayer']}

# Animated tk2d scenery: an object whose only behaviour is its sprite animator
# playing its default clip (Crossroads_ShamanTemple's torches, Crossroads_30's
# waterfalls), or one whose behaviour the port does not move yet and whose look
# is that clip (LiftChain's idle chains, Tutorial_01's glow bugs, which also
# wander a unit about their perch). Each unique frame of the clip is cooked as a
# scenery draw at the object's authored transform, so it sorts with the scenery
# around it, and game/src/decor.rs shows one of them per frame. The key is the
# set of behaviours on the object and its ancestors, so an object anything else
# drives is never claimed here.
DECOR_BEHAVIOURS={frozenset():'loop',frozenset({'LiftChain'}):'chain',
                  frozenset({'FSM:glow_bug','PlayMakerFixedUpdate'}):'glow_bug'}
# Scenes whose decor waits on a RAM decision. A frame is a scenery draw, and the
# guest sizes its per-draw arrays (about 100 bytes a draw) by the busiest view
# and its scene arena by the largest scene: the Ancestral Mound's 18 torches
# lift the busiest view from 704 to 768 draws, and King's Pass (the arena's
# largest scene) grows by its 22 glow bugs; together the image overflowed RAM
# by 10,212 bytes. MISSING.md has the numbers and the options.
DECOR_DEFERRED=frozenset({'Crossroads_ShamanTemple','Tutorial_01'})
# Components that say nothing about what drives an object.
DECOR_PLAIN=frozenset({'GameObject','Transform','SpriteRenderer','MeshRenderer','MeshFilter','tk2dSprite',
    'tk2dSpriteAnimator','Animator','PlayFromRandomFrameMecanim','SetZ','SetZRandom','AudioSource',
    'BoxCollider2D','CircleCollider2D','PolygonCollider2D','EdgeCollider2D','Rigidbody2D','ParticleSystem',
    'ParticleSystemRenderer','ParticleSystemAutoRecycle','ParticleSystemCollisionLagFix','ReduceParticleEffects',
    'NonBouncer','NonThunker','SpriteFlash','ObjectBounce','SpinSelfSimple'})

def decor_behaviours(sc,gid):
    """Behaviours on an object and its ancestors: script names, FSMs by name."""
    found=set()
    while gid in sc.gos:
        for c in sc.gos[gid]['m_Component']:
            kind,tree=sc.objects.get(c['component']['m_PathID'],(None,None))
            if kind is None or kind in DECOR_PLAIN:continue
            found.add('FSM:'+tree['fsm']['name'] if kind=='PlayMakerFSM' else kind)
        father=sc.transforms[sc.go_transform[gid]]['m_Father']['m_PathID'] if gid in sc.go_transform else 0
        if not father or father not in sc.transforms:break
        gid=sc.transforms[father]['m_GameObject']['m_PathID']
    return frozenset(found)

def decor_sources(sc):
    """Every active, rendered tk2dSprite whose behaviours DECOR_BEHAVIOURS names."""
    out=[]
    for i,(kind,sprite) in sorted(sc.objects.items()):
        if kind!='tk2dSprite':continue
        gid=sprite['m_GameObject']['m_PathID']
        if gid not in sc.gos or not sc.active(gid):continue
        family=DECOR_BEHAVIOURS.get(decor_behaviours(sc,gid))
        if family is None:continue
        parts={}
        for c in sc.gos[gid]['m_Component']:
            ck,ct=sc.objects.get(c['component']['m_PathID'],(None,None))
            if ck in ('MeshRenderer','tk2dSpriteAnimator'):parts[ck]=ct
        renderer=parts.get('MeshRenderer')
        if renderer is None or not renderer['m_Enabled']:continue
        out.append({'sprite_id':i,'gid':gid,'renderer_source':sc.sid(i),'family':family,
                    'animator':parts.get('tk2dSpriteAnimator'),'renderer':renderer})
    return out

def decor_draws(s,sc,record,atlas,images,shared_pixels,cap,cull):
    """One scenery draw per unique frame of the object's playing clip.

    Returns the draws (in clip frame order of first use) and the clip: rate,
    guest wrap, loop start and the sequence of draw offsets it steps through.
    None when no frame of it reaches any camera position of this view.
    """
    sprite=sc.objects[record['sprite_id']][1];gid=record['gid'];animator=record['animator'];renderer=record['renderer']
    collection_o=s.ref(sc.file,sprite['collection'])
    if animator and animator['playAutomatically'] and 0<=animator['defaultClipId']:
        library_o=s.ref(sc.file,animator['library']);library=s.read(library_o)
        clip=library['clips'][animator['defaultClipId']]
        steps=[(s.ref(library_o.assets_file,f['spriteCollection']),f['spriteId']) for f in clip['frames']]
        fps,wrap,loop_start=clip['fps'],guest_wrap(clip),clip.get('loopStart',0)
    else:
        steps=[(collection_o,sprite['_spriteId'])];fps,wrap,loop_start=0,2,0
    collections={};textures={};unique=[];sequence=[]
    for co,index in steps:
        key=(s.sid(co),index)
        if key not in unique:unique.append(key)
        sequence.append(unique.index(key))
    k=sprite['_scale'];color=sprite['_color'];draws=[];visible=False
    for sid,index in unique:
        co=next(c for c,i in steps if s.sid(c)==sid)
        if sid not in collections:collections[sid]=s.read(co)
        image,box=tk_sprite(s,co.assets_file,collections[sid],index,textures)
        x0,y0,x1,y1=box[0]*k['x'],box[1]*k['y'],box[2]*k['x'],box[3]*k['y']
        points=[sc.point(gid,x,y) for x,y in [(x0,y1),(x1,y1),(x0,y0),(x1,y0)]]
        z=points[0][2]
        if z<=CAM_Z+2:return None
        scale=FOCAL/(z-CAM_Z)
        (cx0,cx1),(cy0,cy1)=cull;xs=[p[0] for p in points];ys=[p[1] for p in points]
        if not(max(xs)<cx0-160/scale or min(xs)>cx1+160/scale or max(ys)<cy0-120/scale or min(ys)>cy1+120/scale):visible=True
        w=math.dist(points[0],points[1])*scale;h=math.dist(points[0],points[2])*scale
        reason=native_draw_range(scale,points)
        if reason:raise ValueError(reason)
        texture,_=scenery_texture(atlas,images,shared_pixels,f'{sid}:{index}',lambda image=image:image,w,h,color['a'],cap)
        draws.append({'source':record['renderer_source'],'sprite':f'{sid}:{index}','name':sc.gos[gid]['m_Name'],
            'texture':texture,'points':points,'scale':scale,'tint':[min(255,round(color[c]*128)) for c in 'rgb'],'z':z,
            'order':renderer['m_SortingOrder'],'layer':renderer['m_SortingLayer']})
    if not visible:return None
    return {'draws':draws,'family':record['family'],'source':record['renderer_source'],'fps':fps,'wrap':wrap,
            'loop_start':loop_start,'sequence':sequence}

def guest_wrap(clip):
    """tk2d wrapMode to the guest's loop/loop-section/once set.

    Loop (0), LoopSection (1) and Once (2) map directly. Single (6) holds one
    frame, which is Once for a one-frame clip. Other modes fail closed.
    """
    mode=clip['wrapMode'];count=len(clip['frames'])
    if mode in (0,1,2):return mode
    if mode==6 and count==1:return 2
    raise ValueError(f'unsupported tk2d wrap mode {mode} for {count}-frame clip {clip.get("name")}')

def append_actor_art(s,sc,actors,atlas,frames,clips):
    """Append original clips only for the explicitly recognized actor controller."""
    textures={};collections={};cache={};clip_cache={}
    for actor in actors:
        if not actor['movement_supported']:continue
        control=actor['movement_control']
        library_o=s.ref(sc.file,actor['tk2dSpriteAnimator']['library']);library=s.read(library_o)
        matrix=sc.world(sc.go_transform[actor['game_object']]);sprite=actor['tk2dSprite']
        sx=abs(matrix[0][0]*sprite['_scale']['x']);sy=abs(matrix[1][1]*sprite['_scale']['y'])
        color=sprite['_color'];alpha=round(color['a']*255)
        # A recognizer that already knows which of its clips are the walk and
        # turn slots says so, rather than growing this chain a branch per
        # controller. The False Knight is the first: its subset is Idle and Turn
        # and it has no walk at all.
        if 'art_bindings' in control:
            bindings=dict(control['art_bindings'])
        elif control['kind']=='ZombieSwipeWalker':
            if control['parameters'].get('attack',{}).get('kind')=='Gas':
                # The Shaker's one Attack clip anticipates, bursts and cools down (its loop section
                # holds the last three frames); Attack End is never played.
                bindings={'walk':'Walk','turn':'Turn','idle':'Idle','anticipate':'Attack','lunge':'Attack','cooldown':'Attack'}
            elif control['parameters'].get('attack',{}).get('kind')=='Leap':
                # Leaper: the Attack clip anticipates and keeps playing through the
                # jump (lunge slot), Land is the cooldown clip.
                bindings={'walk':'Walk','turn':'Turn','idle':'Idle','anticipate':'Attack','lunge':'Attack','cooldown':'Land'}
            else:
                bindings={'walk':'Walk','turn':'Turn','idle':'Idle','anticipate':'Attack Anticipate',
                          'lunge':'Attack Lunge','cooldown':'Attack Cooldown'}
            # Barger and Hornhead libraries have no Fall clip; the controller never plays it.
            if any(c['name']=='Fall' for c in library['clips']):bindings['fall']='Fall'
        elif control['kind']=='WalkLeftRight':
            bindings={kind:control[kind+'_clip_name'] for kind in ('walk','turn')}
        elif control['kind']=='Climber':
            # Walk continues through source turns; the walk clip doubles as `turn_clip`.
            bindings={'walk':'Walk','turn':'Walk','stun':'Stun'}
        elif control['kind']=='Vengefly':
            # Idle doubles as `walk_clip`; TurnToIdle is the idle-facing turn.
            bindings={'walk':'Idle','turn':'TurnToIdle','startle':'Startle','chase':'Chase','turn_fly':'TurnToFly'}
        elif control['kind']=='Gruzzer':
            bindings={'walk':'Fly','turn':'Fly'}
        elif control['kind']=='Baldur':
            # Idle doubles as `walk_clip`; the turn slot is unused and holds Idle too.
            bindings={'walk':'Idle','turn':'Idle','start':'Start','roll':'Roll','stop':'Stop'}
        elif control['kind']=='Aspid':
            bindings={'walk':'Fly','turn':'TurnToFly','fire':'Fire Long'}
        elif control['kind']=='EggSac':
            # One looping Idle and no movement: the walk and turn slots hold it too.
            bindings={'walk':'Idle','turn':'Idle','idle':'Idle'}
        else:
            raise ValueError('unsupported actor art controller: '+control['kind'])
        # Actor sampling is never reduced: a frame the animation cache cannot
        # hold makes the actor an explicit unsupported record for this cook.
        # A frame past one slot binds a rectangle of them through
        # Atlas.add_tiled, the same route NPC art takes, and
        # enemies.rs::prepare_draws submits one quad per tile with
        # Room::frame_tile's share of the frame's world box.
        #
        # Two frames are still refused, and the refusal is a whole-actor
        # pre-pass so a rejected actor never leaves half a clip in the atlas:
        # one past the tile budget, and one past the axis clamp, which would
        # otherwise resize the art silently.
        def frame_dims(name):
            clip=next(c for c in library['clips'] if c['name']==name)
            for frame in clip['frames']:
                collection_o=s.ref(library_o.assets_file,frame['spriteCollection']);sid=s.sid(collection_o)
                if sid not in collections:collections[sid]=s.read(collection_o)
                _,box=tk_sprite(s,collection_o.assets_file,collections[sid],frame['spriteId'],textures);scale=FOCAL/-CAM_Z
                yield math.ceil((box[2]-box[0])*sx*scale),math.ceil((box[3]-box[1])*sy*scale)
        def frame_tiles(w,h):return -(-w//SLOT_PIXELS)*-(-h//SLOT_PIXELS)
        refused=sorted((w,h) for name in bindings.values() for w,h in frame_dims(name)
                       if frame_tiles(w,h)>MAX_FRAME_TILES
                       or w>MAX_TEXTURE_AXIS or h>MAX_TEXTURE_AXIS)
        if refused:
            w,h=refused[-1]
            reason=(f'exceeds the {MAX_TEXTURE_AXIS}-pixel texture axis and would be resampled'
                    if w>MAX_TEXTURE_AXIS or h>MAX_TEXTURE_AXIS else
                    f'binds {frame_tiles(w,h)} of the {MAX_FRAME_TILES} animation slots a frame may hold')
            actor['movement_supported']=False
            actor.setdefault('limitations',[]).append(
                f'actor frame {w}x{h} {reason}; art not cooked')
            continue
        # A recognizer may ask for one palette per clip rather than one per
        # frame (`shared_palette`): the Blocker does, because its 26 per-frame
        # palettes were all that kept the Ancestral Mound's from fitting.
        shared=bool(control.get('shared_palette'))
        for kind,name in bindings.items():
            clip_key=(s.sid(library_o),name,sx,sy,alpha)
            if clip_key not in clip_cache and shared:
                clip=next(c for c in library['clips'] if c['name']==name);start=len(frames)
                pending=[]
                for frame in clip['frames']:
                    collection_o=s.ref(library_o.assets_file,frame['spriteCollection']);sid=s.sid(collection_o)
                    if sid not in collections:collections[sid]=s.read(collection_o)
                    index=frame['spriteId']
                    image,box=tk_sprite(s,collection_o.assets_file,collections[sid],index,textures)
                    image.putalpha(image.getchannel('A').point(lambda a:round(a*color['a'])))
                    box=(box[0]*sx,box[1]*sy,box[2]*sx,box[3]*sy);scale=FOCAL/-CAM_Z
                    pending.append((image,(box[2]-box[0])*scale,(box[3]-box[1])*scale,box,f'{sid}:{index}',frame))
                for texture,(_,_,_,box,sprite,frame) in zip(atlas.add_frames_shared([p[:3] for p in pending]),pending):
                    frames.append({'texture':texture,'box':box,'sprite':sprite,'event':frame})
                clip_cache[clip_key]=len(clips)
                clips.append({'name':f'{s.sid(library_o)}/{name}','start':start,'count':len(clip['frames']),
                              'fps':clip['fps'],'wrap':guest_wrap(clip),'loopStart':clip.get('loopStart',0)})
            if clip_key not in clip_cache:
                clip=next(c for c in library['clips'] if c['name']==name);start=len(frames)
                # A recognizer may keep every `stride`th frame of a clip at the
                # matching fraction of its rate (`frame_stride`), so the clip
                # lasts as long as the source's: the Husk Guard's loops do, to
                # fit its scene's 256 KiB actor bank.
                stride=control.get('frame_stride',{}).get(name,1)
                if stride!=1:
                    clip=dict(clip,frames=clip['frames'][::stride],fps=clip['fps']/stride,
                              loopStart=clip.get('loopStart',0)//stride)
                for frame in clip['frames']:
                    collection_o=s.ref(library_o.assets_file,frame['spriteCollection']);sid=s.sid(collection_o)
                    if sid not in collections:collections[sid]=s.read(collection_o)
                    index=frame['spriteId'];key=(sid,index,sx,sy,alpha)
                    if key not in cache:
                        image,box=tk_sprite(s,collection_o.assets_file,collections[sid],index,textures)
                        image.putalpha(image.getchannel('A').point(lambda a:round(a*color['a'])))
                        box=(box[0]*sx,box[1]*sy,box[2]*sx,box[3]*sy)
                        scale=FOCAL/-CAM_Z
                        # add_tiled returns the frame's first texture; a frame
                        # inside one slot is still a single streamed texture, so
                        # every actor that cooked before cooks byte for byte.
                        texture=atlas.add_tiled(image,(box[2]-box[0])*scale,(box[3]-box[1])*scale)
                        cache[key]=(texture,box)
                    texture,box=cache[key]
                    frames.append({'texture':texture,'box':box,'sprite':f'{sid}:{index}','event':frame})
                clip_cache[clip_key]=len(clips)
                clips.append({'name':f'{s.sid(library_o)}/{name}','start':start,'count':len(clip['frames']),
                              'fps':clip['fps'],'wrap':guest_wrap(clip),'loopStart':clip.get('loopStart',0)})
            actor[kind+'_clip']=clip_cache[clip_key]
        actor['visual_scale']=[sx,sy]
    from effects import append_corpse_art
    append_corpse_art(s,sc,actors,atlas,frames,clips)
    append_barrel_art(s,actors,atlas,frames,clips)

def append_barrel_art(s,actors,atlas,frames,clips):
    """`FK Barrel Summon`'s pooled `Falling Barrel`, as one streamed frame.

    The barrel is a SpriteRenderer prefab in sharedassets48, not a tk2d clip, so
    it takes neither the actor path above nor effects.py's _ClipCooker: one
    native sprite, one frame, one clip the projectile pool plays exactly the way
    it plays the Aspid shot's. The frame's box is the sprite's own mesh extent,
    which is what enemies.rs::prepare_draws turns into the four corners.

    host/false_knight.py::barrel_source already refused anything about the
    prefab that this draw would get wrong, so nothing here has a second opinion
    about scale, colour or flip.
    """
    for actor in actors:
        control=actor.get('movement_control',{})
        if not actor['movement_supported'] or control.get('kind')!='FalseKnight':continue
        barrel=control['barrel'];name,path_id=barrel['sprite'].rsplit(':',1)
        image,box=native_sprite(s.file(name).objects[int(path_id)]);scale=FOCAL/-CAM_Z
        actor['barrel_clip']=len(clips)
        clips.append({'name':barrel['source']+'/'+BARREL_CLIP,'start':len(frames),'count':1,
                      'fps':1.,'wrap':2,'loopStart':0})
        frames.append({'texture':atlas.add(image,(box[2]-box[0])*scale,(box[3]-box[1])*scale,streamed=True),
                       'box':box,'sprite':barrel['sprite'],'event':{}})

from alpha_covers import HAS_ALPHA_COVERS, RECORD_BYTES, record as alpha_cover_record

# One animation slot, and the tile rectangle a frame may bind beside the four
# keys main.rs always reserves. Both come from hk_cache, where the cache is 24
# slots: eight in the strip at x320 and sixteen in the texture page static
# scenery page 19 gave up. A slot cannot be larger than 64x64, so an oversized
# frame takes more of them rather than a bigger one.
SLOT_PIXELS=64
ANIMATION_SLOTS=24
RESERVED_SLOTS=4
MAX_FRAME_TILES=ANIMATION_SLOTS-RESERVED_SLOTS
# The largest axis a cooked texture can express, and the size add()/add_tiled()
# silently clamp to. Scenery is resampled to fit on purpose; actor art is not,
# so the actor path refuses a frame this clamp would resize rather than ship a
# squashed one.
MAX_TEXTURE_AXIS=252


# HKROOM02's own texture-table limit, checked again by hk_format::MAX_TEXTURES.
# It bounds the 16-byte records a region may carry, which is a different
# question from how many CLUT slots those records need.
MAX_ROOM_TEXTURES=640


class Atlas:
    def __init__(self,deduplicate=True,max_pages=20,max_textures=MAX_ROOM_TEXTURES,alpha_covers=True,max_cluts=None):
        self.flags=HAS_ALPHA_COVERS if alpha_covers else 0
        self.animation_bytes=0;self.alpha_cover_bytes=0
        self.max_textures=max_textures
        # A CLUT slot is a distinct palette, not a texture. HKROOM02 lets any
        # number of textures name one palette index, and the scene bank pools
        # palettes by value before anything reaches VRAM, so the slots a region
        # really costs are its distinct palette words. Callers that only know
        # one number keep the old behaviour of spending both against it.
        self.max_cluts=max_textures if max_cluts is None else max_cluts
        self.cluts=0
        self.max_pages=max_pages
        self.images=[];self.entries=[];self.pages=[];self.palettes=[];self.streamed=set();self.stream=bytearray()
        self.quantized=[];self.canonical={};self.request_map=[];self.deduplicate=deduplicate
        # index -> (columns, rows) for the first tile of a multi-slot frame, and
        # every tile's first tile. Both are empty until add_tiled runs.
        self.grids={};self.tile_owner={}
    def add_quantized(self,w,h,palette,pixels,streamed=False,unique=False):
        """Admit exact final texels; different palettes/storage classes never alias.

        `unique` opts a texture out of canonical sharing. A frame's tiles are
        consecutive texture IDs on the guest, so a tile that deduplicated
        against an earlier one would renumber the rest of its frame.
        """
        if not 1<=w<=256 or not 1<=h<=256 or len(palette)!=32 or len(pixels)!=((w+1)//2)*h:
            raise ValueError('invalid canonical texture dimensions/palette/texels')
        if streamed and (w>SLOT_PIXELS or h>SLOT_PIXELS):
            raise ValueError(f'animation cache dimensions exceed {SLOT_PIXELS}x{SLOT_PIXELS}: {w}x{h}')
        palette,pixels=canonical_black(palette,pixels,w,h)
        key=(bool(streamed),w,h,bytes(palette),bytes(pixels))
        index=self.canonical.get(key) if self.deduplicate and not unique else None
        if index is None:
            index=len(self.images);self.images.append(None);self.quantized.append((w,h,key[3],key[4]))
            if streamed:self.streamed.add(index)
            if not unique:self.canonical[key]=index
        self.request_map.append(index)
        return index
    def _quantize(self,im,w,h):
        """One 16-colour palette and one index plane for a whole frame.

        host/quantize.py decides each texel's class (transparent, opaque, or
        STP/Add semi) from its own alpha and clusters colours in the space the
        GPU displays. Pure-black art keeps the octree path below so black-mask
        palettes and their material admission are unchanged.
        """
        from quantize import quantize
        return quantize(im,w,h,fallback=lambda i,ww,hh:Atlas._quantize_octree(None,i,ww,hh))
    def _quantize_octree(self,im,w,h):
        """The original octree quantiser, kept for pure-black mask art."""
        im=im.resize((w,h),Image.Resampling.LANCZOS)
        q=im.quantize(colors=15,method=Image.Quantize.FASTOCTREE,dither=Image.Dither.NONE)
        pal=q.getpalette('RGBA')+[0]*60;colors=[0]
        for k in range(15):
            r,g,b,a=pal[k*4:k*4+4]
            semi=a<224
            gain=a/255 if semi else 1.0
            r,g,b=[round(v*gain) for v in (r,g,b)]
            v=(r>>3)|((g>>3)<<5)|((b>>3)<<10)
            colors.append((v|0x8000) if semi else (v or 1))
        px=list(q.get_flattened_data());alpha=list(im.getchannel('A').get_flattened_data())
        plane=[px[i]+1 if alpha[i]>=16 else 0 for i in range(w*h)]
        return im,struct.pack('<16H',*colors),plane
    @staticmethod
    def _pack_plane(plane,plane_width,x0,y0,w,h):
        stride=(w+1)//2;pixels=bytearray(stride*h)
        for yy in range(h):
            row=(y0+yy)*plane_width+x0
            for xx in range(w):
                pixels[yy*stride+xx//2]|=plane[row+xx]<<((xx&1)*4)
        return pixels
    def add(self,im,w,h,streamed=False):
        w=max(1,min(MAX_TEXTURE_AXIS,math.ceil(w)));h=max(1,min(MAX_TEXTURE_AXIS,math.ceil(h)))
        if streamed and (w>SLOT_PIXELS or h>SLOT_PIXELS):
            raise ValueError(f'animation cache dimensions exceed {SLOT_PIXELS}x{SLOT_PIXELS}: {w}x{h}')
        im,palette,plane=self._quantize(im,w,h)
        index=self.add_quantized(w,h,palette,self._pack_plane(plane,w,0,0,w,h),streamed)
        self.images[index]=im
        return index
    def add_tiled(self,im,w,h):
        """Admit one streamed frame across a rectangle of 64x64 animation slots.

        The frame is quantized once, so the tiles hold identical colours and a
        seam does not shift hue where two slots meet. That also makes them
        cheap: every tile of a frame carries the same palette words, and the
        region's CLUT cost counts distinct words, so a twelve-tile frame spends
        one slot rather than twelve.

        Tiles are emitted row-major as consecutive texture IDs and the first
        carries the grid, which is what hk_format::Room::frame_grid reads back.
        """
        w=max(1,min(MAX_TEXTURE_AXIS,math.ceil(w)));h=max(1,min(MAX_TEXTURE_AXIS,math.ceil(h)))
        cols=-(-w//SLOT_PIXELS);rows=-(-h//SLOT_PIXELS)
        if cols*rows>MAX_FRAME_TILES:
            raise ValueError(f'frame {w}x{h} binds {cols*rows} of the {MAX_FRAME_TILES} '
                             'animation slots a frame may hold')
        if cols*rows==1:
            return self.add(im,w,h,streamed=True)
        im,palette,plane=self._quantize(im,w,h)
        base=None
        for row in range(rows):
            for col in range(cols):
                x0,y0=col*SLOT_PIXELS,row*SLOT_PIXELS
                tw,th=min(SLOT_PIXELS,w-x0),min(SLOT_PIXELS,h-y0)
                index=self.add_quantized(tw,th,palette,self._pack_plane(plane,w,x0,y0,tw,th),
                                         streamed=True,unique=True)
                if base is None:
                    base=index;self.grids[base]=(cols,rows)
                elif index!=base+row*cols+col:
                    raise ValueError('a frame\'s tiles are no longer consecutive texture IDs')
                self.tile_owner[index]=base
                self.images[index]=im.crop((x0,y0,x0+tw,y0+th))
        return base
    def add_frames_shared(self,items):
        """Several streamed frames on one shared palette: [(image, w, h)] -> the
        first texture of each, as add_tiled returns it.

        Every frame is resized exactly as `add` would, laid in one strip with a
        transparent gap wider than the quantizer's edge reach, and quantized
        once, so the whole group spends one CLUT slot where frame-by-frame
        quantization spends one per distinct palette. An actor whose frames all
        draw from one small set of colours loses nothing a 16-colour palette
        could have kept per frame; the caller decides which actors take this.
        """
        from quantize import EDGE_REACH
        gap=EDGE_REACH*2+2
        sized=[]
        for im,w,h in items:
            w=max(1,min(MAX_TEXTURE_AXIS,math.ceil(w)));h=max(1,min(MAX_TEXTURE_AXIS,math.ceil(h)))
            cols=-(-w//SLOT_PIXELS);rows=-(-h//SLOT_PIXELS)
            if cols*rows>MAX_FRAME_TILES:
                raise ValueError(f'frame {w}x{h} binds {cols*rows} of the {MAX_FRAME_TILES} animation slots a frame may hold')
            sized.append((im.convert('RGBA').resize((w,h),Image.Resampling.LANCZOS),w,h,cols,rows))
        width=sum(w for _,w,_,_,_ in sized)+gap*(len(sized)-1);height=max(h for _,_,h,_,_ in sized)
        sheet=Image.new('RGBA',(width,height),(0,0,0,0));x=0;origins=[]
        for im,w,h,_,_ in sized:
            sheet.paste(im,(x,0));origins.append(x);x+=w+gap
        _,palette,plane=self._quantize(sheet,width,height)
        firsts=[]
        for (im,w,h,cols,rows),x0 in zip(sized,origins):
            sub=[plane[(y)*width+x0+xx] for y in range(h) for xx in range(w)]
            if cols*rows==1:
                index=self.add_quantized(w,h,palette,self._pack_plane(sub,w,0,0,w,h),streamed=True)
                self.images[index]=im;firsts.append(index);continue
            base=None
            for row in range(rows):
                for col in range(cols):
                    tx,ty=col*SLOT_PIXELS,row*SLOT_PIXELS
                    tw,th=min(SLOT_PIXELS,w-tx),min(SLOT_PIXELS,h-ty)
                    index=self.add_quantized(tw,th,palette,self._pack_plane(sub,w,tx,ty,tw,th),streamed=True,unique=True)
                    if base is None:
                        base=index;self.grids[base]=(cols,rows)
                    elif index!=base+row*cols+col:
                        raise ValueError('a frame\'s tiles are no longer consecutive texture IDs')
                    self.tile_owner[index]=base
                    self.images[index]=im.crop((tx,ty,tx+tw,ty+th))
            firsts.append(base)
        return firsts
    def pack(self):
        self.entries=[None]*len(self.quantized);self.palettes=[];self.stream=bytearray();distinct=set()
        page_count,placements=dense_pack([(aligned(w),h,i)for i,(w,h,palette,pixels)in enumerate(self.quantized)if i not in self.streamed])
        if page_count>self.max_pages:raise ValueError('4bpp VRAM page budget exceeded')
        self.pages=[bytearray(32768)for _ in range(page_count)]
        positions={i:(page,x,y)for i,page,x,y,w,h in placements}
        for i in sorted(range(len(self.quantized)),key=lambda i:(-self.quantized[i][1],-self.quantized[i][0],i)):
            w,h,palette,pixels=self.quantized[i];slot=None;stream_offset=0
            if i in self.streamed:
                # Original UV dimensions survive word-aligned transfer padding.
                self.stream.extend(bytes((-len(self.stream))&3))
                stream_offset=len(self.stream);stride=((w+3)&~3)//2
                self.stream.extend(bytes(stride*h))
                # A streamed texture's VRAM origin is its animation slot, so
                # these two fields carry the frame's tile grid instead.
                p,(x,y)=65535,self.grids.get(i,(0,0))
            else:
                p,x,y=positions[i]
            # The block keeps one 32-byte entry per texture because that is how
            # the reader sizes it, but repeats cost nothing in VRAM: the scene
            # bank pools palettes by value and a scene uploads that pool, so the
            # slots a region spends are its distinct palette words. The tiles of
            # one frame were quantized together, which is what keeps their
            # colours identical across the seam, and it now makes them cheaper.
            owner=self.tile_owner.get(i)
            if owner is not None and self.quantized[owner][2]!=palette:
                raise ValueError('tiles of one frame disagree on their palette')
            self.palettes.append(palette);cl=len(self.palettes)-1;distinct.add(palette)
            if len(distinct)>self.max_cluts:raise ValueError('CLUT budget exceeded')
            if len(self.palettes)>self.max_textures:raise ValueError('HKROOM02 texture table exceeded')
            for yy in range(h):
                for xx in range(w):
                    v=(pixels[yy*((w+1)//2)+xx//2]>>((xx&1)*4))&15
                    if i in self.streamed:
                        k=stream_offset+yy*stride+xx//2;shift=(xx&1)*4;target=self.stream
                    else:
                        k=((y+yy)*256+x+xx)//2;shift=((x+xx)&1)*4;target=self.pages[p]
                    target[k]=(target[k]&~(15<<shift))|(v<<shift)
            self.entries[i]=(p,x,y,w,h,cl,stream_offset)
        self.stream.extend(bytes((-len(self.stream))&3))
        self.cluts=len(distinct)
        self.animation_bytes=len(self.stream);self.alpha_cover_bytes=0
        if self.flags&HAS_ALPHA_COVERS:
            for i,(w,h,palette,pixels) in enumerate(self.quantized):
                if i in self.streamed:continue
                self.entries[i]=(*self.entries[i][:6],len(self.stream))
                self.stream.extend(alpha_cover_record(w,h,palette,pixels));self.alpha_cover_bytes+=RECORD_BYTES
        if len(self.stream)>256*1024:raise ValueError('animation and alpha metadata RAM bank exceeds 256 KiB')


def cook(source=None,scene=None,region=None,output=None,shared_pixels=None,shared_geometry=None,write_shared=True,stage_for_similarity=False):
    out=Path(output or ROOT/'data');out.mkdir(parents=True,exist_ok=True)
    s=source or Source();sc=scene or Scene(s,'level6');scene_file=Path(sc.file.name).name
    region=region or {'camera_x':CAM_X,'camera_y':CAM_Y,'collision_bounds':(15,-5,78,35),'activation_bounds':(15,-5,62,25)}
    cam_x,cam_y=region['camera_x'],region['camera_y']
    atlas=Atlas(max_pages=20 if stage_for_similarity else STATIC_PAGE_BUDGET,max_textures=MAX_ROOM_TEXTURES,
                max_cluts=MAX_ROOM_TEXTURES if stage_for_similarity else TEXTURE_BUDGET);draws=[];images={};unsupported=list(sc.errors);rendered=[]
    reveal=reveal_mask_sources(sc)
    supported_reveal_renderers={int(source.split(':')[-1]) for controller in reveal['controllers'] for source in controller['renderer_sources']}
    unsupported.extend(dict(record,type='unsupported reveal controller') for record in reveal['unsupported'])
    shared_pixels=shared_pixels if shared_pixels is not None else {}
    shared_geometry=shared_geometry if shared_geometry is not None else {}
    scenery_cap=SCENERY_SCENE_CAPS.get(region.get('scene_name','Tutorial_01'),SCENERY_TEXEL_CAP)
    sprite_texels=scene_sprite_texels(s,sc,scenery_cap,shared_geometry)
    grass=grass_sources(sc,region.get('grass_bounds'),errors=unsupported);cut_renderers={g[k] for g in grass for k in ('off','on')}
    breakables=breakable_sources(sc,region.get('interaction_bounds',region['activation_bounds']),errors=unsupported)
    from secret_breaks import secret_sources,bind_secrets,FAMILY_WALL_TK2D
    # Every view of the scene lists every secret, so a view the camera draws
    # while the Knight stands in another still hides broken art and fades its
    # masks; only the views that can reach it force its art into their draws.
    secrets=secret_sources(s,sc,errors=unsupported)
    reachable={r['source'] for r in secret_sources(s,sc,region.get('interaction_bounds',region['activation_bounds']))}
    actors=actor_sources(sc,region.get('interaction_bounds',region['activation_bounds']))
    hazards=hazard_sources(sc,region.get('interaction_bounds',region['activation_bounds']))
    shrooms=shroom_sources(sc,region.get('interaction_bounds',region['activation_bounds']))
    from benches import bench_sources
    benches=bench_sources(sc,region.get('interaction_bounds',region['activation_bounds']))
    forced_renderers=cut_renderers|{i for b in breakables for key in ('off_renderer_ids','on_renderer_ids') for i in b[key]}
    forced_renderers|={int(sid.split(':')[-1]) for b in breakables for fade in b.get('mask_fades',[]) for sid in fade['renderer_sources']}
    # A secret's art has to be a draw in every view that binds the secret, the
    # way a Breakable's whole and remnant parts are.
    by_sid={sc.sid(i):i for i,(typ,_) in sc.objects.items() if typ=='SpriteRenderer'}
    forced_renderers|={by_sid[sid] for r in secrets if r['source'] in reachable for sid in r['off_renderer_sources']+[m['renderer'] for m in r['moving']] if sid in by_sid}
    # Every renderer a reveal controller fades, including those a secret's
    # break uncovers, for the black member rule below.
    reveal_members={sid for controller in reveal['controllers'] for sid in controller['renderer_sources']}
    for i,(typ,t) in sc.objects.items():
        if typ!='SpriteRenderer':continue
        gid=t['m_GameObject']['m_PathID']
        if ((not t['m_Enabled'] or not sc.active(gid)) and i not in forced_renderers) or not t['m_Sprite']['m_PathID']:continue
        omission=unsupported_remasker(s,sc,gid) if sc.gos[gid]['m_Name']=='Inverse Remasker' and i not in supported_reveal_renderers else None
        if omission:
            unsupported.append(omission);continue
        if i not in forced_renderers and self_disabling_effect(sc,gid):
            # WaveEffectControl (a shiny's `White Wave`) grows to five times its
            # size and fades out over about a quarter second after OnEnable, then
            # deactivates its own GameObject. Drawn as static scenery it was a
            # permanent opaque white shape over every pickup (Crossroads_01's
            # Hallownest Seal, the Ancestral Mound's Soul Catcher). The pulse
            # plays while the room fades in from its load; it is not reproduced.
            unsupported.append({'id':sc.sid(i),'type':typ,'game_object':sc.sid(gid),'name':sc.gos[gid]['m_Name'],
                'error':'Omitted: WaveEffectControl deactivates this object about 0.27 s after it is enabled'});continue
        pos=sc.point(gid);z=pos[2]
        if z<=CAM_Z+2:continue
        try:
            o=s.ref(sc.file,t['m_Sprite']);sid=s.sid(o)
            if sid not in shared_geometry:shared_geometry[sid]=native_sprite_geometry(o)
            sp,box=shared_geometry[sid];x0,y0,x1,y1=box
            if t['m_FlipX']:x0,x1=-x0,-x1
            if t['m_FlipY']:y0,y1=-y0,-y1
            points=[sc.point(gid,x,y) for x,y in [(x0,y1),(x1,y1),(x0,y0),(x1,y0)]]
            scale=FOCAL/(z-CAM_Z)
            xs=[p[0] for p in points];ys=[p[1] for p in points]
            if i not in forced_renderers and (max(xs)<cam_x[0]-160/scale or min(xs)>cam_x[1]+160/scale or max(ys)<cam_y[0]-120/scale or min(ys)>cam_y[1]+120/scale):continue
            if t['m_DrawMode']!=0:raise ValueError('sliced/tiled SpriteRenderer unsupported')
            w=math.dist(points[0],points[1])*scale;h=math.dist(points[0],points[2])*scale
            if w<0.5 or h<0.5:continue
            reason=native_draw_range(scale,points)
            if reason:raise ValueError(reason)
            # The native scenery grid repair must be able to bound this draw's
            # packets; an unrepresentable stretched sprite is an explicit
            # unsupported record, not a late pack failure.
            target=sprite_texels.get((sid,t['m_Color']['a'])) or scenery_dimensions(w,h,scenery_cap)
            bound=packet_bound([(round(p[0]*scale*256),round(p[1]*scale*256)) for p in points],*target)
            if bound['packets']>CHILD_CAPACITY:raise ValueError(f'draw needs {bound["packets"]} children > {CHILD_CAPACITY}')
            # Preserve individual instances, original ordering and depth parallax.
            image_sid,factory=sid,lambda:sp.image
            if sc.sid(i) in reveal_members:
                black=black_member_image(sp.image,t)
                if black is not None:image_sid,factory=sid+'#black',(lambda black=black:black)
            texture,cooked_image=scenery_texture(atlas,images,shared_pixels,image_sid,factory,w,h,t['m_Color']['a'],scenery_cap,target)
            draw={'source':sc.sid(i),'sprite':sid,'name':sc.gos[gid]['m_Name'],'texture':texture,'points':points,'scale':scale,'tint':[min(255,round(t['m_Color'][k]*128)) for k in 'rgb'],'z':z,'order':t['m_SortingOrder'],'layer':t['m_SortingLayer']}
            material=scenery_material(s,sc.file,t,cooked_image,atlas.quantized[texture][2])
            if material:draw['material']=material
            draws.append(draw);rendered.append(i)
        except Exception as ex:unsupported.append({'id':sc.sid(i),'type':typ,'error':str(ex)})
    for record in secrets:
        if record['family']!=FAMILY_WALL_TK2D:continue
        try:
            draw=tk2d_wall_draw(s,sc,record,atlas,images,shared_pixels,scenery_cap,
                                None if record['source'] in reachable else (cam_x,cam_y))
            if draw is not None:draws.append(draw)
        except Exception as ex:unsupported.append({'id':record['renderer_source'],'type':'tk2dSprite','error':str(ex)})
    decor_groups=[]
    for record in ([] if region.get('scene_name') in DECOR_DEFERRED else decor_sources(sc)):
        try:
            made=decor_draws(s,sc,record,atlas,images,shared_pixels,scenery_cap,(cam_x,cam_y))
            if made is None:continue
            for frame,draw in enumerate(made.pop('draws')):
                draw['decor']=[len(decor_groups),frame];draws.append(draw)
            decor_groups.append(made)
        except Exception as ex:unsupported.append({'id':record['renderer_source'],'type':'tk2d decor','error':str(ex)})
    tilemap_fill=append_tilemap_fills(sc,atlas,draws,region,cam_x,cam_y,FOCAL,CAM_Z)
    tilemap_summary={k:v for k,v in tilemap_fill.items()if k!='meshes'}
    dump(out/'tilemap-fill.json',tilemap_fill)
    print('scene sprites and source tilemap fills',len(draws),'unique images',len(atlas.images),flush=True)
    # Locate Knight assets by their collection and clip names, not serialized IDs.
    rf=s.file('resources.assets');col_o=None;anim_o=None
    for o in list(rf.objects.values()):
        if o.type.name!='MonoBehaviour':continue
        name=s.typename(o)
        if name=='tk2dSpriteCollectionData':
            t=s.read(o)
            if t['spriteCollectionName']=='Knight':col_o=o;collection=t
        elif name=='tk2dSpriteAnimation':
            t=s.read(o)
            if {'Idle','Run','Slash','Airborne','Focus'}.issubset(c['name'] for c in t['clips']):anim_o=o;animation=t
        if col_o and anim_o:break
    if not col_o or not anim_o:raise ValueError('Knight source collections not found')
    # NailSlash components descend from the Knight through an identity Attacks node.
    hero=next(o for o in rf.objects.values() if o.type.name=='MonoBehaviour' and s.typename(o)=='HeroController')
    hero_gid=hero.parse_monobehaviour_head().m_GameObject.m_PathID
    nails=nail_sources(s,rf,hero_gid)
    frames=[];clips=[];framecache={};textures={}
    focus_clip_base=len(CLIPS)+len(nails)
    # Bench clips ride only in views that contain a bench trigger; their index
    # is constant (BENCH_CLIP_BASE) because actor art is appended afterwards.
    from benches import BENCH_CLIPS
    knight_clip_names=CLIPS+tuple(n['clip'] for n in nails)+FOCUS_CLIPS+(BENCH_CLIPS if benches else ())
    for clip_index,name in enumerate(knight_clip_names):
        nail=nails[clip_index-len(CLIPS)] if len(CLIPS)<=clip_index<focus_clip_base else None
        c=next(c for c in animation['clips'] if c['name']==name);start=len(frames)
        for fr in c['frames']:
            assert s.ref(anim_o.assets_file,fr['spriteCollection']).path_id==col_o.path_id
            index=fr['spriteId']
            cachekey=(index,clip_index if nail else -1)
            if cachekey not in framecache:
                im,box=tk_sprite(s,col_o.assets_file,collection,index,textures);scale=FOCAL/-CAM_Z
                if nail:box=transformed_box(box,nail)
                tex=atlas.add(im,(box[2]-box[0])*scale,(box[3]-box[1])*scale,streamed=True)
                framecache[cachekey]=(tex,box)
                if write_shared:im.save(out/f'knight-{index}.png')
            tex,box=framecache[cachekey];frames.append({'texture':tex,'box':box,'sprite':index,'event':fr})
        clips.append({'name':name,'start':start,'count':len(c['frames']),'fps':c['fps'],'wrap':guest_wrap(c),'loopStart':c.get('loopStart',0)})
    # Into an atlas this pack throws away, for the admission rather than the art.
    # host/regions.py::postpack_actor_bank builds one scene-wide actor bank and
    # appends it to every view of the scene, then replaces this row's clip
    # indices with its own, so art cooked into a base pack here is never read:
    # it is only paid for, and only by the one view that happens to contain an
    # actor's authored transform. Measured on Crossroads_10 view 244, which is
    # where the False Knight is authored: 75,752 bytes of the 393,216-byte room
    # budget, which is what its clip set was competing against. The pass still
    # runs because it owns the refusal that clears `movement_supported` for a
    # frame the animation cache cannot hold, and that verdict has to be the same
    # here as in the postpass.
    append_actor_art(s,sc,actors,Atlas(max_textures=MAX_ROOM_TEXTURES),[],[])
    from effects import append_grass_impact_art,append_door_debris_art
    grass_impact=append_grass_impact_art(s,sc,atlas,frames,clips)
    door_debris=append_door_debris_art(s,sc,atlas,frames)
    from particles import append_particle_art
    particle_effects=append_particle_art(s,sc,atlas,frames)
    # Extract only the verified scalar HeroController prefix; full schema has a
    # separate unresolved 52-byte tail mismatch and is never silently accepted.
    hero=next(o for o in rf.objects.values() if o.type.name=='MonoBehaviour' and s.typename(o)=='HeroController')
    from UnityPy.helpers import TypeTreeHelper
    node=hero._get_typetree_node();children=node.m_Children
    cut=next(i for i,n in enumerate(children) if n.m_Name=='hero_state')
    node.m_Children=children[:cut]
    old=TypeTreeHelper.read_typetree_boost
    try:
        TypeTreeHelper.read_typetree_boost=None
        constants=hero.read_typetree(nodes=node,check_read=False)
    finally:
        TypeTreeHelper.read_typetree_boost=old
        node.m_Children=children
    assert 0<constants['RUN_SPEED']<32 and 0<constants['JUMP_SPEED']<64
    marker=next((gid for gid,g in sc.gos.items() if g['m_Name']=='Death Respawn Marker'),None)
    spawn=region.get('spawn') or (sc.point(marker) if marker else (0,0,0))
    hero_go=s.ref(hero.assets_file,constants['m_GameObject']);body=None
    for component in s.read(hero_go)['m_Component']:
        obj=s.ref(hero_go.assets_file,component['component'])
        if obj.type.name=='BoxCollider2D':body=s.read(obj)
    assert body
    glob=s.file('globalgamemanagers');gravity=s.read(glob.objects[15])['m_Gravity']['y']
    step=s.read(glob.objects[8])['Fixed Timestep'];dt=step['m_Count']*step['m_Rate']['m_Denominator']/step['m_Rate']['m_Numerator']
    # Jump() holds speed for source steps 0..JUMP_STEPS inclusive. Preserve
    # that duration when resampling source 50 Hz steps to the guest's 60 Hz.
    # Set in HeroController::.ctor rather than serialized, so it is not in the
    # typetree prefix: `ldc.i4.2; stfld JUMP_QUEUE_STEPS`. A jump pressed this
    # many fixed steps before landing still fires.
    superdash=source_superdash_values(s)
    JUMP_QUEUE_STEPS=2
    # Also .ctor literals: `ldc.i4.s 10; stfld DOUBLE_JUMP_QUEUE_STEPS`, and
    # DoubleJump()'s `ldc.i4.3; ble` that spends the first steps on the wings
    # before the upward velocity starts.
    DOUBLE_JUMP_QUEUE_STEPS=10
    DOUBLE_JUMP_DELAY_STEPS=3
    # Coyote time and the ceiling lockout, both .ctor literals as well.
    LEDGE_BUFFER_STEPS=2
    HEAD_BUMP_STEPS=3
    # The Shade Cloak reuses the dash's speed and duration outright; only the
    # cooldown and the invulnerability differ, so a divergent install must fail
    # here rather than quietly ship a differently shaped dash.
    assert constants['SHADOW_DASH_SPEED']==constants['DASH_SPEED']
    assert constants['SHADOW_DASH_TIME']==constants['DASH_TIME']
    # The shroom bounce carries no timer, so the guest holds no shroom state
    # beyond the rise. A non-zero source time would need one.
    assert constants['BOUNCE_SHROOM_TIME']==0
    fall_accel=-gravity*constants['DEFAULT_GRAVITY']
    # Unity applies gravity inside the same physics step that Jump() pins the
    # velocity, so the source rises at JUMP_SPEED - g*0.02 = 15.702, not at
    # JUMP_SPEED. The guest subtracts g over its own smaller step, so storing
    # JUMP_SPEED unchanged would hold 15.86 and climb 1% too fast for the whole
    # hold. Pre-compensate the difference in step length instead, which leaves
    # the held velocity, and so the hold's displacement, equal to the source's.
    held=constants['JUMP_SPEED']-fall_accel*dt
    vals={'speed':constants['RUN_SPEED'],'jump':held+fall_accel/60,'gravity':fall_accel,'fall':constants['MAX_FALL_VELOCITY'],'half_width':body['m_Size']['x']/2,'bottom':body['m_Offset']['y']-body['m_Size']['y']/2,'top':body['m_Offset']['y']+body['m_Size']['y']/2}
    params=('pub const PARAMS: hk_sim::Params = hk_sim::Params {'
        +','.join(f'{k}:{round(v*65536)}' for k,v in vals.items())
        +f",hold_ticks:{round((constants['JUMP_STEPS']+1)*dt*60)}"
        +f",min_ticks:{math.ceil(constants['JUMP_STEPS_MIN']*dt*60)}"
        +f",jump_queue_ticks:{round(JUMP_QUEUE_STEPS*dt*60)}"
        +f",dash_speed:{round(constants['DASH_SPEED']*65536)}"
        +f",dash_ticks:{round(constants['DASH_TIME']*60)}"
        +f",dash_cooldown_ticks:{round(constants['DASH_COOLDOWN']*60)}"
        +f",dash_queue_ticks:{round(constants['DASH_QUEUE_STEPS']*dt*60)}"
        +f",shadow_dash_cooldown_ticks:{round(constants['SHADOW_DASH_COOLDOWN']*60)}"
        +f",wallslide_speed:{round(constants['WALLSLIDE_SPEED']*65536)}"
        +f",wall_sticky_ticks:{round(constants['WALL_STICKY_STEPS']*dt*60)}"
        +f",walljump_speed:{round(constants['WJ_KICKOFF_SPEED']*65536)}"
        # The source sheds the kickoff down to RUN_SPEED across the whole lock,
        # so the per-tick share follows the resampled lock length, not the
        # source's step count.
        +f",walljump_decel:{round((constants['WJ_KICKOFF_SPEED']-constants['RUN_SPEED'])*65536/round(constants['WJLOCK_STEPS_LONG']*dt*60))}"
        +f",wall_lock_short:{round(constants['WJLOCK_STEPS_SHORT']*dt*60)}"
        +f",wall_lock_long:{round(constants['WJLOCK_STEPS_LONG']*dt*60)}"
        # Pre-compensated exactly as `jump` is, against the same step mismatch.
        +f",double_jump_speed:{round((constants['JUMP_SPEED']*1.1-fall_accel*dt+fall_accel/60)*65536)}"
        +f",double_jump_delay_ticks:{round(DOUBLE_JUMP_DELAY_STEPS*dt*60)}"
        +f",double_jump_ticks:{round(constants['DOUBLE_JUMP_STEPS']*dt*60)}"
        +f",double_jump_queue_ticks:{round(DOUBLE_JUMP_QUEUE_STEPS*dt*60)}"
        +f",super_dash_speed:{round(superdash['speed']*65536)}"
        +f",super_dash_charge_ticks:{round(superdash['charge']*60)}"
        +f",super_dash_cancel_ticks:{round(superdash['cancelable']*60)}"
        +f",super_dash_recover_ticks:{round(superdash['recover']*60)}"
        +f",ledge_buffer_ticks:{round(LEDGE_BUFFER_STEPS*dt*60)}"
        +f",head_bump_ticks:{round(HEAD_BUMP_STEPS*dt*60)}"
        # A one-shot impulse, so no pre-compensation: the arc a single velocity
        # buys does not depend on the step length the way a held jump does.
        +f",shroom_speed:{round(constants['SHROOM_BOUNCE_VELOCITY']*65536)}"
        +'};\n')
    params+=f'pub const SPAWN:(i32,i32)=({round(spawn[0]*65536)},{round(spawn[1]*65536)});\n'
    params+=f'pub const KNIGHT_SCALE:i32={round(FOCAL/-CAM_Z*4096)};\n'
    # Final renderer indices are assigned after sorting below.
    if write_shared:dump(ROOT/'.hkpsx/gameplay-source.json',{'hero':s.sid(hero),'prefix':constants,'collider':body,'fixed_dt':dt,'development_spawn_marker':f'{scene_file}:{marker}','spawn':spawn,'limitations':['Full HeroController tail schema unresolved','Jump release queue and Unity contact solver parity unverified','No original-input trajectory capture yet']})
    atlas.pack();print('pages',len(atlas.pages),'textures',len(atlas.images),'cluts',atlas.cluts,flush=True)
    # Terrain-only collision. Keep exact world segments; guest handles axis-aligned
    # edges, while unsupported slopes are explicit, never converted into floors.
    edges=[]
    # Object identity goes through `Scene.sid`, never the room's file name with
    # a raw index: a scene that additively merges another carries those objects
    # under shifted ids, so `level46:100154` names nothing while `level48:154`
    # is the object. The cook built edge sources the second way and the flat
    # support pass could not open them.
    # Every arena gate is baked, including the twelve the source has open from
    # the first frame. A gate serializes closed whatever state it loads in,
    # because BG Control's start state is what opens it: `Opened` tests
    # `Start Closed` and sets the collider inactive. Baking only the gates that
    # load closed left the other twelve carrying no terrain at all, so the
    # `BG CLOSE` an arena broadcasts had no edge to restore and the arena never
    # sealed at its ends. `game/src/battle_gates.rs` instead excludes the edges
    # of every gate that is currently open, starting from the ones that are open
    # on load, the way `great_door.rs` excludes the door's once it is broken
    # through; `host/battle_gates.py` is the join that names those edges per
    # cooked region. That inverts the failure mode: a lost binding is a gate
    # that never opens, which is visible, rather than one that never closes.
    # What the exclusion is holding back, if it is ever lost: none of these
    # gates cooks a sprite, so each is an invisible wall, and every one measures
    # 4.16 units tall against a jump apex of 2.87, with Crossroads_04's at
    # 12.66. Crossroads_09 would be impassable between its only two doors and
    # Crossroads_08 cut into three columns.
    for i,(typ,t) in sc.objects.items():
        if typ not in ('EdgeCollider2D','BoxCollider2D','PolygonCollider2D'):continue
        gid=t['m_GameObject']['m_PathID']
        if not sc.active(gid) or not t['m_Enabled'] or t['m_IsTrigger'] or sc.gos[gid]['m_Layer']!=8:continue
        off=t['m_Offset'];paths=[]
        if typ=='BoxCollider2D':
            w=t['m_Size']['x']/2;h=t['m_Size']['y']/2;paths=[[{'x':x,'y':y} for x,y in [(-w,-h),(w,-h),(w,h),(-w,h),(-w,-h)]]]
        elif typ=='EdgeCollider2D':paths=[t['m_Points']]
        else:
            paths=t['m_Points']['m_Paths']
            paths=[p+[p[0]] for p in paths if p]
        for path in paths:
            pts=[sc.point(gid,p['x']+off['x'],p['y']+off['y']) for p in path]
            for a,b in zip(pts,pts[1:]):
                segment,refusal=cooked_edge(a,b,region)
                if refusal=='sloped terrain edge':unsupported.append({'id':sc.sid(i),'type':refusal,'points':[a,b]})
                if segment is None:continue
                edges.append({'source':sc.sid(i),'a':segment[0],'b':segment[1]})
    draws.sort(key=lambda d:(d['layer'],d['order'],-d['z'],d['source']))
    # Each animated object's frames, by their final draw index (the sort is
    # stable and they share every key, so a group's draws are consecutive).
    decor=[dict(g,draws=[i for i,d in enumerate(draws) if d.get('decor',[None])[0]==n]) for n,g in enumerate(decor_groups)]
    breakables=bind_breakables(breakables,draws,edges)
    secrets=bind_secrets(secrets,draws,edges)
    params+=generated_params(constants,dt,nails,grass,draws)
    params+=generated_nail_response_params(constants,dt)
    params+=generated_dream_nail_params(source_dream_nail_values(s))
    params+=generated_spell_params(source_spell_values(s))
    if write_shared:
        vital_values=source_vital_values(s,constants)
        params+=generated_vital_params(vital_values)
        focus_values=source_focus_values(s,constants)
        params+=generated_focus_params(focus_values)
        params+=f'pub const FOCUS_CLIP_BASE: usize = {focus_clip_base};\n'
        dump(ROOT/'.hkpsx/focus-source.json',focus_values)
        dump(ROOT/'.hkpsx/vitals-source.json',vital_values)
        (ROOT/'data/params.rs').write_text(params)
        dump(ROOT/'.hkpsx/combat-source.json',{'hero':s.sid(hero),'nails':nails,'grass':grass,'source_methods':['HeroController.Attack','HeroController.DoAttack','HeroController.CanAttack','HeroAnimationController.Update','NailSlash.StartSlash','NailSlash.FixedUpdate','GrassCut.OnTriggerEnter2D','GrassCut.ShouldCut','GrassBehaviour.Start'],'semantics':{'attack_seconds':constants['ATTACK_DURATION'],'cooldown_seconds':constants['ATTACK_COOLDOWN_TIME'],'alternate_reset_seconds':constants['ALT_ATTACK_RESET'],'polygon_source_fixed_steps':[1,5],'source_fixed_dt':dt,'first_horizontal_body_clip':'SlashAlt','first_horizontal_effect':'SlashEffect'},'limitations':['This attack-extraction report does not describe the separate world, damage, enemy and pogo runtime; arbitrary PlayMaker execution remains unsupported','Grass cut renderer switch only; source cut particles, sound and shader bending omitted','Attack input buffering and animation cancellation are not implemented','FixedUpdate collision phase resampled to 60Hz; original runtime trajectories not compared']})
    dump(out/'scene.json',{'tilemap_fill':tilemap_summary,'draws':draws,'frames':frames,'clips':clips,'grass':grass,'grass_impact':grass_impact,'door_debris':door_debris,'particle_effects':particle_effects,'breakables':breakables,'secrets':secrets,'actors':actors,'hazards':hazards,'benches':benches,'shrooms':shrooms,'decor':decor,'edges':edges,'atlas':atlas.entries,'texture_request_to_canonical':atlas.request_map})
    if write_shared:
        for i,page in enumerate(atlas.pages):(out/f'page-{i}.bin').write_bytes(page)
        (out/'palettes.bin').write_bytes(b''.join(atlas.palettes))
    unsupported_components=[{'id':s.sid(o),'type':s.typename(o)} for o in sc.file.objects.values() if o.type.name not in ('GameObject','Transform','SpriteRenderer','BoxCollider2D','EdgeCollider2D','PolygonCollider2D')]
    dump(out/'unsupported.json',{'region':region,'errors':unsupported,'unsupported_components':unsupported_components,'rendered_sprite_ids':rendered,'tilemap_fill':tilemap_summary,'note':'Opaque black tk2d tilemap mesh geometry is supported; other unsupported component behaviors are explicit; supported breakables and actors are separately listed in region metadata.','component_counts':dict(__import__('collections').Counter(t for t,d in sc.objects.values()))})
    # Fixed endian binary pack: independently checked by host and no_std reader.
    pack=bytearray(b'HKROOM02')
    pack.extend(struct.pack('<6I',len(atlas.pages),len(atlas.entries),len(draws),len(frames),len(clips),len(edges)))
    pack.extend(struct.pack('<2I',len(atlas.stream),atlas.flags))
    for e in atlas.entries:pack.extend(struct.pack('<6HI',*e))
    for d in draws:
        pack.extend(struct.pack('<HHI8i4B',d['texture'],int(d['z']<0),round(d['scale']*4096),*[round(p[k]*d['scale']*256) for p in d['points'] for k in [0,1]],*d['tint'],d.get('material',{}).get('mode',0)))
    for fr in frames:pack.extend(struct.pack('<I4i',fr['texture'],*[round(v*65536) for v in fr['box']]))
    for c in clips:pack.extend(struct.pack('<4I',c['start'],c['count'],round(c['fps']*65536),c['wrap']|(c['loopStart']<<16)))
    for e in edges:pack.extend(struct.pack('<4i',*[round(v*65536) for p in [e['a'],e['b']] for v in p]))
    pack.extend(b''.join(atlas.palettes));pack.extend(b''.join(atlas.pages));pack.extend(atlas.stream);(out/'room.hk').write_bytes(pack)
    if len(edges)>1024 or len(draws)>1024 or len(pack)-len(atlas.pages)*32768>ROOM_BYTE_BUDGET:raise ValueError('region RAM/draw/edge budget exceeded')
    report={'tilemap_fill':tilemap_summary,'source':str(s.directory),'scene':region.get('scene_name','Tutorial_01'),'scene_file':scene_file,'knight_collection':s.sid(col_o),'knight_animation':s.sid(anim_o),'pack_sha256':hashlib.sha256(pack).hexdigest(),'pack_bytes':len(pack),'draws':len(draws),'textures':len(atlas.entries),'cluts':atlas.cluts,'pages':len(atlas.pages),'edges':len(edges),'format':'HKROOM02','scenery_max_axis':scenery_cap,'static_page_budget':STATIC_PAGE_BUDGET,'texture_request_to_canonical':atlas.request_map,'stream_bytes':len(atlas.stream),'animation_bytes':atlas.animation_bytes,'alpha_cover_bytes':atlas.alpha_cover_bytes,'format_features':atlas.flags,'stream_textures':len(atlas.streamed),'static_texture_bytes':len(atlas.pages)*32768,'palette_bytes':len(atlas.palettes)*32,'focus_clips':{name:focus_clip_base+i for i,name in enumerate(FOCUS_CLIPS)},'focus_parameter_source':'.hkpsx/focus-source.json','animation_storage':'resident RAM bank with bounded VRAM frame cache','grass':grass,'grass_impact':grass_impact,'door_debris':door_debris,'particle_effects':particle_effects,'breakables':breakables,'secrets':secrets,'actors':actors,'hazards':hazards,'benches':benches,'shrooms':shrooms,'bench_clip_base':focus_clip_base+len(FOCUS_CLIPS),'decor':decor,'edge_sources':[e['source'] for e in edges]}
    if not write_shared:
        print('cooked',len(pack),'bytes',len(edges),'edges',flush=True)
        return report
    inputs={}
    # Only files actually loaded plus streaming texture resources and assemblies.
    files=set(s.directory/p for p in s.files)|set((s.directory/'Managed').glob('*.dll'))
    for key,f in s.env.files.items():
        p=Path(key)
        if p.is_file() and p.is_relative_to(s.directory):files.add(p)
    for f in list(files):
        for ext in ['.resS','.resource']:
            if Path(str(f)+ext).is_file():files.add(Path(str(f)+ext))
    for f in sorted(files):
        if f.is_file():inputs[str(f.relative_to(s.directory))]={'bytes':f.stat().st_size,'sha256':hashlib.file_digest(f.open('rb'),'sha256').hexdigest()}
    report['inputs']=inputs;dump(ROOT/'.hkpsx/provenance.json',report)
    print('cooked',len(pack),'bytes',len(edges),'edges',flush=True)
    return report
if __name__=='__main__':cook()
