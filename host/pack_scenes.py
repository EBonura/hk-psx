"""Pack current HKROOM02 sources into lossless scene banks.

No sprite extraction/resampling and no disc writes. Every pack checks the exact
startup decode sequence before it can be used for final guest packaging.
"""
import argparse
import hashlib
import json
import subprocess
import struct
import sys
from pathlib import Path
from scene_bank import build_scene, compressed, fnv, aligned, compact_resident, bootstrap_atlases, texture_flags, with_texture_attributes

ROOT=Path(__file__).resolve().parents[1]
def rel(path):
    """Checkout-relative when inside it (see source.rel)."""
    p=Path(path).resolve()
    return str(p.relative_to(ROOT)) if p.is_relative_to(ROOT) else str(path)
sys.path.insert(0,str(ROOT/'tools'))
# Current scene pair plus the admitted metadata overlay require
# 321,956 + 32,072 = 354,028 bytes. Keep four-byte alignment and a four-byte
# guard while retaining the exact sequential in-place decoder validation.
SCENE_ARENA_BYTES=354032
# Nineteen of the twenty 256x256 4bpp page slots the VRAM layout can address.
# The twentieth is the animation cache's second region; see
# hk_cache::residency::STATIC_PAGES. The largest of the 45 cooked scenes,
# Tutorial_01, is 18 pages and the next largest is 9.
MAX_PAGES=19
MAX_PALETTES=1248  # Final legacy CLUT strip is reserved for independent Geo art.

GEO_REPORT=ROOT/'.hkpsx/geo-provenance.json'

def sha(data):return hashlib.sha256(data).hexdigest()

def geo_enemies():
    """Enemy Geo payouts per scene id, out of host/geo.py's report.

    These used to link as one flat GEO_ENEMIES table covering every scene at
    once. Each scene's bank carries its own now, and this is the only place the
    Geo cook and the metadata cook meet, so an absent report is an error rather
    than a scene that silently drops no Geo.
    """
    if not GEO_REPORT.is_file():
        raise FileNotFoundError('run host/geo.py first; its report is the enemy payout evidence')
    report=json.loads(GEO_REPORT.read_text())
    if report['format']!='HKGEO01':raise ValueError('the Geo report format changed')
    per_scene={}
    for enemy in report['enemies']:per_scene.setdefault(enemy['scene'],[]).append(enemy)
    return per_scene

