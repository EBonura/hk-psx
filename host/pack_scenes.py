"""Pack current HKROOM02 sources into lossless, jointly resident scene banks.

No sprite extraction/resampling and no disc writes. Every pack checks the exact
startup decode sequence before it can be used for final guest packaging.
"""
import argparse
import hashlib
import json
import subprocess
import struct
from pathlib import Path
from scene_bank import build_scene, compressed, fnv, aligned

ROOT=Path(__file__).resolve().parents[1]
SCENE_ARENA_BYTES=1179648
MAX_PAGES=20
MAX_PALETTES=1248  # Final legacy CLUT strip is reserved for independent Geo art.

def sha(data):return hashlib.sha256(data).hexdigest()

def allocate_scenes(entries, arena_bytes=SCENE_ARENA_BYTES, max_pages=MAX_PAGES, max_palettes=MAX_PALETTES):
    """Largest bank first gives its in-place decoder the entire startup arena."""
    if not entries or len({e['scene_id']for e in entries})!=len(entries):raise ValueError('Empty/duplicate scenes')
    offset=page_base=palette_base=0;result=[]
    for index,entry in enumerate(sorted(entries,key=lambda e:(-e['bytes'],e['scene_id']))):
        e=dict(entry)
        if min(e['bytes'],e['pages'],e['palettes'])<=0:raise ValueError('Invalid scene size')
        offset=aligned(offset)
        e.update(scene_index=index,chunk_id=index+1,raw_len=e['bytes'],raw_bytes=e['bytes'],stored_len=e['stored_bytes'],
                 ram_offset=offset,arena_capacity=arena_bytes-offset,page_base=page_base,palette_base=palette_base)
        offset+=e['bytes'];page_base+=e['pages'];palette_base+=e['palettes'];result.append(e)
    if offset>arena_bytes:raise ValueError('Scene RAM residency exceeds arena')
    if page_base>max_pages:raise ValueError('Scene atlas residency exceeds page limit')
    if palette_base>max_palettes:raise ValueError('Scene CLUT residency exceeds palette limit')
    return result

def manifest(report,metadata):
    scenes=report['scenes'];rows=sorted(metadata['regions'],key=lambda r:r['chunk_id'])
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
    text='// Generated lossless HKSCNE01 banks; local licensed data stays on disc.\n'
    text+='#[derive(Clone,Copy,Debug)]\npub struct SceneDesc{'+','.join('pub '+n+':'+t for n,t in fields)+'}\n'
    text+=f'pub const SCENE_ARENA_BYTES:usize={report["arena_bytes"]};\n'
    text+='pub const SCENE_MANIFEST:&[SceneDesc]=&[\n'+''.join('SceneDesc{'+','.join(n+':'+str(e[n])for n,t in fields)+'},\n'for e in scenes)+'];\n'
    text+='pub const REGION_SCENE_LOCAL:&[(usize,usize)]=&[\n'+''.join(f'({lookup[r["chunk_id"]][0]},{lookup[r["chunk_id"]][1]}),\n'for r in rows)+'];\n'
    text+='pub const SCENE_REGIONS:&[usize]=&['+','.join(map(str,representatives))+'];\n'
    text+=f'pub const SCENE_ALL_RAW_BYTES:usize={report["raw_resident_bytes"]};\n'
    text+=f'pub const SCENE_TOTAL_PAGES:usize={report["total_pages"]};\npub const SCENE_TOTAL_PALETTES:usize={report["total_palettes"]};\n'
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
    args=['cargo','run','--quiet','--locked','--offline','--manifest-path',str(ROOT/'shared/hk-format/Cargo.toml'),
          '--example','check_scene_sequence','--',str(report['arena_bytes'])]
    for e in report['scenes']:args.extend((e['stored_path'],e['raw_path']))
    return args

