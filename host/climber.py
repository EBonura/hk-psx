"""Recognize the verified Windows Climber (Tiktik) variant for guest admission.

The controller lives in shared/hk-sim/src/climber.rs; this module admits only
the placed instances whose serialized fields match the audited contract in
.hkpsx/climber70/CONTRACT.md. Anything else stays an unsupported record.
"""
from runner import ASSEMBLIES, _one, axis_aligned_bounds, body_box
import hashlib
import math

CLIPS = {'Walk': (4, 10, 0), 'Stun': (7, 12, 1), 'Death Air': (8, 30, 0), 'Death Land': (3, 15, 2)}
BODY_SIZE = (1.09375, 0.921875)
BODY_OFFSET = (0.015625, 0.4765625)
# Authored world rotations sit a fraction of a degree off the quarter turn
# (level39:1069 is 179.999988), so the basis is compared against the exact
# quarter it names rather than against the serialized angle.
BASIS_TOLERANCE = 1e-5


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def initial_quarter(sc, gid):
    """The quarter turn `Climber.Start` reads off the transform.

    Start picks its first direction from `transform.rotation.eulerAngles.z` and
    its handedness from `Sign(transform.localScale.x)`. Every placement in the
    catalog carries unit positive scale, so handedness is `startRight` alone and
    the rotation is the one placement input left: 0 for a floor Tiktik, 180 for
    a ceiling one. A rotation between the quarter turns, or a scaled transform,
    stays an unadmitted record rather than being rounded into one.
    """
    scale = sc.transforms[sc.go_transform[gid]]['m_LocalScale']
    matrix = sc.world(sc.go_transform[gid])
    quarter = round(math.degrees(math.atan2(matrix[1][0], matrix[0][0])) / 90) % 4
    cos, sin = [(1, 0), (0, 1), (-1, 0), (0, -1)][quarter]
    if not all(_near(scale[axis], 1.0) for axis in 'xyz') \
            or any(abs(matrix[i][j] - value) > BASIS_TOLERANCE
                   for i, row in enumerate([[cos, -sin], [sin, cos]])
                   for j, value in enumerate(row)):
        raise ValueError('unsupported Climber initial rotation or scale')
    return quarter


def recognize(sc, actor):
    from actors import _component_records
    source = sc.source
    records = _component_records(sc, actor['game_object'])
    _, climber = _one(records, 'Climber')
    for name, value in (('speed', 2.0), ('spinTime', 0.25), ('wallRayPadding', 0.1), ('minTurnDistance', 0.25)):
        if not climber.get('m_Enabled') or not _near(climber.get(name), value):
            raise ValueError('unsupported Climber field: ' + name)
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Climber methods require a fresh source audit: ' + name)
    gid = actor['game_object']
    quarter = initial_quarter(sc, gid)
    # Several placements (level39:424, level39:1069, level76:930) carry the same
    # box twice. The duplicate is field for field the original, so the contact
    # shape is the one box; `body_box` is the same reading the Runner does.
    body = body_box(records, 'Climber')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Climber body collider')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 1 or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Climber rigid body')
    _, recoil = _one(records, 'Recoil')
    if not recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 0:
        raise ValueError('unsupported Climber recoil variant')
    _, animator = _one(records, 'tk2dSpriteAnimator')
    library = source.read(source.ref(sc.file, animator['library']))
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Climber animation: ' + name)
    _, audio = _one(records, 'AudioSource')
    if not audio['Loop'] or not audio['m_PlayOnAwake']:
        raise ValueError('unsupported Climber audio source')
    bounds = axis_aligned_bounds([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]],
                                 [BODY_OFFSET[0], BODY_OFFSET[1]], list(BODY_SIZE))
    return {'kind': 'Climber', 'guest_enabled': True, 'start_right': bool(climber['startRight']),
            'speed': 2.0, 'spin_time': 0.25, 'wall_ray_padding': 0.1, 'min_turn_distance': 0.25,
            'body_bounds_local': bounds, 'rotation_q16': quarter * 90 * 65536,
            'audio_sources': {'loop': source.sid(source.ref(sc.file, audio['m_Resource'])), 'volume': audio['m_Volume']},
            'limitations': ['Kinematic surface follower on the bounded terrain solver; Unity coroutine ordering and float rounding are not reproduced.',
                            'The live climb loop audio is not presented yet.',
                            'Stun freeze uses the seven-frame clip duration; Recoil.recoilDuration is not the source gate.',
                            'The authored rotation is snapped to its quarter turn; the placements are cardinal and the sub-degree serialized error is not carried.']}


# --- Moss Walker (Mosscreep, Greenpath) --------------------------------------
#
# The controller is shared/hk-sim/src/moss_walker.rs. All 13 placements carry
# one `Moss Walker` FSM (pinned by digest, its `Roams` bool aside) and one
# `Wake Range` FSM. Only floor placements are admitted: the three wall
# placements read, from the IL, as walkers whose rays point away from their
# wall and never turn, which has to be watched on the original before it is
# reproduced.
MOSS_WALKER_FSM_SHA256 = '9f9a1cd537e40cf2e0e8e3a0a0012a456ceb87ff51760ca16871dca16049d2b4'
MOSS_WAKE_FSM_SHA256 = '155198314430555fbfb03504d4a5075efc0578491226ff08583e2c7d6c285e29'
MOSS_WALKER_CLIPS = {'Walk': (4, 12, 0), 'Turn': (3, 12, 2), 'Rest': (1, 30, 6), 'Shake': (3, 12, 0),
                     'Appear': (5, 10, 2), 'Bury': (5, 12, 2)}