def world_metadata_banks(meta, output_dir, scene_raw_fnv=None):
    """Cook the checked HKWMTA01 banks that belong to each packed scene.

    Metadata is emitted beside the ignored pack report. Keeping it as a
    separate bank lets the guest admission path validate world state before
    any borrowed view is published; the final packer assigns its chunk after
    scene coverage without changing earlier chunk identities.
    The scene and region catalogue are the same objects used for HKSCNE, so a
    bank cannot silently drift to a different room set.
    """
    from world_metadata import encode_scene
    output_dir=Path(output_dir)
    banks=[];bodies=[]
    source_scenes=meta.get('scenes')
    if source_scenes is None:
        # Unit fixtures for the legacy scene packer intentionally contain only
        # geometry identity.  They do not describe gameplay metadata and must
        # not manufacture a partial HKWMTA bank.
        if any('activation_bounds' not in row for row in meta.get('regions', [])):
            return [], []
        source_scenes=[{'scene_id':scene,'scene_name':f'Scene {scene}','file':f'scene-{scene}'}
                       for scene in sorted({row['scene_id'] for row in meta['regions']})]
    scene_raw_fnv=scene_raw_fnv or {}
    enemies=geo_enemies()
    for scene in sorted(source_scenes,key=lambda value:value['scene_id']):
        rows=[row for row in meta['regions'] if row['scene_id']==scene['scene_id']]
        source_scene=dict(scene)
        if scene['scene_id'] in scene_raw_fnv: source_scene['raw_fnv']=scene_raw_fnv[scene['scene_id']]
        controllers=meta.get('reveal_mask_scenes',{}).get(str(scene['scene_id']),{}).get('controllers',[])
        source_scene['reveal_controllers']=len(controllers)
        source_scene['reveal_mask_controllers']=controllers
        source_scene['geo_enemies']=enemies.get(scene['scene_id'],[])
        raw,descriptor=encode_scene(source_scene,rows)
        stored=compressed(raw)
        path=output_dir/f"scene_{scene['scene_id']}"
        bank=dict(descriptor,raw_len=len(raw),stored_len=len(stored),stored_fnv=fnv(stored),stored_sha256=sha(stored),
                  bank_fnv=fnv(raw),raw_sha256=sha(raw),raw_path=rel(path)+'.hkwm',
                  stored_path=rel(path)+'.hkwm.z',path=rel(path)+'.hkwm.z',
                  fingerprint=raw[16:48].hex())
        banks.append(bank);bodies.append((bank,raw,stored))
    return banks,bodies

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
        if page==65535 or w==0 or h==0 or w>252 or h>252:out.append([0,0,0,0]);continue
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
            if index>=65536 or local>=256:raise ValueError('Scene/room index exceeds the (u16,u8) slot table')
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
    meta_fields=[('scene_id','u32'),('raw_len','usize'),('raw_fnv','u32'),('bank_fnv','u32'),('stored_len','usize'),('stored_fnv','u32'),('chunk_id','usize')]
    text+='#[derive(Clone,Copy,Debug)]\npub struct WorldMetaDesc{'+','.join('pub '+n+':'+t for n,t in meta_fields)+',pub fingerprint:[u8;32]}\n'
    text+='pub const WORLD_META_ARENA_BYTES:usize='+str(max((aligned(e['raw_len']) for e in report.get('world_metadata',[])),default=0))+';\n'
    text+='pub const WORLD_META_MANIFEST:&[WorldMetaDesc]=&[\n'+''.join(
        'WorldMetaDesc{'+','.join(n+':'+str(e[n]) for n,t in meta_fields)+',fingerprint:['+','.join(str(value) for value in bytes.fromhex(e['fingerprint']))+']},\n'
        for e in report.get('world_metadata',[]))+'];\n'
    atlas_fields=[('scene_index','usize'),('stored_len','usize'),('stored_fnv','u32'),('raw_len','usize'),('raw_fnv','u32'),('kind','u8'),('first','usize'),('count','usize')]
    text+='#[derive(Clone,Copy,Debug)]\npub struct AtlasDesc{'+','.join('pub '+n+':'+t for n,t in atlas_fields)+'}\n'
    text+='pub const ATLASES:&[AtlasDesc]=&[\n'+''.join('AtlasDesc{'+','.join(n+':'+str(e[n])for n,t in atlas_fields)+'},\n'for e in report.get('atlases',[]))+'];\n'
    text+='pub const SCENE_ATLAS_RANGES:&[(usize,usize)]=&['+','.join(f'({start},{end})'for start,end in ranges)+'];\n'
    text+='pub const REGION_SCENE_LOCAL:&[(u16,u8)]=&[\n'+''.join(f'({lookup[r["chunk_id"]][0]},{lookup[r["chunk_id"]][1]}),\n'for r in rows)+'];\n'
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
        maximum=largest=0;packets=[]
        for i in range(desc[1]):
            rid=struct.unpack_from('<H',bank,desc[5]+i*2)[0];at=sections['draws']['offset']+rid*44
            texture=struct.unpack_from('<H',bank,at)[0];scale=struct.unpack_from('<i',bank,at+4)[0]
            coords=struct.unpack_from('<8i',bank,at+8);xy=list(zip(coords[::2],coords[1::2]))
            page,u,v,w,h=struct.unpack_from('<5H',bank,sections['textures']['offset']+texture*16)
            if u+w>256 or v+h>256:raise ValueError('Scene UV rectangle exceeds one page')
            bound=packet_bound(xy,w,h)
            if bound['packets']>CHILD_CAPACITY:raise ValueError('Scene draw child capacity exceeded')
            extent,_=check_camera_arithmetic(xy,scale,regions[chunk]['camera_bounds'])
            largest=max(largest,extent);maximum=max(maximum,bound['packets']);packets.append(bound['packets'])
        from scenery_geometry import mandatory_packets
        mandatory=mandatory_packets(packets,[g['draws'] for g in regions[chunk].get('decor',[])])
        if mandatory+ACTOR_RESERVE>CAP:raise ValueError(f'Scene region {chunk} packet reservation exceeded: {mandatory}+{ACTOR_RESERVE} > {CAP}')
        proof.append({'chunk_id':chunk,'draws':desc[1],'mandatory_packets':mandatory,'maximum_draw_packets':maximum,'max_abs_projection':largest})
    return {'status':'PASS','capacity':CAP,'actor_reserve':ACTOR_RESERVE,'child_capacity':CHILD_CAPACITY,'regions':proof}

