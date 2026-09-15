"""Pack current HKROOM02 sources into lossless scene banks.

No sprite extraction/resampling and no disc writes. Every pack checks the exact
startup decode sequence before it can be used for final guest packaging.
"""
import argparse
import hashlib
import json
import subprocess
import struct
from pathlib import Path
from scene_bank import build_scene, compressed, fnv, aligned, compact_resident, bootstrap_atlases, texture_flags

ROOT=Path(__file__).resolve().parents[1]
# Current scene pair passes the exact sequential in-place decoder at this
# capacity, including sector padding. Keep validation mandatory for new assets.
SCENE_ARENA_BYTES=369180
MAX_PAGES=20
MAX_PALETTES=1248  # Final legacy CLUT strip is reserved for independent Geo art.

def sha(data):return hashlib.sha256(data).hexdigest()

def allocate_scenes(entries, arena_bytes=SCENE_ARENA_BYTES, max_pages=MAX_PAGES, max_palettes=MAX_PALETTES, residency="joint"):
    """Allocate joint residency, or per-gate replacement banks.

    A scene_gate plan reuses storage only after all outgoing references expire;
    transition timing remains the runtime loader's responsibility.
    """
    if not entries or len({e['scene_id']for e in entries})!=len(entries):raise ValueError('Empty/duplicate scenes')
    if residency not in ("joint", "scene_gate"):raise ValueError("Unknown scene residency policy")
    offset=page_base=palette_base=0;result=[]
    for index,entry in enumerate(sorted(entries,key=lambda e:(-e.get('resident_raw_len',e['bytes']),e['scene_id']))):
        e=dict(entry)
        if residency=="scene_gate":offset=page_base=palette_base=0
        if min(e['bytes'],e['pages'],e['palettes'])<=0:raise ValueError('Invalid scene size')
        offset=aligned(offset)
        e.update(scene_index=index,chunk_id=index+1,raw_len=e['bytes'],raw_bytes=e['bytes'],stored_len=e['stored_bytes'],
                 ram_offset=offset,arena_capacity=arena_bytes-offset,page_base=page_base,palette_base=palette_base)
        offset+=e.get('resident_raw_len',e['bytes']);page_base+=e['pages'];palette_base+=e['palettes'];result.append(e)
        if offset>arena_bytes or page_base>max_pages or palette_base>max_palettes:
            raise ValueError('Scene residency exceeds RAM/page/palette limit')
    if offset>arena_bytes:raise ValueError('Scene RAM residency exceeds arena')
    if page_base>max_pages:raise ValueError('Scene atlas residency exceeds page limit')
    if palette_base>max_palettes:raise ValueError('Scene CLUT residency exceeds palette limit')
    return result

def black_cores(raw,entry):
    """Largest all-opaque-black (word0x0001) texel rectangle [x,y,w,h] per bank
    texture, or [0,0,0,0]. At full opacity the guest draws that core as a flat
    opaque black quad and lets it hide earlier draws; exact because the texel
    ignores blending and modulates to black. Texels are sampled from the
    finished bank, so shared planes and palettes are the ones the GPU reads."""
    return _texel_cores(raw,entry,lambda word:word==1)

def opaque_cores(raw,entry):
    """Largest nontransparent STP-clear rectangle per static scene texture.

    These texels overwrite the framebuffer regardless of modulation or ABR.
    They can occlude earlier draws at full opacity, but their colours must
    still be rendered normally: this table never authorizes a flat black draw.
    Read final palette words rather than assuming an opaque palette index.
    """
    return _texel_cores(raw,entry,lambda word:word!=0 and not(word&0x8000))

