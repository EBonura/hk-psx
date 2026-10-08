"""Source-backed Geo rock metadata and a bounded, startup-only 4bpp art bank.

No room pack is modified. Retail sprites/reports are generated under ignored
paths. The runtime owns deterministic physics approximations, not this cooker.
"""
import hashlib,json,math,struct
from pathlib import Path
from PIL import Image
from source import Source,ROOT
from scene import Scene
from breakables import _components,collider_polygons
from focus import action_fields
from cook import tk_sprite,FOCAL,CAM_Z
from materials import quantize_alpha_coverage
ART_CELL=32  # projected pixels per Geo art cell in the shared bank
# Distinct rock+coin texel bytes admitted before placement; the remainder of the
# 8 KiB reservation covers the palette and fragment packing slack.
ART_BUDGET=8*1024-640
def payload_bytes(size):
    w,h=size;return (w+3)//4*2*h

# Halfword rectangles approved against the resident scene/font layout by root.
VRAM_RECTS=((320,464,64,16),(352,240,32,16),(352,485,32,27),(320,496,32,16),(352,32,8,32))
MAX_VRAM_BYTES=8192
COIN_CLIPS=('Small Idle','Small Air','Med Idle','Med Air','Large Idle','Large Air')

def q16(v):
    if not math.isfinite(v) or abs(v)>32767:raise ValueError('Geo fixed-point range')
    return round(v*65536)

def compact_fields(data,index):
    out=action_fields(data,index);start=data['actionStartIndex'][index]
    end=data['actionStartIndex'][index+1] if index+1<len(data['actionNames']) else len(data['paramName'])
    for p in range(start,end):
        name=data['paramName'][p] or str(p);kind=data['paramDataType'][p];pos=data['paramDataPos'][p];size=data['paramByteDataSize'][p]
        if kind==19:out[name]=data['fsmGameObjectParams'][pos]
        elif kind==22:out[name]=data['unityObjectParams'][pos]
        elif kind in (4,7) and size==4:out[name]=struct.unpack_from('<i',bytes(data['byteData']),pos)[0]
    return out

def fsm_contract(fsm):
    variables={v['name']:v['value']for group in fsm['variables'].values()if isinstance(group,list)for v in group if isinstance(v,dict)and 'name'in v and 'value'in v}
    states={s['name']:s for s in fsm['states']}
    def actions(st,typ):
        d=states[st]['actionData'];return [compact_fields(d,i)for i,n in enumerate(d['actionNames'])if n.split('.')[-1]==typ and d['actionEnabled'][i]]
    def value(v):return variables[v['name']]if v.get('useVariable')else v['value']
    def transition(st,event):return next(t['toState']for t in states[st]['transitions']if t['fsmEvent']['name']==event)
    hits,per_hit,final=[variables[n]for n in ('Hits','Geo Per Hit','Final Payout')]
    if any(type(v)is not int or not 1<=v<=255 for v in (hits,per_hit,final)):raise ValueError('Geo rock count range')
    if transition('Hit','HIT')!='Pause Frame' or transition('Pause Frame','FINISHED')!='Destroy':raise ValueError('Geo final-hit event order')
    op=actions('Hit','IntOperator')
    if len(op)!=1 or op[0]['integer1']['name']!='Hits' or value(op[0]['integer2'])!=1 or op[0]['storeResult']['name']!='Hits' or op[0]['operation']!=1:raise ValueError('Geo remaining-hit operation')
    flings=[]
    for state,name in [('Hit','Geo Per Hit'),('Destroy','Final Payout')]:
        a=[a for a in actions(state,'FlingObjectsFromGlobalPool')if a['spawnMin']['name']==name and a['spawnMax']['name']==name]
        if len(a)!=1:raise ValueError('Geo payout fling')
        a=a[0]
        if a['gameObject']['useVariable'] or not a['gameObject']['value']['m_PathID']:raise ValueError('Geo prefab is not literal')
        flings.append({'prefab':a['gameObject']['value'],'speed':[value(a[n])for n in ('speedMin','speedMax')], 'angle':[value(a[n])for n in ('angleMin','angleMax')], 'spread':[value(a[n])for n in ('originVariationX','originVariationY')]})
    if flings[0]!=flings[1]:raise ValueError('Geo final/ordinary fling differ')
    broken=actions('Broken','Tk2dPlayAnimation');disabled=actions('Broken','SetCollider')
    if len(broken)!=1 or len(disabled)!=1 or value(disabled[0]['active']):raise ValueError('Geo depleted state')
    return {'hits':hits,'per_hit':per_hit,'final_payout':final,'total':hits*per_hit+final,
            'fling':flings[0],'broken_clip':value(broken[0]['clipName']),'hit_cooldown':max(1,round(variables['Recoil Time']*60)),
            'variables':variables,'states':list(states)}