MOSS_WALKER_SLOTS = (('rest', 'Rest'), ('shake', 'Shake'), ('appear', 'Appear'), ('bury', 'Bury'))
MOSS_WALKER_BODY = ((1.261925458908081, 1.250787377357483), (-0.056549072265625, -0.2771453857421875))
# Child point rays and the wake circle, as shared/hk-sim/src/moss_walker.rs
# carries them (EDGE/WALL/GROUND_ORIGIN, WAKE_RADIUS).
MOSS_WALKER_CHILDREN = {'Edge Range': (-0.91, -0.65), 'Wall Range': (-0.4, -0.38), 'Ground Range': (0., -0.31)}
MOSS_WAKE_RADIUS = 8.27


def recognize_moss_walker(sc, actor):
    """Admit a placed floor Moss Walker, or refuse with the reason."""
    from actors import _component_records
    from false_knight import fsm_digest
    from focus import fsm_variables
    source = sc.source
    gid = actor['game_object']
    records = _component_records(sc, gid)
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Moss Walker methods require a fresh source audit: ' + name)
    fsms = [d for _, t, d in records if t == 'PlayMakerFSM']
    if len(fsms) != 1 or fsms[0]['fsm']['name'] != 'Moss Walker':
        raise ValueError('no single Moss Walker FSM')
    fsm = fsms[0]['fsm']
    if fsm_digest(fsm, ignore=('Roams',)) != MOSS_WALKER_FSM_SHA256:
        raise ValueError('Moss Walker FSM changed: ' + fsm_digest(fsm, ignore=('Roams',)))
    roams = fsm_variables(fsm).get('Roams')
    if roams not in (0, 1, False, True):
        raise ValueError('unsupported Moss Walker Roams')
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('wall or roof Moss Walker: its IL ray directions await verification on the original')
    health = actor['health_manager']
    if health['hp'] != 10 or not health['invincible'] or health['invincibleFromDirection'] or not health['preventInvincibleEffect'] \
            or health['hasSpecialDeath'] or health['hasAlternateHitAnimation'] or health['damageOverride']:
        raise ValueError('unsupported Moss Walker HealthManager')
    (size, offset) = MOSS_WALKER_BODY
    body = body_box(records, 'Moss Walker')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', size)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', offset)):
        raise ValueError('unsupported Moss Walker body collider')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 1 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Moss Walker rigid body')
    _, recoil = _one(records, 'Recoil')
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 15 or not _near(recoil['recoilDuration'], .15):
        raise ValueError('unsupported Moss Walker recoil')
    _, damage = _one(records, 'DamageHero')
    _, bouncer = _one(records, 'NonBouncer')
    if damage['damageDealt'] != 0 or not bouncer['active']:
        raise ValueError('Moss Walker does not start buried')
    tid = sc.go_transform[gid]
    children = {sc.gos[t['m_GameObject']['m_PathID']]['m_Name']: t for t in sc.transforms.values()
                if t['m_Father']['m_PathID'] == tid}
    for name, (x, y) in MOSS_WALKER_CHILDREN.items():
        t = children.get(name)
        if t is None or not _near(t['m_LocalPosition']['x'], x) or not _near(t['m_LocalPosition']['y'], y):
            raise ValueError('unsupported Moss Walker ray child: ' + name)
    wake = children.get('Wake Range')
    if wake is None or wake['m_LocalPosition'] != {'x': 0., 'y': 0., 'z': 0.} or wake['m_LocalScale']['x'] != 1:
        raise ValueError('unsupported Moss Walker Wake Range')
    wake_records = _component_records(sc, wake['m_GameObject']['m_PathID'])
    _, circle = _one(wake_records, 'CircleCollider2D')
    wake_fsm = [d['fsm'] for _, t, d in wake_records if t == 'PlayMakerFSM']
    if not circle['m_IsTrigger'] or circle['m_Offset'] != {'x': 0., 'y': 0.} or not _near(circle['m_Radius'], MOSS_WAKE_RADIUS) \
            or len(wake_fsm) != 1 or fsm_digest(wake_fsm[0]) != MOSS_WAKE_FSM_SHA256:
        raise ValueError('unsupported Moss Walker Wake Range')
    _, animator = _one(records, 'tk2dSpriteAnimator')
    library = source.read(source.ref(sc.file, animator['library']))
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in MOSS_WALKER_CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Moss Walker animation: ' + name)
    limitations = ['Floor placements only; the wall placements are refused until their rays are watched on the original.',
                   'The footstep loop, the emerge and look sounds and the grass particles are not presented.']
    if not fsms[0]['m_Enabled']:
        limitations.append('Authored with its FSM disabled for FSMActivator; seated active like the others.')
    return {'kind': 'MossWalker', 'guest_enabled': True, 'roams': bool(roams),
            # Hidden and harmless until it wakes; the controller switches both.
            'invincible': False, 'contact_damage': 1,
            'art_bindings': dict({'walk': 'Walk', 'turn': 'Turn'}, **dict(MOSS_WALKER_SLOTS)),
            'limitations': limitations}
