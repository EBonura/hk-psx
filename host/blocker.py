"""Recognize the two Crossroads Blockers for guest admission.

The controller lives in shared/hk-sim/src/blocker.rs; this module admits only a
placement whose serialized shape matches the one that was read to write it.

The Blocker is the port's first turret. It carries no `Rigidbody2D`, no
`Walker`, no `Recoil` and no `DamageHero`: it sits on a `Terrain Block` child
that the world cook already lays down as static terrain, and the only things it
does are open, lob a `Shot Mawlek`, and shut. That makes it much less work than
its ten clips suggest, and it is why none of the walking machinery the Runner
and the Shield need appears below.

Two placements are admitted and they differ in exactly one authored value:

- Crossroads_11_alt (`level50:4585`) authors `Unalert Range` false and carries
  an `Unalert Range` child with a room-sized trigger and an FSM that writes the
  bool. That Blocker can return to `Dormant`.
- Crossroads_ShamanTemple (`level76:14853`) authors the same bool true and the
  child is a bare `Transform`. Nothing can ever clear it, so that Blocker never
  sleeps again once it has opened.

Both halves are checked, because the bool alone would admit a placement whose
trigger child had been stripped without the authored value being updated, and
that one really would sleep the instant it opened.

The geometry is deliberately not carried in the `ActorSpec`. Every admitted
Blocker has byte-identical child transforms and colliders and a mirrored root,
so the three trigger boxes are constants in `blocker.rs` and this module proves
the placement's own world boxes against them, the way `aspid.py` proves its
alert radii. That keeps `ActorController` inside the Zombie Shield's 56 bytes
and `ActorSpec` at 152, which every enemy type in the world pays.
"""
from pathlib import Path
import hashlib

from focus import action_fields
from runner import ASSEMBLIES, axis_aligned_bounds, body_box, fsm_fingerprint

FSM_NAME = 'Blocker Control'
# Structural fingerprint of `Blocker Control`, per placement. The two differ in
# one variable, `Unalert Range`, which is the whole of the difference between
# the two Blockers, so the pair is keyed by what that variable means rather than
# by scene: the hash is the evidence and `sleeps` is what it proves.
FSM_SHA256 = {
    # Crossroads_11_alt: the bool starts false and the trigger child drives it.
    '32194e421ca1a5ab7e2f1421620b0e2b534e0c2849edf2b9fde9d96e8a3a6c64': True,
    # Crossroads_ShamanTemple: the bool is authored true and nothing lowers it.
    '854257702c33b7f1441e6c4a530f488a3512f11e752476cacc7407c5e5ba4dd2': False,
}
# The whole component set, so a placement carrying anything this port does not
# run stays an unadmitted record. There is no `DamageHero` here: the Blocker
# deals no contact damage and the hero can stand on it.
COMPONENTS = sorted([
    'AudioSource', 'BoxCollider2D', 'EnemyDeathEffects', 'EnemyDreamnailReaction',
    'ExtraDamageable', 'HealthManager', 'InfectedEnemyEffects', 'MeshFilter', 'MeshRenderer',
    'PersistentBoolItem', 'PersonalObjectPool', 'PlayMakerFSM', 'PlayMakerFixedUpdate',
    'SpriteFlash', 'Transform', 'tk2dSprite', 'tk2dSpriteAnimator'])
BODY_SIZE = (2.78125, 3.15625)
BODY_OFFSET = (0.5625, -0.640625)
# name: (frames, fps, wrapMode). Death and Death Stun are in the library and are
# deliberately absent: see `no_corpse` below.
CLIPS = {
    'Idle': (7, 12., 0), 'Closed': (1, 30., 0), 'Open': (4, 15., 2),
    'Close1': (2, 15., 2), 'Close2': (4, 15., 2),
    'Shoot Antic': (3, 15., 2), 'Shoot CD': (4, 15., 2), 'Hit': (7, 12., 2),
}
# `blocker::Clip::slot()` order. Idle and Closed are the shared `ActorSpec`
# slots and are not in it.
CLIP_SLOTS = ('open', 'close1', 'close2', 'antic', 'cooldown', 'hit')
SLOT_CLIPS = {'open': 'Open', 'close1': 'Close1', 'close2': 'Close2',
              'antic': 'Shoot Antic', 'cooldown': 'Shoot CD', 'hit': 'Hit'}
