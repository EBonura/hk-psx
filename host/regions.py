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
from world import postpack_checkpoints, postpack_masks, MAX_REGIONS
from reveal_masks import postpack_reveal_masks
from quality import SCENE_TABLE, GRID_SCENE_LAYOUTS, REGION_LAYOUT, TOWN_EXTENSION_LAYOUT, SCENERY_MAX_AXIS, STATIC_PAGE_BUDGET, ROOM_BYTE_BUDGET, TEXTURE_BUDGET
from cook import MAX_ROOM_TEXTURES

SCENES = [dict(scene) for scene in SCENE_TABLE]


def over_budget(pages, textures, cluts, packed_bytes):
    """Whether one view is outside the runtime admission budget.

    Records and palettes are different limits. A texture record is a slot in
    HKROOM02's own table, capped by the format at MAX_ROOM_TEXTURES; a CLUT slot
    is a distinct palette value, and any number of records may name one. They
    were the same number until frames started tiling, and gating both on
    TEXTURE_BUDGET rejected content whose palettes fit comfortably: the False
    Knight's phase-one clips are 570 records and 303 slots.

    This is the whole rule, and it lives here so a tool that reports on
    admission does not restate it. Restating it is how a texture count came to
    be quoted as a CLUT figure: `tools/cook_scene_pack.py` gated records on
    TEXTURE_BUDGET and never counted palettes at all.
    """
    return (pages > STATIC_PAGE_BUDGET or textures > MAX_ROOM_TEXTURES
            or cluts > TEXTURE_BUDGET or packed_bytes - pages*32768 > ROOM_BYTE_BUDGET)


def row_over_budget(row):
    """`over_budget` for a cooked report row, which may predate the `cluts` field."""
    return over_budget(row['pages'], row['textures'],
                       row.get('cluts', row['textures']), row['bytes'])


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


# CameraController's scene bounds for the camera centre: 14.6 and 8.3 in from
# the scene's left and bottom (KeepWithinSceneBounds, LateUpdate), and
# GetTilemapInfo's xLimit = width - 14.6, yLimit = height - 8.3 at the far side.
CAMERA_X_MIN, CAMERA_Y_MIN = 14.6, 8.3


def camera_tilemap(sc):
    """GameManager.RefreshTilemapInfo's tilemap: the first root GameObject tagged
    TileMap that carries a tk2dTileMap, else (its first fallback, which logs an
    error in the source) the first tagged one anywhere. Width and height in tiles
    are world units; its position is not read."""
    source=sc.source
    manager=next(o for o in source.file('globalgamemanagers').objects.values() if o.type.name=='TagManager')
    tag=20000+source.read(manager)['tags'].index('TileMap')
    found=[]
    for sid,(kind,tree) in sorted(sc.objects.items()):
        if kind!='tk2dTileMap':continue
        gid=tree['m_GameObject']['m_PathID']
        if gid not in sc.gos or sc.gos[gid]['m_Tag']!=tag:continue
        root=not sc.transforms[sc.go_transform[gid]]['m_Father']['m_PathID']
        found.append((not root,sid,tree))
    if not found:raise ValueError('scene has no TileMap-tagged tilemap')
    _,sid,tree=min(found,key=lambda f:f[0])
    return {'source':sc.sid(sid),'width':tree['width'],'height':tree['height']}


