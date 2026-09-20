"""Recognize the Greenpath Pigeon for guest admission.

The controller lives in shared/hk-sim/src/pigeon.rs; this module admits only a
placement whose serialized shape matches the one that was read to write it.

The Pigeon is the largest single family left in the project, sixty-two
placements across Greenpath and eight in the one Greenpath scene the port
admits today, and it is also the cheapest thing in it. It is a critter, not an
enemy: one hit point, no `DamageHero`, no `Recoil`, no Geo, an
`EnemyDeathEffectsNoEffect` that names no corpse prefab at all, and a single
**trigger** BoxCollider2D on layer 19, `Interactive Object`. So it cannot hurt
the hero, cannot be stood on, and never touches terrain.

That last point is why `walker_control` was never going to admit it by widening
its layer set. Layer 19 is only the first refusal; the second is that its FSM is
`Pigeon` and not `Crawler`, and the third is that the object has no solid body
for `generated_actor_records` to take its `ActorSpec::bounds` from. All three are
answered here rather than by loosening a shared rule.

The trigger geometry is deliberately not carried in the `ActorSpec`. Every
Greenpath placement is authored at the same uniform 0.8 size, mirrored or not,
and carries byte-identical children, so the `Hero Range` circle is a constant in
`pigeon.rs` and this module recomputes the placement's own world circle and
proves it, exactly as `blocker.py` proves its three boxes. That keeps
`ActorController` inside the widest variant already linked and `ActorSpec` at
152, which every enemy type in the world pays.

Run over all sixty-two Greenpath placements this admits fifty. The twelve it
refuses are Fungus1_26's seven and five of Fungus1_01b's eighteen, and they
share one reason: a second, generic `FSM` beside the `Pigeon` one that destroys
the object once the hero is within its own authored distance. That is a second
behaviour with a per-placement number in it, so it is left refused by name
rather than admitted and quietly not run.
"""
import hashlib
import math

from focus import action_fields
from runner import ASSEMBLIES, fsm_fingerprint

FSM_NAME = 'Pigeon'
# Structural fingerprint of the three FSMs a Pigeon carries: its own, and the
# two generic range triggers its children run. All eight Fungus1_01 placements
# hash identically, so a placement whose behaviour was retuned stops being this
# family rather than running the retuned one against this controller.
#
# `fsm_fingerprint` reads the compact scalar, string and variable parameters and
# not the serialized enum ordinals, so an action whose only difference is an
# enum (a `Space`, a `ForceMode2D`) hashes the same. Where that matters below it
# is argued rather than hashed; see `_flight`.
FSM_SHA256 = 'e55f8f77ea3171e357ce1170c79029898cff3afb25f6ee3b373e4a13a5c6b095'
HERO_RANGE_SHA256 = '1049dc3ecf7356b8320a5b3ef07d476ffb84f0ce1ba9ebd00906b586d60f7ebb'
ENEMY_RANGE_SHA256 = '31f41df6835d30fde2436c6e601e9800205bb58c995162a619f62b26ee03f21b'
WAKER_SHA256 = 'e8e8ffc84583e9c97ffe988a9eb606531456935602c500d9af0fb81d1730684e'
# The whole component set, so a placement carrying anything this port does not
# run stays an unadmitted record. There is no `DamageHero` and no `Recoil`, and
# `EnemyDeathEffectsNoEffect` is the class name rather than an interpretation of
# one: the family leaves nothing behind.
COMPONENTS = sorted([
    'AudioSource', 'BoxCollider2D', 'EnemyDeathEffectsNoEffect', 'ExtraDamageable',
    'HealthManager', 'MeshFilter', 'MeshRenderer', 'PlayMakerCollisionEnter2D',
    'PlayMakerCollisionStay2D', 'PlayMakerFSM', 'PlayMakerFixedUpdate', 'Rigidbody2D',
    'SetZ', 'Transform', 'tk2dSprite', 'tk2dSpriteAnimator'])
