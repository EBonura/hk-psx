"""Recognize the Hatcher and its cage of Hatcher Babies for guest admission.

The controllers live in shared/hk-sim/src/hatcher.rs; this module admits only
placements matching docs/HATCHER.md.

The Hatcher is the first family in this port that needs another actor to change
state while the game is running. It is not a spawner in the allocating sense:
the source parks a fixed cage of babies per scene and `Fire` moves one of them
to the Hatcher and unparents it, and the baby's own `Death` puts it back. So the
pool is a cook-time reservation. Every baby a Hatcher can ever release owns a
guest actor slot before the scene loads, which is what makes the bounded 32-slot
pool an invariant rather than something a release has to check.

Two bounds decide whether a scene's family is admitted at all, and both are
scene-wide, because `regions.py::postpack_actor_bank` builds one actor bank per
scene and every region of that scene carries it:

- the 32-slot guest pool (`hk_sim::actors::MAX_ACTORS`), and
- the 20 animation slots a frame may bind (`enemies.rs::MAX_VISIBLE`), which
  `prepare_draws` asserts on. Every Hatcher and baby frame measures inside one
  64x64 slot, so a scene's actor count is its worst-case tile count.

A scene that does not fit has its whole family refused and reported. Truncating
a cage would silently change how many babies that Hatcher can ever release,
which is a difficulty change nothing downstream could see.
"""
from focus import action_fields
from runner import ASSEMBLIES, axis_aligned_bounds, body_box
from quality import SCENE_TABLE
import hashlib

# `Extra Tag`, which is what `Initiate`'s FindGameObject looks the cage up by.
CAGE_TAG = 20054
# Guest bounds this family has to fit inside, both scene-wide.
POOL_SLOTS = 32
FRAME_SLOTS = 20
# `enemies.rs::RELEASES`: releases the runtime can carry on one frame, which
# bounds the Hatchers a scene may hold because each one fires at most once.
MAX_HATCHERS_PER_SCENE = 4

BODY_SIZE = (1.3125, 1.84375)
BODY_OFFSET = (-0.125, -0.171875)
BABY_BODY_SIZE = (0.359375, 0.375)
BABY_BODY_OFFSET = (-0.0234375, -0.03125)
# `Alert Range New`: a 0.5 circle under a uniform 15.608528137207031 scale.
ALERT_RADIUS = 0.5 * 15.608528137207031
# name: (frames, fps, wrapMode, loopStart)
CLIPS = {'Fly': (6, 12., 1, 2), 'Fire': (8, 15., 2, 0)}
BABY_CLIPS = {'Fly': (10, 12., 1, 2), 'Death': (5, 18., 2, 0)}
# Every FSM on the object, so a placement carrying one this port does not run
# stays an unadmitted record instead of running without it. `Remove on battle
# start` (Crossroads_22's Hatcher and cage) only acts when that room's arena
# starts, which this port never runs, so it is ignored, as the Aspid
# recognizer ignores it on the same room's Aspids; the family budget decides.
FSMS = ('Hatcher', 'flyer_receive_direction_msg')
BABY_FSMS = ('Control', 'flyer_receive_direction_msg')
IGNORED_FSMS = ('Remove on battle start',)
TRANSITIONS = {
    'Initiate': [('FINISHED', 'Idle')],
    'Idle': [('ALERT', 'Distance Fly')],
    'Distance Fly': [('WAIT', 'Hatched Max Check')],
    'Hatched Max Check': [('TRUE', 'Distance Fly'), ('FALSE', 'Fire Anticipate')],
    'Fire Anticipate': [('WAIT', 'Fire')],
    'Fire': [('WAIT', 'Distance Fly'), ('CANCEL', 'Distance Fly')],
}
BABY_TRANSITIONS = {
    'Init': [('FINISHED', 'Inert')],
    'Inert': [('SPAWN', 'Chase')],
    'Chase': [('CENTIPEDE DEATH', 'Death')],
    'Death': [('FINISHED', 'Inert')],
}
ACTIONS = {
    ('Idle', 'IdleBuzz'): {'waitMin': .75, 'waitMax': 1., 'speedMax': 1.75,
                           'accelerationMax': 15., 'roamingRange': 1.},
    ('Idle', 'FaceDirection'): {'spriteFacesRight': False, 'playNewAnimation': False,
                                'everyFrame': True, 'pauseBetweenTurns': False, 'pauseTime': 0.},
    ('Distance Fly', 'DistanceFly'): {'distance': 6., 'speedMax': 3.5, 'acceleration': .1,
                                      'targetsHeight': True, 'height': 3.5},
    ('Distance Fly', 'FaceObject'): {'spriteFacesRight': False, 'playNewAnimation': False, 'everyFrame': True},
    ('Distance Fly', 'WaitRandom'): {'timeMin': 2., 'timeMax': 3., 'finishEvent': 'WAIT'},
    ('Hatched Max Check', 'IntCompare'): {'integer2': 0, 'equal': 'TRUE', 'lessThan': 'TRUE',
                                          'greaterThan': 'FALSE', 'everyFrame': False},
    ('Fire Anticipate', 'Tk2dPlayAnimation'): {'clipName': 'Fire'},
    ('Fire Anticipate', 'Wait'): {'time': .335, 'finishEvent': 'WAIT'},
    ('Fire', 'FloatAdd'): {'add': -1., 'everyFrame': False, 'perSecond': False},
    ('Fire', 'SendEventByName'): {'sendEvent': 'SPAWN', 'delay': 0.},
    ('Fire', 'Tk2dWatchAnimationEvents'): {'animationCompleteEvent': 'WAIT'},
}
BABY_ACTIONS = {
    ('Chase', 'FaceDirection'): {'spriteFacesRight': False, 'playNewAnimation': False,
                                 'everyFrame': True, 'pauseBetweenTurns': True, 'pauseTime': .4},
    ('Chase', 'ChaseObject'): {'speedMax': 5., 'acceleration': .1, 'targetSpread': 1.5,
                               'spreadResetTimeMin': 1., 'spreadResetTimeMax': 2.},
    ('Death', 'SetHP'): {'hp': 5},
    ('Death', 'SetDamageHeroAmount'): {'damageDealt': 1},
    ('Death', 'SetIsDead'): {'setValue': False},
    ('Death', 'SetPosition'): {'x': 0., 'y': 0., 'everyFrame': False, 'lateUpdate': False},
}


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def _scalar(value):
    if isinstance(value, dict):
        return value['name'] if value.get('useVariable') else value['value']
    return value