def camera_lock_rows(sc, file):
    """Every enabled, active CameraLockArea: its trigger (BoxCollider2D boxes or
    a PolygonCollider2D, world space), serialized limits and flags, and the one
    lifetime FSM shape the slice has (Crossroads_01's `Disable`: Wait, then
    deactivate itself, which releases the lock through OnDisable)."""
    from focus import action_fields
    rows=[]
    battle=battle_locks(sc)
    for sid,(kind,tree) in sorted(sc.objects.items()):
        if kind!='CameraLockArea':continue
        gid=tree['m_GameObject']['m_PathID']
        if not tree['m_Enabled'] or not (sc.active(gid) or gid in battle):continue
        boxes=[];polygons=[]
        for _,(k,col) in sc.objects.items():
            if col.get('m_GameObject',{}).get('m_PathID')!=gid or not col.get('m_Enabled',1):continue
            off=col.get('m_Offset',{'x':0,'y':0})
            if k=='BoxCollider2D':
                w,h=col['m_Size']['x']/2,col['m_Size']['y']/2
                pts=[sc.point(gid,x+off['x'],y+off['y'])[:2] for x,y in ((-w,-h),(w,-h),(w,h),(-w,h))]
                boxes.append([min(p[0] for p in pts),min(p[1] for p in pts),max(p[0] for p in pts),max(p[1] for p in pts)])
            elif k=='PolygonCollider2D':
                for path in col['m_Points']['m_Paths']:
                    polygons.append([list(sc.point(gid,q['x']+off['x'],q['y']+off['y'])[:2]) for q in path])
        if not boxes and not polygons:continue
        if boxes and polygons:raise ValueError('camera lock mixes box and polygon triggers: '+sc.sid(sid))
        if len(polygons)>1 or (polygons and not 3<=len(polygons[0])<=16):raise ValueError('camera lock polygon outside one path of 3..16 points: '+sc.sid(sid))
        # Several boxes on one lock (Crossroads_ShamanTemple's 5, 5 (1), 5 (2)):
        # the Knight is in the lock while he touches any of them, so each goes
        # in as its own trigger polygon, after dropping exact duplicates.
        unique=[]
        for box in boxes:
            if box not in unique:unique.append(box)
        if len(unique)>1:polygons=[[[x0,y0],[x1,y0],[x1,y1],[x0,y1]] for x0,y0,x1,y1 in unique]
        boxes=[[min(b[0] for b in unique),min(b[1] for b in unique),max(b[2] for b in unique),max(b[3] for b in unique)]] if unique else []
        points=[p for poly in polygons for p in poly]
        bounds=boxes[0] if boxes else [min(p[0] for p in points),min(p[1] for p in points),max(p[0] for p in points),max(p[1] for p in points)]
        expires=None
        for _,(k,fsm) in sc.objects.items():
            if k!='PlayMakerFSM' or fsm['m_GameObject']['m_PathID']!=gid:continue
            states={st['name']:st for st in fsm['fsm']['states']}
            names=lambda st:[n.rsplit('.',1)[-1] for n in st['actionData']['actionNames']]
            start=states.get(fsm['fsm'].get('startState'))
            if fsm['fsm']['name']=='Disable' and start and names(start)==['Wait'] and len(start['transitions'])==1:
                after=states[start['transitions'][0]['toState']]
                fields=[action_fields(after['actionData'],i) for i in range(len(after['actionData']['actionNames']))]
                off=[f for n,f in zip(names(after),fields) if n=='ActivateGameObject' and not f['activate']['value'] and f['gameObject']['ownerOption']==0]
                if off:
                    wait=action_fields(start['actionData'],0)['time']
                    expires=round(float(wait['value'])*60)
                    continue
            if fsm['fsm']['name']=='FSM' and start and names(start)==['NextFrameEvent']:
                # Fungus1_08's Hunter Entry lock: `Check` destroys it on load
                # once PlayerData hasHuntersMark holds. The Hunter's Mark comes
                # from completing the Hunter's Journal, which nothing in the
                # port grants, so the lock always stands, as it does in the
                # original until then. Any other bool is refused.
                check=states.get('Check')
                variables={v['name']:v['value'] for v in fsm['fsm']['variables'].get('stringVariables',[])}
                if not check or names(check)!=['PlayerDataBoolTest'] or variables.get('playerData bool')!='hasHuntersMark':
                    raise ValueError('unmodelled PlayerData test on camera lock '+sc.sid(sid))
                continue
            raise ValueError(f'unmodelled FSM {fsm["fsm"]["name"]} on camera lock {sc.sid(sid)}')
        rows.append({'source':f'{file}:{sid}','name':sc.gos[gid]['m_Name'],'enabled':True,'trigger_bounds':bounds,
                     'trigger_polygons':polygons,'expires_ticks':expires,'battle':gid in battle,
                     'serialized_limits':{k:tree[k] for k in ('cameraXMin','cameraYMin','cameraXMax','cameraYMax','preventLookUp','preventLookDown','maxPriority')}})
    return rows


def battle_locks(sc):
    """Lock GameObjects a `Battle Control` switches on for its fight: `Init`
    finds the child `CameraLockArea B` and the first wave state activates it
    with `BG CLOSE`, `End` deactivates it with `BG OPEN` (Crossroads_22).
    The port runs the lock while that scene's arena is closed. Only locks that
    load switched off count: one that loads on (the False Knight's) stands."""
    from focus import action_fields
    found=set()
    for _,(kind,comp) in sc.objects.items():
        if kind!='PlayMakerFSM' or comp['fsm']['name']!='Battle Control':continue
        owner=comp['m_GameObject']['m_PathID'];states={st['name']:st for st in comp['fsm']['states']}
        init=states.get('Init')
        if not init:continue
        names=[n.rsplit('.',1)[-1] for n in init['actionData']['actionNames']]
        children=[action_fields(init['actionData'],i)['childName'] for i,n in enumerate(names) if n=='FindChild']
        if not any(isinstance(c,dict) and c.get('value')=='CameraLockArea B' for c in children):continue
        def sends(state,event):
            data=states[state]['actionData']
            value=lambda f:f.get('value') if isinstance(f,dict) else f
            return any(n.endswith('.SendEventByName') and value(action_fields(data,i)['sendEvent'])==event for i,n in enumerate(data['actionNames']))
        if not ('Wave 1' in states and 'End' in states and sends('Wave 1','BG CLOSE') and sends('End','BG OPEN')):
            continue
        stack=[owner]
        while stack:
            g=stack.pop()
            for tid,t in sc.transforms.items():
                if t['m_Father']['m_PathID']==sc.go_transform.get(g):
                    child=t['m_GameObject']['m_PathID'];stack.append(child)
                    if sc.gos.get(child,{}).get('m_Name')=='CameraLockArea B' and not sc.active(child):found.add(child)
    return found


