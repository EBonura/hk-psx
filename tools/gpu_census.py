#!/usr/bin/env python3
"""Rank static scenery draws by estimated GPU raster cost along a replay.

Uses the emulator's silicon-calibrated per-pixel model (bus cycles), shared with
tools/frame_packets.py so the two tools cannot drift: textured 2.80/px, flat
opaque 0.535/px. Those rates and the two 128-pixel scissor payoff thresholds
below have no host authority; they are pinned to the emulator raster source
recorded in host/opaque_tiles.py::RASTER_SOURCE.

A draw is billed flat only where host/opaque_groups.py would admit it as an
immutable flat opaque Tilemap quad. Every other draw is billed textured.

That admission is a near neighbour of the renderer's own flat path and not the
same test. `game/src/render.rs` emits `write_black` for a draw whose texture is
solid black, at full opacity, carrying BLACK_AVERAGE; it does not ask whether
the draw is a front draw or whether its source is a tilemap rect, and this has
no opacity to ask about. Today that difference is the whole difference: 2174 of
the 10005 draws a bare tilemap test would bill flat are refused here, and every
one of them is refused for being a back draw. Until something owns the raster
question itself, this bills them textured, which is the direction that does not
quietly understate the frame.

Cameras come from a profile's route/gpu logs, so the estimate can be checked
against the emulator's own per-frame gpu_cycles. Assumes every static draw
visible; dynamic sprites are not counted.
"""
import argparse,csv,json,math,struct,sys,collections
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'host'));sys.path.insert(0,str(ROOT/'tools'))
from region_delta import layout
from opaque_groups import flat_opaque_record,solid_word_one
from opaque_tiles import mutable_draws
from frame_packets import TEXTURED,FLAT
def area(r):return max(0,r[2]-r[0])*max(0,r[3]-r[1])
def clip(v):return(max(0,min(x for x,y in v)),max(0,min(y for x,y in v)),min(320,max(x for x,y in v)),min(240,max(y for x,y in v)))
def projection(q,c):
    px=((c[0]>>8)*q['scale'])>>12;py=((c[1]>>8)*q['scale'])>>12
    return[(160+((q['xy'][i*2]-px)>>8),120-((q['xy'][i*2+1]-py)>>8))for i in range(4)]
def axis(v):return v[0][1]==v[1][1]and v[0][0]==v[2][0]and v[1][0]==v[3][0]and v[2][1]==v[3][1]
def scissored(v,w,h,cover):
    box=clip(v)
    if not axis(v)or not cover or cover[0]>4 or cover[0]==0 or not area(box):return area(box)
    dx=v[1][0]-v[0][0];dy=v[2][1]-v[0][1]
    if not dx or not dy or area(box)<=128:return area(box)
    if cover[0]==1 and tuple(cover[4:8])==(0,0,w-1,h-1):return area(box)
    def interval(a,b,n,low,high,screen):
        lo=max(0,min(a,b));hi=min(screen,max(a,b));step=math.trunc((1 if b>a else-1)*(n-1)*4096/abs(b-a));seed=(n-1 if b<a else 0)*4096+2048
        good=[x for x in range(lo,hi)if low<=(seed+(x-min(a,b))*step)>>12<high]
        return(good[0],good[-1]+1)if good else(0,0)
    out=0
    for i in range(cover[0]):
        x,y,ww,hh=cover[4+i*4:8+i*4];l,r=interval(v[0][0],v[1][0],w,x,x+ww+1,320);t,b=interval(v[0][1],v[2][1],h,y,y+hh+1,240)
        if l<r and t<b:out+=(r-l)*(b-t)
    return out if area(box)-out>max(128,128*max(0,cover[0]-1))else area(box)
def mutated_sources(meta):
    """Every draw source the static renderer mutates, per host/opaque_tiles.py.

    A source pooled into several regions is mutable everywhere once any region
    mutates it, which is why this is one whole-world set rather than a per-region
    one; reading the source names back costs one pass over the scene metadata.
    """
    geo=json.load(open(ROOT/'.hkpsx/geo-provenance.json'));life=json.load(open(ROOT/'.hkpsx/lifeblood-provenance.json'))
    out=set()
    for index,ids in enumerate(mutable_draws(meta,geo,life)):
        if not ids:continue
        chunk=meta['regions'][index]['chunk_id']
        source=json.load(open(ROOT/f'data/regions/region-{chunk:03}/scene.json'))['draws']
        out.update(source[i]['source'] for i in ids)
    return out
