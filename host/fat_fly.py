"""Recognize the Greenpath Fat Fly for guest admission.

The controller lives in shared/hk-sim/src/fat_fly.rs; this module admits only a
placement whose serialized shape matches the one that was read to write it
(docs/FAT_FLY.md).

A Fat Fly is a Gruzzer's bounce with an attack bolted on. `fat fly bounce` is
`Bouncer Control` woken by the Knight coming within 25 units instead of by the
camera, first aimed at him rather than anywhere, and slower (4 against 5.2);
`Fatty Fly Attack` waits two to three seconds (or a hit) and then slows it to a
halt, plays `Attack` and flings four `Spitter Shot R`, the Aspid's own bullet,
out on the diagonals. Both FSMs are pinned by structural digest, so a retuned
placement stops being this family rather than running against this controller,
and the numbers the controller holds are proven below against the source.
"""
import hashlib

from actors import _component_records
from fsm_pins import check_action, check_transitions, enabled_actions, state, variables
from runner import ASSEMBLIES, _one, axis_aligned_bounds, fsm_fingerprint

BOUNCE_FSM = 'fat fly bounce'
ATTACK_FSM = 'Fatty Fly Attack'
BOUNCE_SHA256 = 'dc13fb09efd63d26cd2da8e309eb2beea04febe322bac69fa6bca2b2c73011fa'
ATTACK_SHA256 = 'f3f18d310f8661b2997a7c3aa8c85276be8f721cccf822a953d3723115df3279'
COMPONENTS = sorted([
    'AudioSource', 'BoxCollider2D', 'DamageHero', 'EnemyDeathEffects', 'EnemyDreamnailReaction',
    'ExtraDamageable', 'FSMActivator', 'HealthManager', 'InfectedEnemyEffects', 'MeshFilter',
    'MeshRenderer', 'PersonalObjectPool', 'PlayMakerCollisionEnter2D', 'PlayMakerCollisionStay2D',
    'PlayMakerFSM', 'PlayMakerFSM', 'PlayMakerFixedUpdate', 'PlayMakerLateUpdate', 'Recoil',
    'Rigidbody2D', 'SetZ', 'SpriteFlash', 'Transform', 'tk2dSprite', 'tk2dSpriteAnimator'])
BODY_SIZE = (0.890625, 0.84375)
BODY_OFFSET = (0.0390625, -0.03125)
# name: (frames, fps, wrapMode). `Attack` fires from its sixth frame.
CLIPS = {'Fly': (8, 12., 0), 'Attack': (8, 12., 2), 'Death Air': (2, 12., 2), 'Death Land': (1, 30., 0)}
ATTACK_TRIGGER_FRAME = 5
SHOT_CLIPS = {'Idle': (4, 20., 0), 'Impact': (6, 20., 2)}
# The authored numbers shared/hk-sim/src/fat_fly.rs holds, read back rather than trusted:
# (FSM, state, action) -> {field: value}.
ACTIONS = {
    (BOUNCE_FSM, 'Initialise', 'FloatCompare'): {'float2': 25., 'lessThan': 'FINISHED'},
    (BOUNCE_FSM, 'Fly 2', 'SetVelocityAsAngle'): {'speed': ('var', 'Speed')},
    (ATTACK_FSM, 'Wait', 'WaitRandom'): {'timeMin': 2., 'timeMax': 3.},
    (ATTACK_FSM, 'Attack Antic', 'Wait'): {'time': .35},
    (ATTACK_FSM, 'Attack Antic', 'Decelerate'): {'deceleration': .1},
    (ATTACK_FSM, 'Attack Antic 2', 'Decelerate'): {'deceleration': .1},
    (ATTACK_FSM, 'Attack Antic 2', 'Tk2dPlayAnimationWithEvents'): {
        'clipName': 'Attack', 'animationTriggerEvent': 'FINISHED', 'animationCompleteEvent': 'FINISHED'},
    (ATTACK_FSM, 'Attack', 'Wait'): {'time': .5},
    (ATTACK_FSM, 'CD', 'Wait'): {'time': .5},
    (ATTACK_FSM, 'CD', 'Tk2dPlayAnimation'): {'clipName': 'Fly'},
}
# `Attack`'s four FlingObjectsFromGlobalPool, in authored order.
SHOT_ANGLES = (45., 135., 225., 315.)
SHOT_SPEED = 12.
FSM_TRANSITIONS = {
    BOUNCE_FSM: {'Initialise': [('FINISHED', 'Aim')], 'Aim': [('FINISHED', 'Left or Right?')],
                 'Stopped': [('WAKE', 'Left or Right?')], 'Fly 2': [('COLLISION STAY 2D', 'Collision Check')]},
    ATTACK_FSM: {'Sleep': [('START', 'Wait')], 'Wait': [('FINISHED', 'Attack Antic')],
                 'Attack Antic': [('FINISHED', 'Attack Antic 2')], 'Attack Antic 2': [('FINISHED', 'Attack')],
                 'Attack': [('FINISHED', 'CD')], 'CD': [('FINISHED', 'Wait')]},
}
FSM_GLOBALS = {BOUNCE_FSM: [('STOP', 'Stopped'), ('GO UP', 'Go Up')], ATTACK_FSM: [('TAKE DAMAGE', 'Attack Antic')]}