def postpack_camera(report, source, scenes):
    """Refresh each scene's camera inputs from the source: the TileMap the
    original's GetTilemapInfo reads and every live CameraLockArea."""
    for info,row in zip(SCENES,report['scenes']):
        if row['scene_name']!=info['scene_name']:raise ValueError('report scene order differs from SCENES')
        if info['scene_id'] not in scenes:scenes[info['scene_id']]=Scene(source,info['file'])
        sc=scenes[info['scene_id']]
        row['camera_tilemap']=camera_tilemap(sc)
        row['camera_locks']=camera_lock_rows(sc,info['file'])


def postpack_camera_locks(report, source=None, scenes=None):
    """Every live CameraLockArea of each scene, once, with its limits as
    CameraLockArea.ValidateBounds leaves them: exactly -1 becomes 14.6, xLimit,
    8.3 or yLimit, anything else stays as authored (Crossroads_01's 0 and 9999
    included; the scene bounds clamp after the lock in LateUpdate)."""
    if source is not None:postpack_camera(report,source,scenes if scenes is not None else {})
    by_scene={scene['scene_name']:scene for scene in report['scenes']}
    # A lock under a secret goes with its break (Fungus1_08's cracked floor
    # carries one under `wood small`): the lock names the secret's state.
    owners={}
    for region in report['regions']:
        for secret in region.get('secrets',[]):
            for lock in secret.get('camera_lock_sources',[]):
                owners[lock]=region['scene_id']*128+secret['state_index']
    # Every lock of a scene rides once, in its first view (as its gates do),
    # and the guest tests all of them: a few dozen at most per scene, where one
    # copy per view it touches cost Tutorial_01's bank, the largest, 4 KB.
    for region in report['regions']:
        region['camera_locks']=[]
    for scene in report['scenes']:
        tilemap=scene['camera_tilemap']
        x_limit,y_limit=tilemap['width']-CAMERA_X_MIN,tilemap['height']-CAMERA_Y_MIN
        locks=[]
        for lock in scene.get('camera_locks',[]):
            if not lock['enabled'] or 'trigger_bounds' not in lock:continue
            t=lock['trigger_bounds']
            lim=lock['serialized_limits']
            valid=lambda v,default:default if v==-1 else v
            xmin,xmax=valid(lim['cameraXMin'],CAMERA_X_MIN),valid(lim['cameraXMax'],x_limit)
            ymin,ymax=valid(lim['cameraYMin'],CAMERA_Y_MIN),valid(lim['cameraYMax'],y_limit)
            # Q16 world words hold +-512 units: Crossroads_01's authored 9999 is
            # past every scene (the widest is 263), so 500 limits nothing either.
            far=lambda v:max(-500,min(500,v))
            shape=[[(far(xmin),far(ymin)),(far(xmax),far(ymax))]]
            shape+=[[tuple(p) for p in polygon] for polygon in lock.get('trigger_polygons') or []]
            locks.append({'source':lock['source'],'name':lock['name'],'bounds':t,'owner_state_id':owners.get(lock['source']),
                          'limit_points':shape,'expires_ticks':lock.get('expires_ticks'),'battle':bool(lock.get('battle')),
                          'max_priority':bool(lim.get('maxPriority')),
                          'prevent_look_down':bool(lim.get('preventLookDown')),'prevent_look_up':bool(lim.get('preventLookUp'))})
        if len(locks)>32:raise ValueError('more camera locks than the guest lists: '+scene['scene_name'])
        scene['camera_lock_objects']=locks


