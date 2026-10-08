from focus import action_fields

CLUT = (320, 493, 16, 1)
CLUT_ROWS = 2
def _scalar(value):
    if isinstance(value, dict):
        return value['name'] if value.get('useVariable') else value['value']
    return value


def _fsm(records, name):
    fsms = [d['fsm'] for _, t, d in records if t == 'PlayMakerFSM' and d['fsm']['name'] == name]
    if len(fsms) != 1:
        raise ValueError(f'expected one {name} FSM')
    return fsms[0]


def _states(fsm, contract, who):
    states = {s['name']: s for s in fsm['states']}
    for name, expected in contract.items():
        if name not in states or [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != expected:
            raise ValueError(f'unsupported {who} transitions: {name}')
    return states


def _actions(state, action):
    data = state['actionData']
    return [action_fields(data, i) for i, n in enumerate(data['actionNames'])
            if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]]


def _variables(fsm):
    return {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
            for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}


def _kids(sc, gid):
    out = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        kid = sc.transforms[child['m_PathID']]['m_GameObject']['m_PathID']
        if kid in sc.gos:
            out[sc.gos[kid]['m_Name']] = kid
    return out


def _box_world(sc, gid, box):
    """World bounds of a (non-rotated-shape) BoxCollider2D under any quarter turn."""
    ox, oy = box['m_Offset']['x'], box['m_Offset']['y']
    hx, hy = box['m_Size']['x'] / 2, box['m_Size']['y'] / 2
    pts = [sc.point(gid, ox + dx, oy + dy)[:2] for dx in (-hx, hx) for dy in (-hy, hy)]
    return [min(p[0] for p in pts), min(p[1] for p in pts), max(p[0] for p in pts), max(p[1] for p in pts)]


DRIP_CLIPS = ('Idle', 'Drip', 'Fall', 'Impact')


def drip_sources(sc):
    """Active `WaterDrip`s with the component values the runtime assumes."""
    from breakables import _components
    out = []
    for gid in sorted(sc.gos):
        if not sc.active(gid):
            continue
        comps = {t: c for _, t, c in _components(sc, gid)}
        if 'WaterDrip' not in comps:
            continue
        w, box = comps['WaterDrip'], comps.get('BoxCollider2D')
        if box is None or box['m_IsTrigger']:
            raise ValueError(f'water drip {sc.gos[gid]["m_Name"]} has no solid box collider')
        matrix = sc.world(sc.go_transform[gid])
        # Crossroads_46/46b sit under parents scaled 0.975 and -1.049 on x
        # only; the drop is drawn with that x scale. Rotation, a y scale or a
        # sprite scale would need more than that, so they are refused.
        if abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6 or abs(matrix[1][1] - 1) > 1e-6 \
                or any(abs(comps['tk2dSprite']['_scale'][k] - 1) > 1e-6 for k in 'xy'):
            raise ValueError('water drip is rotated or scaled on y')
        out.append({'gid': gid, 'name': sc.gos[gid]['m_Name'], 'position': sc.point(gid), 'drip': w,
                    'bottom': box['m_Offset']['y'] - box['m_Size']['y'] / 2, 'x_scale': matrix[0][0],
                    'half_width': abs(matrix[0][0]) * box['m_Size']['x'] / 2, 'offset_x': matrix[0][0] * box['m_Offset']['x'],
                    'sprite': comps['tk2dSprite'], 'animator': comps['tk2dSpriteAnimator']})
    return out


def drip_art(source, sc, atlas, frames):
    """Append the drip sheet to a scene actor bank, or None without drips."""
    drips = drip_sources(sc)
    if not drips:
        return None
    from battle_gates import gate_sheets
    from cook import FOCAL, CAM_Z
    library_ids = {source.sid(source.ref(sc.file, d['animator']['library'])) for d in drips}
    collection_ids = {source.sid(source.ref(sc.file, d['sprite']['collection'])) for d in drips}
    if len(library_ids) != 1 or len(collection_ids) != 1:
        raise ValueError('water drips in one scene use different art')
    d0 = drips[0]
    library_o = source.ref(sc.file, d0['animator']['library'])
    collection_o = source.ref(sc.file, d0['sprite']['collection'])
    library, collection = source.read(library_o), source.read(collection_o)
    clips = {c['name']: c for c in library['clips']}
    order, table = [], []
    for name in DRIP_CLIPS:
        clip = clips[name]
        if clip['wrapMode'] not in (2, 6) or clip['fps'] != int(clip['fps']):
            raise ValueError(f'water drip clip {name!r} is no longer a whole-rate play-once clip')
        ids = []
        for frame in clip['frames']:
            if frame['spriteId'] not in order:
                order.append(frame['spriteId'])
            ids.append(order.index(frame['spriteId']))
        table.append({'name': name, 'fps': clip['fps'], 'frames': ids})
    sheets, rects, boxes = gate_sheets(source, collection_o, collection, [(i, 1.0, 1.0) for i in order], FOCAL / -CAM_Z)
    if len(sheets) != 1:
        raise ValueError('water drip art needs more than one sheet')
    first = len(frames)
    texture = atlas.add(sheets[0], sheets[0].width, sheets[0].height, streamed=True)
    frames.append({'texture': texture, 'box': [0, 0, 1, 1], 'event': {}, 'sprite': f'{source.sid(collection_o)}:drip-sheet'})
    return {'first': first, 'clips': table, 'rects': [r[1:] for r in rects], 'boxes': boxes,
            'library': source.sid(library_o)}


def bind_drip_art(record, row, frame_base):
    if record is None:
        return None
    return dict(record, frame_base=frame_base + record['first'])
