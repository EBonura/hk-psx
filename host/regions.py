"""Cook bounded regions of original Tutorial_01 and full Town scenery.

Partitioning changes residency only. Each region keeps every supported sprite
layer intersecting its camera range at the existing quality settings. A region
that exceeds a fixed budget is split spatially, never silently truncated.
"""
import sys
import argparse
import copy
import hashlib
import json
import math
import shutil
import struct
from pathlib import Path

from source import ROOT, Source, dump
from scene import Scene
from cook import cook, CAM_X, CAM_Y, Atlas, append_actor_art
from actors import actor_sources, postpack_pogo
from world import postpack_checkpoints, postpack_masks
from reveal_masks import postpack_reveal_masks
from quality import SCENE_TABLE, GRID_SCENE_LAYOUTS, REGION_LAYOUT, TOWN_EXTENSION_LAYOUT, SCENERY_MAX_AXIS, STATIC_PAGE_BUDGET, ROOM_BYTE_BUDGET, TEXTURE_BUDGET

SCENES = [dict(scene) for scene in SCENE_TABLE]


def intersects(a, b):
    return a[0] <= b[2] and b[0] <= a[2] and a[1] <= b[3] and b[1] <= a[3]


def region_spec(scene, bounds):
    x0,y0,x1,y1 = bounds
    global_camera = scene['camera_global_bounds']
    cx = [max(global_camera[0],min(global_camera[2],x)) for x in (x0,x1)]
    cy = [max(global_camera[1],min(global_camera[3],y+2)) for y in (y0,y1)]
    return {'scene_id': scene['scene_id'], 'scene_name': scene['scene_name'],
            'scene_file': scene['file'], 'activation_bounds': list(bounds),
            'camera_bounds': [cx[0],cy[0],cx[1],cy[1]], 'camera_x': cx, 'camera_y': cy,
            'collision_bounds': [x0-3,y0-5,x1+3,y1+5],
            'interaction_bounds': [x0-2,y0-2,x1+2,y1+2],
            'grass_bounds': [x0-2,y0-2,x1+2,y1+2], 'allow_slopes': True}


def initial_regions():
    initial = region_spec(SCENES[0], [15,-5,62,25])
    # Preserve the existing opening view and initial interaction test route.
    initial.update(camera_x=list(CAM_X), camera_y=list(CAM_Y),
                   camera_bounds=[CAM_X[0],CAM_Y[0],CAM_X[1],CAM_Y[1]],
                   collision_bounds=[15,-5,78,35], grass_bounds=[20,0,70,25])
    pending=[]
    for scene in SCENES:
        left,bottom,right,top=scene['runtime_bounds']
        for y in range(bottom,top,16):
            for x in range(left,right,24):
                bounds=[x,y,min(x+24,right),min(y+16,top)]
                base=initial['activation_bounds']
                if scene['scene_id']==0 and base[0]<=bounds[0] and base[1]<=bounds[1] and base[2]>=bounds[2] and base[3]>=bounds[3]:
                    continue
                pending.append(region_spec(scene,bounds))
    # Expose the immediate first-door continuation first for incremental smoke
    # testing. Remaining IDs stay deterministic for this region plan revision.
    pending.sort(key=lambda r:(r['scene_id'],
                 0 if r['activation_bounds'][:2]==[48,11] else 1,
                 r['activation_bounds'][1],r['activation_bounds'][0]))
    return [initial]+pending


def fixed_regions():
    """Keep the measured layout stable across texture quality changes."""
    # Keep every existing chunk ID and Tutorial view. Town's old x40 clamp was
    # the development coverage edge; extending it allows continuous traversal
    # into the new views without changing screen scale or the gate's x12 clamp.
    result=[region_spec(SCENES[scene],bounds) for scene,bounds in REGION_LAYOUT]
    for bounds,camera in TOWN_EXTENSION_LAYOUT:
        region=region_spec(SCENES[1],bounds)
        region.update(camera_bounds=list(camera),camera_x=[camera[0],camera[2]],
                      camera_y=[camera[1],camera[3]])
        result.append(region)
    for scene_id,boxes in sorted(GRID_SCENE_LAYOUTS.items()):
        for bounds in boxes:
            result.append(region_spec(SCENES[scene_id],bounds))
    initial=initial_regions()[0]
    if result[0]['activation_bounds'] != initial['activation_bounds']:
        raise ValueError('fixed layout opening differs from initial room')
    result[0]=initial
    return result


