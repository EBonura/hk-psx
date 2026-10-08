"""User-authorized >=95% static texture consolidation after a fresh room cook.

Every approximate replacement directly matches its final representative.
Geometry, record order and animation/effect-frame pixels are immutable. Dynamic
masks permit only verified constant index1 collapse to a shared4x4 tile, keeping
an integer UV grid for oversized geometry. No similarity chains or guest/disc
operations occur here.
"""
import copy
import hashlib
import json
import struct
import sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
from audit_texture_similarity import words
from texture_match_groups import all_static_matches,review_groups
from region_delta import layout,textures
from texture_dedup import deduplicate_room,clut_count
from cook import MAX_ROOM_TEXTURES
from materials import binary_black_palette
from quality import STATIC_PAGE_BUDGET,TEXTURE_BUDGET,ROOM_BYTE_BUDGET
from constant_textures import constant_texture_replacements,verify_constant_replacement
from scenery_geometry import texture_draws_safe,packet_bound,mandatory_packets,CAP,ACTOR_RESERVE
THRESHOLD=95
# One cooked view before the scene bank deduplicates it; the resident bank is
# bounded separately by the scene arena.
RUNTIME_ROOM_BYTES=ROOM_BYTE_BUDGET


def sha(data):return hashlib.sha256(data).hexdigest()
def json_bytes(value):return (json.dumps(value,indent=2)+'\n').encode()
def atomic(path,data):
    path.parent.mkdir(parents=True,exist_ok=True);temporary=path.with_suffix(path.suffix+'.similarity-tmp')
    temporary.write_bytes(data);temporary.replace(path)


def protected_textures(raw,row):
    c,_,_,_=layout(raw);draw_at=40+c[1]*16;frame_at=draw_at+c[2]*44
    protected={struct.unpack_from('<I',raw,frame_at+i*20)[0]for i in range(c[3])}
    draws={b['draw']for b in row.get('reveal_mask_bindings',[])}
    for binding in row.get('remote_mask_bindings',[]):draws.update(binding['fade']['draw_indices'])
    for owner in row.get('breakables',[]):
        for fade in owner.get('mask_fades',[]):draws.update(fade.get('draw_indices',[]))
    for i in draws:
        if not 0<=i<c[2]:raise ValueError('Mask draw reference outside room')
        protected.add(struct.unpack_from('<H',raw,draw_at+i*44)[0])
    if any(i>=c[1]for i in protected):raise ValueError('Protected texture reference outside room')
    return protected


def inventory(report,root=ROOT):
    items=[];lookup={};packs={};local_ids={};protected=set()
    for row in report['regions']:
        raw=(root/row['path']).read_bytes()
        if sha(raw)!=row['sha256']:raise ValueError('Similarity input pack differs from metadata')
        c,_,_,_=layout(raw);blobs=textures(raw);packs[row['chunk_id']]=raw
        modes=[set()for _ in blobs];at=40+c[1]*16
        for i in range(c[2]):
            d=raw[at+i*44:at+(i+1)*44];modes[struct.unpack_from('<H',d)[0]].add(d[43])
        source_names={};base=row.get('base_path')
        scene_path=(root/base).with_name('scene.json')if base else root/f'data/regions/region-{row["chunk_id"]:03}/scene.json'
        if scene_path.exists():
            oldmap=row.get('texture_deduplication',{}).get('old_to_canonical',list(range(c[1])))
            for d in json.loads(scene_path.read_text()).get('draws',[]):
                old=d['texture']
                if not 0<=old<len(oldmap):raise ValueError('Source texture map outside base atlas')
                source_names.setdefault(oldmap[old],set()).add(d.get('sprite',d.get('source','unknown')))
        local=[];guarded=protected_textures(raw,row)
        for i,blob in enumerate(blobs):
            stream=struct.unpack_from('<H',raw,40+i*16)[0]==65535;key=(stream,blob)
            if key not in lookup:
                w,h=struct.unpack_from('<HH',blob);tid=len(items);lookup[key]=tid
                items.append({'id':tid,'blob':blob,'words':words(blob),'w':w,'h':h,'stream':stream,
                    'black':binary_black_palette(blob[4:36]),'modes':set(),'sprites':set(),'references':[]})
            tid=lookup[key];local.append(tid);t=items[tid]
            t['modes'].update(modes[i]);t['sprites'].update(source_names.get(i,set()));t['references'].append([row['chunk_id'],i])
            if i in guarded:protected.add(tid)
        local_ids[row['chunk_id']]=local
    for item in items:item['modes']=tuple(sorted(item['modes']))
    return items,packs,local_ids,protected


