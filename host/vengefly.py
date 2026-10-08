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


# --- Acid Flyer (Duranda, Fungus1_09) ----------------------------------------
#
# Not a Vengefly, but the nearest flyer: no AI, a pogo platform over the acid.
# The controller is shared/hk-sim/src/acid_flyer.rs. Every number below is
# read back out of the placement and checked, so a placement whose FSMs or
# boxes moved is refused rather than seated wrong:
#
# * `Tween`: wait 0.5 s, iTweenMoveBy `Move Vector` at `Speed` (easeInOutSine,
#   world space), then back, then SetPosition to the start. Three placements
#   carry a second `Tween` without the Wait; its tween is disposed when the
#   first one's starts (iTween.ConflictCheck), so it only leads.
# * `Acid Flyer`: FaceObject at the hero every frame with TurnToFly on a flip.
#   `Bounce Anim` waits for BLOCKED DOWN, which nothing sends (the root's
#   HealthManager has preventInvincibleEffect), so it is not modelled.
# * HealthManager: invincible from up and down (`invincibleFromDirection` 7),
#   tagged `Spell Vulnerable`, so side slashes and spells damage.
# * The `Shell` child unparents itself on its second frame and follows the
#   body unmirrored: a larger contact box with a TinkEffect, never a damage
#   target (HitTaker finds no HealthManager above a root).
ACID_FLYER_CLIPS = {'Fly': (6, 12, 0, 0), 'TurnToFly': (9, 12, 1, 3)}
ACID_FLYER_BODY = ((0.8472197651863098, 1.14892578125), (-0.51702880859375, -0.44126415252685547))
ACID_FLYER_SHELL = ((1.7023437023162842, 1.7226080894470215), (-0.0997161865234375, -0.13818359375))
ACID_FLYER_TWEEN = {'Init': [('FINISHED', 'Tween Up')], 'Tween Up': [('FINISHED', 'Tween Down')],
                    'Tween Down': [('FINISHED', 'Reset Pos')], 'Reset Pos': [('FINISHED', 'Tween Up')]}
ACID_FLYER_TWEEN_ACTIONS = {'Init': ['SetVector3Value', 'Vector3Multiply', 'GetPosition'], 'Tween Up': ['iTweenMoveBy'],
                            'Tween Down': ['iTweenMoveBy'], 'Reset Pos': ['SetPosition']}
ACID_FLYER_CONTROL = {'Init': [('FINISHED', 'Idle')], 'Idle': [('BLOCKED DOWN', 'Bounce Anim')],
                      'Bounce Anim': [('FINISHED', 'Idle')]}
EASE_IN_OUT_SINE = 14
# `Spell Vulnerable`: user tag 66 in TagManager, serialized as 20000 + 66
# (the same encoding as hatcher.py's CAGE_TAG).
SPELL_VULNERABLE_TAG = 20066


def _enabled_actions(state):
    data = state['actionData']
    return [(i, n.rsplit('.', 1)[-1]) for i, n in enumerate(data['actionNames']) if data['actionEnabled'][i]]


def _raw_int(state, index, name):
    data = state['actionData']
    start = data['actionStartIndex'][index]
    end = data['actionStartIndex'][index + 1] if index + 1 < len(data['actionNames']) else len(data['paramName'])
    for i in range(start, end):
        if data['paramName'][i] == name:
            pos = data['paramDataPos'][i]
            return int.from_bytes(bytes(data['byteData'][pos:pos + 4]), 'little', signed=True)
    raise ValueError(f'Acid Flyer {state["name"]} action lacks {name}')


def _literal(value):
    """A compact scalar's literal; a variable slot left as PlayMaker None
    (useVariable with no name) keeps the literal too."""
    if isinstance(value, dict):
        if value.get('useVariable') and value.get('name'):
            raise ValueError('Acid Flyer action reads a variable where a literal was audited: ' + value['name'])
        return value['value']
    return value