def _near(a, b, tolerance=1e-6):
    return abs(float(a) - float(b)) <= tolerance


def _fsms(records):
    found = {}
    for _, typ, data in records:
        if typ == 'PlayMakerFSM':
            found.setdefault(data['fsm']['name'], []).append(data)
    if sorted(found) != sorted([BOUNCE_FSM, ATTACK_FSM]) or any(len(v) != 1 for v in found.values()):
        raise ValueError('no single Fat Fly bounce and attack FSM pair')
    for name, (digest, start) in {BOUNCE_FSM: (BOUNCE_SHA256, 'Initialise'), ATTACK_FSM: (ATTACK_SHA256, 'Sleep')}.items():
        component = found[name][0]
        # Serialized disabled: `FSMActivator` (activateStaggered) enables both when the enemy
        # is activated, which the guest takes as at load, as for every other family that has one.
        if component['fsm']['startState'] != start or fsm_fingerprint(component['fsm']) != digest:
            raise ValueError('unverified Fat Fly FSM variant: ' + name)
        check_transitions('Fat Fly', component['fsm'], FSM_TRANSITIONS[name], FSM_GLOBALS[name])
    return found[BOUNCE_FSM][0]['fsm'], found[ATTACK_FSM][0]['fsm']


def _actions(bounce, attack):
    fsms = {BOUNCE_FSM: bounce, ATTACK_FSM: attack}
    for (name, fsm_state, action), expected in ACTIONS.items():
        check_action('Fat Fly', state(fsms[name], fsm_state), action, expected)
    if not _near(variables(bounce).get('Speed'), 4.) or variables(bounce).get('Starts Inactive') != 0:
        raise ValueError('unsupported Fat Fly bounce variables')


def _volley(attack, source, sc):
    """`Attack`'s four flings: the pooled shot, the angle and the speed of each."""
    flings = enabled_actions('Fat Fly', state(attack, 'Attack'), 'FlingObjectsFromGlobalPool')
    if len(flings) != 4:
        raise ValueError('unsupported Fat Fly volley size')
    prefab = None
    for fields, angle in zip(flings, SHOT_ANGLES):
        # speedMin/speedMax are bound to `Shot Speed`; the variable is 12.
        for key in ('angleMin', 'angleMax'):
            if not _near(fields[key]['value'], angle):
                raise ValueError('unsupported Fat Fly shot angle')
        for key in ('speedMin', 'speedMax'):
            if fields[key]['name'] != 'Shot Speed' or not fields[key]['useVariable']:
                raise ValueError('unsupported Fat Fly shot speed')
        if fields['spawnMin']['value'] != 1 or fields['spawnMax']['value'] != 1 \
                or fields['originVariationX']['value'] != 0 or fields['originVariationY']['value'] != 0:
            raise ValueError('unsupported Fat Fly shot spread')
        ref = fields['gameObject']['value']
        if prefab is not None and ref != prefab:
            raise ValueError('Fat Fly volley mixes shot prefabs')
        prefab = ref
    if not _near(variables(attack).get('Shot Speed'), SHOT_SPEED):
        raise ValueError('unsupported Fat Fly shot speed variable')
    return prefab


def _shot(source, sc, prefab):
    """The pooled `Spitter Shot R`, exactly as the Aspid validates it."""
    obj = source.ref(sc.file, prefab)
    go = source.read(obj)
    if go['m_Name'] != 'Spitter Shot R':
        raise ValueError('unsupported Fat Fly shot prefab')
    parts = {}
    for ref in go['m_Component']:
        component = source.ref(obj.assets_file, ref['component'])
        parts[source.typename(component)] = (component, source.read(component))
    for kind in ('Rigidbody2D', 'BoxCollider2D', 'DamageHero', 'EnemyBullet', 'tk2dSpriteAnimator', 'tk2dSprite', 'Transform'):
        if kind not in parts:
            raise ValueError('unsupported Fat Fly shot component set')
    if not _near(parts['Rigidbody2D'][1]['m_GravityScale'], .05) or parts['DamageHero'][1]['damageDealt'] != 1:
        raise ValueError('unsupported Fat Fly shot body')
    box = parts['BoxCollider2D'][1]
    if not _near(box['m_Size']['x'], .640625) or not _near(box['m_Size']['y'], .5625) \
            or not _near(box['m_Offset']['x'], .0078125) or not _near(box['m_Offset']['y'], 0):
        raise ValueError('unsupported Fat Fly shot box')
    library_o = source.ref(obj.assets_file, parts['tk2dSpriteAnimator'][1]['library'])
    library = source.read(library_o)
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in SHOT_CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Fat Fly shot animation: ' + name)
    return {'source': source.sid(obj), 'library': source.sid(library_o), 'library_object': library_o,
            'scale': parts['Transform'][1]['m_LocalScale']['x'] * parts['EnemyBullet'][1]['scaleMin']}


