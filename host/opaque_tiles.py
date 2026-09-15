"""Cook exact, phase-independent opaque tile certificates from finished banks.

Only local final palette words/geometry are inputs. A standalone native helper
contains pinned GPL GPU raster equations, so no emulator checkout is needed.
Generated artwork and provenance stay ignored; this module never writes discs.
"""
import argparse, hashlib, json, struct, subprocess
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
MAX_TABLE_BYTES=66*1024
RASTER_SOURCE={
    'repository':'https://github.com/EBonura/PSoXide-emulator',
    'revision':'38af605ac5a6961f3798d432bcfb7cceacece239',
    'path':'emu/crates/emulator-core/src/gpu/raster.rs',
    'sha256':'eb24fcaa17fb0a6f7f876b9c20466c9268d2c5c8bf35950fc39d5b91319ef6b6',
    'license':'GPL-2.0-or-later',
}
def sha(data):return hashlib.sha256(data).hexdigest()
def file_sha(path):return sha(Path(path).read_bytes())
def write_changed(path,data):
    path=Path(path);data=data.encode()if isinstance(data,str)else data
    if path.exists()and path.read_bytes()==data:return False
    path.parent.mkdir(parents=True,exist_ok=True);temp=path.with_name(path.name+'.tmp');temp.write_bytes(data);temp.replace(path);return True

def mutable_draws(metadata,geo,life):
    """Complete current static renderer mutation union, not local hit shapes."""
    rows=metadata['regions']
    if len(rows)!=len(geo['bindings'])or len(rows)!=len(life['bindings']):raise ValueError('Opaque tiles: mutable binding region count differs')
    out=[]
    for index,row in enumerate(rows):
        ids=set()
        for p in row['grass']:ids.update((p['off_draw'],p['on_draw']))
        for b in row['breakables']:
            ids.update(b['off_draws']);ids.update(b['on_draws'])
            for f in b.get('mask_fades',[]):ids.update(f['draw_indices'])
        for b in row.get('remote_mask_bindings',[]):ids.update(b['fade']['draw_indices'])
        for b in row.get('reveal_mask_bindings',[]):ids.add(b['draw'])
        for b in geo['bindings'][index]:ids.update(b['off'])
        ids.update(life['bindings'][index]['off'])
        if any(type(i)is not int or not 0<=i<row['draws']for i in ids):raise ValueError('Opaque tiles: mutable draw index outside region')
        out.append(ids)
    return out

def phase_variants(xy):
    """Every integer projection shape relative to vertex0, including reflection.

    Camera/parallax subtracts one Q8 integer per axis. Its residue mod256 is
    exhaustive here; absolute source origin and atlas UV translation cancel.
    """
    if len(xy)!=8:raise ValueError('Opaque tiles: expected four source vertices')
    xs=sorted({tuple(((xy[k]-p)>>8)-((xy[0]-p)>>8)for k in(0,2,4,6))for p in range(256)})
    ys=sorted({tuple(-(((xy[k]-p)>>8)-((xy[1]-p)>>8))for k in(1,3,5,7))for p in range(256)})
    variants=[list(zip(x,y))for x in xs for y in ys]
    if len(variants)>16:raise ValueError('Opaque tiles: phase bound exceeded')
    return variants

def legal_source(xy):
    # Same source-only legal_extent_q8 contract as the guest: after viewport
    # AABB rejection, all original vertices/edges are legal for every phase.
    return max(xy[::2])-min(xy[::2])<=703*256 and max(xy[1::2])-min(xy[1::2])<=511*256

def compile_helper(cache):
    version=subprocess.check_output(['rustc','--version','--verbose'],text=True)
    source={name:file_sha(ROOT/'host'/name)for name in('coverage_cert.rs','coverage_raster.rs')}
    key=sha(json.dumps({'rustc':version,'source':source},sort_keys=True).encode());binary=cache/'coverage-cert';stamp=cache/'helper.json'
    previous=json.loads(stamp.read_text())if stamp.exists()else{}
    if not binary.exists()or previous.get('key')!=key or previous.get('binary_sha256')!=file_sha(binary):
        subprocess.run(['rustc','--edition=2021','-O',str(ROOT/'host/coverage_cert.rs'),'-o',str(binary)],check=True)
        write_changed(stamp,json.dumps({'key':key,'binary_sha256':file_sha(binary)},indent=2)+'\n')
    return binary,{'rustc':version,'source_sha256':source,'binary_sha256':file_sha(binary),'upstream':RASTER_SOURCE}

