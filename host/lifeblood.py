"""Original Tutorial cocoon and two Health Scuttlers, independent resident art.

No room is recooked. Source art is sampled at the existing camera scale;
texture splitting preserves pixels and geometry. Temporary-health rules and
Scuttler timing are verified against the local Windows CIL/FSM.
"""
import hashlib,json,math
from pathlib import Path
from PIL import Image
from source import Source,ROOT,dump
from scene import Scene
from cook import tk_sprite,FOCAL,CAM_Z
from geo import place_rectangles,q16,rust_array,compact_fields
from breakables import _components,_descendants,collider_polygons
from materials import quantize_alpha_coverage
LIFE_RECTS=((352,96,32,80),)
BUDGET=5120


def dense_positions(items,rectangles):
    """Fixed-seed bounded MaxRects retries; never drop or shrink an item."""
    import random
    rng=random.Random(12337)
    for attempt in range(2048):
        if attempt<3:
            key=[lambda a:-a[1]*a[2],lambda a:(-a[2],-a[1]),lambda a:(-a[1],-a[2])][attempt]
            order=sorted(items,key=key)
        else:
            order=sorted(items,key=lambda a:-(a[1]*a[2])*(.25+rng.random()))
        free=list(rectangles);placed={}
        for name,w,h,alignment in order:
            candidates=[]
            for x,y,fw,fh in free:
                ax=(x+alignment-1)//alignment*alignment
                if ax+w>x+fw or h>fh:continue
                dw=x+fw-ax-w;dh=fh-h
                score=(min(dw,dh),max(dw,dh))if attempt%2 else(fw*fh-w*h,min(dw,dh))
                candidates.append((*score,y,ax))
            if not candidates:break
            *_,y,x=min(candidates);placed[name]=(x,y,w,h);new=[]
            for rx,ry,rw,rh in free:
                if x>=rx+rw or x+w<=rx or y>=ry+rh or y+h<=ry:new.append((rx,ry,rw,rh));continue
                if x>rx:new.append((rx,ry,x-rx,rh))
                if x+w<rx+rw:new.append((x+w,ry,rx+rw-x-w,rh))
                if y>ry:new.append((rx,ry,rw,y-ry))
                if y+h<ry+rh:new.append((rx,y+h,rw,ry+rh-y-h))
            free=[r for r in sorted(set(new))if not any(r!=o and o[0]<=r[0] and o[1]<=r[1] and o[0]+o[2]>=r[0]+r[2] and o[1]+o[3]>=r[1]+r[3]for o in new)]
        if len(placed)==len(items):return placed
    raise ValueError('complete source Lifeblood art did not fit after bounded packing')

