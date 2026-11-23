"""Recognize the Greenpath Moss Charger for guest admission.

The controller is shared/hk-sim/src/moss_charger.rs; this admits only a placement
whose serialized shape matches the one that was read to write it (docs/MOSS_CHARGER.md).
Seven are placed in the admitted scenes: four in Fungus1_10 and three in Fungus1_21,
every one with the same `Mossy Control` digest (one of Fungus1_10's also carries the
two-state `FSM` whose `GRIMMKIN SPAWN` deactivates it, which only the Grimm Troupe
sends).

Like the Plant Trap it has no collider of its own: tk2d builds one from the sprite
definition of the frame showing (`tk2dBaseSprite.UpdateCollider`), so the boxes the
controller holds are read off the sprite definitions here and proved against the
constants in `moss_charger.rs`. The digest pins every action byte; the numbers below
say what the controller's constants are.
"""
import hashlib

from actors import _component_records
from false_knight import fsm_digest
from focus import action_fields
from runner import ASSEMBLIES, _one

CONTROL_FSM = 'Mossy Control'
CONTROL_SHA256 = '7fdfe254bf59e1ffcb2b5c4cb41e110e9f99b5cc386369467b861b05f41aac47'
# `GRIMMKIN SPAWN` -> `Deactivate`: never sent on the port's saves.
GRIMM_FSM = 'FSM'
GRIMM_SHA256 = 'accf20b1e4481f57dc2fea1d261a70ac1862b851b53a2149d4c6e84fa5d105e5'
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
# Pinned scalars the controller carries: (state, action, nth enabled) -> {field: value}.
ACTIONS = {
    ('Emerge Pause', 'WaitRandom', 0): {'timeMin': .5, 'timeMax': 1.},
    ('Emerge Right', 'RandomFloat', 0): {'min': 14., 'max': 14.},
    ('Emerge Left', 'RandomFloat', 0): {'min': 14., 'max': 14.},
    ('Emerge', 'FloatMultiply', 0): {'multiplyBy': .25},
    ('Submerge', 'Decelerate', 0): {'deceleration': .7},
    ('Submerge Grass effect', 'Decelerate', 0): {'deceleration': .7},
    ('Submerge CD', 'Wait', 0): {'time': .35},
    ('Line Loop', 'IntCompare', 0): {'integer2': 11},
    ('Fly Left', 'SetVelocityAsAngle', 0): {'angle': 110., 'speed': 18.},
    ('Fly Right', 'SetVelocityAsAngle', 0): {'angle': 70., 'speed': 18.},
    ('FlyUp', 'SetVelocityAsAngle', 0): {'angle': 90., 'speed': 20.},
    ('Fly Down', 'SetVelocityAsAngle', 0): {'angle': 270., 'speed': 10.},
    ('Burst', 'SetGravity2dScale', 0): {'gravityScale': 1.5},
    ('Run L', 'AccelerateVelocity', 0): {'xAccel': -.5, 'xMaxSpeed': 10.},
    ('Run R', 'AccelerateVelocity', 0): {'xAccel': .5, 'xMaxSpeed': 10.},
    ('Run L', 'Wait', 0): {'time': 1.}, ('Run R', 'Wait', 0): {'time': 1.},
    ('Dig Start', 'Decelerate', 0): {'deceleration': .4},
    ('Charge', 'RayCast2d', 0): {'distance': 5.5, 'repeatInterval': 2},
    ('Charge', 'RayCast2d', 1): {'distance': 3., 'repeatInterval': 2},
    ('Run L', 'RayCast2d', 0): {'distance': 2., 'repeatInterval': 1},
    ('Run L', 'RayCast2d', 1): {'distance': 1.3, 'repeatInterval': 1},
    ('Run R', 'RayCast2d', 0): {'distance': 2., 'repeatInterval': 1},
    ('Run R', 'RayCast2d', 1): {'distance': 1.3, 'repeatInterval': 1},
    ('Dig Start', 'RayCast2dV2', 0): {'distance': 1.},
    ('Dig', 'RayCast2dV2', 0): {'distance': 1.},
    ('Init Pause', 'Wait', 0): {'time': .5},
}
CHARGE_SPEED = 15.
RANGE_MARGIN = 2.
RANGE_SCALE_X = 1.175


def _near(a, b, tolerance=1e-5):
    return abs(float(a) - float(b)) <= tolerance


def _q16(value):
    return round(value * 65536)


def _state(fsm, name):
    return next(st for st in fsm['states'] if st['name'] == name)


def _pins(control):
    for (name, action, nth), expected in ACTIONS.items():
        data = _state(control, name)['actionData']
        found = [i for i, n in enumerate(data['actionNames']) if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]]
        if len(found) <= nth:
            raise ValueError(f'Moss Charger {name} no longer carries {action} #{nth}')
        fields = action_fields(data, found[nth])
        for key, want in expected.items():
            value = fields.get(key)
            value = value.get('value') if isinstance(value, dict) and not value.get('useVariable') else None
            if value is None or not _near(value, want):
                raise ValueError(f'unsupported Moss Charger parameter: {name}/{action}.{key}')
    speeds = {v['name']: v['value'] for v in control['variables']['floatVariables']}
    if not _near(speeds.get('Charge Speed', 0), CHARGE_SPEED):
        raise ValueError('unsupported Moss Charger Charge Speed')


