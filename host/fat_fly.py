"""Recognize the Greenpath Fat Fly (Obble) for guest admission.

The controller is shared/hk-sim/src/fat_fly.rs (docs/FAT_FLY.md). Nine are placed in the
admitted scenes, four in Fungus1_19 and five in Fungus1_31, every one carrying the same
`Fatty Fly Attack` and `fat fly bounce` digests, authored disabled and switched on by its
`FSMActivator` (staggered), so the guest seats them running. Two of Fungus1_31's are
mirrored (scale x -1): they face right until the first aim. The shot is the Aspid's own
pooled `Spitter Shot R`, read and checked the way host/aspid.py reads it.
"""
import hashlib

from actors import _component_records
from false_knight import fsm_digest
from focus import action_fields
from runner import ASSEMBLIES, _one

ATTACK_FSM, BOUNCE_FSM = 'Fatty Fly Attack', 'fat fly bounce'
DIGESTS = {ATTACK_FSM: 'b703123e797c8374ffbbb08164a99ef50d1422fccd2693e11c4e271ebe875b82',
           BOUNCE_FSM: '6db0017c02286084264059e396c7b84cbf5523d55bdc5fcb495feb568244e32b'}
COMPONENTS = sorted([
    'AudioSource', 'BoxCollider2D', 'DamageHero', 'EnemyDeathEffects', 'EnemyDreamnailReaction', 'ExtraDamageable',
    'FSMActivator', 'HealthManager', 'InfectedEnemyEffects', 'MeshFilter', 'MeshRenderer', 'PersonalObjectPool',
    'PlayMakerCollisionEnter2D', 'PlayMakerCollisionStay2D', 'PlayMakerFSM', 'PlayMakerFSM', 'PlayMakerFixedUpdate',
    'PlayMakerLateUpdate', 'Recoil', 'Rigidbody2D', 'SetZ', 'SpriteFlash', 'Transform', 'tk2dSprite',
    'tk2dSpriteAnimator'])
# name: (frames, fps, wrapMode), trigger frames
CLIPS = {'Fly': (8, 12., 0, []), 'Attack': (8, 12., 2, [5]), 'Death Air': (2, 12., 2, []), 'Death Land': (1, 30., 0, [])}
SHOT_CLIPS = {'Idle': (4, 20, 0), 'Impact': (6, 20, 2)}
BODY_SIZE = (0.890625, 0.84375)
BODY_OFFSET = (0.0390625, -0.03125)
# Pinned scalars the controller carries: (fsm, state, action, nth enabled) -> {field: value}.
ACTIONS = {
    (BOUNCE_FSM, 'Initialise', 'FloatCompare', 0): {'float2': 25.},
    (ATTACK_FSM, 'Wait', 'WaitRandom', 0): {'timeMin': 2., 'timeMax': 3.},
    (ATTACK_FSM, 'Attack Antic', 'Decelerate', 0): {'deceleration': .1},
    (ATTACK_FSM, 'Attack Antic', 'Wait', 0): {'time': .35},
    (ATTACK_FSM, 'Attack Antic 2', 'Decelerate', 0): {'deceleration': .1},
    (ATTACK_FSM, 'Attack', 'Wait', 0): {'time': .5},
    (ATTACK_FSM, 'CD', 'Wait', 0): {'time': .5},
    (ATTACK_FSM, 'Attack', 'FlingObjectsFromGlobalPool', 0): {'angleMin': 45., 'angleMax': 45.},
    (ATTACK_FSM, 'Attack', 'FlingObjectsFromGlobalPool', 1): {'angleMin': 135., 'angleMax': 135.},
    (ATTACK_FSM, 'Attack', 'FlingObjectsFromGlobalPool', 2): {'angleMin': 225., 'angleMax': 225.},
    (ATTACK_FSM, 'Attack', 'FlingObjectsFromGlobalPool', 3): {'angleMin': 315., 'angleMax': 315.},
    (BOUNCE_FSM, 'Up Right', 'RandomFloat', 0): {'min': 320., 'max': 350.},
    (BOUNCE_FSM, 'Up Left', 'RandomFloat', 0): {'min': 190., 'max': 220.},
    (BOUNCE_FSM, 'Down Right', 'RandomFloat', 0): {'min': 10., 'max': 40.},
    (BOUNCE_FSM, 'Up Left 2', 'RandomFloat', 0): {'min': 140., 'max': 170.},
    (BOUNCE_FSM, 'Right Down', 'RandomFloat', 0): {'min': 190., 'max': 220.},
    (BOUNCE_FSM, 'Down Right 2', 'RandomFloat', 0): {'min': 140., 'max': 170.},
    (BOUNCE_FSM, 'Left Down', 'RandomFloat', 0): {'min': 320., 'max': 350.},
    (BOUNCE_FSM, 'Left Up', 'RandomFloat', 0): {'min': 10., 'max': 40.},
}
VARIABLES = {(BOUNCE_FSM, 'Speed'): 4., (ATTACK_FSM, 'Shot Speed'): 12.}


def _near(a, b, tolerance=1e-5):
    return abs(float(a) - float(b)) <= tolerance


def _pins(fsms):
    for (fsm, name, action, nth), expected in ACTIONS.items():
        st = next(s for s in fsms[fsm]['states'] if s['name'] == name)
        data = st['actionData']
        found = [i for i, n in enumerate(data['actionNames']) if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]]
        if len(found) <= nth:
            raise ValueError(f'Fat Fly {name} no longer carries {action} #{nth}')
        fields = action_fields(data, found[nth])
        for key, want in expected.items():
            value = fields.get(key)
            value = value.get('value') if isinstance(value, dict) and not value.get('useVariable') else None
            if value is None or not _near(value, want):
                raise ValueError(f'unsupported Fat Fly parameter: {name}/{action}.{key}')
    for (fsm, name), want in VARIABLES.items():
        floats = {v['name']: v['value'] for v in fsms[fsm]['variables']['floatVariables']}
        if not _near(floats.get(name, 0), want):
            raise ValueError(f'unsupported Fat Fly {name}')


