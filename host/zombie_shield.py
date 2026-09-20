"""Recognize the two Crossroads_15 Zombie Shields for guest admission.

The controller lives in shared/hk-sim/src/zombie_shield.rs; this module admits
only a placement whose serialized shape matches the one that was read to write
it.

The Zombie Shield is the second family to carry the source `Walker`, so most of
what the Runner already proved about that component is reused here rather than
restated: `runner.body_contract` for the rigid body, `runner.body_box` for the
collider, `runner.axis_aligned_bounds` for the trigger, and
`runner.fsm_fingerprint` for the structural hash. What differs is the one
authored field the Runner refuses, `pauses = 0`, and the FSM on top.

`pauses = 0` is not a parameterization of the Runner. `Walker::BeginStopped`
calls `EndStopping` immediately when it is false and `UpdateWalking` skips the
walk-timer countdown that would otherwise stop it, so this variant has no idle
state at all; and `walker_parameters` refuses it precisely so that a Walker with
no pauses cannot be admitted as a Runner that happens never to pause. The other
half is the FSM: `ZombieShieldControl` raises a shield and counters, where
`Zombie Swipe` lunges. Both halves need this file.

Sixteen of the twenty-two clips are the shield and the two attack chains, and
`Attack 3` is nine of them, so the whole family's art is the thing to measure
before admitting it. Crossroads_15's tightest view carries 173 of the 416 CLUT
slots today and admits no actor at all, which is what makes the room affordable.
"""
import hashlib

from combat import ticks
from runner import (ASSEMBLIES, axis_aligned_bounds, body_box, body_contract,
                    fsm_fingerprint)

# Structural fingerprint of `ZombieShieldControl`: states, transitions, enabled
# actions and their scalar fields, and the variables. Both Crossroads_15
# placements share it; any changed variant needs another source audit before a
# controller written from this one is allowed to run it.
FSM_SHA256 = '8cd470841556cbf24648cfa41f616ea4bc7d9e10884d7f8e70f4250c019cd386'
FSM_NAME = 'ZombieShieldControl'
# `Walker` fields this controller reproduces. `pauses` is the one that differs
# from the Runner's audited set, and it is the reason this is its own family.
WALKER_FIELDS = dict(rightScale=-1., edgeXAdjuster=0., turnPause=1.,
                     turnAfterIdlePercentage=0, pauses=0, idleClip='Idle',
                     walkClip='Walk', turnClip='Turn', ambush=0, startInactive=0,
                     waitForHeroX=0, preventTurn=0, ignoreHoles=0,
                     preventTurningToFaceHero=0, preventScaleChange=0,
                     m_Enabled=1, walkSpeedR=2., walkSpeedL=-2.)
# name: (frames, fps, wrapMode). Walk, Turn and Idle are the Walker's own three;
# the rest are the FSM's, in `zombie_shield::Clip::slot` order below.
CLIPS = {
    'Walk': (7, 10., 0), 'Turn': (2, 10., 2), 'Idle': (6, 12., 0),
    'Shield Front': (3, 15., 2), 'Shield Top': (3, 15., 2),
    'Shield Front Bump': (2, 10., 2), 'Shield Top Bump': (2, 10., 2),
    'Unshield Front': (2, 15., 2), 'Unshield Top': (2, 15., 2),
    'Attack1 A': (5, 10., 2), 'Attack1 L': (1, 12., 2), 'Attack1 S': (1, 12., 2),
    'Attack1 CD': (6, 10., 2),
    'Attack3 A1': (5, 12., 2), 'Attack3 L1': (1, 10., 2), 'Attack3 S1': (1, 12., 2),
    'Attack3 CD1': (2, 10., 2), 'Attack3 L2': (1, 12., 2), 'Attack3 CD2': (3, 10., 2),
    'Attack3 L3': (1, 10., 2), 'Attack3 S3': (1, 12., 2), 'Attack3 CD3': (4, 10., 2),
}
# The `ActorController::ZombieShield::clips` array, in `Clip::slot()` order.
# Walk and Turn are the shared `ActorSpec` slots and are not in it.
CLIP_SLOTS = ('idle', 'shield_front', 'shield_top', 'bump_front', 'bump_top',
              'unshield_front', 'unshield_top',
              'a1_antic', 'a1_lunge', 'a1_slash', 'a1_cooldown',
              'a3_antic', 'a3_lunge1', 'a3_slash1', 'a3_cooldown1', 'a3_lunge2',
              'a3_cooldown2', 'a3_lunge3', 'a3_slash3', 'a3_cooldown3')
