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
from quality import SCENERY_MAX_AXIS, STATIC_PAGE_BUDGET, ROOM_BYTE_BUDGET, TEXTURE_BUDGET
from focus import FOCUS_CLIPS,source_focus_values,generated_focus_params
from actors import actor_sources,hazard_sources,source_vital_values,generated_vital_params,generated_nail_response_params
from UnityPy.helpers.MeshHelper import MeshHandler

# Camera limits for this isolated development chamber. Source locks are reported.
CAM_X=(30.0,58.0);CAM_Y=(14.11,20.0)
FOCAL=120/math.tan(math.radians(12));CAM_Z=-38.1
CLIPS=('Idle','Run','Airborne','Fall','Land','Slash','SlashAlt','UpSlash','DownSlash')

def scenery_dimensions(width,height,cap):
    """One stable integer sampling size; floating roundoff cannot exceed cap."""
    if not isinstance(cap,int) or not 1<=cap<=252 or not all(math.isfinite(v) and v>0 for v in (width,height)):
        raise ValueError('Invalid scenery dimensions/cap')
    reduction=min(1.0,cap/max(width,height))
    return tuple(max(1,min(cap,math.ceil(v*reduction))) for v in (width,height))

def scenery_texture(atlas,images,shared_pixels,sid,image_factory,width,height,alpha,cap):
    target=scenery_dimensions(width,height,cap)
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

def unsupported_sprite_behavior(name,parent_name,component_types):
    # This named scene helper is driven by a PlayMaker fade/cover state machine.
    # Its ordinary Sprites/Lit material is also used by valid room scenery, so
    # material/shader identity alone must never be used as an exclusion rule.
    if name=='Inverse Remasker' and parent_name=='mask_container' and 'PlayMakerFSM' in component_types:
        return 'FSM-controlled inverse remasker'
    return None

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
        if control['kind']=='ZombieSwipeWalker':
            bindings={'walk':'Walk','turn':'Turn','idle':'Idle','anticipate':'Attack Anticipate',
                      'lunge':'Attack Lunge','cooldown':'Attack Cooldown','fall':'Fall'}
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
        else:
            raise ValueError('unsupported actor art controller: '+control['kind'])
        # Actor sampling is never reduced: a frame beyond the 64x64 animation
        # cache makes the actor an explicit unsupported record for this cook.
        def frame_dims(name):
            clip=next(c for c in library['clips'] if c['name']==name)
            for frame in clip['frames']:
                collection_o=s.ref(library_o.assets_file,frame['spriteCollection']);sid=s.sid(collection_o)
                if sid not in collections:collections[sid]=s.read(collection_o)
                _,box=tk_sprite(s,collection_o.assets_file,collections[sid],frame['spriteId'],textures);scale=FOCAL/-CAM_Z
                yield math.ceil((box[2]-box[0])*sx*scale),math.ceil((box[3]-box[1])*sy*scale)
        oversized=sorted((w,h) for name in bindings.values() for w,h in frame_dims(name) if w>64 or h>64)
        if oversized:
            actor['movement_supported']=False
            actor.setdefault('limitations',[]).append(f'actor frame {oversized[-1][0]}x{oversized[-1][1]} exceeds the 64x64 animation cache; art not cooked')
            continue
        for kind,name in bindings.items():
            clip_key=(s.sid(library_o),name,sx,sy,alpha)
            if clip_key not in clip_cache:
                clip=next(c for c in library['clips'] if c['name']==name);start=len(frames)
                for frame in clip['frames']:
                    collection_o=s.ref(library_o.assets_file,frame['spriteCollection']);sid=s.sid(collection_o)
                    if sid not in collections:collections[sid]=s.read(collection_o)
                    index=frame['spriteId'];key=(sid,index,sx,sy,alpha)
                    if key not in cache:
                        image,box=tk_sprite(s,collection_o.assets_file,collections[sid],index,textures)
                        image.putalpha(image.getchannel('A').point(lambda a:round(a*color['a'])))
                        box=(box[0]*sx,box[1]*sy,box[2]*sx,box[3]*sy)
                        scale=FOCAL/-CAM_Z
                        texture=atlas.add(image,(box[2]-box[0])*scale,(box[3]-box[1])*scale,streamed=True)
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

from alpha_covers import HAS_ALPHA_COVERS, RECORD_BYTES, record as alpha_cover_record

