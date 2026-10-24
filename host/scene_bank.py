"""HKSCNE01: lossless per-scene record/texture banks with a global4bpp atlas.

Local draw/frame/clip/edge order is preserved. Only texture IDs, atlas positions,
palette IDs and stream offsets change. No source texture is resampled here.
All emitted content stays local build data; this module never writes discs.
"""
import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def rel(path):
    """Checkout-relative when inside it (see source.rel)."""
    p=Path(path).resolve()
    return str(p.relative_to(ROOT)) if p.is_relative_to(ROOT) else str(path)
sys.path.insert(0,str(ROOT/'tools'))
from audit_resident_bank import dense_pack,aligned,Pool
from region_delta import layout,textures,read_row,write_row,compressed
from alpha_covers import record as alpha_record
MAGIC=b'HKSCNE01'
HEADER_BYTES=128
ROOM_BYTES=40
SECTIONS=('textures','draws','frames','clips','edges','palettes','pages','stream','rooms','refs')


def digest(b):return hashlib.sha256(b).hexdigest()
def canonical(raw):
    c,p,ps,ss=layout(raw);blobs=textures(raw)
    return [(struct.unpack_from('<H',raw,40+i*16)[0]==65535,b)for i,b in enumerate(blobs)]


def texel_words(blob):
    w,h=struct.unpack_from('<HH',blob);stride=(w+1)//2;palette=struct.unpack_from('<16H',blob,4);pixels=blob[36:]
    out=bytearray(blob[:4])
    for y in range(h):
        for x in range(w):out.extend(struct.pack('<H',palette[(pixels[y*stride+x//2]>>((x&1)*4))&15]))
    return bytes(out)


def joint_refine(codes, words):
    """Return <=16-state joint raster and its(oldindex,newword) alphabet."""
    if len(codes)!=len(words):raise ValueError('Mismatched joint raster')
    symbols={};pairs=[];out=bytearray()
    for old,word in zip(codes,words):
        pair=(old,word)
        if pair not in symbols:
            if len(symbols)==16:return None
            symbols[pair]=len(symbols);pairs.append(pair)
        out.append(symbols[pair])
    return bytes(out),pairs


def shared_planes(tagged_blobs, enabled=False):
    """Share static indexplanes, keeping independent CLUTs and alpha covers.

    Strict binary-black masks bypass reindexing: their special opacity CLUT
    interprets fixed source indices. Animated textures also bypass this path.
    Every other shared plane retains each member's complete16bit sampled word.
    """
    buckets={};clusters=[]
    for index,tagged in enumerate(tagged_blobs):
        if tagged[0]:continue
        blob=tagged[1:];w,h=struct.unpack_from('<HH',blob)
        black=blob[4:36]==struct.pack('<16H',0,1,*([0x8000]*14))
        raw_words=texel_words(blob)[4:];words=struct.unpack('<'+'H'*(len(raw_words)//2),raw_words)
        record={'id':index,'blob':blob,'words':words,'black':black,'colours':len(set(words))}
        buckets.setdefault((w,h),[]).append(record)
    for dimensions,records in buckets.items():
        local=[]
        for record in sorted(records,key=lambda r:(-r['colours'],r['id'])):
            found=None
            if enabled and not record['black']:
                for i,c in enumerate(local):
                    if c['members'][0]['black']:continue
                    union=joint_refine(c['codes'],record['words'])
                    if union is None:continue
                    codes,pairs=union
                    palettes=[tuple(p[old]for old,_ in pairs)for p in c['palettes']]
                    palettes.append(tuple(word for _,word in pairs))
                    found=(i,{'members':c['members']+[record],'codes':codes,'palettes':palettes});break
            if found is not None:local[found[0]]=found[1]
            else:
                codes,pairs=joint_refine(bytes(len(record['words'])),record['words'])
                local.append({'members':[record],'codes':codes,'palettes':[tuple(word for _,word in pairs)]})
        clusters.extend(local)
    mappings={};planes=[];proof=[]
    for cluster in clusters:
        members=cluster['members'];first=members[0];w,h=struct.unpack_from('<HH',first['blob'])
        plane_id=len(planes)
        # Keep singleton bytes literally unchanged, including unusedCLUTwords.
        # This also keeps every strict binary-mask index/CLUT invariant exact.
        if len(members)==1:
            pixels=first['blob'][36:];mappings[first['id']]=(plane_id,first['blob'][4:36])
        else:
            pixels=bytearray(((w+1)//2)*h)
            for i,index in enumerate(cluster['codes']):pixels[(i//w)*((w+1)//2)+(i%w)//2]|=index<<(((i%w)&1)*4)
            for member,palette in zip(members,cluster['palettes']):
                if tuple(palette[index]for index in cluster['codes'])!=member['words']:raise ValueError('Joint plane changed sampled texels')
                mappings[member['id']]=(plane_id,struct.pack('<16H',*(palette+(0,)*(16-len(palette)))))
            proof.append({'texture_ids':[m['id']for m in members],'source_sha256':[digest(m['blob'])for m in members],
                'dimensions':[w,h],'joint_symbols':len(cluster['palettes'][0]),'saved_texel_bytes':((w+1)//2)*h*(len(members)-1),'plane_sha256':digest(pixels)})
        planes.append((w,h,bytes(pixels)))
    return planes,mappings,proof


def resize_static(blob, cap):
    """Explicit indexed nearest sampling; exact palette and source mask retained.

    Only textures whose original cooked maximum dimension is48 are candidates.
    Pixel centers map floor((2*x+1)*old/(2*new)); no requantization or RGBA blend.
    """
    w,h=struct.unpack_from('<HH',blob)
    black=blob[4:36]==struct.pack('<16H',0,1,*([0x8000]*14))
    if cap==48 or max(w,h)!=48 or black:return blob
    if not 1<=cap<48:raise ValueError('Invalid scenery cap')
    nw=max(1,(w*cap+47)//48);nh=max(1,(h*cap+47)//48)
    pixels=blob[36:];oldstride=(w+1)//2;newstride=(nw+1)//2;out=bytearray(newstride*nh)
    for y in range(nh):
        sy=min(h-1,(2*y+1)*h//(2*nh))
        for x in range(nw):
            sx=min(w-1,(2*x+1)*w//(2*nw));v=(pixels[sy*oldstride+sx//2]>>((sx&1)*4))&15
            out[y*newstride+x//2]|=v<<((x&1)*4)
    return struct.pack('<HH',nw,nh)+blob[4:36]+out


# --- Draw-order-aware atlas placement -------------------------------------
#
# The GPU pays for every texture page change: the first primitive drawn after a
# page switch refills the texture cache in proportion to its pixels (hwtest
# v2.1, mirrored in the emulator's cost model). Which textures share a page is
# free to change: `dense_pack` only minimises page count, in area order, so
# textures drawn one after another land on unrelated pages. This packs the same
# planes into no more pages, but puts textures the renderer draws in sequence
# on the same page. Positions are data; the sampled texels are unchanged.
SAMPLES_X=6
SAMPLES_Y=4
SCREEN_W=320
SCREEN_H=240
PAGE_TEXELS=256*256


def draw_sequences(raw,camera):
    """Per camera sample, the room's visible static draws in render order.

    Yields lists of (local texture, estimated visible pixels). The renderer
    walks the draw table once per pass, background pass first, skipping the
    draws of the other pass; a draw is visible when its projected quad overlaps
    the 320x240 screen. Occlusion is ignored: it removes some draws from the
    run but never reorders it.
    """
    counts,prefix,page_start,stream_start=layout(raw)
    draw_at=40+counts[1]*16
    draws=[]
    for i in range(counts[2]):
        rec=raw[draw_at+i*44:draw_at+(i+1)*44]
        texture,front,scale=struct.unpack_from('<HHI',rec)
        xy=struct.unpack_from('<8i',rec,8)
        draws.append((texture,front,scale,xy))
    (x0,x1),(y0,y1)=camera
    seqs=[]
    for sy in range(SAMPLES_Y):
        cy=y0+(y1-y0)*sy/max(1,SAMPLES_Y-1)
        for sx in range(SAMPLES_X):
            cx=x0+(x1-x0)*sx/max(1,SAMPLES_X-1)
            visible=[[],[]]
            for texture,front,scale,xy in draws:
                zoom=scale/4096
                px=[160+xy[k*2]/256-cx*zoom for k in range(4)]
                py=[120-xy[k*2+1]/256+cy*zoom for k in range(4)]
                lo_x,hi_x=min(px),max(px);lo_y,hi_y=min(py),max(py)
                if hi_x<=0 or lo_x>=SCREEN_W or hi_y<=0 or lo_y>=SCREEN_H:continue
                box=(hi_x-lo_x)*(hi_y-lo_y)
                clipped=(min(hi_x,SCREEN_W)-max(lo_x,0))*(min(hi_y,SCREEN_H)-max(lo_y,0))
                # Shoelace area over the bounding box gives the fill ratio of a
                # rotated quad; an axis-aligned one is 1.
                ring=(0,1,3,2)
                a=abs(sum(px[ring[j]]*py[ring[(j+1)%4]]-px[ring[(j+1)%4]]*py[ring[j]]for j in range(4)))/2
                fill=min(1.0,a/box)if box>0 else 0.0
                visible[1 if front else 0].append((texture,clipped*fill))
            seqs.append(visible[0]+visible[1])
    return seqs


def transition_weights(inputs,maps,plane_of,cameras):
    """{(plane a, plane b): pixels} summed over every consecutive visible pair.

    The weight of a pair is the visible pixels of the second draw, which is what
    its page-change refill charges, averaged over the camera samples of its
    room. `cameras` maps a room's chunk id to ((x0, x1), (y0, y1)); a room
    without one contributes nothing.
    """
    weights={}
    for (chunk,raw),mapping in zip(inputs,maps):
        camera=cameras.get(chunk)
        if camera is None:continue
        counts=layout(raw)[0]
        if counts[2]==0:continue
        runs=draw_sequences(raw,camera)
        share=1.0/len(runs)
        for run in runs:
            previous=None
            for texture,pixels in run:
                plane=plane_of(mapping[texture])
                if plane is None:continue
                if previous is not None and previous!=plane:
                    key=(min(previous,plane),max(previous,plane))
                    weights[key]=weights.get(key,0.0)+pixels*share
                previous=plane
    return weights


def pack_one_page(rectangles):
    """dense_pack's MaxRects restricted to one page: {index: (x, y)} or None.

    Same ordering and scoring as dense_pack, so a set it accepts is a set
    dense_pack puts on one page. The overlap audit is left to the caller.
    """
    free=[(0,0,256,256)];placed={}
    for width,height,index in sorted(rectangles,key=lambda r:(r[0]*r[1],max(r[:2]),r[2]),reverse=True):
        best=None
        for x,y,w,h in free:
            if width<=w and height<=h:
                key=(min(w-width,h-height),max(w-width,h-height),0,y,x)
                if best is None or key<best[0]:best=(key,x,y)
        if best is None:return None
        _,x,y=best;placed[index]=(x,y);changed=[]
        for fx,fy,fw,fh in free:
            if x>=fx+fw or x+width<=fx or y>=fy+fh or y+height<=fy:
                changed.append((fx,fy,fw,fh));continue
            if fx<x:changed.append((fx,fy,x-fx,fh))
            if x+width<fx+fw:changed.append((x+width,fy,fx+fw-x-width,fh))
            if fy<y:changed.append((fx,fy,fw,y-fy))
            if y+height<fy+fh:changed.append((fx,y+height,fw,fy+fh-y-height))
        free=[r for j,r in enumerate(changed)if not any(k!=j and
            q[0]<=r[0]and q[1]<=r[1]and q[0]+q[2]>=r[0]+r[2]and q[1]+q[3]>=r[1]+r[3]
            and(q!=r or k<j)for k,q in enumerate(changed))]
    return placed


def cut_weight(assign,weights):
    """Total pair weight whose two planes sit on different pages."""
    return sum(w for (a,b),w in weights.items()if assign[a]!=assign[b])


def cluster_planes(rects,weights):
    """Group planes into page-sized clusters by merging the most-linked pairs first.

    `rects` maps plane -> (width, height); `weights` is {(a, b): pixels}. Two
    clusters merge when they are linked and their union still packs on one page.
    Returns the clusters (sets of planes) once no linked pair can merge.
    """
    area={i:w*h for i,(w,h)in rects.items()}
    members={i:{i}for i in rects};size=dict(area)
    link={}
    for (a,b),w in weights.items():link[(a,b)]=link.get((a,b),0.0)+w
    fits={}
    def packs(union):
        key=frozenset(union)
        if key not in fits:fits[key]=pack_one_page([(rects[i][0],rects[i][1],i)for i in key])is not None
        return fits[key]
    refused=set()
    while True:
        merged=False
        for (a,b),w in sorted(link.items(),key=lambda kv:(-kv[1],kv[0])):
            if size[a]+size[b]>PAGE_TEXELS:continue
            pair=(frozenset(members[a]),frozenset(members[b]))
            if pair in refused or pair[::-1]in refused:continue
            if not packs(members[a]|members[b]):
                refused.add(pair);continue
            members[a]|=members.pop(b);size[a]+=size.pop(b)
            relinked={}
            for (x,y),v in link.items():
                x=a if x==b else x;y=a if y==b else y
                if x!=y:
                    key=(min(x,y),max(x,y));relinked[key]=relinked.get(key,0.0)+v
            link=relinked;merged=True;break
        if not merged:return [members[c]for c in sorted(members)],packs


MERGE_ATTEMPTS=3000


def merge_down(clusters,packs,rects,limit):
    """Join clusters, largest first, until at most `limit` remain; None if stuck.

    Gives up after MERGE_ATTEMPTS page-packing checks so a scene that cannot reach
    `limit` (one already at the page ceiling) costs a bounded amount of cook time.
    """
    clusters=[set(c)for c in clusters]
    area=lambda c:sum(rects[i][0]*rects[i][1]for i in c)
    attempts=0
    while len(clusters)>limit:
        clusters.sort(key=lambda c:(-area(c),min(c)))
        for i,a in enumerate(clusters):
            partner=None
            for b in clusters[i+1:]:
                if area(a)+area(b)>PAGE_TEXELS:continue
                attempts+=1
                if attempts>MERGE_ATTEMPTS:return None
                if packs(a|b):partner=b;break
            if partner is not None:
                a|=partner;clusters.remove(partner);break
        else:return None
    return clusters


EXTRA_PAGE_COST=0.01
# Page 18 is the quick map's (shared/hk-cache residency.rs MAP_PAGE); only a scene
# that already needs nineteen pages overwrites it, so extra pages stop at eighteen.
ORDERED_PAGE_CEILING=18
# Scene 0 (Tutorial_01) is the first scene of every new game: its load precedes the first
# frame and its art chunk follows its atlases on the disc. Two extra pages there cost 12
# ticks of boot and delayed the Knight's art past the first frames (HK_MODULE_ART_LATE 46
# in kings-death), so it is reordered within dense_pack's page count only.
BOOT_SCENE=0
MAX_EXTRA_PAGES=4


def ordered_pack(rectangles,weights,page_limit=None,max_extra=MAX_EXTRA_PAGES):
    """Like dense_pack, but planes drawn in sequence share pages.

    Candidates: dense_pack's own placement and a clustering of the most-linked
    planes, each at dense_pack's page count and up to `max_extra` pages more (the
    page budget `page_limit` still holds). An extra page must remove at least
    EXTRA_PAGE_COST of the total pair weight from the cut to be worth its upload.
    The winner is polished by moving or swapping planes while that lowers the cut.
    Deterministic. Returns (page_count, placements, report).
    """
    base_pages,base=dense_pack(rectangles)
    rects={i:(w,h)for w,h,i in rectangles}
    base_assign={i:page for i,page,x,y,w,h in base}
    total=sum(weights.values())or 1.0
    before=cut_weight(base_assign,weights)
    options=[(before,0,base_assign)]
    clusters,packs=cluster_planes(rects,weights)
    for extra in range(max_extra+1):
        limit=base_pages+extra
        if limit>max(base_pages,ORDERED_PAGE_CEILING) or(page_limit is not None and limit>page_limit):break
        grouped=merge_down(clusters,packs,rects,limit)
        if grouped is None:continue
        assign={i:k for k,group in enumerate(grouped)for i in group}
        options.append((cut_weight(assign,weights)+EXTRA_PAGE_COST*total*max(0,len(grouped)-base_pages),max(0,len(grouped)-base_pages),assign))
    score,extra,assign=min(options,key=lambda o:(o[0],o[1]))
    pages=sorted(set(assign.values()))
    layouts={page:{}for page in pages}
    for i,page in assign.items():layouts[page][i]=None
    for page in pages:
        placed=pack_one_page([(rects[i][0],rects[i][1],i)for i in layouts[page]])
        if placed is None:return base_pages,base,{'weight_cut_before':before,'weight_cut_after':before,'moved':0,'extra_pages':0,'fallback':True}
        layouts[page]=placed
    used={page:sum(rects[i][0]*rects[i][1]for i in layouts[page])for page in pages}
    adjacent={}
    for (a,b),w in weights.items():
        adjacent.setdefault(a,{})[b]=w;adjacent.setdefault(b,{})[a]=w
    cache={}
    def fit(members):
        key=frozenset(members)
        if key not in cache:cache[key]=pack_one_page([(rects[i][0],rects[i][1],i)for i in key])
        return cache[key]
    def together(p,page,skip=None):
        return sum(w for q,w in adjacent.get(p,{}).items()if assign[q]==page and q!=skip)
    order=sorted(adjacent,key=lambda p:(-sum(adjacent[p].values()),p))
    for _ in range(12):
        improved=False
        for p in order:
            home=assign[p];here=together(p,home)
            best=None
            for page in pages:
                if page==home:continue
                there=together(p,page)
                area=rects[p][0]*rects[p][1]
                if used[page]+area<=PAGE_TEXELS:
                    placed=fit(set(layouts[page])|{p})
                    if placed is not None:
                        if there-here>1e-9 and(best is None or there-here>best[0]):best=(there-here,page,None,placed,None)
                        continue
                # No room (or no packing): swap with a plane there, if the pair gains.
                for q in sorted(layouts[page]):
                    pair=adjacent.get(p,{}).get(q,0.0)
                    gain=(there-pair)-here+together(q,home,skip=p)-together(q,page)
                    if gain<=1e-9 or(best is not None and gain<=best[0]):continue
                    into=(set(layouts[page])-{q})|{p};out=(set(layouts[home])-{p})|{q}
                    if sum(rects[i][0]*rects[i][1]for i in into)>PAGE_TEXELS or sum(rects[i][0]*rects[i][1]for i in out)>PAGE_TEXELS:continue
                    placed=fit(into);other=fit(out)
                    if placed is not None and other is not None:best=(gain,page,q,placed,other)
            if best is None:continue
            gain,page,q,placed,other=best
            if q is None:
                del layouts[home][p];layouts[page]=placed;assign[p]=page
                used[home]-=rects[p][0]*rects[p][1];used[page]+=rects[p][0]*rects[p][1]
            else:
                layouts[page]=placed;layouts[home]=other;assign[p]=page;assign[q]=home
                used[page]+=rects[p][0]*rects[p][1]-rects[q][0]*rects[q][1]
                used[home]+=rects[q][0]*rects[q][1]-rects[p][0]*rects[p][1]
            improved=True
        if not improved:break
    live=[page for page in pages if layouts[page]]
    renumber={old:new for new,old in enumerate(live)}
    placements=[(i,renumber[assign[i]],x,y,rects[i][0],rects[i][1])for page in live for i,(x,y)in sorted(layouts[page].items())]
    after=cut_weight(assign,weights)
    if after>before:
        return base_pages,base,{'weight_cut_before':before,'weight_cut_after':before,'moved':0,'extra_pages':0,'fallback':True}
    moved=sum(1 for i,page,x,y,w,h in base if renumber[assign[i]]!=page)
    return len(live),placements,{'weight_cut_before':before,'weight_cut_after':after,'weight_total':total,'moved':moved,'extra_pages':len(live)-base_pages,'fallback':False}


def build_scene(scene_id, inputs, page_limit=20, scenery_cap=48, dedup_mode="blob", share_pixel_planes=False, cameras=None):
    """inputs=(global chunk_id, HKROOM02bytes), returns checked bytes+provenance.

    `cameras` maps a chunk id to its ((x0, x1), (y0, y1)) camera range. With it the
    atlas puts textures the renderer draws in sequence on the same page (ordered_pack);
    without it pages are packed by area alone."""
    if not inputs or len({i for i,_ in inputs})!=len(inputs):raise ValueError('Empty/duplicate room IDs')
    if dedup_mode not in ('blob','texel_words'):raise ValueError('Unknown texture equality')
    tex=Pool();pal=Pool();pools={n:Pool()for n in ('draws','frames','clips','edges')}
    original=[];roomrefs=[];covers={};streamed=set();changed_textures={};representatives={};word_aliases=0
    # A multi-tile animation frame is a run of consecutive streamed records
    # whose first carries the (cols, rows) grid in u,v (host/cook.py
    # Atlas.add_tiled; hk_format::Room::frame_grid). The run is one identity:
    # it is shared only as a whole, its tiles never alias a lone texture, and
    # its head keeps the grid. Without this every frame read as one tile, so
    # the guest stretched the top-left 64x64 tile over the whole frame box.
    grids={};runs={}
    for chunk,raw in inputs:
        c,p,ps,ss=layout(raw);mapping=[];local=canonical(raw);follow=0
        for i,(is_stream,blob) in enumerate(local):
            if follow:
                follow-=1;continue
            head=struct.unpack_from('<3H',raw,40+i*16)
            if is_stream and (head[1] or head[2]):
                cols,rows=head[1],head[2];n=cols*rows
                if n<2 or i+n>len(local) or not all(local[i+k][0] and struct.unpack_from('<2H',raw,42+(i+k)*16)==(0,0) for k in range(1,n)):
                    raise ValueError('Invalid animation tile run')
                tiles=tuple(local[i+k][1] for k in range(n));key=('run',cols,rows,tiles)
                if key not in runs:
                    base=len(tex.values)
                    for k,tile in enumerate(tiles):
                        tex.values.append(bytes([1])+tile);streamed.add(base+k)
                    runs[key]=base;grids[base]=(cols,rows)
                mapping.extend(range(runs[key],runs[key]+n));follow=n-1;continue
            source_blob=blob
            if not is_stream:blob=resize_static(blob,scenery_cap)
            if blob!=source_blob:changed_textures[digest(source_blob)]={'source_sha256':digest(source_blob),'output_sha256':digest(blob),'source_dimensions':list(struct.unpack_from('<HH',source_blob)),'output_dimensions':list(struct.unpack_from('<HH',blob))}
            # Storage class is part of identity; an animated texture never
            # becomes a static atlas entry merely because its pixels match.
            cover=None
            if not is_stream and struct.unpack_from('<I',raw,36)[0]&1:
                offset=struct.unpack_from('<I',raw,40+i*16+12)[0];cover=raw[ss+offset:ss+offset+20]
                if blob!=source_blob:cover=alpha_record(*struct.unpack_from('<HH',blob),blob[4:36],blob[36:])
                if len(cover)!=20:raise ValueError('Invalid alpha cover')
            black=blob[4:36]==struct.pack('<16H',0,1,*([0x8000]*14))
            key=(is_stream,black,cover,texel_words(blob)if dedup_mode=='texel_words' and not (is_stream or black)else blob)
            if key in representatives:
                tid=representatives[key]
                if tex.values[tid][1:]!=blob:word_aliases+=1
            else:
                tid=tex.add(bytes([int(is_stream)])+blob);representatives[key]=tid
            mapping.append(tid)
            if is_stream:streamed.add(tid)
            elif cover is not None:
                if tid in covers and covers[tid]!=cover:raise ValueError('Conflicting alpha covers')
                covers[tid]=cover
        at=40+c[1]*16;refs=[]
        for name,n,size in zip(('draws','frames','clips','edges'),c[2:],(44,20,16,16)):
            ids=[]
            for j in range(n):
                rec=raw[at+j*size:at+(j+1)*size]
                if name in ('draws','frames'):
                    width=2 if name=='draws' else 4;local=int.from_bytes(rec[:width],'little')
                    if local>=len(mapping):raise ValueError('Invalid texture reference')
                    rec=mapping[local].to_bytes(width,'little')+rec[width:]
                ids.append(pools[name].add(rec))
            refs.append(struct.pack('<'+'H'*len(ids),*ids));at+=n*size
        original.append({'chunk_id':chunk,'sha256':digest(raw),'counts':list(c),'texture_map':mapping})
        roomrefs.append((chunk,c,refs))
    planes,plane_map,plane_proof=shared_planes(tex.values,share_pixel_planes)
    rectangles=[(aligned(w),h,i)for i,(w,h,pixels)in enumerate(planes)]
    order_report=None
    if cameras:
        weights=transition_weights(inputs,[o['texture_map']for o in original],lambda tid:None if tid in streamed else plane_map[tid][0],cameras)
        page_count,placements,order_report=ordered_pack(rectangles,weights,page_limit,0 if scene_id==BOOT_SCENE else MAX_EXTRA_PAGES)
    else:page_count,placements=dense_pack(rectangles)
    if page_count>page_limit:raise ValueError(f'Scene{scene_id}: {page_count} pages exceeds {page_limit}; source recook required')
    positions={i:(p,x,y)for i,p,x,y,w,h in placements};pages=bytearray(page_count*32768);stream=bytearray();table=[]
    for i,tagged in enumerate(tex.values):
        blob=tagged[1:];w,h=struct.unpack_from('<HH',blob);palette=pal.add(blob[4:36]if i in streamed else plane_map[i][1]);pixels=blob[36:]
        while len(stream)%4:stream.append(0)
        offset=len(stream)
        if i in streamed:
            if w>64 or h>64:raise ValueError('Animation exceeds64x64')
            page=65535;u,v=grids.get(i,(0,0));stride=aligned(w)//2
            for y in range(h):stream.extend(pixels[y*((w+1)//2):(y+1)*((w+1)//2)]);stream.extend(bytes(stride-(w+1)//2))
        else:
            plane_id,_=plane_map[i];page,u,v=positions[plane_id];pixels=planes[plane_id][2];stride=(w+1)//2
            for y in range(h):write_row(pages,page*32768+(v+y)*128+u//2,u&1,w,pixels[y*stride:(y+1)*stride])
            # Legacy source rooms have no precomputed covers; reject rather
            # than making runtime scissoring interpret invented metadata.
            if i not in covers:raise ValueError('Static texture lacks alpha cover')
            stream.extend(covers[i])
        table.append(struct.pack('<6HI',page,u,v,w,h,palette,offset))
    if len(tex.values)>2048 or len(pal.values)>1536:raise ValueError('Global texture/CLUT bound exceeded')
    sections={'textures':b''.join(table),**{n:b''.join(pool.values)for n,pool in pools.items()},
        'palettes':b''.join(pal.values),'pages':bytes(pages),'stream':bytes(stream)}
    offsets={};at=HEADER_BYTES
    for name in SECTIONS[:8]:offsets[name]=at;at=aligned(at+len(sections[name]))
    offsets['rooms']=at;at+=ROOM_BYTES*len(roomrefs);offsets['refs']=at
    references=bytearray();descriptors=[]
    for chunk,c,refs in roomrefs:
        locations=[]
        for arr in refs:
            while len(references)%4:references.append(0)
            locations.append(at+len(references));references.extend(arr)
        descriptors.append(struct.pack('<10I',chunk,*c[2:],*locations,0))
    sections['rooms']=b''.join(descriptors);sections['refs']=bytes(references)
    total=aligned(at+len(references));out=bytearray(total);out[:8]=MAGIC
    struct.pack_into('<14I',out,8,scene_id,len(inputs),len(tex.values),*[len(pools[n].values)for n in ('draws','frames','clips','edges')],len(pal.values),page_count,len(stream),total,1,0,0)
    struct.pack_into('<10I',out,64,*(offsets[n]for n in SECTIONS))
    for name,body in sections.items():out[offsets[name]:offsets[name]+len(body)]=body
    payload=bytes(out);verify(payload,inputs,scenery_cap,dedup_mode,share_pixel_planes)
    report={'scene_id':scene_id,'bytes':len(payload),'sha256':digest(payload),'pages':page_count,'textures':len(tex.values),
      'palettes':len(pal.values),'stream_bytes':len(stream),'pools':{n:len(p.values)for n,p in pools.items()},
      'sections':{n:{'offset':offsets[n],'bytes':len(sections[n])}for n in SECTIONS},
      'source_rooms':original,'all_source_fields_and_texels_verified':True,'dedup_mode':dedup_mode,'word_exact_alias_references':word_aliases,'shared_pixel_planes':share_pixel_planes,'pixel_plane_groups':plane_proof,'static_pixel_planes':len(planes),'scenery_cap':scenery_cap,'page_order':order_report,'changed_static_textures':list(changed_textures.values()),'quality':('Dimensions, source records and sampled16bit texel words are exact; streamed texture bytes and strict-mask indices/palettes are preserved.' if not changed_textures else 'Listed static48-axis textures were resized; source records and protected masks/animation were preserved.')}
    return payload,report


def verify(bank,inputs,scenery_cap=48,dedup_mode="blob",share_pixel_planes=False):
    if len(bank)<128 or bank[:8]!=MAGIC:raise ValueError('Invalid scene header')
    scene,nroom,nt,nd,nf,nc,ne,np,npages,ns,total,flags,r0,r1=struct.unpack_from('<14I',bank,8)
    if total!=len(bank)or flags!=1 or r0 or r1 or any(bank[104:128])or nroom!=len(inputs):raise ValueError('Invalid scene layout')
    off=dict(zip(SECTIONS,struct.unpack_from('<10I',bank,64)))
    sizes=dict(zip(SECTIONS,(nt*16,nd*44,nf*20,nc*16,ne*16,np*32,npages*32768,ns,nroom*40,len(bank)-off['refs'])))
    end=128
    for name in SECTIONS:
        if off[name]!=aligned(end)or off[name]+sizes[name]>len(bank):raise ValueError('Invalid scene section')
        end=off[name]+sizes[name]
    global_textures=[]
    for i in range(nt):
        page,u,v,w,h,palette,offset=struct.unpack_from('<6HI',bank,off['textures']+i*16)
        if not w or not h or palette>=np:raise ValueError('Invalid scene texture')
        if page==65535:
            if w>64 or h>64 or (u==0)!=(v==0) or u*v==1 or u>8 or v>8 or offset%4 or offset+aligned(w)//2*h>ns:raise ValueError('Invalid animation placement')
            base=off['stream']+offset;stride=aligned(w)//2;x=0
        else:
            if page>=npages or u+w>256 or v+h>256 or offset%4 or offset+20>ns:raise ValueError('Invalid atlas placement')
            base=off['pages']+page*32768+v*128;stride=128;x=u
        pixels=b''.join(read_row(bank,base+y*stride+x//2,x&1,w)for y in range(h))
        global_textures.append((page==65535,struct.pack('<HH',w,h)+bank[off['palettes']+palette*32:off['palettes']+(palette+1)*32]+pixels))
    def equivalent(t):
        # Stream uploads and fixed-index fade masks retain their exact CLUT and
        # index bytes. Other shared planes may reindex only word-exact samples.
        protected=t[0] or t[1][4:36]==struct.pack('<16H',0,1,*([0x8000]*14))
        if protected:return(t[0],'protected',t[1])
        return(t[0],'words',texel_words(t[1]))if dedup_mode=='texel_words'or share_pixel_planes else(t[0],'bytes',t[1])
    global_keys={equivalent(t)for t in global_textures}
    for ri,(chunk,raw)in enumerate(inputs):
        desc=struct.unpack_from('<10I',bank,off['rooms']+ri*40);c,p,ps,ss=layout(raw);oldtex=canonical(raw);expected_tex=[(animated,blob if animated else resize_static(blob,scenery_cap))for animated,blob in oldtex]
        if any(equivalent(t) not in global_keys for t in expected_tex):raise ValueError('Missing or changed source texture')
        if desc[0]!=chunk or list(desc[1:5])!=list(c[2:])or desc[9]:raise ValueError('Changed room identity/count')
        at=40+c[1]*16
        for section,count,size,ref_off,limit in zip(('draws','frames','clips','edges'),c[2:],(44,20,16,16),desc[5:9],(nd,nf,nc,ne)):
            if ref_off%4 or ref_off<off['refs']or ref_off+count*2>len(bank):raise ValueError('Invalid room references')
            for i in range(count):
                rid=struct.unpack_from('<H',bank,ref_off+i*2)[0]
                if rid>=limit:raise ValueError('Invalid record reference')
                rec=bank[off[section]+rid*size:off[section]+(rid+1)*size];original=raw[at+i*size:at+(i+1)*size]
                if section in ('draws','frames'):
                    width=2 if section=='draws' else 4;a=int.from_bytes(original[:width],'little');b=int.from_bytes(rec[:width],'little')
                    if b>=nt or equivalent(expected_tex[a])!=equivalent(global_textures[b])or original[width:]!=rec[width:]:raise ValueError('Changed draw/frame content')
                    if section=='frames':
                        # A tiled frame keeps its grid and its whole run of tiles.
                        grid=struct.unpack_from('<2H',raw,42+a*16);bgrid=struct.unpack_from('<2H',bank,off['textures']+b*16+2)
                        if expected_tex[a][0] and grid!=bgrid:raise ValueError('Changed animation tile grid')
                        if expected_tex[a][0] and grid!=(0,0) and any(b+k>=nt or equivalent(expected_tex[a+k])!=equivalent(global_textures[b+k]) for k in range(grid[0]*grid[1])):raise ValueError('Changed animation tile run')
                    if section=='draws':
                        oldoff=struct.unpack_from('<I',raw,40+a*16+12)[0];newoff=struct.unpack_from('<I',bank,off['textures']+b*16+12)[0]
                        cover=raw[ss+oldoff:ss+oldoff+20]
                        if expected_tex[a]!=oldtex[a]:
                            blob=expected_tex[a][1];cover=alpha_record(*struct.unpack_from('<HH',blob),blob[4:36],blob[36:])
                        if cover!=bank[off['stream']+newoff:off['stream']+newoff+20]:raise ValueError('Changed alpha cover')
                elif original!=rec:raise ValueError('Changed clip/edge content')
            at+=count*size
    return True


def cook_scene_banks(metadata,output_dir,page_limit=20,allow_reduction=False,dedup_mode="blob",share_pixel_planes=False):
    source=metadata.read_bytes();report=json.loads(source);results=[]
    # Produce all scenes in memory before writing any final bank.
    for scene in sorted({r['scene_id']for r in report['regions']}):
        inputs=[]
        for r in report['regions']:
            if r['scene_id']!=scene:continue
            raw=(ROOT/r['path']).read_bytes()
            if digest(raw)!=r['sha256']:raise ValueError('Stale cooked source room')
            inputs.append((r['chunk_id'],raw))
        attempts=[]
        for cap in ((48,47,46,45,44)if allow_reduction else(48,)):
            try:payload,entry=build_scene(scene,inputs,page_limit,cap,dedup_mode,share_pixel_planes)
            except ValueError as exc:
                if 'pages exceeds' not in str(exc):raise
                attempts.append({'cap':cap,'error':str(exc)});continue
            entry['packing_attempts']=attempts;results.append((payload,entry));break
        else:raise ValueError('No admitted scene atlas cap fits: '+str(attempts))
    output_dir.mkdir(parents=True,exist_ok=True)
    for payload,entry in results:
        path=output_dir/f'scene_{entry["scene_id"]}.hk';path.write_bytes(payload);entry['path']=rel(path)
        stored=compressed(payload);stored_path=output_dir/f'scene_{entry["scene_id"]}.hlzc';stored_path.write_bytes(stored)
        entry.update(stored_path=rel(stored_path),stored_bytes=len(stored),stored_sha256=digest(stored),stored_fnv=fnv(stored),raw_fnv=fnv(payload))
    result={'format':'HKSCNE01','source_metadata_sha256':digest(source),'cooker_sha256':digest(Path(__file__).read_bytes()),'scenes':[e for _,e in results]}
    (output_dir/'report.json').write_text(json.dumps(result,indent=2)+'\n')
    (output_dir/'scene_manifest.rs').write_text(manifest(result,report))
    return result


def fnv(data):
    value=0x811c9dc5
    for byte in data:value=((value^byte)*0x01000193)&0xffffffff
    return value


def manifest(result,metadata):
    scenes=result['scenes'];indices={s['scene_id']:i for i,s in enumerate(scenes)}
    local={r['chunk_id']:(indices[s['scene_id']],i)for s in scenes for i,r in enumerate(s['source_rooms'])}
    rows=sorted(metadata['regions'],key=lambda r:r['chunk_id'])
    if [r['chunk_id']for r in rows]!=list(range(1,len(rows)+1)):raise ValueError('Noncontiguous global room IDs')
    text='// Generated HKSCNE01 scene chunks; source-derived content stays in local banks.\n'
    text+='pub const SCENE_MANIFEST:&[(usize,u32,usize,u32)]=&[\n'
    text+=''.join(f'({s["stored_bytes"]},{s["stored_fnv"]},{s["bytes"]},{s["raw_fnv"]}),\n'for s in scenes)+'];\n'
    text+='pub const REGION_SCENE_LOCAL:&[(u8,u8)]=&[\n'+''.join(f'({local[r["chunk_id"]][0]},{local[r["chunk_id"]][1]}),\n'for r in rows)+'];\n'
    text+=f'pub const SCENE_MAX_RAW_BYTES:usize={max(s["bytes"]for s in scenes)};\n'
    text+=f'pub const SCENE_ALL_RAW_BYTES:usize={sum(aligned(s["bytes"])for s in scenes)};\n'
    return text


def compact_resident(raw):
    """HKSCNE02: remove atlas bytes, preserving every logical runtime record.

    Absolute pool/reference offsets are relocated once by the host. Runtime
    accessors stay direct slices, with no per-record indirection or decoding.
    The original HKSCNE01 bank remains the texture/provenance input.
    """
    if len(raw)<HEADER_BYTES or raw[:8]!=MAGIC or struct.unpack_from('<I',raw,48)[0]!=len(raw):
        raise ValueError('Expected complete HKSCNE01 source')
    offsets=struct.unpack_from('<10I',raw,64)
    start,end=offsets[5],offsets[7]
    palettes,pages=struct.unpack_from('<2I',raw,36)
    if not HEADER_BYTES<=start<=end<=len(raw) or end-start!=palettes*32+pages*32768:
        raise ValueError('Invalid source atlas extent')
    gap=end-start
    out=bytearray(raw[:start]+raw[end:]);out[:8]=b'HKSCNE02'
    struct.pack_into('<I',out,48,len(out))
    relocated=[old if old<=start else max(start,old-gap)for old in offsets]
    struct.pack_into('<10I',out,64,*relocated)
    rooms=struct.unpack_from('<I',raw,12)[0]
    for i in range(rooms):
        at=relocated[8]+i*ROOM_BYTES+20
        refs=struct.unpack_from('<4I',out,at)
        if any(r<end or r>len(raw)for r in refs):raise ValueError('Invalid source reference offset')
        struct.pack_into('<4I',out,at,*(r-gap for r in refs))
    return bytes(out)


ATTRIBUTE_STRIDE=12

def with_texture_attributes(resident,flags,black,opaque):
    """Append the per-texture attribute section to a compact resident bank.

    flags: the packed two-bit list from texture_flags(); black/opaque: one
    [x,y,w,h] core per texture. The offset word at 104 points at the section.
    """
    if resident[:8]!=b'HKSCNE02' or struct.unpack_from('<I',resident,104)[0]!=0:raise ValueError('Expected a compact bank without attributes')
    textures=struct.unpack_from('<I',resident,16)[0]
    if len(black)!=textures or len(opaque)!=textures:raise ValueError('Texture attribute count mismatch')
    out=bytearray(resident);out.extend(b'\0'*(-len(out)%4));start=len(out)
    for i in range(textures):
        flag=(flags[i//4]>>((i%4)*2))&3
        out.extend(bytes([flag,0,0,0,*black[i],*opaque[i]]))
    out.extend(b'\0'*(-len(out)%4))
    struct.pack_into('<I',out,104,start);struct.pack_into('<I',out,48,len(out))
    return bytes(out)


def bootstrap_atlases(raw,entry):
    """Page-sized immutable upload chunks with scene-local destination IDs."""
    s=entry['sections'];out=[]
    for kind,name,stride in((0,'pages',32768),(1,'palettes',32)):
        section=s[name];payload=raw[section['offset']:section['offset']+section['bytes']]
        if len(payload)!=section['bytes'] or len(payload)%stride:raise ValueError('Invalid atlas payload')
        for first in range(0,len(payload)//stride,32768//stride):
            chunk=payload[first*stride:(first+32768//stride)*stride]
            out.append((dict(kind=kind,first=first,count=len(chunk)//stride),chunk))
    return out


def texture_flags(raw,entry):
    """Two exact facts per final texture: binary mask palette, solid word1.

    Streamed animation IDs have neither flag. Padding is zero and each bank
    is word-aligned; no raw atlas/palette reads are needed during activation.
    """
    s=entry['sections'];out=bytearray((entry['textures']+15)//16*4)
    for i in range(entry['textures']):
        page,u,v,w,h,pal,_=struct.unpack_from('<6HI',raw,s['textures']['offset']+i*16)
        if page==65535:continue
        palette=struct.unpack_from('<16H',raw,s['palettes']['offset']+pal*32)
        binary=palette==(0,1,*([0x8000]*14))
        base=s['pages']['offset']+page*32768
        solid=all(palette[(raw[base+(v+y)*128+(u+x)//2]>>(((u+x)&1)*4))&15]==1
                  for y in range(h)for x in range(w))
        out[i//4]|=(int(binary)|(int(solid)<<1))<<((i%4)*2)
    return list(out)


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--metadata',type=Path,default=ROOT/'data/regions.json');p.add_argument('--output',type=Path,required=True);p.add_argument('--page-limit',type=int,default=20);p.add_argument('--allow-reduction',action='store_true');p.add_argument('--dedup-mode',choices=('blob','texel_words'),default='blob');p.add_argument('--share-pixel-planes',action='store_true');a=p.parse_args()
    result=cook_scene_banks(a.metadata,a.output,a.page_limit,a.allow_reduction,a.dedup_mode,a.share_pixel_planes)
    print(json.dumps([{k:s[k]for k in ('scene_id','bytes','pages','textures','palettes','stream_bytes','sha256')}for s in result['scenes']],indent=2))
if __name__=='__main__':main()