def door_destination(sc, gid):
    """Where a door leads, from the FSM that actually performs its transition.

    A TransitionPoint carrying a `Door Control` FSM is never walked out of. `In
    Range` listens for UP and `Change Scene` calls `BeginSceneTransition`, so
    the component's own `targetScene` and `entryPoint` are not read on the way
    out, and eleven of the seventeen doors in the admitted scenes show it: nine
    leave the pair empty, Crossroads_01's `door1` has the two fields transposed
    and Town's `room_grimm` names a scene its FSM does not.

    Two things make the FSM readable rather than a guess. The action's own
    inline `sceneName`/`entryGateName` are stale editor defaults, `Room_temple`
    on every door in these scenes, and the shipped values are in the FSM
    variables the action points at; and no enabled action in any admitted scene
    writes `New Scene` or `Entry Gate`, so the serialized variable is the value
    the door departs with.

    Returns None whenever any of that cannot be read, which leaves the gate
    exactly as it is today. A door with no edge is visible the moment someone
    walks into it; a door with the wrong edge sends the player to the wrong room.
    """
    from focus import action_fields
    doors=[comp['fsm'] for _,(kind,comp) in sc.objects.items()
           if kind=='PlayMakerFSM' and comp['m_GameObject']['m_PathID']==gid
           and comp['fsm']['name']=='Door Control']
    if len(doors)!=1:
        return None
    fsm=doors[0]
    variables={v['name']:v['value'] for group in fsm['variables'].values()
               if isinstance(group,list) for v in group
               if isinstance(v,dict) and 'name' in v and 'value' in v}
    found=[]
    for state in fsm['states']:
        data=state['actionData']
        for i,name in enumerate(data['actionNames']):
            if data['actionEnabled'][i] and name.rsplit('.',1)[-1]=='BeginSceneTransition':
                found.append(action_fields(data,i))
    if len(found)!=1:
        return None
    def literal(field):
        if not isinstance(field,dict):
            return None
        value=variables.get(field['name']) if field.get('useVariable') else field.get('value')
        return value if isinstance(value,str) and value else None
    scene=literal(found[0]['sceneName']);gate=literal(found[0]['entryGateName'])
    return (scene,gate) if scene and gate else None


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
            door=door_destination(sc,gid)
            if door:
                row['door_control']={'target_scene':door[0],'entry_point':door[1]}
                serialized=(row['target_scene'],row['entry_point'])
                if not any(serialized):
                    row['target_scene'],row['entry_point']=door
                elif serialized!=door:
                    # Two records of the same destination that disagree. The FSM
                    # is what the source departs through, so the component is
                    # almost certainly the stale one, but almost certainly is
                    # what this refuses on: Crossroads_01's door1 is the only
                    # case and it produces no gate today either. Both are
                    # recorded so the choice can be made on the evidence later.
                    row['door_control']['disagrees_with_serialized']=True
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
    from quality import SCENERY_MAX_AXIS, STATIC_PAGE_BUDGET, TEXTURE_BUDGET, ROOM_BYTE_BUDGET, SCENERY_TEXEL_CAP, SCENERY_SCENE_CAPS
    def sha(path):
        with Path(path).open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
    code=hashlib.sha256()
    # host/cook_inputs.txt is the shared list. Its postpass section is the whole
    # difference between the two caches this feeds: the build hashes the file
    # entire, so a postpass change re-runs the cook, while a base region's key
    # stops at the divider so that same change reuses all 693 cooked packs.
    for line in (ROOT/'host/cook_inputs.txt').read_text().splitlines():
        if line.strip()=='# --- postpass only ---':break
        line=line.strip()
        if not line or line.startswith('#'):continue
        code.update(f'{line}:{sha(ROOT/line)}\n'.encode())
    code.update(json.dumps([SCENERY_MAX_AXIS,STATIC_PAGE_BUDGET,TEXTURE_BUDGET,ROOM_BYTE_BUDGET,SCENERY_TEXEL_CAP,sorted(SCENERY_SCENE_CAPS.items())]).encode())
    inputs={};provenance=ROOT/'.hkpsx/regions-provenance.json'
    if not provenance.exists():return None
    for name in sorted(json.loads(provenance.read_text())['inputs']):
        path=source.directory/name
        if not path.is_file():return None
        inputs[name]=sha(path)
    return {'code':code.hexdigest(),'inputs':inputs}


def inputs_digest(shas, names):
    digest=hashlib.sha256()
    for name in names:digest.update(f'{name}:{shas[name]}\n'.encode())
    return digest.hexdigest()


def cook_cache_key(region, fingerprints):
    """Reuse identity for one cooked region; None disables reuse.

    The source files are checked rather than keyed: a cook records the files
    the provenance listed when it ran, and stays fresh while each of those is
    unchanged. The list only grows (write_provenance keeps every file any run
    loaded), so keying on the whole list made one more asset file read by a
    postpass (the False Knight's bank) recook every region.
    """
    if not fingerprints:return None
    shas=fingerprints['inputs'];names=sorted(shas)
    key=lambda value:hashlib.sha256(json.dumps(value,sort_keys=True).encode()).hexdigest()
    return {'key':key({'region':region,'code':fingerprints['code']}),'names':names,'shas':shas,
            'inputs':inputs_digest(shas,names),
            # What a cook written before this keyed on, so those packs are kept.
            'legacy':key({'region':region,'fingerprints':{'code':fingerprints['code'],'inputs':inputs_digest(shas,names)}})}