def load(row,mutated):
    raw=Path(ROOT/row['path']).read_bytes();counts,prefix,pages,stream=layout(raw)
    source=json.load(open(ROOT/f"data/regions/region-{row['chunk_id']:03}/scene.json"))['draws'];draws=[]
    # solid_word_one reads a pack's own texture table, palettes and pages; in an
    # HKROOM02 room those three sit where region_delta.layout says they do, so
    # the cooker's solid-black test runs here unchanged.
    sections={'textures':{'offset':40},'palettes':{'offset':prefix},'pages':{'offset':pages}};solid={}
    for i in range(counts[2]):
        t,front,scale,*f=struct.unpack_from('<HHi8i4B',raw,40+counts[1]*16+i*44)
        page,u,v,w,h,pal,offset=struct.unpack_from('<6HI',raw,40+t*16)
        cov=raw[stream+offset:stream+offset+20]if page!=65535 and struct.unpack_from('<I',raw,36)[0]&1 else None
        flat=flat_opaque_record(source[i],front,scale,f[:8],f[11],mutated)
        if flat:
            if t not in solid:solid[t]=solid_word_one(raw,sections,t)
            flat=solid[t]
        draws.append(dict(id=i,front=bool(front),scale=scale,xy=f[:8],w=w,h=h,cover=cov,source=source[i]['source'],name=source[i]['name'],tilemap='tilemap_rect'in source[i],flat=flat,black_average=f[11]==1,texture=t,empty=page==65535,tint=tuple(f[8:11])))
    return draws
def frame_cost(draws,camera):
    c=[round(camera[0]*65536),round(camera[1]*65536)];out=[]
    for d in draws:
        if d['empty']:continue
        v=projection(d,c);px=scissored(v,d['w'],d['h'],d['cover'])
        if not px:continue
        rate=FLAT if d['flat'] else TEXTURED
        out.append((px*rate,px,d))
    return out
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--profile',type=Path,help='profile/replay output dir with route.csv, gpu.csv and replay.json or command.json')
    p.add_argument('--camera',type=float,nargs=2);p.add_argument('--region',type=int,default=1,help='HK_REGION_ID (1-based)');p.add_argument('--top',type=int,default=25);p.add_argument('--every',type=int,default=10)
    a=p.parse_args();metadata=json.load(open(ROOT/'data/regions.json'));meta=metadata['regions'];cache={}
    mutated=mutated_sources(metadata)
    def region(i):
        if i not in cache:cache[i]=load(meta[i-1],mutated)
        return cache[i]
    if a.camera:
        costs=sorted(frame_cost(region(a.region),a.camera),reverse=True,key=lambda x:x[0]);total=sum(c for c,_,_ in costs)
        print(f'region {a.region} camera {a.camera}: {len(costs)} draws, {total:,.0f} estimated bus cycles = {total/33868.8:.1f} ms')
        for c,px,d in costs[:a.top]:print(f"  {c:9,.0f} cyc {px:7,d} px {'front' if d['front'] else 'back '} {'FLAT ' if d['flat'] else ('avg' if d['black_average'] else 'tex')} scale {d['scale']/4096:6.2f} {d['w']}x{d['h']} {d['name']} [{d['source']}]")
        return
    watches=None
    for name in ('replay.json','command.json'):
        if (a.profile/name).exists():watches=json.load(open(a.profile/name))['watches'];break
    col=lambda n:'ram_'+watches[n][2:]
    rows=list(csv.DictReader(open(a.profile/'route.csv')));gpu={int(r['route_tick']):r for r in csv.DictReader(open(a.profile/'gpu.csv'))}
    frames=[];cur=None
    for r in rows:
        t=int(r['route_tick']);g=gpu.get(t)
        if g and g['display_start_changed']=='1':
            if cur:frames.append(cur)
            cur=dict(tick=t,cycles=0,quads=0,x=int(r[col('HK_PLAYER_X')]),y=int(r[col('HK_PLAYER_Y')]),region=int(r[col('HK_REGION_ID')]))
        if cur and g:cur['cycles']+=int(g['gpu_cycles']);cur['quads']+=int(g['textured_quads'])
    frames=[f for f in frames if f['region']>0 and f['x']<2**31]
    by_source=collections.Counter();by_name=collections.Counter();pairs=[];n=0
    for f in frames[::a.every]:
        m=meta[f['region']-1];x=f['x']/65536;y=f['y']/65536
        cam=(min(max(x,m['camera_x'][0]),m['camera_x'][1]),min(max(y+2,m['camera_y'][0]),m['camera_y'][1]))
        costs=frame_cost(region(f['region']),cam);est=sum(c for c,_,_ in costs);pairs.append((est,f['cycles']));n+=1
        for c,px,d in costs:by_source[(f['region'],d['id'],d['name'],d['front'],d['flat'])]+=c
    if pairs:
        import statistics
        ratio=[e/max(g,1) for e,g in pairs];print(f'{n} sampled frames: estimate/emulator ratio mean {statistics.mean(ratio):.3f} median {statistics.median(ratio):.3f}; emulator mean {statistics.mean(g for _,g in pairs):,.0f} cyc/frame, estimate mean {statistics.mean(e for e,_ in pairs):,.0f}')
    total=sum(by_source.values())
    print(f'top draws by summed estimated cost over sampled frames (share of static estimate):')
    for (reg,i,name,front,flat),c in by_source.most_common(a.top):print(f"  {c*100/total:5.1f}%  region {reg:3d} draw {i:3d} {'front' if front else 'back '} {'FLAT' if flat else 'tex '} {name}")
    byname=collections.Counter()
    for (reg,i,name,front,flat),c in by_source.items():byname[name.rstrip('0123456789 ()')]+=c
    print('by name family:');[print(f'  {c*100/total:5.1f}%  {k}') for k,c in byname.most_common(a.top)]
if __name__=='__main__':main()