# `Shot Mawlek`'s own Idle/Impact, which the pooled projectile plays.
SHOT_CLIPS = {'Idle': (4, 20., 0), 'Impact': (6, 20., 2)}
SHOT_NAME = 'Shot Mawlek'
SHOT_GRAVITY = .6
# Every child `Init`'s four `FindChild` calls look up, plus the two that only
# exist to be looked at: the terrain the Blocker stands on and the close puff.
CHILDREN = ('Alert Range New', 'Attack Range', 'Spit Effect', 'Unalert Range',
            'Terrain Block', 'Pt Close')
# The three sense boxes, relative to the actor origin in Q16, exactly as
# `shared/hk-sim/src/blocker.rs` carries them. A placement whose own world boxes
# differ is refused rather than run against somebody else's geometry.
ALERT_Q16 = [-37122, -226637, 957772, 512589]
ATTACK_Q16 = [-162202, -229293, 427134, 733486]
UNALERT_Q16 = [-785266, -652750, 1873773, 771067]
# `Right`'s `Shot Origin` and `X Speed Min`/`X Speed Max`, and `Shot Y Speed`.
SHOT_ORIGIN_Q16 = [180879, 51118]
SHOT_VX_Q16 = [196608, 983040]
SHOT_VY_Q16 = 1310720
# `Idle`'s `WaitRandom(0.8, 1.2)` in ticks, which blocker.rs holds as IDLE_TICKS.
IDLE_TICKS = [48, 72]
# The `Corpse Blocker` this family's `EnemyDeathEffects` names. Read rather than
# assumed: the Runner's contract would have refused it for the wrong reason.
CORPSE_NAME = 'Corpse Blocker'
CORPSE_PLAYER_DATA = 'Blocker'


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def _scalar(value):
    """A compact FSM parameter's literal, or its variable name if it has one."""
    if isinstance(value, dict):
        return value['name'] if value.get('useVariable') else value['value']
    return value


def _assemblies(source):
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Blocker actions require a fresh source audit: ' + name)


def _fsm(records):
    """The one FSM on the object, and which of the two placements it is."""
    matches = [data for _, typ, data in records if typ == 'PlayMakerFSM']
    if len(matches) != 1 or matches[0]['fsm']['name'] != FSM_NAME:
        present = ', '.join(sorted(d['fsm']['name'] for _, t, d in records if t == 'PlayMakerFSM'))
        raise ValueError('unsupported Blocker FSM set: ' + (present or 'none'))
    component = matches[0]
    fingerprint = fsm_fingerprint(component['fsm'])
    if not component['m_Enabled'] or fingerprint not in FSM_SHA256:
        raise ValueError('unverified Blocker FSM variant')
    return component['fsm'], fingerprint, FSM_SHA256[fingerprint]


def _children(sc, gid):
    result = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        tid = child['m_PathID']
        kid = sc.transforms[tid]['m_GameObject']['m_PathID']
        result[sc.gos[kid]['m_Name']] = (kid, tid)
    return result


def _trigger_box(sc, children, name, origin, expected):
    """One `AlertRange`-style trigger child, as a Q16 box around the actor."""
    from actors import _component_records
    if name not in children:
        raise ValueError('Blocker is missing its ' + name + ' child')
    gid, tid = children[name]
    if not sc.active(gid):
        raise ValueError('inactive Blocker ' + name + ' child')
    boxes = [d for _, t, d in _component_records(sc, gid) if t == 'BoxCollider2D']
    if len(boxes) != 1 or not boxes[0]['m_Enabled'] or not boxes[0]['m_IsTrigger'] \
            or boxes[0]['m_EdgeRadius'] != 0:
        raise ValueError('unsupported Blocker ' + name + ' trigger')
    box = boxes[0]
    world = axis_aligned_bounds(sc.world(tid), [box['m_Offset'][k] for k in 'xy'],
                                [box['m_Size'][k] for k in 'xy'])
    actual = [round((v - origin[i % 2]) * 65536) for i, v in enumerate(world)]
    if actual != expected:
        raise ValueError(f'Blocker {name} box {actual} is not the admitted {expected}')
    return actual


def _alert_range(sc, children, name):
    """`CheckAlertRangeByName` reads the child's own `AlertRange` component."""
    from actors import _component_records
    gid, _ = children[name]
    alerts = [d for _, t, d in _component_records(sc, gid) if t == 'AlertRange']
    if len(alerts) != 1 or not alerts[0]['m_Enabled']:
        raise ValueError('unsupported Blocker ' + name + ' AlertRange component')


