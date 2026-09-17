"""Recognize the verified Gruzzer (Fly) `Bouncer Control` variant for guest admission.

The controller lives in shared/hk-sim/src/gruzzer.rs; this module admits only
placed instances whose FSM parameters and body match .hkpsx/gruzzer/CONTRACT.md.
"""
from runner import ASSEMBLIES, _one, axis_aligned_bounds
from focus import action_fields
import hashlib

CLIPS = {'Fly': (7, 10, 0)}
BODY_SIZE = (0.890625, 0.84375)
BODY_OFFSET = (0.0390625, -0.03125)
RANGES = {'Aim': (0., 360.), 'Up Right': (320., 350.), 'Up Left': (190., 220.), 'Down Right': (10., 40.),
          'Up Left 2': (140., 170.), 'Right Down': (190., 220.), 'Down Right 2': (140., 170.),
          'Left Down': (320., 350.), 'Left Up': (10., 40.)}
TRANSITIONS = {
    'Initialise': [('FINISHED', 'Aim')], 'Aim': [('FINISHED', 'Left or Right?')],
    'Left or Right?': [('LEFT', 'Face Left'), ('RIGHT', 'Face Right')],
    'Face Left': [('FINISHED', 'Fly 2')], 'Face Right': [('FINISHED', 'Fly 2')],
    'Fly 2': [('BONK DOWN', 'Hit Down'), ('BONK LEFT', 'Hit Left'), ('BONK RIGHT', 'Hit Right'), ('BONK UP', 'Hit Up')],
    'Hit Up': [('RIGHT', 'Up Right'), ('LEFT', 'Up Left')], 'Hit Down': [('RIGHT', 'Down Right'), ('LEFT', 'Up Left 2')],
    'Hit Right': [('UP', 'Down Right 2'), ('DOWN', 'Right Down')], 'Hit Left': [('UP', 'Left Up'), ('DOWN', 'Left Down')],
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
            raise ValueError('Gruzzer methods require a fresh source audit: ' + name)
    fsms = [d['fsm'] for _, t, d in records if t == 'PlayMakerFSM']
    if len(fsms) != 1 or fsms[0]['name'] != 'Bouncer Control' or fsms[0]['startState'] != 'Initialise':
        raise ValueError('no single Gruzzer Bouncer Control FSM')
    fsm = fsms[0]
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}
    if not _near(variables.get('Speed'), 5.2) or variables.get('Start Up') != 0 or variables.get('Starts Inactive') != 0:
        raise ValueError('unsupported Gruzzer variables')
    states = {st['name']: st for st in fsm['states']}
    for name, transitions in TRANSITIONS.items():
        if name not in states or [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != transitions:
            raise ValueError('unsupported Gruzzer transitions: ' + name)
    for state, (low, high) in RANGES.items():
        data = states[state]['actionData']
        matches = [i for i, n in enumerate(data['actionNames']) if n.endswith('RandomFloat') and data['actionEnabled'][i]]
        if len(matches) != 1:
            raise ValueError('unsupported Gruzzer aim: ' + state)
        fields = action_fields(data, matches[0])
        if not _near(_scalar(fields['min']), low) or not _near(_scalar(fields['max']), high) or _scalar(fields['storeResult']) != 'Angle':
            raise ValueError('unsupported Gruzzer aim range: ' + state)
    data = states['Initialise']['actionData']
    compare = [action_fields(data, i) for i, n in enumerate(data['actionNames']) if n.endswith('FloatCompare')]
    if len(compare) != 1 or not _near(_scalar(compare[0]['float2']), 44.) or compare[0]['lessThan'] != 'FINISHED':
        raise ValueError('unsupported Gruzzer camera range')
    data = states['Fly 2']['actionData']
    names = [n.rsplit('.', 1)[-1] for i, n in enumerate(data['actionNames']) if data['actionEnabled'][i]]
    if names[:2] != ['FaceDirection', 'SetVelocityAsAngle'] or names.count('CheckCollisionSide') != 3 or names.count('CheckCollisionSideEnter') != 3:
        raise ValueError('unsupported Gruzzer flight actions')
    face = action_fields(data, 0)
    if any(_scalar(face[k]) for k in ('spriteFacesRight', 'playNewAnimation', 'pauseBetweenTurns')):
        raise ValueError('unsupported Gruzzer facing')
    gid = actor['game_object']
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('unsupported Gruzzer initial rotation or scale')
    _, body = _one(records, 'BoxCollider2D')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Gruzzer body collider')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Gruzzer rigid body')
    _, recoil = _one(records, 'Recoil')
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 15 or not _near(recoil['recoilDuration'], .15) or recoil['preventRecoilUp']:
        raise ValueError('unsupported Gruzzer recoil variant')
    _, animator = _one(records, 'tk2dSpriteAnimator')
    library = source.read(source.ref(sc.file, animator['library']))
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Gruzzer animation: ' + name)
    bounds = axis_aligned_bounds([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]],
                                 list(BODY_OFFSET), list(BODY_SIZE))
    return {'kind': 'Gruzzer', 'guest_enabled': True, 'speed': 5.2, 'body_bounds_local': bounds,
            'limitations': ['Gravity-free dynamic body on the bounded terrain solver; bonk sides come from the blocked solver axis instead of the three 0.08 contact rays.',
                            'The live buzz loop audio is not presented; SetZ depth randomization and FSMActivator staggering are ignored.',
                            'The breaker corpse smashes on its third landing without break pieces and does not spin.']}