def cached_cook(destination, key):
    """Return the earlier cook report when its key and cooked files are intact."""
    cache=destination/'cook-cache.json'
    if key is None or not cache.is_file():return None
    try:old=json.loads(cache.read_text())
    except ValueError:return None
    room=destination/'room.hk'
    names=old.get('input_names')
    if names is None:fresh=old.get('key')==key['legacy']
    else:fresh=old.get('key')==key['key'] and all(name in key['shas'] for name in names) and inputs_digest(key['shas'],names)==old.get('inputs')
    if not fresh or not room.is_file() or not (destination/'scene.json').is_file() or not (destination/'unsupported.json').is_file():return None
    with room.open('rb') as stream:
        if hashlib.file_digest(stream,'sha256').hexdigest()!=old['cooked'].get('pack_sha256'):return None
    if names is None:record_cook(destination,key,old['cooked'])
    return old['cooked']


def record_cook(destination, key, cooked):
    try:(destination/'cook-cache.json').write_text(json.dumps({'key':key['key'],'input_names':key['names'],'inputs':key['inputs'],'cooked':cooked}))
    except TypeError:pass  # a report that is not JSON simply is not reused


def cook_region(source,sc,region,chunk_id,destination,key,pixels,geometry):
    """Cook one region into its destination and record its cache key."""
    try:
        cooked=cook(source,sc,region,destination,pixels,geometry,write_shared=chunk_id==1,stage_for_similarity=True)
    except ValueError as error:
        if chunk_id==1 or not any(text in str(error) for text in ('budget exceeded','pool exceeds','cache dimensions','bank exceeds')):
            raise
        raise ValueError(f'Fixed layout region {chunk_id} exceeds measured residency budget: {error}') from error
    if key is not None:record_cook(destination,key,cooked)
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


def append_room_bank(raw, atlas, frames, clips, clip_base, what):
    """Append one packed art bank to an immutable HKROOM02 pack.

    The base stays unchanged, making a repeated postpass byte-identical.
    Existing palette/page contents and texture IDs do not move; the appended
    descriptors explicitly offset their own palettes, stream data and frame IDs.
    Clips are padded up to `clip_base` so every view of a scene can share one
    clip index space. Returns the new pack and the base's own frame count, which
    is the offset any binding into the appended frames needs.
    """
    if raw[:8]!=b'HKROOM02':raise ValueError(f'{what} requires an HKROOM02 base pack')
    np,nt,nd,nf,nc,ne=struct.unpack_from('<6I',raw,8)
    ns,flags=struct.unpack_from('<2I',raw,32)
    if flags&~1:raise ValueError('Unknown room features')
    offset=40;blocks=[]
    for length in (nt*16,nd*44,nf*20,nc*16,ne*16,nt*32,np*32768,ns):
        blocks.append(raw[offset:offset+length]);offset+=length
    if offset!=len(raw):raise ValueError(f'{what} base pack length mismatch')
    padding=clip_base-nc
    if padding<0 or nf+len(frames)>2048 or clip_base+len(clips)>128 or (padding and nf==0):
        raise ValueError(f'{what} exceeds region format limits')
    # A bank may bring static textures of its own (the False Knight's parts):
    # its pages follow the base's, and a static texture needs the base to carry
    # alpha covers, because the bank's cover records ride in its stream.
    if atlas.pages and not flags&1:raise ValueError(f'{what} brings static pages to a pack without alpha covers')
    if np+len(atlas.pages)>20:raise ValueError(f'{what} exceeds the region page limit')
    pack=bytearray(b'HKROOM02'+struct.pack('<8I',np+len(atlas.pages),nt+len(atlas.entries),nd,nf+len(frames),clip_base+len(clips),ne,ns+len(atlas.stream),flags))
    pack+=blocks[0]
    for page,u,v,w,h,palette,stream_offset in atlas.entries:
        pack+=struct.pack('<6HI',page if page==65535 else page+np,u,v,w,h,palette+nt,stream_offset+ns)
    pack+=blocks[1]+blocks[2]
    for frame in frames:
        pack+=struct.pack('<I4i',frame['texture']+nt,*[round(v*65536) for v in frame['box']])
    pack+=blocks[3]
    pack+=struct.pack('<4I',0,1,65536,0)*padding
    for clip in clips:
        pack+=struct.pack('<4I',clip['start']+nf,clip['count'],round(clip['fps']*65536),clip['wrap']|(clip['loopStart']<<16))
    pack+=blocks[4]+blocks[5]+b''.join(atlas.palettes)+blocks[6]+b''.join(atlas.pages)+blocks[7]+atlas.stream
    return pack,nf