def select_replacements(items,protected,threshold=THRESHOLD):
    eligible=[t for t in items if t['id']not in protected]
    matches=all_static_matches(eligible,threshold)
    groups=review_groups(matches['candidates'],eligible,threshold);mapping={}
    for group in groups:
        representative=group['representative']
        for member in group['members']:
            if member['id']==representative:continue
            if member['similarity']<threshold:raise ValueError('Indirect similarity replacement')
            mapping[member['id']]=representative
    if any(rep in mapping for rep in mapping.values()):raise ValueError('Similarity chain')
    if any(p['a']not in mapping and p['b']not in mapping for p in matches['candidates']):
        raise ValueError('Eligible threshold edge remains between final representatives')
    return mapping,matches,groups


def packet_protected(mapping,items,packs):
    """Members whose representative's size would break a referencing draw's packet bound."""
    return sorted(tid for tid,rep in mapping.items()
                  if any(not texture_draws_safe(packs[chunk],i,items[rep]['w'],items[rep]['h']) for chunk,i in items[tid]['references']))


def draw_packets(raw,replaced=None,groups=()):
    """Mandatory packet total of a pack when local textures in `replaced` (id->(w,h)) change size.
    `groups` are the view's decor groups, which count their largest frame only."""
    c,_,_,_=layout(raw);at=40+c[1]*16;packets=[];replaced=replaced or {}
    for i in range(c[2]):
        d=raw[at+i*44:at+(i+1)*44];tex=struct.unpack_from('<H',d)[0];co=struct.unpack_from('<8i',d,8)
        w,h=replaced.get(tex) or struct.unpack_from('<HH',raw,40+tex*16+6)
        packets.append(packet_bound(list(zip(co[::2],co[1::2])),w,h)['packets'])
    return mandatory_packets(packets,list(groups))


def reservation_guard(rows,packs,local_ids,items,mapping,constants_by_chunk):
    """Drop replacements until every region keeps its mandatory packet reservation.

    Smaller representatives subdivide into more packets, so a region that fit
    before similarity can exceed CAP afterwards. The largest single increase in
    the first offending region is dropped first; mutates mapping/constants.
    """
    dropped=[]
    while True:
        offender=None
        for row in rows:
            chunk=row['chunk_id'];raw=packs[chunk];local=local_ids[chunk]
            dims={i:(items[mapping[tid]]['w'],items[mapping[tid]]['h'])for i,tid in enumerate(local)if tid in mapping}
            dims.update({i:(4,4)for i in constants_by_chunk[chunk]})
            groups=[g['draws'] for g in row.get('decor',[])]
            if draw_packets(raw,dims,groups)+ACTOR_RESERVE<=CAP:continue
            if not dims:raise ValueError(f'region {chunk} exceeds the packet reservation before similarity')
            base=draw_packets(raw,None,groups)
            offender=(chunk,max(dims,key=lambda i:(draw_packets(raw,{i:dims[i]},groups)-base,i)));break
        if offender is None:return dropped
        chunk,i=offender;tid=local_ids[chunk][i]
        if i in constants_by_chunk[chunk]:del constants_by_chunk[chunk][i]
        else:del mapping[tid]
        dropped.append({'chunk_id':chunk,'texture':i,'global_id':tid})


def frame_textures(raw):
    c,_,_,_=layout(raw);at=40+c[1]*16+c[2]*44
    ids={struct.unpack_from('<I',raw,at+i*20)[0]for i in range(c[3])}
    if any(i>=c[1]for i in ids):raise ValueError('Frame texture outside atlas')
    return ids


