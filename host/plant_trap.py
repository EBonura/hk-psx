"""Recognize the Greenpath Plant Trap (Snapper Trap) for guest admission.

The controller lives in shared/hk-sim/src/plant_trap.rs; this module admits only
a placement whose serialized shape matches the one that was read to write it
(docs/PLANT_TRAP.md).

The object has no collider of its own. tk2d builds a `BoxCollider2D` from the
sprite definition of the frame showing (`tk2dBaseSprite.UpdateCollider`:
`colliderType` 2 is a box centred on `colliderVertices[0]` with half extents
`colliderVertices[1]`, and 1 disables it), so the jaws hurt and can be hurt only
on the `Snap` frames and the first three `Retract` frames. This module reads
those boxes off the sprite definitions and proves them against the constants
`plant_trap.rs` holds, together with the `Detector` child's trigger box.
"""
import hashlib

from actors import _component_records
from fsm_pins import check_action, check_transitions, state
from runner import ASSEMBLIES, _one, fsm_fingerprint

# The cooked clips beyond walk and turn, in `plant_trap::Clip::slot` order.
CLIP_SLOTS = ('ready', 'snap', 'retract')
CONTROL_FSM = 'Plant Trap Control'
DAMAGES_FSM = 'damages_enemy'
CONTROL_SHA256 = 'fb0b5b0a8df9c655fe3407d7f84dfdf0b2025e92979c5ab561152783fd9b2b32'
DAMAGES_SHA256 = '1f384ad187e2b1e643be82fdc7d486e3d28bf1221d6def2c6e17642b06b84629'
DETECT_SHA256 = '830731ad9c6eb938fd0102ea2405832553d5e3753bec08a2e07aed16d1492736'
COMPONENTS = sorted([
    'AudioSource', 'DamageHero', 'EnemyDeathEffects', 'EnemyDreamnailReaction', 'ExtraDamageable',
    'HealthManager', 'InfectedEnemyEffects', 'MeshFilter', 'MeshRenderer', 'PersistentBoolItem',
    'PlayMakerCollisionEnter2D', 'PlayMakerFSM', 'PlayMakerFSM', 'SpriteFlash', 'Transform',
    'tk2dSprite', 'tk2dSpriteAnimator'])
# name: (frames, fps, wrapMode, loopStart)
CLIPS = {'Idle': (1, 12., 2, 0), 'Snap Ready': (6, 12., 1, 1), 'Snap': (3, 12., 2, 0),
         'Retract': (7, 12., 2, 0), 'Death': (7, 12., 2, 0)}
# The frames that define a collider, as [x0, y0, x1, y1] Q16 (plant_trap.rs):
# clip -> frame -> box. Every other frame of the clips above defines none.
COLLIDERS = {
    'Snap': {0: [-129024, -167936, 134144, -54272], 1: [-56320, -167936, 41984, 89088],
             2: [-56320, -167936, 49152, 36864]},
    'Retract': {0: [-56320, -167936, 49152, 36864], 1: [-34816, -167936, 33792, 36864],
                2: [-34816, -167936, 30720, -1024]},
}
DETECT_Q16 = [-72704, -167936, 89088, -85283]
# `Plant Trap Control`'s audited waits, in seconds (plant_trap.rs holds them in ticks).
ACTIONS = {
    ('Ready', 'Wait'): {'time': .75}, ('Ready', 'Tk2dPlayAnimation'): {'clipName': 'Snap Ready'},
    ('Snap', 'Wait'): {'time': 1.}, ('Snap', 'Tk2dPlayAnimation'): {'clipName': 'Snap'},
    ('Retract', 'Tk2dPlayAnimationWithEvents'): {
        'clipName': 'Retract', 'animationTriggerEvent': '', 'animationCompleteEvent': 'FINISHED'},
    ('Cooldown', 'Wait'): {'time': .5},
}
TRANSITIONS = {'Idle': [('DETECT', 'Ready')], 'Ready': [('FINISHED', 'Snap')], 'Init': [('FINISHED', 'Idle')],
               'Snap': [('WAIT', 'Retract')], 'Retract': [('FINISHED', 'Cooldown')],
               'Cooldown': [('FINISHED', 'Init')]}