def split_region(region):
    x0,y0,x1,y1=region['activation_bounds']
    if max(x1-x0,y1-y0)<=3:
        raise ValueError(f'One camera view exceeds residency budgets at {region["activation_bounds"]}')
    if x1-x0 >= y1-y0:
        mid=(x0+x1)/2;pieces=([x0,y0,mid,y1],[mid,y0,x1,y1])
    else:
        mid=(y0+y1)/2;pieces=([x0,y0,x1,mid],[x0,mid,x1,y1])
    scene=SCENES[region['scene_id']]
    return [region_spec(scene,piece) for piece in pieces]


def postpack_camera_locks(report):
    """Attach every enabled CameraLockArea whose trigger touches a region's
    activation envelope, with -1 / 9999 limits resolved to the scene's camera
    bounds (CameraController.LockToArea substitutes 14.6 / xLimit / 8.3 / yLimit,
    which are those bounds in camera-centre units)."""
    by_scene={scene['scene_name']:scene for scene in report['scenes']}
    for region in report['regions']:
        scene=by_scene[region['scene_name']];g=scene['camera_global_bounds'];b=region['activation_bounds']
        locks=[]
        for lock in scene.get('camera_locks',[]):
            if not lock['enabled'] or 'trigger_bounds' not in lock:continue
            t=lock['trigger_bounds']
            if not (t[0]<=b[2] and t[2]>=b[0] and t[1]<=b[3] and t[3]>=b[1]):continue
            lim=lock['serialized_limits']
            xmin=g[0] if lim['cameraXMin']<0 else min(max(lim['cameraXMin'],g[0]),g[2])
            xmax=g[2] if lim['cameraXMax']<0 else min(max(lim['cameraXMax'],g[0]),g[2])
            ymin=g[1] if lim['cameraYMin']<0 else min(max(lim['cameraYMin'],g[1]),g[3])
            ymax=g[3] if lim['cameraYMax']<0 else min(max(lim['cameraYMax'],g[1]),g[3])
            # Crossroads_18 authors xMin 26.27 above xMax 25.43; Mathf.Clamp then
            # pins the camera near either value. The guest clamps to the sorted pair.
            locks.append({'source':lock['source'],'name':lock['name'],'bounds':t,
                          'limit_points':[[(xmin,ymin),(xmax,ymax)]],
                          'prevent_look_down':bool(lim.get('preventLookDown')),'prevent_look_up':bool(lim.get('preventLookUp'))})
        region['camera_locks']=locks


def scene_metadata(sc, info):
    result=dict(info, gates=[], camera_locks=[])
    result['grass_state_count']=sum(typ=='GrassCut' for typ,_ in sc.objects.values())
    for sid,(typ,tree) in sc.objects.items():
        if typ not in ('TransitionPoint','CameraLockArea'):
            continue
        gid=tree['m_GameObject']['m_PathID']
        if not sc.active(gid):
            continue
        row={'source':f'{info["file"]}:{sid}','name':sc.gos[gid]['m_Name'],
             'position':sc.point(gid),'enabled':bool(tree['m_Enabled'])}
        for _,(kind,col) in sc.objects.items():
            if kind!='BoxCollider2D' or col['m_GameObject']['m_PathID']!=gid:
                continue
            off=col['m_Offset'];size=col['m_Size']
            points=[sc.point(gid,x+off['x'],y+off['y']) for x,y in [
                (-size['x']/2,-size['y']/2),(size['x']/2,-size['y']/2),
                (size['x']/2,size['y']/2),(-size['x']/2,size['y']/2)]]
            row['trigger_bounds']=[min(p[0] for p in points),min(p[1] for p in points),max(p[0] for p in points),max(p[1] for p in points)]
            break
        if typ=='TransitionPoint':
            row.update(target_scene=tree['targetScene'],entry_point=tree['entryPoint'],entry_offset=tree['entryOffset'])
            # Top gates ship with a disabled collider and a `Delay Collider` FSM
            # (Wait, then SetCollider active), so the arriving Knight cannot
            # bounce straight back through the gate it fell out of.
            row['collider_delay']=0.
            for _,(kind,comp) in sc.objects.items():
                if kind!='PlayMakerFSM' or comp['m_GameObject']['m_PathID']!=gid or comp['fsm']['name']!='Delay Collider':continue
                from focus import action_fields
                for st in comp['fsm']['states']:
                    data=st['actionData']
                    for i,name in enumerate(data['actionNames']):
                        if name.endswith('.Wait'):
                            value=action_fields(data,i)['time'];value=value['value'] if isinstance(value,dict) else value
                            row['collider_delay']=float(value)
            row['collider_initially_enabled']=all(col.get('m_Enabled',1) for _,(kind,col) in sc.objects.items() if kind=='BoxCollider2D' and col['m_GameObject']['m_PathID']==gid)
            result['gates'].append(row)
        else:
            row['serialized_limits']={k:v for k,v in tree.items() if k.startswith('camera') or k.startswith('preventLook')}
            result['camera_locks'].append(row)
    return result


