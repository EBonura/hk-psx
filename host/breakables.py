"""Source-driven Breakable C# subset; source data and CIL evidence stay local.

Breakable.Hit calls Break on the first valid hit, with no health counter.
Break toggles authored whole/remnant objects, disables colliders and sets its
isBroken flag. PersistentBoolItem callbacks expose that flag. Effects, debris,
audio and forwarded FSM events are recorded separately rather than fabricated.
"""
import math
import struct
from pathlib import Path

MAX_SCENE_BREAKABLES = 128
MAX_HIT_POINTS = 16


def _local_id(ref):
    if ref['m_FileID']:
        raise ValueError('external Breakable scene-object reference unsupported')
    return ref['m_PathID']


def _components(sc, gid):
    for component in sc.gos[gid]['m_Component']:
        index = _local_id(component['component'])
        if index in sc.objects:
            typ, tree = sc.objects[index]
            yield index, typ, tree


def _descendants(sc, root):
    children = {}
    for tid, tree in sc.transforms.items():
        children.setdefault(tree['m_Father']['m_PathID'], []).append(tid)
    stack = [sc.go_transform[root]]
    result = set()
    while stack:
        tid = stack.pop()
        if tid in result:
            raise ValueError('cyclic Breakable part hierarchy')
        result.add(tid)
        stack.extend(children.get(tid, []))
    return {sc.transforms[tid]['m_GameObject']['m_PathID'] for tid in result}


def collider_polygons(sc, gid, typ, tree):
    offset = tree['m_Offset']
    if typ == 'BoxCollider2D':
        x, y = tree['m_Size']['x'] / 2, tree['m_Size']['y'] / 2
        paths = [[{'x': a, 'y': b} for a, b in [(-x,-y),(x,-y),(x,y),(-x,y)]]]
    elif typ == 'PolygonCollider2D':
        paths = tree['m_Points']['m_Paths']
    else:
        raise ValueError(f'unsupported Breakable hit collider: {typ}')
    result = []
    for path in paths:
        if not 3 <= len(path) <= MAX_HIT_POINTS:
            raise ValueError('Breakable hit polygon requires 3..16 source vertices')
        points = [sc.point(gid, p['x']+offset['x'], p['y']+offset['y'])[:2] for p in path]
        if any(not math.isfinite(v) or abs(v)>512 for point in points for v in point):
            raise ValueError('Breakable world coordinate exceeds bounded Q16 range')
        result.append(points)
    if not result:
        raise ValueError('Breakable has no hit polygon')
    return result


def _audio(sc, tree):
    source = sc.source
    out = {'event': tree['breakAudioEvent'], 'table_id': None, 'options': []}
    table_ref = tree['breakAudioClipTable']
    if table_ref['m_PathID']:
        obj = source.ref(sc.file, table_ref)
        table = source.read(obj)
        out.update(table_id=source.sid(obj), pitch_min=table['pitchMin'], pitch_max=table['pitchMax'])
        for option in table['options']:
            if not option['Clip']['m_PathID']:
                continue
            clip = source.ref(obj.assets_file, option['Clip'])
            data = source.read(clip)
            out['options'].append({'source': source.sid(clip), 'weight': option['Weight'],
                'name': data['m_Name'], 'channels': data['m_Channels'], 'frequency': data['m_Frequency'],
                'seconds': data['m_Length'], 'compression_format': data['m_CompressionFormat'],
                'resource': data['m_Resource']})
    return out


