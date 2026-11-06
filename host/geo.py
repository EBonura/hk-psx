"""Source-backed Geo rock metadata and a bounded, startup-only 4bpp art bank.

No room pack is modified. Retail sprites/reports are generated under ignored
paths. The runtime owns deterministic physics approximations, not this cooker.
"""
import hashlib,json,math,random,struct
from pathlib import Path
from PIL import Image
from source import Source,ROOT
from scene import Scene
from breakables import _components,collider_polygons
from focus import action_fields
from cook import tk_sprite,FOCAL,CAM_Z
from materials import quantize_alpha_coverage
ART_CELL=36  # the widest/tallest projected rock art the bank takes (the Greenpath rocks project 33x31 to 35x36)
# The reserved VRAM rectangles (VRAM_RECTS) are 16 and 27 rows tall, so a texture taller than MAX_ROWS is cut into horizontal
# strips of at most STRIP_ROWS rows, drawn as one quad each (`finalize`): the frame's first strip stays its art, the rest are
# the rock's extra parts.
MAX_ROWS=27
STRIP_ROWS=16
RESTARTS=400  # shuffled placement attempts after the three ordered ones (`place_rectangles`)
ALPHA_KEPT=64  # the alpha coverage quantizer's cut: pixels below it draw nothing
# Distinct rock+coin texel bytes admitted before placement; the remainder of the
# 8 KiB reservation covers the palette and fragment packing slack.
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

def free_rectangles(rectangles,placed):
    """The maximal empty rectangles of `rectangles` once the `placed` ones (x,y,w,h) are taken; they may overlap."""
    out=[]
    for rx,ry,rw,rh in rectangles:
        occ=[[False]*rw for _ in range(rh)]
        for x,y,w,h in placed:
            for yy in range(max(y,ry),min(y+h,ry+rh)):
                for xx in range(max(x,rx),min(x+w,rx+rw)):occ[yy-ry][xx-rx]=True
        def empty(x0,y0,w,h):return all(not occ[yy][xx]for yy in range(y0,y0+h)for xx in range(x0,x0+w))
        found=set()
        for y0 in range(rh):
            for x0 in range(rw):
                width=rw-x0
                for y1 in range(y0,rh):
                    w=0
                    while w<width and not occ[y1][x0+w]:w+=1
                    width=w
                    if not w:break
                    found.add((x0,y0,w,y1-y0+1))
        for x0,y0,w,h in sorted(found):
            if (x0>0 and empty(x0-1,y0,1,h)) or (y0>0 and empty(x0,y0-1,w,1)) or (x0+w<rw and empty(x0+w,y0,1,h)) or (y0+h<rh and empty(x0,y0+h,w,1)):continue
            out.append((rx+x0,ry+y0,w,h))
    return out

def place_rectangles(items,rectangles=VRAM_RECTS,free=None):
    """The first placement `placements` finds, else an error."""
    for placed in placements(items,rectangles,free):return placed
    raise ValueError('Geo textures do not fit reserved VRAM fragments: '+str(sum(w*h*2 for k,w,h,a in items))+' bytes '+str([(w,h)for k,w,h,a in items]))

