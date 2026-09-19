"""Recover the False Knight (Crossroads_10_boss) and its arena from the Windows source.

The boss controller lives in shared/hk-sim/src/false_knight.rs and the reusable
arena lifecycle in shared/hk-sim/src/boss.rs; this module is the source side.
Every number the guest uses is read back out of the serialized `FalseyControl`,
`Check Health`, `Health Check`, `Battle Control`, `summon` and `BG Control`
state machines and asserted against the tables below, so a source or cooker
change fails loudly instead of leaving the guest running stale constants.

The False Knight does not live in Crossroads_10 (`level46`). `BossLoader`, a
SceneAdditiveLoadConditional in that room, loads `Crossroads_10_boss`
(`level48`) while `falseKnightDefeated` is false and `Crossroads_10_boss_defeated`
(`level49`) afterwards. Neither additive scene is in host/quality.py's
SCENE_TABLE, so nothing in the port cooks the boss today.

`measure_animation` is the reason the guest is not wired up: see
docs/FALSE_KNIGHT.md. The measurement is reproduced here rather than quoted so
that a later art-path change is checked against the source, not against prose.
"""
import hashlib
import json
import math
import struct
from pathlib import Path

from combat import ticks
from runner import ASSEMBLIES

SCENE_FILE = 'level48'
ROOM_FILE = 'level46'
# BossLoader in Crossroads_10; the alternative scene carries the defeated arena.
ADDITIVE = {'scene': 'Crossroads_10_boss', 'alt_scene': 'Crossroads_10_boss_defeated',
            'gate_flag': 'falseKnightDefeated', 'gate_value': 0}

# HealthManager. The body restores to `Check Health.Recover HP`, which
# `Start Fall` overwrites with the live HealthManager hp, after every stagger.
BODY_HEALTH = 65
HEAD_HEALTH = 40
BODY_INVULNERABLE_TIME = .25
HEAD_INVULNERABLE_TIME = .15
CONTACT_DAMAGE = 1
# Both HealthManagers drop no Geo: the reward is the PlayerData set below plus
# the journal entry. Nothing in FalseyControl reads its own `Geo Pool` child.
GEO_DROPS = {'small': 0, 'medium': 0, 'large': 0}
PLAYER_DATA = {
    # Death Anim Start, before the corpse sequence.
    'falseKnightDefeated': True,
    # Open Map Shop and Journal, after the head is killed for the third time.
    'openedMapperShop': True, 'corn_crossroadsLeft': True,
    'killedFalseKnight': True, 'newDataFalseKnight': True, 'killsFalseKnight': 0,
    # Roll End / Pause Long: the long first plop only plays once per save.
    'falseKnightFirstPlop': True,
}
STAGGERS_TO_DEATH = 3
RAGE_SLAMS = 8

# FalseyControl clip inventory, shared by the body, the Hitter overlay, the Head
# and the Death Head: (frames, fps, wrapMode, loopStart).
CLIPS = {
    'Idle': (5, 12., 0, 0), 'Jump Antic': (3, 10., 2, 0), 'Land': (5, 10., 2, 0),
    'Jump': (4, 12., 0, 0), 'Attack Antic': (6, 12., 1, 4), 'Turn': (2, 12., 2, 0),
    'Jump Attack Up': (5, 12., 2, 0), 'Jump Attack Hit 1': (2, 12., 2, 0),
    'Jump Attack Hit 2': (2, 12., 2, 0), 'Jump Attack Hit 3': (2, 12., 2, 0),
    'Attack': (3, 15., 2, 0), 'Attack Recover': (5, 12., 2, 0), 'Blank': (1, 30., 6, 0),
    'Run Antic': (2, 12., 2, 0), 'Run': (5, 12., 1, 1), 'Stun Roll': (5, 12., 1, 2),
    'Stun Roll End': (4, 12., 2, 0), 'Stun Open': (4, 12., 2, 0), 'Stun Hit': (3, 12., 2, 0),
    'Stun Recover': (6, 12., 2, 0), 'Rage': (5, 12., 2, 0), 'Death Fall': (3, 12., 1, 1),
    'Head Idle': (5, 12., 0, 0), 'Death Land': (5, 12., 2, 0), 'Head Hit': (8, 12., 1, 3),
    'Death Head 1': (10, 10., 2, 0), 'Death Head 2': (4, 10., 2, 0),
    'Death Spaz': (3, 12., 0, 0), 'Body': (1, 30., 6, 0), 'Mace Emerge': (16, 12., 2, 0),
    'Mace Leave': (4, 12., 0, 0), 'Stun Opened': (1, 12., 2, 0), 'Head Spaz': (3, 12., 0, 0),
    'Mace Roll': (7, 20., 2, 0),
}
# Every serialized `Wait`/`WaitRandom` the controller actually reaches, in
# seconds, keyed by the state that owns it.
WAITS = {
    'JA Recoil 2': .08399999886751175, 'JA Slam': .08299999684095383,
    'S Attack Antic': 1.2000000476837158, 'S Attack': .11999999731779099,
    'Opened': 5., 'Pause Short': 1.2000000476837158, 'Pause Long': 2.5,
    'Idle Pause': .5, 'R Attack Antic': .699999988079071, 'Rage': .24899999797344208,
    'Rage End': .75, 'Particle Pause': .10000000149011612, 'Stun Land': .5,
    'Death Land': 2., 'Death Anim Start': 1., 'Steam': 3., 'Ready': 1.,
    'Death Head Land': 1.5, 'First Idle': 1.5,
}
# The clips the controller plays, in the order shared/hk-sim's `Clip` declares
# them, so `CLIP_TICKS` can be checked entry by entry.
CLIP_ORDER = (
    'Idle', 'Turn', 'Jump Antic', 'Jump', 'Land', 'Run', 'Run Antic', 'Jump Attack Up',
    'Jump Attack Hit 1', 'Jump Attack Hit 2', 'Jump Attack Hit 3', 'Attack Antic', 'Attack',
    'Attack Recover', 'Rage', 'Stun Roll', 'Stun Roll End', 'Stun Open', 'Stun Opened',
    'Stun Hit', 'Stun Recover', 'Death Fall', 'Death Land', 'Death Spaz', 'Blank',
)
# `To Phase 2` / `To Phase 3` rewrite these; index 0 is the FSM's own defaults.
# (idle wait min, idle wait max, jump barrels min/max, slam barrels min/max)
PHASES = (
    {'idle': (1., 1.), 'jump_barrels': (0, 0), 'slam_barrels': (0, 0)},
    {'idle': (.800000011920929, 1.), 'jump_barrels': (2, 3), 'slam_barrels': (2, 3)},
    {'idle': (.800000011920929, 1.), 'jump_barrels': (2, 2), 'slam_barrels': (3, 4)},
)
# Gravity scales the controller sets on its own dynamic body, whose serialized
# Rigidbody2D gravity scale is 0.
GRAVITY = {'idle': .38999998569488525, 'jump': .125, 'jump_attack': .11999999731779099,
           'rage_jump': .30000001192092896, 'death_jump': .20000000298023224, 'stun': 1.}