def place_rectangles(items,rectangles=VRAM_RECTS):
    """Bounded deterministic MaxRects search; CLUT X remains16-word aligned."""
    for i,(x,y,w,h)in enumerate(rectangles):
        if not(0<=x<x+w<=1024 and 0<=y<y+h<=512):raise ValueError('Geo rectangle outside VRAM')
        if any(x<ox+ow and x+w>ox and y<oy+oh and y+h>oy for ox,oy,ow,oh in rectangles[:i]):raise ValueError('Overlapping Geo rectangles')
    if len({k for k,w,h,a in items})!=len(items) or any(w<=0 or h<=0 or a<=0 for k,w,h,a in items):raise ValueError('invalid Geo allocations')
    if sum(w*h*2 for k,w,h,a in items)>MAX_VRAM_BYTES:raise ValueError('Geo VRAM exceeds8KiB')
    orders=(lambda v:(-v[2],-v[1],str(v[0])),lambda v:(-v[1]*v[2],-v[2],str(v[0])),lambda v:(-v[1],-v[2],str(v[0])))
    for order in orders:
        for mode in (0,1):
            free=list(rectangles);placed={}
            for key,w,h,alignment in sorted(items,key=order):
                candidates=[]
                for rx,ry,rw,rh in free:
                    x=(rx+alignment-1)//alignment*alignment
                    if x+w>rx+rw or h>rh:continue
                    dw=rx+rw-x-w;dh=rh-h
                    score=(min(dw,dh),max(dw,dh))if mode==0 else(rw*rh-w*h,min(dw,dh))
                    candidates.append((*score,ry,x))
                if not candidates:break
                *_,y,x=min(candidates);placed[key]=(x,y,w,h);new=[]
                for rx,ry,rw,rh in free:
                    if x>=rx+rw or x+w<=rx or y>=ry+rh or y+h<=ry:new.append((rx,ry,rw,rh));continue
                    if x>rx:new.append((rx,ry,x-rx,rh))
                    if x+w<rx+rw:new.append((x+w,ry,rx+rw-x-w,rh))
                    if y>ry:new.append((rx,ry,rw,y-ry))
                    if y+h<ry+rh:new.append((rx,y+h,rw,ry+rh-y-h))
                free=[r for r in sorted(set(new))if not any(r!=o and o[0]<=r[0] and o[1]<=r[1] and o[0]+o[2]>=r[0]+r[2] and o[1]+o[3]>=r[1]+r[3]for o in new)]
            if len(placed)==len(items):return placed
    raise ValueError('Geo textures do not fit reserved VRAM fragments: '+str(sum(w*h*2 for k,w,h,a in items))+' bytes '+str([(w,h)for k,w,h,a in items]))