def placements(items,rectangles=VRAM_RECTS,free=None):
    """Bounded deterministic MaxRects search, every complete placement it finds in turn; CLUT X remains16-word aligned.
    `free` starts the search from those (possibly overlapping) empty rectangles instead of `rectangles`."""
    for i,(x,y,w,h)in enumerate(rectangles):
        if not(0<=x<x+w<=1024 and 0<=y<y+h<=512):raise ValueError('Geo rectangle outside VRAM')
        if any(x<ox+ow and x+w>ox and y<oy+oh and y+h>oy for ox,oy,ow,oh in rectangles[:i]):raise ValueError('Overlapping Geo rectangles')
    if len({k for k,w,h,a in items})!=len(items) or any(w<=0 or h<=0 or a<=0 for k,w,h,a in items):raise ValueError('invalid Geo allocations')
    if sum(w*h*2 for k,w,h,a in items)>MAX_VRAM_BYTES:raise ValueError('Geo VRAM exceeds8KiB')
    orders=[lambda v:(-v[2],-v[1],str(v[0])),lambda v:(-v[1]*v[2],-v[2],str(v[0])),lambda v:(-v[1],-v[2],str(v[0]))]
    # The reservation is a handful of small rectangles and the heuristic orders above leave gaps a full bank needs: after
    # them, `RESTARTS` deterministic shuffles of the same items (the same seeds in every cook, so the layout is stable).
    for seed in range(RESTARTS):
        shuffled={str(v[0]):i for i,v in enumerate(random.Random(seed).sample(items,len(items)))}
        orders.append(lambda v,shuffled=shuffled:shuffled[str(v[0])])
    for order in orders:
        for mode in (0,1):
            free_now=list(rectangles if free is None else free);placed={}
            for key,w,h,alignment in sorted(items,key=order):
                candidates=[]
                for rx,ry,rw,rh in free_now:
                    x=(rx+alignment-1)//alignment*alignment
                    if x+w>rx+rw or h>rh:continue
                    dw=rx+rw-x-w;dh=rh-h
                    score=(min(dw,dh),max(dw,dh))if mode==0 else(rw*rh-w*h,min(dw,dh))
                    candidates.append((*score,ry,x))
                if not candidates:break
                *_,y,x=min(candidates);placed[key]=(x,y,w,h);new=[]
                for rx,ry,rw,rh in free_now:
                    if x>=rx+rw or x+w<=rx or y>=ry+rh or y+h<=ry:new.append((rx,ry,rw,rh));continue
                    if x>rx:new.append((rx,ry,x-rx,rh))
                    if x+w<rx+rw:new.append((x+w,ry,rx+rw-x-w,rh))
                    if y>ry:new.append((rx,ry,rw,y-ry))
                    if y+h<ry+rh:new.append((rx,y+h,rw,ry+rh-y-h))
                free_now=[r for r in sorted(set(new))if not any(r!=o and o[0]<=r[0] and o[1]<=r[1] and o[0]+o[2]>=r[0]+r[2] and o[1]+o[3]>=r[1]+r[3]for o in new)]
            if len(placed)==len(items):yield placed

def art_bank(images,rectangles=VRAM_RECTS,free=None,score=None):
    """Joint sourceRGB palette; per-image source alpha uses existing coverage policy: `art_pack`, then `art_place`."""
    return art_place(art_pack(images),rectangles,free,score)

def art_pack(images):
    """Quantize `images` jointly: (palette, [(size, 4bpp payload)] of the distinct texel planes, each image's index into them, the sheet)."""
    if not images:raise ValueError('empty Geo art')
    # Rock prefabs repeat across scenes: quantize each distinct source image once
    # and expand the per-frame mapping afterwards, so the 256x256 joint sheet is
    # bounded by distinct art rather than by rock occurrences.
    signatures=[(im.size,im.tobytes()) for im in images];first={}
    for index,signature in enumerate(signatures):first.setdefault(signature,index)
    if len(first)!=len(images):
        pal,unique,distinct_mapping,sheet=art_pack([images[i] for i in first.values()])
        position={signature:slot for slot,signature in enumerate(first)}
        return pal,unique,[distinct_mapping[position[s]] for s in signatures],sheet
    # A single palette avoids wasting47 independently quantized CLUTs. Every
    # sprite starts on a4px grid, preserving the established Bayer alpha phase.
    # The sheet only hosts the joint quantization and is sliced per image, so it
    # widens with the catalog; VRAM placement is bounded separately by VRAM_RECTS.
    # The joint quantizer takes one sheet of at most 256x256, so the images are shelf-packed (tallest first, left to
    # right, every origin on the 4 px grid the Bayer matrix repeats on) instead of one per fixed cell.
    oversize=[im.size for im in images if im.width>ART_CELL or im.height>ART_CELL]
    if oversize:raise ValueError(f'Geo art projected extent: oversize {oversize[:4]} above {ART_CELL}')
    place=[None]*len(images);x=y=shelf=0
    for i in sorted(range(len(images)),key=lambda i:(-images[i].height,-images[i].width,i)):
        w,h=images[i].size
        if x+w>256:x=0;y+=(shelf+3)//4*4;shelf=0
        if y+h>256:raise ValueError(f'Geo art sheet: {len(images)} images do not pack into 256x256')
        place[i]=(x,y);x+=(w+3)//4*4;shelf=max(shelf,h)
    sheet=Image.new('RGBA',(256,min(256,y+shelf)))
    for i,im in enumerate(images):sheet.paste(im,place[i])
    sw,sh,pal,packed=quantize_alpha_coverage(sheet,128);payloads=[]
    for i,im in enumerate(images):
        w,h=im.size;stride=(w+3)//4*2;pix=bytearray(stride*h);x0,y0=place[i]
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
    return pal,unique,mapping,sheet

