"""Recognize the Greenpath Moss Charger for guest admission.

The controller lives in shared/hk-sim/src/moss_charger.rs; this module admits only
a placement whose serialized shape matches the one that was read to write it
(docs/MOSS_CHARGER.md).

Like the Plant Trap it has no collider of its own: tk2d builds one from the sprite
definition of the frame showing (`tk2dBaseSprite.UpdateCollider`), so the boxes the
controller holds are read off the sprite definitions here and proved against the
constants in `moss_charger.rs`. Everything its FSM does with a number is pinned
below: the structural digest says nothing changed, and these say what the constants
are, including the vectors and enums the digest does not cover.
"""
import hashlib

from actors import _component_records
from fsm_pins import (check_action, check_enums, check_transitions, check_vectors, enabled_actions, state, variables)
from runner import ASSEMBLIES, _one
from fsm_pins import fingerprint as fsm_fingerprint

CONTROL_FSM = 'Mossy Control'
# `GRIMMKIN SPAWN` -> `Deactivate`: the Grimm Troupe clears this one out. One placement carries it; the
# event is broadcast only when the Troupe arrives, which a fresh save never reaches.
GRIMM_FSM = 'FSM'
CONTROL_SHA256 = '9590bed6fcbb65d6a9f3e041f77a090d6881d5833ba29b786df9777db8dd39d6'
GRIMM_SHA256 = '48907c6957a40c0b74b500a9f6231b243bda8825840723ac8cfd7478e252bda3'
COMPONENTS = sorted([
    'AudioSource', 'DamageHero', 'EnemyDeathEffects', 'EnemyDreamnailReaction', 'ExtraDamageable',
    'HealthManager', 'InfectedEnemyEffects', 'MeshFilter', 'MeshRenderer', 'NonBouncer', 'ObjectBounce',
    'PersistentBoolItem', 'PlayMakerFSM', 'PlayMakerFixedUpdate', 'Recoil', 'Rigidbody2D', 'SetZ',
    'SpriteFlash', 'Transform', 'tk2dSprite', 'tk2dSpriteAnimator'])
# name: (frames, fps, wrapMode, loopStart)
CLIPS = {'Appear': (6, 12., 2, 0), 'Charge': (4, 15., 0, 0), 'Disappear': (12, 12., 2, 0),
         'Stun': (3, 12., 0, 0), 'Get Up': (4, 18., 2, 0), 'TurnRun': (6, 12., 1, 2), 'Run': (4, 30., 0, 0),
         'Escape': (14, 12., 2, 0), 'Death Air': (2, 12., 2, 0), 'Grass Burst': (9, 12., 2, 0),
         'Death Land': (2, 12., 2, 0)}
# The cooked clips, in `moss_charger::Clip` order.
CLIP_SLOTS = (('appear', 'Appear'), ('charge', 'Charge'), ('disappear', 'Disappear'), ('stun', 'Stun'),
              ('get_up', 'Get Up'), ('turn_run', 'TurnRun'), ('escape', 'Escape'))
TRIGGERS = {'Disappear': [5], 'Escape': [5]}
# Sprite-definition colliders [x0, y0, x1, y1] in Q16 relative to the body, as moss_charger.rs
# holds them: clip -> frame -> box. Frames not listed define none.
BIG, BIG_LOW, BIG_MID, BIG_HIGH = ([-102400, -125952, 123904, 12288], [-102400, -125952, 123904, -51200],
                                   [-102400, -125952, 123904, 48128], [-102400, -125952, 123904, 62464])