def recognize(sc, actor):
    """A Gruzzer-like bouncer, woken at 25 units, that spits four shots."""
    source = sc.source
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Fat Fly methods require a fresh source audit: ' + name)
    gid = actor['game_object']
    records = _component_records(sc, gid)
    bounce, attack = _fsms(records)
    if sorted(kind for _, kind, _ in records) != COMPONENTS:
        raise ValueError('unsupported Fat Fly component set')
    if sc.gos[gid]['m_Layer'] != 11:
        raise ValueError(f'Fat Fly outside the enemy layer: layer {sc.gos[gid]["m_Layer"]}')
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('unsupported Fat Fly initial rotation or scale')
    if len(actor['position']) < 3 or abs(actor['position'][2]) > .01:
        raise ValueError('Fat Fly depth differs from guest source plane')
    _actions(bounce, attack)
    health = actor['health_manager']
    if health['hp'] != 10 or any(health[key] for key in (
            'invincible', 'invincibleFromDirection', 'hasSpecialDeath', 'hasAlternateHitAnimation',
            'damageOverride', 'megaFlingGeo', 'mediumGeoDrops', 'largeGeoDrops')) or health['smallGeoDrops'] != 4:
        raise ValueError('unsupported Fat Fly HealthManager variant')
    _, body = _one(records, 'BoxCollider2D')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Fat Fly body collider')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or not rigid['m_Simulated'] or rigid['m_UseAutoMass'] or rigid['m_Mass'] != 1 \
            or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Fat Fly rigid body')
    _, recoil = _one(records, 'Recoil')
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 15 or not _near(recoil['recoilDuration'], .15) \
            or recoil['preventRecoilUp']:
        raise ValueError('unsupported Fat Fly recoil variant')
    _, damage = _one(records, 'DamageHero')
    if damage['damageDealt'] != 1 or damage['hazardType'] != 1 or not damage['m_Enabled']:
        raise ValueError('unsupported Fat Fly contact damage')
    _, animator = _one(records, 'tk2dSpriteAnimator')
    if not animator['m_Enabled'] or animator['isRealtime']:
        raise ValueError('Fat Fly requires enabled scaled-time animation')
    library_o = source.ref(sc.file, animator['library'])
    clips = {c['name']: c for c in source.read(library_o)['clips'] if c['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = clips.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap) \
                or clip.get('loopStart', 0) != 0:
            raise ValueError('unsupported Fat Fly animation: ' + name)
        triggers = [i for i, frame in enumerate(clip['frames']) if frame.get('triggerEvent')]
        if triggers != ([ATTACK_TRIGGER_FRAME] if name == 'Attack' else []):
            raise ValueError('unsupported Fat Fly frame event: ' + name)
    prefab = _volley(attack, source, sc)
    shot = _shot(source, sc, prefab)
    pool = _one(records, 'PersonalObjectPool')[1]['startupPool']
    if len(pool) != 1 or pool[0]['size'] != 4 or pool[0]['prefab'] != prefab:
        raise ValueError('unsupported Fat Fly shot pool')
    bounds = axis_aligned_bounds([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]],
                                 list(BODY_OFFSET), list(BODY_SIZE))
    return {
        'kind': 'FatFly', 'guest_enabled': True, 'body_bounds_local': bounds,
        'fsm_sha256': {BOUNCE_FSM: BOUNCE_SHA256, ATTACK_FSM: ATTACK_SHA256},
        'assemblies_sha256': dict(ASSEMBLIES), 'library_source': source.sid(library_o), 'shot': shot,
        'art_bindings': {'walk': 'Fly', 'turn': 'Fly', 'attack': 'Attack'},
        'limitations': [
            'Gravity-free dynamic body on the bounded terrain solver; bonk sides come from the blocked solver'
            ' axis instead of the contact normal, and the 50 Hz fixed steps run from a 60 Hz accumulator.',
            'Shots fly straight under gravity .05 without rotation or stretch; their audio and the wing buzz'
            ' loop are not presented, and `damages_enemy` never meets another enemy.',
            'The corpse is a permanent prop that bounces to rest; Corpse Flame, Steam and the spore clouds'
            ' are not presented.'],
    }