def _literal_action(data, name, kind, size):
    index = data['paramName'].index(name)
    if data['paramDataType'][index] != kind or data['paramByteDataSize'][index] != size:
        raise ValueError(f'unsupported serialized action parameter {name}')
    start = data['paramDataPos'][index]
    raw = bytes(data['byteData'])
    if start < 0 or start+size > len(raw):
        raise ValueError('action parameter byte range')
    value = raw[start:start+size]
    # FsmUtility.ByteArrayToFsmFloat/Bool use value bytes followed by a
    # UseVariable byte; variable names would follow and enlarge the record.
    if kind in (15,17) and value[-1] != 0:
        raise ValueError(f'variable action parameter {name}')
    if kind == 15:
        number = struct.unpack('<f', value[:4])[0]
        if not math.isfinite(number):raise ValueError('non-finite action float')
        return number
    if kind == 17:
        if value[0] not in (0,1):raise ValueError('non-boolean action literal')
        return bool(value[0])
    return struct.unpack('<i', value)[0]


def mask_fades(sc, receiver_gid):
    """Strictly recognize an authored HIT -> iTweenFadeTo mask controller.

    This is a compiled behavior subset, not a general PlayMaker interpreter.
    Unrecognized handlers remain explicit and do not block core Breakable.Hit.
    """
    file = Path(sc.file.name).name
    fades, errors = [], []
    for index, kind, tree in _components(sc, receiver_gid):
        if kind != 'PlayMakerFSM':continue
        try:
            fsm = tree['fsm']
            initial = next(st for st in fsm['states'] if st['name'] == fsm['startState'])
            transition = next(t for t in initial['transitions'] if t['fsmEvent']['name'] == 'HIT')
            state = next(st for st in fsm['states'] if st['name'] == transition['toState'])
            data = state['actionData']
            if data['actionNames'] != ['HutongGames.PlayMaker.Actions.iTweenFadeTo'] or data['actionEnabled'] != [1]:
                raise ValueError('HIT destination is not one enabled iTweenFadeTo action')
            owner = data['fsmOwnerDefaultParams']
            if len(owner)!=1 or owner[0]['ownerOption']!=0:
                raise ValueError('iTweenFadeTo target is not owner')
            alpha = _literal_action(data,'alpha',15,5)
            duration = _literal_action(data,'time',15,5)
            delay = _literal_action(data,'delay',15,5)
            children = _literal_action(data,'includeChildren',17,2)
            ease = _literal_action(data,'easeType',7,4)
            loop = _literal_action(data,'loopType',7,4)
            realtime = _literal_action(data,'realTime',17,2)
            if not 0<=alpha<=1 or not 0<duration<=10 or delay!=0 or ease!=21 or loop!=0 or realtime:
                raise ValueError('unsupported iTweenFadeTo timing/ease/loop')
            # Both installed EaseType enums declare linear=21 in metadata.
            gids = _descendants(sc,receiver_gid) if children else {receiver_gid}
            renderers=[]
            for gid in gids:
                for renderer_id, renderer_type, renderer in _components(sc,gid):
                    if renderer_type=='SpriteRenderer' and renderer['m_Enabled']:
                        renderers.append({'source':f'{file}:{renderer_id}','id':renderer_id,
                                          'initial_alpha':renderer['m_Color']['a']})
            if not renderers:raise ValueError('mask controller has no sprite renderers')
            fades.append({'source_fsm':f'{file}:{index}','fsm_name':fsm['name'],
                'from_state':initial['name'],'to_state':state['name'],'event':'HIT',
                'renderers':renderers,'renderer_ids':[r['id']for r in renderers],
                'renderer_sources':[r['source']for r in renderers],
                'target_alpha':alpha,'seconds':duration,'ticks_60hz':math.ceil(duration*60-1e-5),
                'include_children':children,'ease':'linear','ease_enum':ease})
        except Exception as error:
            errors.append({'source':f'{file}:{index}','type':'forwarded Breakable event handler','error':str(error)})
    return fades, errors


