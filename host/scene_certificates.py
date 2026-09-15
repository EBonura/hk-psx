"""Relocate exact existing coverage proofs into independently resident scenes.

No rasterization, new group selection, geometry changes, or disc writes. The
legacy generated tables remain a hash-bound oracle for exhaustive comparisons.
"""
import argparse
import hashlib
import json
import re
import struct
from pathlib import Path
from scene_bank import compressed, fnv

ROOT=Path(__file__).resolve().parents[1]
MAGIC=b'HKOCSC01'
# The largest current proof is 83,692 bytes. Keep four-byte alignment while
# sizing this overlay to the measured maximum instead of reserving a whole
# 84 KiB page; the saved RAM is needed by the scene admission code.
ARENA_BYTES=83_700
HEADER_BYTES=80
NO_CERT=65535
STRIDES=(12,4,2,12,4,10)
FIELDS=('gx','gy','width','height','offset')


def sha(data):return hashlib.sha256(data).hexdigest()
def aligned(n):return (n+3)&~3

def atlas_fingerprint(atlases):
    return fnv(b''.join(struct.pack('<5I',*(a[k]for k in ('kind','first','count','raw_len','raw_fnv')))for a in atlases))

def _numbers(text):
    if re.sub(r'(?:0x[0-9a-fA-F]+|[0-9]+)|[\s,]','',text):raise ValueError('Invalid generated integer table')
    return [int(v,0)for v in re.findall(r'0x[0-9a-fA-F]+|[0-9]+',text)]

def _array(text,name):
    match=re.search(r'pub static '+name+r':[^=]+=&\[\n(.*?)\n\];',text,re.S)
    if not match:raise ValueError('Missing generated table '+name)
    return match.group(1)

def read_legacy(report,text,groups=False):
    """Compare report metadata against the actual generated Rust, then read bits."""
    prefix='GROUP'if groups else'TILE_CERT'
    cert_name='GROUP_CERTS'if groups else'TILE_CERTS'
    typ='GroupCert'if groups else'TileCert'
    body=_array(text,cert_name);pattern=typ+r'\{gx:(-?\d+),gy:(-?\d+),width:(\d+),height:(\d+),offset:(\d+)\},'
    certs=[dict(zip(FIELDS,map(int,row)))for row in re.findall(pattern,body)]
    if re.sub(pattern,'',body).strip() or certs!=report['certificates']:raise ValueError('Legacy descriptor report differs from Rust')
    bits=_numbers(_array(text,prefix+'_BITS'))
    if len(bits)*4!=report['bitmap_bytes']:raise ValueError('Legacy bitmap count differs')
    if groups:
        body=_array(text,'SCENE_GROUPS');rows=re.findall(r'&\[\n(.*?)\],',body,re.S)
        bindings=[]
        for row in rows:
            pattern=r'Group\{certificate:(\d+),members:\[([0-9,]+)\]\},'
            values=[dict(certificate=int(c),members=_numbers(m))for c,m in re.findall(pattern,row)]
            if re.sub(pattern,'',row).strip():raise ValueError('Invalid legacy group records')
            bindings.append(values)
        if re.sub(r'&\[\n.*?\],','',body,flags=re.S).strip() or bindings!=report['scene_groups']:raise ValueError('Legacy group memberships differ')
    else:
        body=_array(text,'SCENE_DRAW_CERTS');bindings=[_numbers(row)for row in re.findall(r'&\[([^\]]*)\],',body)]
        if re.sub(r'&\[[^\]]*\],','',body).strip() or bindings!=report['bank_bindings']:raise ValueError('Legacy pool maps differ')
        shift=re.search(r'pub const TILE_CERT_SHIFT:u32=(\d+);',text)
        if not shift or int(shift.group(1))!=report['grid_shift'] or report['grid_shift']!=2:raise ValueError('Unsupported or stale certificate grid')
    return dict(certificates=certs,bits=bits,bindings=bindings)