def _texel_cores(raw,entry,admitted):
    s=entry['sections'];out=[]
    for t in range(entry['textures']):
        page,u,v,w,h,pal,offset=struct.unpack_from('<6HI',raw,s['textures']['offset']+t*16)
        if page==65535 or w==0 or h==0 or w>63 or h>63:out.append([0,0,0,0]);continue
        palette=struct.unpack_from('<16H',raw,s['palettes']['offset']+pal*32);accepted=[admitted(c) for c in palette]
        base=s['pages']['offset']+page*32768;hist=[0]*w;best=(0,[0,0,0,0])
        for y in range(h):
            row=base+(v+y)*128
            for x in range(w):
                b=raw[row+((u+x)>>1)];hist[x]=hist[x]+1 if accepted[(b>>4)&15 if (u+x)&1 else b&15] else 0
            stack=[]
            for x in range(w+1):
                cur=hist[x] if x<w else 0;start=x
                while stack and stack[-1][1]>=cur:
                    sx,sh=stack.pop();area=sh*(x-sx)
                    if area>best[0]:best=(area,[sx,y-sh+1,x-sx,sh])
                    start=sx
                stack.append((start,cur))
        out.append(best[1] if best[0]>=64 else [0,0,0,0])
    return out

def atlas_ranges(scenes, atlases):
    """Half-open chunk-index ranges, with exact per-scene VRAM coverage.

    A range can be streamed without admitting another scene's pages or CLUTs.
    Validate the cooked descriptor order rather than silently regrouping it:
    WORLD.PAK uses this order and checksums belong to those exact chunk indices.
    """
    ranges=[];cursor=0
    for index,scene in enumerate(scenes):
        start=cursor;page=scene['page_base'];palette=scene['palette_base']
        while cursor<len(atlases) and atlases[cursor]['scene_index']==index:
            atlas=atlases[cursor];kind=atlas['kind'];count=atlas['count']
            if kind not in (0,1) or count<=0:
                raise ValueError('Invalid scene atlas kind/count')
            if atlas['raw_len']!=count*(32768 if kind==0 else 32) or atlas['raw_len']>32768:
                raise ValueError('Atlas byte length differs from bounded upload shape')
            expected=page if kind==0 else palette
            limit=(scene['page_base']+scene['pages']) if kind==0 else (scene['palette_base']+scene['palettes'])
            if atlas['first']!=expected or expected+count>limit:
                raise ValueError('Noncontiguous scene atlas coverage')
            if kind==0:page+=count
            else:palette+=count
            cursor+=1
        if page!=scene['page_base']+scene['pages'] or palette!=scene['palette_base']+scene['palettes']:
            raise ValueError('Incomplete scene atlas coverage')
        ranges.append((start,cursor))
    if cursor!=len(atlases):raise ValueError('Atlas owner/order differs from scene manifest')
    return ranges