def breakable_sources(sc, bounds=None, errors=None):
    """Return authored records, optionally overlapping (xmin,ymin,xmax,ymax).

    state_index is the sorted ordinal of ALL Breakable components in the source
    scene, independent of region bounds, activation and draw ordering. Keep the
    state bitmap associated with its scene; source is the persistent identity.
    If errors is supplied, unsupported records are appended there; otherwise a
    malformed selected object fails explicitly rather than silently disappearing.
    """
    file = Path(sc.file.name).name
    all_ids = sorted(o.path_id for o in sc.file.objects.values()
                     if o.type.name == 'MonoBehaviour' and sc.source.typename(o) == 'Breakable')
    if len(all_ids) > MAX_SCENE_BREAKABLES:
        raise ValueError(f'Breakable scene state budget exceeded: {len(all_ids)} > {MAX_SCENE_BREAKABLES}')
    result = []
    for state_index, index in enumerate(all_ids):
        try:
            if index not in sc.objects:
                raise ValueError('Breakable schema was not successfully read')
            typ, tree = sc.objects[index]
            gid = _local_id(tree['m_GameObject'])
            if not tree['m_Enabled'] or not sc.active(gid):
                continue
            position = sc.point(gid)
            if not tree['inertForegroundThreshold'] <= position[2] <= tree['inertBackgroundThreshold']:
                continue
            colliders = [(i, t, c) for i, t, c in _components(sc, gid) if t.endswith('Collider2D')]
            if not colliders:
                raise ValueError('Breakable has no body collider')
            body_id, collider_type, body = colliders[0]  # Awake.GetComponent<Collider2D>
            if not body['m_Enabled']:
                continue
            hit_polygons = collider_polygons(sc, gid, collider_type, body)
            points = [p for poly in hit_polygons for p in poly]
            box = [min(p[0] for p in points), min(p[1] for p in points),
                   max(p[0] for p in points), max(p[1] for p in points)]
            if bounds and (box[2]<bounds[0] or box[0]>bounds[2] or box[3]<bounds[1] or box[1]>bounds[3]):
                continue
            whole_gids = set()
            remnant_gids = set()
            for ref in tree['wholeParts']:
                if ref['m_PathID']:
                    whole_gids.update(_descendants(sc, _local_id(ref)))
            for ref in tree['remnantParts']:
                if ref['m_PathID']:
                    remnant_gids.update(_descendants(sc, _local_id(ref)))
            off = {_local_id(tree['wholeRenderer'])} if tree['wholeRenderer']['m_PathID'] else set()
            on = set()
            disabled = {body_id}
            for part_gid in whole_gids | remnant_gids:
                for part_id, part_type, part in _components(sc, part_gid):
                    if part_type == 'SpriteRenderer' and part['m_Enabled']:
                        (off if part_gid in whole_gids else on).add(part_id)
                    elif part_type.endswith('Collider2D') and part_gid in whole_gids and part['m_Enabled']:
                        disabled.add(part_id)
            for renderer_id in off | on:
                if sc.objects.get(renderer_id, (None,))[0] != 'SpriteRenderer':
                    raise ValueError('non-SpriteRenderer Breakable static part')
            if off & on:
                raise ValueError('Breakable renderer is both whole and remnant')
            persistence = []
            for component_id, component_type, component in _components(sc, gid):
                if component_type == 'PersistentBoolItem':
                    persistence.append({'source': f'{file}:{component_id}',
                        'authored_id': component['persistentBoolData']['id'],
                        'runtime_id_if_empty': sc.gos[gid]['m_Name'],
                        'authored_scene_name': component['persistentBoolData']['sceneName'],
                        'semi_persistent': bool(component['semiPersistent']), 'dont_save': bool(component['dontSave'])})
            debris = []
            for ref in tree['debrisParts']:
                if not ref['m_PathID']:
                    continue
                part_gid = _local_id(ref)
                components = [{'source': f'{file}:{part_id}', 'type': part_type,
                               **({'serialized': part} if part_type in ('Rigidbody2D','SpinSelf','ObjectBounce') else {})}
                              for part_id, part_type, part in _components(sc, part_gid)]
                debris.append({'game_object': f'{file}:{part_gid}', 'name': sc.gos[part_gid]['m_Name'],
                               'position': sc.point(part_gid), 'components': components})
            receiver = tree['hitEventReciever']
            fades, event_errors = mask_fades(sc,_local_id(receiver)) if receiver['m_PathID'] else ([],[])
            result.append({'source': f'{file}:{index}', 'game_object': f'{file}:{gid}', 'gid': gid,
                'name': sc.gos[gid]['m_Name'], 'state_index': state_index, 'scene_state_count': len(all_ids),
                'position': position, 'hit_points': 1, 'box': box, 'hit_polygons': hit_polygons,
                'body_collider': f'{file}:{body_id}', 'body_is_trigger': bool(body['m_IsTrigger']),
                'off_renderer_ids': sorted(off), 'on_renderer_ids': sorted(on),
                'disabled_collider_ids': sorted(disabled),
                'off_renderer_sources': [f'{file}:{i}' for i in sorted(off)],
                'on_renderer_sources': [f'{file}:{i}' for i in sorted(on)],
                'disabled_collider_sources': [f'{file}:{i}' for i in sorted(disabled)],
                'persistence': persistence, 'audio': _audio(sc, tree), 'mask_fades': fades, 'event_errors': event_errors,
                'debris': debris, 'fling_speed': [tree['flingSpeedMin'], tree['flingSpeedMax']],
                'angle_offset': tree['angleOffset'],
                'forwarded_events': {'hit_receiver': sc.source.sid(sc.source.ref(sc.file,receiver)) if receiver['m_PathID'] else None,
                    'receiver_event': 'HIT' if receiver['m_PathID'] else None,
                    'self_event': 'BREAK' if tree['forwardBreakEvent'] else None},
                'limitations': ['Debris physics, dust and impact effects, audio playback and forwarded FSM events require separate runtime support',
                    'PersistentBoolItem identity/state is recorded; no memory-card or retail-save compatibility is implied'],
            })
        except Exception as error:
            if errors is None:
                raise
            errors.append({'id': f'{file}:{index}', 'type': 'Breakable', 'error': str(error)})
    return result