def cook_fingerprints(source):
    """Inputs and code identity for per-region cook reuse.

    Code excludes quality.py (the scene catalog) so growing the catalog keeps
    earlier regions; the budget constants it contributes are hashed by value.
    """
    from quality import SCENERY_MAX_AXIS, STATIC_PAGE_BUDGET, TEXTURE_BUDGET, ROOM_BYTE_BUDGET
    def sha(path):
        with Path(path).open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
    code=hashlib.sha256()
    # host/cook_inputs.txt is the shared list; the catalog (quality.py) and this
    # orchestrator do not change a region's cooked bytes, the region spec does.
    for line in (ROOT/'host/cook_inputs.txt').read_text().splitlines():
        line=line.strip()
        if not line or line.startswith('#') or line in ('host/quality.py','host/regions.py','host/world.py'):continue  # catalog, orchestration and generator do not change cooked bytes
        code.update(f'{line}:{sha(ROOT/line)}\n'.encode())
    code.update(json.dumps([SCENERY_MAX_AXIS,STATIC_PAGE_BUDGET,TEXTURE_BUDGET,ROOM_BYTE_BUDGET]).encode())
    inputs=hashlib.sha256();provenance=ROOT/'.hkpsx/regions-provenance.json'
    if not provenance.exists():return None
    for name in sorted(json.loads(provenance.read_text())['inputs']):
        path=source.directory/name
        if not path.is_file():return None
        inputs.update(f'{name}:{sha(path)}\n'.encode())
    return {'code':code.hexdigest(),'inputs':inputs.hexdigest()}


def cook_cache_key(region, fingerprints):
    """Stable key for one cooked region; None disables reuse."""
    if not fingerprints:return None
    return hashlib.sha256(json.dumps({'region':region,'fingerprints':fingerprints},sort_keys=True).encode()).hexdigest()


def cached_cook(destination, key):
    """Return the earlier cook report when its key and cooked files are intact."""
    cache=destination/'cook-cache.json'
    if key is None or not cache.is_file():return None
    try:old=json.loads(cache.read_text())
    except ValueError:return None
    room=destination/'room.hk'
    if old.get('key')!=key or not room.is_file() or not (destination/'scene.json').is_file() or not (destination/'unsupported.json').is_file():return None
    with room.open('rb') as stream:
        if hashlib.file_digest(stream,'sha256').hexdigest()!=old['cooked'].get('pack_sha256'):return None
    return old['cooked']


def cook_region(source,sc,region,chunk_id,destination,key,pixels,geometry):
    """Cook one region into its destination and record its cache key."""
    try:
        cooked=cook(source,sc,region,destination,pixels,geometry,write_shared=chunk_id==1,stage_for_similarity=True)
    except ValueError as error:
        if chunk_id==1 or not any(text in str(error) for text in ('budget exceeded','pool exceeds','cache dimensions','bank exceeds')):
            raise
        raise ValueError(f'Fixed layout region {chunk_id} exceeds measured residency budget: {error}') from error
    if key is not None:
        try:(destination/'cook-cache.json').write_text(json.dumps({'key':key,'cooked':cooked}))
        except TypeError:pass  # a report that is not JSON simply is not reused
    return cooked


