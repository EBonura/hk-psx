"""Recognize the verified Aspid Hunter (Spitter) `spitter` FSM for guest admission.

The controller lives in shared/hk-sim/src/aspid.rs; this module admits only
placed instances matching .hkpsx/aspid/CONTRACT.md.
"""
from runner import ASSEMBLIES, axis_aligned_bounds
from focus import action_fields
import hashlib

CLIPS = {'Fly': (8, 12, 0), 'TurnToFly': (10, 12, 1), 'Fire Long': (12, 12, 2)}
SHOT_CLIPS = {'Idle': (4, 20, 0), 'Impact': (6, 20, 2)}
BODY_SIZE = (1.09375, 1.234375)
BODY_OFFSET = (-0.0625, -0.0390625)
ALERT_RADIUS = 0.5 * 15.608528137207031
UNALERT_RADIUS = 12.100000381469727
ACTIONS = {
    ('Idle', 'IdleBuzz'): {'waitMin': .75, 'waitMax': 1., 'speedMax': 1.75, 'accelerationMax': 15., 'roamingRange': 1.},
    ('Idle', 'FaceDirection'): {'newAnimationClip': 'TurnToFly', 'pauseTime': .5, 'pauseBetweenTurns': True, 'spriteFacesRight': False},
    ('Distance Fly', 'DistanceFly'): {'distance': 7., 'speedMax': 4., 'acceleration': .1, 'targetsHeight': False},
    ('Distance Fly', 'WaitRandom'): {'timeMin': 1.5, 'timeMax': 2.25},
    ('Distance Fly', 'FloatCompare'): {'float2': 8., 'greaterThan': 'UNALERT'},
    ('Raycast', 'FloatCompare'): {'float2': 14., 'greaterThan': 'FALSE'},
    ('Fly Back', 'Wait'): {'time': .5},
    ('Fly Back', 'DistanceFly'): {'distance': 8.25, 'speedMax': 4., 'acceleration': .1},
    ('Fire Anticipate', 'DistanceFly'): {'distance': 9., 'speedMax': 2., 'acceleration': .1},
    ('Fire Anticipate', 'Tk2dPlayAnimationWithEvents'): {'clipName': 'Fire Long', 'animationTriggerEvent': 'WAIT'},
    ('Fire', 'FireAtTarget'): {'speed': 15., 'spread': 0.},
}
TRANSITIONS = {
    'Idle': [('ALERT', 'Alert')], 'Alert': [('FINISHED', 'Distance Fly')],
    'Distance Fly': [('WAIT', 'Raycast'), ('UNALERT', 'Unalert Frame')],
    'Raycast': [('WAIT', 'Raycast Check'), ('FALSE', 'Distance Fly')],
    'Raycast Check': [('TRUE', 'Distance Fly'), ('FALSE', 'Fly Back')],
    'Fly Back': [('FINISHED', 'Fire Anticipate')], 'Fire Anticipate': [('WAIT', 'Fire')],
    'Fire': [('WAIT', 'Fire Dribble')], 'Fire Dribble': [('WAIT', 'Distance Fly')], 'Unalert Frame': [('FINISHED', 'Idle')],
}


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def _scalar(value):
    if isinstance(value, dict):
        return value['name'] if value.get('useVariable') else value['value']
    return value


def _only(records, kind):
    matches = [(sid, data) for sid, typ, data in records if typ == kind]
    if len(matches) != 1:
        raise ValueError('Aspid requires exactly one ' + kind)
    return matches[0]