SLOT_CLIPS = {
    'idle': 'Idle', 'shield_front': 'Shield Front', 'shield_top': 'Shield Top',
    'bump_front': 'Shield Front Bump', 'bump_top': 'Shield Top Bump',
    'unshield_front': 'Unshield Front', 'unshield_top': 'Unshield Top',
    'a1_antic': 'Attack1 A', 'a1_lunge': 'Attack1 L', 'a1_slash': 'Attack1 S',
    'a1_cooldown': 'Attack1 CD',
    'a3_antic': 'Attack3 A1', 'a3_lunge1': 'Attack3 L1', 'a3_slash1': 'Attack3 S1',
    'a3_cooldown1': 'Attack3 CD1', 'a3_lunge2': 'Attack3 L2',
    'a3_cooldown2': 'Attack3 CD2', 'a3_lunge3': 'Attack3 L3',
    'a3_slash3': 'Attack3 S3', 'a3_cooldown3': 'Attack3 CD3',
}
# `FindAlertRange`'s own `childName`, which is what picks this object's gate out
# of the two AlertRange children. The `Walker` reads the same one.
ATTACK_RANGE = 'Attack Range'
# The two `DamageHero` sword colliders the attack states switch on. They are
# authored inactive and the guest never switches them on; see `limitations`.
SLASH_CHILDREN = ('Slash', 'Slash 2')


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def _assemblies(source):
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Zombie Shield actions require a fresh source audit: ' + name)


def _walker(records):
    matches = [(sid, data) for sid, typ, data in records if typ == 'Walker']
    if len(matches) != 1:
        raise ValueError('Zombie Shield requires exactly one Walker')
    _, walker = matches[0]
    for name, value in WALKER_FIELDS.items():
        actual = walker.get(name)
        if isinstance(value, float):
            if not isinstance(actual, float) or not _near(actual, value):
                raise ValueError('unsupported Zombie Shield Walker field: ' + name)
        elif actual != value:
            raise ValueError('unsupported Zombie Shield Walker field: ' + name)
    return walker


def _fsm(records):
    """The one FSM on the object, which has to be the shield's own."""
    matches = [data for _, typ, data in records if typ == 'PlayMakerFSM']
    if len(matches) != 1 or matches[0]['fsm']['name'] != FSM_NAME:
        present = ', '.join(sorted(d['fsm']['name'] for _, t, d in records if t == 'PlayMakerFSM'))
        raise ValueError('unsupported Zombie Shield FSM set: ' + (present or 'none'))
    component = matches[0]
    fingerprint = fsm_fingerprint(component['fsm'])
    if not component['m_Enabled'] or fingerprint != FSM_SHA256:
        raise ValueError('unverified Zombie Shield FSM variant')
    return component['fsm'], fingerprint


def _children(sc, gid):
    result = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        kid = sc.transforms[child['m_PathID']]['m_GameObject']['m_PathID']
        result[sc.gos[kid]['m_Name']] = kid
    return result