# The authored uniform scale every placement stands at, which is the frame the
# Q16 constants below and in pigeon.rs were measured in.
SCALE = 0.800000011920929
LAYER = 19  # Interactive Object
BODY_SIZE = (0.6744292974472046, 0.9062597751617432)
BODY_OFFSET = (0.116851806640625, 0.6600000262260437)
# name: (frames, fps, wrapMode). All four loop; the three idles are long and
# reuse four sprites each, so the family costs sixteen cooked images in total.
CLIPS = {'Fly': (4, 12., 0), 'Idle 01': (67, 12., 0),
         'Idle 02': (41, 12., 0), 'Idle 03': (61, 12., 0)}
# `pigeon::Clip::slot()` order. `Idle 01` and `Fly` are the shared `ActorSpec`
# slots and are not in it.
CLIP_SLOTS = ('idle2', 'idle3')
SLOT_CLIPS = {'idle2': 'Idle 02', 'idle3': 'Idle 03'}
# The three children, in authored order. `Waker` is the flock cascade this port
# does not run; it is still proven, because a placement missing it is a
# different object and because the check is what makes the omission a statement
# rather than an oversight.
CHILDREN = ('Hero Range', 'Enemy Range', 'Waker')
# The two range circles as (centre x, centre y, radius) relative to the actor
# origin in Q16, exactly as `shared/hk-sim/src/pigeon.rs` carries the first of
# them. A placement whose own world circles differ is refused rather than run
# against somebody else's geometry.
HERO_RANGE_Q16 = (0, 39426, 332399)
# Not carried by the guest. Its FSM writes the same hero visibility into the
# same bool as `Hero Range` does, and `_containment` proves its circle lies
# inside that one, so it can add no reach. What it would add is who may trip
# it: it sits on layer 15, `Enemy Detector`, where `Hero Range` is on layer 13.
ENEMY_RANGE_Q16 = (0, 42992, 185074)
# `Waker`'s own circle, which the source enables a quarter second after takeoff
# so that a flying bird trips its neighbours' `Enemy Range`. Layer 11.
WAKER_Q16 = (0, 42992, 185074)
CHILD_LAYERS = {'Hero Range': 13, 'Enemy Range': 15, 'Waker': 11}
# `Fly`'s `Translate`, and the two `RandomFloat` spans `Fly` and `Right` write.
# Q16 units, and for the forces Q16 units per second squared: `AddForce2d` runs
# `ForceMode2D.Force` every fixed step on a unit mass with no drag, so the
# authored number is an acceleration and the step rate cancels out of it.
TAKEOFF_RISE_Q16 = 32768
RISE_FORCE_Q16 = [655360, 2293760]
SIDE_FORCE_Q16 = [2293760, 4915200]
# `Right`/`Left`'s `Wait(5)` in ticks, and `Set Frame`'s inclusive `RandomInt`.
LIFE_TICKS = 300
START_FRAMES = [0, 41]


def _near(a, b, tolerance=1e-6):
    return abs(float(a) - float(b)) <= tolerance


def _assemblies(source):
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Pigeon actions require a fresh source audit: ' + name)


def _fsm(records):
    """The one FSM on the object, which has to be the audited `Pigeon`."""
    matches = [data for _, typ, data in records if typ == 'PlayMakerFSM']
    if len(matches) != 1 or matches[0]['fsm']['name'] != FSM_NAME:
        present = ', '.join(sorted(d['fsm']['name'] for _, t, d in records if t == 'PlayMakerFSM'))
        raise ValueError('unsupported Pigeon FSM set: ' + (present or 'none'))
    component = matches[0]
    if not component['m_Enabled'] or fsm_fingerprint(component['fsm']) != FSM_SHA256:
        raise ValueError('unverified Pigeon FSM variant')
    return component['fsm']


def _children(sc, gid):
    result = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        tid = child['m_PathID']
        kid = sc.transforms[tid]['m_GameObject']['m_PathID']
        result[sc.gos[kid]['m_Name']] = (kid, tid)
    return result