def manifest(report,metadata):
    scenes=report['scenes'];rows=sorted(metadata['regions'],key=lambda r:r['chunk_id'])
    ranges=atlas_ranges(scenes,report.get('atlases',[]))
    residency=report.get('residency','joint')
    if residency not in ('joint','scene_gate'):raise ValueError('Unknown scene residency policy')
    if residency=='scene_gate' and any(e['ram_offset'] or e['page_base'] or e['palette_base'] for e in scenes):
        raise ValueError('Scene-gate descriptors must reuse arena/VRAM origins')
    aggregate=max if residency=='scene_gate' else sum
    if report['total_pages']!=aggregate(e['pages']for e in scenes) or report['total_palettes']!=aggregate(e['palettes']for e in scenes):
        raise ValueError('Scene residency totals differ from allocation policy')
    if [r['chunk_id']for r in rows]!=list(range(1,len(rows)+1)):raise ValueError('Noncontiguous global region IDs')
    lookup={};representatives=[]
    for index,e in enumerate(scenes):
        chunks=[r['chunk_id']for r in e['source_rooms']]
        if not chunks:raise ValueError('Scene has no region')
        representatives.append(min(chunks)-1)
        for local,chunk in enumerate(chunks):
            if chunk in lookup:raise ValueError('Region belongs to multiple scenes')
            lookup[chunk]=(index,local)
    if set(lookup)!={r['chunk_id']for r in rows}:raise ValueError('Missing scene region mapping')
    for r in rows:
        if scenes[lookup[r['chunk_id']][0]]['scene_id']!=r['scene_id']:raise ValueError('Region mapped to wrong source scene')
    fields=[('scene_id','usize'),('stored_len','usize'),('stored_fnv','u32'),('raw_len','usize'),('raw_fnv','u32'),
            ('ram_offset','usize'),('arena_capacity','usize'),('page_base','usize'),('palette_base','usize')]
    text='// Generated compact HKSCNE02 banks and bootstrap-only atlas chunks.\n'
    text+='#[derive(Clone,Copy,Debug)]\npub struct SceneDesc{'+','.join('pub '+n+':'+t for n,t in fields)+'}\n'
    text+=f'pub const SCENE_ARENA_BYTES:usize={report["arena_bytes"]};\n'
    text+='pub const SCENE_GATE_LOAD:bool='+str(residency=='scene_gate').lower()+';\n'
    # Runtime working tables need only the largest admitted scene/view, not the
    # format's maximum capacity. Round bitset-backed tables to whole u32 words.
    textures=max(e['textures'] for e in scenes)
    draws=max(r['counts'][2] for e in scenes for r in e['source_rooms'])
    if not 1<=textures<=2048 or not 1<=draws<=1024:
        raise ValueError('Renderer working set exceeds format limits')
    text+=f'pub const SCENE_TEXTURE_CAPACITY:usize={(textures+31)//32*32};\n'
    text+=f'pub const SCENE_DRAW_CAPACITY:usize={(draws+31)//32*32};\n'
    text+='pub const SCENE_MANIFEST:&[SceneDesc]=&[\n'+''.join('SceneDesc{'+','.join(n+':'+str(e.get('resident_'+n,e[n]))for n,t in fields)+'},\n'for e in scenes)+'];\n'
    atlas_fields=[('scene_index','usize'),('stored_len','usize'),('stored_fnv','u32'),('raw_len','usize'),('raw_fnv','u32'),('kind','u8'),('first','usize'),('count','usize')]
    text+='#[derive(Clone,Copy,Debug)]\npub struct AtlasDesc{'+','.join('pub '+n+':'+t for n,t in atlas_fields)+'}\n'
    text+='pub const ATLASES:&[AtlasDesc]=&[\n'+''.join('AtlasDesc{'+','.join(n+':'+str(e[n])for n,t in atlas_fields)+'},\n'for e in report.get('atlases',[]))+'];\n'
    text+='pub const SCENE_ATLAS_RANGES:&[(usize,usize)]=&['+','.join(f'({start},{end})'for start,end in ranges)+'];\n'
    text+='pub const REGION_SCENE_LOCAL:&[(usize,usize)]=&[\n'+''.join(f'({lookup[r["chunk_id"]][0]},{lookup[r["chunk_id"]][1]}),\n'for r in rows)+'];\n'
    text+='pub const SCENE_REGIONS:&[usize]=&['+','.join(map(str,representatives))+'];\n'
    text+=f'pub const SCENE_ALL_RAW_BYTES:usize={report["raw_resident_bytes"]};\n'
    text+=f'pub const SCENE_TOTAL_PAGES:usize={report["total_pages"]};\npub const SCENE_TOTAL_PALETTES:usize={report["total_palettes"]};\n'
    text+='pub static SCENE_BLACK_CORES:&[&[[u8;4]]]=&[\n'+''.join('&['+','.join('[%d,%d,%d,%d]'%tuple(c) for c in e.get('black_cores',[]))+'],\n' for e in scenes)+'];\n'
    text+='pub static SCENE_OPAQUE_CORES:&[&[[u8;4]]]=&[\n'+''.join('&['+','.join('[%d,%d,%d,%d]'%tuple(c) for c in e.get('opaque_cores',[]))+'],\n' for e in scenes)+'];\n'
    text+='pub static SCENE_TEXTURE_FLAGS:&[&[u8]]=&[\n'+''.join('&['+','.join(map(str,e.get('texture_flags',[])))+'],\n'for e in scenes)+'];\n'
    return text

