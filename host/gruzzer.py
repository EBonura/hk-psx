"""Recognize the verified Gruzzer (Fly) `Bouncer Control` variant for guest admission.

The controller lives in shared/hk-sim/src/gruzzer.rs; this module admits only
placed instances whose FSM parameters and body match .hkpsx/gruzzer/CONTRACT.md.
"""
from runner import ASSEMBLIES, _one, axis_aligned_bounds
from focus import action_fields
import hashlib

CLIPS = {'Fly': (7, 10, 0)}
BODY_SIZE = (0.890625, 0.84375)
BODY_OFFSET = (0.0390625, -0.03125)
RANGES = {'Aim': (0., 360.), 'Up Right': (320., 350.), 'Up Left': (190., 220.), 'Down Right': (10., 40.),
          'Up Left 2': (140., 170.), 'Right Down': (190., 220.), 'Down Right 2': (140., 170.),
          'Left Down': (320., 350.), 'Left Up': (10., 40.)}
TRANSITIONS = {
    'Initialise': [('FINISHED', 'Aim')], 'Aim': [('FINISHED', 'Left or Right?')],
    'Left or Right?': [('LEFT', 'Face Left'), ('RIGHT', 'Face Right')],
    'Face Left': [('FINISHED', 'Fly 2')], 'Face Right': [('FINISHED', 'Fly 2')],
    'Fly 2': [('BONK DOWN', 'Hit Down'), ('BONK LEFT', 'Hit Left'), ('BONK RIGHT', 'Hit Right'), ('BONK UP', 'Hit Up')],
    'Hit Up': [('RIGHT', 'Up Right'), ('LEFT', 'Up Left')], 'Hit Down': [('RIGHT', 'Down Right'), ('LEFT', 'Up Left 2')],
    'Hit Right': [('UP', 'Down Right 2'), ('DOWN', 'Right Down')], 'Hit Left': [('UP', 'Left Up'), ('DOWN', 'Left Down')],
}


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def _scalar(value):
    if isinstance(value, dict):
        return value['name'] if value.get('useVariable') else value['value']
    return value