def _acid_flyer_tween(fsm):
    """(Move Vector y, Speed, waits) of one `Tween` FSM, or a refusal."""
    from focus import fsm_variables
    if fsm['startState'] != 'Init':
        raise ValueError('Acid Flyer Tween starts elsewhere')
    states = {st['name']: st for st in fsm['states']}
    if set(states) != set(ACID_FLYER_TWEEN):
        raise ValueError('unsupported Acid Flyer Tween states')
    for name, transitions in ACID_FLYER_TWEEN.items():
        if [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != transitions:
            raise ValueError('unsupported Acid Flyer Tween transitions: ' + name)
    init = [n for _, n in _enabled_actions(states['Init'])]
    waits = init == ACID_FLYER_TWEEN_ACTIONS['Init'] + ['Wait']
    if not waits and init != ACID_FLYER_TWEEN_ACTIONS['Init']:
        raise ValueError('unsupported Acid Flyer Tween Init')
    if waits and not _near(_literal(action_fields(states['Init']['actionData'], 3)['time']), .5):
        raise ValueError('unsupported Acid Flyer Tween wait')
    multiply = action_fields(states['Init']['actionData'], 1)
    if not _near(_literal(multiply['multiplyBy']), -1):
        raise ValueError('unsupported Acid Flyer inverse vector')
    for name in ('Tween Up', 'Tween Down', 'Reset Pos'):
        if [n for _, n in _enabled_actions(states[name])] != ACID_FLYER_TWEEN_ACTIONS[name]:
            raise ValueError('unsupported Acid Flyer Tween actions: ' + name)
    for name in ('Tween Up', 'Tween Down'):
        state = states[name]
        fields = action_fields(state['actionData'], 0)
        if (fields['speed'].get('useVariable'), fields['speed'].get('name')) != (True, 'Speed') \
                or _literal(fields['time']) != 0 or _literal(fields['delay']) != 0 or fields['finishEvent'] != 'FINISHED' \
                or not _literal(fields['stopOnExit']) or not _literal(fields['loopDontFinish']) or _literal(fields['orientToPath']) \
                or (_raw_int(state, 0, 'easeType'), _raw_int(state, 0, 'loopType'), _raw_int(state, 0, 'space')) \
                != (EASE_IN_OUT_SINE, 0, 0):
            raise ValueError('unsupported Acid Flyer iTweenMoveBy: ' + name)
    variables = fsm_variables(fsm)
    vector, speed = variables.get('Move Vector'), variables.get('Speed')
    if not isinstance(vector, dict) or vector.get('x') != 0 or vector.get('z') != 0 or not vector.get('y') \
            or not isinstance(speed, float) or speed <= 0:
        raise ValueError('unsupported Acid Flyer Move Vector or Speed')
    return vector['y'], speed, waits


def recognize_acid_flyer(sc, actor):
    """Admit a placed Acid Flyer, or refuse with the reason."""
    from actors import _component_records
    source = sc.source
    gid = actor['game_object']
    records = _component_records(sc, gid)
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Acid Flyer methods require a fresh source audit: ' + name)
    if sc.gos[gid]['m_Layer'] != 11 or sc.gos[gid].get('m_Tag') != SPELL_VULNERABLE_TAG:
        raise ValueError('Acid Flyer is not a Spell Vulnerable enemy')
    fsms = [d['fsm'] for _, t, d in records if t == 'PlayMakerFSM' and d['m_Enabled']]
    control = [f for f in fsms if f['name'] == 'Acid Flyer']
    tweens = [f for f in fsms if f['name'] == 'Tween']
    if len(control) != 1 or len(fsms) != 1 + len(tweens) or not 1 <= len(tweens) <= 2:
        raise ValueError('unsupported Acid Flyer FSM set')
    states = {st['name']: st for st in control[0]['states']}
    if control[0]['startState'] != 'Init' or set(states) != set(ACID_FLYER_CONTROL):
        raise ValueError('unsupported Acid Flyer control states')
    for name, transitions in ACID_FLYER_CONTROL.items():
        if [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != transitions:
            raise ValueError('unsupported Acid Flyer control transitions: ' + name)
    if [n for _, n in _enabled_actions(states['Idle'])] != ['FaceObject', 'Tk2dPlayAnimation']:
        raise ValueError('unsupported Acid Flyer Idle actions')
    face = action_fields(states['Idle']['actionData'], 0)
    play = action_fields(states['Idle']['actionData'], 1)
    if _literal(face['spriteFacesRight']) or not _literal(face['playNewAnimation']) or _literal(face['newAnimationClip']) != 'TurnToFly' \
            or not _literal(face['resetFrame']) or not _literal(face['everyFrame']) or _literal(play['clipName']) != 'Fly':
        raise ValueError('unsupported Acid Flyer facing')
    leads = [t for t in map(_acid_flyer_tween, tweens) if not t[2]]
    mains = [t for t in map(_acid_flyer_tween, tweens) if t[2]]
    if len(mains) != 1 or len(leads) > 1:
        raise ValueError('Acid Flyer needs one waiting Tween and at most one lead')
    amount, speed, _ = mains[0]
    lead = [round(leads[0][0] * 65536), round(leads[0][1] * 65536)] if leads else [0, 0]
    health = actor['health_manager']
    if health['hp'] != 30 or not health['invincible'] or health['invincibleFromDirection'] != 7 \
            or not health['preventInvincibleEffect'] or health['hasSpecialDeath'] or health['hasAlternateHitAnimation'] \
            or health['damageOverride'] or not _near(health['invulnerableTime'], .25):
        raise ValueError('unsupported Acid Flyer HealthManager')
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('unsupported Acid Flyer initial rotation or scale')
    (size, offset) = ACID_FLYER_BODY
    body = body_box(records, 'Acid Flyer')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', size)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', offset)):
        raise ValueError('unsupported Acid Flyer body collider')
    if not any(t == 'BigBouncer' and d['m_Enabled'] for _, t, d in records):
        raise ValueError('Acid Flyer without BigBouncer')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or rigid['m_GravityScale'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Acid Flyer rigid body')
    _, recoil = _one(records, 'Recoil')
    if recoil['recoilSpeedBase'] != 0 or recoil['freezeInPlace']:
        raise ValueError('unsupported Acid Flyer recoil')
    tid = sc.go_transform[gid]
    shells = [t['m_GameObject']['m_PathID'] for t in sc.transforms.values()
              if t['m_Father']['m_PathID'] == tid and sc.gos[t['m_GameObject']['m_PathID']]['m_Name'] == 'Shell']
    if len(shells) != 1 or not sc.active(shells[0]):
        raise ValueError('Acid Flyer without one active Shell')
    shell_records = _component_records(sc, shells[0])
    shell_transform = sc.transforms[sc.go_transform[shells[0]]]
    shell_fsms = sorted(d['fsm']['name'] for _, t, d in shell_records if t == 'PlayMakerFSM' and d['m_Enabled'])
    _, shell_box = _one(shell_records, 'BoxCollider2D')
    _, shell_damage = _one(shell_records, 'DamageHero')
    _, tink = _one(shell_records, 'TinkEffect')
    (size, offset) = ACID_FLYER_SHELL
    if shell_fsms != ['Block Bounce', 'Destroy if parent null', 'FSM'] or sc.gos[shells[0]]['m_Layer'] != 11 \
            or shell_transform['m_LocalPosition'] != {'x': 0., 'y': 0., 'z': 0.} \
            or shell_transform['m_LocalScale'] != {'x': 1., 'y': 1., 'z': 1.} \
            or not shell_box['m_Enabled'] or shell_box['m_IsTrigger'] \
            or not all(_near(shell_box['m_Size'][k], v) for k, v in zip('xy', size)) \
            or not all(_near(shell_box['m_Offset'][k], v) for k, v in zip('xy', offset)) \
            or shell_damage['damageDealt'] != 1 or tink['useNailPosition'] or tink['sendFSMEvent'] \
            or any(t in ('HealthManager', 'BigBouncer', 'NonBouncer') for _, t, _ in shell_records):
        raise ValueError('unsupported Acid Flyer Shell')
    _, animator = _one(records, 'tk2dSpriteAnimator')
    library = source.read(source.ref(sc.file, animator['library']))
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap, loop_start) in ACID_FLYER_CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode'], clip.get('loopStart', 0)) != (frames, fps, wrap, loop_start):
            raise ValueError('unsupported Acid Flyer animation: ' + name)
    shell = axis_aligned_bounds([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]], list(offset), list(size))
    return {'kind': 'AcidFlyer', 'guest_enabled': True,
            'amount': round(amount * 65536), 'speed': round(speed * 65536), 'lead': lead,
            'shell': [round(v * 65536) for v in shell],
            # The live direction test replaces the serialized flag; the
            # generator would otherwise refuse invincibleFromDirection.
            'invincible': False, 'invincible_from_direction': 7,
            'art_bindings': {'walk': 'Fly', 'turn': 'TurnToFly'},
            'limitations': ['The tween is a fixed 60 Hz sample of iTween easeInOutSine; the disposed lead tween is'
                            ' modelled from its curve, not from a running second tween.',
                            'The fly loop audio, the Shell\'s Block Hit v2 effect and corpse steam are not presented.',
                            'BounceHigh is taken when a down-slash reaches the body box; the source takes whichever'
                            ' of the body and the Shell its trigger callbacks report first.']}