def art_bank(images):
    """Joint sourceRGB palette; per-image source alpha uses existing coverage policy."""
    if not images:raise ValueError('empty Geo art')
    # Rock prefabs repeat across scenes: quantize each distinct source image once
    # and expand the per-frame mapping afterwards, so the 256x256 joint sheet is
    # bounded by distinct art rather than by rock occurrences.
    signatures=[(im.size,im.tobytes()) for im in images];first={}
    for index,signature in enumerate(signatures):first.setdefault(signature,index)
    if len(first)!=len(images):
        blob,uploads,textures,distinct_mapping,sheet=art_bank([images[i] for i in first.values()])
        position={signature:slot for slot,signature in enumerate(first)}
        return blob,uploads,textures,[distinct_mapping[position[s]] for s in signatures],sheet
    # A single palette avoids wasting47 independently quantized CLUTs. Every
    # sprite starts on a4px grid, preserving the established Bayer alpha phase.
    # Coin and rock art can share a palette through a taller narrow RGB sample
    # by using32px cells; projected current art is admitted at<=32 each axis.
    # The sheet only hosts the joint quantization and is sliced per image, so it
    # widens with the catalog; VRAM placement is bounded separately by VRAM_RECTS.
    cell=ART_CELL;cell_h=ART_CELL;cols=8;rows=(len(images)+cols-1)//cols
    oversize=[im.size for im in images if im.width>cell or im.height>cell_h]
    if rows*cell_h>256 or oversize:raise ValueError(f'Geo art projected extent: {len(images)} images in {rows} rows of {cell_h} px (limit 256), oversize {oversize[:4]}')
    sheet=Image.new('RGBA',(cols*cell,rows*cell_h))
    for i,im in enumerate(images):sheet.paste(im,((i%cols)*cell,(i//cols)*cell_h))
    sw,sh,pal,packed=quantize_alpha_coverage(sheet,128);payloads=[]
    for i,im in enumerate(images):
        w,h=im.size;stride=(w+3)//4*2;pix=bytearray(stride*h);x0=(i%cols)*cell;y0=(i//cols)*cell_h
        for y in range(h):
            for x in range(w):
                b=packed[(y+y0)*(sw//2)+(x+x0)//2];v=(b>>(((x+x0)&1)*4))&15
                pix[y*stride+x//2]|=v<<((x&1)*4)
        payloads.append(bytes(pix))
    # Exact texel-plane duplicates are legal without dropping source frame refs.
    keys={};unique=[];mapping=[]
    for im,p in zip(images,payloads):
        key=(im.size,p)
        if key not in keys:keys[key]=len(unique);unique.append((im.size,p))
        mapping.append(keys[key])
    items=[('palette',16,1,16)]+[(i,(size[0]+3)//4,size[1],1)for i,(size,p)in enumerate(unique)]
    locations=place_rectangles(items);blob=bytearray();uploads=[];textures=[]
    def upload(key,payload):
        x,y,w,h=locations[key];off=len(blob);blob.extend(payload);uploads.append({'offset':off,'x':x,'y':y,'w':w,'h':h})
    upload('palette',pal)
    for i,(size,p)in enumerate(unique):
        upload(i,p);x,y,w,h=locations[i];px,py,_,_=locations['palette'];base_x=x//64*64;base_y=y//256*256
        if (x-base_x)*4+size[0]>256 or y-base_y+size[1]>256:raise ValueError('Geo texture page wrap')
        textures.append({'x':x,'y':y,'w':size[0],'h':size[1],'u':(x-base_x)*4,'v':y-base_y,
            'clut':(py<<6)|(px//16),'tpage':(base_x//64)|((base_y//256)<<4),'material':1})
    if len(blob)>MAX_VRAM_BYTES:raise ValueError('Geo bank byte budget')
    return bytes(blob),uploads,textures,mapping,sheet

def _components_source(s,file,gid):
    return [(s.ref(file,c['component']),s.read(s.ref(file,c['component'])))for c in s.read(file.objects[gid])['m_Component']]

def collect(source):
    rocks=[];enemies=[];coins=[];scene_objects=[];art=[];clips=[];cache={};textures={}
    def clip_art(file,library,name,scale=(1.,1.)):
        lo=source.ref(file,library);lib=source.read(lo);clip=next(c for c in lib['clips']if c['name']==name)
        start=len(art)
        for f in clip['frames']:
            co=source.ref(lo.assets_file,f['spriteCollection']);im,box=tk_sprite(source,co.assets_file,source.read(co),f['spriteId'],textures)
            box=tuple(v*scale[i%2]for i,v in enumerate(box));dims=tuple(max(1,math.ceil((box[i+2]-box[i])*FOCAL/-CAM_Z))for i in (0,1))
            art.append({'source':f'{source.sid(co)}:{f["spriteId"]}','box':box,'image':im.resize(dims,Image.Resampling.LANCZOS),'original_size':im.size,'original_image':im})
        clips.append({'source':source.sid(lo),'name':name,'start':start,'count':len(clip['frames']),'fps':clip['fps'],'wrap':clip['wrapMode']});return len(clips)-1
    file=source.file('resources.assets')
    for o in file.objects.values():
        if o.type.name!='MonoBehaviour' or source.typename(o)!='GeoControl':continue
        t=source.read(o);comps=_components_source(source,file,t['m_GameObject']['m_PathID']);by={source.typename(o):(o,c)for o,c in comps}
        tr=by['Transform'][1];scale=tr['m_LocalScale'];sprite=by['tk2dSprite'][1];anim=by['tk2dSpriteAnimator'][1];body=by['BoxCollider2D'][1]
        if tr['m_Father']['m_PathID'] or scale['x']!=scale['y'] or sprite['_scale']!={'x':1.0,'y':1.0,'z':1.0}:raise ValueError('Geo prefab scale hierarchy')
        size=t['sizes'][t['type']];coin={'source':source.sid(o),'prefab':source.sid(file.objects[t['m_GameObject']['m_PathID']]),'type':t['type'],'value':size['value'],'scale':scale['x'],
            'body':body,'rigidbody':by['Rigidbody2D'][1],'bounce':by['ObjectBounce'][1], 'material':source.read(source.ref(file,body['m_Material'])),'pickup_sounds':[], 'clips':{}}
        for k in ('idleAnim','airAnim'):coin['clips'][k]=clip_art(file,anim['library'],size[k],(scale['x'],scale['y']))
        for ref in t['pickupSounds']:
            so=source.ref(file,ref);st=source.read(so);coin['pickup_sounds'].append({'source':source.sid(so),'name':st['m_Name'],'seconds':st['m_Length']})
        coins.append(coin)
    coins.sort(key=lambda c:c['type'])
    if [c['value']for c in coins]!=[1,5,25]:raise ValueError('Geo denomination source changed')
    from quality import SCENE_FILES
    unsupported_rocks=[]
    seen_art={(a['image'].size,a['image'].tobytes()) for a in art};art_bytes=sum(payload_bytes(size) for size,_ in seen_art)
    for scene_id,name in enumerate(SCENE_FILES):
        sc=Scene(source,name);scene_objects.append(sc)
        for ident,(kind,t) in sc.objects.items():
            if kind not in ('GeoRock','HealthManager') or not t.get('m_Enabled',1):continue
            gid=t['m_GameObject']['m_PathID']
            if not sc.active(gid):continue
            if kind=='HealthManager':
                enemies.append({'source_id':ident,'source':f'{name}:{ident}','scene':scene_id,'drops':[t[n+'GeoDrops']for n in ('small','medium','large')], 'mega':t['megaFlingGeo'],'position':sc.point(gid), 'effect_origin':t.get('effectOrigin',{})});continue
            comps=list(_components(sc,gid));by={k:(i,c)for i,k,c in comps};fid,ft=by['PlayMakerFSM'];contract=fsm_contract(ft['fsm'])
            prefab=source.sid(source.ref(sc.file,contract['fling'].pop('prefab')))
            if prefab!=coins[0]['prefab']:raise ValueError('Geo rock emits non-small coin')
            polys=[];colliders=[]
            for ci,ck,ct in comps:
                if ck.endswith('Collider2D') and ct['m_Enabled']:
                    polys.extend(collider_polygons(sc,gid,ck,ct));colliders.append(f'{name}:{ci}')
            if not polys:raise ValueError('Geo rock has no source collider')
            matrix=sc.world(sc.go_transform[gid]);sp=by['tk2dSprite'][1];sx=abs(matrix[0][0]*sp['_scale']['x']);sy=abs(matrix[1][1]*sp['_scale']['y'])
            # Keep original intact and broken images; current scenery omits tk2d rocks.
            animator=by['tk2dSpriteAnimator'][1];lo=source.ref(sc.file,animator['library']);initial=source.read(lo)['clips'][animator['defaultClipId']]['name']
            art_mark,clip_mark=len(art),len(clips)
            intact=clip_art(sc.file,animator['library'],initial,(sx,sy))
            if clips[intact]['count']!=1:raise ValueError('Geo initial clip not static')
            intact_frame=clips[intact]['start']
            ai=clip_art(sc.file,by['tk2dSpriteAnimator'][1]['library'],contract['broken_clip'],(sx,sy))
            if clips[ai]['count']!=1:raise ValueError('Geo broken clip not static')
            # The shared Geo art bank admits 32x32 projected cells; a larger rock is
            # an explicit omission for now rather than a resampled sprite.
            oversize=[a['image'].size for a in art[art_mark:] if max(a['image'].size)>ART_CELL]
            if oversize:
                del art[art_mark:];del clips[clip_mark:]
                unsupported_rocks.append({'source':f'{name}:{ident}','scene':scene_id,'reason':f'rock art projects to {max(oversize)} pixels, above the {ART_CELL}x{ART_CELL} Geo art cell'})
                continue
            # Rock art shares one fixed VRAM reservation with the coins. Rocks are
            # admitted in catalog order until the distinct-image budget is spent;
            # later rocks are explicit omissions until Geo art streams per scene.
            fresh=[(a['image'].size,a['image'].tobytes()) for a in art[art_mark:]]
            added=sum(payload_bytes(size) for size,pixels in {s:None for s in fresh if s not in seen_art})
            if art_bytes+added>ART_BUDGET:
                del art[art_mark:];del clips[clip_mark:]
                unsupported_rocks.append({'source':f'{name}:{ident}','scene':scene_id,'reason':f'Geo art VRAM budget: {art_bytes+added} distinct bytes would exceed {ART_BUDGET}'})
                continue
            art_bytes+=added;seen_art.update(fresh)
            points=[p for poly in polys for p in poly];origin=sc.point(gid)
            # `state` is the rock's index within its own scene: the guest keys
            # rock state as scene*MAX_ROCKS_PER_SCENE+state and asserts
            # state<MAX_ROCKS_PER_SCENE on every swing in the scene. The catalogue
            # index it used to be passed 16 once 35 rocks were admitted, so the
            # first nail swing in any scene holding the 17th rock or later
            # panicked (geo.rs, the assert at the top of `strike`).
            state=rock_state(rocks,scene_id)
            rocks.append(contract|{'source':f'{name}:{ident}','source_id':ident,'scene':scene_id,'state':state,'x':origin[0],'y':origin[1],
                'polygons':polys,'bounds':[min(p[0]for p in points),min(p[1]for p in points),max(p[0]for p in points),max(p[1]for p in points)],
                'draw_sources':[f'{name}:{by[k][0]}'for k in ('tk2dSprite','MeshRenderer')if k in by], 'collider_sources':colliders,'broken_frame':clips[ai]['start'], 'intact_frame':intact_frame, 'intact_vertices':[sc.point(gid,xx/sx*sp['_scale']['x'],yy/sy*sp['_scale']['y'])[:2] for xx,yy in [(art[intact_frame]['box'][0],art[intact_frame]['box'][3]),(art[intact_frame]['box'][2],art[intact_frame]['box'][3]),(art[intact_frame]['box'][0],art[intact_frame]['box'][1]),(art[intact_frame]['box'][2],art[intact_frame]['box'][1])]], 'fsm_source':f'{name}:{fid}', 'broken_vertices':[sc.point(gid,xx/sx*sp['_scale']['x'],yy/sy*sp['_scale']['y'])[:2] for xx,yy in [(art[-1]['box'][0],art[-1]['box'][3]),(art[-1]['box'][2],art[-1]['box'][3]),(art[-1]['box'][0],art[-1]['box'][1]),(art[-1]['box'][2],art[-1]['box'][1])]]})
    canonical_images(art)
    for r in rocks:r['intact_extra']=[];r['broken_extra']=[]
    return rocks,enemies,coins,art,clips,unsupported_rocks

# game/src/geo.rs MAX_ROCKS_PER_SCENE.
MAX_ROCKS_PER_SCENE=16
def rock_state(rocks,scene_id):
    """The next rock's per-scene state index, refused at the guest's limit."""
    state=sum(r['scene']==scene_id for r in rocks)
    if state>=MAX_ROCKS_PER_SCENE:raise ValueError(f'scene {scene_id} admits more than {MAX_ROCKS_PER_SCENE} Geo rocks')
    return state

def canonical_images(art):
    """Share source pixels at the maximum requested axes without shrinking any."""
    groups={};pixels={}
    for a in art:
        key=a['source'];im=a['original_image'];original=(im.size,im.tobytes())
        if key in pixels and pixels[key]!=original:raise ValueError('Geo source identity aliases different pixels')
        pixels[key]=original;w,h=a['image'].size;old=groups.get(key,(0,0));groups[key]=(max(w,old[0]),max(h,old[1]))
    for a in art:a['image']=a.pop('original_image').resize(groups[a['source']],Image.Resampling.LANCZOS)

def rust_parts(parts):return '&['+','.join('GeoPart{art:'+str(p['art'])+',vertices:['+','.join(rust_array([q16(v)for v in q])for q in p['vertices'])+']}'for p in parts)+']'

def rust_array(values):return '['+','.join(str(v)for v in values)+']'
def rust_fling(f):return 'Fling{'+','.join(k+':'+rust_array([q16(v)for v in f[k]])for k in ('speed','angle','spread'))+'}'
def generate(rocks,enemies,coins,art,clips,textures,mapping,uploads,bindings,hero_box,gravity):
    lines=['// Generated from read-only Windows source by host/geo.py.\n']
    lines.append('pub const HERO_BOX:[i32;4]='+rust_array([q16(v)for v in hero_box])+';')
    lines.append('pub const GEO_PARAMS:Params=Params{pickup_ticks:15,wallet_max:9999999,coins:[')
    for c in coins:
        scale=c['scale'];b=c['body'];offset=[q16(b['m_Offset'][k]*scale)for k in 'xy'];half=[q16(b['m_Size'][k]*scale/2)for k in 'xy']
        lines.append('CoinSpec{value:'+str(c['value'])+',gravity:'+str(q16(gravity*c['rigidbody']['m_GravityScale']))+',body_offset:'+rust_array(offset)+',half:'+rust_array(half)+',pickup_offset:'+rust_array(offset)+',pickup_half:'+rust_array(half)+',bounce:'+str(q16(c['bounce']['bounceFactor']))+',threshold:'+str(q16(c['bounce']['speedThreshold']))+',friction:'+str(q16(c['material']['friction']))+'},')
    lines.append(']};\npub const GEO_ROCKS:&[RockSpec]=&[')
    for r in rocks:
        polys='&['+','.join('&['+','.join(rust_array([q16(v)for v in p])for p in poly)+']'for poly in r['polygons'])+']'
        fields={k:r[k]for k in ('source_id','scene','state','hits','per_hit','final_payout','hit_cooldown','broken_frame')}
        lines.append('RockSpec{'+','.join(f'{k}:{v}'for k,v in fields.items())+',x:'+str(q16(r['x']))+',y:'+str(q16(r['y']))+',bounds:'+rust_array([q16(v)for v in r['bounds']])+',polygons:'+polys+',fling:'+rust_fling(r['fling'])+'},')
    # Enemy payouts are not linked: every one of them rides in its own scene's
    # metadata bank, which host/pack_scenes.py fills from this cook's report.
    # Only `megaFlingGeo` survives the move, because the two fling profiles it
    # chooses between are the guest's own `geo::FLING` pair.
    lines.append('];\n#[derive(Clone,Copy)] pub struct GeoArt{pub u:u8,pub v:u8,pub w:u8,pub h:u8,pub clut:u16,pub tpage:u16,pub bounds:[i32;4]}\npub const GEO_ART:&[GeoArt]=&[')
    for a,tid in zip(art,mapping):
        t=textures[tid];lines.append('GeoArt{'+','.join(f'{k}:{t[k]}'for k in ('u','v','w','h','clut','tpage'))+',bounds:'+rust_array([q16(v)for v in a['box']])+'},')
    lines.append('];\n#[derive(Clone,Copy)] pub struct GeoClip{pub start:u16,pub count:u16,pub fps:u16}\npub const GEO_COIN_CLIPS:[[GeoClip;2];3]=[')
    for c in coins:
        vals=[]
        for name in ('idleAnim','airAnim'):
            cl=clips[c['clips'][name]]
            if cl['fps']!=int(cl['fps']) or cl['wrap']!=0:raise ValueError('Geo loop animation contract')
            vals.append('GeoClip{'+','.join(f'{k}:{int(cl[k])}'for k in ('start','count','fps'))+'}')
        lines.append('['+','.join(vals)+'],')
    lines.append('];\n#[derive(Clone,Copy)] pub struct GeoUpload{pub offset:usize,pub x:u16,pub y:u16,pub w:u16,pub h:u16}\npub const GEO_UPLOADS:&[GeoUpload]=&[')
    for u in uploads:lines.append('GeoUpload{'+','.join(f'{k}:{v}'for k,v in u.items())+'},')
    # A slot per catalogue region spent eight linked bytes on every view that
    # never sees a rock. The sparse sorted list grows with the bound rocks.
    lines.append('];\npub struct GeoPart{pub art:u16,pub vertices:[[i32;2];4]}\npub struct GeoDrawBinding{pub state:u8,pub off:&\'static[u16],pub edges:&\'static[u16],pub broken_art:u16,pub vertices:[[i32;2];4],pub intact_art:u16,pub intact_vertices:[[i32;2];4],pub intact_extra:&\'static[GeoPart],pub broken_extra:&\'static[GeoPart]}\n/// Regions with at least one bound rock, sorted by catalogue region index.\npub const GEO_BINDINGS:&[(u16,&[GeoDrawBinding])]=&[')
    for index,row in enumerate(bindings):
        if not row:continue
        if index>0xFFFF:raise ValueError('geo binding region index exceeds u16')
        lines.append(f'({index},&[')
        for b in row:lines.append('GeoDrawBinding{state:'+str(b['state'])+',off:&'+rust_array(b['off'])+',edges:&'+rust_array(b['edges'])+',broken_art:'+str(b['broken_art'])+',vertices:['+','.join(rust_array([q16(v)for v in p])for p in b['vertices'])+'],intact_art:'+str(b['intact_art'])+',intact_vertices:['+','.join(rust_array([q16(v)for v in p])for p in b['intact_vertices'])+'],intact_extra:'+rust_parts(b['intact_extra'])+',broken_extra:'+rust_parts(b['broken_extra'])+'},')
        lines.append(']),')
    lines.append('];\n');return '\n'.join(lines)

def bind_regions(rocks,metadata):
    bindings=[]
    for region in metadata['regions']:
        scene=json.loads((ROOT/region.get('base_path',f'data/regions/region-{region["chunk_id"]:03}/room.hk')).with_name('scene.json').read_text());row=[]
        for r in rocks:
            off=[i for i,d in enumerate(scene['draws'])if d['source']in r['draw_sources']]
            sources=region['edge_sources'];edges=[i for i,e in enumerate(sources)if (e if isinstance(e,str)else e.get('source'))in r['collider_sources']]
            b=region['interaction_bounds'];v=r['intact_vertices']+r['broken_vertices']+[p for k in ('intact_extra','broken_extra')for part in r[k]for p in part['vertices']]
            visible=region['scene_id']==r['scene'] and min(p[0]for p in v)<=b[2] and max(p[0]for p in v)>=b[0] and min(p[1]for p in v)<=b[3] and max(p[1]for p in v)>=b[1]
            if off or edges or visible:row.append({'state':r['state'],'off':off,'edges':edges,'broken_art':r['broken_frame'],'vertices':r['broken_vertices'],'intact_art':r['intact_frame'],'intact_vertices':r['intact_vertices'],'intact_extra':r['intact_extra'],'broken_extra':r['broken_extra']})
        if sum(max(1+len(b['intact_extra']),1+len(b['broken_extra']))for b in row)>16:raise ValueError('Geo rock draw budget')
        bindings.append(row)
    return bindings

def source_cil_evidence(source):
    import dnfile
    from dncil.cil.body.reader import read_method_body_from_bytes
    from actors import _literal
    path=source.directory/'Managed/Assembly-CSharp.dll';pe=dnfile.dnPE(str(path));found={}
    expected={('GeoControl','OnEnable'):(.25,),('PlayerData','AddGeo'):(9999999,),('HealthManager','Die'):(15.,30.,80.,100.)}
    for typ in pe.net.mdtables.TypeDef.rows:
        for ref in typ.MethodList:
            method=ref.row;key=(str(typ.TypeName),str(method.Name))
            if key not in expected:continue
            body=read_method_body_from_bytes(pe.get_data(method.Rva,100000));literals=[_literal(i)for i in body.instructions]
            if not all(v in literals for v in expected[key]):raise ValueError('Geo source CIL constants changed: '+str(key))
            found['.'.join(key)]={'sha256':hashlib.sha256(pe.get_data(method.Rva,body.size)).hexdigest(),'verified_literals':expected[key]}
    if len(found)!=len(expected):raise ValueError('Geo CIL method absent')
    return found

def cook():
    source=Source();cil=source_cil_evidence(source);rocks,enemies,coins,art,clips,unsupported_rocks=collect(source)
    blob,uploads,textures,mapping,sheet=art_bank([a['image']for a in art])
    metadata=json.loads((ROOT/'data/regions.json').read_text());bindings=bind_regions(rocks,metadata)
    # HeroBox is a child at identity local transform in the observed source.
    f=source.file('resources.assets');hero=[]
    for o in f.objects.values():
        if o.type.name!='BoxCollider2D':continue
        t=source.read(o);go=source.ref(f,t['m_GameObject']);gt=source.read(go)
        if gt['m_Name']!='HeroBox':continue
        if not t['m_IsTrigger'] or not t['m_Enabled']:continue
        comps=_components_source(source,f,go.path_id);tr=next(c for o,c in comps if o.type.name=='Transform')
        if tr['m_LocalPosition']!={'x':0.,'y':0.,'z':0.} or tr['m_LocalScale']!={'x':1.,'y':1.,'z':1.}:raise ValueError('HeroBox local transform changed')
        hero.append(([t['m_Offset'][k]+sign*t['m_Size'][k]/2 for sign in (-1,1)for k in 'xy'],source.sid(o)))
    if len(hero)!=1:raise ValueError('HeroBox source is ambiguous')
    physics=next(o for o in source.file('globalgamemanagers').objects.values()if o.type.name=='Physics2DSettings');gravity=source.read(physics)['m_Gravity']['y']
    rust=generate(rocks,enemies,coins,art,clips,textures,mapping,uploads,bindings,hero[0][0],gravity)
    out=ROOT/'data';out.mkdir(exist_ok=True);(out/'geo.hk').write_bytes(blob);(out/'geo.rs').write_text(rust)
    report={'format':'HKGEO01','region_metadata_sha256':hashlib.sha256((ROOT/'data/regions.json').read_bytes()).hexdigest(),'code_sha256':{n:hashlib.sha256((ROOT/n).read_bytes()).hexdigest()for n in ('host/geo.py','host/source.py','host/cook.py','host/materials.py','host/scene.py','host/focus.py')},'cil_evidence':cil,'previously_omitted_rock_art':True,'rocks':rocks,'unsupported_rocks':unsupported_rocks,'enemies':enemies,'coins':coins,'clips':clips,'art':[{k:v for k,v in a.items()if k!='image'}|{'texture':tid}for a,tid in zip(art,mapping)],'textures':textures,'uploads':uploads,'bindings':bindings,'hero_box':hero,'gravity':gravity,
        'vram_bytes':len(blob),'vram_limit':MAX_VRAM_BYTES,'permitted_rectangles':VRAM_RECTS,'payload_sha256':hashlib.sha256(blob).hexdigest(),'rust_sha256':hashlib.sha256(rust.encode()).hexdigest(),
        'source_files':{str(Path(n).relative_to(source.directory)):hashlib.sha256(Path(n).read_bytes()).hexdigest()for n in sorted({str(source.directory/name)for name in source.files}|{str(source.directory/'Managed/Assembly-CSharp.dll'),str(source.directory/'Managed/PlayMaker.dll')}|{str(p)for name in source.files for p in source.directory.glob(name+'*')if p.is_file()})if Path(n).is_file()},
        'limitations':['Source colors quantized jointly to15 visible colors; source alpha approximated by ordered0/half/full coverage.', 'Coin images sampled at current camera projection; no scenery/room changes or gameplay disc reads.', 'GeoControl Get clip is unused in the observed collection handler and is not cooked.', 'Rock gleam, hit jitter, debris and acid reactions are recorded as incomplete runtime effects.']}
    dest=ROOT/'.hkpsx/geo-source';dest.mkdir(exist_ok=True);(dest/'report.json').write_text(json.dumps(report,indent=2));(ROOT/'.hkpsx/geo-provenance.json').write_text(json.dumps(report,indent=2));sheet.save(dest/'art.png')
    print(f'Geo: {len(rocks)} rocks, {len(enemies)} enemy payouts, {len(art)} frames/{len(textures)} textures, {len(blob)} VRAM bytes')
    return report

if __name__=='__main__':cook()