# Jump launch speeds: SetVelocity2d y in each launching state.
JUMP_SPEED_Y = {'Jump': 90., 'JA Jump': 90., 'S Jump': 105., 'Jump 2': 105.,
                'JA Jump 2': 90., 'Stun Start': 20., 'Floor Break': 15.}
# Rise/Fall shape the vertical speed every FixedUpdate instead of using gravity.
RISE_MULTIPLIER = .8500000238418579
FALL_MULTIPLIER = 1.149999976158142
RUN_SPEED = 14.
RUN_STOP_DISTANCE = 14.
RUN_TRIGGER_DISTANCE = 21.
STUN_ROLL_SPEED = 10.
STUN_ROLL_STOP_SPEED = 3.
# Move Choice: three equally weighted branches once the hero is within 21.
MOVE_WEIGHTS = {'SMASH': 1., 'JUMP ATTACK': 1., 'JUMP': 1.}
TURNS_BEFORE_ATTACK = 3
JUMP_ATTACKS_IN_A_ROW = 3   # Row Check returns when JA In A Row is already above this
SLAMS_IN_A_ROW = 2          # S Check Hero Pos returns when Slam In A Row is above this
# Jump X selection.
TOWARDS_FACTOR = .8999999761581421
TOWARDS_CLAMP = 12.
HERO_X_CLAMP = (15., 42.)
RANDOM_JUMP_RANGE = (-10., 10.)
RANDOM_JUMP_MIN = 5.
WALL_RAY_DISTANCE = 8.
WALL_RAY_LAYER = 8
JA_OFFSET = 3.
JA_RECOIL_SPEED = 3.
JA_ANTIC_FACTOR = .5799999833106995
SLAM_OVERSHOOT = (12., 18.)
SLAM_SKIP_JUMP_DISTANCE = 12.
SHOCKWAVE_X_ORIGIN = 5.5
SHOCKWAVE_Y_ORIGIN = -5.800000190734863
SHOCKWAVE_SPEED = 22.
RAGE_POINT_X = 28.899999618530273
RAGE_JUMP_FACTOR = 1.0499999523162842
FINAL_POINT_X = 34.
FINAL_JUMP_FACTOR = .7599999904632568
FALL_RAY_DISTANCE = 9.5
# Body scale magnitude; Turn L/R and the death jump write its sign.
BODY_SCALE = 1.2999999523162842
# Local BoxCollider2D size/offset, before the 1.3 transform scale.
BODY_BOX = ((3.359375, 3.875), (.0546875, -2.359375))
HITTER_BOX = ((5.921875, 2.546875), (3.2265625, -3.0234375))
HEAD_BOX = ((1.600000023841858, 1.7100000381469727), (0., -.2199999988079071))
TERRAIN_BLOCK_BOX = ((2.5744729042053223, 2.051547050476074), (.37306493520736694, -2.8299999237060547))

# Battle Control: the arena lifecycle every later boss reuses.
ARENA_END_WAIT = 2.
ARENA_TRIGGER_BOX = ((1., 19.592126846313477), (0., 2.913707733154297))
ARENA_CAMERA_LOCK = (21.600000381469727, 31.3700008392334, 33.83000183105469, 31.3700008392334)
BARREL_SPAWN_X = (13.180000305175781, 44.27000045776367)
BARREL_SPAWN_GAP = (.15000000596046448, .25)