def _check_assemblies(source, who):
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError(f'{who} actions require a fresh source audit: ' + name)


def _fsm(records, name, who):
    """The named FSM, and a refusal if the object carries one this port skips."""
    present = sorted(d['fsm']['name'] for _, t, d in records
                     if t == 'PlayMakerFSM' and d['fsm']['name'] not in IGNORED_FSMS)
    expected = sorted(FSMS if name == 'Hatcher' else BABY_FSMS)
    if present != expected:
        raise ValueError(f'unsupported {who} FSM set: ' + ', '.join(present))
    return next(d['fsm'] for _, t, d in records if t == 'PlayMakerFSM' and d['fsm']['name'] == name)


def _check_states(fsm, transitions, actions, who):
    states = {state['name']: state for state in fsm['states']}
    for name, expected in transitions.items():
        if name not in states or [(t['fsmEvent']['name'], t['toState'])
                                  for t in states[name]['transitions']] != expected:
            raise ValueError(f'unsupported {who} transitions: ' + name)
    for (state, action), expected in actions.items():
        data = states[state]['actionData']
        matches = [i for i, n in enumerate(data['actionNames'])
                   if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]]
        if len(matches) != 1:
            raise ValueError(f'unsupported {who} action set: {state}/{action}')
        fields = action_fields(data, matches[0])
        for key, value in expected.items():
            actual = _scalar(fields.get(key))
            if isinstance(value, float) and isinstance(actual, (int, float)) and not isinstance(actual, bool):
                if not _near(actual, value):
                    raise ValueError(f'unsupported {who} parameter: {state}/{action}.{key}')
            elif actual != value:
                raise ValueError(f'unsupported {who} parameter: {state}/{action}.{key}')
    return states


def _check_clips(source, sc, actor, expected, who):
    animator = actor['tk2dSpriteAnimator']
    if not animator['m_Enabled']:
        raise ValueError(f'{who} animator disabled')
    library_object = source.ref(sc.file, animator['library'])
    library = source.read(library_object)
    clips = {clip['name']: clip for clip in library['clips'] if clip['name']}
    for name, want in expected.items():
        clip = clips.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode'],
                            clip.get('loopStart', 0)) != want:
            raise ValueError(f'unsupported {who} animation: ' + name)
    return library_object