STUN, RUN = [-28672, -43008, 39936, 22528], [-28672, -52224, 39936, 22528]
COLLIDERS = {
    'Appear': {i: BIG for i in range(6)}, 'Charge': {i: BIG for i in range(4)},
    'Disappear': {0: BIG, 1: BIG_LOW, 2: BIG, 3: BIG_MID, 4: BIG_HIGH},
    'Stun': {i: STUN for i in range(3)}, 'Get Up': {i: STUN for i in range(4)},
    'TurnRun': {0: STUN, 1: STUN, 2: RUN, 3: RUN, 4: RUN, 5: RUN}, 'Run': {i: RUN for i in range(4)},
    'Escape': {i: STUN for i in range(6)}, 'Death Air': {0: STUN, 1: STUN}, 'Death Land': {0: STUN},
}
# Pinned scalars: (state, action) -> {field: value | ('var', name)}.
ACTIONS = {
    ('Emerge Pause', 'WaitRandom'): {'timeMin': .5, 'timeMax': 1.},
    ('Emerge', 'FloatMultiply'): {'multiplyBy': .25},
    ('Emerge', 'Tk2dPlayAnimationWithEvents'): {'clipName': 'Appear', 'animationCompleteEvent': 'FINISHED'},
    ('Charge', 'Tk2dPlayAnimation'): {'clipName': 'Charge'},
    ('Submerge', 'Decelerate'): {'deceleration': .7},
    ('Submerge', 'Tk2dPlayAnimationWithEvents'): {'clipName': 'Disappear', 'animationTriggerEvent': 'FINISHED'},
    ('Submerge Grass effect', 'Decelerate'): {'deceleration': .7},
    ('Submerge CD', 'Wait'): {'time': .35},
    ('Line Loop', 'IntCompare'): {'integer2': 11, 'greaterThan': 'LOOP COMPLETE'},
    ('Fly Left', 'SetVelocityAsAngle'): {'angle': 110., 'speed': 18.},
    ('Fly Right', 'SetVelocityAsAngle'): {'angle': 70., 'speed': 18.},
    ('FlyUp', 'SetVelocityAsAngle'): {'angle': 90., 'speed': 20.},
    ('Fly Down', 'SetVelocityAsAngle'): {'angle': 270., 'speed': 10.},
    ('Get Up', 'Tk2dPlayAnimationWithEvents'): {'clipName': 'Get Up', 'animationCompleteEvent': 'FINISHED'},
    ('Run L', 'AccelerateVelocity'): {'xAccel': -.5, 'xMaxSpeed': 10.},
    ('Run R', 'AccelerateVelocity'): {'xAccel': .5, 'xMaxSpeed': 10.},
    ('Run L', 'Wait'): {'time': 1.}, ('Run R', 'Wait'): {'time': 1.},
    ('Dig Start', 'Decelerate'): {'deceleration': .4},
    ('Dig Start', 'Tk2dPlayAnimationWithEvents'): {'clipName': 'Escape', 'animationTriggerEvent': 'FINISHED'},
    ('Dig', 'Tk2dWatchAnimationEvents'): {'animationCompleteEvent': 'FINISHED'},
}
# Vector2 pins: (state, action, nth enabled instance) -> {field: (x, y) | ('var', name)}.
VECTORS = {
    ('Charge', 'RayCast2d', 0): {'fromPosition': (0., -.5), 'direction': ('var', 'RayForward Direction')},
    ('Charge', 'RayCast2d', 1): {'fromPosition': ('var', 'RayDown X'), 'direction': (0., -1.)},
    ('Run L', 'RayCast2d', 0): {'fromPosition': ('var', ''), 'direction': (-1., 0.)},
    ('Run L', 'RayCast2d', 1): {'fromPosition': (-3., 0.), 'direction': (0., -1.)},
    ('Run R', 'RayCast2d', 0): {'fromPosition': ('var', ''), 'direction': (1., 0.)},
    ('Run R', 'RayCast2d', 1): {'fromPosition': (3., 0.), 'direction': (0., -1.)},
}
# `SetVector2XY` in order: (state, nth) -> (variable, x, y). The ray offsets are world offsets ahead of the
# charge, so `Emerge Right` (charging left) and `Emerge Left` mirror each other.
SETTERS = {('Emerge Right', 0): ('RayDown X', -6.5, -.5), ('Emerge Right', 1): ('RayForward Direction', -1., 0.),
           ('Emerge Left', 0): ('RayForward Direction', 1., 0.), ('Emerge Left', 1): ('RayDown X', 6.5, -.5)}
# RayCast2d distances (state, nth) -> metres, and the enums: space Self = 1.
RAY_DISTANCES = {('Charge', 0): 5.5, ('Charge', 1): 3., ('Run L', 0): 2., ('Run L', 1): 1.3,
                 ('Run R', 0): 2., ('Run R', 1): 1.3}