def _shape(cert,bits):
    gx,gy,w,h,offset=(cert[k]for k in FIELDS)
    if not(-32768<=gx<=32767 and -32768<=gy<=32767 and 1<=w<=65535 and 1<=h<=65535) or offset%32:
        raise ValueError('Invalid certificate descriptor')
    n=(w*h+31)//32;start=offset//32
    if start+n>len(bits):raise ValueError('Certificate bitmap outside table')
    words=bits[start:start+n]
    if not any(words) or any(not 0<=v<=0xffffffff for v in words):raise ValueError('Empty/invalid certificate bitmap')
    if w*h%32 and words[-1]>>(w*h%32):raise ValueError('Nonzero unused certificate bits')
    return (gx,gy,w,h),words

def _relocate(legacy,ids):
    ids=sorted(set(ids))
    if len(ids)>NO_CERT or any(not 0<=c<len(legacy['certificates'])for c in ids):raise ValueError('Invalid legacy certificate ID')
    descriptors=[];bits=[]
    for old in ids:
        shape,words=_shape(legacy['certificates'][old],legacy['bits'])
        descriptors.append(dict(zip(FIELDS,(*shape,len(bits)*32))));bits.extend(words)
    return descriptors,bits,{old:new for new,old in enumerate(ids)}

def build_bundle(scene_id,scene_raw_fnv,atlas_fnv,pool_count,tile,group):
    if not 1<=pool_count<=4096 or len(tile['map'])!=pool_count:raise ValueError('Scene draw pool size mismatch')
    tc,tb,tm=_relocate(tile,[i for i in tile['map']if i!=NO_CERT])
    gc,gb,gm=_relocate(group,[g['certificate']for g in group['groups']])
    tile_map=[NO_CERT if old==NO_CERT else tm[old]for old in tile['map']]
    new_groups=[dict(certificate=gm[g['certificate']],members=list(g['members']))for g in group['groups']]
    sections=(b''.join(struct.pack('<hhHHI',*(c[k]for k in FIELDS))for c in tc),
              struct.pack('<'+'I'*len(tb),*tb),struct.pack('<'+'H'*len(tile_map),*tile_map),
              b''.join(struct.pack('<hhHHI',*(c[k]for k in FIELDS))for c in gc),
              struct.pack('<'+'I'*len(gb),*gb),
              b''.join(struct.pack('<5H',g['certificate'],*g['members'])for g in new_groups))
    out=bytearray(HEADER_BYTES);out[:8]=MAGIC
    struct.pack_into('<6I',out,8,scene_id,scene_raw_fnv,atlas_fnv,pool_count,2,0)
    for index,(payload,stride)in enumerate(zip(sections,STRIDES)):
        out+=bytes(aligned(len(out))-len(out));struct.pack_into('<2I',out,32+index*8,len(out),len(payload)//stride);out+=payload
    out+=bytes(aligned(len(out))-len(out))
    if len(out)>ARENA_BYTES:raise ValueError('Per-scene coverage exceeds fixed auxiliary arena')
    raw=bytes(out);proof=verify_equivalence(raw,tile,group)
    return raw,proof

def parse_bundle(raw):
    if len(raw)<HEADER_BYTES or len(raw)>ARENA_BYTES or raw[:8]!=MAGIC:raise ValueError('Invalid coverage header')
    scene_id,scene_raw_fnv,atlas_fnv,pools,shift,reserved=struct.unpack_from('<6I',raw,8)
    if not 1<=pools<=4096 or shift!=2 or reserved:raise ValueError('Invalid coverage configuration')
    sections=[];end=HEADER_BYTES
    for index,stride in enumerate(STRIDES):
        offset,count=struct.unpack_from('<2I',raw,32+index*8)
        if offset!=aligned(end) or offset>len(raw) or any(raw[end:offset]) or count>(len(raw)-offset)//stride:raise ValueError('Invalid coverage section extent')
        end=offset+count*stride;sections.append(raw[offset:end])
    if len(raw)!=aligned(end) or any(raw[end:]):raise ValueError('Nonzero padding/trailing coverage data')
    def certs(data):return [dict(zip(FIELDS,row))for row in struct.iter_unpack('<hhHHI',data)]
    tc=certs(sections[0]);tb=[x[0]for x in struct.iter_unpack('<I',sections[1])]
    mapping=[x[0]for x in struct.iter_unpack('<H',sections[2])]
    gc=certs(sections[3]);gb=[x[0]for x in struct.iter_unpack('<I',sections[4])]
    groups=[dict(certificate=row[0],members=list(row[1:]))for row in struct.iter_unpack('<5H',sections[5])]
    if len(mapping)!=pools or len(tc)>NO_CERT or len(gc)>NO_CERT:raise ValueError('Invalid coverage map/ID count')
    for descriptors,bits in ((tc,tb),(gc,gb)):
        next_bit=0
        for c in descriptors:
            _,words=_shape(c,bits)
            if c['offset']!=next_bit:raise ValueError('Noncontiguous certificate bitmap records')
            next_bit+=len(words)*32
        if next_bit!=len(bits)*32:raise ValueError('Unused certificate bitmap records')
    if any(c!=NO_CERT and c>=len(tc)for c in mapping):raise ValueError('Invalid mapped certificate')
    previous=None
    for g in groups:
        members=g['members'];live=[i for i in members if i!=NO_CERT]
        if g['certificate']>=len(gc) or not 2<=len(live)<=4 or members!=live+[NO_CERT]*(4-len(live)) or live!=sorted(set(live)) or max(live)>=pools:raise ValueError('Invalid coverage group binding')
        if previous is not None and live<=previous:raise ValueError('Unsorted/duplicate coverage groups')
        previous=live
    return dict(scene_id=scene_id,scene_raw_fnv=scene_raw_fnv,atlas_fnv=atlas_fnv,pool_count=pools,
                tile=dict(certificates=tc,bits=tb,map=mapping),group=dict(certificates=gc,bits=gb,groups=groups))

def verify_equivalence(raw,tile,group):
    """Resolve every original pool/map and member against relocated bitmap words."""
    parsed=parse_bundle(raw);new=parsed['tile']
    if len(tile['map'])!=len(new['map']):raise ValueError('Relocated map length changed')
    bits_compared=0
    for old_id,new_id in zip(tile['map'],new['map']):
        if (old_id==NO_CERT)!=(new_id==NO_CERT):raise ValueError('Relocation changed certificate eligibility')
        if old_id==NO_CERT:continue
        a=_shape(tile['certificates'][old_id],tile['bits']);b=_shape(new['certificates'][new_id],new['bits'])
        if a!=b:raise ValueError('Relocation changed tile coverage')
        bits_compared+=a[0][2]*a[0][3]
    new=parsed['group']
    if len(group['groups'])!=len(new['groups']):raise ValueError('Relocation changed group selection')
    for a,b in zip(group['groups'],new['groups']):
        if a['members']!=b['members']:raise ValueError('Relocation changed group members/order')
        old=_shape(group['certificates'][a['certificate']],group['bits']);now=_shape(new['certificates'][b['certificate']],new['bits'])
        if old!=now:raise ValueError('Relocation changed seam coverage')
        bits_compared+=old[0][2]*old[0][3]
    return dict(status='PASS',pool_bindings=len(tile['map']),groups=len(group['groups']),resolved_bits_compared=bits_compared)

def verify_region_bindings(raw,scene,scene_bytes,tile,group,expected_regions):
    """Use exact serialized pool references, including repeated local pool IDs."""
    new=parse_bundle(raw);rows=[]
    for local,source in enumerate(scene['source_rooms']):
        desc=struct.unpack_from('<10I',scene_bytes,scene['sections']['rooms']['offset']+local*40)
        chunk,count=desc[:2]
        if chunk!=source['chunk_id'] or count!=source['counts'][2]:raise ValueError('Scene region draw identity differs')
        pools=struct.unpack_from('<'+'H'*count,scene_bytes,desc[5])
        if any(pool>=len(tile['map'])for pool in pools):raise ValueError('Region draw reference exceeds coverage map')
        old_bindings={draw:tile['map'][pool]for draw,pool in enumerate(pools)if tile['map'][pool]!=NO_CERT}
        if old_bindings!=dict(expected_regions[chunk-1]):raise ValueError('Legacy region certificate resolution differs')
        for draw,pool in enumerate(pools):
            before=tile['map'][pool];after=new['tile']['map'][pool]
            if before==NO_CERT:
                if after!=NO_CERT:raise ValueError('Region certificate eligibility changed')
            elif _shape(tile['certificates'][before],tile['bits'])!=_shape(new['tile']['certificates'][after],new['tile']['bits']):
                raise ValueError('Region certificate resolution changed')
        inverse={}
        for draw,pool in enumerate(pools):inverse.setdefault(pool,draw)
        def resolve(groups):
            out=[]
            for g in groups:
                members=[p for p in g['members']if p!=NO_CERT]
                if all(p in inverse for p in members):out.append([inverse[p]for p in members])
            return out
        old_groups=resolve(group['groups']);new_groups=resolve(new['group']['groups'])
        if old_groups!=new_groups:raise ValueError('Region group local resolution changed')
        rows.append(dict(chunk_id=chunk,local_draws=count,certified_draws=len(old_bindings),resolved_groups=len(old_groups)))
    return rows

def _read_bound(root,path,digest):
    path=root/path;data=path.read_bytes()
    if sha(data)!=digest:raise ValueError('Stale coverage input: '+str(path))
    return data

def rust_manifest(bundles,pool_capacity):
    fields=[('scene_id','u32'),('scene_raw_fnv','u32'),('atlas_fnv','u32'),('draw_pool_count','u32'),('chunk_id','usize'),
            ('raw_len','usize'),('raw_fnv','u32'),('stored_len','usize'),('stored_fnv','u32')]
    text='// Exact scene-local coverage relocation; generated from licensed local inputs.\n'
    text+='#[derive(Clone,Copy,Debug)]\npub struct CoverageDesc{'+','.join('pub '+n+':'+t for n,t in fields)+'}\n'
    text+=f'pub const COVERAGE_ARENA_BYTES:usize={ARENA_BYTES};\npub const COVERAGE_GRID_SHIFT:u32=2;\npub const COVERAGE_GROUP_POOL_CAPACITY:usize={pool_capacity};\n'
    text+='pub const COVERAGE_MANIFEST:&[CoverageDesc]=&[\n'+''.join('CoverageDesc{'+','.join(n+':'+str(b[n])for n,_ in fields)+'},\n'for b in bundles)+'];\n'
    return text

def cook(root=ROOT,output_dir=None,report_path=None,manifest_path=None):
    root=Path(root);output_dir=Path(output_dir)if output_dir else root/'data/scene-coverage'
    report_path=Path(report_path)if report_path else root/'.hkpsx/scene-certificates.json'
    manifest_path=Path(manifest_path)if manifest_path else root/'data/scene_coverage_manifest.rs'
    paths=[root/p for p in ('.hkpsx/packed-scenes.json','.hkpsx/opaque-tiles/report.json','.hkpsx/opaque-groups/report.json','.hkpsx/ambience.json')]
    packed,tr,gr,ambience=[json.loads(p.read_text())for p in paths]
    if packed['residency']!='scene_gate':raise ValueError('Scene-local coverage currently requires exclusive scene residency')
    # Proof provenance includes original bank/region identities, source mutation
    # exclusions and the exact generated code being replaced. Recheck all hashes.
    for report in (tr,gr):
        for name,digest in report['input_sha256'].items():_read_bound(root,Path(name),digest)
        for name,digest in report['outputs'].items():_read_bound(root,Path(name),digest)
    tile=read_legacy(tr,(root/'data/opaque_tiles.rs').read_text())
    group=read_legacy(gr,(root/'data/opaque_groups.rs').read_text(),True)
    scenes=packed['scenes']
    if len(tile['bindings'])!=len(scenes) or len(group['bindings'])!=len(scenes):raise ValueError('Coverage owner count mismatch')
    first_chunk=len(scenes)+len(ambience['clips'])+len(packed['atlases'])+2
    bodies=[];bundles=[]
    for index,scene in enumerate(scenes):
        _read_bound(root,Path(scene['resident_raw_path']),scene['resident_raw_sha256'])
        owner_atlases=[a for a in packed['atlases']if a['scene_index']==index]
        if not owner_atlases:raise ValueError('Missing atlas owner')
        old_tile=dict(tile,map=tile['bindings'][index]);old_group=dict(group,groups=group['bindings'][index])
        raw,proof=build_bundle(scene['scene_id'],scene['resident_raw_fnv'],atlas_fingerprint(owner_atlases),scene['pools']['draws'],old_tile,old_group)
        full_scene=_read_bound(root,Path(scene['raw_path']),scene['raw_sha256'])
        proof['regions']=verify_region_bindings(raw,scene,full_scene,old_tile,old_group,tr['region_bindings'])
        stored=compressed(raw)
        if aligned(len(raw))>ARENA_BYTES or (len(stored)+2047)//2048*2048>packed['arena_bytes']:raise ValueError('Coverage staging exceeds arena')
        path=output_dir/f"scene_{scene['scene_id']}"
        bundles.append(dict(scene_index=index,scene_id=scene['scene_id'],scene_raw_fnv=scene['resident_raw_fnv'],atlas_fnv=atlas_fingerprint(owner_atlases),
                            draw_pool_count=scene['pools']['draws'],chunk_id=first_chunk+index,raw_len=len(raw),raw_fnv=fnv(raw),raw_sha256=sha(raw),stored_len=len(stored),stored_fnv=fnv(stored),stored_sha256=sha(stored),
                            raw_path=str(path)+'.hk',stored_path=str(path)+'.hlzc',path=str(path)+'.hlzc',equivalence=proof))
        bodies.append((raw,stored))
    rust=rust_manifest(bundles,max(s['pools']['draws']for s in scenes))
    report=dict(format='HKOCSC01',arena_bytes=ARENA_BYTES,header_bytes=HEADER_BYTES,grid_shift=2,
                source_sha256={str(p):sha(p.read_bytes())for p in paths},
                code_sha256=sha(Path(__file__).read_bytes()),bundles=bundles,manifest_path=str(manifest_path),manifest_sha256=sha(rust.encode()),
                proof='Every scene pool binding and selected ordered seam group resolves to identical legacy geometry, dimensions and all bitmap words; only certificate IDs and offsets relocate.')
    # Validate all owners before publishing any output.
    output_dir.mkdir(parents=True,exist_ok=True);report_path.parent.mkdir(parents=True,exist_ok=True);manifest_path.parent.mkdir(parents=True,exist_ok=True)
    for bundle,(raw,stored)in zip(bundles,bodies):Path(bundle['raw_path']).write_bytes(raw);Path(bundle['stored_path']).write_bytes(stored)
    manifest_path.write_text(rust);report_path.write_text(json.dumps(report,indent=2)+'\n')
    return report

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path);parser.add_argument('--report',type=Path);parser.add_argument('--manifest',type=Path)
    args=parser.parse_args();report=cook(output_dir=args.output,report_path=args.report,manifest_path=args.manifest)
    print(json.dumps({'arena_bytes':ARENA_BYTES,'bundles':[{k:b[k]for k in ('scene_id','raw_len','stored_len','chunk_id','equivalence')}for b in report['bundles']]},indent=2))