def _check_body(sc, actor, records, size, offset, who):
    gid = actor['game_object']
    if sc.gos[gid]['m_Layer'] != 11:
        raise ValueError(f'{who} outside the enemy layer')
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)):
        raise ValueError(f'unsupported {who} rotation or scale')
    box = body_box(records, who)
    if not box['m_Enabled'] or box['m_IsTrigger'] or box['m_EdgeRadius'] != 0 \
            or not all(_near(box['m_Size'][k], v) for k, v in zip('xy', size)) \
            or not all(_near(box['m_Offset'][k], v) for k, v in zip('xy', offset)):
        raise ValueError(f'unsupported {who} body collider')
    bodies = [d for _, t, d in records if t == 'Rigidbody2D']
    if len(bodies) != 1:
        raise ValueError(f'{who} requires exactly one Rigidbody2D')
    rigid = bodies[0]
    if rigid['m_BodyType'] != 0 or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 \
            or rigid['m_Constraints'] != 4:
        raise ValueError(f'unsupported {who} rigid body')
    damage = [d for _, t, d in records if t == 'DamageHero']
    if len(damage) != 1 or not damage[0]['m_Enabled'] or damage[0]['damageDealt'] != 1:
        raise ValueError(f'unsupported {who} contact damage')
    return axis_aligned_bounds([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]],
                               list(offset), list(size))


def scene_bounds(sc):
    """The scene's own runtime bounds, which is what makes a Hatcher a placement.

    Crossroads_22 parks four `Hatcher NP` copies under `Hatcher Summon` objects
    well outside the room, to be summoned into its arena. They are as real as
    the placed one structurally, so position is the honest discriminator: an
    enemy the room does not contain is not standing anywhere the player goes.
    """
    name = getattr(sc.file, 'name', '')
    for info in SCENE_TABLE:
        if name.endswith('/' + info['file']) or name == info['file']:
            return info['runtime_bounds']
    raise ValueError('Hatcher placement outside the admitted scene table')


def cage(sc):
    """The scene's single `Extra Tag` cage and the babies parked in it."""
    cages = [gid for gid, go in sc.gos.items() if go.get('m_Tag') == CAGE_TAG and sc.active(gid)]
    if len(cages) != 1:
        raise ValueError(f'Hatcher scene needs exactly one Extra Tag cage, found {len(cages)}')
    gid = cages[0]
    children = []
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        kid = sc.transforms[child['m_PathID']]['m_GameObject']['m_PathID']
        if sc.active(kid):
            children.append(kid)
    return gid, children


def _others(sc):
    """Supported actors of this scene that are not part of the Hatcher family.

    The cage is parked outside every region envelope, so `actor_sources` only
    enumerates it on the scene-wide call while `cook.py` passes bounds. Both
    calls have to reach the same admission or the per-region pack would cook art
    for a Hatcher the scene bank then drops, so the budget reads the scene
    rather than whichever list `actor_sources` happens to be building.

    The sentinel makes the family refuse itself for the duration of the probe,
    which is what stops the recursion and what makes the count exclude it.
    """
    counted = getattr(sc, 'hatcher_others', None)
    if counted is None:
        sc.hatcher_others = -1
        from actors import actor_sources
        sc.hatcher_others = sum(1 for a in actor_sources(sc) if a['movement_supported'])
        counted = sc.hatcher_others
    if counted < 0:
        raise ValueError('Hatcher family excluded while its own scene budget is measured')
    return counted


def placed_hatchers(sc):
    """Live Hatcher bodies standing inside the room, by name and position."""
    bounds = scene_bounds(sc)
    owners = {tree['m_GameObject']['m_PathID'] for kind, tree in sc.objects.values()
              if kind == 'HealthManager' and tree['m_Enabled']}
    found = []
    for gid, go in sc.gos.items():
        name = go['m_Name']
        if not name.startswith('Hatcher') or name.startswith('Hatcher Baby') or name.startswith('Hatcher Cage'):
            continue
        if gid not in owners or not sc.active(gid):
            continue
        x, y = sc.point(gid)[:2]
        if bounds[0] <= x <= bounds[2] and bounds[1] <= y <= bounds[3]:
            found.append(gid)
    return found