# Init's FloatOperator enum values in order: X Min -= length, X Max += length, X Min += 2, X Max -= 2.
INIT_OPERATIONS = (1, 0, 0, 1)
TRANSITIONS = {
    'Init': [('FINISHED', 'Hidden')], 'Hidden': [('IN RANGE', 'Emerge Pause')],
    'Emerge Pause': [('FINISHED', 'Hero Beyond?')],
    'Hero Beyond?': [('CANCEL', 'Hidden'), ('FINISHED', 'Left or Right?')],
    'Left or Right?': [('LEFT', 'Emerge Left'), ('RIGHT', 'Emerge Right')],
    'Emerge Right': [('FINISHED', 'Emerge'), ('LEFT', 'Pause')], 'Emerge Left': [('FINISHED', 'Emerge'), ('RIGHT', 'Pause 2')],
    'Emerge': [('FINISHED', 'Charge')], 'Charge': [('SUBMERGE', 'Submerge'), ('TAKE DAMAGE', 'Line Loop')],
    'Submerge': [('FINISHED', 'Submerge Grass effect')], 'Submerge Grass effect': [('FINISHED', 'Submerge CD')],
    'Submerge CD': [('FINISHED', 'Play Range')], 'Play Range': [('FINISHED', 'Hidden')],
    'Line Loop': [('FINISHED', 'State 2'), ('LOOP COMPLETE', 'Burst')],
    'Burst': [('LEFT', 'Fly Left'), ('RIGHT', 'Fly Right'), ('UP', 'FlyUp'), ('DOWN', 'Fly Down')],
    'Fly Left': [('FINISHED', 'In Air')], 'Fly Right': [('FINISHED', 'In Air')], 'FlyUp': [('FINISHED', 'In Air')],
    'Fly Down': [('FINISHED', 'In Air')], 'In Air': [('DOWN', 'Land')], 'Land': [('FINISHED', 'Get Up')],
    'Get Up': [('FINISHED', 'Direction')], 'Direction': [('LEFT', 'Run R'), ('RIGHT', 'Run L')],
    'Run L': [('LEFT', 'Run R'), ('SUBMERGE', 'On Ground?')], 'Run R': [('RIGHT', 'Run L'), ('SUBMERGE', 'On Ground?')],
    'On Ground?': [('DOWN', 'Dig Start'), ('FINISHED', 'Dig Start')],
    'Dig Start': [('FINISHED', 'Dig'), ('FALL', 'In Air')], 'Dig': [('FINISHED', 'Submerge CD'), ('FALL', 'In Air')],
}
GLOBALS = [('ZERO HP', 'Detach'), ('BLOCKED HIT', 'Line Loop')]


def _near(a, b, tolerance=1e-6):
    return abs(float(a) - float(b)) <= tolerance


def _q16(value):
    return round(value * 65536)


def _fsms(sc, gid, records):
    fsms = [data['fsm'] for _, typ, data in records if typ == 'PlayMakerFSM']
    found = {fsm['name']: fsm for fsm in fsms}
    extra = found.pop(GRIMM_FSM, None)
    if list(found) != [CONTROL_FSM] or len(fsms) != 1 + (extra is not None):
        raise ValueError('no single Moss Charger Mossy Control FSM')
    if extra is not None and fsm_fingerprint(extra) != GRIMM_SHA256:
        raise ValueError('unverified Moss Charger extra FSM')
    control = found[CONTROL_FSM]
    if control['startState'] != 'Init Pause' or fsm_fingerprint(control) != CONTROL_SHA256:
        raise ValueError('unverified Moss Charger FSM variant')
    check_transitions('Moss Charger', control, TRANSITIONS, GLOBALS)
    return control