def recognize(sc, actor):
    from actors import _component_records
    source = sc.source
    records = _component_records(sc, actor['game_object'])
    # Gruz Mother's reserve (Crossroads_04 `Fly Spawn/Fly`..`Fly 6`) waits
    # below the room until the burster moves `Fly Spawn` to itself. It is
    # admitted as `GruzzerReserve`: seated with the scene, parked (no tick, no
    # draw, no hit) until the release, which is the Hatcher cage's pattern.
    # Any other Fly outside the room is refused: position is the honest
    # discriminator, as for the Hatcher's parked arena copies (hatcher.py).
    from hatcher import scene_bounds
    bounds = scene_bounds(sc)
    x, y = actor['position'][:2]
    origin = None
    if not (bounds[0] <= x <= bounds[2] and bounds[1] <= y <= bounds[3]):
        origin = reserve_origin(sc, actor['game_object'])
        if origin is None:
            raise ValueError('Gruzzer parked outside the room is not a Fly Spawn reserve')
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Gruzzer methods require a fresh source audit: ' + name)
    fsms = [d['fsm'] for _, t, d in records if t == 'PlayMakerFSM']
    if len(fsms) != 1 or fsms[0]['name'] != 'Bouncer Control' or fsms[0]['startState'] != 'Initialise':
        raise ValueError('no single Gruzzer Bouncer Control FSM')
    fsm = fsms[0]
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}
    if not _near(variables.get('Speed'), 5.2) or variables.get('Start Up') != 0 or variables.get('Starts Inactive') != 0:
        raise ValueError('unsupported Gruzzer variables')
    states = {st['name']: st for st in fsm['states']}
    for name, transitions in TRANSITIONS.items():
        if name not in states or [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != transitions:
            raise ValueError('unsupported Gruzzer transitions: ' + name)
    for state, (low, high) in RANGES.items():
        data = states[state]['actionData']
        matches = [i for i, n in enumerate(data['actionNames']) if n.endswith('RandomFloat') and data['actionEnabled'][i]]
        if len(matches) != 1:
            raise ValueError('unsupported Gruzzer aim: ' + state)
        fields = action_fields(data, matches[0])
        if not _near(_scalar(fields['min']), low) or not _near(_scalar(fields['max']), high) or _scalar(fields['storeResult']) != 'Angle':
            raise ValueError('unsupported Gruzzer aim range: ' + state)
    data = states['Initialise']['actionData']
    compare = [action_fields(data, i) for i, n in enumerate(data['actionNames']) if n.endswith('FloatCompare')]
    if len(compare) != 1 or not _near(_scalar(compare[0]['float2']), 44.) or compare[0]['lessThan'] != 'FINISHED':
        raise ValueError('unsupported Gruzzer camera range')
    data = states['Fly 2']['actionData']
    names = [n.rsplit('.', 1)[-1] for i, n in enumerate(data['actionNames']) if data['actionEnabled'][i]]
    if names[:2] != ['FaceDirection', 'SetVelocityAsAngle'] or names.count('CheckCollisionSide') != 3 or names.count('CheckCollisionSideEnter') != 3:
        raise ValueError('unsupported Gruzzer flight actions')
    face = action_fields(data, 0)
    if any(_scalar(face[k]) for k in ('spriteFacesRight', 'playNewAnimation', 'pauseBetweenTurns')):
        raise ValueError('unsupported Gruzzer facing')
    gid = actor['game_object']
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError('unsupported Gruzzer initial rotation or scale')
    _, body = _one(records, 'BoxCollider2D')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Gruzzer body collider')
    _, rigid = _one(records, 'Rigidbody2D')
    if rigid['m_BodyType'] != 0 or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Gruzzer rigid body')
    _, recoil = _one(records, 'Recoil')
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 15 or not _near(recoil['recoilDuration'], .15) or recoil['preventRecoilUp']:
        raise ValueError('unsupported Gruzzer recoil variant')
    _, animator = _one(records, 'tk2dSpriteAnimator')
    library = source.read(source.ref(sc.file, animator['library']))
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Gruzzer animation: ' + name)
    bounds = axis_aligned_bounds([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]],
                                 list(BODY_OFFSET), list(BODY_SIZE))
    if origin is not None:
        return {'kind': 'GruzzerReserve', 'guest_enabled': True, 'speed': 5.2, 'body_bounds_local': bounds,
                'origin': [round(v * 65536) for v in origin], 'art_bindings': {'walk': 'Fly', 'turn': 'Fly'},
                'limitations': ['Parked below the room with no tick, draw or hit until Gruz Mother\'s burster'
                                ' releases it at itself plus its offset from Fly Spawn; then an ordinary Gruzzer.',
                                'Each death decrements the arena\'s Battle Enemies (HealthManager.battleScene).']}
    return {'kind': 'Gruzzer', 'guest_enabled': True, 'speed': 5.2, 'body_bounds_local': bounds,
            'limitations': ['Gravity-free dynamic body on the bounded terrain solver; bonk sides come from the blocked solver axis instead of the three 0.08 contact rays.',
                            'The live buzz loop audio is not presented; SetZ depth randomization and FSMActivator staggering are ignored.',
                            'The breaker corpse smashes on its third landing without break pieces and does not spin.']}


# --- Gruz Mother (`Giant Fly`, Crossroads_04) and its reserve flies ---------
#
# The controller is shared/hk-sim/src/gruz_mother.rs and the arena is
# shared/hk-sim/src/boss.rs's `Battle Control`. This half is above the
# cook_inputs divider and kept small, the way host/mawlek.py is:
#
# * `recognize_giant_fly` admits the one `Giant Fly` into the guest actor pool.
#   Every clip is cooked by host/false_knight_art.py's Gruz Mother bank (below
#   the divider) into the scene's own bank; the generic actor bank only needs
#   a stand-in clip for the two slots every ActorSpec carries.
# * `reserve_origin` answers whether a `Fly` parked below the room is one of
#   the seven `Fly Spawn` children the burster releases, which `recognize`
#   admits as `GruzzerReserve` placements instead of refusing them.
#
# A state or child that is missing, or a HealthManager variant the guest does
# not model, refuses the cook rather than seating a half-understood boss.

GIANT_FLY_SCENE = 'Crossroads_04'
GIANT_FLY_FILE = 'level40'
GIANT_FLY_NAME = 'Giant Fly'
GIANT_FLY_FSMS = {'bouncer_control', 'Big Fly Control'}
GIANT_FLY_STATES = ('Init', 'Invincible', 'Sleep', 'Wake Sound', 'Wake', 'Fly', 'Buzz', 'Super Choose', 'Charge Antic',
          'Charge', 'Charge Recover L', 'Charge Recover R', 'Charge Recover U', 'Charge Recover D', 'Recover End',
          'Super End', 'Slam Antic', 'Check Direction', 'Go Left', 'Go Right', 'Launch Up', 'Launch Down',
          'Flying', 'Turn Left', 'Turn Right', 'Slam Down', 'Slam Up', 'Slam End')