def _near(a, b, tolerance=1e-6):
    return abs(float(a) - float(b)) <= tolerance


def _q16(value):
    return round(value * 65536)


def _fsms(records):
    found = [data['fsm'] for _, typ, data in records if typ == 'PlayMakerFSM']
    by_name = {fsm['name']: fsm for fsm in found}
    if sorted(by_name) != sorted([CONTROL_FSM, DAMAGES_FSM]) or len(found) != 2:
        raise ValueError('no single Plant Trap Control and damages_enemy FSM pair')
    control = by_name[CONTROL_FSM]
    if control['startState'] != 'Init' or fsm_fingerprint(control) != CONTROL_SHA256 \
            or fsm_fingerprint(by_name[DAMAGES_FSM]) != DAMAGES_SHA256:
        raise ValueError('unverified Plant Trap FSM variant')
    check_transitions('Plant Trap', control, TRANSITIONS, [])
    for (name, action), expected in ACTIONS.items():
        check_action('Plant Trap', state(control, name), action, expected)
    return control


def _detector(sc, gid, origin):
    """The `Detector` child: a trigger box on the Knight-only layer whose FSM tells the trap."""
    children = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        tid = child['m_PathID']
        kid = sc.transforms[tid]['m_GameObject']['m_PathID']
        children[sc.gos[kid]['m_Name']] = (kid, tid)
    if sorted(children) != ['Detector', 'Ready Grass']:
        raise ValueError('unsupported Plant Trap children: ' + ', '.join(sorted(children)))
    kid, tid = children['Detector']
    records = _component_records(sc, kid)
    boxes = [d for _, t, d in records if t == 'BoxCollider2D']
    fsms = [d['fsm'] for _, t, d in records if t == 'PlayMakerFSM']
    if not sc.active(kid) or sc.gos[kid]['m_Layer'] != 13 or len(boxes) != 1 or len(fsms) != 1 \
            or not boxes[0]['m_Enabled'] or not boxes[0]['m_IsTrigger'] or boxes[0]['m_EdgeRadius'] != 0 \
            or fsm_fingerprint(fsms[0]) != DETECT_SHA256:
        raise ValueError('unsupported Plant Trap Detector')
    matrix = sc.world(tid)
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('rotated or scaled Plant Trap Detector')
    box = boxes[0]
    x, y = matrix[0][3] - origin[0], matrix[1][3] - origin[1]
    actual = [_q16(x + box['m_Offset']['x'] - box['m_Size']['x'] / 2),
              _q16(y + box['m_Offset']['y'] - box['m_Size']['y'] / 2),
              _q16(x + box['m_Offset']['x'] + box['m_Size']['x'] / 2),
              _q16(y + box['m_Offset']['y'] + box['m_Size']['y'] / 2)]
    if actual != DETECT_Q16:
        raise ValueError(f'Plant Trap Detector box {actual} is not the admitted {DETECT_Q16}')