def _fsms(records):
    fsms = [data for _, typ, data in records if typ == 'PlayMakerFSM']
    found = {data['fsm']['name']: data for data in fsms}
    extra = found.pop(GRIMM_FSM, None)
    if list(found) != [CONTROL_FSM] or len(fsms) != 1 + (extra is not None):
        raise ValueError('no single Moss Charger Mossy Control FSM')
    if extra is not None and fsm_digest(extra['fsm']) != GRIMM_SHA256:
        raise ValueError('unverified Moss Charger extra FSM')
    control = found[CONTROL_FSM]
    if not control['m_Enabled'] or fsm_digest(control['fsm']) != CONTROL_SHA256:
        raise ValueError('Moss Charger FSM changed: ' + fsm_digest(control['fsm']))
    return control['fsm']


def _children(sc, gid):
    out = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        tid = child['m_PathID']
        kid = sc.transforms[tid]['m_GameObject']['m_PathID']
        out[sc.gos[kid]['m_Name']] = (kid, tid)
    return out


def _range_box(sc, gid, origin):
    """`Attack Range`: the trigger box the Knight must stand in, left at the tuft by `SetParent`,
    and its reach (`X Min`/`X Max` are the tuft's x less/plus half its bounds width, 2 inside)."""
    children = _children(sc, gid)
    if 'Attack Range' not in children:
        raise ValueError('Moss Charger has no Attack Range')
    kid, tid = children['Attack Range']
    records = _component_records(sc, kid)
    boxes = [d for _, t, d in records if t == 'BoxCollider2D']
    if len(boxes) != 1 or not boxes[0]['m_IsTrigger'] or not boxes[0]['m_Enabled'] \
            or not any(t == 'AlertRange' for _, t, _ in records) or not sc.active(kid):
        raise ValueError('unsupported Moss Charger Attack Range')
    matrix = sc.world(tid)
    if abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6 or not _near(matrix[0][0], RANGE_SCALE_X) \
            or not _near(matrix[1][1], 1.):
        raise ValueError('rotated, mirrored or rescaled Moss Charger Attack Range')
    box = boxes[0]
    cx = matrix[0][3] + matrix[0][0] * box['m_Offset']['x'] - origin[0]
    cy = matrix[1][3] + matrix[1][1] * box['m_Offset']['y'] - origin[1]
    hx, hy = matrix[0][0] * box['m_Size']['x'] / 2, matrix[1][1] * box['m_Size']['y'] / 2
    if hx - RANGE_MARGIN < 4:
        raise ValueError('Moss Charger reach too narrow to emerge')
    return [_q16(cx - hx), _q16(cy - hy), _q16(cx + hx), _q16(cy + hy)], _q16(hx - RANGE_MARGIN)


def _clips(source, sc, animator):
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
        if [i for i, f in enumerate(clip['frames']) if f.get('triggerEvent')] != TRIGGERS.get(name, []):
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
            centre, half = definition['colliderVertices'][0], definition['colliderVertices'][1]
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
    control = _fsms(records)
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
    _pins(control)
    health = actor['health_manager']
    if health['hp'] != 15 or health['smallGeoDrops'] != 8 or not health['invincible'] \
            or not health['preventInvincibleEffect'] or any(health[key] for key in (
                'invincibleFromDirection', 'hasSpecialDeath', 'hasAlternateHitAnimation',
                'damageOverride', 'megaFlingGeo', 'mediumGeoDrops', 'largeGeoDrops')):
        raise ValueError('unsupported Moss Charger HealthManager variant')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or not rigid['m_Simulated'] or rigid['m_Mass'] != 10 \
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
    if any(t == 'BoxCollider2D' or t == 'PolygonCollider2D' for _, t, _ in records):
        raise ValueError('Moss Charger with a serialized body collider')
    _, animator = _one(records, 'tk2dSpriteAnimator')
    library_o = _clips(source, sc, animator)
    range_q16, reach_q16 = _range_box(sc, gid, actor['position'][:2])
    return {
        'kind': 'MossCharger', 'guest_enabled': True, 'range_q16': range_q16, 'reach_q16': reach_q16,
        'fsm_sha256': {CONTROL_FSM: CONTROL_SHA256}, 'library_source': source.sid(library_o),
        # Hidden and harmless in its tuft; the controller switches the box (and so the hurt) on.
        'invincible': True, 'contact_damage': 1, 'no_body_collider': True,
        'art_bindings': dict({'walk': 'Charge', 'turn': 'TurnRun'}, **{slot: clip for slot, clip in CLIP_SLOTS}),
        'limitations': [
            'The hurt box is the collider tk2d builds for the frame showing, read from the sprite definitions.',
            'The grass particles (Range Grass, Grass Burst, Charge Grass, Grass Submerge, Grass Dig, G Explode),'
            ' the hit effect, the camera shakes and the sounds are not presented.'],
    }