# --- Mosquito (Squit, Greenpath) ----------------------------------------------
#
# The controller is shared/hk-sim/src/mosquito.rs. Its numbers were read out
# of `Mozzie` and its custom actions' IL; both FSMs are identical on all 14
# placements, so they are pinned by digest (host/false_knight.py fsm_digest)
# and any change refuses the placement instead of seating stale numbers.
MOSQUITO_FSM_SHA256 = {
    'Mozzie': 'd5c5dbe9844f54dd6897e07f2f3bf9c4cee04520c0f72aae93a47bd887cf6a0f',
    # `GO UP/LEFT/RIGHT/DOWN` mover; nothing in the five scenes sends to it.
    'FSM': '6118e236bc3a678dd700800def0c2554cf0d1e501f7ea825b5198c5ff15f75ae',
}
MOSQUITO_CLIPS = {'Idle': (8, 10, 0, 0), 'TurnToIdle': (10, 12, 1, 2), 'Startle': (4, 12, 2, 0),
                  'Attack Antic': (6, 10, 2, 0), 'Attack': (3, 12, 0, 0), 'Death Air': (3, 12, 2, 0)}
# Slot order of `ActorController::Mosquito::clips`, after walk (Idle) and turn
# (TurnToIdle): mosquito::Clip from Startle on.
MOSQUITO_SLOTS = (('startle', 'Startle'), ('antic', 'Attack Antic'), ('attack', 'Attack'), ('pull_out', 'Death Air'))
MOSQUITO_BODY = ((1.40625, 0.265625), (-0.453125, -0.0703125))
MOSQUITO_ALERT_RADIUS = 0.41109946370124817 * 21.115947723388672
# `TileDetector`: a second solid box (terrain and nail), until `Attack Antic`.
MOSQUITO_TILE = ((0.6755398511886597, 0.965610146522522), (0.0, 0.01719517633318901), (-0.15, -0.16), (1.33, 1.47628915309906))