def family_budget(sc):
    """Refuse the scene's whole family unless the cage fits both guest bounds."""
    _, children = cage(sc)
    hatchers = placed_hatchers(sc)
    if len(hatchers) > MAX_HATCHERS_PER_SCENE:
        raise ValueError(f'{len(hatchers)} Hatchers in one scene exceeds the {MAX_HATCHERS_PER_SCENE}'
                         ' releases the guest carries on a frame')
    total = _others(sc) + len(hatchers) + len(children)
    if total > POOL_SLOTS:
        raise ValueError(f'Hatcher cage of {len(children)} needs {total} of the {POOL_SLOTS} guest'
                         ' actor slots this scene has')
    if total > FRAME_SLOTS:
        raise ValueError(f'Hatcher cage of {len(children)} needs {total} of the {FRAME_SLOTS}'
                         ' animation slots a frame binds')
    return len(children)


def recognize(sc, actor):
    """The Hatcher itself: a gravity-free flyer that releases its cage."""
    source = sc.source
    from actors import _component_records
    _check_assemblies(source, 'Hatcher')
    records = _component_records(sc, actor['game_object'])
    bounds = scene_bounds(sc)
    x, y = actor['position'][:2]
    if not (bounds[0] <= x <= bounds[2] and bounds[1] <= y <= bounds[3]):
        raise ValueError('Hatcher parked outside the room is arena content, not a placement')
    fsm = _fsm(records, 'Hatcher', 'Hatcher')
    if fsm['startState'] != 'Initiate':
        raise ValueError('Hatcher begins in an unsupported state')
    if fsm.get('globalTransitions'):
        raise ValueError('unsupported Hatcher global transitions')
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}
    if variables.get('startAlert') not in (0, 1):
        raise ValueError('unsupported Hatcher start alert')
    states = _check_states(fsm, TRANSITIONS, ACTIONS, 'Hatcher')
    # `Hatched Max` is the disabled cap; the live gate is the cage child count,
    # so the one thing that matters is that `GetChildCount` still reads the cage.
    cage_reads = [i for i, n in enumerate(states['Hatched Max Check']['actionData']['actionNames'])
                  if n.endswith('GetChildCount') and states['Hatched Max Check']['actionData']['actionEnabled'][i]]
    if len(cage_reads) != 1:
        raise ValueError('Hatcher no longer counts its cage before firing')
    spawn = [i for i, n in enumerate(states['Fire']['actionData']['actionNames'])
             if n.endswith('GetRandomChild') and states['Fire']['actionData']['actionEnabled'][i]]
    if len(spawn) != 1:
        raise ValueError('Hatcher no longer draws its shot from the cage')
    velocity = action_fields(states['Fire']['actionData'],
                             next(i for i, n in enumerate(states['Fire']['actionData']['actionNames'])
                                  if n.endswith('SetVelocity2d') and states['Fire']['actionData']['actionEnabled'][i]))
    if not _near(_scalar(velocity.get('y')), -5.):
        raise ValueError('unsupported Hatcher release velocity')
    body = _check_body(sc, actor, records, BODY_SIZE, BODY_OFFSET, 'Hatcher')
    recoils = [d for _, t, d in records if t == 'Recoil']
    if len(recoils) != 1 or recoils[0]['freezeInPlace'] or recoils[0]['recoilSpeedBase'] != 20 \
            or not _near(recoils[0]['recoilDuration'], .15) or recoils[0]['preventRecoilUp']:
        raise ValueError('unsupported Hatcher recoil variant')
    circles = {}
    for child in sc.transforms[sc.go_transform[actor['game_object']]]['m_Children']:
        kid = sc.transforms[child['m_PathID']]['m_GameObject']['m_PathID']
        for _, kind, data in _component_records(sc, kid):
            if kind == 'CircleCollider2D':
                circles[sc.gos[kid]['m_Name']] = data['m_Radius'] * sc.transforms[child['m_PathID']]['m_LocalScale']['x']
    if not _near(circles.get('Alert Range New', 0), ALERT_RADIUS):
        raise ValueError('unsupported Hatcher alert range')
    library_object = _check_clips(source, sc, actor, CLIPS, 'Hatcher')
    reserved = family_budget(sc)
    return {'kind': 'Hatcher', 'guest_enabled': True, 'body_bounds_local': body, 'no_corpse': True,
            'start_alert': bool(variables.get('startAlert')), 'cage_reserved': reserved,
            'library_source': source.sid(library_object),
            # Neither FaceDirection nor FaceObject plays a clip on this
            # placement, so the turn slot holds Fly the way the Gruzzer's does.
            'art_bindings': {'walk': 'Fly', 'turn': 'Fly', 'fire': 'Fire'},
            'limitations': [
                f'The cage is reserved at cook time: {reserved} guest actor slots this scene always'
                ' holds, parked, so a release can never fail for want of one.',
                'The cage is shared by every Hatcher in the scene, as the source shares it.',
                'GetRandomChild picks uniformly; the guest releases the first parked member.',
                'Gravity-free dynamic body on the bounded terrain solver; 50 Hz fixed steps from a 60 Hz accumulator.',
                'The corpse is Corpse Hatcher v2, a CorpseHatcher rather than the plain Corpse the cook'
                ' recognizes, so the body is removed on death and the Burst clip is not presented.',
                'FSMActivator.activateStaggered, PersistentBoolItem, the SetZ depth, the'
                ' flyer_receive_direction_msg push, the global-pool flings, the audio pitch ramp and'
                ' every one shot are not presented.']}