def read_certificates(data,count):
    if data[:8]!=b'HKTCOT01' or len(data)<12 or struct.unpack_from('<I',data,8)[0]!=count:raise ValueError('Opaque tiles: native proof header mismatch')
    pos=12;out=[]
    for expected in range(count):
        ident,gx,gy,w,h,n=struct.unpack_from('<IhhHHI',data,pos);pos+=16
        if ident!=expected or n!=(w*h+31)//32 or bool(w)!=bool(h):raise ValueError('Opaque tiles: native proof descriptor mismatch')
        words=struct.unpack_from('<'+'I'*n,data,pos);pos+=4*n
        if n and(w*h)%32 and words[-1]>>((w*h)%32):raise ValueError('Opaque tiles: nonzero unused proof bits')
        if n and not any(words):raise ValueError('Opaque tiles: all-zero mask was not rejected')
        out.append(dict(gx=gx,gy=gy,width=w,height=h,words=words))
    if pos!=len(data):raise ValueError('Opaque tiles: trailing native proof output')
    return out

def cook(root=ROOT,grid_shift=2,variant=False):
    if grid_shift not in(2,3):raise ValueError('Opaque tiles: supported grid shifts are2and3')
    root=Path(root);cache=root/('.hkpsx/opaque-tiles-grid4'if variant else'.hkpsx/opaque-tiles');cache.mkdir(parents=True,exist_ok=True)
    window=16+(1<<grid_shift)-1;table_budget=96*1024 if grid_shift==2 else MAX_TABLE_BYTES
    packed=json.loads((root/'.hkpsx/packed-scenes.json').read_text());metadata_path=root/'data/regions.json';metadata=json.loads(metadata_path.read_text());metadata_hash=file_sha(metadata_path)
    if metadata_hash!=packed['source_metadata_sha256']:raise ValueError('Opaque tiles: packed scene metadata is stale')
    geo=json.loads((root/'.hkpsx/geo-provenance.json').read_text());life=json.loads((root/'.hkpsx/lifeblood-provenance.json').read_text())
    for report,path in((geo,root/'data/geo.rs'),(life,root/'data/lifeblood.rs')):
        if report['rust_sha256']!=file_sha(path)or report['region_metadata_sha256']!=metadata_hash:raise ValueError('Opaque tiles: generated mutable bindings are stale')
    exclusions=mutable_draws(metadata,geo,life);sources={};input_hashes={}
    for row in metadata['regions']:
        path=root/f"data/regions/region-{row['chunk_id']:03}/scene.json";source=json.loads(path.read_text())['draws']
        if len(source)!=row['draws']:raise ValueError('Opaque tiles: source draw identity count differs')
        sources[row['chunk_id']]=source;input_hashes[str(path.relative_to(root))]=file_sha(path)
    for path in('data/regions.json','data/regions.rs','data/geo.rs','data/lifeblood.rs','host/opaque_tiles.py','host/coverage_cert.rs','host/coverage_raster.rs','game/src/world.rs','game/src/reveal_masks.rs','game/src/geo_render.rs','game/src/lifeblood.rs','game/src/great_door.rs'):
        input_hashes[path]=file_sha(root/path)
    for bank in packed['scenes']:
        rawpath=root/bank['raw_path']
        if file_sha(rawpath)!=bank['raw_sha256']:raise ValueError('Opaque tiles: final bank payload changed')
        input_hashes[str(rawpath.relative_to(root))]=bank['raw_sha256']
        for sr in bank['source_rooms']:
            row=metadata['regions'][sr['chunk_id']-1];raw=(root/row['path']).read_bytes()
            if sha(raw)!=row['sha256']or sha(raw)!=sr['sha256']or struct.unpack_from('<I',raw,12)[0]!=len(sr['texture_map']):raise ValueError('Opaque tiles: final room texture map changed')
    binary,helper=compile_helper(cache);key=sha(json.dumps({'inputs':input_hashes,'helper':helper,'format':'HKOPAQUETILES01','grid_shift':grid_shift},sort_keys=True).encode());report_path=cache/'report.json'
    old=json.loads(report_path.read_text())if report_path.exists()else{}
    if old.get('input_key')==key and all((root/name).exists()and file_sha(root/name)==digest for name,digest in old.get('outputs',{}).items())and old.get('outputs'):
        print(f"Opaque tiles: cached {old['certificate_count']} certificates, {old['table_bytes']} bytes",flush=True);return old
    mutated_sources={sources[i+1][d]['source']for i,ids in enumerate(exclusions)for d in ids};mutated_records=set();instances=[];masks={}
    for bank in packed['scenes']:
        sid=bank['scene_id'];raw=(root/bank['raw_path']).read_bytes();s=bank['sections']
        for t in range(bank['textures']):
            page,u,v,w,h,pal,_=struct.unpack_from('<6HI',raw,s['textures']['offset']+t*16)
            if page==65535 or not 0<w<=63 or not 0<h<=63:continue
            palette=struct.unpack_from('<16H',raw,s['palettes']['offset']+pal*32);base=s['pages']['offset']+page*32768
            mask=bytes(int((word:=palette[(raw[base+(v+y)*128+(u+x)//2]>>(4*((u+x)&1)))&15])!=0 and not(word&0x8000))for y in range(h)for x in range(w))
            if any(mask):masks[sid,t]=(w,h,mask,sha(struct.pack('<HH',w,h)+mask))
        for local,sr in enumerate(bank['source_rooms']):
            region=sr['chunk_id'];source=sources[region];desc=struct.unpack_from('<10I',raw,s['rooms']['offset']+40*local)
            if desc[0]!=region or desc[1]!=len(source):raise ValueError('Opaque tiles: final bank room reference mismatch')
            for draw,d in enumerate(source):
                gid=struct.unpack_from('<H',raw,desc[5]+draw*2)[0];tex,front,scale,*values=struct.unpack_from('<HHi8i4B',raw,s['draws']['offset']+gid*44);xy=values[:8]
                if draw in exclusions[region-1]:mutated_records.add((sid,gid))
                instances.append(dict(scene=sid,region=region,draw=draw,bank_draw=gid,texture=tex,source=d['source'],front=bool(front),xy=xy))
    poses=[];lookup={}
    for d in instances:
        mask=masks.get((d['scene'],d['texture']))
        if not mask or not legal_source(d['xy'])or d['source']in mutated_sources or(d['scene'],d['bank_draw'])in mutated_records:continue
        w,h,pixels,digest=mask;rel=tuple(v-d['xy'][i%2]for i,v in enumerate(d['xy']));posekey=(digest,rel)
        if posekey not in lookup:
            phase=phase_variants(d['xy']);bounds=[min(v[k][a]for v in phase for k in range(4))for a in(0,1)]+[max(v[k][a]for v in phase for k in range(4))+1 for a in(0,1)]
            if bounds[2]-bounds[0]<window or bounds[3]-bounds[1]<window:continue
            lookup[posekey]=len(poses);poses.append(dict(width=w,height=h,mask=pixels,variants=phase,relative_q8=rel,opaque_mask_sha256=digest))
        d['pose']=lookup[posekey]
    data=bytearray(b'HKTCIN01'+struct.pack('<I',len(poses)))
    for i,p in enumerate(poses):
        data+=struct.pack('<IIHH',i,len(p['variants']),p['width'],p['height'])
        for v in p['variants']:data+=struct.pack('<8i',*[a for xy in v for a in xy])
        data+=struct.pack('<'+'H'*len(p['mask']),*p['mask'])
    native_input=cache/'input.bin';native_output=cache/'output.bin';write_changed(native_input,data)
    subprocess.run([str(binary),str(native_input),str(native_output),str(grid_shift)],check=True)
    certs=read_certificates(native_output.read_bytes(),len(poses));descriptors=[];bits=[];pose_to_cert={}
    for i,c in enumerate(certs):
        if not c['width']:continue
        pose_to_cert[i]=len(descriptors);descriptors.append({k:c[k]for k in('gx','gy','width','height')}|{'offset':len(bits)*32});bits.extend(c['words'])
    bindings=[[]for _ in metadata['regions']]
    for d in sorted(instances,key=lambda d:(d['region'],d['front'],d['draw'])):
        if d.get('pose')in pose_to_cert:bindings[d['region']-1].append((d['draw'],pose_to_cert[d['pose']]))
    bank_bindings=[[65535]*b['pools']['draws']for b in packed['scenes']]
    bank_index={b['scene_id']:i for i,b in enumerate(packed['scenes'])}
    for d in instances:
        if d.get('pose')in pose_to_cert:
            slot=bank_bindings[bank_index[d['scene']]];ident=pose_to_cert[d['pose']]
            if slot[d['bank_draw']]not in(65535,ident):raise ValueError('Opaque tiles: pooled draw certificate identity differs')
            slot[d['bank_draw']]=ident
    # Includes32bit target slice descriptors and generated arrays, not text size.
    table_bytes=12*len(descriptors)+4*len(bits)+2*sum(map(len,bank_bindings))+8*len(bank_bindings)+24
    if table_bytes>table_budget:raise ValueError(f'Opaque tiles: generated tables need {table_bytes} bytes, budget {table_budget}; refusing to truncate')
    text=f'// Generated exact{1<<grid_shift}px-grid/{window}x{window} opaque certificates. Local licensed data.\n'
    text+=f'pub const TILE_CERT_SHIFT:u32={grid_shift};\n'
    text+='pub static TILE_CERTS:&[TileCert]=&[\n'+''.join('TileCert{'+','.join(f'{k}:{v}'for k,v in c.items())+'},\n'for c in descriptors)+'];\n'
    text+='pub static TILE_CERT_BITS:&[u32]=&[\n'+''.join(','.join(f'0x{v:08x}'for v in bits[i:i+8])+',\n'for i in range(0,len(bits),8))+'];\n'
    text+='pub static SCENE_DRAW_CERTS:&[&[u16]]=&[\n'+''.join('&['+','.join(map(str,row))+'],\n'for row in bank_bindings)+'];\n'
    destination=cache/'opaque_tiles.rs'if variant else root/'data/opaque_tiles.rs';write_changed(destination,text)
    report={'format':'HKOPAQUETILES01','input_key':key,'input_sha256':input_hashes,'helper':helper,'native_input_sha256':sha(data),'native_output_sha256':file_sha(native_output),
        'pose_count':len(poses),'certificate_count':len(descriptors),'rejected_empty_poses':len(poses)-len(descriptors),'phase_count':sum(len(p['variants'])for p in poses),'table_bytes':table_bytes,'table_budget':table_budget,'grid_shift':grid_shift,'window':window,
        'descriptor_bytes':12*len(descriptors),'bitmap_bytes':4*len(bits),'binding_bytes':2*sum(map(len,bank_bindings)),'binding_count':sum(map(len,bindings)),'maximum_region_candidates':max(map(len,bindings),default=0),
        'region_bindings':bindings,'bank_bindings':bank_bindings,'certificates':descriptors,'pose_to_certificate':pose_to_cert,'poses':[{k:v for k,v in p.items()if k not in('mask','variants')}for p in poses],
        'source_bindings':[{k:v for k,v in d.items()if k!='xy'}for d in instances if d.get('pose')in pose_to_cert],
        'proof':f'Every bit certifies{window}x{window} nonzero STP-clear samples across every exact relativeQ8 projection shape. Original topology0-1-2/1-3-2 and pinned native PS1 raster equations. Unknown atlas neighbors never certify. All-zero masks and borders removed.',
        'outputs':{str(destination.relative_to(root)):file_sha(destination)}}
    write_changed(report_path,json.dumps(report,indent=2)+'\n');print(f"Opaque tiles: {len(descriptors)}/{len(poses)} poses, {table_bytes}/{table_budget} bytes, {max(map(len,bindings),default=0)} maximum region candidates",flush=True);return report

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--grid-shift',type=int,choices=(2,3),default=2);parser.add_argument('--variant',action='store_true',help='Write4px trial only under ignored cache');args=parser.parse_args();cook(grid_shift=args.grid_shift,variant=args.variant)