def _unalert(sc, children, origin, sleeps):
    """The `Unalert Range` child, which is the two placements' one difference.

    A Blocker that sleeps has the trigger and the FSM that writes the bool; one
    that does not has a bare `Transform` under the same name. Both are checked
    against what the FSM variable claims, because either half alone would admit
    a placement whose behaviour had silently changed.
    """
    from actors import _component_records
    if 'Unalert Range' not in children:
        raise ValueError('Blocker is missing its Unalert Range child')
    gid, _ = children['Unalert Range']
    kinds = sorted(t for _, t, _ in _component_records(sc, gid))
    if not sleeps:
        if kinds != ['Transform']:
            raise ValueError('Blocker authors Unalert Range true but carries ' + ', '.join(kinds))
        return None
    if kinds != ['BoxCollider2D', 'PlayMakerFSM', 'Transform']:
        raise ValueError('unsupported Blocker Unalert Range child: ' + ', '.join(kinds))
    fsm = next(d['fsm'] for _, t, d in _component_records(sc, gid) if t == 'PlayMakerFSM')
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}
    # The trigger FSM is generic: it writes whatever bool its own variables
    # name, on whatever FSM they name, so those are what decide that this child
    # drives this Blocker's own gate rather than something else's.
    if variables.get('FSM Name') != FSM_NAME or variables.get('Bool Name') != 'Unalert Range':
        raise ValueError('Blocker Unalert Range trigger writes a different gate')
    return _trigger_box(sc, children, 'Unalert Range', origin, UNALERT_Q16)


def _shot(source, sc, fsm):
    """`Goop`'s `Shot Mawlek`, the projectile `Fire` takes from the pool."""
    state = next(st for st in fsm['states'] if st['name'] == 'Goop')
    data = state['actionData']
    assigns = [i for i, n in enumerate(data['actionNames'])
               if n.rsplit('.', 1)[-1] == 'SetGameObject' and data['actionEnabled'][i]]
    if len(assigns) != 1:
        raise ValueError('Blocker Goop no longer assigns a single projectile')
    start = data['actionStartIndex'][assigns[0]]
    end = (data['actionStartIndex'][assigns[0] + 1] if assigns[0] + 1 < len(data['actionNames'])
           else len(data['paramName']))
    refs = [data['fsmGameObjectParams'][data['paramDataPos'][k]] for k in range(start, end)
            if data['paramDataType'][k] == 19]
    literal = [r['value'] for r in refs if not r['useVariable'] and r['value']['m_PathID']]
    if len(literal) != 1:
        raise ValueError('Blocker shot prefab reference missing')
    obj = source.ref(sc.file, literal[0])
    go = source.read(obj)
    if go['m_Name'] != SHOT_NAME:
        raise ValueError('unsupported Blocker shot prefab: ' + go['m_Name'])
    parts = {}
    for ref in go['m_Component']:
        component = source.ref(obj.assets_file, ref['component'])
        parts[source.typename(component)] = (component, source.read(component))
    for kind in ('Rigidbody2D', 'BoxCollider2D', 'DamageHero', 'EnemyBullet',
                 'tk2dSpriteAnimator', 'tk2dSprite', 'Transform'):
        if kind not in parts:
            raise ValueError('unsupported Blocker shot component set')
    if not _near(parts['Rigidbody2D'][1]['m_GravityScale'], SHOT_GRAVITY) \
            or parts['DamageHero'][1]['damageDealt'] != 1:
        raise ValueError('unsupported Blocker shot body')
    # The guest pool carries the Aspid bullet's box for every projectile that is
    # not a barrel, so a shot with a different one would be given the wrong
    # hitbox rather than its own.
    box = parts['BoxCollider2D'][1]
    if not _near(box['m_Size']['x'], .640625) or not _near(box['m_Size']['y'], .5625):
        raise ValueError('Blocker shot box differs from the pooled projectile box')
    library_o = source.ref(obj.assets_file, parts['tk2dSpriteAnimator'][1]['library'])
    library = source.read(library_o)
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in SHOT_CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Blocker shot animation: ' + name)
    return {'source': source.sid(obj), 'library': source.sid(library_o), 'library_object': library_o,
            'scale': parts['Transform'][1]['m_LocalScale']['x'] * parts['EnemyBullet'][1]['scaleMin']}