def recognize_baby(sc, actor):
    """One member of a Hatcher's cage: parked until a release, recycled on death."""
    source = sc.source
    from actors import _component_records
    _check_assemblies(source, 'Hatcher Baby')
    records = _component_records(sc, actor['game_object'])
    cage_gid, children = cage(sc)
    parent = sc.transforms[sc.go_transform[actor['game_object']]]['m_Father']['m_PathID']
    if not parent or sc.transforms[parent]['m_GameObject']['m_PathID'] != cage_gid:
        raise ValueError('Hatcher Baby outside the scene cage is not a reserved pool member')
    fsm = _fsm(records, 'Control', 'Hatcher Baby')
    if fsm['startState'] != 'Init':
        raise ValueError('Hatcher Baby begins in an unsupported state')
    if fsm.get('globalTransitions'):
        raise ValueError('unsupported Hatcher Baby global transitions')
    _check_states(fsm, BABY_TRANSITIONS, BABY_ACTIONS, 'Hatcher Baby')
    body = _check_body(sc, actor, records, BABY_BODY_SIZE, BABY_BODY_OFFSET, 'Hatcher Baby')
    if any(kind == 'Recoil' for _, kind, _ in records):
        raise ValueError('Hatcher Baby with a Recoil component is a different placement')
    bounce = [d for _, t, d in records if t == 'ObjectBounce']
    if len(bounce) != 1 or not _near(bounce[0]['bounceFactor'], .3) or not _near(bounce[0]['speedThreshold'], 1.):
        raise ValueError('unsupported Hatcher Baby ObjectBounce variant')
    death = [d for _, t, d in records if t == 'EnemyDeathEffects']
    if len(death) != 1 or death[0]['corpsePrefab']['m_PathID']:
        raise ValueError('Hatcher Baby with a corpse is a different placement')
    library_object = _check_clips(source, sc, actor, BABY_CLIPS, 'Hatcher Baby')
    reserved = family_budget(sc)
    if len(children) != reserved:
        raise ValueError('Hatcher cage membership changed between reads')
    return {'kind': 'HatcherBaby', 'guest_enabled': True, 'body_bounds_local': body, 'no_corpse': True,
            'cage_reserved': reserved, 'library_source': source.sid(library_object),
            # Death is the enemy's own death effect, and the port removes the
            # body instead of playing one, so only Fly is cooked.
            'art_bindings': {'walk': 'Fly', 'turn': 'Fly'},
            'limitations': [
                'Reserved, not spawned: this actor is seated with the scene and parked in the cage'
                ' until a Hatcher releases it, which is what the source does with it too.',
                'Death recycles rather than removing: hp back to 5, the dead flag cleared and the body'
                ' returned to the cage, one frame after the kill is counted.',
                'The Death clip and the global-pool flings are not presented; the body simply leaves.',
                'No Recoil component, so the nail does not displace it; the shared recoil pass is skipped.',
                'ObjectBounce (factor .3, threshold 1) is not the terrain solver slide the body uses.',
                'EnemyDreamnailReaction pays SOUL once per scene load rather than once per life.']}