def precook_scene(scene_id,out):
    """Worker: cook every cache miss of one scene; the parent then finds them cached."""
    source=Source();fingerprints=cook_fingerprints(source);pixels={};geometry={};sc=None
    for index,region in enumerate(fixed_regions()):
        if region['scene_id']!=scene_id:continue
        chunk_id=index+1;destination=out/f'region-{chunk_id:03}'
        key=cook_cache_key(region,fingerprints)
        if cached_cook(destination,key) is not None:continue
        if sc is None:sc=Scene(source,region['scene_file'])
        print(f'REGION {chunk_id}: {region["scene_name"]} {region["activation_bounds"]} (worker)',flush=True)
        cook_region(source,sc,region,chunk_id,destination,key,pixels,geometry)


def precook_parallel(pending,fingerprints,out):
    """Cook cache misses with one worker process per scene, most regions first.

    Each worker loads its own source and scene once; the cook of a region is a
    pure function of its spec, the source and the cooker code, so the parent
    loop that follows reuses every worker result through the per-region cache.
    """
    import subprocess,os
    misses={}
    for index,region in enumerate(pending):
        destination=out/f'region-{index+1:03}'
        if cached_cook(destination,cook_cache_key(region,fingerprints)) is None:
            misses[region['scene_id']]=misses.get(region['scene_id'],0)+1
    if not misses:return
    # Region 1 writes the shared knight/gameplay files: cook its scene first
    # so the other workers never race it, then the rest in parallel.
    order=sorted(misses,key=lambda scene:(-(scene==pending[0]['scene_id']),-misses[scene]))
    workers=max(1,min(len(order),(os.cpu_count() or 4)-2,8))
    print(f'Precooking {sum(misses.values())} regions across {len(order)} scenes with {workers} workers',flush=True)
    logs=ROOT/'.hkpsx'/'precook-logs';logs.mkdir(parents=True,exist_ok=True)
    def spawn(scene):
        handle=(logs/f'scene-{scene}.log').open('w')
        proc=subprocess.Popen([sys.executable,str(Path(__file__).resolve()),'--precook',str(scene)],cwd=ROOT,stdout=handle,stderr=subprocess.STDOUT)
        proc.log_path=logs/f'scene-{scene}.log';proc.log_handle=handle;return proc
    def finish(proc):
        proc.log_handle.close();return proc.log_path.read_text()
    first=order[0];running={}
    if first==pending[0]['scene_id']:
        proc=spawn(first);proc.wait();log=finish(proc)
        if proc.returncode:raise ValueError(f'Precook of scene {first} failed with status {proc.returncode}:\n{log[-4000:]}')
        order=order[1:]
    queue=list(order);attempts={}
    while queue or running:
        while queue and len(running)<workers:
            scene=queue.pop(0);running[scene]=spawn(scene);attempts[scene]=attempts.get(scene,0)+1
        for scene,proc in list(running.items()):
            if proc.poll() is not None:
                log=finish(proc);del running[scene]
                if proc.returncode<0 and attempts[scene]<3:
                    # A worker killed by a signal under parallel load (observed
                    # SIGSEGV at startup) is retried; finished regions are cached.
                    print(f'Precook worker for scene {scene} died with signal {-proc.returncode}; retrying',flush=True)
                    queue.append(scene);continue
                if proc.returncode:
                    for other in running.values():other.kill()
                    raise ValueError(f'Precook of scene {scene} failed with status {proc.returncode}:\n{log[-4000:]}')
        import time;time.sleep(0.2)


def write_report(path, report):
    temp=path.with_suffix('.tmp')
    dump(temp,report);temp.replace(path)