def recognize(sc, actor):
    from actors import _component_records
    source = sc.source
    records = _component_records(sc, actor['game_object'])
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Aspid methods require a fresh source audit: ' + name)
    fsms = [d['fsm'] for _, t, d in records if t == 'PlayMakerFSM' and d['fsm']['name'] == 'spitter']
    if len(fsms) != 1 or fsms[0]['startState'] != 'Idle':
        raise ValueError('no single Aspid spitter FSM')
    fsm = fsms[0]
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}
    if variables.get('startAlert') not in (0, 1):
        raise ValueError('unsupported Aspid start alert')
    states = {st['name']: st for st in fsm['states']}
    for name, transitions in TRANSITIONS.items():
        if name not in states or [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != transitions:
            raise ValueError('unsupported Aspid transitions: ' + name)
    for (state, action), expected in ACTIONS.items():
        data = states[state]['actionData']
        matches = [i for i, n in enumerate(data['actionNames']) if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]]
        if len(matches) != 1:
            raise ValueError(f'unsupported Aspid action set: {state}/{action}')
        fields = action_fields(data, matches[0])
        for key, value in expected.items():
            actual = _scalar(fields.get(key))
            if isinstance(value, float) and isinstance(actual, (int, float)) and not isinstance(actual, bool):
                if not _near(actual, value):
                    raise ValueError(f'unsupported Aspid parameter: {state}/{action}.{key}')
            elif actual != value:
                raise ValueError(f'unsupported Aspid parameter: {state}/{action}.{key}')
    fire = states['Fire']['actionData']
    shot_ref = next((p['value'] for p in fire['fsmGameObjectParams'] if not p['useVariable'] and p['value']['m_PathID']), None)
    if shot_ref is None:
        raise ValueError('Aspid shot prefab reference missing')
    shot_obj = source.ref(sc.file, shot_ref)
    shot_go = source.read(shot_obj)
    if shot_go['m_Name'] != 'Spitter Shot R':
        raise ValueError('unsupported Aspid shot prefab')
    shot_parts = {}
    for ref in shot_go['m_Component']:
        component = source.ref(shot_obj.assets_file, ref['component'])
        shot_parts[source.typename(component)] = (component, source.read(component))
    for kind in ('Rigidbody2D', 'BoxCollider2D', 'DamageHero', 'EnemyBullet', 'tk2dSpriteAnimator', 'tk2dSprite', 'Transform'):
        if kind not in shot_parts:
            raise ValueError('unsupported Aspid shot component set')
    if not _near(shot_parts['Rigidbody2D'][1]['m_GravityScale'], .05) or shot_parts['DamageHero'][1]['damageDealt'] != 1:
        raise ValueError('unsupported Aspid shot body')
    shot_library_o = source.ref(shot_obj.assets_file, shot_parts['tk2dSpriteAnimator'][1]['library'])
    shot_library = source.read(shot_library_o)
    shot_by_name = {c['name']: c for c in shot_library['clips'] if c['name']}
    for name, (frames, fps, wrap) in SHOT_CLIPS.items():
        clip = shot_by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Aspid shot animation: ' + name)
    gid = actor['game_object']
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('unsupported Aspid initial rotation or scale')
    _, body = _only(records, 'BoxCollider2D')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Aspid body collider')
    _, rigid = _only(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Aspid rigid body')
    _, recoil = _only(records, 'Recoil')
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 15 or not _near(recoil['recoilDuration'], .15) or recoil['preventRecoilUp']:
        raise ValueError('unsupported Aspid recoil variant')
    circles = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        child_gid = sc.transforms[child['m_PathID']]['m_GameObject']['m_PathID']
        for _, kind, data in _component_records(sc, child_gid):
            if kind == 'CircleCollider2D':
                circles[sc.gos[child_gid]['m_Name']] = data['m_Radius'] * sc.transforms[child['m_PathID']]['m_LocalScale']['x']
    if not _near(circles.get('Alert Range New', 0), ALERT_RADIUS) or not _near(circles.get('Unalert Range', 0), UNALERT_RADIUS):
        raise ValueError('unsupported Aspid alert ranges')
    _, animator = _only(records, 'tk2dSpriteAnimator')
    library = source.read(source.ref(sc.file, animator['library']))
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Aspid animation: ' + name)
    triggers = [i for i, f in enumerate(by_name['Fire Long']['frames']) if f.get('triggerEvent')]
    if triggers != [9]:
        raise ValueError('unsupported Aspid Fire Long trigger frame')
    bounds = axis_aligned_bounds([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]], list(BODY_OFFSET), list(BODY_SIZE))
    return {'kind': 'Aspid', 'guest_enabled': True, 'body_bounds_local': bounds, 'start_alert': bool(variables.get('startAlert')),
            'shot': {'source': source.sid(shot_obj), 'library': source.sid(shot_library_o), 'library_object': shot_library_o,
                     'scale': shot_parts['Transform'][1]['m_LocalScale']['x'] * shot_parts['EnemyBullet'][1]['scaleMin']},
            'limitations': ['Gravity-free dynamic body on the bounded terrain solver; 50 Hz fixed steps from a 60 Hz accumulator.',
                            'Shots fly straight under gravity .05 without rotation or stretch; dribble spatter, shot audio and shockwave are not presented.',
                            'The collider-less corpse is removed on its first landing instead of falling out of the scene.']}
