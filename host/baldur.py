"""Recognize the verified Baldur (Roller) `Roller` FSM variant for guest admission.

The controller lives in shared/hk-sim/src/baldur.rs; this module admits only
placed instances whose FSM parameters and body match .hkpsx/baldur/CONTRACT.md.
"""
from runner import ASSEMBLIES, axis_aligned_bounds
from focus import action_fields
import hashlib

CLIPS = {'Start': (4, 10, 2), 'Stop': (4, 10, 2), 'Idle': (4, 12, 0), 'Roll': (3, 12, 0)}
BODY_SIZE = (1.09375, 1.09375)
BODY_OFFSET = (-0.015625, -0.109375)
ALERT_SCALE = (21.139999389648438, 1.899999976158142)
ALERT_OFFSET_Y = 0.3100000023841858
VARIABLES = {'Acceleration': .45, 'Max Speed': 11., 'Roll time Min': 2., 'Roll time Max': 3., 'Stop Time': .5}
TRANSITIONS = {
    'Initiate': [('FINISHED', 'Idle')], 'Idle': [('ALERT', 'Facing Check')],
    'Facing Check': [('LEFT', 'Start Left'), ('RIGHT', 'Start Right')],
    'Start Right': [('FINISHED', 'Start')], 'Start Left': [('FINISHED', 'Start')], 'Start': [('WAIT', 'Left or right?')],
    'Left or right?': [('RIGHT', 'Roll R'), ('LEFT', 'Roll L')],
    'Roll R': [('WALL', 'Collide Right'), ('STOP', 'Stop'), ('RECOIL HORIZONTAL', 'Recoil Decel R')],
    'Roll L': [('WALL', 'Collide Left'), ('STOP', 'Stop'), ('RECOIL HORIZONTAL', 'Recoil Decel L')],
    'Collide Right': [('WAIT', 'In Air')], 'Collide Left': [('WAIT', 'In Air')], 'In Air': [('GROUND', 'Land')],
    'Land': [('FINISHED', 'Left or right?')], 'Stop': [('WAIT', 'Rest')], 'Rest': [('WAIT', 'Idle')],
    'Recoil Decel R': [('FINISHED', 'Roll R')], 'Recoil Decel L': [('FINISHED', 'Roll L')],
}
ANGLES = {'Collide Right': (115., 12.), 'Collide Left': (65., 12.)}


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def _scalar(value):
    if isinstance(value, dict):
        return value['name'] if value.get('useVariable') else value['value']
    return value


def _only(records, kind):
    matches = [(sid, data) for sid, typ, data in records if typ == kind]
    if len(matches) != 1:
        raise ValueError('Baldur requires exactly one ' + kind)
    return matches[0]