def _trigger_circle(sc, children, name, origin, expected, fingerprint, enabled):
    """One range child, as a Q16 circle around the actor origin."""
    from actors import _component_records
    if name not in children:
        raise ValueError('Pigeon is missing its ' + name + ' child')
    gid, tid = children[name]
    if not sc.active(gid) or sc.gos[gid]['m_Layer'] != CHILD_LAYERS[name]:
        raise ValueError('inactive or relayered Pigeon ' + name + ' child')
    records = _component_records(sc, gid)
    circles = [d for _, t, d in records if t == 'CircleCollider2D']
    if len(circles) != 1 or bool(circles[0]['m_Enabled']) != enabled \
            or not circles[0]['m_IsTrigger']:
        raise ValueError('unsupported Pigeon ' + name + ' trigger')
    fsms = [d['fsm'] for _, t, d in records if t == 'PlayMakerFSM']
    if len(fsms) != 1 or fsm_fingerprint(fsms[0]) != fingerprint:
        raise ValueError('unverified Pigeon ' + name + ' trigger FSM')
    circle = circles[0]
    matrix = sc.world(tid)
    # A mirrored placement mirrors its children too, and a circle centred on
    # x = 0 is unmoved by that; a rotation or a non-uniform scale would not be.
    if any(abs(matrix[row][column]) > 1e-6 for row, column in [(0, 1), (1, 0)]) \
            or not _near(abs(matrix[0][0]), abs(matrix[1][1])):
        raise ValueError('rotated or non-uniform Pigeon ' + name + ' child')
    actual = (round((matrix[0][3] + matrix[0][0] * circle['m_Offset']['x'] - origin[0]) * 65536),
              round((matrix[1][3] + matrix[1][1] * circle['m_Offset']['y'] - origin[1]) * 65536),
              round(circle['m_Radius'] * abs(matrix[0][0]) * 65536))
    if actual != expected:
        raise ValueError(f'Pigeon {name} circle {actual} is not the admitted {expected}')
    return actual


def _containment(inner, outer):
    """Prove the smaller circle lies inside the one the guest actually carries.

    The two range children write the same hero visibility into the same FSM
    bool, so the only thing the guest loses by reading one of them is reach, and
    reach is what this measures. It is proven per placement rather than asserted
    once, because it is what licenses dropping the child.
    """
    span = math.dist(inner[:2], outer[:2])
    if span + inner[2] > outer[2]:
        raise ValueError(f'Pigeon {inner} is not inside the admitted {outer}')