def _attack_range(sc, actor, walker, children):
    """`Attack Range`, the one AlertRange both the Walker and the FSM read."""
    from actors import _component_records
    gid = children.get(ATTACK_RANGE)
    if gid is None or not sc.active(gid):
        raise ValueError('Zombie Shield has no active Attack Range child')
    records = _component_records(sc, gid)
    alerts = [(sid, data) for sid, typ, data in records if typ == 'AlertRange']
    if len(alerts) != 1 or not alerts[0][1]['m_Enabled']:
        raise ValueError('unsupported Zombie Shield Attack Range component')
    reference = walker['alertRange']
    if reference['m_FileID'] != 0 or reference['m_PathID'] != alerts[0][0]:
        raise ValueError('Zombie Shield Walker reads a different alert range')
    boxes = [d for _, t, d in records if t == 'BoxCollider2D']
    if len(boxes) != 1 or not boxes[0]['m_Enabled'] or not boxes[0]['m_IsTrigger'] \
            or boxes[0]['m_EdgeRadius'] != 0:
        raise ValueError('unsupported Zombie Shield Attack Range trigger')
    box = boxes[0]
    return axis_aligned_bounds(sc.world(sc.go_transform[gid]),
                               [box['m_Offset'][k] for k in 'xy'],
                               [box['m_Size'][k] for k in 'xy'])