def validate_geometry(bank,entry,metadata):
    """Recheck mandatory packet bounds through the actual serialized Scene view."""
    from scenery_geometry import packet_bound,check_camera_arithmetic,CAP,ACTOR_RESERVE,CHILD_CAPACITY
    regions={r['chunk_id']:r for r in metadata['regions']};sections=entry['sections'];proof=[]
    for local,source in enumerate(entry['source_rooms']):
        desc=struct.unpack_from('<10I',bank,sections['rooms']['offset']+local*40);chunk=desc[0]
        if chunk!=source['chunk_id'] or chunk not in regions:raise ValueError('Scene geometry region identity differs')
        mandatory=maximum=largest=0
        for i in range(desc[1]):
            rid=struct.unpack_from('<H',bank,desc[5]+i*2)[0];at=sections['draws']['offset']+rid*44
            texture=struct.unpack_from('<H',bank,at)[0];scale=struct.unpack_from('<i',bank,at+4)[0]
            coords=struct.unpack_from('<8i',bank,at+8);xy=list(zip(coords[::2],coords[1::2]))
            page,u,v,w,h=struct.unpack_from('<5H',bank,sections['textures']['offset']+texture*16)
            if u+w>256 or v+h>256:raise ValueError('Scene UV rectangle exceeds one page')
            bound=packet_bound(xy,w,h)
            if bound['packets']>CHILD_CAPACITY:raise ValueError('Scene draw child capacity exceeded')
            extent,_=check_camera_arithmetic(xy,scale,regions[chunk]['camera_bounds'])
            largest=max(largest,extent);maximum=max(maximum,bound['packets']);mandatory+=bound['packets']
        if mandatory+ACTOR_RESERVE>CAP:raise ValueError('Scene region packet reservation exceeded')
        proof.append({'chunk_id':chunk,'draws':desc[1],'mandatory_packets':mandatory,'maximum_draw_packets':maximum,'max_abs_projection':largest})
    return {'status':'PASS','capacity':CAP,'actor_reserve':ACTOR_RESERVE,'child_capacity':CHILD_CAPACITY,'regions':proof}

def decoder_command(report):
    if report.get('residency')=='scene_gate':
        args=['cargo','run','--quiet','--locked','--offline','--manifest-path',str(ROOT/'shared/hk-format/Cargo.toml'),
              '--example','check_scene_gate_load','--',str(report['arena_bytes'])]
        for e in report['scenes']:
            args.extend(('--scene',str(e['scene_id']),e['resident_stored_path'],e['resident_raw_path']))
            for a in report['atlases']:
                if a['scene_index']==e['scene_index']:
                    args.extend(('--atlas',str(a['kind']),str(a['first']),str(a['count']),a['stored_path'],a['raw_path']))
        return args
    args=['cargo','run','--quiet','--locked','--offline','--manifest-path',str(ROOT/'shared/hk-format/Cargo.toml'),
          '--example','check_scene_sequence','--',str(report['arena_bytes'])]
    for e in report['scenes']:args.extend((e.get('resident_stored_path',e['stored_path']),e.get('resident_raw_path',e['raw_path'])))
    if report.get('atlases'):
        args.append('--atlases')
        for e in report['atlases']:args.extend((e['stored_path'],e['raw_path']))
    return args