def _body(records):
    """The one trigger box, which is the whole of the Pigeon's hurt surface."""
    boxes = [d for _, t, d in records if t == 'BoxCollider2D']
    if len(boxes) != 1:
        raise ValueError('unsupported Pigeon body colliders')
    box = boxes[0]
    if not box['m_Enabled'] or not box['m_IsTrigger'] or box['m_EdgeRadius'] != 0 \
            or not all(_near(box['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(box['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Pigeon body collider')
    return box


def _depth(records):
    """`SetZ`, which is why the authored transform z is not checked.

    Read out of the installed CIL rather than inferred from the field names.
    `SetZ::OnEnable` writes `setZ = Random.Range(z, z + 0.0009999)` from the
    component's own `z` field, takes the transform's own z instead only when
    `randomizeFromStartingValue` is set, and starts a coroutine that waits
    `delayBeforeRandomizing` and then calls `transform.SetPositionZ(setZ)`.

    So every placement ends up at the same depth within half a second whatever
    its authored z was, and refusing a placement on an authored z the source
    itself discards would have lost ten of the sixty-two for nothing. What is
    checked instead is that the depth this family really settles at is the
    guest's own source plane.
    """
    records_ = [d for _, t, d in records if t == 'SetZ']
    if len(records_) != 1 or not records_[0]['m_Enabled']:
        raise ValueError('Pigeon needs exactly one enabled SetZ')
    depth = records_[0]
    if depth['randomizeFromStartingValue']:
        raise ValueError('a Pigeon that keeps its authored depth is not the admitted variant')
    if not 0 <= depth['z'] <= .01 or not 0 <= depth['delayBeforeRandomizing'] <= 1:
        raise ValueError(f'Pigeon settles at depth {depth["z"]}, off the guest source plane')
    return {'z': depth['z'], 'delay_seconds': depth['delayBeforeRandomizing'],
            'randomized': not depth['dontRandomize']}


def _rigid_body(records):
    """`gravityScale 0`, unit mass, no drag: the force is a clean acceleration."""
    bodies = [d for _, t, d in records if t == 'Rigidbody2D']
    if len(bodies) != 1:
        raise ValueError('unsupported Pigeon rigid body count')
    body = bodies[0]
    if body['m_BodyType'] != 0 or not body['m_Simulated'] or body['m_UseAutoMass'] \
            or not _near(body['m_Mass'], 1) or not _near(body['m_GravityScale'], 0) \
            or not _near(body['m_LinearDamping'], 0):
        raise ValueError('unsupported Pigeon rigid body')


def _state(fsm, name):
    return next(st for st in fsm['states'] if st['name'] == name)


def _literals(state, action, keys):
    """One named action's compact scalars, refusing any bound to a variable."""
    data = state['actionData']
    found = [i for i, n in enumerate(data['actionNames'])
             if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]]
    if len(found) != 1:
        raise ValueError(f'Pigeon {state["name"]} no longer carries one enabled {action}')
    fields = action_fields(data, found[0])
    values = []
    for key in keys:
        value = fields.get(key)
        if isinstance(value, dict):
            if value.get('useVariable'):
                raise ValueError(f'Pigeon {state["name"]}/{action}/{key} is written from a variable')
            value = value['value']
        if value is None:
            raise ValueError(f'Pigeon {state["name"]}/{action} no longer carries {key}')
        values.append(value)
    return values


def _flight(fsm):
    """The authored numbers `pigeon.rs` holds, read rather than trusted.

    `Fly` lifts the bird half a unit, draws `Rise Force`, plays `Fly` and then
    picks a side; `Right` and `Left` draw `Side Force` with opposite signs, push
    every fixed step and wait five seconds before destroying the object.

    `Translate`'s `Space.Self` is a serialized enum, which the fingerprint does
    not cover. It does not need to: the placement's basis is unrotated, and
    `Transform.Translate` resolves a Self translation through
    `TransformDirection`, which ignores scale, so Self and World are the same
    half unit of world lift here either way.
    """
    rise, = _literals(_state(fsm, 'Fly'), 'Translate', ['y'])
    if round(rise * 65536) != TAKEOFF_RISE_Q16:
        raise ValueError(f'Pigeon takeoff lift {rise} is not the admitted {TAKEOFF_RISE_Q16 / 65536}')
    lo, hi = _literals(_state(fsm, 'Fly'), 'RandomFloat', ['min', 'max'])
    if [round(lo * 65536), round(hi * 65536)] != RISE_FORCE_Q16:
        raise ValueError(f'Pigeon rise force [{lo}, {hi}] is not the admitted {RISE_FORCE_Q16}')
    # `CheckTargetDirection` names the event per side of the hero, and the state
    # transitions name where each event goes. Both halves are read, because it
    # is the pair that says the bird flies away rather than towards.
    right_event, left_event = _literals(_state(fsm, 'Fly'), 'CheckTargetDirection',
                                        ['rightEvent', 'leftEvent'])
    transitions = dict((t['fsmEvent']['name'], t['toState']) for t in _state(fsm, 'Fly')['transitions'])
    if transitions.get(right_event) != 'Left' or transitions.get(left_event) != 'Right':
        raise ValueError('a Pigeon that flies towards the hero is not the admitted variant')
    for state, span in (('Right', SIDE_FORCE_Q16), ('Left', [-SIDE_FORCE_Q16[1], -SIDE_FORCE_Q16[0]])):
        lo, hi = _literals(_state(fsm, state), 'RandomFloat', ['min', 'max'])
        if [round(lo * 65536), round(hi * 65536)] != span:
            raise ValueError(f'Pigeon {state} side force [{lo}, {hi}] is not the admitted {span}')
        wait, = _literals(_state(fsm, state), 'Wait', ['time'])
        if round(wait * 60) != LIFE_TICKS:
            raise ValueError(f'Pigeon {state} flight lasts {wait}s, not the admitted {LIFE_TICKS / 60}s')
    low, high, inclusive = _literals(_state(fsm, 'Set Frame'), 'RandomInt',
                                     ['min', 'max', 'inclusiveMax'])
    if [low, high] != START_FRAMES or not inclusive:
        raise ValueError(f'Pigeon start frame [{low}, {high}] is not the admitted {START_FRAMES}')


def _clips(source, sc, actor):
    animator = actor['tk2dSpriteAnimator']
    if not animator['m_Enabled'] or animator['isRealtime'] or not animator['playAutomatically']:
        raise ValueError('Pigeon requires enabled scaled-time animation')
    library_object = source.ref(sc.file, animator['library'])
    library = source.read(library_object)
    clips = {clip['name']: clip for clip in library['clips'] if clip['name']}
    if library['clips'][animator['defaultClipId']]['name'] != 'Idle 01':
        raise ValueError('Pigeon does not start on Idle 01')
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = clips.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Pigeon animation: ' + name)
        # `Set Frame` seeks by frame index and the guest's clip clock is the
        # cooked frame count, so a source trigger frame would mean the FSM acts
        # partway through one, which none of these does.
        if any(frame.get('triggerEvent') for frame in clip['frames']):
            raise ValueError('unsupported Pigeon frame event: ' + name)
        if clip.get('loopStart', 0) != 0:
            raise ValueError('unsupported Pigeon loop start: ' + name)
    return library_object


def recognize(sc, actor):
    """A perched bird that lifts off away from the hero and does not come back."""
    source = sc.source
    from actors import _component_records
    _assemblies(source)
    gid = actor['game_object']
    records = _component_records(sc, gid)
    # The FSM set first, because it is the difference that carries a reason:
    # twelve of the sixty-two placements hang a second, generic `FSM` off the
    # object that destroys it once the hero is within its own authored
    # distance, and naming that is more use than reporting a component count.
    fsm = _fsm(records)
    if sorted(kind for _, kind, _ in records) != COMPONENTS:
        raise ValueError('unsupported Pigeon component set')
    if sc.gos[gid]['m_Layer'] != LAYER:
        raise ValueError(f'Pigeon outside the Interactive Object layer: layer {sc.gos[gid]["m_Layer"]}')
    depth = _depth(records)
    matrix = sc.world(sc.go_transform[gid])
    # The constants below are measured at the authored 0.8, so a placement at a
    # different size would be run against somebody else's circle and drawn at
    # somebody else's size, and stays an unadmitted record. A mirror is fine and
    # three Fungus1_36 placements carry one: the circles are centred on x = 0 so
    # a mirror does not move them, the cooked art is the same image either way,
    # and `initial_direction` carries the pose `Invert Scale` flips from.
    if any(abs(matrix[row][column]) > 1e-6 for row, column in [(0, 1), (1, 0)]) \
            or not _near(abs(matrix[0][0]), SCALE) or not _near(matrix[1][1], SCALE):
        raise ValueError('unsupported Pigeon rotation or scale')
    if fsm['startState'] != 'Set Size':
        raise ValueError('Pigeon begins in an unsupported state')
    globals_ = [(t['fsmEvent']['name'], t['toState']) for t in fsm.get('globalTransitions', [])]
    if globals_:
        raise ValueError('unsupported Pigeon global transitions: ' + repr(globals_))
    _body(records)
    _rigid_body(records)
    health = actor['health_manager']
    if health['hp'] != 1 or any(health[key] for key in (
            'invincible', 'invincibleFromDirection', 'hasSpecialDeath',
            'hasAlternateHitAnimation', 'damageOverride', 'megaFlingGeo',
            'smallGeoDrops', 'mediumGeoDrops', 'largeGeoDrops')):
        raise ValueError('unsupported Pigeon HealthManager variant')
    deaths = [d for _, t, d in records if t == 'EnemyDeathEffectsNoEffect']
    if len(deaths) != 1 or not deaths[0]['m_Enabled']:
        raise ValueError('Pigeon needs exactly one enabled EnemyDeathEffectsNoEffect')
    sprite = actor['tk2dSprite']
    if sprite['_color'] != {'r': 1., 'g': 1., 'b': 1., 'a': 1.} \
            or sprite['_scale'] != {'x': 1., 'y': 1., 'z': 1.}:
        raise ValueError('unsupported Pigeon sprite scale/color')
    children = _children(sc, gid)
    if sorted(children) != sorted(CHILDREN):
        raise ValueError('unsupported Pigeon children: ' + ', '.join(sorted(children)))
    origin = actor['position'][:2]
    hero_range = _trigger_circle(sc, children, 'Hero Range', origin, HERO_RANGE_Q16,
                                 HERO_RANGE_SHA256, True)
    enemy_range = _trigger_circle(sc, children, 'Enemy Range', origin, ENEMY_RANGE_Q16,
                                  ENEMY_RANGE_SHA256, True)
    # The `Waker` starts disabled: `Activate` only switches it on a quarter
    # second after this bird's own takeoff.
    waker = _trigger_circle(sc, children, 'Waker', origin, WAKER_Q16, WAKER_SHA256, False)
    _containment(enemy_range, hero_range)
    _containment(waker, hero_range)
    _flight(fsm)
    library_object = _clips(source, sc, actor)
    return {
        'kind': 'Pigeon', 'guest_enabled': True, 'no_corpse': True,
        # Its one collider is a trigger, so there is no solid body to measure
        # the spec bounds from and the trigger is the hurt surface itself.
        'trigger_body': True,
        'fsm_sha256': FSM_SHA256, 'assemblies_sha256': dict(ASSEMBLIES),
        'library_source': source.sid(library_object),
        'hero_range_q16': hero_range, 'enemy_range_q16': enemy_range, 'waker_q16': waker,
        'set_z': depth,
        # Nothing of the transform reaches the controller: `Set Frame` flips the
        # mirror on a coin and `Right`/`Left` overwrite it with the flight
        # direction. This is the authored pose, which the guest writes as -1.
        'initial_direction': 1 if matrix[0][0] < 0 else -1,
        'art_bindings': dict({'walk': 'Idle 01', 'turn': 'Fly'},
                             **{slot: SLOT_CLIPS[slot] for slot in CLIP_SLOTS}),
        'limitations': [
            '`Set Size` is not presented. The source draws `RandomFloat(0.8, 1.0)` on the first'
            ' frame and rescales the whole object, which also rescales its trigger circles; the'
            ' guest draws every bird at the authored 0.8 and reads the circle measured there,'
            ' because a cooked frame cannot be resized and the circle is a linked constant.',
            'The `Waker` flock cascade is not presented. In the source a bird that lifts off'
            ' unparents an `Enemies`-layer trigger a quarter second later, which trips its'
            " neighbours' `Enemy Range` and lifts the whole flock, at any distance with a clear"
            ' line to the hero. The guest reads only `Hero Range`, so each bird answers the hero'
            " on its own. In Fungus1_01 the two flocks stand inside each other's `Hero Range`"
            ' circles, so they still leave together there.',
            '`HERO CAST SPELL` is not presented. The source sends it to every Pigeon in the scene'
            ' and `Check` lifts any within sixty units of the hero, which is the whole room; in'
            ' the guest a spell startles nothing.',
            '`FaceAngle` is not presented: the source rotates a flying bird to its velocity every'
            ' frame, and actor draws carry a rotation only for the Climber. It keeps its upright'
            ' pose and only the horizontal mirror follows the flight direction.',
            "The flight is integrated at 60 Hz against the source's 50 Hz fixed step. The force"
            ' is an acceleration, so the speed matches; the positions differ by the Euler'
            ' integration residue rather than by a constant.',
            'A bird that leaves the actor neighbourhood freezes where it is instead of finishing'
            ' its five seconds, and the guest removes one that reaches the edge of the validated'
            ' coordinate range. Both happen far outside the room and neither is ever drawn.',
            'The `SetZ` depth is not presented. The source holds the authored transform z for'
            f' {depth["delay_seconds"]}s and then replaces it with `Random.Range(z, z + 0.001)` off the'
            f' component field, which is {depth["z"]} for every placement; the guest draws every actor'
            ' on its one source plane throughout, so neither depth reaches the screen.',
            'The takeoff `AudioPlayRandom` and the looping `AudioSource` are not presented, and'
            ' neither is the recycled `PlayMakerCollisionEnter2D`/`Stay2D` pair, which this FSM'
            ' has no action for.'],
    }