def write_provenance(report, source):
    files={source.directory/name for name in source.files}
    for key in source.env.files:
        path=Path(key)
        if path.is_file() and path.is_relative_to(source.directory):files.add(path)
    for scene in report['scenes']:files.add(source.directory/scene['file'])
    # Metadata-only refreshes may load fewer files than the complete cook.
    # Preserve earlier observed provenance for the regions they leave intact.
    inputs={}
    for path in (ROOT/'.hkpsx/provenance.json',ROOT/'.hkpsx/regions-provenance.json'):
        if path.is_file():inputs.update(json.loads(path.read_text()).get('inputs',{}))
    files.update(source.directory/name for name in inputs)
    files.update((source.directory/'Managed').glob('*.dll'))
    for path in list(files):
        for suffix in ('.resS','.resource'):
            if Path(str(path)+suffix).is_file():files.add(Path(str(path)+suffix))
    for path in sorted(files):
        if path.is_file():
            with path.open('rb') as stream:sha=hashlib.file_digest(stream,'sha256').hexdigest()
            inputs[str(path.relative_to(source.directory))]={'bytes':path.stat().st_size,'sha256':sha}
    provenance={'source':str(source.directory),'inputs':inputs,
                'regions':[{k:r[k] for k in ('chunk_id','scene_name','path','bytes','sha256')} for r in report['regions']],
                'complete':report['complete']}
    dump(ROOT/'.hkpsx/regions-provenance.json',provenance)
    report['provenance_report']='.hkpsx/regions-provenance.json'


def remap_actor_clips(actor, clip_base):
    """Copy every live/corpse binding into the appended scene-bank clip space."""
    actor=copy.deepcopy(actor)
    # Every controller-specific binding (Runner, Climber stun, Vengefly) lives
    # in the same appended clip space as walk/turn.
    keys=['walk_clip','turn_clip']+sorted(k for k in actor if k.endswith('_clip') and k not in ('walk_clip','turn_clip'))
    if actor['movement_control']['kind']=='ZombieSwipeWalker':
        for key in ('idle_clip','anticipate_clip','lunge_clip','cooldown_clip'):
            if key not in keys:raise ValueError('actor bank is missing cooked binding: '+key)
    for key in keys:
        if key not in actor:raise ValueError('actor bank is missing cooked binding: '+key)
        value=actor[key]+clip_base
        if not 0<=value<=65535:raise ValueError('actor bank clip binding exceeds u16')
        actor[key]=value
    if actor.get('corpse'):
        for key in ('air_clip','land_clip'):
            value=actor['corpse'][key]+clip_base
            if not 0<=value<=65535:raise ValueError('corpse clip binding exceeds u16')
            actor['corpse'][key]=value
    return actor


