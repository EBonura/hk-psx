#!/usr/bin/env python3
"""Host-only, byte-exact resident-bank feasibility audit. Never writes discs.

Run with .venv/bin/python. Reports contain sizes/hashes, not retail asset bytes.
The prototype retains all atlas placement, palette, animation, alpha-cover and
local draw/frame indices; canonicalization is by actual bytes, not source ID.
"""
import argparse
import csv
import hashlib
import json
import struct
import sys
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'host'))
from region_delta import layout, textures, reconstruct, compressed


def sha(b): return hashlib.sha256(b).hexdigest()
def aligned(n, alignment=4): return (n+alignment-1)//alignment*alignment

def adjacent(a,b):
    x=min(a[2],b[2])-max(a[0],b[0]);y=min(a[3],b[3])-max(a[1],b[1])
    return x>=0 and y>=0 and (x>0 or y>0)

class Pool:
    def __init__(self): self.values=[];self.ids={}
    def add(self,b):
        if b not in self.ids:
            if len(self.values)>=65535: raise ValueError('16-bit pool exhausted')
            self.ids[b]=len(self.values);self.values.append(b)
        return self.ids[b]
    def bytes(self):return sum(aligned(len(b)) for b in self.values)

class Bank:
    def __init__(self):
        self.texture=Pool();self.draw=Pool();self.frame=Pool();self.clip=Pool();self.edge=Pool();self.covers={};self.rooms=[]
    def add(self,raw):
        counts,prefix,page_start,stream_start=layout(raw)
        blobs=textures(raw);placements=[]
        for i,blob in enumerate(blobs):
            page,u,v,w,h,palette,offset=struct.unpack_from('<6HI',raw,40+16*i)
            if page!=65535 and page>254 or u>255 or v>255 or offset>65535:raise ValueError('Compact placement overflow')
            tid=self.texture.add(blob)
            placements.append(struct.pack('<HBBBHH',tid,255 if page==65535 else page,u,v,palette,offset))
            if page!=65535 and struct.unpack_from('<I',raw,36)[0]&1:
                cover=raw[stream_start+offset:stream_start+offset+20]
                if tid in self.covers and self.covers[tid]!=cover:raise ValueError('Texture-identical alpha covers differ')
                self.covers[tid]=cover
        at=40+counts[1]*16;refs=[]
        for name,count,size in zip(('draw','frame','clip','edge'),counts[2:],(44,20,16,16)):
            pool=getattr(self,name);records=[]
            for i in range(count):
                record=raw[at+i*size:at+(i+1)*size]
                if name in ('draw','frame'):
                    width=2 if name=='draw' else 4
                    texture=int.from_bytes(record[:width],'little')
                    if texture>=len(blobs):raise ValueError('Invalid local texture reference')
                    records.append(struct.pack('<HH',pool.add(record[width:]),texture))
                else:records.append(struct.pack('<H',pool.add(record)))
            refs.append(b''.join(records));at+=count*size
        # Eight offsets/lengths per descriptor conservatively cost32 bytes;
        # original40-byte header and32-byte digest remain explicit.
        recipe={'header':raw[:40],'sha256':sha(raw),'placements':b''.join(placements),'refs':refs}
        self.rooms.append(recipe);return recipe
    def restore(self,recipe):
        counts=struct.unpack_from('<6I',recipe['header'],8);prefix=bytearray(recipe['header']);blobs=[]
        for i in range(counts[1]):
            tid,page,u,v,palette,offset=struct.unpack_from('<HBBBHH',recipe['placements'],i*9)
            blob=self.texture.values[tid];w,h=struct.unpack_from('<HH',blob)
            prefix+=struct.pack('<6HI',65535 if page==255 else page,u,v,w,h,palette,offset);blobs.append(blob)
        for name,refs in zip(('draw','frame','clip','edge'),recipe['refs']):
            pool=getattr(self,name)
            if name in ('draw','frame'):
                for at in range(0,len(refs),4):
                    rid,tid=struct.unpack_from('<HH',refs,at)
                    prefix+=tid.to_bytes(2 if name=='draw' else 4,'little')+pool.values[rid]
            else:
                for at in range(0,len(refs),2):prefix+=pool.values[struct.unpack_from('<H',refs,at)[0]]
        size=len(prefix)+counts[1]*32+counts[0]*32768+struct.unpack_from('<I',prefix,32)[0]
        raw=reconstruct(bytes(prefix),size,blobs)
        if sha(raw)!=recipe['sha256']:raise ValueError('Reconstructed pack differs')
        return raw
    def costs(self):
        result={name+'_pool':getattr(self,name).bytes() for name in ('texture','draw','frame','clip','edge')}
        result['texture_offsets']=4*(len(self.texture.values)+1)
        result['alpha_covers']=20*len(self.texture.values) # fixed indexing; zero for animation
        result['room_descriptors']=104*len(self.rooms)
        result['placements']=sum(aligned(len(r['placements'])) for r in self.rooms)
        result['record_references']=sum(aligned(len(ref))for r in self.rooms for ref in r['refs'])
        result['total']=sum(result.values());return result