def postpack_npc_bank(report, source, scenes):
    """Append each view's cooked NPC art to its own immutable base pack.

    An NPC stands in one view of one scene, so its clips ride in that view's
    bank rather than in the scene-wide actor bank every view of the scene
    carries. This runs after postpack_actor_bank and re-reads the same immutable
    base, so a view holding both would lose the actor bank; neither Town nor
    Crossroads_45 has a supported actor, and a view that needs both is refused
    rather than composed untested.
    """
    from npc_sources import npc_sources, append_npc_art
    from texture_dedup import deduplicate_room
    placed=[]
    for row in report['regions']:
        sc=scenes[row['scene_id']]
        npcs=npc_sources(row['scene_name'],sc,row['interaction_bounds'])
        row['npcs']=npcs
        if not npcs:continue
        # The guest reserves one animation-cache slot for the view's NPC, and
        # main.rs reads one record per view. No admitted NPC shares a view with
        # another today, so the second slot is unwritten rather than untested.
        if len(npcs)>1:raise ValueError(f'view {row["chunk_id"]} holds {len(npcs)} cooked NPCs')
        if any(actor['movement_supported'] for actor in row['actors']):
            raise ValueError(f'view {row["chunk_id"]} would need both an NPC and an actor bank')
        for npc in npcs:npc['scene_id']=row['scene_id']
        atlas=Atlas();frames=[];clips=[]
        append_npc_art(source,sc,npcs,atlas,frames,clips)
        atlas.pack()
        if atlas.pages:raise ValueError('NPC bank unexpectedly requires resident pages')
        base=ROOT/'data/regions'/f'region-{row["chunk_id"]:03}'/'room.hk'
        raw=base.read_bytes()
        # Appended straight after the view's own clips: nothing else shares this
        # index space, so no padding is needed and no existing clip index moves.
        clip_base=struct.unpack_from('<6I',raw,8)[4]
        pack,_nf=append_room_bank(raw,atlas,frames,clips,clip_base,'NPC bank')
        for npc in npcs:npc['clip_base']+=clip_base
        npc_bank_bytes=len(pack)-len(raw)
        pack,deduplication=deduplicate_room(bytes(pack))
        if len(pack)>ROOM_BYTE_BUDGET:raise ValueError('NPC bank exceeds host staging budget')
        (ROOT/row['path']).write_bytes(pack)
        row.update(base_path=str(base.relative_to(ROOT)),base_sha256=hashlib.sha256(raw).hexdigest(),
                   bytes=len(pack),sha256=hashlib.sha256(pack).hexdigest(),textures=deduplication['textures_after'],
                   pages=deduplication['pages_after'],stream_bytes=deduplication['stream_bytes_after'],
                   npc_bank_bytes=npc_bank_bytes,texture_deduplication=deduplication,
                   alpha_cover_bytes=deduplication['alpha_cover_bytes'],animation_bytes=deduplication['animation_bytes'])
        # One conversation per NPC however many views its trigger reaches into.
        placed.extend(n for n in npcs if not any(p['scene_id']==n['scene_id']
                      and p['game_object']==n['game_object'] for p in placed))
        print(f'NPC bank {row["scene_name"]} view {row["chunk_id"]}: '
              f'{", ".join(n["name"] for n in npcs)}, {len(atlas.entries)} images, '
              f'{len(atlas.stream)} texel bytes, {row["textures"]} textures',flush=True)
    if placed:
        from npc_dialogue import cook as cook_npc_lines
        cook_npc_lines(source,scenes,placed)
    report['total_pack_bytes']=sum(row['bytes'] for row in report['regions'])
    report['npc_bank_policy']='One NPC per view, cooked into that view alone, present under the same fresh-save gate answer as the rest of the world; its conversation chain links through data/npc_lines.rs'
    return placed