def pack_scenes(metadata=ROOT/'data/regions.json',output_dir=ROOT/'data/scenes',report_path=ROOT/'.hkpsx/packed-scenes.json',manifest_path=ROOT/'data/scene_manifest.rs',arena_bytes=SCENE_ARENA_BYTES, residency="joint", generate_world=True):
    metadata=Path(metadata);output_dir=Path(output_dir);report_path=Path(report_path);manifest_path=Path(manifest_path)
    source=metadata.read_bytes();meta=json.loads(source)
    if not meta.get('complete')or meta.get('pending_regions'):raise ValueError('Source region cook is incomplete')
    from world import generate
    if generate_world:generate(meta)
    entries=[];bodies={}
    for scene in sorted({r['scene_id']for r in meta['regions']}):
        inputs=[]
        for r in sorted(meta['regions'],key=lambda r:r['chunk_id']):
            if r['scene_id']!=scene:continue
            raw=(ROOT/r['path']).read_bytes()
            if sha(raw)!=r['sha256']:raise ValueError('Stale source room: '+str(r['chunk_id']))
            inputs.append((r['chunk_id'],raw))
        raw,e=build_scene(scene,inputs,page_limit=MAX_PAGES,scenery_cap=48,dedup_mode='blob',share_pixel_planes=True)
        if e['changed_static_textures']:raise ValueError('Lossless bank unexpectedly resampled scenery')
        e['geometry_packet_bound']=validate_geometry(raw,e,meta)
        e['black_cores']=black_cores(raw,e)
        e['opaque_cores']=opaque_cores(raw,e)
        stored=compressed(raw);resident=compact_resident(raw);resident_stored=compressed(resident)
        bodies[scene]=(raw,stored,resident,resident_stored)
        e.update(stored_bytes=len(stored),stored_fnv=fnv(stored),raw_fnv=fnv(raw),stored_sha256=sha(stored),raw_sha256=sha(raw),
                 raw_path=str(output_dir/f'scene_{scene}.hk'),stored_path=str(output_dir/f'scene_{scene}.hlzc'),
                 resident_raw_path=str(output_dir/f'scene_{scene}.resident.hk'),resident_stored_path=str(output_dir/f'scene_{scene}.resident.hlzc'),
                 resident_raw_len=len(resident),resident_raw_fnv=fnv(resident),resident_raw_sha256=sha(resident),
                 resident_stored_len=len(resident_stored),resident_stored_fnv=fnv(resident_stored),resident_stored_sha256=sha(resident_stored),
                 path=str(output_dir/f'scene_{scene}.resident.hlzc'),texture_flags=texture_flags(raw,e))
        entries.append(e)
    entries=allocate_scenes(entries,arena_bytes,residency=residency)
    atlases=[];atlas_bodies=[]
    for e in entries:
        raw=bodies[e['scene_id']][0]
        for spec,chunk in bootstrap_atlases(raw,e):
            stored=compressed(chunk);index=len(atlases)
            spec['first']+=e['page_base']if spec['kind']==0 else e['palette_base']
            path=output_dir/f'atlas_{index:02}'
            atlases.append(spec|dict(scene_index=e['scene_index'],raw_len=len(chunk),raw_fnv=fnv(chunk),raw_sha256=sha(chunk),
                stored_len=len(stored),stored_fnv=fnv(stored),stored_sha256=sha(stored),raw_path=str(path)+'.raw',stored_path=str(path)+'.hlzc',path=str(path)+'.hlzc'))
            atlas_bodies.append((chunk,stored))
    report={'residency':residency,'format':'HKPACKEDSCENES02','source_format':'HKSCNE01','resident_format':'HKSCNE02','source_metadata_path':str(metadata),'source_metadata_sha256':sha(source),
      'code_sha256':{str(p.relative_to(ROOT)):sha(p.read_bytes())for p in (ROOT/'host/scene_bank.py',Path(__file__).resolve(),ROOT/'tools/audit_resident_bank.py',ROOT/'host/region_delta.py',ROOT/'host/scenery_geometry.py',ROOT/'game/src/scenery_geometry.rs')},
      'region_count':len(meta['regions']),'scene_arena_bytes':arena_bytes,'raw_bytes':sum(e['raw_len']for e in entries),'stored_bytes':sum(e['stored_len']for e in entries),
      'arena_bytes':arena_bytes,'raw_resident_bytes':sum(aligned(e['resident_raw_len'])for e in entries),
      'resident_stored_bytes':sum(e['resident_stored_len']for e in entries),'atlas_raw_bytes':sum(e['raw_len']for e in atlases),'atlas_stored_bytes':sum(e['stored_len']for e in atlases),
      'total_pages':(max if residency=='scene_gate' else sum)(e['pages']for e in entries),'total_palettes':(max if residency=='scene_gate' else sum)(e['palettes']for e in entries),'scenes':entries,'atlases':atlases,
      'sequential_decoder_validation':'pending native pinned-SDK/incremental proof against these exact stored payloads and allocation capacities',
      'quality':'No new resizing or lossy texture aliasing. Shared planes preserve every sampled16bit word; streamed textures and strict black mask palette/index bytes remain exact.'}
    report['required_resident_bytes']=(max if residency=='scene_gate' else sum)(aligned(e['resident_raw_len'])for e in entries)
    rust=manifest(report,meta)
    # Admission checks precede all output mutations.
    output_dir.mkdir(parents=True,exist_ok=True);report_path.parent.mkdir(parents=True,exist_ok=True);manifest_path.parent.mkdir(parents=True,exist_ok=True)
    for e in entries:
        raw,stored,resident,resident_stored=bodies[e['scene_id']];Path(e['raw_path']).write_bytes(raw);Path(e['stored_path']).write_bytes(stored)
        Path(e['resident_raw_path']).write_bytes(resident);Path(e['resident_stored_path']).write_bytes(resident_stored)
    for e,(raw,stored)in zip(atlases,atlas_bodies):
        Path(e['raw_path']).write_bytes(raw);Path(e['stored_path']).write_bytes(stored)
    manifest_path.write_text(rust);report['manifest_path']=str(manifest_path);report['manifest_sha256']=sha(rust.encode())
    report_path.write_text(json.dumps(report,indent=2)+'\n')
    command=decoder_command(report);proof=subprocess.run(command,cwd=ROOT,capture_output=True,text=True)
    log=report_path.with_suffix('.decoder.log');log.write_text(proof.stdout+proof.stderr)
    proof_paths=[ROOT/('shared/hk-format/examples/check_scene_gate_load.rs' if residency=='scene_gate' else 'shared/hk-format/examples/check_scene_sequence.rs'),ROOT/'shared/hk-format/src/lib.rs',ROOT/'game/src/room_decode.rs',ROOT/'sdk.lock.json']
    scene_parser=ROOT/'shared/hk-format/src/scene.rs'
    if scene_parser.exists():proof_paths.append(scene_parser)
    report['sequential_decoder_validation']={'status':'PASS'if proof.returncode==0 else'FAIL','command':command,'log_path':str(log),
      'arena_bytes':arena_bytes,'code_sha256':{str(p.relative_to(ROOT)):sha(p.read_bytes())for p in proof_paths},'stdout':proof.stdout}
    report_path.write_text(json.dumps(report,indent=2)+'\n')
    if proof.returncode:raise ValueError('Sequential scene decoding failed; see '+str(log))
    return report

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('metadata',type=Path,nargs='?',default=ROOT/'data/regions.json');p.add_argument('--output',type=Path,default=ROOT/'data/scenes');p.add_argument('--report',type=Path,default=ROOT/'.hkpsx/packed-scenes.json');p.add_argument('--manifest',type=Path,default=ROOT/'data/scene_manifest.rs');p.add_argument('--residency',choices=('joint','scene_gate'),default='joint',help='joint keeps all scenes resident; scene_gate replaces one scene at authored gates');a=p.parse_args()
    r=pack_scenes(a.metadata,a.output,a.report,a.manifest,residency=a.residency);print(json.dumps({k:r[k]for k in ('arena_bytes','raw_resident_bytes','total_pages','total_palettes')},indent=2))
if __name__=='__main__':main()
