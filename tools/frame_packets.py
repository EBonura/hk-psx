#!/usr/bin/env python3
"""Cost census of the last built frame from a headless run's RAM dump.

Parses the renderer's static packet pool (addresses from the link map), applies
the emulator's silicon-calibrated per-pixel model (bus cycles) and names each
textured packet through the shared scene bank's texture table. With
--potential it also samples every axis-aligned textured packet's texels to
measure exact savings available from flat solid-black cores, transparent
texels and pixels hidden under later opaque black.
"""
import argparse,json,struct,sys,collections
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
TEXTURED=179/64;FLAT=137/256;FLAT_SEMI=51/64
def symbols(map_path):
    out={}
    for line in open(map_path):
        s=line.split()
        try:
            if len(s)>=5:out[s[4]]=(int(s[0],16),int(s[2],16))
        except ValueError:pass
    return out
def tri_pixels(a,b,c,clip):
    (x0,y0),(x1,y1),(x2,y2)=sorted([a,b,c],key=lambda p:p[1]);n=0
    for y in range(max(y0,clip[1]),min(y2,clip[3])):
        xs=[]
        for (ax,ay),(bx,by) in (((x0,y0),(x2,y2)),((x0,y0),(x1,y1)),((x1,y1),(x2,y2))):
            if ay<=y<by or by<=y<ay:xs.append(ax+(bx-ax)*(y+0.5-ay)/(by-ay))
        if len(xs)>=2:
            l=max(clip[0],int(min(xs)+0.5));r=min(clip[2],int(max(xs)+0.5));n+=max(0,r-l)
    return n
def quad_pixels(v,clip):return tri_pixels(v[0],v[1],v[2],clip)+tri_pixels(v[1],v[3],v[2],clip)
def vert(w):
    x=w&0xffff;y=(w>>16)&0xffff;return(x-65536 if x>=32768 else x,y-65536 if y>=32768 else y)
def texel_index(a,b,n,x):
    step=int((1 if b>a else-1)*(n-1)*4096/abs(b-a));seed=(n-1 if b<a else 0)*4096+2048;return(seed+(x-min(a,b))*step)>>12
