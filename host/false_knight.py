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
SCENE_TABLE; host/scene.py merges the chosen one into the room instead.

The fight is wired. What is still a subset is the art: `measure_animation`
reproduces host/cook.py's own actor path so the clip set is admitted against a
measurement rather than a guess, and docs/FALSE_KNIGHT.md carries the per-view
figures. It is reproduced here rather than quoted so that a later art-path
change is checked against the source, not against prose.
"""
import hashlib
import json
import math
import struct
from pathlib import Path

from combat import HIT_EVASION_SECONDS, ticks
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

# `FK Barrel Summon`'s PersonalObjectPool and the prefab it holds. The prefab is
# not in level48: it is `Falling Barrel` in sharedassets48, reached through the
# pool rather than by name, so a source that repoints the pool fails here.
BARREL_PREFAB = 'Falling Barrel'
BARREL_POOL = 8
BARREL_LAYER = 17
BARREL_BOX = ((1.1799999475479126, 1.1100000143051147), (0., 0.))
BARREL_GRAVITY_SCALE = .32499998807907104
BARREL_DAMAGE = 1
BARREL_HAZARD = 1
BARREL_SPIN = 720.
BARREL_RANDOM_SCALE = (.800000011920929, 1.)
FALL_BARREL_SHA256 = '25064c55a01b08378bddcb4fe51b2dfd76ec74302e06ea443f21c224e5c7dad4'

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
        'invulnerable_ticks': ticks(HIT_EVASION_SECONDS),
        'head_invulnerable_ticks': ticks(HIT_EVASION_SECONDS),
        'contact_damage': CONTACT_DAMAGE, 'staggers': STAGGERS_TO_DEATH, 'rage_slams': RAGE_SLAMS,
        'phases': [dict(phase) for phase in PHASES], 'geo_drops': dict(GEO_DROPS),
        'player_data': dict(PLAYER_DATA), 'colliders_local': boxes, 'scale': BODY_SCALE,
        'clips': library, 'arena': arena_sources(sc, room),
        'fsm_sha256': {name: fsm_digest(_fsms(sc, owner)[name]['fsm'])
                       for owner, name in ((body, 'FalseyControl'), (body, 'Check Health'),
                                           (head, 'Health Check'), (battle, 'Battle Control'),
                                           (summoner, 'summon'))},
        'limitations': [
            'Six of the thirty-four clips are cooked. At the authored 1.3 scale the whole set is'
            ' 716,110 bytes of 4bpp texels against a 393,216-byte room pack that already holds'
            " Crossroads_10's own art, so the clip set is admitted against room bytes rather than"
            ' palettes or animation slots. The per-clip figures are in the animation block and the'
            ' per-view measurement is in docs/FALSE_KNIGHT.md.',
            'BG CLOSE, BG OPEN and BG QUICK OPEN move all five gates, through'
            " host/battle_gates.py's per-view edge binding. host/cook.py bakes every gate as"
            ' terrain and the runtime lifts the three the source loads open, so closing them'
            ' is a restore rather than a placement and the arena seals at its ends.',
            'The sim reproduces FalseyControl at 60 Hz; the source shapes rising and falling speed'
            ' per 50 Hz FixedUpdate, so airtime differs slightly.',
            'The `Esc` branch is unreachable in the Crossroads arena: nothing there sets `Hero Escaped`.',
            "`FK Barrel Summon` and its pooled `Falling Barrel` are presented: the barrels fall from"
            ' the summoner and damage the hero. Their spin, their RandomScale and the nail knock-back'
            ' are not, and the break is the collider and the sprite going away.',
            'The shockwave object, the floor crack and break, the staff, the Death Head and every'
            ' particle are recorded, not presented; docs/FALSE_KNIGHT.md carries the measurement'
            ' behind each refusal.',
        ],
        'source_methods': ['HealthManager.Hit', 'HealthManager.Die', 'SceneAdditiveLoadConditional.Start',
                           'PersistentBoolItem.OnEnable', 'CameraLockArea.OnTriggerEnter2D'],
    }


# The clips the fight is cooked with, in the order the bank lays them out:
# `walk_clip` holds Idle and `turn_clip` holds Turn, and the other four ride in
# the ActorController. Six of thirty-four, and the limit is room-pack bytes
# rather than palettes. Measured against the real pipeline (cook the boss view's
# base, append the scene-wide bank to all 20 Crossroads_10 views, dedup): this
# set puts the worst view at 354,968 of the 393,216-byte room budget, 395
# texture records of 640 and 279 CLUT slots of 416. Adding Jump as a seventh
# reaches 383,036, and Attack Recover or an eighth clip does not fit at all.
# docs/FALSE_KNIGHT.md carries the per-clip table behind the choice.
ART_BINDINGS = {'walk': 'Idle', 'turn': 'Turn', 'jump_antic': 'Jump Antic',
                'land': 'Land', 'stun_opened': 'Stun Opened', 'attack': 'Attack'}
# The clip records checked against CLIPS before admission, which is every clip
# the cook will actually read.
ADMITTED_CLIPS = tuple(dict.fromkeys(ART_BINDINGS.values()))


def arena_trigger_world_box(sc):
    """`Battle Scene`'s trigger box in world coordinates, or a refusal.

    Crossing it is what sends BATTLE START, so the guest needs it as a plain
    world AABB: it is the only thing the boss tests while it is dormant. The
    object carries no rotation or scale in this scene and the recognizer refuses
    one rather than projecting a box it has never seen.
    """
    battle = _named(sc, 'Battle Scene')
    matrix = sc.world(sc.go_transform[battle])
    if not _near(matrix[0][0], 1.) or not _near(matrix[1][1], 1.) \
            or abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6:
        raise ValueError('arena trigger carries an unsupported rotation or scale')
    (width, height), (ox, oy) = _box(sc, battle)
    x, y = sc.point(battle)[:2]
    return [x + ox - width / 2, y + oy - height / 2, x + ox + width / 2, y + oy + height / 2]


def recognize_placement(sc, actor):
    """Admit the one placed False Knight into the guest actor pool, or refuse.

    `recognize` above is the whole-fight source contract and needs the parent
    room, because the battle gates live there. This is the actor-pool admission,
    which needs only the boss's own object and the `Battle Scene` beside it: the
    guest binds `shared/hk-sim`'s `false_knight::FalseKnight` and the
    `boss::Arena` that sends it BATTLE START.

    The `FalseyControl` digest is checked here too, so a source change fails at
    admission rather than leaving the guest running a boss whose behaviour has
    moved underneath it.
    """
    source = sc.source
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('False Knight admission requires a fresh source audit: ' + name)
    gid = actor['game_object']
    if sc.gos[gid]['m_Layer'] != 11:
        raise ValueError('False Knight outside the enemy layer')
    fsms = _fsms(sc, gid)
    if set(fsms) != {'FalseyControl', 'Check Health'}:
        raise ValueError('unsupported False Knight FSM set: ' + ', '.join(sorted(fsms)))
    for name in ('FalseyControl', 'Check Health'):
        if not fsms[name]['m_Enabled']:
            raise ValueError(f'disabled {name!r} FSM')
        digest = fsm_digest(fsms[name]['fsm'])
        if digest != FSM_SHA256[name]:
            raise ValueError(f'unverified {name} FSM variant: {digest}')
    control = fsms['FalseyControl']['fsm']
    if control['startState'] != 'State 4':
        raise ValueError('False Knight starts in an unsupported state')
    health = actor['health_manager']
    if health['hp'] != BODY_HEALTH or not _near(health['invulnerableTime'], BODY_INVULNERABLE_TIME) \
            or not health['hasSpecialDeath'] or health['invincible'] or health['damageOverride']:
        raise ValueError('unsupported False Knight HealthManager variant')
    if not all(_near(a, b) for pair in zip(_box(sc, gid), BODY_BOX) for a, b in zip(*pair)):
        raise ValueError('unsupported False Knight body collider')
    matrix = sc.world(sc.go_transform[gid])
    if not _near(abs(matrix[0][0]), BODY_SCALE) or not _near(matrix[1][1], BODY_SCALE) \
            or abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6:
        raise ValueError('unsupported False Knight rotation or scale')
    library = clip_contract(sc, gid)
    by_name = {clip['name']: clip for clip in library['clips']}
    for name in ADMITTED_CLIPS:
        clip = by_name[name]
        if (clip['frames'], clip['fps'], clip['wrap_mode'], clip['loop_start']) != CLIPS[name]:
            raise ValueError('unsupported False Knight cooked animation: ' + name)
    facing_right = matrix[0][0] > 0
    # `FalseKnight::new` starts the guest controller at Facing Right false, the
    # way `FalseyControl`'s own variable is serialized, so a placement mirrored
    # the other way would start the fight facing the wrong side. There is one
    # placement and it faces left; refuse rather than silently disagree.
    if facing_right:
        raise ValueError('False Knight placement faces right; the guest controller starts facing left')
    return {
        'kind': 'FalseKnight', 'guest_enabled': True, 'art_bindings': dict(ART_BINDINGS),
        # Both HealthManagers restore rather than die, so nothing of the boss
        # ever falls: the death sequence ends at the HealthManager death event
        # the arena waits on, and the corpse is a separate authored object.
        'no_corpse': True,
        # Crossroads_10 merges Crossroads_10_boss in for this actor; nothing
        # else that scene carries is admitted with it (see actor_sources).
        'admit_from_additive_scene': True,
        'facing_right': facing_right,
        # The cook takes the absolute transform scale, so a positive source
        # scale is the art as authored, which the guest's draw expresses as -1.
        'initial_direction': -1 if facing_right else 1,
        # The body is killable now: reaching zero restores it to 65 and staggers
        # it, which is what `Check Health` does, and the exposed Head is what a
        # phase costs. `hasSpecialDeath` is the death sequence the guest runs.
        'invincible': False, 'special_death': True,
        'head_health': HEAD_HEALTH, 'head_invulnerable_ticks': ticks(HIT_EVASION_SECONDS),
        'staggers': STAGGERS_TO_DEATH,
        # The one thing the boss tests while it is dormant.
        'arena_trigger_world': arena_trigger_world_box(sc),
        # `FK Barrel Summon` is a different source object, but the guest reaches
        # it through the boss: the boss is the only thing that ever sends SUMMON
        # and the arena already rides on the boss for the same reason.
        'barrel': barrel_source(sc),
        'turn_ticks': ticks(CLIPS['Turn'][0] / CLIPS['Turn'][1]),
        'library_source': library['library_source'],
        'fsm_sha256': {name: fsm_digest(fsms[name]['fsm']) for name in ('FalseyControl', 'Check Health')},
        'limitations': [
            'Six of the thirty-four clips are cooked (Idle, Turn, Jump Antic, Land, Stun Opened,'
            ' Attack); the rest of the fight plays the nearest of those. The limit is'
            " Crossroads_10's 393,216-byte room budget, not palettes: see docs/FALSE_KNIGHT.md.",
            'BG OPEN and BG QUICK OPEN reach the two gates the source loads closed, which lift'
            ' for good once the fight is won. BG CLOSE reaches the other three and moves'
            ' nothing: they are open on the first frame, so host/cook.py bakes no terrain for'
            ' them and the arena never seals at its ends.',
            'The three pre-battle Zombies are recorded rather than admitted, so KILL ALL ENEMIES'
            ' has no target; the arena counts only the boss, as the source does.',
            'The Hitter overlay plays its attack clip on the body rather than as a second draw,'
            ' and its DamageHero trigger is reproduced as a box.',
            'The barrels fall and hurt. What the barrel does not do is spin, vary its scale or'
            ' answer the nail; the break is the collider and the sprite going away, with no'
            ' Bits, Dust Puff, Splat or one-shot.',
            'The shockwave, the floor crack and break, the staff, the Death Head and every'
            ' particle are recorded, not presented.',
            'Rise and Fall shape vertical speed per 50 Hz FixedUpdate in the source and per'
            ' 60 Hz tick in the guest, so airtime differs slightly.',
        ],
        'source_methods': ['HealthManager.Hit', 'HealthManager.Die', 'tk2dSpriteAnimator.Play',
                           'CameraLockArea.OnTriggerEnter2D'],
    }


def barrel_source(sc):
    """`FK Barrel Summon`'s pooled `Falling Barrel`, or a refusal.

    The one thing the fight throws that a player can be hit by, and the only
    part of the summoner the guest needs: one SpriteRenderer, one box, one
    gravity scale and one DamageHero. Everything about it is read back here so
    that a source change fails the cook instead of leaving the guest dropping a
    barrel with the wrong damage or the wrong reach.

    The prefab lives in sharedassets48 rather than in the boss scene, so it is
    reached through the pool's own reference. `spawn` carries the summoner's
    world transform because `summon`'s `Spawn` state takes the y off it, and
    that object is not the boss.
    """
    from actors import _component_records
    source = sc.source
    summoner = _named(sc, 'FK Barrel Summon')
    pools = [tree for _, kind, tree in _component_records(sc, summoner) if kind == 'PersonalObjectPool']
    if len(pools) != 1 or len(pools[0]['startupPool']) != 1:
        raise ValueError('FK Barrel Summon no longer holds exactly one pooled prefab')
    entry = pools[0]['startupPool'][0]
    if entry['size'] != BARREL_POOL or entry['initialiseSpawnedObjects']:
        raise ValueError('barrel pool reserve changed')
    prefab = source.ref(sc.file, entry['prefab'])
    file = prefab.assets_file
    go = source.read(prefab)
    if go['m_Name'] != BARREL_PREFAB or go['m_Layer'] != BARREL_LAYER:
        raise ValueError('unsupported barrel prefab identity')
    parts = {}
    for ref in go['m_Component']:
        component = file.objects[ref['component']['m_PathID']]
        kind = source.typename(component)
        tree = source.read(component)
        if kind == 'PlayMakerFSM':
            parts.setdefault('fsm', {})[tree['fsm']['name']] = tree
        else:
            if kind in parts:
                raise ValueError('duplicate barrel component: ' + kind)
            parts[kind] = tree
    for kind in ('Transform', 'BoxCollider2D', 'Rigidbody2D', 'SpriteRenderer', 'DamageHero', 'RandomScale'):
        if kind not in parts:
            raise ValueError('barrel prefab is missing a ' + kind)
    control = parts['fsm'].get('Fall Barrel Control')
    if control is None or not control['m_Enabled']:
        raise ValueError('barrel prefab has no enabled Fall Barrel Control')
    digest = fsm_digest(control['fsm'])
    if digest != FALL_BARREL_SHA256:
        raise ValueError(f'unverified Fall Barrel Control variant: {digest}')
    transform = parts['Transform']
    if transform['m_LocalScale'] != {'x': 1., 'y': 1., 'z': 1.} \
            or transform['m_LocalRotation'] != {'x': 0., 'y': 0., 'z': 0., 'w': 1.}:
        raise ValueError('unsupported barrel prefab transform')
    box = parts['BoxCollider2D']
    shape = ((box['m_Size']['x'], box['m_Size']['y']), (box['m_Offset']['x'], box['m_Offset']['y']))
    # The box is a trigger, which is why the barrel falls through the arena
    # floor instead of resting on it: `Idle`'s Trigger2dEventLayer on layer 8
    # is what ends the fall, and the guest reproduces that as the fall segment
    # crossing a terrain edge rather than as a solve against one.
    if not box['m_IsTrigger'] or not all(_near(a, b) for pair in zip(shape, BARREL_BOX) for a, b in zip(*pair)):
        raise ValueError('unsupported barrel collider')
    body = parts['Rigidbody2D']
    if body['m_BodyType'] != 0 or body['m_LinearDamping'] != 0 \
            or not _near(body['m_GravityScale'], BARREL_GRAVITY_SCALE):
        raise ValueError('unsupported barrel body')
    hurt = parts['DamageHero']
    if hurt['damageDealt'] != BARREL_DAMAGE or hurt['hazardType'] != BARREL_HAZARD or hurt['shadowDashHazard']:
        raise ValueError('unsupported barrel DamageHero')
    render = parts['SpriteRenderer']
    if not render['m_Enabled'] or not render['m_Sprite']['m_PathID'] or render['m_FlipX'] or render['m_FlipY'] \
            or render['m_Color'] != {'r': 1., 'g': 1., 'b': 1., 'a': 1.}:
        raise ValueError('unsupported barrel SpriteRenderer')
    scaler = parts['RandomScale']
    if not all(_near(scaler[key], value) for key, value in
               zip(('minScale', 'maxScale'), BARREL_RANDOM_SCALE)):
        raise ValueError('barrel RandomScale range changed')
    # `Determine Spawns` carries a disabled RandomInt 6..8: the live count is
    # the one FalseyControl writes into `Spawns` before it sends SUMMON, which
    # is the phase table. A source that re-enables it would randomise the count
    # under the phase table and the guest would not know.
    summon = _fsms(sc, summoner)['summon']['fsm']
    data = _states(summon)['Determine Spawns']['actionData']
    if any(name.endswith('RandomInt') and enabled
           for name, enabled in zip(data['actionNames'], data['actionEnabled'])):
        raise ValueError('summon now chooses its own spawn count')
    # Recorded as a `file:path_id` sid rather than as the live object, so the
    # record stays JSON-serializable whatever the cook later decides about this
    # actor; host/cook.py::append_barrel_art reopens it from the sid.
    sprite = source.ref(file, render['m_Sprite'])
    return {
        'source': source.sid(prefab), 'sprite': source.sid(sprite),
        'pool': BARREL_POOL, 'damage': BARREL_DAMAGE,
        'collider_local': shape, 'gravity_scale': BARREL_GRAVITY_SCALE,
        'spawn_x': list(BARREL_SPAWN_X), 'spawn_world': sc.point(summoner),
        'gap_ticks': [ticks(BARREL_SPAWN_GAP[0]), ticks(BARREL_SPAWN_GAP[1])],
        'limitations': [
            'The 720 degrees per second `Rotate` the barrel spins at is not reproduced: the'
            ' draw path emits an axis-aligned quad from the frame box and turning one would'
            ' cost a rotation per barrel per frame on a stall-bound frame.',
            'RandomScale 0.8 to 1.0 is not reproduced; every barrel is the authored size.',
            "`Check Direct` is not reproduced: the source lets the nail knock a barrel away at"
            ' 45 units/s, and here a barrel is not a nail target.',
            'The break is the collider and the sprite going away. `Break`\'s Bits and Dust Puff'
            ' emitters, its Splat, its SpawnBlood and its one-shot are particles and audio.',
        ],
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
        # The pooled prefab itself, so the whole-fight contract fails at
        # extraction rather than at the cook if the barrel moves.
        'barrel': barrel_source(sc),
        'pre_battle_enemies': sorted(
            sc.gos[gid]['m_Name'] for gid, go in sc.gos.items()
            if go['m_Name'].startswith('Zombie ') and sc.active(gid)),
        'persistence': {'arena_activated': 'Battle Scene PersistentBoolItem',
                        'scene_selection': ADDITIVE['gate_flag'],
                        # Both ride the save record's PlayerData reserve, at the
                        # top of it, because no cooked FSM names either one:
                        # game/src/script.rs FIELD_FALSE_KNIGHT_*.
                        'guest_fields': ['arena Activated', 'falseKnightFirstPlop'],
                        'guest_store': 'game/src/save.rs SCRIPT_FIELD_SLOTS'},
    }


def measure_animation(sc):
    """Reproduce host/cook.py's actor art path for the False Knight, exactly.

    Returns the per-clip and unique-sprite cost the cooker would produce, which
    is what decides whether the boss can be admitted at all. `Atlas.add_tiled`
    spreads a frame past 64x64 over a rectangle of animation slots, so the unit
    of cost is the tile, not the frame: HKROOM02 sizes its palette block at one
    32-byte entry per texture and the region uploads it whole, so every tile
    takes one of the 416 CLUT slots in every view it is resident in.
    """
    from cook import tk_sprite, FOCAL, CAM_Z, SLOT_PIXELS, ANIMATION_SLOTS, MAX_FRAME_TILES
    from quality import TEXTURE_BUDGET, ROOM_BYTE_BUDGET
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
    # Atlas.pack pads each streamed row to a word, so the linked-RAM cost is the
    # padded stride, not width/2.
    def frame_bytes(size):
        return (((size[0] + 3) & ~3) // 2) * size[1]

    def frame_tiles(size):
        return math.ceil(size[0] / SLOT_PIXELS) * math.ceil(size[1] / SLOT_PIXELS)

    textures, collections, unique, clips = {}, {}, {}, []
    for clip in library['clips']:
        if not clip['name']:
            continue
        widest = (0, 0)
        # A clip pays for its own distinct sprites, because Atlas dedup shares a
        # repeated frame; a residency route holding one clip at a time pays this.
        seen = {}
        for frame in clip['frames']:
            collection_object = source.ref(library_object.assets_file, frame['spriteCollection'])
            sid = source.sid(collection_object)
            if sid not in collections:
                collections[sid] = source.read(collection_object)
            _, box = tk_sprite(source, collection_object.assets_file, collections[sid],
                               frame['spriteId'], textures)
            size = (math.ceil((box[2] - box[0]) * sx * projection),
                    math.ceil((box[3] - box[1]) * sy * projection))
            seen[(sid, frame['spriteId'])] = size
            unique[(sid, frame['spriteId'])] = size
            widest = max(widest, size)
        clips.append({'name': clip['name'], 'frames': len(clip['frames']),
                      'unique_sprites': len(seen), 'largest': list(widest),
                      'tiles': sum(frame_tiles(size) for size in seen.values()),
                      'bytes': sum(frame_bytes(size) for size in seen.values()),
                      'max_tiles_one_frame': max(frame_tiles(size) for size in seen.values())})
    oversized = {key: size for key, size in unique.items() if frame_tiles(size) > 1}
    largest = max(unique.values(), key=lambda size: (size[0] * size[1], size))
    tiles = sum(frame_tiles(size) for size in unique.values())
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
        'total_tiles_64': tiles,
        'max_tiles_one_frame': max(frame_tiles(size) for size in unique.values()),
        # A tile is a texture, so the tile count is what the pack's texture
        # table has to address. It is not the CLUT cost: `Atlas.add_tiled`
        # quantizes a whole frame once and the scene bank pools palettes by
        # value, so the 499 tiles carry far fewer distinct palette words.
        # `tools/texture_headroom.py` reports the palette side.
        'texture_records_required': tiles, 'texture_budget': TEXTURE_BUDGET,
        'room_byte_budget': ROOM_BYTE_BUDGET,
        'animation_cache_slot': [SLOT_PIXELS, SLOT_PIXELS],
        'animation_cache_slots': ANIMATION_SLOTS,
        'animation_cache_max_frame_tiles': MAX_FRAME_TILES,
        'animation_cache_upload_bytes': ANIMATION_SLOTS * SLOT_PIXELS * SLOT_PIXELS // 2,
        # The art path admits these frames now: Atlas.add_tiled spreads a frame
        # past one slot over a rectangle of them and enemies.rs::prepare_draws
        # submits a quad per tile. What is left is residency, which is a per-view
        # question this measurement cannot answer on its own: compare the tile
        # count against tools/texture_headroom.py's free slots for the view the
        # boss would live in.
        'verdict': (f'drawable: every frame binds at most {max(frame_tiles(size) for size in unique.values())} '
                    f'of the {MAX_FRAME_TILES} slots a frame may hold; the whole clip set is '
                    f'{sum(frame_bytes(size) for size in unique.values())} bytes against a '
                    f'{ROOM_BYTE_BUDGET}-byte room pack that already holds the room, which is '
                    f'what admits a clip at a time'),
        'tiles_fit_the_cache': max(frame_tiles(size) for size in unique.values()) <= MAX_FRAME_TILES,
    }


def generated_false_knight_params():
    """The tick/Q16 constants the guest must agree with, as Rust source text.

    tests/test_false_knight.py compares this against the hand-written constants
    in shared/hk-sim/src/false_knight.rs, so the two cannot drift apart.
    """
    fields = {
        'HEALTH': BODY_HEALTH, 'HEAD_HEALTH': HEAD_HEALTH,
        'INVULNERABLE_TICKS': ticks(HIT_EVASION_SECONDS),
        'HEAD_INVULNERABLE_TICKS': ticks(HIT_EVASION_SECONDS),
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
        # `FK Barrel Summon` and its pooled `Falling Barrel`. The pool reserve
        # doubles as the guest's projectile budget, which is why it is a guest
        # constant rather than a record: the rage asks for exactly eight.
        'BARREL_POOL': BARREL_POOL,
        'BARREL_DAMAGE': BARREL_DAMAGE,
        'BARREL_GRAVITY': round(BARREL_GRAVITY_SCALE * 60 * 65536),
    }
    for key, value in fields.items():
        limit = 65535 if key.endswith('TICKS') else 0x7fffffff
        if type(value) is not int or not 0 <= value <= limit:
            raise ValueError(f'False Knight constant exceeds its guest representation: {key}')
    fields['CLIP_TICKS'] = [ticks(CLIPS[name][0] / CLIPS[name][1]) for name in CLIP_ORDER]
    # The two child hitboxes in the body's own frame, already multiplied by its
    # 1.3 transform scale, as [x0,y0,x1,y1]. The source frame is the +1.3 one,
    # so the Hitter reaches forward in +x and a caller facing left mirrors it.
    for name, ((width, height), (ox, oy)) in (('HEAD_BOX', HEAD_BOX), ('HITTER_BOX', HITTER_BOX)):
        fields[name] = [round(value * BODY_SCALE * 65536) for value in
                        (ox - width / 2, oy - height / 2, ox + width / 2, oy + height / 2)]
    fields['SLAM_OVERSHOOT'] = [round(value * 65536) for value in SLAM_OVERSHOOT]
    fields['HERO_X_CLAMP'] = [round(value * 65536) for value in HERO_X_CLAMP]
    fields['BARREL_SPAWN_X'] = [round(value * 65536) for value in BARREL_SPAWN_X]
    # `Determine Spawns` waits before the first barrel and `Spawn` waits after
    # every one, and both hold the same WaitRandom.
    fields['BARREL_GAP_TICKS'] = [ticks(BARREL_SPAWN_GAP[0]), ticks(BARREL_SPAWN_GAP[1])]
    (bw, bh), _ = BARREL_BOX
    fields['BARREL_HALF'] = [round(bw / 2 * 65536), round(bh / 2 * 65536)]
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
          f"{art['oversized_sprites']} beyond one slot, extent {art['max_extent'][0]}x{art['max_extent'][1]}, "
          f"{art['total_tiles_64']} tiles, {art['total_bytes'] / 1024:.1f} KiB of 4bpp texels, "
          f"{art['max_tiles_one_frame']} of {art['animation_cache_max_frame_tiles']} tiles for its "
          f"largest frame; {art['verdict']}.")
    cheapest = min(art['clips'], key=lambda clip: clip['tiles'])
    dearest = max(art['clips'], key=lambda clip: clip['tiles'])
    print(f"Per clip, in tiles: {cheapest['name']} is the cheapest at {cheapest['tiles']}, "
          f"{dearest['name']} the dearest at {dearest['tiles']}.")