def _pins(control):
    for (name, action), expected in ACTIONS.items():
        check_action('Moss Charger', state(control, name), action, expected)
    for (name, action, nth), expected in VECTORS.items():
        if expected:
            check_vectors('Moss Charger', state(control, name), action, nth, expected)
    for (name, nth), (variable, x, y) in SETTERS.items():
        check_vectors('Moss Charger', state(control, name), 'SetVector2XY', nth, {'vector2Variable': ('var', variable)})
        fields = enabled_actions('Moss Charger', state(control, name), 'SetVector2XY')[nth]
        if not _near(fields['x']['value'], x) or not _near(fields['y']['value'], y):
            raise ValueError(f'unsupported Moss Charger parameter: {name}/SetVector2XY #{nth}')
    for (name, nth), distance in RAY_DISTANCES.items():
        data = state(control, name)['actionData']
        found = [i for i, n in enumerate(data['actionNames']) if n.rsplit('.', 1)[-1] == 'RayCast2d' and data['actionEnabled'][i]]
        from focus import action_fields
        fields = action_fields(data, found[nth])
        if not _near(fields['distance']['value'], distance):
            raise ValueError(f'unsupported Moss Charger parameter: {name}/RayCast2d #{nth}.distance')
        check_enums('Moss Charger', state(control, name), 'RayCast2d', nth, {'space': 1})
    ops = state(control, 'Init')['actionData']
    found = [i for i, n in enumerate(ops['actionNames']) if n.rsplit('.', 1)[-1] == 'FloatOperator' and ops['actionEnabled'][i]]
    if len(found) != 4:
        raise ValueError('unsupported Moss Charger Init operators')
    for nth, want in enumerate(INIT_OPERATIONS):
        check_enums('Moss Charger', state(control, 'Init'), 'FloatOperator', nth, {'operation': want})
    plain = variables(control)
    if not _near(plain.get('Charge Speed'), 15.):
        raise ValueError('unsupported Moss Charger Charge Speed')


def _range_box(sc, gid, origin):
    """`Attack Range`: the trigger box the Knight must stand in, detached at the tuft."""
    children = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        tid = child['m_PathID']
        kid = sc.transforms[tid]['m_GameObject']['m_PathID']
        children[sc.gos[kid]['m_Name']] = (kid, tid)
    if 'Attack Range' not in children:
        raise ValueError('Moss Charger has no Attack Range')
    kid, tid = children['Attack Range']
    records = _component_records(sc, kid)
    boxes = [d for _, t, d in records if t == 'BoxCollider2D']
    if sc.gos[kid]['m_Layer'] != 13 or len(boxes) != 1 or not boxes[0]['m_IsTrigger'] or not boxes[0]['m_Enabled'] \
            or not any(t == 'AlertRange' for _, t, _ in records):
        raise ValueError('unsupported Moss Charger Attack Range')
    matrix = sc.world(tid)
    if abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6 or matrix[0][0] <= 0 or matrix[1][1] <= 0:
        raise ValueError('rotated or mirrored Moss Charger Attack Range')
    box = boxes[0]
    cx = matrix[0][3] + matrix[0][0] * box['m_Offset']['x'] - origin[0]
    cy = matrix[1][3] + matrix[1][1] * box['m_Offset']['y'] - origin[1]
    hx, hy = matrix[0][0] * box['m_Size']['x'] / 2, matrix[1][1] * box['m_Size']['y'] / 2
    return [_q16(cx - hx), _q16(cy - hy), _q16(cx + hx), _q16(cy + hy)]


def _clips(source, sc, actor):
    animator = actor['tk2dSpriteAnimator']
    if not animator['m_Enabled'] or animator['isRealtime'] or animator['playAutomatically']:
        raise ValueError('Moss Charger requires enabled scaled-time animation it starts itself')
    library_o = source.ref(sc.file, animator['library'])
    clips = {c['name']: c for c in source.read(library_o)['clips'] if c['name']}
    collections = {}
    for name, (frames, fps, wrap, loop_start) in CLIPS.items():
        clip = clips.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode'], clip.get('loopStart', 0)) \
                != (frames, fps, wrap, loop_start):
            raise ValueError('unsupported Moss Charger animation: ' + name)
        triggers = [i for i, f in enumerate(clip['frames']) if f.get('triggerEvent')]
        if triggers != TRIGGERS.get(name, []):
            raise ValueError('unsupported Moss Charger frame event: ' + name)
        if name == 'Grass Burst':
            continue
        for index, frame in enumerate(clip['frames']):
            collection_o = source.ref(library_o.assets_file, frame['spriteCollection'])
            if collection_o.path_id not in collections:
                collections[collection_o.path_id] = source.read(collection_o)
            definition = collections[collection_o.path_id]['spriteDefinitions'][frame['spriteId']]
            wanted = COLLIDERS.get(name, {}).get(index)
            if definition['physicsEngine'] != 1:
                raise ValueError('Moss Charger sprite is not a 2D physics sprite: ' + name)
            if wanted is None:
                if definition['colliderType'] == 2:
                    raise ValueError(f'unexpected Moss Charger collider: {name} frame {index}')
                continue
            vertices = definition['colliderVertices']
            centre, half = vertices[0], vertices[1]
            actual = [_q16(centre['x'] - half['x']), _q16(centre['y'] - half['y']),
                      _q16(centre['x'] + half['x']), _q16(centre['y'] + half['y'])]
            if definition['colliderType'] != 2 or actual != wanted:
                raise ValueError(f'Moss Charger collider {name} frame {index} {actual} is not the admitted {wanted}')
    return library_o