def _clips(source, sc, actor):
    animator = actor['tk2dSpriteAnimator']
    if not animator['m_Enabled'] or animator['isRealtime'] or animator['playAutomatically']:
        raise ValueError('Plant Trap requires enabled scaled-time animation it starts itself')
    library_o = source.ref(sc.file, animator['library'])
    library = source.read(library_o)
    clips = {c['name']: c for c in library['clips'] if c['name']}
    collections = {}
    for name, (frames, fps, wrap, loop_start) in CLIPS.items():
        clip = clips.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode'], clip.get('loopStart', 0)) \
                != (frames, fps, wrap, loop_start) or any(f.get('triggerEvent') for f in clip['frames']):
            raise ValueError('unsupported Plant Trap animation: ' + name)
        for index, frame in enumerate(clip['frames']):
            collection_o = source.ref(library_o.assets_file, frame['spriteCollection'])
            if collection_o.path_id not in collections:
                collections[collection_o.path_id] = source.read(collection_o)
            definition = collections[collection_o.path_id]['spriteDefinitions'][frame['spriteId']]
            wanted = COLLIDERS.get(name, {}).get(index)
            if definition['physicsEngine'] != 1:
                raise ValueError('Plant Trap sprite is not a 2D physics sprite: ' + name)
            if wanted is None:
                if definition['colliderType'] == 2 and name in ('Snap', 'Retract'):
                    raise ValueError(f'unexpected Plant Trap collider: {name} frame {index}')
                if name in ('Snap Ready', 'Idle') and definition['colliderType'] == 2:
                    raise ValueError(f'unexpected Plant Trap collider: {name} frame {index}')
                continue
            vertices = definition['colliderVertices']
            centre, half = vertices[0], vertices[1]
            actual = [_q16(centre['x'] - half['x']), _q16(centre['y'] - half['y']),
                      _q16(centre['x'] + half['x']), _q16(centre['y'] + half['y'])]
            if definition['colliderType'] != 2 or actual != wanted:
                raise ValueError(f'Plant Trap collider {name} frame {index} {actual} is not the admitted {wanted}')
    return library_o


def recognize(sc, actor):
    """A rooted trap that snaps shut when the Knight stands over it."""
    source = sc.source
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Plant Trap methods require a fresh source audit: ' + name)
    gid = actor['game_object']
    records = _component_records(sc, gid)
    _fsms(records)
    if sorted(kind for _, kind, _ in records) != COMPONENTS:
        raise ValueError('unsupported Plant Trap component set')
    if sc.gos[gid]['m_Layer'] != 11:
        raise ValueError(f'Plant Trap outside the enemy layer: layer {sc.gos[gid]["m_Layer"]}')
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('unsupported Plant Trap rotation or scale')
    if len(actor['position']) < 3 or abs(actor['position'][2]) > .01:
        raise ValueError('Plant Trap depth differs from guest source plane')
    _detector(sc, gid, actor['position'][:2])
    health = actor['health_manager']
    if health['hp'] != 16 or health['smallGeoDrops'] != 9 or any(health[key] for key in (
            'invincible', 'invincibleFromDirection', 'hasSpecialDeath', 'hasAlternateHitAnimation',
            'damageOverride', 'megaFlingGeo', 'mediumGeoDrops', 'largeGeoDrops')):
        raise ValueError('unsupported Plant Trap HealthManager variant')
    _, damage = _one(records, 'DamageHero')
    if damage['damageDealt'] != 1 or damage['hazardType'] != 1 or not damage['m_Enabled']:
        raise ValueError('unsupported Plant Trap contact damage')
    sprite = actor['tk2dSprite']
    if sprite['_color'] != {'r': 1., 'g': 1., 'b': 1., 'a': 1.} or sprite['_scale'] != {'x': 1., 'y': 1., 'z': 1.} \
            or sprite['boxCollider2D']['m_PathID'] or sprite['polygonCollider2D']:
        raise ValueError('unsupported Plant Trap sprite')
    library_o = _clips(source, sc, actor)
    # The spec needs some box for its near test and the spawn: the open jaws' first frame.
    return {
        'kind': 'PlantTrap', 'guest_enabled': True, 'bounds_q16': COLLIDERS['Snap'][0],
        'fsm_sha256': {CONTROL_FSM: CONTROL_SHA256, DAMAGES_FSM: DAMAGES_SHA256},
        'assemblies_sha256': dict(ASSEMBLIES), 'library_source': source.sid(library_o),
        'art_bindings': {'walk': 'Snap Ready', 'turn': 'Snap Ready', 'ready': 'Snap Ready', 'snap': 'Snap',
                         'retract': 'Retract'},
        'limitations': [
            'The hurt box is the collider tk2d builds for the frame showing, read from the sprite'
            ' definitions; contact is box overlap, as for every other actor.',
            '`damages_enemy` never meets another enemy; the Ready Grass puff, the snap sound and the'
            ' death puffs are not presented.',
            'At rest it shows the last Retract frame, the sprite it is authored with and returns to.'],
    }