def verify_records(before,after,mapping,protected,constant_ids=()):
    old,op,_,os=layout(before);new,np,_,ns=layout(after)
    if old[2:]!=new[2:]or len(mapping)!=old[1]:raise ValueError('Similarity changed record counts')
    ob,nb=textures(before),textures(after)
    constants=set(constant_ids)
    if constants&frame_textures(before):raise ValueError('Constant collapse changed frame texture')
    if any(not isinstance(i,int)or not 0<=i<old[1]for i in constants):raise ValueError('Constant texture outside atlas')
    for i in constants:
        if struct.unpack_from('<H',before,40+i*16)[0]==65535 or struct.unpack_from('<H',after,40+mapping[i]*16)[0]==65535:raise ValueError('Constant collapse changed streamed texture')
        verify_constant_replacement(ob[i],nb[mapping[i]])
    for i in protected:
        if i in constants:continue
        if ob[i]!=nb[mapping[i]]:raise ValueError('Similarity modified a protected animation/effect/mask texture')
        page=struct.unpack_from('<H',before,40+i*16)[0]
        if page!=65535 and struct.unpack_from('<I',before,36)[0]&1:
            oo=struct.unpack_from('<I',before,40+i*16+12)[0];no=struct.unpack_from('<I',after,40+mapping[i]*16+12)[0]
            if before[os+oo:os+oo+20]!=after[ns+no:ns+no+20]:raise ValueError('Similarity modified protected alpha cover')
    oa=40+old[1]*16;na=40+new[1]*16
    for count,size,width in [(old[2],44,2),(old[3],20,4)]:
        for i in range(count):
            a=before[oa+i*size:oa+(i+1)*size];b=after[na+i*size:na+(i+1)*size]
            if int.from_bytes(b[:width],'little')!=mapping[int.from_bytes(a[:width],'little')]or a[width:]!=b[width:]:raise ValueError('Similarity changed draw/frame geometry or material')
        oa+=count*size;na+=count*size
    if before[oa:op]!=after[na:np]:raise ValueError('Similarity changed clips or collision edges')


def compose_row(row,raw,packed,dedup):
    result=copy.deepcopy(row);mapping=dedup['old_to_canonical'];old_count=layout(raw)[0][1]
    prior=row.get('texture_deduplication',{});base_map=prior.get('old_to_canonical',list(range(old_count)))
    if any(not 0<=i<len(mapping)for i in base_map):raise ValueError('Prior atlas map outside similarity input')
    composed=[mapping[i]for i in base_map]
    requests=row.get('texture_request_to_canonical',[])
    if any(not 0<=i<len(composed)for i in requests):raise ValueError('Source request outside base atlas')
    result['texture_request_to_canonical']=[composed[i]for i in requests]
    result['texture_deduplication']={**prior,**dedup,'old_to_canonical':composed,
        'textures_before':prior.get('textures_before',old_count),'mapping_domain':'Immutable base/actor-append texture index to final atlas'}
    c,_,_,_=layout(packed)
    result.update(bytes=len(packed),sha256=sha(packed),pages=c[0],textures=c[1],draws=c[2],edges=c[5],
        stream_bytes=struct.unpack_from('<I',packed,32)[0],alpha_cover_bytes=dedup['alpha_cover_bytes'],animation_bytes=dedup['animation_bytes'])
    result['similarity_deduplication']={'input_sha256':sha(raw),'output_sha256':sha(packed),'input_to_final':mapping,
        'replaced_textures':dedup['replaced_textures'],'threshold':THRESHOLD,'frame_indices':'unchanged; frame texture references remapped'}
    return result