def decoder_command(report):
    if report.get('residency')=='scene_gate':
        args=['cargo','run','--quiet','--locked','--offline','--manifest-path',str(ROOT/'shared/hk-format/Cargo.toml'),
              '--example','check_scene_gate_load','--',str(report['arena_bytes'])]
        for e in report['scenes']:
            args.extend(('--scene',str(e['scene_id']),e['resident_stored_path'],e['resident_raw_path']))
            for bank in report.get('world_metadata',[]):
                if bank['scene_id']==e['scene_id']:args.extend(('--meta',bank['stored_path'],bank['raw_path']))
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

def scene_arena_bytes(entries,meta,bodies,output_dir,residency):
    """Arena the guest reserves, measured from the pack rather than fixed.

    Every payload decoded in place there (scene, atlas chunks, metadata bank in
    the tail) is read as whole CD sectors and relocated to the end of its slice,
    so each needs both its sector-rounded stored size and raw+LZ4 margin; the
    metadata tail is the largest bank's requirement because the guest checks
    every scene against that maximum. Four-byte guard, four-byte aligned.
    """
    sector=lambda n:(n+2047)//2048*2048
    banks,_=world_metadata_banks(meta,Path(output_dir)/'unused',{e['scene_id']:e['raw_fnv'] for e in entries})
    tails={b['scene_id']:max(aligned(b['raw_len']),sector(b['stored_len'])) for b in banks}
    widest=max(tails.values(),default=0)
    def need(e):
        # Under scene-gate residency only one scene and its own bank are ever
        # resident, so each scene needs room for its own bank, not the widest
        # one: the widest (Tutorial_01's) set the tail for every scene, and
        # Crossroads_10, the largest resident scene, paid 58 KiB for it.
        meta_tail=tails.get(e['scene_id'],widest) if residency=='scene_gate' else widest
        resident=aligned(e['resident_raw_len']);margin=(e['resident_stored_len']>>8)+32
        atlas=max((max(len(chunk)+(len(stored)>>8)+32,sector(len(stored))) for _,chunk,stored in atlas_chunks(bodies[e['scene_id']][0],e)),default=0)
        return max(resident+max(meta_tail,margin),sector(e['resident_stored_len']),atlas)
    if residency=='scene_gate':peak=max(need(e) for e in entries)
    else:peak=sum(aligned(e['resident_raw_len']) for e in entries)+max(need(e)-aligned(e['resident_raw_len']) for e in entries)
    return aligned(peak+4)

def atlas_chunks(raw,entry):
    """Bootstrap atlas chunks with their stored bytes; independent of allocation."""
    return [(spec,chunk,compressed(chunk)) for spec,chunk in bootstrap_atlases(raw,entry)]