def _launch(fsm):
    """`Direction` -> `Right`, which is the only branch an admitted Blocker takes.

    Facing here is the FSM's own `Facing Right` variable and not the placement's
    transform: `SetVector3XYZ` writes a world-space `Shot Origin` that
    `SpawnObjectFromGlobalPool` adds to the spawn point's position without
    consulting a transform, and the `X Speed` pair is signed the same way. So a
    mirrored root moves the sprite and nothing else.
    """
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v
                 and not isinstance(v['value'], dict)}
    if variables.get('Facing Right') != 1:
        raise ValueError('a left-facing Blocker takes the unaudited Left launch branch')
    if not _near(variables.get('Shot Y Speed', 0), SHOT_VY_Q16 / 65536):
        raise ValueError('unsupported Blocker Shot Y Speed')
    state = next(st for st in fsm['states'] if st['name'] == 'Right')
    data = state['actionData']
    written = {}
    for index, name in enumerate(data['actionNames']):
        if not data['actionEnabled'][index] or not name.endswith('SetFloatValue'):
            continue
        fields = action_fields(data, index)
        target, value = fields.get('floatVariable'), fields.get('floatValue')
        if not isinstance(target, dict) or not target.get('useVariable') \
                or not isinstance(value, dict) or value.get('useVariable'):
            raise ValueError('unsupported Blocker Right launch assignment')
        written[target['name']] = value['value']
    speeds = [round(written.get(key, 0) * 65536) for key in ('X Speed Min', 'X Speed Max')]
    if speeds != SHOT_VX_Q16:
        raise ValueError(f'Blocker launch speeds {speeds} are not the admitted {SHOT_VX_Q16}')
    origins = [i for i, n in enumerate(data['actionNames'])
               if n.endswith('SetVector3XYZ') and data['actionEnabled'][i]]
    if len(origins) != 1:
        raise ValueError('Blocker Right no longer writes a single Shot Origin')
    fields = action_fields(data, origins[0])
    written = [_scalar(fields.get(key, 0.)) for key in 'xyz']
    if any(isinstance(value, str) for value in written):
        raise ValueError('Blocker Shot Origin is written from a variable')
    origin = [round(float(value) * 65536) for value in written[:2]]
    if origin != SHOT_ORIGIN_Q16:
        raise ValueError(f'Blocker Shot Origin {origin} is not the admitted {SHOT_ORIGIN_Q16}')
    if not _near(written[2], 0):
        raise ValueError('Blocker Shot Origin leaves the guest source plane')


def _idle_wait(fsm):
    """`Idle`'s `WaitRandom`, which is the whole of the Blocker's fire rate."""
    data = next(st for st in fsm['states'] if st['name'] == 'Idle')['actionData']
    waits = [i for i, n in enumerate(data['actionNames'])
             if n.endswith('WaitRandom') and data['actionEnabled'][i]]
    if len(waits) != 1:
        raise ValueError('Blocker Idle no longer carries a single WaitRandom')
    fields = action_fields(data, waits[0])
    ticks = [round(float(fields[key]['value']) * 60) for key in ('timeMin', 'timeMax')]
    if any(fields[key]['useVariable'] for key in ('timeMin', 'timeMax')) or ticks != IDLE_TICKS:
        raise ValueError(f'Blocker idle wait {ticks} is not the admitted {IDLE_TICKS}')