def _shot(sc, records):
    """The pooled `Spitter Shot R`: gravity 0.05, DamageHero 1, Idle and Impact at the Aspid's timings."""
    source = sc.source
    _, pool = _one(records, 'PersonalObjectPool')
    if len(pool['startupPool']) != 1:
        raise ValueError('unsupported Fat Fly shot pool')
    shot_obj = source.ref(sc.file, pool['startupPool'][0]['prefab'])
    shot_go = source.read(shot_obj)
    if shot_go['m_Name'] != 'Spitter Shot R':
        raise ValueError('unsupported Fat Fly shot prefab')
    parts = {}
    for ref in shot_go['m_Component']:
        component = source.ref(shot_obj.assets_file, ref['component'])
        parts[source.typename(component)] = (component, source.read(component))
    for kind in ('Rigidbody2D', 'BoxCollider2D', 'DamageHero', 'EnemyBullet', 'tk2dSpriteAnimator', 'tk2dSprite', 'Transform'):
        if kind not in parts:
            raise ValueError('unsupported Fat Fly shot component set')
    if not _near(parts['Rigidbody2D'][1]['m_GravityScale'], .05) or parts['DamageHero'][1]['damageDealt'] != 1:
        raise ValueError('unsupported Fat Fly shot body')
    library_o = source.ref(shot_obj.assets_file, parts['tk2dSpriteAnimator'][1]['library'])
    by_name = {c['name']: c for c in source.read(library_o)['clips'] if c['name']}
    for name, (frames, fps, wrap) in SHOT_CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Fat Fly shot animation: ' + name)
    return {'source': source.sid(shot_obj), 'library': source.sid(library_o), 'library_object': library_o,
            'scale': parts['Transform'][1]['m_LocalScale']['x'] * parts['EnemyBullet'][1]['scaleMin']}


def recognize(sc, actor):
    """An Obble: floats at the Knight, bounces off what it touches, throws four diagonals."""
    source = sc.source
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Fat Fly methods require a fresh source audit: ' + name)
    gid = actor['game_object']
    records = _component_records(sc, gid)
    if sorted(kind for _, kind, _ in records) != COMPONENTS:
        raise ValueError('unsupported Fat Fly component set')
    fsms = {d['fsm']['name']: d['fsm'] for _, t, d in records if t == 'PlayMakerFSM'}
    if set(fsms) != set(DIGESTS) or any(fsm_digest(fsms[n]) != d for n, d in DIGESTS.items()):
        raise ValueError('Fat Fly FSMs changed')
    _, activator = _one(records, 'FSMActivator')
    if not activator['m_Enabled']:
        raise ValueError('Fat Fly FSMActivator disabled: its trees would never run')
    if sc.gos[gid]['m_Layer'] != 11:
        raise ValueError('Fat Fly outside the enemy layer')
    matrix = sc.world(sc.go_transform[gid])
    if abs(abs(matrix[0][0]) - 1) > 1e-6 or abs(matrix[1][1] - 1) > 1e-6 or abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6:
        raise ValueError('unsupported Fat Fly rotation or scale')
    _pins(fsms)
    health = actor['health_manager']
    if health['hp'] != 10 or health['smallGeoDrops'] != 4 or health['invincible'] or any(health[key] for key in (
            'invincibleFromDirection', 'hasSpecialDeath', 'hasAlternateHitAnimation', 'damageOverride', 'megaFlingGeo',
            'mediumGeoDrops', 'largeGeoDrops')):
        raise ValueError('unsupported Fat Fly HealthManager variant')
    _, body = _one(records, 'BoxCollider2D')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Fat Fly body collider')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Fat Fly rigid body')
    _, recoil = _one(records, 'Recoil')
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 15 or not _near(recoil['recoilDuration'], .15) or recoil['preventRecoilUp']:
        raise ValueError('unsupported Fat Fly recoil variant')
    _, damage = _one(records, 'DamageHero')
    if damage['damageDealt'] != 1 or damage['hazardType'] != 1 or not damage['m_Enabled']:
        raise ValueError('unsupported Fat Fly contact damage')
    sprite = actor['tk2dSprite']
    if sprite['_color'] != {'r': 1., 'g': 1., 'b': 1., 'a': 1.} or sprite['_scale'] != {'x': 1., 'y': 1., 'z': 1.}:
        raise ValueError('unsupported Fat Fly sprite')
    _, animator = _one(records, 'tk2dSpriteAnimator')
    by_name = {c['name']: c for c in source.read(source.ref(sc.file, animator['library']))['clips'] if c['name']}
    for name, (frames, fps, wrap, triggers) in CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap) \
                or [i for i, f in enumerate(clip['frames']) if f.get('triggerEvent')] != triggers:
            raise ValueError('unsupported Fat Fly animation: ' + name)
    # The art faces left at scale +1; a mirrored placement faces right until its first aim.
    return {'kind': 'FatFly', 'guest_enabled': True, 'start_right': matrix[0][0] < 0,
            'shot': _shot(sc, records),
            'art_bindings': {'walk': 'Fly', 'turn': 'Fly', 'attack': 'Attack'},
            'limitations': ['The wing buzz loop and the attack sound, the shot sounds and its spatter, the corpse spin,'
                            ' steam and flame are not presented.',
                            'A contact side comes from the blocked solver axis (x first), not the Box2D contact normal.']}