def postpack_actor_bank(report, source, scenes=None):
    """Append a shared scene actor bank from immutable pre-append base packs.

    Base region files stay unchanged, making repeated postpasses byte-identical.
    Existing palette/page contents and texture IDs do not move. Appended texture
    descriptors explicitly offset their own palettes, stream data and frame IDs.
    """
    scenes=scenes if scenes is not None else {}
    for info in SCENES:
        if info['scene_id'] not in scenes:scenes[info['scene_id']]=Scene(source,info['file'])
        sc=scenes[info['scene_id']]
        actors=[a for a in actor_sources(sc) if a['movement_supported']]
        if not actors:continue
        atlas=Atlas();frames=[];clips=[]
        append_actor_art(source,sc,actors,atlas,frames,clips)
        from great_door import append_art,bind
        great_door=append_art(source,sc,atlas,frames)
        atlas.pack()
        if atlas.pages:raise ValueError('Scene actor bank unexpectedly requires resident pages')
        # Actor clips start at one scene-wide clip index so every region of the
        # scene shares one ActorSpec per actor; shorter clip tables are padded
        # with an unreferenced one-frame clip.
        scene_rows=[row for row in report['regions'] if row['scene_id']==info['scene_id']]
        clip_base=max(struct.unpack_from('<6I',(ROOT/'data/regions'/f'region-{row["chunk_id"]:03}'/'room.hk').read_bytes(),8)[4] for row in scene_rows)
        for row in scene_rows:
            base=ROOT/'data/regions'/f'region-{row["chunk_id"]:03}'/'room.hk'
            raw=base.read_bytes()
            if raw[:8]!=b'HKROOM02':raise ValueError('Actor append requires HKROOM02 base pack')
            np,nt,nd,nf,nc,ne=struct.unpack_from('<6I',raw,8)
            ns,flags=struct.unpack_from('<2I',raw,32)
            if flags&~1:raise ValueError('Unknown room features')
            offset=40;blocks=[]
            for length in (nt*16,nd*44,nf*20,nc*16,ne*16,nt*32,np*32768,ns):
                blocks.append(raw[offset:offset+length]);offset+=length
            if offset!=len(raw):raise ValueError('Actor append base pack length mismatch')
            padding=clip_base-nc
            if nf+len(frames)>2048 or clip_base+len(clips)>128 or (padding and nf==0):
                raise ValueError('Scene actor bank exceeds region format limits')
            pack=bytearray(b'HKROOM02'+struct.pack('<8I',np,nt+len(atlas.entries),nd,nf+len(frames),clip_base+len(clips),ne,ns+len(atlas.stream),flags))
            pack+=blocks[0]
            for page,u,v,w,h,palette,stream_offset in atlas.entries:
                pack+=struct.pack('<6HI',page,u,v,w,h,palette+nt,stream_offset+ns)
            pack+=blocks[1]+blocks[2]
            for frame in frames:
                pack+=struct.pack('<I4i',frame['texture']+nt,*[round(v*65536) for v in frame['box']])
            pack+=blocks[3]
            pack+=struct.pack('<4I',0,1,65536,0)*padding
            for clip in clips:
                pack+=struct.pack('<4I',clip['start']+nf,clip['count'],round(clip['fps']*65536),clip['wrap']|(clip['loopStart']<<16))
            pack+=blocks[4]+blocks[5]+b''.join(atlas.palettes)+blocks[6]+blocks[7]+atlas.stream
            from texture_dedup import deduplicate_room
            actor_bank_bytes=len(pack)-len(raw)
            pack,deduplication=deduplicate_room(bytes(pack))
            if len(pack)>ROOM_BYTE_BUDGET:raise ValueError('Scene actor bank exceeds host staging budget')
            (ROOT/row['path']).write_bytes(pack)
            row.update(base_path=str(base.relative_to(ROOT)),base_sha256=hashlib.sha256(raw).hexdigest(),
                       bytes=len(pack),sha256=hashlib.sha256(pack).hexdigest(),textures=deduplication['textures_after'],
                       pages=deduplication['pages_after'],stream_bytes=deduplication['stream_bytes_after'],
                       actor_bank_bytes=actor_bank_bytes,texture_deduplication=deduplication,
                       alpha_cover_bytes=deduplication['alpha_cover_bytes'],animation_bytes=deduplication['animation_bytes'])
            preserved=[a for a in row['actors'] if not a['movement_supported']]
            for actor in actors:
                preserved.append(remap_actor_clips(actor,clip_base))
            row['actors']=preserved
            row['great_door']=bind(great_door,row,nf)
            if row['chunk_id']==1:
                (ROOT/'data/room.hk').write_bytes(pack)
                provenance_path=ROOT/'.hkpsx/provenance.json'
                if provenance_path.exists():
                    provenance=json.loads(provenance_path.read_text())
                    stream_textures=sum(struct.unpack_from('<H',pack,40+i*16)[0]==65535 for i in range(row['textures']))
                    provenance.update(pack_sha256=row['sha256'],pack_bytes=len(pack),textures=row['textures'],
                                      palette_bytes=row['textures']*32,stream_bytes=row['stream_bytes'],
                                      alpha_cover_bytes=row['alpha_cover_bytes'],animation_bytes=row['animation_bytes'],
                                      stream_textures=stream_textures,pages=row['pages'])
                    dump(provenance_path,provenance)
        print(f'Scene actor bank {info["scene_name"]}: {len(actors)} actors, {len(atlas.entries)} images, {len(atlas.stream)} texel bytes',flush=True)
    report['total_pack_bytes']=sum(row['bytes'] for row in report['regions'])
    report['actor_bank_policy']='All supported scene actors available in every region; inactive simulation must be bounded by resident collision coverage'
    return scenes