def _clips(source, sc, actor):
    animator = actor['tk2dSpriteAnimator']
    if not animator['m_Enabled'] or animator['isRealtime']:
        raise ValueError('Blocker requires enabled scaled-time animation')
    library_object = source.ref(sc.file, animator['library'])
    library = source.read(library_object)
    clips = {clip['name']: clip for clip in library['clips'] if clip['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = clips.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Blocker animation: ' + name)
        # The guest's clip clock is the cooked frame count; a source trigger
        # frame would mean the FSM acts partway through one, which none does.
        if any(frame.get('triggerEvent') for frame in clip['frames']):
            raise ValueError('unsupported Blocker frame event: ' + name)
        if clip.get('loopStart', 0) != 0:
            raise ValueError('unsupported Blocker loop start: ' + name)
    return library_object


def _corpse(source, sc, records):
    """What `EnemyDeathEffects` actually names, which decides `no_corpse`.

    `Corpse Blocker` is not a `Corpse` at all: no `Corpse` component, no
    `Rigidbody2D`, no collider and no `ObjectBounce`. It is a static prop with
    its own `corpse` FSM that plays `Death` where the Blocker stood and then
    blackens. Nothing in `effects.py`'s corpse contract can express that, so the
    guest removes the body and this says so rather than letting the cook guess
    from the family name.
    """
    deaths = [d for _, t, d in records if t == 'EnemyDeathEffects']
    if len(deaths) != 1 or not deaths[0]['m_Enabled']:
        raise ValueError('Blocker needs exactly one enabled EnemyDeathEffects')
    death = deaths[0]
    if death['playerDataName'] != CORPSE_PLAYER_DATA:
        raise ValueError('unvalidated Blocker kill counter: ' + str(death['playerDataName']))
    obj = source.ref(sc.file, death['corpsePrefab'])
    go = source.read(obj)
    if go['m_Name'] != CORPSE_NAME:
        raise ValueError('unvalidated Blocker corpse prefab: ' + go['m_Name'])
    kinds = {source.typename(source.ref(obj.assets_file, ref['component'])) for ref in go['m_Component']}
    if 'Corpse' in kinds or 'Rigidbody2D' in kinds:
        raise ValueError('Blocker corpse is a falling Corpse after all; wire it rather than dropping it')
    return source.sid(obj)


# Scenes whose tightest view cannot hold this family's palettes, with the
# measured figure that refused them. The per-view CLUT budget is only knowable
# after the atlas cook and the similarity postpass, which run long after this
# recognizer, so a scene that overflows is recorded here rather than discovered
# as a failed build. Crossroads_ShamanTemple came to 427 of 416 in region 661
# with the Blocker's 26 palettes in its bank, and 401 without them.
#
# This is a per-scene list and not a rule, which is honest about what it is: the
# general answer is for the actor bank to drop an actor and record the omission
# when the postpass finds the view over budget, the way append_actor_art already
# does for a frame that will not fit an animation slot.
#
# Nothing is refused today: the Blocker's clips now cook one palette per clip
# (`shared_palette` below, cook.py `add_frames_shared`) instead of one per
# frame, which is what brought the Ancestral Mound under the budget.
CLUT_REFUSED = {}


def recognize(sc, actor):
    """A stationary shell that opens, lobs a goop and shuts again."""
    source = sc.source
    over = CLUT_REFUSED.get(Path(sc.file.name).stem)
    if over:
        raise ValueError('the scene cannot hold the Blocker art: ' + over)
    from actors import _component_records
    _assemblies(source)
    gid = actor['game_object']
    records = _component_records(sc, gid)
    if sorted(kind for _, kind, _ in records) != COMPONENTS:
        raise ValueError('unsupported Blocker component set')
    fsm, fingerprint, sleeps = _fsm(records)
    if fsm['startState'] != 'Pause':
        raise ValueError('Blocker begins in an unsupported state')
    globals_ = [(t['fsmEvent']['name'], t['toState']) for t in fsm.get('globalTransitions', [])]
    if globals_ != [('TOOK DAMAGE', 'Hit Pause')]:
        raise ValueError('unsupported Blocker global transitions: ' + repr(globals_))
    if sc.gos[gid]['m_Layer'] != 11:
        raise ValueError(f'Blocker outside the enemy layer: layer {sc.gos[gid]["m_Layer"]}')
    if len(actor['position']) < 3 or abs(actor['position'][2]) > .01:
        raise ValueError('Blocker depth differs from guest source plane')
    matrix = sc.world(sc.go_transform[gid])
    # Both admitted placements are mirrored, which is the pose the constant
    # sense boxes were measured in. A placement with the opposite basis would
    # need every box flipped and the `Left` launch branch, neither of which is
    # audited, so it stays an unadmitted record.
    if matrix[0][0] >= 0 or abs(abs(matrix[0][0]) - 1) > 1e-6 or abs(matrix[1][1] - 1) > 1e-6 \
            or abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6:
        raise ValueError('unsupported Blocker layer or initial scale')
    box = body_box(records, 'Blocker')
    if not box['m_Enabled'] or box['m_IsTrigger'] or box['m_EdgeRadius'] != 0 \
            or not all(_near(box['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(box['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Blocker body collider')
    children = _children(sc, gid)
    missing = [name for name in CHILDREN if name not in children]
    if missing:
        raise ValueError('Blocker is missing children: ' + ', '.join(missing))
    origin = actor['position'][:2]
    _trigger_box(sc, children, 'Alert Range New', origin, ALERT_Q16)
    _trigger_box(sc, children, 'Attack Range', origin, ATTACK_Q16)
    for name in ('Alert Range New', 'Attack Range'):
        _alert_range(sc, children, name)
    _unalert(sc, children, origin, sleeps)
    # `Terrain Block` is a layer 8 child the world geometry cook already lays
    # down as static terrain, independently of whether this actor is admitted.
    # It is checked here only so that a placement whose solid footprint differs
    # from its body box is not admitted quietly; see `limitations`.
    terrain_gid, terrain_tid = children['Terrain Block']
    terrain = [d for _, t, d in _component_records(sc, terrain_gid) if t == 'BoxCollider2D']
    if sc.gos[terrain_gid]['m_Layer'] != 8 or len(terrain) != 1 or terrain[0]['m_IsTrigger'] \
            or terrain[0]['m_Size'] != box['m_Size'] or terrain[0]['m_Offset'] != box['m_Offset']:
        raise ValueError('unsupported Blocker Terrain Block')
    _launch(fsm)
    _idle_wait(fsm)
    shot = _shot(source, sc, fsm)
    corpse = _corpse(source, sc, records)
    library_object = _clips(source, sc, actor)
    return {
        'kind': 'Blocker', 'guest_enabled': True, 'no_corpse': True,
        # One CLUT per clip rather than per frame (host/cook.py add_frames_shared):
        # the whole shell draws from a handful of greys and the eye's orange.
        'shared_palette': True,
        'fsm_sha256': fingerprint, 'assemblies_sha256': dict(ASSEMBLIES),
        'library_source': source.sid(library_object),
        'sleeps': sleeps, 'shot': shot, 'corpse_source': corpse,
        # `Blocker Control` never turns and never moves; the mirror the sprite
        # is drawn with is the whole of what the transform contributes.
        'initial_direction': 1 if matrix[0][0] < 0 else -1,
        'art_bindings': dict({'walk': 'Idle', 'turn': 'Closed'},
                             **{slot: SLOT_CLIPS[slot] for slot in CLIP_SLOTS}),
        'limitations': [
            'The Roller half of `Attack Choose` is presented once PlayerData `fireballLevel` is'
            ' above 0 (host/blocker_roller.py), but the 50/50 is only drawn then: the source draws'
            ' it every attack and discards it at `Can Roller?`, which only shifts which draws'
            ' land where. The spell cheat does not raise `fireballLevel`, so it keeps the goop.',
            '`Corpse Blocker` is not presented: it carries no `Corpse` component, no rigid body and'
            ' no collider, so it is a static prop with its own `corpse` FSM rather than anything the'
            " cook's corpse contract can express. The body is removed on death and the Death and"
            ' Death Stun clips are not cooked.',
            'The `Terrain Block` child is cooked as static world terrain, so the Blocker stays solid'
            ' after it dies, where the source destroys the whole GameObject and takes the block with'
            ' it. That footprint is world geometry rather than part of this actor.',
            'Shots are the shared projectile pool rather than this object\'s own'
            ' `PersonalObjectPool` reserve of two, so more than two goops could in principle be'
            ' live at once; the fire cycle is long enough that the source limit is not reached.',
            'The shot flies under gravity .6 without the source stretch, rotation or EnemyBullet'
            ' scale jitter, and ends on the first terrain or hero contact.',
            '`Hit Pause` is one `NextFrameEvent` before `Hit` and is not reproduced, and the'
            ' `BLOCKER DAMAGED` it sends to a host FSM goes nowhere in the source either.',
            'The idle wait and the shot speed are per-actor deterministic samples of the source'
            ' `WaitRandom(0.8, 1.2)` and `RandomFloat(3, 15)`, not Unity RNG parity.',
            'The `Spit Effect` child, the `Pt Close` particle emitter, the goop spatter'
            ' `FlingObjectsFromGlobalPool` burst, the `ObjectJitter` shake in `Hit`, the audio'
            ' snapshot transition and every one shot are not presented.'],
    }