def postpack_similarity(report,root=ROOT):
    if not report.get('complete'):return False
    if report.get('similarity_dedup_policy'):
        # Re-running the already finalized pass is a verified no-op. New source
        # assets require the fresh full cook, never cumulative lossy passes.
        for row in report['regions']:
            if sha((root/row['path']).read_bytes())!=row['sha256']:raise ValueError('Fresh full cook required after similarity output mutation')
        return False
    items,packs,local_ids,protected=inventory(report,root)
    mapping,matches,groups=select_replacements(items,protected)
    # A smaller representative must keep every referencing draw inside the
    # native grid-repair packet bound; otherwise the member stays exact.
    packet_guard=packet_protected(mapping,items,packs)
    for tid in packet_guard:del mapping[tid]
    # A static occurrence cannot collapse if its exact texture is a frame in
    # any other room. Dynamic masks are separate: only proven constant index1
    # masks may shrink, with exact CLUT and full-quad sampled-word equivalence.
    global_frames={local_ids[chunk][i]for chunk,raw in packs.items()for i in frame_textures(raw)}
    constants_by_chunk={row['chunk_id']:constant_texture_replacements(packs[row['chunk_id']],[i for i,tid in enumerate(local_ids[row['chunk_id']])if tid in global_frames])for row in report['regions']}
    reservation_dropped=reservation_guard(report['regions'],packs,local_ids,items,mapping,constants_by_chunk)
    staged=[];records=[]
    for row in report['regions']:
        chunk=row['chunk_id'];raw=packs[chunk];local=local_ids[chunk]
        replacements={i:items[mapping[tid]]['blob']for i,tid in enumerate(local)if tid in mapping}
        constants=constants_by_chunk[chunk]
        if replacements.keys()&constants.keys():raise ValueError('Approximate and lossless replacement classes overlap')
        replacements.update(constants)
        packed,dedup=deduplicate_room(raw,replacements)
        verify_records(raw,packed,dedup['old_to_canonical'],protected_textures(raw,row),constants)
        c,_,_,_=layout(packed)
        # A CLUT slot is a distinct palette rather than a texture record, so the
        # two counts are checked against their own limits: the table against
        # what HKROOM02 can address, the palette words against the slot budget.
        cluts=clut_count(packed)
        if c[0]>STATIC_PAGE_BUDGET or c[1]>MAX_ROOM_TEXTURES or cluts>TEXTURE_BUDGET or len(packed)-c[0]*32768>RUNTIME_ROOM_BYTES:
            raise ValueError(f'Similarity result exceeds runtime budget in region{chunk}: '
                             f'{c[0]}pages/{c[1]}textures/{cluts}cluts/{len(packed)}bytes')
        final=compose_row(row,raw,packed,dedup)
        final['similarity_deduplication']['lossless_constant_indices']=sorted(constants)
        staged.append((final,packed))
        records.append({'chunk_id':chunk,'input_sha256':sha(raw),'output_sha256':sha(packed),'lossless_constant_indices':sorted(constants),**dedup})
    proof={'threshold':THRESHOLD,'metric':matches['scope'],'eligible_textures':matches['eligible_textures'],'compared_pairs':matches['compared_pairs'],
        'groups':groups,'matches':matches['candidates'],'protected_global_texture_ids':sorted(protected),'packet_protected_texture_ids':packet_guard,'reservation_protected':reservation_dropped,'global_frame_texture_ids':sorted(global_frames),'mapping':mapping,
        'constant_proof':'Only non-streamed textures never frame-referenced globally, exact strict mask CLUT, every meaningful original and final index1. Final4x4 sentinel1 tile retains integer-UV subdivision intervals and samples the identical word across the full unchanged quad; dynamic alternate-CLUT lookup remains index1. Alpha cover regenerated for the equivalent complete rectangle.',
        'textures':[{'id':t['id'],'sha256':sha(t['blob']),'dimensions':[t['w'],t['h']],'material_modes':t['modes'],'source_refs':t['references'],'sprites':sorted(t['sprites'])}for t in items],
        'regions':records,'code_sha256':{str(p.relative_to(ROOT)):sha(p.read_bytes())for p in (Path(__file__),ROOT/'tools/texture_match_groups.py',ROOT/'tools/audit_texture_similarity.py',ROOT/'host/texture_dedup.py',ROOT/'host/constant_textures.py')},
        'semantics':'User-authorized lossy static art consolidation >=95%; direct final representative, no chains; every animation/effect-frame unchanged; dynamic masks unchanged except independently proven constant-full-quad lossless collapse; record geometry/material/clip/collider bytes unchanged.'}
    # All regions and protections pass before publishing any replacement.
    for final,packed in staged:atomic(root/final['path'],packed)
    report['regions']=[r for r,p in staged];report['total_pack_bytes']=sum(r['bytes']for r,p in staged)
    report['similarity_dedup_policy']={'threshold':THRESHOLD,'representative_rule':'maximum remaining direct-match degree; area then stable ID ties; no transitive chains',
        'protected':'All frame-referenced and streamed pixels exact; dynamic masks/black exact except proven lossless full-quad constant collapse','removed_global_textures':len(mapping),
        'report':'.hkpsx/similarity-dedup.json','report_sha256':sha(json_bytes(proof))}
    first=next(((r,p)for r,p in staged if r['chunk_id']==report.get('initial_chunk_id',1)),None)
    if first:
        row,packed=first;atomic(root/'data/room.hk',packed)
        provenance=root/'.hkpsx/provenance.json'
        if provenance.exists():
            info=json.loads(provenance.read_text());info.update(pack_sha256=row['sha256'],pack_bytes=row['bytes'],textures=row['textures'],pages=row['pages'],
                palette_bytes=row['textures']*32,static_texture_bytes=row['pages']*32768,stream_bytes=row['stream_bytes'],
                alpha_cover_bytes=row['alpha_cover_bytes'],animation_bytes=row['animation_bytes'],texture_request_to_canonical=row['texture_request_to_canonical'],
                stream_textures=sum(struct.unpack_from('<H',packed,40+i*16)[0]==65535 for i in range(row['textures'])),similarity_deduplication=row['similarity_deduplication'])
            atomic(provenance,json_bytes(info))
    atomic(root/'.hkpsx/similarity-dedup.json',json_bytes(proof))
    return True
