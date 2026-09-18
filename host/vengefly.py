"""Recognize the verified Vengefly (Buzzer) `chaser` variant for guest admission.

The controller lives in shared/hk-sim/src/vengefly.rs; this module admits only
placed instances whose serialized FSM parameters and body match the audited
contract in .hkpsx/vengefly/CONTRACT.md. Anything else stays unsupported.
"""
from runner import ASSEMBLIES, _one, axis_aligned_bounds, body_box
from focus import action_fields
import hashlib

CLIPS = {'Idle': (5, 12, 0), 'TurnToIdle': (7, 12, 1), 'Startle': (4, 12, 2),
         'Chase': (4, 12, 0), 'TurnToFly': (6, 12, 1)}
BODY_SIZE = (1.25, 0.625)
BODY_OFFSET = (0.0, -0.1875)
ALERT_RADIUS = 0.5 * 15.608528137207031
ACTIONS = {
    ('Idle', 'IdleBuzz'): {'waitMin': .75, 'waitMax': 1., 'speedMax': 1.75, 'accelerationMax': 15., 'roamingRange': 1.},
    ('Idle', 'FaceDirection'): {'spriteFacesRight': False, 'playNewAnimation': True, 'newAnimationClip': 'TurnToIdle',
                                'everyFrame': True, 'pauseBetweenTurns': True, 'pauseTime': .5},
    ('Idle', 'Tk2dPlayAnimation'): {'clipName': 'Idle'},
    ('Startle', 'Tk2dPlayAnimation'): {'clipName': 'Startle'},
    ('Chase Start', 'Tk2dPlayAnimation'): {'clipName': 'Chase'},
    ('Chase Start', 'Tk2dPlayFrame'): {'frame': 3},
    ('Chase Start', 'Wait'): {'time': 0.},
    ('Chase - In Sight', 'ChaseObject'): {'speedMax': 5., 'acceleration': .045, 'targetSpread': 0.},
    ('Chase - In Sight', 'FaceDirection'): {'newAnimationClip': 'TurnToFly', 'pauseTime': .5, 'pauseBetweenTurns': True},
    ('Chase - Out of Sight', 'ChaseObject'): {'speedMax': 5., 'acceleration': .045, 'targetSpread': 0.},
    ('Chase - Out of Sight', 'Wait'): {'time': 'Attention Span'},
    ('Stop', 'Decelerate'): {'deceleration': .12},
    ('Stop', 'Wait'): {'time': 1.},
    ('Stop', 'Tk2dPlayAnimation'): {'clipName': 'Idle'},
    ('Stop', 'Tk2dPlayFrame'): {'frame': 3},
}
TRANSITIONS = {
    'Initiate': [('FINISHED', 'Idle'), ('ALERT', 'Chase Start')],
    'Idle': [('ALERT', 'Startles?'), ('TOOK DAMAGE', 'Startles?')],
    'Startles?': [('TRUE', 'Startle'), ('FALSE', 'Chase Start')],
    'Startle': [('ANIM END', 'Chase Start')],
    'Chase Start': [('WAIT', 'Chase - In Sight')],
    'Chase - In Sight': [('WAIT', 'Chase - Out of Sight')],
    'Chase - Out of Sight': [('ALERT', 'Chase - In Sight'), ('WAIT', 'Stop')],
    'Stop': [('WAIT', 'Idle')],
}


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def _scalar(value):
    if isinstance(value, dict):
        return value['name'] if value.get('useVariable') else value['value']
    return value


def recognize(sc, actor):
    from actors import _component_records
    source = sc.source
    records = _component_records(sc, actor['game_object'])
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Vengefly methods require a fresh source audit: ' + name)
    fsms = [d['fsm'] for _, t, d in records if t == 'PlayMakerFSM' and d['fsm']['name'] == 'chaser']
    if len(fsms) != 1:
        raise ValueError('no single Vengefly chaser FSM')
    fsm = fsms[0]
    if fsm['startState'] != 'Initiate':
        raise ValueError('Vengefly chaser begins in unsupported state')
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}
    if variables.get('Attention Span') != 10. or variables.get('Startles') != 1 or variables.get('Start Alert') != 0:
        raise ValueError('unsupported Vengefly chaser variables')
    states = {st['name']: st for st in fsm['states']}
    for name, transitions in TRANSITIONS.items():
        if name not in states or [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != transitions:
            raise ValueError('unsupported Vengefly chaser transitions: ' + name)
    for (state, action), expected in ACTIONS.items():
        data = states[state]['actionData']
        matches = [i for i, n in enumerate(data['actionNames']) if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]]
        if len(matches) != 1:
            raise ValueError(f'unsupported Vengefly action set: {state}/{action}')
        fields = action_fields(data, matches[0])
        for key, value in expected.items():
            actual = _scalar(fields.get(key))
            if isinstance(value, float) and isinstance(actual, (int, float)) and not isinstance(actual, bool):
                if not _near(actual, value):
                    raise ValueError(f'unsupported Vengefly parameter: {state}/{action}.{key}')
            elif actual != value:
                raise ValueError(f'unsupported Vengefly parameter: {state}/{action}.{key}')
    gid = actor['game_object']
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('unsupported Vengefly initial rotation or scale')
    body = body_box(records, 'Vengefly')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Vengefly body collider')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Vengefly rigid body')
    _, recoil = _one(records, 'Recoil')
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 15 or recoil['recoilDuration'] != .25 or recoil['preventRecoilUp']:
        raise ValueError('unsupported Vengefly recoil variant')
    _, sight = _one(records, 'LineOfSightDetector')
    if not sight['m_Enabled'] or len(sight['alertRanges']) != 1:
        raise ValueError('unsupported Vengefly line of sight detector')
    alert_id = sight['alertRanges'][0]['m_PathID']
    alert_gid = sc.objects[alert_id][1]['m_GameObject']['m_PathID']
    alert_records = _component_records(sc, alert_gid)
    _, circle = _one(alert_records, 'CircleCollider2D')
    alert_scale = sc.transforms[sc.go_transform[alert_gid]]['m_LocalScale']
    if not sc.active(alert_gid) or not circle['m_IsTrigger'] or circle['m_Offset'] != {'x': 0., 'y': 0.} \
            or not _near(circle['m_Radius'] * alert_scale['x'], ALERT_RADIUS) or not _near(alert_scale['x'], alert_scale['y']) \
            or sc.transforms[sc.go_transform[alert_gid]]['m_LocalPosition'] != {'x': 0., 'y': 0., 'z': 0.}:
        raise ValueError('unsupported Vengefly alert range')
    _, animator = _one(records, 'tk2dSpriteAnimator')
    library = source.read(source.ref(sc.file, animator['library']))
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Vengefly animation: ' + name)
    bounds = axis_aligned_bounds([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]],
                                 list(BODY_OFFSET), list(BODY_SIZE))
    return {'kind': 'Vengefly', 'guest_enabled': True, 'alert_radius': ALERT_RADIUS,
            'body_bounds_local': bounds,
            'limitations': ['Gravity-free dynamic body on the bounded terrain solver; Unity fixed-step ordering and Random.Range are not reproduced.',
                            'Startle one-shot and the live buzz loop audio are not presented.',
                            'The breaker corpse is removed on landing; its break pieces are not presented.']}