GIANT_FLY_CHILDREN = ('Hero Damager', 'Snore', 'Battle Range')
SPAWN_NAME = 'Fly Spawn'
# The generic actor bank binds one real one-frame clip for the two slots every
# ActorSpec carries (the library has no blank clip); host/gruz_mother_art.py
# cooks every clip the fight plays into its own bank.
GIANT_FLY_ART = {'walk': 'Charge', 'turn': 'Charge'}


def _giant_fly_fsms(sc, gid):
    from actors import _component_records
    return {data['fsm']['name']: data for _, kind, data in _component_records(sc, gid) if kind == 'PlayMakerFSM'}


def _giant_fly_children(sc, gid):
    tid = sc.go_transform[gid]
    return {sc.gos[t['m_GameObject']['m_PathID']]['m_Name']: t['m_GameObject']['m_PathID']
            for t in sc.transforms.values() if t['m_Father']['m_PathID'] == tid}


def giant_fly(sc):
    """The scene's one `Giant Fly`, or None."""
    found = [gid for gid, go in sc.gos.items() if go['m_Name'] == GIANT_FLY_NAME and sc.active(gid)]
    if len(found) > 1:
        raise ValueError('more than one Giant Fly in the scene')
    return found[0] if found else None


def reserve_origin(sc, gid):
    """`Fly Spawn`'s world position if `gid` is one of its children in a scene
    with a Gruz Mother, else None."""
    parent = sc.transforms[sc.go_transform[gid]]['m_Father']['m_PathID']
    if not parent:
        return None
    spawn = sc.transforms[parent]['m_GameObject']['m_PathID']
    if sc.gos[spawn]['m_Name'] != SPAWN_NAME or giant_fly(sc) is None:
        return None
    # `Spawn Flies 2` SetPosition on `Fly Spawn` is in world space, so its own
    # parent (if any) does not change where the flies land.
    return sc.point(spawn)[:2]


def recognize_giant_fly(sc, actor):
    """Admit the placed Gruz Mother, or refuse with the reason."""
    gid = actor['game_object']
    if sc.gos[gid]['m_Name'] != GIANT_FLY_NAME or sc.gos[gid]['m_Layer'] != 11:
        raise ValueError('not the Giant Fly on the enemy layer')
    fsms = _giant_fly_fsms(sc, gid)
    if set(fsms) != GIANT_FLY_FSMS or not all(f['m_Enabled'] for f in fsms.values()):
        raise ValueError('unsupported Giant Fly FSM set: ' + ', '.join(sorted(fsms)))
    control = fsms['Big Fly Control']['fsm']
    states = {state['name'] for state in control['states']}
    missing = [name for name in GIANT_FLY_STATES if name not in states]
    if missing:
        raise ValueError('Big Fly Control lacks ' + ', '.join(missing))
    children = _giant_fly_children(sc, gid)
    missing = [name for name in GIANT_FLY_CHILDREN if name not in children]
    if missing:
        raise ValueError('Giant Fly lacks ' + ', '.join(missing))
    health = actor['health_manager']
    # Serialized invincible; `Sleep` clears it once the hero is in range.
    if not health['invincible'] or health['hasSpecialDeath'] or health['damageOverride'] \
            or health['invincibleFromDirection'] or health['hasAlternateHitAnimation']:
        raise ValueError('unsupported Giant Fly HealthManager variant')
    matrix = sc.world(sc.go_transform[gid])
    if abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6 or matrix[0][0] <= 0:
        raise ValueError('Giant Fly is rotated or authored mirrored')
    return {
        'kind': 'GruzMother', 'guest_enabled': True, 'art_bindings': dict(GIANT_FLY_ART),
        # The corpse prefab has no Rigidbody2D and runs its own FSM; the
        # controller and host/gruz_mother_art.py own it.
        'no_corpse': True,
        'initial_direction': -1,
        'fsm_sha256': hashlib.sha256(repr(sorted(
            (s['name'], tuple(s['actionData']['actionNames'])) for s in control['states'])).encode()).hexdigest(),
        'limitations': [
            'Every clip, the corpse and the burster are cooked by host/gruz_mother_art.py into this scene '
            'alone; the ActorSpec clip fields point at the one-frame Charge clip.',
            'CheckCollisionSide is the blocked axis of the bounded terrain step, not three rays a side.',
            'The snore zzz, the dust, slam effects, rocks, blood and steam particles are not presented.',
        ],
    }
