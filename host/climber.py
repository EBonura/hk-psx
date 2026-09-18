"""Recognize the verified Windows Climber (Tiktik) variant for guest admission.

The controller lives in shared/hk-sim/src/climber.rs; this module admits only
the placed instances whose serialized fields match the audited contract in
.hkpsx/climber70/CONTRACT.md. Anything else stays an unsupported record.
"""
from runner import ASSEMBLIES, _one, axis_aligned_bounds
import hashlib

CLIPS = {'Walk': (4, 10, 0), 'Stun': (7, 12, 1), 'Death Air': (8, 30, 0), 'Death Land': (3, 15, 2)}
BODY_SIZE = (1.09375, 0.921875)
BODY_OFFSET = (0.015625, 0.4765625)


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


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
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('unsupported Climber initial rotation or scale')
    # Duplicate-box Climber placements (Crossroads_03, _05) stay out: their
    # views already sit at the 416 texture budget after similarity dedup.
    _, body = _one(records, 'BoxCollider2D')
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
            'body_bounds_local': bounds,
            'audio_sources': {'loop': source.sid(source.ref(sc.file, audio['m_Resource'])), 'volume': audio['m_Volume']},
            'limitations': ['Kinematic surface follower on the bounded terrain solver; Unity coroutine ordering and float rounding are not reproduced.',
                            'The live climb loop audio is not presented yet.',
                            'Stun freeze uses the seven-frame clip duration; Recoil.recoilDuration is not the source gate.']}