def recognize(sc, actor):
    from actors import _component_records
    source = sc.source
    records = _component_records(sc, actor['game_object'])
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Baldur methods require a fresh source audit: ' + name)
    fsms = [d['fsm'] for _, t, d in records if t == 'PlayMakerFSM']
    if len(fsms) != 1 or fsms[0]['name'] != 'Roller' or fsms[0]['startState'] != 'Initiate':
        raise ValueError('no single Baldur Roller FSM')
    fsm = fsms[0]
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}
    for name, value in VARIABLES.items():
        if not _near(variables.get(name, float('nan')), value):
            raise ValueError('unsupported Baldur variable: ' + name)
    states = {st['name']: st for st in fsm['states']}
    for name, transitions in TRANSITIONS.items():
        if name not in states or [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != transitions:
            raise ValueError('unsupported Baldur transitions: ' + name)
    for state, (angle, speed) in ANGLES.items():
        data = states[state]['actionData']
        matches = [i for i, n in enumerate(data['actionNames']) if n.endswith('SetVelocityAsAngle') and data['actionEnabled'][i]]
        if len(matches) != 1:
            raise ValueError('unsupported Baldur collide: ' + state)
        fields = action_fields(data, matches[0])
        if not _near(_scalar(fields['angle']), angle) or not _near(_scalar(fields['speed']), speed):
            raise ValueError('unsupported Baldur collide launch: ' + state)
    for state in ('Roll R', 'Roll L'):
        data = states[state]['actionData']
        names = [n.rsplit('.', 1)[-1] for i, n in enumerate(data['actionNames']) if data['actionEnabled'][i]]
        add = 'FloatAdd' if state == 'Roll R' else 'FloatSubtract'
        if add not in names or 'FloatClamp' not in names or names.count('CheckCollisionSide') != 3 or 'FloatCompare' not in names:
            raise ValueError('unsupported Baldur roll actions: ' + state)
        fields = action_fields(data, names.index(add) if data['actionEnabled'] == [1] * len(data['actionNames']) else [i for i, n in enumerate(data['actionNames']) if n.endswith(add)][0])
        if _scalar(fields.get('perSecond', False)):
            raise ValueError('unsupported Baldur per-second acceleration')
    gid = actor['game_object']
    matrix = sc.world(sc.go_transform[gid])
    # Placed Rollers start mirrored (scale.x -1); the FSM sets the scale sign
    # itself from the first Idle frame, so only the magnitude matters here.
    if abs(abs(matrix[0][0]) - 1) > 1e-6 or abs(matrix[1][1] - 1) > 1e-6 or abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6:
        raise ValueError('unsupported Baldur initial rotation or scale')
    # Some placed Rollers carry the same box twice; identical copies are one body.
    bodies = [d for _, t, d in records if t == 'BoxCollider2D']
    if not 1 <= len(bodies) <= 2 or any((b['m_Size'], b['m_Offset'], b['m_IsTrigger']) != (bodies[0]['m_Size'], bodies[0]['m_Offset'], bodies[0]['m_IsTrigger']) for b in bodies):
        raise ValueError('unsupported Baldur body colliders')
    body = bodies[0]
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Baldur body collider')
    _, rigid = _only(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or not _near(rigid['m_GravityScale'], .8) or rigid['m_LinearDamping'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Baldur rigid body')
    _, recoil = _only(records, 'Recoil')
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 25 or not _near(recoil['recoilDuration'], .15) or recoil['preventRecoilUp']:
        raise ValueError('unsupported Baldur recoil variant')
    _, sight = _only(records, 'LineOfSightDetector')
    if not sight['m_Enabled'] or len(sight['alertRanges']) != 1:
        raise ValueError('unsupported Baldur line of sight detector')
    alert_gid = sc.objects[sight['alertRanges'][0]['m_PathID']][1]['m_GameObject']['m_PathID']
    if sc.gos[alert_gid]['m_Name'] != 'Alert Range New' or not sc.active(alert_gid):
        raise ValueError('unsupported Baldur alert range object')
    _, box = _only(_component_records(sc, alert_gid), 'BoxCollider2D')
    alert = sc.transforms[sc.go_transform[alert_gid]]
    if not box['m_IsTrigger'] or box['m_Size'] != {'x': 1., 'y': 1.} or box['m_Offset'] != {'x': 0., 'y': 0.} \
            or not all(_near(alert['m_LocalScale'][k], v) for k, v in zip('xy', ALERT_SCALE)) \
            or not _near(alert['m_LocalPosition']['x'], 0.) or not _near(alert['m_LocalPosition']['y'], ALERT_OFFSET_Y):
        raise ValueError('unsupported Baldur alert range')
    _, animator = _only(records, 'tk2dSpriteAnimator')
    library = source.read(source.ref(sc.file, animator['library']))
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Baldur animation: ' + name)
    bounds = axis_aligned_bounds([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]],
                                 list(BODY_OFFSET), list(BODY_SIZE))
    return {'kind': 'Baldur', 'guest_enabled': True, 'body_bounds_local': bounds,
            'alert_bounds_local': [-ALERT_SCALE[0] / 2, ALERT_OFFSET_Y - ALERT_SCALE[1] / 2, ALERT_SCALE[0] / 2, ALERT_OFFSET_Y + ALERT_SCALE[1] / 2],
            'limitations': ['Gravity body on the bounded terrain solver; WALL and GROUND come from the blocked solver axes instead of the contact rays.',
                            'Roll dust, the roll audio loop and land effects are not presented.',
                            'The rolling corpse uses box bounds and the solver slide instead of the circle body with 0.7 linear damping, and is removed 120 ticks after landing.']}