def art_place(pack,rectangles=VRAM_RECTS,free=None,score=None):
    """Place a packed bank: (blob, uploads, textures, mapping, sheet). With `score`, the placement it rates highest of
    all those found (a fixed region that leaves the most room for what follows), else the first that suits a texture page."""
    pal,unique,mapping,sheet=pack
    items=[('palette',16,1,16)]+[(i,(size[0]+3)//4,size[1],1)for i,(size,p)in enumerate(unique)]
    best=None
    for locations in placements(items,rectangles,free):
        px,py,_,_=locations['palette']
        # The page rule: a texture may not wrap a 256x256 texture page.
        if any((x-x//64*64)*4+size[0]>256 or y-y//256*256+size[1]>256 for i,(size,p)in enumerate(unique)for x,y,_,_ in (locations[i],)):continue
        if score is None:best=locations;break
        rating=score(locations)
        if best is None or rating>best[0]:best=(rating,locations)
    if best is None:raise ValueError('Geo textures do not fit reserved VRAM fragments: '+str(sum(w*h*2 for k,w,h,a in items))+' bytes '+str([(w,h)for k,w,h,a in items]))
    locations=best if score is None else best[1]
    blob=bytearray();uploads=[];textures=[]
    def upload(key,payload):
        x,y,w,h=locations[key];off=len(blob);blob.extend(payload);uploads.append({'offset':off,'x':x,'y':y,'w':w,'h':h})
    upload('palette',pal)
    for i,(size,p)in enumerate(unique):
        upload(i,p);x,y,w,h=locations[i];px,py,_,_=locations['palette'];base_x=x//64*64;base_y=y//256*256
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
    unsupported_rocks=[];scene_ranges={}
    coin_art=len(art)  # the coins' frames come first; only rock art is trimmed and cut
    coin_bank=coin_region([a['image'] for a in finalize(art,coin_art)[0]]);rock_free=coin_bank[5]
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
            # The shared Geo art bank admits ART_CELL projected pixels each way; a larger rock is an explicit omission.
            oversize=[a['image'].size for a in art[art_mark:] if max(a['image'].size)>ART_CELL]
            if oversize:
                del art[art_mark:];del clips[clip_mark:]
                unsupported_rocks.append({'source':f'{name}:{ident}','scene':scene_id,'reason':f'rock art projects to {max(oversize)} pixels, above the {ART_CELL}x{ART_CELL} Geo art cell'})
                continue
            # A scene's rock art is a bank of its own, uploaded when the scene is admitted into the part of the Geo
            # reservation the coins leave (`coin_region`). A rock is admitted when the bank built from this scene's
            # admitted rocks and this one, finished the way the cook will finish it (`finalize`), still places in that
            # part: the exact packed bytes, not an estimate. Rocks of other scenes never compete for the space.
            scene_ranges.setdefault(scene_id,[]).append((art_mark,len(art)))
            try:scene_bank(art,coin_art,scene_ranges[scene_id],rock_free)
            except ValueError as error:
                scene_ranges[scene_id].pop();del art[art_mark:];del clips[clip_mark:]
                unsupported_rocks.append({'source':f'{name}:{ident}','scene':scene_id,'reason':f'Geo art VRAM budget: the scene bank no longer places with this rock ({str(error)[:90]})'})
                continue
            def vertices_of(index,gid=gid,sc=sc,sx=sx,sy=sy,sp=sp):
                box=art[index]['box'];return [sc.point(gid,xx/sx*sp['_scale']['x'],yy/sy*sp['_scale']['y'])[:2] for xx,yy in [(box[0],box[3]),(box[2],box[3]),(box[0],box[1]),(box[2],box[1])]]
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
                'draw_sources':[f'{name}:{by[k][0]}'for k in ('tk2dSprite','MeshRenderer')if k in by], 'collider_sources':colliders,'broken_frame':clips[ai]['start'], 'intact_frame':intact_frame,
                'fsm_source':f'{name}:{fid}','vertices_of':vertices_of,'intact_vertices':None,'broken_vertices':None})
    banks={scene:scene_bank(art,coin_art,ranges,rock_free) for scene,ranges in sorted(scene_ranges.items()) if ranges}
    canonical_images(art)
    # Trim, cut and place: the art the bank holds, then every rock's quads from its frames' final boxes.
    final,parts=finalize(art,coin_art)
    art[:]=final
    for r in rocks:
        vertices=r.pop('vertices_of')
        r['intact_vertices']=vertices(r['intact_frame']);r['broken_vertices']=vertices(r['broken_frame'])
        r['intact_extra']=[{'art':i,'vertices':vertices(i)} for i in parts.get(r['intact_frame'],[])]
        r['broken_extra']=[{'art':i,'vertices':vertices(i)} for i in parts.get(r['broken_frame'],[])]
    return rocks,enemies,coins,art,clips,unsupported_rocks,banks,coin_bank

# game/src/geo.rs MAX_ROCKS_PER_SCENE.
MAX_ROCKS_PER_SCENE=16
def rock_state(rocks,scene_id):
    """The next rock's per-scene state index, refused at the guest's limit."""
    state=sum(r['scene']==scene_id for r in rocks)
    if state>=MAX_ROCKS_PER_SCENE:raise ValueError(f'scene {scene_id} admits more than {MAX_ROCKS_PER_SCENE} Geo rocks')
    return state

COIN_RECTS=tuple(r for r in VRAM_RECTS if r!=(352,485,32,27))  # the coins keep clear of the tall sliver, where the taller rocks go

def coin_region(images):
    """The coins' art bank, placed once for every scene: (blob, uploads, textures, mapping, sheet, empty rectangles left for rocks).
    Of all placements in COIN_RECTS the one that leaves the largest empty rectangle (then the most area in whole) wins."""
    def area(r):return r[2]*r[3]
    def left(locations):
        free=free_rectangles(VRAM_RECTS,list(locations.values()))
        return (max(map(area,free)),sum(map(area,free)),-len(free))
    bank=art_bank(images,COIN_RECTS,score=left)
    return (*bank,free_rectangles(VRAM_RECTS,[(u['x'],u['y'],u['w'],u['h'])for u in bank[1]]))

def scene_bank(art,coin_art,ranges,free):
    """One scene's rock bank from the `ranges` of `art` its rocks hold: (blob, uploads, textures, mapping, sheet, entries), where entry
    k of `entries` is the art index (as `finalize` numbers them, strips included) texture `mapping[k]` is for."""
    final,parts=finalize(art,coin_art)
    entries=sorted({i for a,b in ranges for i in range(a,b)}|{j for a,b in ranges for i in range(a,b) for j in parts.get(i,[])})
    return (*art_bank([final[i]['image']for i in entries],VRAM_RECTS,free),entries)

def finalize(art,coin_art=0):
    """The art the bank holds: (entries, {frame index: [indices of its extra strips]}), `art` untouched.

    Every rock entry (those after the coins') is shrunk to the pixels the alpha coverage keeps, with its box shrunk
    in step so the drawn texels keep their place (a transparent border cost bank bytes and drew nothing), and one
    still taller than MAX_ROWS is cut into strips of at most STRIP_ROWS rows. The first strip is the entry itself;
    the rest are appended after every entry, one quad each, stacked exactly where the whole frame was.
    Canonical sizes first: one source drawn at several scales shares its largest pixels (`canonical_images`).
    """
    groups={}
    for a in art:
        w,h=a['image'].size;old=groups.get(a['source'],(0,0));groups[a['source']]=(max(w,old[0]),max(h,old[1]))
    out=[];tails=[];parts={}
    for index,a in enumerate(art):
        image=a['original_image'].resize(groups[a['source']],Image.Resampling.LANCZOS) if 'original_image' in a else a['image']
        box=a['box'];source=a['source']
        if index>=coin_art:
            keep=image.getchannel('A').point(lambda v:255 if v>=ALPHA_KEPT else 0).getbbox()
            if keep and keep!=(0,0)+image.size:
                x0,y0,x1,y1=box;w,h=image.size
                box=(x0+(x1-x0)*keep[0]/w,y1-(y1-y0)*keep[3]/h,x0+(x1-x0)*keep[2]/w,y1-(y1-y0)*keep[1]/h)
                image=image.crop(keep)
        w,h=image.size
        if index<coin_art or h<=MAX_ROWS:
            out.append({**a,'image':image,'box':box});continue
        count=-(-h//STRIP_ROWS);x0,y0,x1,y1=box;made=[]
        for k in range(count):
            r0,r1=k*h//count,(k+1)*h//count
            strip={'source':f'{source}@{w}x{h}#strip{k}of{count}','box':(x0,y1-(y1-y0)*r1/h,x1,y1-(y1-y0)*r0/h),'image':image.crop((0,r0,w,r1)),'original_size':a['original_size']}
            if k==0:out.append({**a,**strip})
            else:tails.append(strip);made.append(len(art)+len(tails)-1)
        parts[index]=made
    return out+tails,parts

def canonical_images(art):
    """Share source pixels at the maximum requested axes without shrinking any."""
    groups={};pixels={}
    for a in art:
        key=a['source'];im=a['original_image'];original=(im.size,im.tobytes())
        if key in pixels and pixels[key]!=original:raise ValueError('Geo source identity aliases different pixels')
        pixels[key]=original;w,h=a['image'].size;old=groups.get(key,(0,0));groups[key]=(max(w,old[0]),max(h,old[1]))
    for a in art:a['image']=a.pop('original_image').resize(groups[a['source']],Image.Resampling.LANCZOS)


def rust_array(values):return '['+','.join(str(v)for v in values)+']'
# game/src/geo.rs ROCK_FLING: every source rock carries this one profile, so the table spends no bytes on it.
ROCK_FLING_Q16=[[23*65536,30*65536],[80*65536,100*65536],[16384,16384]]
def corners(quad):
    """Three corners (TL, TR, BL) of a sprite quad in world units; a rotated or scaled sprite rectangle is a parallelogram, so the
    guest rebuilds BR as TR+BL-TL (geo_render::parallelogram). Refuse a quad where that is off by more than 2 Q16 units."""
    q=[[q16(v)for v in p]for p in quad]
    if any(abs(q[3][k]-(q[1][k]+q[2][k]-q[0][k]))>2 for k in (0,1)):raise ValueError(f'Geo rock art quad is not a parallelogram: {q}')
    return [list(p)for p in quad[:3]]
def generate(rocks,enemies,coins,art,clips,textures,mapping,uploads,hero_box,gravity):
    # Rocks are not linked: each rides in its own scene's metadata bank (`bank_rows`, filled by host/pack_scenes.py),
    # and so do the enemy payouts. Only what a bank cannot hold lives here: the coin parameters, the art bank's texture
    # entries (uploaded at startup, whatever the scene) and the coin frames' boxes.
    for r in rocks:
        if [[q16(v)for v in r['fling'][k]]for k in ('speed','angle','spread')]!=ROCK_FLING_Q16:raise ValueError('Geo rock fling differs from the guest geo::ROCK_FLING')
    lines=['// Generated from read-only Windows source by host/geo.py.\n']
    lines.append('pub const HERO_BOX:[i32;4]='+rust_array([q16(v)for v in hero_box])+';')
    lines.append('pub const GEO_PARAMS:Params=Params{pickup_ticks:15,wallet_max:9999999,coins:[')
    for c in coins:
        scale=c['scale'];b=c['body'];offset=[q16(b['m_Offset'][k]*scale)for k in 'xy'];half=[q16(b['m_Size'][k]*scale/2)for k in 'xy']
        lines.append('CoinSpec{value:'+str(c['value'])+',gravity:'+str(q16(gravity*c['rigidbody']['m_GravityScale']))+',body_offset:'+rust_array(offset)+',half:'+rust_array(half)+',pickup_offset:'+rust_array(offset)+',pickup_half:'+rust_array(half)+',bounce:'+str(q16(c['bounce']['bounceFactor']))+',threshold:'+str(q16(c['bounce']['speedThreshold']))+',friction:'+str(q16(c['material']['friction']))+'},')
    coin_frames=max(clips[c['clips'][name]]['start']+clips[c['clips'][name]]['count']for c in coins for name in ('idleAnim','airAnim'))
    lines.append(']};\n#[derive(Clone,Copy)] pub struct GeoArt{pub u:u8,pub v:u8,pub w:u8,pub h:u8,pub clut:u16,pub tpage:u16}\npub const GEO_ART:&[GeoArt]=&[')
    for a,tid in zip(art,mapping):
        t=textures[tid];lines.append('GeoArt{'+','.join(f'{k}:{t[k]}'for k in ('u','v','w','h','clut','tpage'))+'},')
    # Only coins are drawn from their art box; a rock's quad is its own, in its scene's bank.
    lines.append('];\npub const GEO_COIN_BOUNDS:&[[i32;4]]=&[')
    for a in art[:coin_frames]:lines.append(rust_array([q16(v)for v in a['box']])+',')
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
    lines.append('];\n');return '\n'.join(lines)

def scene_art(banks):
    """Per scene: ({art index: where the bank put it}, [the rectangles to upload when the scene's bank is admitted])."""
    out={}
    for scene,(blob,uploads,textures,mapping,sheet,entries) in banks.items():
        where={entry:{k:textures[mapping[n]][k]for k in ('u','v','w','h','clut','tpage')}for n,entry in enumerate(entries)}
        rows=[{'x':u['x'],'y':u['y'],'w':u['w'],'h':u['h'],'words':list(struct.unpack(f'<{u["w"]*u["h"]}H',blob[u['offset']:u['offset']+u['w']*u['h']*2]))}for u in uploads]
        out[scene]=(where,rows)
    return out

def bank_rows(rocks,bindings,metadata,banks):
    """What the scene metadata banks carry for Geo rocks (tools/world_metadata.py KIND_GEO_*).

    `rocks` by scene id (collision, state key and payout counts), and `views` by region chunk id: for each rock a region
    draws or hides things for, its art quads (tag 0 while broken, 1 while whole) and the draw and edge indices a broken
    rock removes there. Coordinates stay in world units; the encoder fixes them to Q16."""
    art=scene_art(banks);by_scene={}
    for r in rocks:
        by_scene.setdefault(r['scene'],[]).append({k:r[k]for k in ('source','state','x','y','bounds','polygons','hits','per_hit','final_payout','hit_cooldown')})
    for rows in by_scene.values():rows.sort(key=lambda row:row['state'])
    views={}
    for region,row in zip(metadata['regions'],bindings):
        out=[]
        for b in row:
            r=rocks[b['draw']];arts=[(0,r['broken_frame'],corners(r['broken_vertices'])),(1,r['intact_frame'],corners(r['intact_vertices']))]
            arts+=[(0,p['art'],corners(p['vertices']))for p in r['broken_extra']]+[(1,p['art'],corners(p['vertices']))for p in r['intact_extra']]
            out.append({'source':r['source'],'state':b['state'],'arts':arts,'off':b['off'],'edges':b['edges']})
        if out:views[str(region['chunk_id'])]=out
    return {'rocks':{str(k):v for k,v in by_scene.items()},'views':views,'uploads':{str(k):rows for k,(where,rows)in art.items()}}

def bind_regions(rocks,metadata):
    bindings=[]
    for region in metadata['regions']:
        scene=json.loads((ROOT/region.get('base_path',f'data/regions/region-{region["chunk_id"]:03}/room.hk')).with_name('scene.json').read_text());row=[]
        for draw,r in enumerate(rocks):
            off=[i for i,d in enumerate(scene['draws'])if d['source']in r['draw_sources']]
            sources=region['edge_sources'];edges=[i for i,e in enumerate(sources)if (e if isinstance(e,str)else e.get('source'))in r['collider_sources']]
            b=region['interaction_bounds'];v=r['intact_vertices']+r['broken_vertices']+[p for k in ('intact_extra','broken_extra')for part in r[k]for p in part['vertices']]
            visible=region['scene_id']==r['scene'] and min(p[0]for p in v)<=b[2] and max(p[0]for p in v)>=b[0] and min(p[1]for p in v)<=b[3] and max(p[1]for p in v)>=b[1]
            if off or edges or visible:row.append({'state':r['state'],'draw':draw,'off':off,'edges':edges,'intact_extra':r['intact_extra'],'broken_extra':r['broken_extra']})
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
    source=Source();cil=source_cil_evidence(source);rocks,enemies,coins,art,clips,unsupported_rocks,banks,coin_bank=collect(source)
    blob,uploads,textures,mapping,sheet,_=coin_bank
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
    rust=generate(rocks,enemies,coins,art[:len(mapping)],clips,textures,mapping,uploads,hero[0][0],gravity)
    out=ROOT/'data';out.mkdir(exist_ok=True);(out/'geo.hk').write_bytes(blob);(out/'geo.rs').write_text(rust)
    report={'format':'HKGEO01','region_metadata_sha256':hashlib.sha256((ROOT/'data/regions.json').read_bytes()).hexdigest(),'code_sha256':{n:hashlib.sha256((ROOT/n).read_bytes()).hexdigest()for n in ('host/geo.py','host/source.py','host/cook.py','host/materials.py','host/scene.py','host/focus.py')},'cil_evidence':cil,'previously_omitted_rock_art':True,'rocks':rocks,'unsupported_rocks':unsupported_rocks,'enemies':enemies,'coins':coins,'clips':clips,'art':[{k:v for k,v in a.items()if k!='image'}|{'texture':tid}for a,tid in zip(art,mapping)],'rock_banks':{str(scene):{'uploads':[{k:v for k,v in u.items()}for u in bank[1]],'bytes':len(bank[0])}for scene,bank in banks.items()},'textures':textures,'uploads':uploads,'bindings':bindings,'bank':bank_rows(rocks,bindings,metadata,banks),'hero_box':hero,'gravity':gravity,
        'vram_bytes':len(blob),'vram_limit':MAX_VRAM_BYTES,'permitted_rectangles':VRAM_RECTS,'payload_sha256':hashlib.sha256(blob).hexdigest(),'rust_sha256':hashlib.sha256(rust.encode()).hexdigest(),
        'source_files':{str(Path(n).relative_to(source.directory)):hashlib.sha256(Path(n).read_bytes()).hexdigest()for n in sorted({str(source.directory/name)for name in source.files}|{str(source.directory/'Managed/Assembly-CSharp.dll'),str(source.directory/'Managed/PlayMaker.dll')}|{str(p)for name in source.files for p in source.directory.glob(name+'*')if p.is_file()})if Path(n).is_file()},
        'limitations':['Source colors quantized jointly to15 visible colors; source alpha approximated by ordered0/half/full coverage.', 'Coin images sampled at current camera projection; no scenery/room changes or gameplay disc reads.', 'GeoControl Get clip is unused in the observed collection handler and is not cooked.', 'Rock gleam, hit jitter, debris and acid reactions are recorded as incomplete runtime effects.']}
    dest=ROOT/'.hkpsx/geo-source';dest.mkdir(exist_ok=True);(dest/'report.json').write_text(json.dumps(report,indent=2));(ROOT/'.hkpsx/geo-provenance.json').write_text(json.dumps(report,indent=2));sheet.save(dest/'art.png')
    print(f'Geo: {len(rocks)} rocks, {len(enemies)} enemy payouts, {len(art)} frames/{len(textures)} textures, {len(blob)} VRAM bytes')
    return report

if __name__=='__main__':cook()
