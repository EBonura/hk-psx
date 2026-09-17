"""Optional bounded seam certificates for immutable flat Tilemap groups.

Every retained group is atomic. Budget admission affects only optional culling;
this is deliberately not a promise to cover every scene seam. Retail content
and cooked proof tables remain local bring-your-own-assets outputs.
"""
import argparse, hashlib, itertools, json, struct, subprocess
from fractions import Fraction
from pathlib import Path
from opaque_tiles import ROOT, RASTER_SOURCE, file_sha, sha, write_changed, mutable_draws, read_certificates
MAX_TABLE_BYTES=8*1024
MAX_REGION_GROUPS=32
NO_MEMBER=65535

def group_phases(xy):
    if len(xy)not in(16,24,32):raise ValueError('Opaque groups: expected2–4 quads')
    xs=sorted({tuple(((xy[k]-p)>>8)-((xy[0]-p)>>8)for k in range(0,len(xy),2))for p in range(256)})
    ys=sorted({tuple(-((xy[k]-p)>>8)+((xy[1]-p)>>8)for k in range(1,len(xy),2))for p in range(256)})
    out=[list(zip(x,y))for x in xs for y in ys]
    if len(out)>256:raise ValueError('Opaque groups: phase bound exceeded')
    return out

def axis(xy):
    return xy[1]==xy[3]and xy[0]==xy[4]and xy[2]==xy[6]and xy[5]==xy[7]

def connected_groups(records):
    """Stable2–4-member groups with common scale and touching source bounds."""
    ids=sorted(records);boxes={i:(min(records[i]['xy'][::2]),min(records[i]['xy'][1::2]),max(records[i]['xy'][::2]),max(records[i]['xy'][1::2]))for i in ids}
    def adjacent(a,b):
        x,y=boxes[a],boxes[b]
        return records[a]['scale']==records[b]['scale']and min(x[2],y[2])>=max(x[0],y[0])and min(x[3],y[3])>=max(x[1],y[1])
    edges={i:{j for j in ids if i!=j and adjacent(i,j)}for i in ids}
    current={tuple(sorted((i,j)))for i in ids for j in edges[i]}
    for size in(2,3,4):
        if size>2:current={tuple(sorted((*g,j)))for g in current for i in g for j in edges[i]if j not in g}
        for group in sorted(current):
            xy=sum((records[i]['xy']for i in group),[])
            if max(xy[::2])-min(xy[::2])<=703*256 and max(xy[1::2])-min(xy[1::2])<=511*256:yield group