def dense_pack(rectangles):
    """Deterministic unrotated MaxRects in legal256x256 texture pages.

    Widths must be4-texel aligned. No filtering padding is invented; PS1
    sampling uses the unchanged inclusive UV rectangle. Returns checked boxes.
    """
    pages=[];placements=[]
    for width,height,index in sorted(rectangles,key=lambda r:(r[0]*r[1],max(r[:2]),r[2]),reverse=True):
        if not 0<width<=256 or not 0<height<=256 or width%4:raise ValueError('Invalid page rectangle')
        choices=[((min(w-width,h-height),max(w-width,h-height),pi,y,x),pi,x,y)
          for pi,free in enumerate(pages)for x,y,w,h in free if width<=w and height<=h]
        if choices:_,page,x,y=min(choices)
        else:page=len(pages);pages.append([(0,0,256,256)]);x=y=0
        changed=[]
        for fx,fy,fw,fh in pages[page]:
            if x>=fx+fw or x+width<=fx or y>=fy+fh or y+height<=fy:
                changed.append((fx,fy,fw,fh));continue
            if fx<x:changed.append((fx,fy,x-fx,fh))
            if x+width<fx+fw:changed.append((x+width,fy,fx+fw-x-width,fh))
            if fy<y:changed.append((fx,fy,fw,y-fy))
            if y+height<fy+fh:changed.append((fx,y+height,fw,fy+fh-y-height))
        pages[page]=[r for j,r in enumerate(changed)if not any(k!=j and
            q[0]<=r[0]and q[1]<=r[1]and q[0]+q[2]>=r[0]+r[2]and q[1]+q[3]>=r[1]+r[3]
            and(q!=r or k<j)for k,q in enumerate(changed))]
        placements.append((index,page,x,y,width,height))
    occupied=set()
    for index,page,x,y,w,h in placements:
        assert x%4==0 and x+w<=256 and y+h<=256
        for yy in range(y,y+h):
            for xx in range(x,x+w,4):
                key=(page,xx//4,yy)
                if key in occupied:raise ValueError('Atlas overlap')
                occupied.add(key)
    return len(pages),placements


def scene_audit(regions):
    result=[]
    for scene in sorted({r['scene_id']for r in regions}):
        bank=Bank();static=set();animation=set()
        for r in regions:
            if r['scene_id']!=scene:continue
            raw=(ROOT/r['path']).read_bytes();bank.add(raw)
            for i,blob in enumerate(textures(raw)):
                (animation if struct.unpack_from('<H',raw,40+i*16)[0]==65535 else static).add(blob)
        packing=[]
        for cap in (48,47,46,44):
            rects=[];changed=0
            for i,blob in enumerate(sorted(static)):
                w,h=struct.unpack_from('<HH',blob)
                # Exact source binary masks must not be resampled. This is a
                # dimension-only experiment: production must recook from source.
                black=blob[4:36]==struct.pack('<16H',0,1,*([0x8000]*14))
                if cap<48 and max(w,h)==48 and not black:
                    w=max(1,(w*cap+47)//48);h=max(1,(h*cap+47)//48);changed+=1
                rects.append((aligned(w),h,i))
            pages,placements=dense_pack(rects)
            packing.append({'dimension_cap':cap,'pages':pages,'atlas_bytes':pages*32768,
              'aligned_texel_bytes':sum(w*h//2 for w,h,_ in rects),'changed_dimensions':changed,
              'placement_sha256':sha(json.dumps(placements).encode()),'pixels_requantized':False})
        result.append({'scene_id':scene,'rooms':len(bank.rooms),'bank':bank.costs(),
            'static_textures':len(static),'static_texel_bytes':sum(len(b)-36 for b in static),
            'unique_palettes':len({b[4:36]for b in static}),
            'animation_textures':len(animation),'animation_canonical_bytes':sum(map(len,animation)),
            'packing':packing})
    return result


def trace_audit(path):
    replay=json.loads((path/'replay.json').read_text());w=replay['watches'];rows=[]
    for rr in csv.DictReader((path/'route.csv').open()):
        row={'poll':int(rr['port1_polls']),'tick':int(rr['route_tick'])}
        row.update({k.removeprefix('HK_'):int(rr['ram_'+v[2:]]) for k,v in w.items()});rows.append(row)
    groups=[]
    for i in range(1,len(rows)):
        if rows[i]['BOUNDARY_WAIT_TICKS']<=rows[i-1]['BOUNDARY_WAIT_TICKS']:continue
        if not groups or rows[i]['poll']-rows[groups[-1][-1]]['poll']>4:groups.append([])
        groups[-1].append(i)
    episodes=[]
    for group in groups:
        lo,hi=group[0],group[-1];old_region=rows[lo]['REGION_ID']
        selected=next((r for r in rows[hi:]if r['REGION_ID']!=old_region),None)
        target=selected['REGION_ID'] if selected else None
        changes=[];last=None
        for r in rows[max(0,lo-160):min(len(rows),hi+20)]:
            key=tuple(r[k]for k in ('REGION_ID','ROOM_READ_REGION','ROOM_DECODE_REGION','VRAM_PENDING_REGION'))
            if key!=last:changes.append({k:r[k]for k in ('poll','REGION_ID','ROOM_READ_REGION','ROOM_DECODE_REGION','ROOM_DECODE_PHASE','VRAM_PENDING_REGION')});last=key
        stage={}
        # State counters are snapshots, not a complete cache-lease inventory.
        # Report observed target work near the hold; do not invent ready times.
        for key in ('ROOM_READ_REGION','ROOM_DECODE_REGION','VRAM_PENDING_REGION'):
            intervals=[];start=None
            for r in rows[max(0,lo-160):min(len(rows),hi+30)]:
                if r[key]==target and start is None:start=r['poll']
                if r[key]!=target and start is not None:intervals.append([start,r['poll']]);start=None
            if start is not None:intervals.append([start,None])
            stage[key]=intervals
        episodes.append({'start_poll':rows[lo]['poll'],'end_poll':rows[hi]['poll'],'from':old_region,'to':target,
          'wait_ticks':sum(rows[i]['BOUNDARY_WAIT_TICKS']-rows[i-1]['BOUNDARY_WAIT_TICKS']for i in group),
          'target_stage_intervals':stage,'timeline':changes})
    return {'input_hashes':replay['inputs'],'route_sha256':sha((path/'route.csv').read_bytes()),'episodes':episodes}


def audit(metadata,packed,trace=None):
    meta_bytes=metadata.read_bytes();packed_bytes=packed.read_bytes();report=json.loads(meta_bytes);records=json.loads(packed_bytes)
    bank=Bank();rooms=[];packed_by={r['chunk_id']:r for r in records['regions']}
    for r in report['regions']:
        raw=(ROOT/r['path']).read_bytes()
        if sha(raw)!=r['sha256']:raise ValueError('Stale room metadata')
        pr=packed_by[r['chunk_id']];stored=Path(pr['path']).read_bytes()
        if len(stored)!=pr['stored_bytes'] or len(raw)!=pr['raw_bytes']:raise ValueError('Stale packed report')
        if stored!=compressed(raw):raise ValueError('Packed bytes mismatch')
        recipe=bank.add(raw);assert bank.restore(recipe)==raw
        counts,prefix,page_start,stream_start=layout(raw)
        rooms.append({'id':r['chunk_id'],'raw_sha256':sha(raw),'stored_sha256':sha(stored),'raw':len(raw),'stored':len(stored),
          'pages':counts[0]*32768,'metadata_prefix':prefix,'palettes':counts[1]*32,
          'alpha_covers':r['alpha_cover_bytes'],'animation':r['animation_bytes'],
          'lean_without_pages':len(raw)-counts[0]*32768,'recipe_bytes':104+aligned(len(recipe['placements']))+sum(aligned(len(x))for x in recipe['refs']),
          'direct':[n['chunk_id']for n in report['regions']if n['chunk_id']!=r['chunk_id']and n['scene_id']==r['scene_id']and adjacent(r['activation_bounds'],n['activation_bounds'])]})
    by={r['id']:r for r in rooms};sets=[]
    for room in rooms:
        direct={room['id'],*room['direct']};two=direct|{n for i in direct for n in by[i]['direct']}
        sets.append({'id':room['id'],**{label:{'ids':sorted(ids),'count':len(ids),**{k:sum(by[i][k]for i in ids)for k in ('raw','stored','lean_without_pages','recipe_bytes')}}for label,ids in [('direct',direct),('two_hop',two)]}})
    costs=bank.costs();costs['page_scratch']=32768;costs['sector_ring_4']=8192
    costs['with_page_and_ring']=costs['total']+32768+8192
    return {'source':{'metadata_sha256':sha(meta_bytes),'packed_report_sha256':sha(packed_bytes),'tool_sha256':sha(Path(__file__).read_bytes())},
       'all_rooms_verified_byte_exact':len(rooms),'rooms':rooms,'working_sets':sets,'resident_bank':costs,
       'pool_counts':{n:len(getattr(bank,n).values)for n in ('texture','draw','frame','clip','edge')},
       'unique_texture_compressed':sum(len(compressed(b))for b in bank.texture.values),
       'totals':{k:sum(r[k]for r in rooms)for k in ('raw','stored','pages','metadata_prefix','palettes','alpha_covers','animation','lean_without_pages')},
       'scenes':scene_audit(report['regions']),
       'trace':trace_audit(trace)if trace else None,
       'limitations':['Host reconstruction uses allocations; proposed guest view/row scatter is not implemented or timed.',
        'New indexing and startup section streaming/checksums required; no decoder-overlap assumption.',
        'VRAM still has four banks; fully resident CPU data does not prove upload readiness for every arbitrary crossing.',
        'Generated gameplay/effect metadata already linked in the guest is unchanged and outside the replaceable room-buffer budget.']}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--metadata',type=Path,default=ROOT/'data/regions.json');p.add_argument('--packed',type=Path,default=ROOT/'.hkpsx/packed-rooms.json');p.add_argument('--trace',type=Path);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    result=audit(a.metadata,a.packed,a.trace);a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:result[k]for k in ('all_rooms_verified_byte_exact','totals','pool_counts','resident_bank','unique_texture_compressed')},indent=2))
if __name__=='__main__':main()