# Structural digests over states, transitions, enabled actions and their
# serialized parameter bytes, excluding per-scene object references. The five
# battle gates share one BG Control apart from `Start Closed`, which is the
# placement parameter, so that variable is excluded and recorded per gate.
FSM_SHA256 = {
    'FalseyControl': '21599984415d020af3289824aa6a0d33ab51c9f2c24278e7f34ed72506328f57',
    'Check Health': '4f68e313367ba807b5659ab8b8e6e3a623a3f662acfa2b7c7c95587ced8751bc',
    'Health Check': '549ec13c4e525af9fd48ff37b462efa8a65c44c3d47cae3926ddcaccb901ad3b',
    'Battle Control': '00aad10af131295e598c57bdff094a2df8d18ed52898481d4f552ac5e780868e',
    'summon': '53bb9db6af2e5b6e9482634a91b07b0dd91bd9676ec1091e04a4ce80a07f064f',
    'BG Control': '98a0b55f39c2f01fa2e16c353d581532179c4893a03437b10cbee49b19e8ca9f',
}
GATE_PLACEMENT_VARIABLES = ('Start Closed',)

SCALAR_PARAM_TABLES = ('fsmFloatParams', 'fsmIntParams', 'fsmBoolParams', 'fsmStringParams',
                       'fsmVector2Params', 'fsmVector3Params', 'fsmColorParams', 'fsmRectParams')


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def _scalar_param(value):
    """A typed PlayMaker parameter without its per-scene object reference."""
    if not isinstance(value, dict):
        return value
    if value.get('useVariable'):
        return 'VAR:' + str(value.get('name'))
    return value.get('value')


def fsm_digest(fsm, ignore=()):
    """Structure and serialized scalar parameters of one FSM.

    `runner.fsm_fingerprint` decodes every action through `action_fields`, which
    refuses the parameter kinds FalseyControl uses (event targets, layer-mask
    arrays, function calls). This digest hashes the raw serialized action bytes
    and typed scalar tables instead, so it covers actions no decoder handles
    while still ignoring the PPtrs that differ per scene instance. `ignore`
    drops the placement variables a family legitimately varies, the way
    `runner.fsm_fingerprint` drops the Zombie Swipe `Lunge Speed`.
    """
    summary = {
        'name': fsm['name'], 'start': fsm['startState'],
        # Object variables hold per-placement PPtrs (each battle gate points at
        # its own dust emitters), so only scalar variables enter the digest.
        'variables': sorted(
            (group, v['name'], json.dumps(v['value'], sort_keys=True, default=str))
            for group, items in fsm['variables'].items() if isinstance(items, list)
            for v in items if isinstance(v, dict) and 'name' in v and 'value' in v
            and v['name'] not in ignore
            and not (isinstance(v['value'], dict) and 'm_PathID' in v['value'])),
        'globals': [(t['fsmEvent']['name'], t['toState']) for t in fsm.get('globalTransitions', [])],
        'states': [],
    }
    for state in fsm['states']:
        data = state['actionData']
        summary['states'].append({
            'name': state['name'],
            'transitions': [(t['fsmEvent']['name'], t['toState']) for t in state['transitions']],
            'actions': [(name, int(data['actionEnabled'][i])) for i, name in enumerate(data['actionNames'])],
            'starts': list(data['actionStartIndex']),
            'params': list(zip(data['paramName'], data['paramDataType'], data['paramByteDataSize'])),
            'bytes': bytes(bytearray(data['byteData'])).hex(),
            'typed': {table: [_scalar_param(v) for v in data.get(table, [])]
                      for table in SCALAR_PARAM_TABLES if table in data},
        })
    return hashlib.sha256(json.dumps(summary, sort_keys=True, default=str).encode()).hexdigest()


def _states(fsm):
    return {state['name']: state for state in fsm['states']}


def _variables(fsm):
    return {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
            for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}


def _fsms(sc, gid):
    from actors import _component_records
    return {data['fsm']['name']: data for _, kind, data in _component_records(sc, gid) if kind == 'PlayMakerFSM'}


def _box(sc, gid, index=0):
    from actors import _component_records
    boxes = [tree for _, kind, tree in _component_records(sc, gid) if kind == 'BoxCollider2D']
    if len(boxes) <= index:
        raise ValueError(f'missing BoxCollider2D on game object {gid}')
    box = boxes[index]
    return ((box['m_Size']['x'], box['m_Size']['y']), (box['m_Offset']['x'], box['m_Offset']['y']))


def _named(sc, name):
    matches = [gid for gid, go in sc.gos.items() if go['m_Name'] == name]
    if len(matches) != 1:
        raise ValueError(f'expected exactly one {name!r} in {sc.file.name}')
    return matches[0]


def _wait_seconds(state):
    """The one enabled Wait/WaitRandom time in a state, as serialized."""
    from focus import action_fields
    data = state['actionData']
    found = []
    for index, name in enumerate(data['actionNames']):
        short = name.rsplit('.', 1)[-1]
        if short not in ('Wait', 'WaitRandom') or not data['actionEnabled'][index]:
            continue
        fields = action_fields(data, index)
        if short == 'Wait':
            found.append(_scalar_param(fields['time']))
        else:
            found.append((_scalar_param(fields['timeMin']), _scalar_param(fields['timeMax'])))
    if len(found) != 1:
        raise ValueError(f'expected one Wait in state {state["name"]!r}, found {len(found)}')
    return found[0]