def pack_scenes(metadata=ROOT/'data/regions.json',output_dir=ROOT/'data/scenes',report_path=ROOT/'.hkpsx/packed-scenes.json',manifest_path=ROOT/'data/scene_manifest.rs',arena_bytes=SCENE_ARENA_BYTES):
    metadata=Path(metadata);output_dir=Path(output_dir);report_path=Path(report_path);manifest_path=Path(manifest_path)
    source=metadata.read_bytes();meta=json.loads(source)
    if not meta.get('complete')or meta.get('pending_regions'):raise ValueError('Source region cook is incomplete')
    from world import generate
    generate(meta)
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
        stored=compressed(raw);bodies[scene]=(raw,stored)
        e.update(stored_bytes=len(stored),stored_fnv=fnv(stored),raw_fnv=fnv(raw),stored_sha256=sha(stored),raw_sha256=sha(raw),
                 raw_path=str(output_dir/f'scene_{scene}.hk'),stored_path=str(output_dir/f'scene_{scene}.hlzc'),path=str(output_dir/f'scene_{scene}.hlzc'))
        entries.append(e)
    entries=allocate_scenes(entries,arena_bytes)
    report={'format':'HKPACKEDSCENES01','source_format':'HKSCNE01','source_metadata_path':str(metadata),'source_metadata_sha256':sha(source),
      'code_sha256':{str(p.relative_to(ROOT)):sha(p.read_bytes())for p in (ROOT/'host/scene_bank.py',Path(__file__).resolve(),ROOT/'tools/audit_resident_bank.py',ROOT/'host/region_delta.py',ROOT/'host/scenery_geometry.py',ROOT/'game/src/scenery_geometry.rs')},
      'region_count':len(meta['regions']),'scene_arena_bytes':arena_bytes,'raw_bytes':sum(e['raw_len']for e in entries),'stored_bytes':sum(e['stored_len']for e in entries),
      'arena_bytes':arena_bytes,'raw_resident_bytes':sum(aligned(e['raw_len'])for e in entries),
      'total_pages':sum(e['pages']for e in entries),'total_palettes':sum(e['palettes']for e in entries),'scenes':entries,
      'sequential_decoder_validation':'pending native pinned-SDK/incremental proof against these exact stored payloads and allocation capacities',
      'quality':'No new resizing or lossy texture aliasing. Shared planes preserve every sampled16bit word; streamed textures and strict black mask palette/index bytes remain exact.'}
    rust=manifest(report,meta)
    # Admission checks precede all output mutations.
    output_dir.mkdir(parents=True,exist_ok=True);report_path.parent.mkdir(parents=True,exist_ok=True);manifest_path.parent.mkdir(parents=True,exist_ok=True)
    for e in entries:
        raw,stored=bodies[e['scene_id']];Path(e['raw_path']).write_bytes(raw);Path(e['stored_path']).write_bytes(stored)
    manifest_path.write_text(rust);report['manifest_path']=str(manifest_path);report['manifest_sha256']=sha(rust.encode())
    report_path.write_text(json.dumps(report,indent=2)+'\n')
    command=decoder_command(report);proof=subprocess.run(command,cwd=ROOT,capture_output=True,text=True)
    log=report_path.with_suffix('.decoder.log');log.write_text(proof.stdout+proof.stderr)
    proof_paths=[ROOT/'shared/hk-format/examples/check_scene_sequence.rs',ROOT/'shared/hk-format/src/lib.rs',ROOT/'game/src/room_decode.rs',ROOT/'sdk.lock.json']
    scene_parser=ROOT/'shared/hk-format/src/scene.rs'
    if scene_parser.exists():proof_paths.append(scene_parser)
    report['sequential_decoder_validation']={'status':'PASS'if proof.returncode==0 else'FAIL','command':command,'log_path':str(log),
      'arena_bytes':arena_bytes,'code_sha256':{str(p.relative_to(ROOT)):sha(p.read_bytes())for p in proof_paths},'stdout':proof.stdout}
    report_path.write_text(json.dumps(report,indent=2)+'\n')
    if proof.returncode:raise ValueError('Sequential scene decoding failed; see '+str(log))
    return report

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('metadata',type=Path,nargs='?',default=ROOT/'data/regions.json');p.add_argument('--output',type=Path,default=ROOT/'data/scenes');p.add_argument('--report',type=Path,default=ROOT/'.hkpsx/packed-scenes.json');p.add_argument('--manifest',type=Path,default=ROOT/'data/scene_manifest.rs');a=p.parse_args()
    r=pack_scenes(a.metadata,a.output,a.report,a.manifest);print(json.dumps({k:r[k]for k in ('arena_bytes','raw_resident_bytes','total_pages','total_palettes')},indent=2))
if __name__=='__main__':main()