def pack_scenes(metadata=ROOT/'data/regions.json',output_dir=ROOT/'data/scenes',report_path=ROOT/'.hkpsx/packed-scenes.json',manifest_path=ROOT/'data/scene_manifest.rs',arena_bytes=None, residency="joint", generate_world=True):
    metadata=Path(metadata);output_dir=Path(output_dir);report_path=Path(report_path);manifest_path=Path(manifest_path)
    source=metadata.read_bytes();meta=json.loads(source)
    if not meta.get('complete')or meta.get('pending_regions'):raise ValueError('Source region cook is incomplete')
    from world import generate
    if generate_world:generate(meta)
    entries=[];bodies={}
    # Screen/LinearDodge haze becomes additive light in the bank itself, so the
    # certificates and cores below see its texels as see-through (host/scene_grading.py).
    import scene_grading
    blend_kinds={int(k):v.get('blend',{}) for k,v in scene_grading.load(meta)['scenes'].items()}
    for scene in sorted({r['scene_id']for r in meta['regions']}):
        inputs=[]
        for r in sorted(meta['regions'],key=lambda r:r['chunk_id']):
            if r['scene_id']!=scene:continue
            raw=(ROOT/r['path']).read_bytes()
            if sha(raw)!=r['sha256']:raise ValueError('Stale source room: '+str(r['chunk_id']))
            inputs.append((r['chunk_id'],raw))
        raw,e=build_scene(scene,inputs,page_limit=MAX_PAGES,scenery_cap=48,dedup_mode='blob',share_pixel_planes=True)
        if e['changed_static_textures']:raise ValueError('Lossless bank unexpectedly resampled scenery')
        raw,additive=scene_grading.additive_palettes(raw,e,meta,blend_kinds.get(scene,{}))
        e['additive_palettes']={str(p):k for p,k in sorted(additive.items())}
        e['geometry_packet_bound']=validate_geometry(raw,e,meta)
        e['black_cores']=black_cores(raw,e)
        e['opaque_cores']=opaque_cores(raw,e)
        # Flags and cores ride in the resident bank; nothing per texture links.
        stored=compressed(raw);resident=with_texture_attributes(compact_resident(raw),texture_flags(raw,e),e['black_cores'],e['opaque_cores']);resident_stored=compressed(resident)
        bodies[scene]=(raw,stored,resident,resident_stored)
        e.update(stored_bytes=len(stored),stored_fnv=fnv(stored),raw_fnv=fnv(raw),stored_sha256=sha(stored),raw_sha256=sha(raw),
                 raw_path=rel(output_dir/f'scene_{scene}.hk'),stored_path=rel(output_dir/f'scene_{scene}.hlzc'),
                 resident_raw_path=rel(output_dir/f'scene_{scene}.resident.hk'),resident_stored_path=rel(output_dir/f'scene_{scene}.resident.hlzc'),
                 resident_raw_len=len(resident),resident_raw_fnv=fnv(resident),resident_raw_sha256=sha(resident),
                 resident_stored_len=len(resident_stored),resident_stored_fnv=fnv(resident_stored),resident_stored_sha256=sha(resident_stored),
                 path=rel(output_dir/f'scene_{scene}.resident.hlzc'),texture_flags=texture_flags(raw,e))
        entries.append(e)
    if arena_bytes is None:arena_bytes=scene_arena_bytes(entries,meta,bodies,report_path.parent,residency)
    entries=allocate_scenes(entries,arena_bytes,residency=residency)
    metadata_output=report_path.parent/'world-metadata-packed'
    world_banks,world_bodies=world_metadata_banks(meta,metadata_output,{e['scene_id']:e['raw_fnv'] for e in entries})
    # The guest indexes every manifest by scene_index (allocation order), so the
    # metadata banks must follow SCENE_MANIFEST order, not scene_id order.
    position={e['scene_id']:e['scene_index'] for e in entries}
    world_banks.sort(key=lambda bank:position[bank['scene_id']]);world_bodies.sort(key=lambda body:position[body[0]['scene_id']])
    atlases=[];atlas_bodies=[]
    # The original's per-scene ambient, curves and saturation are constants:
    # bake them into the palette atlases the guest uploads (host/scene_grading.py).
    # Scene banks keep the source words, so flags, cores and certificates that
    # read the bank still see the exact cooked palettes.
    grades=scene_grading.gradings(meta);means={}
    for e in entries:
        raw=bodies[e['scene_id']][0];actors=scene_grading.streamed_palettes(raw,e);grade=grades.get(e['scene_id'])
        additive={int(p):k for p,k in e['additive_palettes'].items()}
        # A metadata file without scene records (the unit fixtures) has no grading.
        if grade:
            means[e['scene_id']]=scene_grading.scenery_mean(raw,e,grade,actors,additive)
            background=scene_grading.scenery_background(raw,e,grade,actors,additive)
        for spec,chunk in bootstrap_atlases(raw,e):
            if spec['kind']==1 and grade:chunk=scene_grading.grade_palette_chunk(chunk,spec['first'],grade,actors,additive,background)
            stored=compressed(chunk);index=len(atlases)
            spec['first']+=e['page_base']if spec['kind']==0 else e['palette_base']
            path=output_dir/f'atlas_{index:02}'
            atlases.append(spec|dict(scene_index=e['scene_index'],raw_len=len(chunk),raw_fnv=fnv(chunk),raw_sha256=sha(chunk),
                stored_len=len(stored),stored_fnv=fnv(stored),stored_sha256=sha(stored),raw_path=rel(path)+'.raw',stored_path=rel(path)+'.hlzc',path=rel(path)+'.hlzc'))
            atlas_bodies.append((chunk,stored))
    if grades:scene_grading.write_rust(meta,means)
    report={'residency':residency,'format':'HKPACKEDSCENES02','source_format':'HKSCNE01','resident_format':'HKSCNE02','source_metadata_path':str(metadata),'source_metadata_sha256':sha(source),
      'code_sha256':{str(p.relative_to(ROOT)):sha(p.read_bytes())for p in (ROOT/'host/scene_bank.py',Path(__file__).resolve(),ROOT/'host/scene_grading.py',ROOT/'tools/audit_resident_bank.py',ROOT/'host/region_delta.py',ROOT/'host/scenery_geometry.py',ROOT/'game/src/scenery_geometry.rs')},
      'region_count':len(meta['regions']),'scene_arena_bytes':arena_bytes,'raw_bytes':sum(e['raw_len']for e in entries),'stored_bytes':sum(e['stored_len']for e in entries),
      'arena_bytes':arena_bytes,'raw_resident_bytes':sum(aligned(e['resident_raw_len'])for e in entries),
      'resident_stored_bytes':sum(e['resident_stored_len']for e in entries),'atlas_raw_bytes':sum(e['raw_len']for e in atlases),'atlas_stored_bytes':sum(e['stored_len']for e in atlases),
      'total_pages':(max if residency=='scene_gate' else sum)(e['pages']for e in entries),'total_palettes':(max if residency=='scene_gate' else sum)(e['palettes']for e in entries),'scenes':entries,'atlases':atlases,
      'world_metadata':world_banks,'world_metadata_policy':'generated, checksummed and assigned after scene coverage; guest admission is wired but this build has not replaced the playable disc',
      'sequential_decoder_validation':'pending native pinned-SDK/incremental proof against these exact stored payloads and allocation capacities',
      'quality':'No new resizing or lossy texture aliasing. Shared planes preserve every sampled16bit word; streamed textures and strict black mask palette/index bytes remain exact.'}
    # Ambient/focus/coverage chunk numbering is owned by the packer contract.
    # Keep metadata after coverage so existing scene, audio and atlas IDs stay
    # stable while the guest migration is staged.
    ambience_report=ROOT/'.hkpsx/ambience.json'
    clip_count=len(json.loads(ambience_report.read_text()).get('clips',[])) if ambience_report.exists() else 0
    metadata_first=len(entries)+clip_count+len(atlases)+2+len(entries)
    for index,bank in enumerate(world_banks): bank['chunk_id']=metadata_first+index
    report['world_metadata_base_chunk']=metadata_first
    report['world_metadata_policy']='generated, checksummed and assigned after scene coverage; guest admission is wired but this build has not replaced the playable disc'
    report['required_resident_bytes']=(max if residency=='scene_gate' else sum)(aligned(e['resident_raw_len'])for e in entries)
    rust=manifest(report,meta)
    # Admission checks precede all output mutations.
    output_dir.mkdir(parents=True,exist_ok=True);report_path.parent.mkdir(parents=True,exist_ok=True);manifest_path.parent.mkdir(parents=True,exist_ok=True)
    for e in entries:
        raw,stored,resident,resident_stored=bodies[e['scene_id']];(ROOT/e['raw_path']).write_bytes(raw);(ROOT/e['stored_path']).write_bytes(stored)
        (ROOT/e['resident_raw_path']).write_bytes(resident);(ROOT/e['resident_stored_path']).write_bytes(resident_stored)
    for e,(raw,stored)in zip(atlases,atlas_bodies):
        (ROOT/e['raw_path']).write_bytes(raw);(ROOT/e['stored_path']).write_bytes(stored)
    for e,raw,stored in world_bodies:
        (ROOT/e['raw_path']).parent.mkdir(parents=True,exist_ok=True)
        (ROOT/e['raw_path']).write_bytes(raw);(ROOT/e['stored_path']).write_bytes(stored)
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