def bind_breakables(records, draws, edges):
    """Attach current region indices without changing stable scene state indices."""
    draw_ids = {d['source']: i for i, d in enumerate(draws)}
    out = []
    for record in records:
        bound = dict(record)
        bound['off_draws'] = [draw_ids[s] for s in record['off_renderer_sources'] if s in draw_ids]
        bound['on_draws'] = [draw_ids[s] for s in record['on_renderer_sources'] if s in draw_ids]
        bound['edge_indices'] = [i for i, edge in enumerate(edges) if edge['source'] in record['disabled_collider_sources']]
        bound['mask_fades'] = [dict(f,draw_indices=[draw_ids[s] for s in f['renderer_sources'] if s in draw_ids]) for f in record.get('mask_fades',[])]
        bound['unresident_renderer_sources'] = [s for s in record['off_renderer_sources']+record['on_renderer_sources'] if s not in draw_ids]
        out.append(bound)
    return out


if __name__ == '__main__':
    from source import Source, ROOT, dump
    from scene import Scene
    source = Source(); scene = Scene(source, 'level6'); errors = []
    records = breakable_sources(scene, errors=errors)
    dump(ROOT/'.hkpsx/breakables-source.json', {'scene': 'Tutorial_01', 'records': records, 'errors': errors,
         'methods': ['Breakable.Awake','Breakable.Start','Breakable.Hit','Breakable.Break','Breakable.SetStaticPartsActivation','PersistentBoolItem.SetMyID'],
         'source_constraints': 'Original C# first-hit behavior; no generic PlayMaker execution'})
    print(f'{len(records)} active in-depth Breakables, {len(errors)} unsupported records')
    for record in records:
        print(record['state_index'], record['name'], record['box'], record['off_renderer_ids'], record['on_renderer_ids'], record['disabled_collider_ids'])