def solid_word_one(raw,sections,texture):
    page,u,v,w,h,pal,_=struct.unpack_from('<6HI',raw,sections['textures']['offset']+texture*16)
    if page==65535 or not w or not h:return False
    palette=struct.unpack_from('<16H',raw,sections['palettes']['offset']+pal*32);base=sections['pages']['offset']+page*32768
    return all(palette[(raw[base+(v+y)*128+(u+x)//2]>>(4*((u+x)&1)))&15]==1 for y in range(h)for x in range(w))

def select_groups(candidates,certificates,bank_count,budget=MAX_TABLE_BYTES,max_region=MAX_REGION_GROUPS):
    """Greedy stable area/byte admission with whole memberships and hard bounds.

    Recompute marginal bytes after shared bitmap admission. Region multiplicity
    weights usefulness across spatial views; ties use stable bank/member IDs.
    """
    overhead=24+8*bank_count;used=overhead;selected=[];admitted=set();per_region={};pending=list(candidates)
    def cert_key(c):return(c['gx'],c['gy'],c['width'],c['height'],tuple(c['words']))
    keys={i:cert_key(c)for i,c in enumerate(certificates)}
    while pending:
        choices=[]
        for index,g in enumerate(pending):
            c=certificates[g['pose']]
            if not c['width']or any(per_region.get(r,0)>=max_region for r in g['regions']):continue
            key=keys[g['pose']];cost=10+(0 if key in admitted else 12+len(c['words'])*4)
            if used+cost>budget:continue
            area=sum(w.bit_count()for w in c['words']);score=Fraction(area*len(g['regions']),cost)
            choices.append((score,tuple(-x for x in(g['scene'],*g['members'])),index,cost,key))
        if not choices:break
        _,_,index,cost,key=max(choices);g=pending.pop(index);used+=cost;admitted.add(key);selected.append(g)
        for r in g['regions']:per_region[r]=per_region.get(r,0)+1
    return selected,used,per_region

def compile_helper(cache):
    version=subprocess.check_output(['rustc','--version','--verbose'],text=True)
    sources={name:file_sha(ROOT/'host'/name)for name in('coverage_groups.rs','coverage_raster.rs')};key=sha(json.dumps({'rustc':version,'sources':sources},sort_keys=True).encode());binary=cache/'coverage-groups';stamp=cache/'helper.json';old=json.loads(stamp.read_text())if stamp.exists()else{}
    if not binary.exists()or old.get('key')!=key or old.get('binary_sha256')!=file_sha(binary):
        subprocess.run(['rustc','--edition=2021','-O',str(ROOT/'host/coverage_groups.rs'),'-o',str(binary)],check=True)
        write_changed(stamp,json.dumps({'key':key,'binary_sha256':file_sha(binary)},indent=2)+'\n')
    return binary,{'rustc':version,'source_sha256':sources,'binary_sha256':file_sha(binary),'upstream':RASTER_SOURCE}

def cook(root=ROOT):
    root=Path(root);cache=root/'.hkpsx/opaque-groups';cache.mkdir(parents=True,exist_ok=True)
    packed=json.loads((root/'.hkpsx/packed-scenes.json').read_text());metadata=json.loads((root/'data/regions.json').read_text());mh=file_sha(root/'data/regions.json')
    if mh!=packed['source_metadata_sha256']:raise ValueError('Opaque groups: stale packed scene metadata')
    geo=json.loads((root/'.hkpsx/geo-provenance.json').read_text());life=json.loads((root/'.hkpsx/lifeblood-provenance.json').read_text())
    for report,path in((geo,'data/geo.rs'),(life,'data/lifeblood.rs')):
        if report['rust_sha256']!=file_sha(root/path)or report['region_metadata_sha256']!=mh:raise ValueError('Opaque groups: stale mutable bindings')
    exclusions=mutable_draws(metadata,geo,life);sources={};inputs={}
    for row in metadata['regions']:
        name=f"data/regions/region-{row['chunk_id']:03}/scene.json";sources[row['chunk_id']]=json.loads((root/name).read_text())['draws'];inputs[name]=file_sha(root/name)
        if len(sources[row['chunk_id']])!=row['draws']:raise ValueError('Opaque groups: source identity count differs')
    for name in('data/regions.json','data/geo.rs','data/lifeblood.rs','host/opaque_groups.py','host/opaque_tiles.py','host/coverage_groups.rs','host/coverage_raster.rs','game/src/world.rs','game/src/reveal_masks.rs','game/src/geo_render.rs','game/src/lifeblood.rs','game/src/great_door.rs'):
        inputs[name]=file_sha(root/name)
    for bank in packed['scenes']:
        if file_sha(root/bank['raw_path'])!=bank['raw_sha256']:raise ValueError('Opaque groups: final bank payload changed')
        inputs[bank['raw_path']]=bank['raw_sha256']
        for sr in bank['source_rooms']:
            row=metadata['regions'][sr['chunk_id']-1];raw=(root/row['path']).read_bytes()
            if sha(raw)!=row['sha256']or sha(raw)!=sr['sha256']or struct.unpack_from('<I',raw,12)[0]!=len(sr['texture_map']):raise ValueError('Opaque groups: final room mapping changed')
    binary,helper=compile_helper(cache);key=sha(json.dumps({'inputs':inputs,'scene_order':[b['scene_id'] for b in packed['scenes']],'helper':helper,'format':'HKOPAQUEGROUPS01','budget':MAX_TABLE_BYTES,'region_limit':MAX_REGION_GROUPS},sort_keys=True).encode());report_path=cache/'report.json';old=json.loads(report_path.read_text())if report_path.exists()else{}
    if old.get('input_key')==key and old.get('outputs')and all((root/p).exists()and file_sha(root/p)==h for p,h in old['outputs'].items()):
        print(f"Opaque groups: cached {old['selected_groups']} groups, {old['table_bytes']} bytes",flush=True);return old
    mutated_sources={sources[i+1][d]['source']for i,ids in enumerate(exclusions)for d in ids};mutated_records=set();records={};regions=[]
    for bank_index,bank in enumerate(packed['scenes']):
        raw=(root/bank['raw_path']).read_bytes();sections=bank['sections'];solid={}
        for local,sr in enumerate(bank['source_rooms']):
            region=sr['chunk_id'];desc=struct.unpack_from('<10I',raw,sections['rooms']['offset']+40*local);source=sources[region];row={}
            if desc[0]!=region or desc[1]!=len(source):raise ValueError('Opaque groups: room reference mismatch')
            for draw,src in enumerate(source):
                gid=struct.unpack_from('<H',raw,desc[5]+draw*2)[0];tex,front,scale,*values=struct.unpack_from('<HHi8i4B',raw,sections['draws']['offset']+gid*44);xy=values[:8]
                if draw in exclusions[region-1]:mutated_records.add((bank_index,gid))
                if 'tilemap_rect'not in src or front!=1 or values[-1]!=1 or src['source']in mutated_sources or not axis(xy)or scale<=0:continue
                if tex not in solid:solid[tex]=solid_word_one(raw,sections,tex)
                if not solid[tex]:continue
                record={'xy':xy,'scale':scale,'texture':tex};existing=records.setdefault((bank_index,gid),record)
                if existing!=record:raise ValueError('Opaque groups: pooled source mismatch')
                row[gid]=draw
            regions.append((bank_index,region,row))
    for ident in mutated_records:records.pop(ident,None)
    candidates={};poses=[];pose_lookup={}
    for bank,region,row in regions:
        local={gid:records[bank,gid]for gid in row if(bank,gid)in records}
        for members in connected_groups(local):
            identity=(bank,members)
            if identity not in candidates:
                xy=sum((local[i]['xy']for i in members),[]);relative=tuple(v-xy[i%2]for i,v in enumerate(xy));pose=pose_lookup.get(relative)
                if pose is None:
                    phases=group_phases(xy)
                    if any(max(x for x,y in p)-min(x for x,y in p)>703 or max(y for x,y in p)-min(y for x,y in p)>511 for p in phases):continue
                    pose=len(poses);pose_lookup[relative]=pose;poses.append({'relative_q8':relative,'phases':phases,'members':len(members)})
                candidates[identity]={'scene':bank,'members':members,'pose':pose,'regions':[]}
            candidates[identity]['regions'].append(region)
    payload=bytearray(b'HKGPIN01'+struct.pack('<I',len(poses)))
    for i,p in enumerate(poses):
        payload+=struct.pack('<3I',i,len(p['phases']),p['members'])
        for v in p['phases']:payload+=struct.pack('<'+'i'*(len(v)*2),*[a for q in v for a in q])
    write_changed(cache/'input.bin',payload);subprocess.run([str(binary),str(cache/'input.bin'),str(cache/'output.bin')],check=True);certificates=read_certificates((cache/'output.bin').read_bytes(),len(poses))
    selected,total,counts=select_groups(list(candidates.values()),certificates,len(packed['scenes']));descriptors=[];bits=[];lookup={};banks=[[]for _ in packed['scenes']];selected_report=[]
    for g in sorted(selected,key=lambda g:(g['scene'],g['members'])):
        c=certificates[g['pose']];k=(c['gx'],c['gy'],c['width'],c['height'],tuple(c['words']))
        if k not in lookup:
            lookup[k]=len(descriptors);descriptors.append({n:c[n]for n in('gx','gy','width','height')}|{'offset':len(bits)*32});bits.extend(c['words'])
        ident=lookup[k];members=list(g['members'])+[NO_MEMBER]*(4-len(g['members']));banks[g['scene']].append({'certificate':ident,'members':members});selected_report.append(g|{'certificate':ident})
    actual=24+8*len(banks)+10*len(selected)+12*len(descriptors)+4*len(bits)
    if actual!=total or actual>MAX_TABLE_BYTES:raise ValueError('Opaque groups: budget accounting differs')
    pool_capacity=max((b['pools']['draws']for b in packed['scenes']),default=0)
    text='// Generated optional4px/19x19 seam proofs. Local licensed data.\n'
    text+=f'pub const GROUP_POOL_CAPACITY:usize={pool_capacity};\n'
    text+='pub static GROUP_CERTS:&[GroupCert]=&[\n'+''.join('GroupCert{'+','.join(f'{k}:{v}'for k,v in c.items())+'},\n'for c in descriptors)+'];\n'
    text+='pub static GROUP_BITS:&[u32]=&[\n'+''.join(','.join(f'0x{v:08x}'for v in bits[i:i+8])+',\n'for i in range(0,len(bits),8))+'];\n'
    text+='pub static SCENE_GROUPS:&[&[Group]]=&[\n'+''.join('&[\n'+''.join('Group{certificate:'+str(g['certificate'])+',members:['+','.join(map(str,g['members']))+']},\n'for g in row)+'],\n'for row in banks)+'];\n'
    destination=root/'data/opaque_groups.rs';write_changed(destination,text)
    report={'format':'HKOPAQUEGROUPS01','input_key':key,'input_sha256':inputs,'helper':helper,'native_input_sha256':sha(payload),'native_output_sha256':file_sha(cache/'output.bin'),'candidate_groups':len(candidates),'pose_count':len(poses),'nonempty_poses':sum(bool(c['width'])for c in certificates),'selected_groups':len(selected),'pool_capacity':pool_capacity,'bank_pool_capacities':[b['pools']['draws']for b in packed['scenes']],'certificate_count':len(descriptors),'table_bytes':actual,'bitmap_bytes':len(bits)*4,'descriptor_bytes':len(descriptors)*12,'membership_bytes':len(selected)*10,'budget':MAX_TABLE_BYTES,'maximum_region_groups':max(counts.values(),default=0),'region_counts':counts,'selection':'Stable greedy new seam-bit count times resident region count per marginal table byte; whole groups, shared identical bitmaps,8KiB and32 groups/region caps. Optional incomplete culling coverage.','certificates':descriptors,'selected':selected_report,'scene_groups':banks,'poses':[{'relative_q8':p['relative_q8'],'members':p['members'],'phase_count':len(p['phases'])}for p in poses],'proof':'Union original flat quad raster for each exact common camera phase, intersect phases, erode19x19 on4px grid; remove windows covered by any single member in every phase. All members static immutable opaque Tilemaps with identical scale and legal combined span.','outputs':{'data/opaque_groups.rs':file_sha(destination)}}
    write_changed(report_path,json.dumps(report,indent=2)+'\n');print(f'Opaque groups: {len(selected)}/{len(candidates)} groups, {actual}/{MAX_TABLE_BYTES} bytes, {max(counts.values(),default=0)} maximum region groups',flush=True);return report

if __name__=='__main__':cook()