def _clips(source, sc, actor):
    animator = actor['tk2dSpriteAnimator']
    if not animator['m_Enabled'] or animator['isRealtime']:
        raise ValueError('Zombie Shield requires enabled scaled-time animation')
    library_object = source.ref(sc.file, animator['library'])
    library = source.read(library_object)
    clips = {clip['name']: clip for clip in library['clips'] if clip['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = clips.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Zombie Shield animation: ' + name)
        # The guest's clip clock is the cooked frame count; a source trigger
        # frame would mean the FSM acts partway through one, which none does.
        if any(frame.get('triggerEvent') for frame in clip['frames']):
            raise ValueError('unsupported Zombie Shield frame event: ' + name)
        if not 0 <= clip.get('loopStart', 0) < len(clip['frames']):
            raise ValueError('invalid Zombie Shield loop start: ' + name)
    return library_object


def recognize(sc, actor):
    """A Walker with no pauses, a shield that tracks the hero, and two chains."""
    source = sc.source
    from actors import _component_records
    _assemblies(source)
    gid = actor['game_object']
    records = _component_records(sc, gid)
    walker = _walker(records)
    fsm, fingerprint = _fsm(records)
    if fsm['startState'] != 'Initialise':
        raise ValueError('Zombie Shield begins in an unsupported state')
    if fsm.get('globalTransitions'):
        raise ValueError('unsupported Zombie Shield global transitions')
    if len(actor['position']) < 3 or abs(actor['position'][2]) > .01:
        raise ValueError('Zombie Shield depth differs from guest source plane')
    matrix = sc.world(sc.go_transform[gid])
    if sc.gos[gid]['m_Layer'] != 11 or abs(abs(matrix[0][0]) - 1) > 1e-6 \
            or abs(matrix[1][1] - 1) > 1e-6 or abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6:
        raise ValueError('unsupported Zombie Shield layer or initial scale')
    # `rightScale` is -1, so a mirrored transform is the one facing right.
    mirror = -1 if matrix[0][0] < 0 else 1
    initial_direction = -mirror
    box = body_box(records, 'Zombie Shield')
    if not box['m_Enabled'] or box['m_IsTrigger'] or box['m_EdgeRadius'] != 0:
        raise ValueError('unsupported Zombie Shield body collider')
    bodies = [d for _, t, d in records if t == 'Rigidbody2D']
    if len(bodies) != 1:
        raise ValueError('Zombie Shield requires exactly one Rigidbody2D')
    body_contract(bodies[0])
    if bodies[0]['m_GravityScale'] != 1.:
        raise ValueError('unsupported Zombie Shield gravity scale')
    recoils = [d for _, t, d in records if t == 'Recoil']
    if len(recoils) != 1 or recoils[0]['freezeInPlace'] or recoils[0]['preventRecoilUp'] \
            or recoils[0]['recoilSpeedBase'] != 10 or not _near(recoils[0]['recoilDuration'], .15):
        raise ValueError('unsupported Zombie Shield recoil variant')
    damage = [d for _, t, d in records if t == 'DamageHero']
    if len(damage) != 1 or not damage[0]['m_Enabled'] or damage[0]['damageDealt'] != 1:
        raise ValueError('unsupported Zombie Shield contact damage')
    detectors = [sid for sid, t, _ in records if t == 'LineOfSightDetector']
    reference = walker['lineOfSightDetector']
    if len(detectors) != 1 or reference['m_FileID'] != 0 or reference['m_PathID'] != detectors[0]:
        raise ValueError('Zombie Shield sensing references differ')
    children = _children(sc, gid)
    # The two sword hitboxes are authored inactive and nothing in this port
    # switches them on, so an active one would be contact damage the guest
    # silently drops rather than a hitbox it presents.
    for name in SLASH_CHILDREN:
        if name not in children:
            raise ValueError('Zombie Shield is missing its ' + name + ' hitbox')
        if sc.active(children[name]):
            raise ValueError('unsupported active Zombie Shield ' + name + ' hitbox')
    attack_world = _attack_range(sc, actor, walker, children)
    library_object = _clips(source, sc, actor)
    body_bounds = axis_aligned_bounds(matrix, [box['m_Offset'][k] for k in 'xy'],
                                      [box['m_Size'][k] for k in 'xy'])
    x, y = actor['position'][:2]

    def relative_q16(bounds):
        result = [round((v - [x, y][i % 2]) * 65536) for i, v in enumerate(bounds)]
        if any(abs(v) > 16 * 65536 for v in result):
            raise ValueError('Zombie Shield local bounds exceed Q16 contract')
        return result

    body_q16, attack_q16 = relative_q16(body_bounds), relative_q16(attack_world)
    # `runner_senses::Shape` mirrors the body with the facing and leaves the
    # trigger alone, which is only right for a trigger centred on the actor.
    if attack_q16[0] != -attack_q16[2] or body_q16[0] >= body_q16[2] \
            or body_q16[1] >= body_q16[3] or attack_q16[1] >= attack_q16[3]:
        raise ValueError('Zombie Shield sensing shape is not a mirror-stable box')
    return {
        'kind': 'ZombieShield', 'guest_enabled': True,
        'fsm_sha256': fingerprint, 'assemblies_sha256': dict(ASSEMBLIES),
        'library_source': source.sid(library_object),
        'initial_direction': initial_direction,
        'walk_speed': walker['walkSpeedR'],
        'turn_cooldown_ticks': 60, 'turn_ticks': ticks(CLIPS['Turn'][0] / CLIPS['Turn'][1]),
        'body_bounds_q16': body_q16, 'attack_bounds_q16': attack_q16,
        'art_bindings': dict({'walk': 'Walk', 'turn': 'Turn'},
                             **{slot: SLOT_CLIPS[slot] for slot in CLIP_SLOTS}),
        'limitations': [
            'The shield is SetInvincible: HealthManager::IsBlockingByDirection is run as the source'
            ' wrote it, so the overhead shield blocks every direction and the front one blocks the'
            " hero's side and an up slash but not a pogo.",
            'Unshield Front and Unshield Top carry no SetInvincible, so a Shield that loses the hero'
            ' walks away still guarded until its next lunge clears it. That is the source, not a'
            ' defect of this controller.',
            'The Slash and Slash 2 sword hitboxes are not presented: the lunge damages by the body,'
            ' the way the Runner already does, so an attack reaches the body box rather than about'
            ' a unit and a half further.',
            'Nothing of the corpse is presented. The source corpse prefab is a Zombie corpse that'
            " effects.py's Runner contract would likely admit, and wiring it is the one piece of"
            ' this family left outside the guest.',
            'Shield Counter is a per-actor deterministic sample of the source RandomInt(60, 100),'
            ' not Unity RNG parity; one decrement a tick, including a tick that re-aims the shield.',
            'Before the Walker starts, the guest holds Idle. The source animator has'
            ' playAutomatically on with a default clip of Attack3 S3, a single frame it shows until'
            " something plays over it; the Walker's camera gate opens 60 units away, so nothing is"
            ' ever on screen for that.',
            'The Dust Kick emitter, the audio one shots and the SetZ depth are not presented.'],
    }