def postpack_actor_bank(report, source, scenes=None):
    """Append a shared scene actor bank from immutable pre-append base packs.

    Base region files stay unchanged, making repeated postpasses byte-identical.
    Existing palette/page contents and texture IDs do not move. Appended texture
    descriptors explicitly offset their own palettes, stream data and frame IDs.
    """
    scenes=scenes if scenes is not None else {}
    report['refused_pickups']=[]
    for info in SCENES:
        if info['scene_id'] not in scenes:scenes[info['scene_id']]=Scene(source,info['file'])
        sc=scenes[info['scene_id']]
        actors=[a for a in actor_sources(sc) if a['movement_supported']]
        import pickups
        # Arena gates draw from the same bank (host/battle_gates.py), so a scene
        # with gates and nothing else still takes one.
        from breakables import battle_gates as gate_sources
        from battle_gates import append_art as append_gate_art,bind_art as bind_gate_art
        # Water drips too (host/props.py drip_art).
        from props import drip_sources,drip_art,bind_drip_art
        if not actors and not pickups.present(sc) and not gate_sources(sc) and not drip_sources(sc):continue
        scene_rows=[row for row in report['regions'] if row['scene_id']==info['scene_id']]
        # The False Knight's art is its own bank (host/false_knight_art.py):
        # the generic path cooks only its barrel and a Blank for every body
        # slot, and the bank below carries every clip, partly in the scene's
        # own texture pages. It is the one actor whose bank brings pages.
        from false_knight_art import neutral_actor,cook_scene_bank
        boss=next((a for a in actors if a['movement_control']['kind']=='FalseKnight'),None)
        if boss is not None:actors=[neutral_actor(a) if a is boss else a for a in actors]
        # A view already at its texture table takes the scene with fewer
        # pickup extras, then without its chests and pickups, which are then
        # refused rather than the build (host/pickups.py LEVEL_*).
        import copy
        before=[(row,copy.deepcopy(row),(ROOT/row['path']).read_bytes()) for row in scene_rows]
        for level in (pickups.LEVEL_FULL,pickups.LEVEL_OPEN,pickups.LEVEL_LITE,pickups.LEVEL_BASIC,0):
          try:
              atlas=Atlas();frames=[];clips=[]
              append_actor_art(source,sc,actors,atlas,frames,clips)
              from great_door import append_art,bind
              great_door=append_art(source,sc,atlas,frames)
              write_boss_art=None
              if boss is not None:
                  boss_art,write_boss_art=cook_scene_bank(source,sc,boss,scene_rows,atlas,frames,clips)
              # Brooding Mawlek's bank the same way (host/mawlek_art.py): the
              # recognizer already binds Dummy Blank for the generic clips.
              mawlek=next((a for a in actors if a['movement_control']['kind']=='Mawlek'),None)
              if mawlek is not None:
                  if boss is not None:raise ValueError('one boss bank per scene')
                  from mawlek_art import cook_scene_bank as cook_mawlek_bank
                  boss_art,write_boss_art=cook_mawlek_bank(source,sc,mawlek,scene_rows,atlas,frames,clips)
              # Gruz Mother's bank the same way (host/false_knight_art.py, its
              # Gruz Mother section): the recognizer binds the one-frame Charge.
              gruz=next((a for a in actors if a['movement_control']['kind']=='GruzMother'),None)
              if gruz is not None:
                  if boss is not None or mawlek is not None:raise ValueError('one boss bank per scene')
                  from false_knight_art import gruz_cook_scene_bank
                  boss_art,write_boss_art=gruz_cook_scene_bank(source,sc,gruz,scene_rows,atlas,frames,clips)
              # Chests and pickups ride the same per-scene bank (host/pickups.py).
              pickup_base=len(frames);pickup_record=pickups.append_art(source,sc,atlas,frames,level) if level else None
              gate_record=append_gate_art(source,sc,atlas,frames)
              drip_record=drip_art(source,sc,atlas,frames)
              atlas.pack()
              if atlas.pages and boss is None and mawlek is None and gruz is None:raise ValueError('Scene actor bank unexpectedly requires resident pages')
              # Actor clips start at one scene-wide clip index so every region of the
              # scene shares one ActorSpec per actor; shorter clip tables are padded
              # with an unreferenced one-frame clip.
              clip_base=max(struct.unpack_from('<6I',(ROOT/'data/regions'/f'region-{row["chunk_id"]:03}'/'room.hk').read_bytes(),8)[4] for row in scene_rows)
              if write_boss_art is not None:write_boss_art(clip_base)
              for row in scene_rows:
                  base=ROOT/'data/regions'/f'region-{row["chunk_id"]:03}'/'room.hk'
                  raw=base.read_bytes()
                  pack,nf=append_room_bank(raw,atlas,frames,clips,clip_base,'Scene actor bank')
                  from texture_dedup import deduplicate_room
                  actor_bank_bytes=len(pack)-len(raw)
                  pack,deduplication=deduplicate_room(bytes(pack))
                  # Counted without pages, as over_budget counts a view: a view's
                  # pages live in the scene's VRAM atlas, not in any RAM arena.
                  if len(pack)-deduplication['pages_after']*32768>ROOM_BYTE_BUDGET:raise ValueError('Scene actor bank exceeds host staging budget')
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
                  row['pickups']=pickups.bind(pickup_record,row,nf+pickup_base)
                  row['battle_gate_art']=bind_gate_art(gate_record,row,nf)
                  row['drip_art']=bind_drip_art(drip_record,row,nf)
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
              break
          except ValueError as error:
            if not level or 'texture table exceeded' not in str(error) or not pickups.present(sc):raise
            report.setdefault('refused_pickups',[]).append({'scene':info['scene_name'],'level':level,'reason':str(error)})
            if level-1<pickups.level_floor(info['scene_name']):
                raise ValueError(f'{info["scene_name"]}: the scene bank no longer fits its pickups at level '
                                 f'{level}, below its floor {pickups.level_floor(info["scene_name"])} '
                                 '(host/pickups.py LEVEL_FLOOR); something new in the bank pushed them out') from error
            # Put back every view the failed attempt already rewrote.
            for row,saved,pack in before:
                row.clear();row.update(saved);(ROOT/row['path']).write_bytes(pack)
            if level==pickups.LEVEL_BASIC and not actors:break
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
        for key in ('pages','textures','cluts','draws','edges','stream_bytes','alpha_cover_bytes','animation_bytes','format_features','breakables','actors','hazards','edge_sources','texture_request_to_canonical'):
            row[key]=cooked[key]
        row.update(bytes=cooked['pack_bytes'],sha256=cooked['pack_sha256'])
    for info in SCENES:
        if info['scene_id'] not in scenes:scenes[info['scene_id']]=Scene(source,info['file'])
        for external in scenes[info['scene_id']].file.externals:
            name=Path(external.path).name
            if (source.directory/name).is_file():source.file(name)
    report['scenes']=[scene_metadata(scenes[info['scene_id']],info) for info in SCENES]
    report['total_pack_bytes']=sum(row['bytes'] for row in report['regions'])
    postpack_npc_bank(report,source,scenes)
    postpack_checkpoints(report,source,scenes);postpack_pogo(report,source,scenes);postpack_camera_locks(report,source,scenes)
    postpack_masks(report)
    postpack_reveal_masks(report,source,scenes)
    for row in report['regions']:
        if row_over_budget(row):
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
    parser.add_argument('--refresh-camera',action='store_true',help='Re-read only the camera tilemaps and lock areas (postpack_camera_locks)')
    args=parser.parse_args()
    if args.refresh_camera:
        report=json.loads((ROOT/'data/regions.json').read_text())
        postpack_camera_locks(report,Source(),{})
        write_report(ROOT/'data/regions.json',report);return
    if args.append_actor_bank:
        report=json.loads((ROOT/'data/regions.json').read_text());source=Source()
        scenes=postpack_actor_bank(report,source);postpack_npc_bank(report,source,scenes);postpack_checkpoints(report,source,scenes);postpack_pogo(report,source,scenes);postpack_camera_locks(report,source,scenes);postpack_masks(report);postpack_reveal_masks(report,source,scenes);write_provenance(report,source)
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
        # The same ceiling world.generate refuses at, applied while cooking so a
        # long batch fails on the slot that breaks it instead of at the end. The
        # basis for the number is written beside world.MAX_REGIONS.
        if chunk_id>MAX_REGIONS:raise ValueError(f'Region chunk table exceeds {MAX_REGIONS} slots')
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
                 sha256=cooked['pack_sha256'],pages=cooked['pages'],textures=cooked['textures'],cluts=cooked['cluts'],
                 draws=cooked['draws'],edges=cooked['edges'],stream_bytes=cooked['stream_bytes'],
                 alpha_cover_bytes=cooked['alpha_cover_bytes'],animation_bytes=cooked['animation_bytes'],format_features=cooked['format_features'],
                 grass=cooked['grass'],grass_impact=cooked.get('grass_impact'),door_debris=cooked.get('door_debris',[]),particle_effects=cooked.get('particle_effects'),breakables=cooked['breakables'],secrets=cooked.get('secrets',[]),actors=cooked['actors'],
                 hazards=cooked['hazards'],benches=cooked.get('benches',[]),bench_clip_base=cooked.get('bench_clip_base'),shrooms=cooked.get('shrooms',[]),decor=cooked.get('decor',[]),edge_sources=cooked['edge_sources'],
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
    postpack_npc_bank(report,source,scenes)
    postpack_checkpoints(report,source,scenes);postpack_pogo(report,source,scenes);postpack_camera_locks(report,source,scenes)
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
        if row_over_budget(row):
            raise ValueError('final region exceeds scenery/RAM admission budget')
    write_provenance(report,source)
    write_report(ROOT/'data/regions.json',report)
    print(f'Cooked {len(regions)} regions ({report.get("total_pack_bytes",0)} bytes), complete={report["complete"]}',flush=True)


if __name__=='__main__':
    main()