class Bank:
    def __init__(self,scene_id):
        rep=json.load(open(ROOT/'.hkpsx/packed-scenes.json'))['scenes'][scene_id];self.rep=rep;self.raw=(ROOT/f'data/scenes/scene_{scene_id}.hk').read_bytes();s=rep['sections']
        self.pages=s['pages']['offset'];self.palettes=s['palettes']['offset'];self.tex=[]
        for t in range(rep['textures']):
            page,u,v,w,h,pal,off=struct.unpack_from('<6HI',self.raw,s['textures']['offset']+t*16);self.tex.append((page,u,v,w,h,pal))
        self.by_pos={(page,u,v):t for t,(page,u,v,w,h,pal) in enumerate(self.tex) if page!=65535};self.grids={}
    def grid(self,t):
        if t not in self.grids:
            page,u,v,w,h,pal=self.tex[t];palette=struct.unpack_from('<16H',self.raw,self.palettes+pal*32)
            self.grids[t]=[[palette[(self.raw[self.pages+page*32768+(v+y)*128+((u+x)>>1)]>>(4 if (u+x)&1 else 0))&15] for x in range(w)] for y in range(h)]
        return self.grids[t]
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('run',type=Path);p.add_argument('--map',type=Path,default=ROOT/'build/hk-psx-normal.map');p.add_argument('--top',type=int,default=20);p.add_argument('--potential',action='store_true')
    a=p.parse_args();ram=(a.run/'ram.bin').read_bytes();sym=symbols(a.map)
    rep=json.load(open(a.run/('replay.json' if (a.run/'replay.json').exists() else 'command.json')));w=rep['watches']
    region=struct.unpack_from('<I',ram,int(w['HK_REGION_ID'],16)&0x1fffff)[0];meta=json.load(open(ROOT/'data/regions.json'))['regions'][region-1]
    bank=Bank(meta['scene_id'])
    # Name packets through the runtime draw table: draw i keeps the source
    # order of the region's scene.json, and its projected vertex 0 with the
    # final camera identifies the packet (texture ids alone are shared).
    source=json.load(open(ROOT/f"data/regions/region-{meta['chunk_id']:03}/scene.json"))['draws']
    dbase=sym['hk_psx::render::DRAWS'][0]&0x1fffff;dcount=struct.unpack_from('<I',ram,sym['hk_psx::render::DRAW_COUNT'][0]&0x1fffff)[0]
    px_=struct.unpack_from('<i',ram,int(w['HK_PLAYER_X'],16)&0x1fffff)[0]/65536;py_=struct.unpack_from('<i',ram,int(w['HK_PLAYER_Y'],16)&0x1fffff)[0]/65536
    cam=(round(min(max(px_,meta['camera_x'][0]),meta['camera_x'][1])*65536),round(min(max(py_+2,meta['camera_y'][0]),meta['camera_y'][1])*65536))
    by_vertex=collections.defaultdict(list)
    for i in range(dcount):
        xy=struct.unpack_from('<8i',ram,dbase+i*44);scale=struct.unpack_from('<i',ram,dbase+i*44+32)[0];tex=struct.unpack_from('<I',ram,dbase+i*44+36)[0]
        cx=((cam[0]>>8)*scale)>>12;cy=((cam[1]>>8)*scale)>>12
        by_vertex[(tex,160+((xy[0]-cx)>>8),120-((xy[1]-cy)>>8))].append(i)
    def name_of(t,v0):
        c=by_vertex.get((t,v0[0],v0[1]));return source[c[0]]['name'] if c else f'global tex {t}'
    base,size=sym['hk_psx::render::PACKETS'];used=struct.unpack_from('<I',ram,sym['hk_psx::render::USED'][0]&0x1fffff)[0];back=struct.unpack_from("<I",ram,sym["hk_psx::render::KICKED"][0]&0x1fffff)[0]
    rows=[];total=0
    for i in range(used):
        off=(base&0x1fffff)+i*56;tag=struct.unpack_from('<I',ram,off)[0];n=tag>>24;words=struct.unpack_from(f'<{n}I',ram,off+4);clip=(0,0,320,240);j=0
        if words[0]>>24==0xE3:
            l=words[0]&1023;t=(words[0]>>10)&511;r=(words[1]&1023)+1;b=((words[1]>>10)&511)+1;fy=240 if t>=240 else 0;clip=(l,t-fy,r,b-fy);j=2
        op=words[j]>>24;row=dict(i=i,back=i<back,clip=clip,kind='op%02x'%op,name='',px=0,cost=0,tex=None,v=None)
        if 0x2C<=op<=0x2F:
            v=[vert(words[j+1]),vert(words[j+3]),vert(words[j+5]),vert(words[j+7])];px=quad_pixels(v,clip)
            tp=(words[j+4]>>16)&0xffff;x=(tp&15)*64;y=((tp>>4)&1)*256;pidx=((x-384)//64)+(y//256)*10 if x>=384 else -1
            u=words[j+2]&255;uv=(words[j+2]>>8)&255;t=bank.by_pos.get((pidx,u,uv))
            row.update(kind='FT4 abr%d'%((tp>>5)&3),px=px,cost=px*TEXTURED,tex=t,name=name_of(t,v[0]) if t is not None else f'tex@{pidx}:{u},{uv}',v=v,uv=(u,uv),color=words[j]&0xffffff)
        elif op in(0x28,0x2A):
            v=[vert(words[j+1]),vert(words[j+2]),vert(words[j+3]),vert(words[j+4])];px=quad_pixels(v,clip)
            row.update(kind='F4 semi' if op==0x2A else 'F4',px=px,cost=px*(FLAT_SEMI if op==0x2A else FLAT),name='flat %06x'%(words[j]&0xffffff),v=v)
        elif 0x64<=op<=0x67:
            wh=words[j+3];px=(wh&0xffff)*(wh>>16);row.update(kind='sprite',px=px,cost=px*135/128,name='rect')
        rows.append(row);total+=row['cost']
    print(f"region {region} ({meta['scene_name']}) frame: {used} packets (back list {back}), estimated {total:,.0f} bus cycles = {total/33868.8:.1f} ms")
    fam=collections.Counter()
    for r in rows:fam[r['name'].rstrip('0123456789 ()')]+=r['cost']
    print('by name family:',[(k,f'{c*100/total:.1f}%') for k,c in fam.most_common(14)])
    for r in sorted(rows,key=lambda r:-r['cost'])[:a.top]:print(f"  {r['cost']:9,.0f} cyc {r['px']:7,d} px {'back ' if r['back'] else 'front'} {r['kind']:9s}{' clip' if r['clip']!=(0,0,320,240) else '     '} {r['name']}")
    if not a.potential:return
    import numpy as np
    occl=np.zeros((240,320),bool);waste=0;core_save=0;transp=0;details=[]
    for r in reversed(rows):
        if r['v'] is None:continue
        v=r['v'];axis=v[0][1]==v[1][1] and v[0][0]==v[2][0] and v[1][0]==v[3][0] and v[2][1]==v[3][1]
        clip=r['clip'];l,t=max(clip[0],min(v[0][0],v[1][0])),max(clip[1],min(v[0][1],v[2][1]));rr,b=min(clip[2],max(v[0][0],v[1][0])),min(clip[3],max(v[0][1],v[2][1]))
        if not axis or rr<=l or b<=t:continue
        rect=np.zeros((240,320),bool);rect[t:b,l:rr]=True
        hidden=int((rect&occl).sum());waste+=hidden*(TEXTURED if r['tex'] is not None else FLAT)
        if r['tex'] is None:
            if r['kind']=='F4':occl|=rect
            continue
        page,u0,v0,w,h,pal=bank.tex[r['tex']];g=bank.grid(r['tex']);ux=[texel_index(v[0][0],v[1][0],w,x) for x in range(l,rr)];vy=[texel_index(v[0][1],v[2][1],h,y) for y in range(t,b)]
        arr=np.array(g)[np.ix_([min(max(y,0),h-1) for y in vy],[min(max(x,0),w-1) for x in ux])]
        vis=~occl[t:b,l:rr]
        tr=int(((arr==0)&vis).sum());transp+=tr*TEXTURED
        black=(arr==1)
        best=(0,None);hist=np.zeros(rr-l,int)
        for yy in range(b-t):
            hist=np.where(black[yy],hist+1,0);stack=[]
            for xx in range(rr-l+1):
                cur=int(hist[xx]) if xx<rr-l else 0;start=xx
                while stack and stack[-1][1]>=cur:
                    sx,sh=stack.pop();area=sh*(xx-sx)
                    if area>best[0]:best=(area,(sx,yy-sh+1,xx,yy+1))
                    start=sx
                stack.append((start,cur))
        core=best[0];core_save+=core*(TEXTURED-FLAT)
        details.append((hidden*TEXTURED,tr*TEXTURED,core*(TEXTURED-FLAT),r['name'],r['px']))
        full=np.zeros((240,320),bool);full[t:b,l:rr]=black;occl|=full
    print(f"potential (bus cycles): hidden under later opaque black {waste:,.0f} ({waste*100/total:.1f}%), transparent texels {transp:,.0f} ({transp*100/total:.1f}%), flat black cores {core_save:,.0f} ({core_save*100/total:.1f}%)")
    for h,tr,c,n,px in sorted(details,key=lambda d:-(d[0]+d[1]+d[2]))[:12]:print(f"  hidden {h:8,.0f} transparent {tr:8,.0f} core {c:8,.0f}  {n} ({px:,} px)")
if __name__=='__main__':main()