def recognize(sc, room):
    """Admit the one placed False Knight, or refuse with the differing field.

    `sc` is the additive boss scene (Crossroads_10_boss) and `room` the parent
    room (Crossroads_10) that owns the battle gates and the BossLoader.
    """
    source = sc.source
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('False Knight recognition requires a fresh source audit: ' + name)
    body = _named(sc, 'False Knight New')
    head = _named(sc, 'Head')
    hitter = _named(sc, 'Hitter')
    battle = _named(sc, 'Battle Scene')
    summoner = _named(sc, 'FK Barrel Summon')

    fsms = _fsms(sc, body)
    if set(fsms) != {'FalseyControl', 'Check Health'}:
        raise ValueError('unsupported False Knight FSM set: ' + ', '.join(sorted(fsms)))
    control = fsms['FalseyControl']['fsm']
    if control['startState'] != 'State 4':
        raise ValueError('False Knight starts in an unsupported state')
    for owner, name in ((body, 'FalseyControl'), (body, 'Check Health'), (head, 'Health Check'),
                        (battle, 'Battle Control'), (summoner, 'summon')):
        fsm = _fsms(sc, owner).get(name)
        if fsm is None or not fsm['m_Enabled']:
            raise ValueError(f'missing or disabled {name!r} FSM')
        digest = fsm_digest(fsm['fsm'])
        if digest != FSM_SHA256[name]:
            raise ValueError(f'unverified {name} FSM variant: {digest}')

    health = actor_health(sc, body)
    if health['hp'] != BODY_HEALTH or not _near(health['invulnerableTime'], BODY_INVULNERABLE_TIME) \
            or not health['hasSpecialDeath'] or health['invincible'] or health['damageOverride']:
        raise ValueError('unsupported False Knight HealthManager variant')
    head_health = actor_health(sc, head)
    if head_health['hp'] != HEAD_HEALTH or not _near(head_health['invulnerableTime'], HEAD_INVULNERABLE_TIME) \
            or not head_health['hasSpecialDeath']:
        raise ValueError('unsupported False Knight Head HealthManager variant')
    for record in (health, head_health):
        if (record['smallGeoDrops'], record['mediumGeoDrops'], record['largeGeoDrops']) != \
                (GEO_DROPS['small'], GEO_DROPS['medium'], GEO_DROPS['large']):
            raise ValueError('False Knight Geo drops changed; the fight paid none')

    recover = _variables(_fsms(sc, body)['Check Health']['fsm']).get('Recover HP')
    if recover != BODY_HEALTH:
        raise ValueError('Check Health restores an unexpected amount')
    head_reset = [action for action in _states(fsms['Check Health']['fsm'])['Stun']['actionData']['actionNames']]
    if not any(name.endswith('SetHP') for name in head_reset):
        raise ValueError('Check Health no longer restores the body after a stagger')

    states = _states(control)
    for name, expected in WAITS.items():
        if name not in states:
            raise ValueError('missing False Knight state: ' + name)
        actual = _wait_seconds(states[name])
        if isinstance(expected, tuple) != isinstance(actual, tuple) or (
                not _near(actual, expected) if not isinstance(expected, tuple)
                else not all(_near(a, b) for a, b in zip(actual, expected))):
            raise ValueError(f'False Knight wait changed: {name} {actual} != {expected}')

    variables = _variables(control)
    for name, expected in (('Idle Min', PHASES[0]['idle'][0]), ('Idle Max', PHASES[0]['idle'][1]),
                           ('Run Speed', -RUN_SPEED), ('Stun X Speed', STUN_ROLL_SPEED),
                           ('Rage Point X', RAGE_POINT_X), ('Final Point X', FINAL_POINT_X),
                           ('Range Min', HERO_X_CLAMP[0]), ('Range Max', HERO_X_CLAMP[1])):
        if not _near(variables.get(name, float('nan')), expected):
            raise ValueError(f'unsupported False Knight variable: {name}')
    for name in ('Jump Barrel Min', 'Jump Barrel Max', 'Slam Barrel Min', 'Slam Barrel Max',
                 'Stunned Amount', 'Rages', 'Turns', 'Jump Count', 'JA In A Row', 'Slam In A Row'):
        if variables.get(name) != 0:
            raise ValueError(f'False Knight counter does not start at zero: {name}')

    library = clip_contract(sc, body)
    boxes = {'body': _box(sc, body), 'hitter': _box(sc, hitter), 'head': _box(sc, head),
             'terrain_block': _box(sc, _named(sc, 'FK Terrain Block'))}
    for key, expected in (('body', BODY_BOX), ('hitter', HITTER_BOX), ('head', HEAD_BOX),
                          ('terrain_block', TERRAIN_BLOCK_BOX)):
        if not all(_near(a, b) for pair in zip(boxes[key], expected) for a, b in zip(*pair)):
            raise ValueError(f'unsupported False Knight collider: {key}')
    matrix = sc.world(sc.go_transform[body])
    if not _near(abs(matrix[0][0]), BODY_SCALE) or not _near(matrix[1][1], BODY_SCALE) \
            or abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6:
        raise ValueError('unsupported False Knight rotation or scale')

    return {
        'kind': 'FalseKnight', 'guest_enabled': False,
        'scene': SCENE_FILE, 'room': ROOM_FILE, 'additive': dict(ADDITIVE),
        'source': f'{Path(sc.file.name).name}:{body}',
        'position': sc.point(body), 'facing_right': matrix[0][0] > 0,
        'health': BODY_HEALTH, 'head_health': HEAD_HEALTH,
        'invulnerable_ticks': ticks(BODY_INVULNERABLE_TIME),
        'head_invulnerable_ticks': ticks(HEAD_INVULNERABLE_TIME),
        'contact_damage': CONTACT_DAMAGE, 'staggers': STAGGERS_TO_DEATH, 'rage_slams': RAGE_SLAMS,
        'phases': [dict(phase) for phase in PHASES], 'geo_drops': dict(GEO_DROPS),
        'player_data': dict(PLAYER_DATA), 'colliders_local': boxes, 'scale': BODY_SCALE,
        'clips': library, 'arena': arena_sources(sc, room),
        'fsm_sha256': {name: fsm_digest(_fsms(sc, owner)[name]['fsm'])
                       for owner, name in ((body, 'FalseyControl'), (body, 'Check Health'),
                                           (head, 'Health Check'), (battle, 'Battle Control'),
                                           (summoner, 'summon'))},
        'limitations': [
            'Guest admission is off: at the authored 1.3 scale 77 of the 110 unique animation'
            ' sprites exceed one 64x64 slot (largest 204x188). The cache holds the largest frame'
            ' now; the actor art path does not, because enemies.rs draws a frame as one quad.'
            ' The slot counts are in the animation block. See docs/FALSE_KNIGHT.md.',
            'The sim reproduces FalseyControl at 60 Hz; the source shapes rising and falling speed'
            ' per 50 Hz FixedUpdate, so airtime differs slightly.',
            'The `Esc` branch is unreachable in the Crossroads arena: nothing there sets `Hero Escaped`.',
            'Particles, camera shake, the barrel prefabs, the shockwave object and every audio cue'
            ' are recorded, not presented.',
        ],
        'source_methods': ['HealthManager.Hit', 'HealthManager.Die', 'SceneAdditiveLoadConditional.Start',
                           'PersistentBoolItem.OnEnable', 'CameraLockArea.OnTriggerEnter2D'],
    }