def pack_art(images):
    """Joint palette; lossless strip splitting to fit the reserved small atlas."""
    if len(images)>18:raise ValueError('Lifeblood art inventory grew')
    # Explicit source-sheet placements, each Bayer phase remains texel-local.
    sheet=Image.new('RGBA',(256,160));origins=[];cursor=0
    for i,im in enumerate(images):
        if i==0:origin=(0,0)
        else:origin=(64+(cursor%6)*32,(cursor//6)*40);cursor+=1
        # Larger static splat gets its own bottom row rather than overlapping.
        if im.width>32 and i!=0:origin=(0,96)
        x,y=origin
        if x+im.width>256 or y+im.height>160:raise ValueError('source art sheet extent')
        origins.append(origin);sheet.paste(im,origin)
    sw,sh,palette,packed=quantize_alpha_coverage(sheet,128)
    parts=[];frames=[]
    for image_index,(im,(x0,y0))in enumerate(zip(images,origins)):
        frame=[]
        strip=im.height if image_index==len(images)-1 else 16
        for y0local in range(0,im.height,strip):
            h=min(strip,im.height-y0local);w=im.width;stride=(w+3)//4*2;payload=bytearray(stride*h)
            for y in range(h):
                for x in range(w):
                    b=packed[(y+y0+y0local)*(sw//2)+(x+x0)//2];n=(b>>(((x+x0)&1)*4))&15
                    payload[y*stride+x//2]|=n<<((x&1)*4)
            frame.append(len(parts));parts.append({'width':w,'height':h,'y':y0local,'pixels':bytes(payload)})
        frames.append(frame)
    # Different frames may refer to precisely identical encoded texel planes.
    unique=[];ids={};mapping=[]
    for p in parts:
        key=(p['width'],p['height'],p['pixels'])
        if key not in ids:ids[key]=len(unique);unique.append(p)
        mapping.append(ids[key])
    items=[('clut',16,1,16)]+[(i,(p['width']+3)//4,p['height'],1)for i,p in enumerate(unique)]
    positions=dense_positions(items,LIFE_RECTS);blob=bytearray();uploads=[];textures=[]
    def emit(key,payload):
        x,y,w,h=positions[key];uploads.append(dict(x=x,y=y,w=w,h=h,offset=len(blob)));blob.extend(payload)
    emit('clut',palette);cx,cy,_,_=positions['clut']
    for i,p in enumerate(unique):
        emit(i,p['pixels']);x,y,w,h=positions[i]
        if (x-320)*4+p['width']>256 or y+p['height']>256:raise ValueError('Lifeblood UV wrap')
        textures.append(dict(u=(x-320)*4,v=y,w=p['width'],h=p['height'],clut=(cy<<6)|(cx//16),tpage=5))
    if len(blob)>BUDGET:raise ValueError('Lifeblood bank budget')
    return bytes(blob),uploads,textures,parts,frames,mapping,sheet


def source_contract(source):
    import dnfile
    from dncil.cil.body.reader import read_method_body_from_bytes
    from actors import _literal
    path=source.directory/'Managed/Assembly-CSharp.dll';pe=dnfile.dnPE(str(path));found={}
    expected={('ScuttlerControl','.ctor'):(.3,.25),('ScuttlerControl','Start'):(1.35,1.5,6.,9.),
              ('<Heal>d__48','MoveNext'):(1.2,),('<Bounce>d__47','MoveNext'):(5.,.5),
              ('PlayerData','TakeHealth'):(),('PlayerData','UpdateBlueHealth'):()}
    for typ in pe.net.mdtables.TypeDef.rows:
        for ref in typ.MethodList:
            method=ref.row;key=(str(typ.TypeName),str(method.Name))
            if key not in expected:continue
            body=read_method_body_from_bytes(pe.get_data(method.Rva,100000));lit=[v for i in body.instructions if isinstance(v:=_literal(i),(int,float))]
            if not all(any(abs(v-x)<1e-5 for x in lit)for v in expected[key]):raise ValueError('changed Lifeblood CIL literals: '+str(key))
            found['.'.join(key)]={'sha256':hashlib.sha256(pe.get_data(method.Rva,body.size)).hexdigest(),'literals':expected[key]}
    if len(found)!=len(expected):raise ValueError('missing Lifeblood CIL method')
    f=source.file('resources.assets');fsm=source.read(f.objects[20959])['fsm'];state=next(st for st in fsm['states']if st['name']=='Add Blue Health');d=state['actionData']
    adds=[compact_fields(d,i)for i,n in enumerate(d['actionNames'])if n.endswith('.IntAdd')]
    if not any(a['intVariable']['name']=='Blue HP'and a['add']=={'value':1,'useVariable':False,'name':''}for a in adds):raise ValueError('blue health grant is no longer one')
    return found


def cook():
    source=Source();cil=source_contract(source);sc=Scene(source,'level6');f=sc.file
    comps={k:(i,t)for i,k,t in _components(sc,973)};cocoon=comps['HealthCocoon'][1]
    if comps['HealthCocoon'][0]!=12337 or cocoon['disableColliders']!=[{'m_FileID':0,'m_PathID':8330},{'m_FileID':0,'m_PathID':8165}]:raise ValueError('changed cocoon identity/colliders')
    fling=[v for v in cocoon['flingPrefabs']if source.sid(source.ref(f,v['prefab']))=='sharedassets6.assets:436']
    if len(fling)!=1 or fling[0]['minAmount']!=2 or fling[0]['maxAmount']!=2:raise ValueError('expected two Health Scuttlers')
    fling=fling[0];prefab=source.file('sharedassets6.assets');bug={source.typename(o):(o,t)for o,t in __import__('geo')._components_source(source,prefab,436)}
    control=bug['ScuttlerControl'][1]
    if not control['healthScuttler'] or control['startIdle'] or control['startRunning']:raise ValueError('changed Scuttler initial behavior')
    body=source.read(prefab.objects[869]);rigid=bug['Rigidbody2D'][1]
    if body['m_Offset']['x']!=0 or body['m_IsTrigger']:raise ValueError('asymmetric Scuttler body')
    gravity_obj=next(o for o in source.file('globalgamemanagers').objects.values()if o.type.name=='Physics2DSettings')
    gravity=-source.read(gravity_obj)['m_Gravity']['y']*rigid['m_GravityScale']
    images=[];boxes=[];origins=[];art_sources=[];clips=[];textures={}
    def sprite(file,collection,index,scale=1.,hud=False,tint=None):
        obj=source.ref(file,collection);im,box=tk_sprite(source,obj.assets_file,source.read(obj),index,textures)
        box=[v*scale for v in box]
        if tint is not None:
            channels=im.split();im=Image.merge('RGBA',tuple(ch.point(lambda value,factor=tint[k]:round(value*factor))for ch,k in zip(channels,'rgba')))
        dims=tuple(max(1,math.ceil((box[i+2]-box[i])*FOCAL/-CAM_Z))for i in(0,1))
        if hud:
            factor=min(14/im.width,14/im.height);dims=(max(1,round(im.width*factor)),max(1,round(im.height*factor)))
        images.append(im.resize(dims,Image.Resampling.LANCZOS));boxes.append(box);art_sources.append(f'{source.sid(obj)}:{index}');return len(images)-1
    owner=comps['tk2dSprite'][1];intact=sprite(f,owner['collection'],owner['_spriteId'])
    libobj=source.ref(prefab,bug['tk2dSpriteAnimator'][1]['library']);lib=source.read(libobj)
    for name in ['Spawn','Scuttler Land','Scuttler Run']:
        clip=next(c for c in lib['clips']if c['name']==name);start=len(images)
        for fr in clip['frames']:sprite(libobj.assets_file,fr['spriteCollection'],fr['spriteId'],1.5)
        if clip['fps']!=12.:raise ValueError('changed Scuttler animation FPS')
        clips.append(dict(name=name,start=start,count=len(clip['frames']),fps=12,wrap=clip['wrapMode']))
    splat_obj=next((i,t)for i,k,t in _components(sc,3645)if k=='tk2dSprite')
    splat=sprite(f,splat_obj[1]['collection'],splat_obj[1]['_spriteId'],tint=splat_obj[1]['_color']);splat_pos=sc.point(3645)[:2]
    matrix=sc.world(sc.go_transform[3645])
    if matrix[0][1]!=0 or matrix[1][0]!=0:raise ValueError('rotated source cocoon splat unsupported')
    boxes[splat]=[v*matrix[i%2][i%2] for i,v in enumerate(boxes[splat])]
    resources=source.file('resources.assets');hudlib=source.read(resources.objects[20665]);blue=next(c for c in hudlib['clips']if c['name']=='Blue Idle')['frames'][0]
    hud=sprite(resources,blue['spriteCollection'],blue['spriteId'],hud=True)
    blob,uploads,tex,parts,frames,mapping,sheet=pack_art(images)
    poly=collider_polygons(sc,973,'BoxCollider2D',source.read(f.objects[8330]))[0]
    bounds=[min(p[0]for p in poly),min(p[1]for p in poly),max(p[0]for p in poly),max(p[1]for p in poly)]
    body_box=[body['m_Offset'][k]+sgn*body['m_Size'][k]/2 for sgn in(-1,1)for k in'xy']
    metadata=json.loads((ROOT/'data/regions.json').read_text());bindings=[]
    off_ids={f'level6:{i}'for gid in _descendants(sc,971)|{973}for i,k,t in _components(sc,gid)if k in('MeshRenderer','tk2dSprite','SpriteRenderer')}
    for r in metadata['regions']:
        scene=json.loads((ROOT/r.get('base_path',f'data/regions/region-{r["chunk_id"]:03}/room.hk')).with_name('scene.json').read_text())
        off=[i for i,d in enumerate(scene['draws'])if d['source']in off_ids]
        edges=[i for i,e in enumerate(r['edge_sources'])if(e if isinstance(e,str)else e.get('source'))in('level6:8165','level6:8330')]
        # The bound is `world::SCRIPT_EDGE_SLOTS`, read rather than repeated: it
        # was 8 here and moved to 12 when the arena gates needed room, and a
        # second copy of a number that moves is how a generator goes on
        # refusing work the guest would have taken.
        from battle_gates import edge_scratch_slots
        if len(edges)>edge_scratch_slots():raise ValueError('Lifeblood edge binding capacity')
        bindings.append(dict(off=off,edges=edges))
    spec=dict(scene=0,origin=sc.point(973)[:2],bounds=bounds,fling_speed=[fling['minSpeed'],fling['maxSpeed']],fling_angle=[fling['minAngle'],fling['maxAngle']],spread=[fling['originVariation'][k]for k in'xy'],scale=[1.35,1.5],speed=[6.,9.],body=body_box,gravity=gravity,acceleration=.3,activate_ticks=15,heal_ticks=72,land_ticks=15,bounce_ticks=30)
    rust=['// Generated source Lifeblood art and behavior; no embedded retail source dump.','pub const LIFE_SPEC:Spec=Spec{']
    for k,v in spec.items():
        if isinstance(v,(list,tuple)):val=rust_array([q16(x)for x in v])
        elif k in ('scene','activate_ticks','heal_ticks','land_ticks','bounce_ticks'):val=str(v)
        else:val=str(q16(v))
        rust.append(k+':'+val+',')
    rust.append('};\npub const LIFE_CLIPS:[Clip;3]=['+','.join('Clip{'+','.join(f'{k}:{c[k]}'for k in('start','count','fps','wrap'))+'}'for c in clips)+'];')
    rust.append('pub const LIFE_FRAMES:&[Frame]=&[')
    for box,im,refs in zip(boxes,images,frames):
        vals=[]
        for ref in refs:
            p=parts[ref];t=tex[mapping[ref]];b=[box[0],box[3]-(box[3]-box[1])*(p['y']+p['height'])/im.height,box[2],box[3]-(box[3]-box[1])*p['y']/im.height]
            vals.append('Art{'+','.join(f'{k}:{t[k]}'for k in('u','v','w','h','clut','tpage'))+',bounds:'+rust_array([q16(x)for x in b])+'}')
        rust.append('Frame{parts:&['+','.join(vals)+']},')
    rust.append('];\npub const LIFE_UPLOADS:&[Upload]=&['+','.join('Upload{'+','.join(f'{k}:{v}'for k,v in u.items())+'}'for u in uploads)+'];')
    # One entry per catalogue region cost 16 linked bytes per view to carry
    # nothing: eleven of 723 regions see the cocoon. The sorted sparse list the
    # Great Door bindings already use grows with the cocoons, not the world.
    bound=[(i,b)for i,b in enumerate(bindings)if b['off']or b['edges']]
    if bound and bound[-1][0]>0xFFFF:raise ValueError('Lifeblood binding region index exceeds u16')
    rust.append('/// Regions that see the cocoon, sorted by catalogue region index.')
    rust.append('pub const LIFE_BINDINGS:&[(u16,Binding)]=&['+','.join(f'({i},Binding{{off:&'+rust_array(b['off'])+',edges:&'+rust_array(b['edges'])+'})'for i,b in bound)+'];')
    rust.extend([f'pub const INTACT:usize={intact};',f'pub const SPLAT:usize={splat};',f'pub const BLUE_HUD:usize={hud};','pub const SPLAT_ORIGIN:[i32;2]='+rust_array([q16(x)for x in splat_pos])+';'])
    text='\n'.join(rust)+'\n';(ROOT/'data/lifeblood.rs').write_text(text);(ROOT/'data/lifeblood.hk').write_bytes(blob)
    report={'bytes':len(blob),'vram_limit':BUDGET,'rectangles':LIFE_RECTS,'spec':spec,'clips':clips,'source_art':art_sources,'image_dimensions':[im.size for im in images], 'textures':tex,'uploads':uploads,'bindings':bindings,'cocoon_source':'level6:12337','scuttler_source':'sharedassets6.assets:1353','hud_fsm':'resources.assets:20959','cil':cil,
       'limitations':['Source art sampled at current camera projection; sourceRGB shares15-color palette, alpha ordered0/half/full coverage.', 'Scuttler collision uses existing60Hz swept-box physics instead of Unity50Hz Box2D; floor ObjectBounce and ray lookahead are not exact.', 'Cocoon idle and Blue Idle HUD use their original first pose; sweat, HUD appear/break animations and charm interactions are not implemented.', 'Cocoon cap, burst shells/blood, Scuttler death splat/particles and dedicated audio not yet resident.', 'Source cyan cocoon splat preserves worldscale2x3 but samples its source image at33x21 within the5KiB bank.'],
       'payload_sha256':hashlib.sha256(blob).hexdigest(),'rust_sha256':hashlib.sha256(text.encode()).hexdigest(),'region_metadata_sha256':hashlib.sha256((ROOT/'data/regions.json').read_bytes()).hexdigest(),
       'source_sha256':{name:hashlib.sha256((source.directory/name).read_bytes()).hexdigest()for name in list(source.files)+['Managed/Assembly-CSharp.dll']},
       'tool_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    (ROOT/'.hkpsx/lifeblood').mkdir(parents=True,exist_ok=True)
    dump(ROOT/'.hkpsx/lifeblood-provenance.json',report);sheet.save(ROOT/'.hkpsx/lifeblood/art.png')
    print(f'Lifeblood: {len(images)} original frames, {len(tex)} resident parts, {len(blob)}/{BUDGET} VRAM bytes')
    return report
if __name__=='__main__':cook()