def recognize_mosquito(sc, actor):
    """Admit a placed Mosquito, or refuse with the reason."""
    from actors import _component_records
    from false_knight import fsm_digest
    source = sc.source
    gid = actor['game_object']
    records = _component_records(sc, gid)
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Mosquito methods require a fresh source audit: ' + name)
    fsms = {d['fsm']['name']: d['fsm'] for _, t, d in records if t == 'PlayMakerFSM' and d['m_Enabled']}
    if set(fsms) != set(MOSQUITO_FSM_SHA256):
        raise ValueError('unsupported Mosquito FSM set: ' + ', '.join(sorted(fsms)))
    for name, digest in MOSQUITO_FSM_SHA256.items():
        if fsm_digest(fsms[name]) != digest:
            raise ValueError(f'Mosquito FSM {name} changed: {fsm_digest(fsms[name])}')
    health = actor['health_manager']
    if health['hp'] != 10 or health['invincible'] or health['invincibleFromDirection'] or health['hasSpecialDeath'] \
            or health['hasAlternateHitAnimation'] or health['damageOverride']:
        raise ValueError('unsupported Mosquito HealthManager')
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('unsupported Mosquito initial rotation or scale')
    (size, offset) = MOSQUITO_BODY
    body = body_box(records, 'Mosquito')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', size)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', offset)):
        raise ValueError('unsupported Mosquito body collider')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Mosquito rigid body')
    _, recoil = _one(records, 'Recoil')
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 20 or not _near(recoil['recoilDuration'], .15) or recoil['preventRecoilUp']:
        raise ValueError('unsupported Mosquito recoil')
    _, sight = _one(records, 'LineOfSightDetector')
    if not sight['m_Enabled'] or len(sight['alertRanges']) != 1:
        raise ValueError('unsupported Mosquito line of sight detector')
    tid = sc.go_transform[gid]
    children = {sc.gos[t['m_GameObject']['m_PathID']]['m_Name']: t['m_GameObject']['m_PathID']
                for t in sc.transforms.values() if t['m_Father']['m_PathID'] == tid}
    if set(children) != {'TileDetector', 'Thunk Effect', 'Alert Range New'}:
        raise ValueError('unsupported Mosquito children')
    alert_gid = sc.objects[sight['alertRanges'][0]['m_PathID']][1]['m_GameObject']['m_PathID']
    if alert_gid != children['Alert Range New']:
        raise ValueError('Mosquito sight reads another alert range')
    _, circle = _one(_component_records(sc, alert_gid), 'CircleCollider2D')
    alert = sc.transforms[sc.go_transform[alert_gid]]
    if not circle['m_IsTrigger'] or circle['m_Offset'] != {'x': 0., 'y': 0.} or alert['m_LocalPosition'] != {'x': 0., 'y': 0., 'z': 0.} \
            or not _near(circle['m_Radius'] * max(alert['m_LocalScale']['x'], alert['m_LocalScale']['y']), MOSQUITO_ALERT_RADIUS):
        raise ValueError('unsupported Mosquito alert range')
    tile_gid = children['TileDetector']
    _, tile_box = _one(_component_records(sc, tile_gid), 'BoxCollider2D')
    tile = sc.transforms[sc.go_transform[tile_gid]]
    (tsize, toffset, tpos, tscale) = MOSQUITO_TILE
    if not sc.active(tile_gid) or tile_box['m_IsTrigger'] or sc.gos[tile_gid]['m_Layer'] != 11 \
            or not all(_near(tile_box['m_Size'][k], v) for k, v in zip('xy', tsize)) \
            or not all(abs(tile_box['m_Offset'][k] - v) < 1e-5 for k, v in zip('xy', toffset)) \
            or not all(_near(tile['m_LocalPosition'][k], v) for k, v in zip('xy', tpos)) \
            or not all(_near(tile['m_LocalScale'][k], v) for k, v in zip('xy', tscale)):
        raise ValueError('unsupported Mosquito TileDetector')
    _, animator = _one(records, 'tk2dSpriteAnimator')
    library = source.read(source.ref(sc.file, animator['library']))
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap, loop_start) in MOSQUITO_CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode'], clip.get('loopStart', 0)) != (frames, fps, wrap, loop_start):
            raise ValueError('unsupported Mosquito animation: ' + name)
    identity = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]
    tile_bounds = axis_aligned_bounds(identity, [tpos[0] + toffset[0] * tscale[0], tpos[1] + toffset[1] * tscale[1]],
                                      [tsize[0] * tscale[0], tsize[1] * tscale[1]])
    return {'kind': 'Mosquito', 'guest_enabled': True, 'alert_radius': MOSQUITO_ALERT_RADIUS,
            'tile': [round(v * 65536) for v in tile_bounds],
            'art_bindings': dict({'walk': 'Idle', 'turn': 'TurnToIdle'}, **dict(MOSQUITO_SLOTS)),
            'limitations': ['The body box stays axis aligned through the lunge; the source rotates it with the sprite.',
                            'The fly loop, Impact Lines and Thunk Effect are not presented.',
                            'The inert `FSM` mover (GO UP/LEFT/RIGHT/DOWN, no sender in any of its scenes) is not run.']}