def actor_health(sc, gid):
    from actors import _component_records
    records = [tree for _, kind, tree in _component_records(sc, gid) if kind == 'HealthManager']
    if len(records) != 1:
        raise ValueError(f'expected one HealthManager on game object {gid}')
    return records[0]


def clip_contract(sc, gid):
    """The shared tk2d library behind the body, Hitter, Head and Death Head."""
    from actors import _component_records
    source = sc.source
    animators = [tree for _, kind, tree in _component_records(sc, gid) if kind == 'tk2dSpriteAnimator']
    if len(animators) != 1:
        raise ValueError('expected one tk2dSpriteAnimator on the False Knight')
    library_object = source.ref(sc.file, animators[0]['library'])
    library = source.read(library_object)
    by_name = {clip['name']: clip for clip in library['clips'] if clip['name']}
    if set(by_name) != set(CLIPS):
        raise ValueError('unsupported False Knight animation inventory')
    result = []
    for name, (frames, fps, wrap, loop_start) in CLIPS.items():
        clip = by_name[name]
        if (len(clip['frames']), clip['fps'], clip['wrapMode'], clip.get('loopStart', 0)) != \
                (frames, fps, wrap, loop_start):
            raise ValueError('unsupported False Knight animation: ' + name)
        result.append({'name': name, 'frames': frames, 'fps': fps, 'wrap_mode': wrap,
                       'loop_start': loop_start, 'duration_ticks': ticks(frames / fps)})
    return {'library_source': source.sid(library_object), 'clips': result}


def arena_sources(sc, room):
    """`Battle Control` plus the gates it broadcasts to, as one arena record.

    The gates live in the parent room, not the additive boss scene: the battle
    broadcasts BG CLOSE / BG OPEN / BG QUICK OPEN to every FSM in both.
    """
    battle = _named(sc, 'Battle Scene')
    control = _fsms(sc, battle)['Battle Control']['fsm']
    states = _states(control)
    end_wait = _wait_seconds(states['End Wait'])
    if not _near(end_wait, ARENA_END_WAIT):
        raise ValueError('arena end wait changed')
    trigger = _box(sc, battle)
    if not all(_near(a, b) for pair in zip(trigger, ARENA_TRIGGER_BOX) for a, b in zip(*pair)):
        raise ValueError('arena start trigger changed')
    from actors import _component_records
    persistent = [tree for _, kind, tree in _component_records(sc, battle) if kind == 'PersistentBoolItem']
    if len(persistent) != 1 or persistent[0]['semiPersistent'] or persistent[0]['dontSave']:
        raise ValueError('arena activation is no longer fully persistent')
    lock_gid = _named(sc, 'CameraLockArea B')
    lock = [tree for _, kind, tree in _component_records(sc, lock_gid) if kind == 'CameraLockArea'][0]
    camera = (lock['cameraXMin'], lock['cameraYMin'], lock['cameraXMax'], lock['cameraYMax'])
    if not all(_near(a, b) for a, b in zip(camera, ARENA_CAMERA_LOCK)):
        raise ValueError('arena camera lock changed')

    gates = []
    for sid, (kind, tree) in sorted(room.objects.items()):
        if kind != 'PlayMakerFSM' or tree['fsm']['name'] != 'BG Control':
            continue
        gid = tree['m_GameObject']['m_PathID']
        if not room.active(gid):
            continue
        digest = fsm_digest(tree['fsm'], GATE_PLACEMENT_VARIABLES)
        if digest != FSM_SHA256['BG Control']:
            raise ValueError(f'unverified BG Control variant: {digest}')
        gates.append({'source': f'{Path(room.file.name).name}:{sid}',
                      'name': room.gos[gid]['m_Name'], 'position': room.point(gid),
                      'start_closed': bool(_variables(tree['fsm']).get('Start Closed'))})
    if not gates:
        raise ValueError('no battle gates in the parent room')

    loader = [tree for _, (kind, tree) in room.objects.items() if kind == 'SceneAdditiveLoadConditional']
    if len(loader) != 1 or loader[0]['sceneNameToLoad'] != ADDITIVE['scene'] \
            or loader[0]['altSceneNameToLoad'] != ADDITIVE['alt_scene'] \
            or loader[0]['needsPlayerDataBool'] != ADDITIVE['gate_flag'] \
            or loader[0]['playerDataBoolValue'] != ADDITIVE['gate_value']:
        raise ValueError('BossLoader no longer selects the boss scene the same way')

    summoner = _named(sc, 'FK Barrel Summon')
    summon = _variables(_fsms(sc, summoner)['summon']['fsm'])
    if not _near(summon['Summon Min'], BARREL_SPAWN_X[0]) or not _near(summon['Summon Max'], BARREL_SPAWN_X[1]):
        raise ValueError('barrel summon span changed')
    return {
        'trigger_box_local': trigger, 'trigger_world': sc.point(battle),
        'end_wait_ticks': ticks(ARENA_END_WAIT), 'camera_lock': list(camera),
        'battle_enemies': 1, 'gates': gates, 'additive': dict(ADDITIVE),
        'barrel_spawn_x': list(BARREL_SPAWN_X),
        'barrel_gap_ticks': [ticks(BARREL_SPAWN_GAP[0]), ticks(BARREL_SPAWN_GAP[1])],
        'pre_battle_enemies': sorted(
            sc.gos[gid]['m_Name'] for gid, go in sc.gos.items()
            if go['m_Name'].startswith('Zombie ') and sc.active(gid)),
        'persistence': {'arena_activated': 'Battle Scene PersistentBoolItem',
                        'scene_selection': ADDITIVE['gate_flag']},
    }


