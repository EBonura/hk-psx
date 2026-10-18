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