def recognize(sc, actor):
    """A tuft that surfaces beside the Knight and charges across the room."""
    source = sc.source
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Moss Charger methods require a fresh source audit: ' + name)
    gid = actor['game_object']
    records = _component_records(sc, gid)
    control = _fsms(sc, gid, records)
    kinds = sorted(kind for _, kind, _ in records)
    if kinds.count('PlayMakerFSM') == 2:
        kinds.remove('PlayMakerFSM')
    if kinds != COMPONENTS:
        raise ValueError('unsupported Moss Charger component set')
    if sc.gos[gid]['m_Layer'] != 11:
        raise ValueError(f'Moss Charger outside the enemy layer: layer {sc.gos[gid]["m_Layer"]}')
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('unsupported Moss Charger rotation or scale')
    if len(actor['position']) < 3 or abs(actor['position'][2]) > .01:
        raise ValueError('Moss Charger depth differs from guest source plane')
    _pins(control)
    health = actor['health_manager']
    if health['hp'] != 15 or health['smallGeoDrops'] != 8 or not health['invincible'] \
            or not health['preventInvincibleEffect'] or any(health[key] for key in (
                'invincibleFromDirection', 'hasSpecialDeath', 'hasAlternateHitAnimation',
                'damageOverride', 'megaFlingGeo', 'mediumGeoDrops', 'largeGeoDrops')):
        raise ValueError('unsupported Moss Charger HealthManager variant')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or not rigid['m_Simulated'] or rigid['m_UseAutoMass'] or rigid['m_Mass'] != 10 \
            or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Moss Charger rigid body')
    _, recoil = _one(records, 'Recoil')
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 15 or not _near(recoil['recoilDuration'], .15) \
            or not recoil['preventRecoilUp'] or recoil['stopVelocityXWhenRecoilingUp']:
        raise ValueError('unsupported Moss Charger recoil variant')
    _, damage = _one(records, 'DamageHero')
    if damage['damageDealt'] != 1 or damage['hazardType'] != 1 or not damage['m_Enabled']:
        raise ValueError('unsupported Moss Charger contact damage')
    sprite = actor['tk2dSprite']
    if sprite['_color'] != {'r': 1., 'g': 1., 'b': 1., 'a': 1.} or sprite['_scale'] != {'x': 1., 'y': 1., 'z': 1.} \
            or sprite['boxCollider2D']['m_PathID'] or sprite['polygonCollider2D']:
        raise ValueError('unsupported Moss Charger sprite')
    library_o = _clips(source, sc, actor)
    range_q16 = _range_box(sc, gid, actor['position'][:2])
    return {
        'kind': 'MossCharger', 'guest_enabled': True, 'bounds_q16': BIG, 'range_q16': range_q16,
        'fsm_sha256': {CONTROL_FSM: CONTROL_SHA256}, 'assemblies_sha256': dict(ASSEMBLIES),
        'library_source': source.sid(library_o),
        'art_bindings': dict({'walk': 'Appear', 'turn': 'Appear'}, **{slot: clip for slot, clip in CLIP_SLOTS}),
        'limitations': [
            'The hurt box is the collider tk2d builds for the frame showing, read from the sprite'
            ' definitions; the charge is a kinematic slide on its ground line and the burst and the run'
            ' use the bounded gravity body.',
            'The Dig Check child, grass puffs, hit effects, camera shake and every sound are not presented.'],
    }