def refresh_actors():
    """Refresh only regions containing actors, plus shared hero parameters."""
    report=json.loads((ROOT/'data/regions.json').read_text())
    if not report['complete']:raise ValueError('Complete the region cook before actor refresh')
    source=Source();scenes={};pixels={};geometry={}
    for row in report['regions']:
        if row['chunk_id']!=1 and not row['actors']:continue
        scene_id=row['scene_id']
        if scene_id not in scenes:scenes[scene_id]=Scene(source,row['scene_file'])
        destination=ROOT/'data/regions'/f'region-{row["chunk_id"]:03}'
        print('REFRESH ACTORS',row['chunk_id'],flush=True)
        cooked=cook(source,scenes[scene_id],row,destination,pixels,geometry,write_shared=row['chunk_id']==1)
        shutil.copy2(destination/'room.hk',ROOT/row['path'])
        if row['chunk_id']==1:
            shutil.copy2(ROOT/row['path'],ROOT/'data/room.hk')
            shutil.copy2(destination/'scene.json',ROOT/'data/scene.json')
        for key in ('pages','textures','draws','edges','stream_bytes','alpha_cover_bytes','animation_bytes','format_features','breakables','actors','hazards','edge_sources','texture_request_to_canonical'):
            row[key]=cooked[key]
        row.update(bytes=cooked['pack_bytes'],sha256=cooked['pack_sha256'])
    for info in SCENES:
        if info['scene_id'] not in scenes:scenes[info['scene_id']]=Scene(source,info['file'])
        for external in scenes[info['scene_id']].file.externals:
            name=Path(external.path).name
            if (source.directory/name).is_file():source.file(name)
    report['scenes']=[scene_metadata(scenes[info['scene_id']],info) for info in SCENES]
    report['total_pack_bytes']=sum(row['bytes'] for row in report['regions'])
    postpack_checkpoints(report,source,scenes);postpack_pogo(report,source,scenes);postpack_camera_locks(report)
    postpack_masks(report)
    postpack_reveal_masks(report,source,scenes)
    for row in report['regions']:
        if row['pages']>STATIC_PAGE_BUDGET or row['textures']>TEXTURE_BUDGET or row['bytes']>ROOM_BYTE_BUDGET:
            raise ValueError('final region exceeds scenery/RAM admission budget')
    write_provenance(report,source)
    write_report(ROOT/'data/regions.json',report)
    print('Actor clips and provenance refreshed',flush=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--limit',type=int,help='Diagnostic: stop after this many cooked regions')
    parser.add_argument('--precook',type=int,metavar='SCENE_ID',help='Worker: cook this scene\'s cache misses into data/regions and exit')
    parser.add_argument('--serial',action='store_true',help='Cook cache misses in this process instead of one worker per scene')
    parser.add_argument('--refresh-actors',action='store_true',help='Re-cook only actor regions after controller extraction changes')
    parser.add_argument('--append-actor-bank',action='store_true',help='Rebuild the scene-wide actor bank from immutable base packs')
    args=parser.parse_args()
    if args.append_actor_bank:
        report=json.loads((ROOT/'data/regions.json').read_text());source=Source()
        scenes=postpack_actor_bank(report,source);postpack_checkpoints(report,source,scenes);postpack_pogo(report,source,scenes);postpack_camera_locks(report);postpack_masks(report);postpack_reveal_masks(report,source,scenes);write_provenance(report,source)
        write_report(ROOT/'data/regions.json',report);return
    if args.refresh_actors:
        refresh_actors();return
    out=ROOT/'data/regions';out.mkdir(parents=True,exist_ok=True)
    if args.precook is not None:
        precook_scene(args.precook,out);return
    source=Source();scenes={};pixels={};geometry={}
    fingerprints=cook_fingerprints(source)
    pending=fixed_regions();regions=[]
    if not args.serial:precook_parallel(pending,fingerprints,out)
    report={'format':'HKREGIONS01','complete':False,'initial_chunk_id':1,'scenes':[],
            'regions':regions,'quality':{'scenery_max_axis':SCENERY_MAX_AXIS,'static_page_budget':STATIC_PAGE_BUDGET,'texture_budget':TEXTURE_BUDGET,'room_byte_budget':ROOM_BYTE_BUDGET,'layout':'fixed106','animation_sampling':'unchanged'},'limitations':[
                'Tutorial and Town regions cover supported scenery and collision across their development envelopes; Town NPCs, shops, benches and progression scripts remain unsupported',
                'Region boundaries and camera global clamps are residency/development choices, not original room boundaries',
                'Source camera locks and gate triggers are recorded separately; runtime must not infer absent links',
                'Unsupported scripts and effects remain recorded per region; asset cooking alone does not implement their behavior',
            ]}
    while pending and (args.limit is None or len(regions)<args.limit):
        region=pending.pop(0);scene_id=region['scene_id']
        if scene_id not in scenes:
            scenes[scene_id]=Scene(source,region['scene_file'])
            report['scenes'].append(scene_metadata(scenes[scene_id],SCENES[scene_id]))
        chunk_id=len(regions)+1
        if chunk_id>1024:raise ValueError('Region chunk table exceeds 1024 slots')  # sanity bound; static RAM per view is the real limit
        destination=out/f'region-{chunk_id:03}'
        key=cook_cache_key(region,fingerprints);cooked=cached_cook(destination,key);cached=cooked is not None
        if cached:
            print(f'REGION {chunk_id}: {region["scene_name"]} {region["activation_bounds"]} (cached)',flush=True)
        else:
            print(f'REGION {chunk_id}: {region["scene_name"]} {region["activation_bounds"]}',flush=True)
            cooked=cook_region(source,scenes[scene_id],region,chunk_id,destination,key,pixels,geometry)
        path=out/f'chunk_{chunk_id}.hk';shutil.copy2(destination/'room.hk',path)
        if chunk_id==1:
            shutil.copy2(path,ROOT/'data/room.hk')
            shutil.copy2(destination/'scene.json',ROOT/'data/scene.json')
        row=dict(region,chunk_id=chunk_id,path=str(path.relative_to(ROOT)),bytes=cooked['pack_bytes'],
                 sha256=cooked['pack_sha256'],pages=cooked['pages'],textures=cooked['textures'],
                 draws=cooked['draws'],edges=cooked['edges'],stream_bytes=cooked['stream_bytes'],
                 alpha_cover_bytes=cooked['alpha_cover_bytes'],animation_bytes=cooked['animation_bytes'],format_features=cooked['format_features'],
                 grass=cooked['grass'],grass_impact=cooked.get('grass_impact'),door_debris=cooked.get('door_debris',[]),particle_effects=cooked.get('particle_effects'),breakables=cooked['breakables'],actors=cooked['actors'],
                 hazards=cooked['hazards'],benches=cooked.get('benches',[]),bench_clip_base=cooked.get('bench_clip_base'),shrooms=cooked.get('shrooms',[]),edge_sources=cooked['edge_sources'],
                 texture_request_to_canonical=cooked['texture_request_to_canonical'])
        grass_indices={sid:index for index,(sid,(typ,_)) in enumerate(
            (item for item in scenes[scene_id].objects.items() if item[1][0]=='GrassCut'))}
        for grass in row['grass']:
            grass['state_index']=grass_indices[int(grass['source'].split(':')[-1])]
            grass['bounds']=grass['box']
        regions.append(row)
        report.update(pending_regions=len(pending),total_pack_bytes=sum(r['bytes'] for r in regions))
        # Checkpoint the multi-megabyte report after fresh cooks only, and at
        # most every tenth region: serializing it per cached region cost more
        # than the cache saved.
        if not cached and len(regions)%10==0:write_report(ROOT/'data/regions.json',report)
    report['complete']=not pending
    for region in regions:
        x0,y0,x1,y1=region['activation_bounds']
        region['neighbour_chunks']=[other['chunk_id'] for other in regions
            if other['chunk_id']!=region['chunk_id'] and other['scene_id']==region['scene_id']
            and intersects([x0-.01,y0-.01,x1+.01,y1+.01],other['activation_bounds'])]
    postpack_actor_bank(report,source,scenes)
    postpack_checkpoints(report,source,scenes);postpack_pogo(report,source,scenes);postpack_camera_locks(report)
    postpack_masks(report)
    postpack_reveal_masks(report,source,scenes)
    if report['complete']:
        from similarity_dedup import postpack_similarity
        postpack_similarity(report)
        # Draw order is stable; recheck authored bindings against final atlases.
        postpack_masks(report)
        postpack_reveal_masks(report,source,scenes)
        from world import generate
        generate(report)
    for row in report['regions']:
        if row['pages']>STATIC_PAGE_BUDGET or row['textures']>TEXTURE_BUDGET or row['bytes']>ROOM_BYTE_BUDGET:
            raise ValueError('final region exceeds scenery/RAM admission budget')
    write_provenance(report,source)
    write_report(ROOT/'data/regions.json',report)
    print(f'Cooked {len(regions)} regions ({report.get("total_pack_bytes",0)} bytes), complete={report["complete"]}',flush=True)


if __name__=='__main__':
    main()