def measure_animation(sc):
    """Reproduce host/cook.py's actor art path for the False Knight, exactly.

    Returns the per-clip and unique-sprite cost the cooker would produce, which
    is what decides whether the boss can be admitted at all: `Atlas.add` refuses
    a streamed frame wider or taller than 64, and every frame takes one of the
    416 CLUT slots in every region it is resident in.
    """
    from cook import tk_sprite, FOCAL, CAM_Z, SLOT_PIXELS, ANIMATION_SLOTS, MAX_FRAME_TILES
    from quality import TEXTURE_BUDGET
    from actors import _component_records
    source = sc.source
    gid = _named(sc, 'False Knight New')
    records = _component_records(sc, gid)
    animator = [tree for _, kind, tree in records if kind == 'tk2dSpriteAnimator'][0]
    sprite = [tree for _, kind, tree in records if kind == 'tk2dSprite'][0]
    matrix = sc.world(sc.go_transform[gid])
    sx = abs(matrix[0][0] * sprite['_scale']['x'])
    sy = abs(matrix[1][1] * sprite['_scale']['y'])
    projection = FOCAL / -CAM_Z
    library_object = source.ref(sc.file, animator['library'])
    library = source.read(library_object)
    textures, collections, unique, clips = {}, {}, {}, []
    for clip in library['clips']:
        if not clip['name']:
            continue
        widest = (0, 0)
        for frame in clip['frames']:
            collection_object = source.ref(library_object.assets_file, frame['spriteCollection'])
            sid = source.sid(collection_object)
            if sid not in collections:
                collections[sid] = source.read(collection_object)
            _, box = tk_sprite(source, collection_object.assets_file, collections[sid],
                               frame['spriteId'], textures)
            size = (math.ceil((box[2] - box[0]) * sx * projection),
                    math.ceil((box[3] - box[1]) * sy * projection))
            unique[(sid, frame['spriteId'])] = size
            widest = max(widest, size)
        clips.append({'name': clip['name'], 'frames': len(clip['frames']), 'largest': list(widest)})
    oversized = {key: size for key, size in unique.items() if size[0] > 64 or size[1] > 64}
    largest = max(unique.values(), key=lambda size: (size[0] * size[1], size))
    # Atlas.pack pads each streamed row to a word, so the linked-RAM cost is the
    # padded stride, not width/2.
    def frame_bytes(size):
        return (((size[0] + 3) & ~3) // 2) * size[1]

    def frame_tiles(size):
        return math.ceil(size[0] / 64) * math.ceil(size[1] / 64)

    return {
        'library_source': source.sid(library_object), 'sprite_scale': [sx, sy],
        'projection_scale': projection, 'clips': clips,
        'clip_frames': sum(clip['frames'] for clip in clips),
        'unique_sprites': len(unique), 'oversized_sprites': len(oversized),
        'largest_sprite': list(largest), 'largest_sprite_bytes': frame_bytes(largest),
        # Width and height maxima taken separately: the slot grid has to cover
        # the widest frame and the tallest frame, which are not the same sprite.
        'max_extent': [max(size[0] for size in unique.values()),
                       max(size[1] for size in unique.values())],
        'total_bytes': sum(frame_bytes(size) for size in unique.values()),
        'total_tiles_64': sum(frame_tiles(size) for size in unique.values()),
        'max_tiles_one_frame': max(frame_tiles(size) for size in unique.values()),
        'clut_slots_required': len(unique), 'texture_budget': TEXTURE_BUDGET,
        'animation_cache_slot': [SLOT_PIXELS, SLOT_PIXELS],
        'animation_cache_slots': ANIMATION_SLOTS,
        'animation_cache_max_frame_tiles': MAX_FRAME_TILES,
        'animation_cache_upload_bytes': ANIMATION_SLOTS * SLOT_PIXELS * SLOT_PIXELS // 2,
        # The cache now holds the largest frame. What still refuses the actor is
        # the actor art path itself: enemies.rs draws a frame as one quad, so
        # host/cook.py drops any actor with a frame beyond one slot, whatever
        # the tile budget says.
        'verdict': ('refused: host/cook.py drops any actor with a frame beyond '
                    f'{SLOT_PIXELS}x{SLOT_PIXELS} because enemies.rs draws one quad per frame'
                    if oversized else 'admissible against the animation cache'),
        'tiles_fit_the_cache': max(frame_tiles(size) for size in unique.values()) <= MAX_FRAME_TILES,
    }


def generated_false_knight_params():
    """The tick/Q16 constants the guest must agree with, as Rust source text.

    tests/test_false_knight.py compares this against the hand-written constants
    in shared/hk-sim/src/false_knight.rs, so the two cannot drift apart.
    """
    fields = {
        'HEALTH': BODY_HEALTH, 'HEAD_HEALTH': HEAD_HEALTH,
        'INVULNERABLE_TICKS': ticks(BODY_INVULNERABLE_TIME),
        'HEAD_INVULNERABLE_TICKS': ticks(HEAD_INVULNERABLE_TIME),
        'CONTACT_DAMAGE': CONTACT_DAMAGE, 'STAGGERS': STAGGERS_TO_DEATH, 'RAGE_SLAMS': RAGE_SLAMS,
        'STUN_WINDOW_TICKS': ticks(WAITS['Opened']),
        'TURN_TICKS': ticks(CLIPS['Turn'][0] / CLIPS['Turn'][1]),
        'JUMP_ANTIC_TICKS': ticks(CLIPS['Jump Antic'][0] / CLIPS['Jump Antic'][1]),
        'LAND_TICKS': ticks(CLIPS['Land'][0] / CLIPS['Land'][1]),
        'JA_RECOIL_TICKS': ticks(CLIPS['Jump Attack Hit 2'][0] / CLIPS['Jump Attack Hit 2'][1]),
        'JA_RECOIL2_TICKS': ticks(WAITS['JA Recoil 2']),
        'JA_END_TICKS': ticks(CLIPS['Jump Attack Hit 3'][0] / CLIPS['Jump Attack Hit 3'][1]),
        'JA_SLAM_TICKS': ticks(WAITS['JA Slam']),
        'SLAM_ANTIC_TICKS': ticks(WAITS['S Attack Antic']),
        'SLAM_STRIKE_TICKS': ticks(WAITS['S Attack']),
        'SLAM_RECOVER_TICKS': ticks(CLIPS['Attack Recover'][0] / CLIPS['Attack Recover'][1]),
        'RUN_ANTIC_TICKS': ticks(CLIPS['Run Antic'][0] / CLIPS['Run Antic'][1]),
        'ROLL_STOP_TICKS': ticks(WAITS['Stun Land']),
        'ROLL_END_TICKS': ticks(CLIPS['Stun Roll End'][0] / CLIPS['Stun Roll End'][1]),
        'PLOP_SHORT_TICKS': ticks(WAITS['Pause Short']),
        'PLOP_LONG_TICKS': ticks(WAITS['Pause Long']),
        'OPEN_TICKS': ticks(CLIPS['Stun Open'][0] / CLIPS['Stun Open'][1]),
        'STUN_HIT_TICKS': ticks(CLIPS['Stun Hit'][0] / CLIPS['Stun Hit'][1]),
        'RECOVER_TICKS': ticks(CLIPS['Stun Recover'][0] / CLIPS['Stun Recover'][1]),
        'IDLE_PAUSE_TICKS': ticks(WAITS['Idle Pause']),
        'RAGE_ANTIC_TICKS': ticks(WAITS['R Attack Antic']),
        'RAGE_SLAM_TICKS': ticks(WAITS['Rage']),
        'RAGE_TURN_TICKS': ticks(CLIPS['Rage'][0] / CLIPS['Rage'][1]),
        'RAGE_PARTICLE_TICKS': ticks(WAITS['Particle Pause']),
        'RAGE_END_TICKS': ticks(WAITS['Rage End']),
        'DEATH_LAND_TICKS': ticks(WAITS['Death Land']),
        'FIRST_IDLE_TICKS': ticks(WAITS['First Idle']),
        'ARENA_END_WAIT_TICKS': ticks(ARENA_END_WAIT),
        'RUN_SPEED': round(RUN_SPEED * 65536),
        'RUN_TRIGGER_DISTANCE': round(RUN_TRIGGER_DISTANCE * 65536),
        'RUN_STOP_DISTANCE': round(RUN_STOP_DISTANCE * 65536),
        'STUN_ROLL_SPEED': round(STUN_ROLL_SPEED * 65536),
        'STUN_ROLL_STOP_SPEED': round(STUN_ROLL_STOP_SPEED * 65536),
        'JA_RECOIL_SPEED': round(JA_RECOIL_SPEED * 65536),
        'TOWARDS_CLAMP': round(TOWARDS_CLAMP * 65536),
        'RANDOM_JUMP_MIN': round(RANDOM_JUMP_MIN * 65536),
        'WALL_RAY_DISTANCE': round(WALL_RAY_DISTANCE * 65536),
        'SLAM_SKIP_JUMP_DISTANCE': round(SLAM_SKIP_JUMP_DISTANCE * 65536),
        'SHOCKWAVE_SPEED': round(SHOCKWAVE_SPEED * 65536),
        'RAGE_POINT_X': round(RAGE_POINT_X * 65536),
        'FINAL_POINT_X': round(FINAL_POINT_X * 65536),
        'FALL_RAY_DISTANCE': round(FALL_RAY_DISTANCE * 65536),
        'JA_OFFSET': round(JA_OFFSET * 65536),
        'RANDOM_JUMP_MAX': round(RANDOM_JUMP_RANGE[1] * 65536),
        'SHOCKWAVE_X_ORIGIN': round(SHOCKWAVE_X_ORIGIN * 65536),
        'JUMP_SPEED_Y': round(JUMP_SPEED_Y['Jump'] * 65536),
        'SLAM_JUMP_SPEED_Y': round(JUMP_SPEED_Y['S Jump'] * 65536),
        'STUN_JUMP_SPEED_Y': round(JUMP_SPEED_Y['Stun Start'] * 65536),
        'DEATH_FALL_SPEED_Y': round(JUMP_SPEED_Y['Floor Break'] * 65536),
        'GRAVITY_IDLE': round(GRAVITY['idle'] * 65536),
        'GRAVITY_JUMP': round(GRAVITY['jump'] * 65536),
        'GRAVITY_JUMP_ATTACK': round(GRAVITY['jump_attack'] * 65536),
        'GRAVITY_RAGE_JUMP': round(GRAVITY['rage_jump'] * 65536),
        'GRAVITY_DEATH_JUMP': round(GRAVITY['death_jump'] * 65536),
        'GRAVITY_STUN': round(GRAVITY['stun'] * 65536),
        'RISE_MULTIPLIER': round(RISE_MULTIPLIER * 65536),
        'FALL_MULTIPLIER': round(FALL_MULTIPLIER * 65536),
        'TOWARDS_FACTOR': round(TOWARDS_FACTOR * 65536),
        'JA_ANTIC_FACTOR': round(JA_ANTIC_FACTOR * 65536),
        'RAGE_JUMP_FACTOR': round(RAGE_JUMP_FACTOR * 65536),
        'FINAL_JUMP_FACTOR': round(FINAL_JUMP_FACTOR * 65536),
        'TURNS_BEFORE_ATTACK': TURNS_BEFORE_ATTACK,
        'JUMP_ATTACKS_IN_A_ROW': JUMP_ATTACKS_IN_A_ROW,
        'SLAMS_IN_A_ROW': SLAMS_IN_A_ROW,
    }
    for key, value in fields.items():
        limit = 65535 if key.endswith('TICKS') else 0x7fffffff
        if type(value) is not int or not 0 <= value <= limit:
            raise ValueError(f'False Knight constant exceeds its guest representation: {key}')
    fields['CLIP_TICKS'] = [ticks(CLIPS[name][0] / CLIPS[name][1]) for name in CLIP_ORDER]
    fields['SLAM_OVERSHOOT'] = [round(value * 65536) for value in SLAM_OVERSHOOT]
    fields['HERO_X_CLAMP'] = [round(value * 65536) for value in HERO_X_CLAMP]
    fields['IDLE_TICKS'] = [[ticks(phase['idle'][0]), ticks(phase['idle'][1])] for phase in PHASES]
    fields['JUMP_BARRELS'] = [list(phase['jump_barrels']) for phase in PHASES]
    fields['SLAM_BARRELS'] = [list(phase['slam_barrels']) for phase in PHASES]
    # `Death Anim Start`, `Steam`, `Ready`, the Death Head 1 clip and
    # `Death Head Land`: the delay between the last head kill and the arena
    # learning the boss is gone.
    fields['DEATH_TAIL_TICKS'] = [
        ticks(WAITS['Death Anim Start']), ticks(WAITS['Steam']), ticks(WAITS['Ready']),
        ticks(CLIPS['Death Head 1'][0] / CLIPS['Death Head 1'][1]), ticks(WAITS['Death Head Land'])]
    return fields


if __name__ == '__main__':
    import argparse
    from source import Source, ROOT, dump
    from scene import Scene
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / '.hkpsx/false-knight/source-contract.json')
    args = parser.parse_args()
    source = Source()
    boss = Scene(source, SCENE_FILE)
    room = Scene(source, ROOM_FILE)
    contract = recognize(boss, room)
    contract['animation'] = measure_animation(boss)
    contract['generated_constants'] = generated_false_knight_params()
    contract['source_sha256'] = {
        name: hashlib.sha256((source.directory / name).read_bytes()).hexdigest()
        for name in [SCENE_FILE, ROOM_FILE, 'Managed/Assembly-CSharp.dll']}
    contract['code_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    dump(args.output, contract)
    art = contract['animation']
    print(f"False Knight: {contract['health']} hp body, {contract['head_health']} hp head, "
          f"{contract['staggers']} staggers; {len(contract['arena']['gates'])} arena gates.")
    print(f"Animation: {art['clip_frames']} clip frames, {art['unique_sprites']} unique sprites, "
          f"{art['oversized_sprites']} beyond 64x64, extent {art['max_extent'][0]}x{art['max_extent'][1]}, "
          f"{art['total_bytes'] / 1024:.1f} KiB of 4bpp texels, "
          f"{art['max_tiles_one_frame']} of {art['animation_cache_max_frame_tiles']} tiles for its "
          f"largest frame; {art['verdict']}.")