class Atlas:
    def __init__(self,deduplicate=True,max_pages=20,max_textures=640,alpha_covers=True):
        self.flags=HAS_ALPHA_COVERS if alpha_covers else 0
        self.animation_bytes=0;self.alpha_cover_bytes=0
        self.max_textures=max_textures
        self.max_pages=max_pages
        self.images=[];self.entries=[];self.pages=[];self.palettes=[];self.streamed=set();self.stream=bytearray()
        self.quantized=[];self.canonical={};self.request_map=[];self.deduplicate=deduplicate
    def add_quantized(self,w,h,palette,pixels,streamed=False):
        """Admit exact final texels; different palettes/storage classes never alias."""
        if not 1<=w<=256 or not 1<=h<=256 or len(palette)!=32 or len(pixels)!=((w+1)//2)*h:
            raise ValueError('invalid canonical texture dimensions/palette/texels')
        if streamed and (w>64 or h>64):raise ValueError(f'animation cache dimensions exceed 64x64: {w}x{h}')
        key=(bool(streamed),w,h,bytes(palette),bytes(pixels))
        index=self.canonical.get(key) if self.deduplicate else None
        if index is None:
            index=len(self.images);self.images.append(None);self.quantized.append((w,h,key[3],key[4]))
            if streamed:self.streamed.add(index)
            self.canonical[key]=index
        self.request_map.append(index)
        return index
    def add(self,im,w,h,streamed=False):
        w=max(1,min(252,math.ceil(w)));h=max(1,min(252,math.ceil(h)))
        if streamed and (w>64 or h>64):raise ValueError(f'animation cache dimensions exceed 64x64: {w}x{h}')
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
        stride=(w+1)//2;pixels=bytearray(stride*h)
        for yy in range(h):
            for xx in range(w):
                v=px[yy*w+xx]+1 if alpha[yy*w+xx]>=16 else 0
                pixels[yy*stride+xx//2]|=v<<((xx&1)*4)
        index=self.add_quantized(w,h,struct.pack('<16H',*colors),pixels,streamed)
        self.images[index]=im
        return index
    def pack(self):
        self.entries=[None]*len(self.quantized);self.palettes=[];self.stream=bytearray()
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
                self.stream.extend(bytes(stride*h));p,x,y=65535,0,0
            else:
                p,x,y=positions[i]
            self.palettes.append(palette);cl=len(self.palettes)-1
            if cl>=self.max_textures:raise ValueError('CLUT budget exceeded')
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
    cam_x,cam_y=region['camera_x'],region['camera_y'];collision_bounds=region['collision_bounds']
    atlas=Atlas(max_pages=20 if stage_for_similarity else STATIC_PAGE_BUDGET,max_textures=640 if stage_for_similarity else TEXTURE_BUDGET);draws=[];images={};unsupported=list(sc.errors);rendered=[]
    reveal=reveal_mask_sources(sc)
    supported_reveal_renderers={int(source.split(':')[-1]) for controller in reveal['controllers'] for source in controller['renderer_sources']}
    unsupported.extend(dict(record,type='unsupported reveal controller') for record in reveal['unsupported'])
    shared_pixels=shared_pixels if shared_pixels is not None else {}
    shared_geometry=shared_geometry if shared_geometry is not None else {}
    grass=grass_sources(sc,region.get('grass_bounds'),errors=unsupported);cut_renderers={g[k] for g in grass for k in ('off','on')}
    breakables=breakable_sources(sc,region.get('interaction_bounds',region['activation_bounds']),errors=unsupported)
    actors=actor_sources(sc,region.get('interaction_bounds',region['activation_bounds']))
    hazards=hazard_sources(sc,region.get('interaction_bounds',region['activation_bounds']))
    forced_renderers=cut_renderers|{i for b in breakables for key in ('off_renderer_ids','on_renderer_ids') for i in b[key]}
    forced_renderers|={int(sid.split(':')[-1]) for b in breakables for fade in b.get('mask_fades',[]) for sid in fade['renderer_sources']}
    for i,(typ,t) in sc.objects.items():
        if typ!='SpriteRenderer':continue
        gid=t['m_GameObject']['m_PathID']
        if ((not t['m_Enabled'] or not sc.active(gid)) and i not in forced_renderers) or not t['m_Sprite']['m_PathID']:continue
        omission=unsupported_remasker(s,sc,gid) if sc.gos[gid]['m_Name']=='Inverse Remasker' and i not in supported_reveal_renderers else None
        if omission:
            unsupported.append(omission);continue
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
            bound=packet_bound([(round(p[0]*scale*256),round(p[1]*scale*256)) for p in points],*scenery_dimensions(w,h,SCENERY_MAX_AXIS))
            if bound['packets']>CHILD_CAPACITY:raise ValueError(f'draw needs {bound["packets"]} children > {CHILD_CAPACITY}')
            # Preserve individual instances, original ordering and depth parallax.
            texture,cooked_image=scenery_texture(atlas,images,shared_pixels,sid,lambda:sp.image,w,h,t['m_Color']['a'],SCENERY_MAX_AXIS)
            draw={'source':f'{scene_file}:{i}','sprite':sid,'name':sc.gos[gid]['m_Name'],'texture':texture,'points':points,'scale':scale,'tint':[min(255,round(t['m_Color'][k]*128)) for k in 'rgb'],'z':z,'order':t['m_SortingOrder'],'layer':t['m_SortingLayer']}
            material=scenery_material(s,sc.file,t,cooked_image,atlas.quantized[texture][2])
            if material:draw['material']=material
            draws.append(draw);rendered.append(i)
        except Exception as ex:unsupported.append({'id':f'{scene_file}:{i}','type':typ,'error':str(ex)})
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
    for clip_index,name in enumerate(CLIPS+tuple(n['clip'] for n in nails)+FOCUS_CLIPS):
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
    append_actor_art(s,sc,actors,atlas,frames,clips)
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
    vals={'speed':constants['RUN_SPEED'],'jump':constants['JUMP_SPEED'],'gravity':-gravity*constants['DEFAULT_GRAVITY'],'fall':constants['MAX_FALL_VELOCITY'],'half_width':body['m_Size']['x']/2,'bottom':body['m_Offset']['y']-body['m_Size']['y']/2,'top':body['m_Offset']['y']+body['m_Size']['y']/2}
    params='pub const PARAMS: hk_sim::Params = hk_sim::Params {'+','.join(f'{k}:{round(v*65536)}' for k,v in vals.items())+f",hold_ticks:{round((constants['JUMP_STEPS']+1)*dt*60)},min_ticks:{math.ceil(constants['JUMP_STEPS_MIN']*dt*60)}"+'};\n'
    params+=f'pub const SPAWN:(i32,i32)=({round(spawn[0]*65536)},{round(spawn[1]*65536)});\n'
    params+=f'pub const KNIGHT_SCALE:i32={round(FOCAL/-CAM_Z*4096)};\n'
    # Final renderer indices are assigned after sorting below.
    if write_shared:dump(ROOT/'.hkpsx/gameplay-source.json',{'hero':s.sid(hero),'prefix':constants,'collider':body,'fixed_dt':dt,'development_spawn_marker':f'{scene_file}:{marker}','spawn':spawn,'limitations':['Full HeroController tail schema unresolved','Jump release queue and Unity contact solver parity unverified','No original-input trajectory capture yet']})
    atlas.pack();print('pages',len(atlas.pages),'textures',len(atlas.images),flush=True)
    # Terrain-only collision. Keep exact world segments; guest handles axis-aligned
    # edges, while unsupported slopes are explicit, never converted into floors.
    edges=[]
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
                if max(a[0],b[0])<collision_bounds[0] or min(a[0],b[0])>collision_bounds[2] or max(a[1],b[1])<collision_bounds[1] or min(a[1],b[1])>collision_bounds[3]:continue
                if abs(a[0]-b[0])>0.005 and abs(a[1]-b[1])>0.005:
                    if not region.get('allow_slopes'):
                        unsupported.append({'id':f'{scene_file}:{i}','type':'sloped terrain edge','points':[a,b]});continue
                a=list(a);b=list(b)
                if abs(a[0]-b[0])<=0.005:b[0]=a[0]
                elif abs(a[1]-b[1])<=0.005:b[1]=a[1]
                edges.append({'source':f'{scene_file}:{i}','a':a[:2],'b':b[:2]})
    draws.sort(key=lambda d:(d['layer'],d['order'],-d['z'],d['source']))
    breakables=bind_breakables(breakables,draws,edges)
    params+=generated_params(constants,dt,nails,grass,draws)
    params+=generated_nail_response_params(constants,dt)
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
    dump(out/'scene.json',{'tilemap_fill':tilemap_summary,'draws':draws,'frames':frames,'clips':clips,'grass':grass,'grass_impact':grass_impact,'door_debris':door_debris,'particle_effects':particle_effects,'breakables':breakables,'actors':actors,'hazards':hazards,'edges':edges,'atlas':atlas.entries,'texture_request_to_canonical':atlas.request_map})
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
    if len(edges)>1024 or len(draws)>1024 or len(pack)>ROOM_BYTE_BUDGET:raise ValueError('region RAM/draw/edge budget exceeded')
    report={'tilemap_fill':tilemap_summary,'source':str(s.directory),'scene':region.get('scene_name','Tutorial_01'),'scene_file':scene_file,'knight_collection':s.sid(col_o),'knight_animation':s.sid(anim_o),'pack_sha256':hashlib.sha256(pack).hexdigest(),'pack_bytes':len(pack),'draws':len(draws),'textures':len(atlas.entries),'pages':len(atlas.pages),'edges':len(edges),'format':'HKROOM02','scenery_max_axis':SCENERY_MAX_AXIS,'static_page_budget':STATIC_PAGE_BUDGET,'texture_request_to_canonical':atlas.request_map,'stream_bytes':len(atlas.stream),'animation_bytes':atlas.animation_bytes,'alpha_cover_bytes':atlas.alpha_cover_bytes,'format_features':atlas.flags,'stream_textures':len(atlas.streamed),'static_texture_bytes':len(atlas.pages)*32768,'palette_bytes':len(atlas.palettes)*32,'focus_clips':{name:focus_clip_base+i for i,name in enumerate(FOCUS_CLIPS)},'focus_parameter_source':'.hkpsx/focus-source.json','animation_storage':'resident RAM bank with bounded VRAM frame cache','grass':grass,'grass_impact':grass_impact,'door_debris':door_debris,'particle_effects':particle_effects,'breakables':breakables,'actors':actors,'hazards':hazards,'edge_sources':[e['source'] for e in edges]}
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
