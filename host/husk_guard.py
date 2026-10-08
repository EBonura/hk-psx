"""Recognize the Husk Guard (`Zombie Guard`) of Crossroads_21 and Crossroads_48.

The controller lives in shared/hk-sim/src/husk_guard.rs; this module admits a
placement only when its serialized shape matches the one read to write that
controller, and proves that the prefab constants the controller carries are
this placement's own.

`Zombie Guard` has no Walker: one 48-state FSM owns every velocity. Asleep
(`Dormant`) until the hero is in `Alert Range New` with a line of sight or it is
hit; then `Wake`, and from `Idle` it roams within `Roam Distance` of its spawn,
chases (walking inside `Chase Distance`, running beyond it), and in `Attack
Range` turns to face the hero and picks a club slam (3 in 4) or a backward
stomp hop (1 in 4), never more than four clubs or two stomps in a row. The club
arms the `Swipe` hitbox (2 masks) for its clip and throws a `Slam Effect R`
where it lands; the stomp's landing sends a `Shockwave Wave` each way. It walks
home after four seconds of idling.

Two places the port reads past the FSM, both recorded in `limitations`:
`Stomp Cooldown` waits with a `WAIT` event it has no transition for, so read
literally a guard would stop for good after its first stomp; the shipped
game's guards keep attacking, so the port leaves after the wait. And the
FSM's `Facing Right` is the
transform's +1 scale while the art faces -x, so the port faces the sprite
where the body moves and hits; see husk_guard.rs.
"""
import hashlib

from combat import ticks
from runner import ASSEMBLIES, axis_aligned_bounds, body_box, body_contract, fsm_fingerprint

FSM_NAME = 'Zombie Guard'
# Structural fingerprint (states, transitions, enabled actions and their scalar
# fields, variables): both placements share it, and a variant needs a new audit.
FSM_SHA256 = 'd90edca6658df5ec636075f6c355dbb1b2673c0db62af98c447b09aba317a870'
# name: (frames, fps, wrapMode).
CLIPS = {
    'Walk': (10, 12., 0), 'Turn': (2, 12., 2),
    'Dormant': (1, 30., 6), 'Wake': (6, 12., 2), 'Idle': (7, 12., 0), 'Run': (6, 10., 0),
    'Stop Run': (6, 12., 2), 'Stop Walk': (2, 12., 2), 'Anticipate': (6, 12., 2),
    'Attack2': (7, 15., 2), 'Startle': (4, 12., 2), 'Stomp Antic': (3, 12., 1),
    'Stomp Jump': (4, 12., 2), 'Stomp Land': (6, 12., 2),
    'Death Stun': (1, 30., 6), 'Death Air': (3, 12., 2), 'Death Land': (9, 12., 2),
}
# `husk_guard::Clip::slot` order; Walk and Turn are the shared ActorSpec slots.
CLIP_SLOTS = ('dormant', 'wake', 'idle', 'run', 'stop_run', 'stop_walk', 'anticipate', 'attack',
              'startle', 'stomp_antic', 'stomp_jump', 'stomp_land')
SLOT_CLIPS = {'dormant': 'Dormant', 'wake': 'Wake', 'idle': 'Idle', 'run': 'Run',
              'stop_run': 'Stop Run', 'stop_walk': 'Stop Walk', 'anticipate': 'Anticipate',
              'attack': 'Attack2', 'startle': 'Startle', 'stomp_antic': 'Stomp Antic',
              'stomp_jump': 'Stomp Jump', 'stomp_land': 'Stomp Land'}
# Death Stun, Death Air and Death Land are the corpse's (host/effects.py), from
# this same library, so they are not actor slots.
# The prefab boxes, Q16 relative to the actor origin in the frame where the art
# is drawn unmirrored (its face and club toward -x), exactly as husk_guard.rs
# carries them. Refused if a placement's own children differ.
ALERT_Q16 = [-1102971, -259850, 1102971, 181207]
# `Attack Range` after `Wake`'s SetScale x 10 (it is authored 16.29 wide, and
# only the woken states read it).
ATTACK_Q16 = [-327680, -246088, 327680, 172687]
OVERHEAD_Q16 = [-101253, 12880, 101253, 244406]
# `Swipe`'s PolygonCollider2D, as its bounding box.
SWIPE_Q16 = [-323584, -70656, 142336, 299008]
# Stomp shockwave: `Land`'s SetFsmFloat Speed 18 and SetScale 1.25 on the pooled
# `Shockwave Wave`, whose `Start Move`/`Move` the False Knight's wave shares.
WAVE_SPEED = 18.
WAVE_SCALE = 1.25
SLAM_LIBRARY = ('resources.assets', 21298)
# Clips cooked at every other frame and half their rate (see cook.py).
FRAME_STRIDE = {'Walk': 2, 'Idle': 2, 'Run': 2, 'Stop Run': 2, 'Wake': 2, 'Stomp Land': 2}
SLAM_CLIP = 'Slam'


def _q(v):
    return round(v * 65536)


def _fsm(records):
    matches = [data for _, typ, data in records if typ == 'PlayMakerFSM']
    names = sorted(d['fsm']['name'] for d in matches)
    if names != [FSM_NAME]:
        raise ValueError('unsupported Husk Guard FSM set: ' + ', '.join(names))
    fsm = matches[0]
    if not fsm['m_Enabled']:
        raise ValueError('Husk Guard FSM disabled')
    return fsm['fsm']


def _variables(fsm):
    out = {}
    for group in fsm['variables'].values():
        if isinstance(group, list):
            for v in group:
                if isinstance(v, dict) and 'name' in v and not isinstance(v.get('value'), dict):
                    out[v['name']] = v['value']
    return out


def _children(sc, gid):
    out = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        kid = sc.transforms[child['m_PathID']]['m_GameObject']['m_PathID']
        out[sc.gos[kid]['m_Name']] = kid
    return out


def _local_box(sc, gid, origin, mirror, scale_x=None):
    """A child's trigger box relative to the actor, in the unmirrored art frame."""
    from actors import _component_records
    boxes = [d for _, t, d in _component_records(sc, gid) if t == 'BoxCollider2D']
    if len(boxes) != 1 or not boxes[0]['m_Enabled'] or not boxes[0]['m_IsTrigger']:
        raise ValueError('Husk Guard child needs one trigger box: ' + sc.gos[gid]['m_Name'])
    b = boxes[0]
    t = sc.transforms[sc.go_transform[gid]]
    sx = abs(scale_x if scale_x is not None else t['m_LocalScale']['x'])
    sy = abs(t['m_LocalScale']['y'])
    cx = t['m_LocalPosition']['x'] + b['m_Offset']['x'] * sx
    cy = t['m_LocalPosition']['y'] + b['m_Offset']['y'] * sy
    w, h = b['m_Size']['x'] * sx, b['m_Size']['y'] * sy
    return [_q(cx - w / 2), _q(cy - h / 2), _q(cx + w / 2), _q(cy + h / 2)]


def _wave(source, sc, fsm):
    """`Land`'s pooled `Shockwave Wave` and the `Shockwave Spurt` it leaves."""
    from false_knight_art import _state, _actions, _one, _scalar, _enum, _prefab_parts, _prefab_box
    land = _state(fsm, 'Land')
    spawns = [f for f, _ in _actions(land, 'SpawnObjectFromGlobalPool')]
    waves = [f for f in spawns if source.read(source.ref(sc.file, f['gameObject']['value']))['m_Name'] == 'Shockwave Wave']
    if len(waves) != 2:
        raise ValueError('Land no longer sends two shockwaves')
    speeds = {_scalar(f['setValue']) for f, _ in _actions(land, 'SetFsmFloat')}
    scales = {_scalar(f['x']) for f, _ in _actions(land, 'SetScale')}
    if speeds != {WAVE_SPEED} or scales != {WAVE_SCALE}:
        raise ValueError('Land changed its shockwave speed or scale')
    wave_o = source.ref(sc.file, waves[0]['gameObject']['value'])
    _, wave_parts = _prefab_parts(source, wave_o)
    wfsm = next(t['fsm'] for kind, _, t in wave_parts if kind == 'PlayMakerFSM' and t['fsm']['name'] == 'shockwave')
    start = _state(wfsm, 'Start Move')
    (operator, op_index), = _actions(start, 'FloatOperator')
    if _enum(start, op_index, 'operation') != 2:
        raise ValueError('Start Move no longer multiplies the incrementer')
    accel_factor = _scalar(operator['float2'])
    start_factor = _scalar(_one(start, 'FloatMultiplyV2')['multiplyBy'])
    ray = _scalar(_one(_state(wfsm, 'Move'), 'RayCast2d')['distance'])
    spurt_o = None
    data = _state(wfsm, 'Right')['actionData']
    for i, n in enumerate(data['actionNames']):
        if n.endswith('SetGameObject') and data['actionEnabled'][i]:
            st = data['actionStartIndex'][i]
            en = data['actionStartIndex'][i + 1] if i + 1 < len(data['actionNames']) else len(data['paramName'])
            for j in range(st, en):
                if data['paramDataType'][j] == 19:
                    v = data['fsmGameObjectParams'][data['paramDataPos'][j]].get('value')
                    if v and v.get('m_PathID'):
                        spurt_o = source.ref(wave_o.assets_file, v)
    if spurt_o is None:
        raise ValueError('the wave no longer names its spurt')
    spurt_go, spurt_parts = _prefab_parts(source, spurt_o)
    timing = next(t['fsm'] for kind, _, t in spurt_parts if kind == 'PlayMakerFSM' and t['fsm']['name'] == 'Damage timing')
    arm = _scalar(_one(_state(timing, 'Wait'), 'Wait')['time'])
    armed = _scalar(_one(_state(timing, 'Activate'), 'Wait')['time'])
    damage = _one(_state(timing, 'Activate'), 'SetDamageHeroAmount')['damageDealt']
    damage = _scalar(damage) if isinstance(damage, dict) else damage
    animator = next(t for kind, _, t in spurt_parts if kind == 'tk2dSpriteAnimator')
    library = source.ref(spurt_o.assets_file, animator['library'])
    clip = source.read(library)['clips'][animator['defaultClipId']]
    if clip['name'] != 'Shockwave Spurt' or clip['wrapMode'] != 2:
        raise ValueError('the spurt no longer plays Shockwave Spurt once')
    box = [v * WAVE_SCALE if i % 2 == 0 else v for i, v in enumerate(_prefab_box(wave_parts))]
    return {
        'start_speed': _q(WAVE_SPEED * start_factor), 'accel': _q(WAVE_SPEED * accel_factor),
        'wave_box': [_q(v) for v in box], 'ground_ray': _q(ray),
        'spurt_box': [_q(v) for v in _prefab_box(spurt_parts)],
        'damage_from': ticks(arm), 'damage_to': ticks(arm + armed), 'damage': int(damage),
        'spurt_ticks': ticks(len(clip['frames']) / clip['fps']),
        'library': [library.assets_file.name, library.path_id], 'clip': clip['name'],
        'sources': {'wave': source.sid(wave_o), 'spurt': source.sid(spurt_o)},
    }


# Scenes whose room stream cannot take the guard's bank. Every region pack
# holds its own streamed art plus the scene actor bank inside 256 KiB:
# Crossroads_21's rooms stream about 76 KiB (74,400 of it the Knight's clips)
# and its Bargers, Runner and Leaper about 46 KiB more, leaving about 134 KiB,
# while the guard's own art is about 197 KiB even at every other frame. Refused
# by that budget, as the Crossroads_22 Hatcher is refused by its actor budget.
# Crossroads_48's rooms stream about 30 KiB, so its guard fits.
BANK_REFUSED = {'Crossroads_21': 'Crossroads_21 region stream (~76 KiB) + its other actors (~46 KiB)'
                                 ' + the guard (~197 KiB) exceed the 256 KiB room bank'}


def recognize(sc, actor):
    source = sc.source
    from quality import SCENE_TABLE
    scene_file = actor['source'].split(':')[0]
    scene_name = next((s['scene_name'] for s in SCENE_TABLE if s['file'] == scene_file), None)
    if scene_name in BANK_REFUSED:
        raise ValueError('Husk Guard refused by the room bank budget: ' + BANK_REFUSED[scene_name])
    from actors import _component_records
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Husk Guard actions require a fresh source audit: ' + name)
    gid = actor['game_object']
    records = _component_records(sc, gid)
    fsm = _fsm(records)
    fingerprint = fsm_fingerprint(fsm)
    if fingerprint != FSM_SHA256:
        raise ValueError('unverified Husk Guard FSM variant')
    if fsm['startState'] != 'Initiate' or fsm.get('globalTransitions'):
        raise ValueError('Husk Guard FSM starts elsewhere or has global transitions')
    variables = _variables(fsm)
    expected = {'Chase Distance': 9.0, 'Roam Distance': 23.5, 'Woken': 1, 'Clubs In A Row': 0,
                'Stomps In A Row': 0}
    for k, v in expected.items():
        if variables.get(k) != v:
            raise ValueError(f'unsupported Husk Guard variable {k}={variables.get(k)!r}')
    matrix = sc.world(sc.go_transform[gid])
    if abs(abs(matrix[0][0]) - 1) > 1e-6 or abs(matrix[1][1] - 1) > 1e-6 or abs(matrix[0][1]) > 1e-6:
        raise ValueError('unsupported Husk Guard transform')
    health = [d for _, t, d in records if t == 'HealthManager']
    if len(health) != 1 or health[0]['hp'] != 70:
        raise ValueError('unsupported Husk Guard health')
    damage = [d for _, t, d in records if t == 'DamageHero']
    if len(damage) != 1 or damage[0]['damageDealt'] != 1:
        raise ValueError('unsupported Husk Guard contact damage')
    bodies = [d for _, t, d in records if t == 'Rigidbody2D']
    if len(bodies) != 1 or bodies[0]['m_GravityScale'] != 1.:
        raise ValueError('unsupported Husk Guard body')
    body = body_box(records, 'Husk Guard')
    kids = _children(sc, gid)
    for name in ('Attack Range', 'Alert Range New', 'Overhead Detect', 'Swipe'):
        if name not in kids:
            raise ValueError('Husk Guard is missing its ' + name)
    boxes = {
        'alert': _local_box(sc, kids['Alert Range New'], None, None),
        'attack': _local_box(sc, kids['Attack Range'], None, None, scale_x=10.),
        'overhead': _local_box(sc, kids['Overhead Detect'], None, None),
    }
    swipe = kids['Swipe']
    if sc.active(swipe):
        raise ValueError('Husk Guard Swipe is authored active')
    swipe_records = _component_records(sc, swipe)
    polys = [d for _, t, d in swipe_records if t == 'PolygonCollider2D']
    hits = [d for _, t, d in swipe_records if t == 'DamageHero']
    if len(polys) != 1 or len(hits) != 1 or hits[0]['damageDealt'] != 2:
        raise ValueError('unsupported Husk Guard Swipe')
    points = polys[0]['m_Points']['m_Paths'][0]
    boxes['swipe'] = [_q(min(p['x'] for p in points)), _q(min(p['y'] for p in points)),
                      _q(max(p['x'] for p in points)), _q(max(p['y'] for p in points))]
    for key, constant in (('alert', ALERT_Q16), ('attack', ATTACK_Q16), ('overhead', OVERHEAD_Q16),
                          ('swipe', SWIPE_Q16)):
        if boxes[key] != constant:
            raise ValueError(f'Husk Guard {key} box {boxes[key]} is not the audited prefab one')
    animator = actor['tk2dSpriteAnimator']
    library_o = source.ref(sc.file, animator['library'])
    library = {c['name']: c for c in source.read(library_o)['clips'] if c['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        c = library.get(name)
        if c is None or (len(c['frames']), c['fps'], c['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Husk Guard animation: ' + name)
    wave = _wave(source, sc, fsm)
    audited = {'start_speed': 29491, 'accel': 2359296, 'wave_box': [-46363, 21182, 7464, 115753],
               'ground_ray': 104858, 'spurt_box': [-16352, -118, 10151, 114151], 'damage_from': 3,
               'damage_to': 6, 'damage': 1, 'spurt_ticks': 18}
    if any(wave[k] != v for k, v in audited.items()):
        raise ValueError('Husk Guard shockwave is not the one husk_guard.rs carries')
    x, y = actor['position'][:2]
    body_world = axis_aligned_bounds(matrix, [body['m_Offset'][k] for k in 'xy'],
                                     [body['m_Size'][k] for k in 'xy'])
    body_q16 = [_q(v - [x, y][i % 2]) for i, v in enumerate(body_world)]
    return {
        'kind': 'HuskGuard', 'guest_enabled': True, 'fsm_sha256': fingerprint,
        'library_source': source.sid(library_o),
        # `Initiate` branches on `Start Facing Left`; the art faces -x.
        'initial_direction': -1 if variables.get('Start Facing Left') else 1,
        'boxes_q16': boxes, 'body_bounds_q16': body_q16, 'wave': wave,
        'art_bindings': dict({'walk': 'Walk', 'turn': 'Turn'}, **{s: SLOT_CLIPS[s] for s in CLIP_SLOTS}),
        # Every other frame at half the rate, same durations: the guard's art is
        # 257 KiB of stream on its own and Crossroads_21's bank holds 256 KiB.
        'frame_stride': dict(FRAME_STRIDE),
        'extra_art': {'spurt': {'file': wave['library'][0], 'path_id': wave['library'][1], 'clip': wave['clip']},
                      'slam': {'file': SLAM_LIBRARY[0], 'path_id': SLAM_LIBRARY[1], 'clip': SLAM_CLIP}},
        'limitations': [
            'Stomp Cooldown waits on a WAIT event with no transition; read literally the guard would stand'
            " for good after its first stomp. The shipped game's guards keep attacking, so the port goes on"
            ' to Cooldown after the 0.4 s wait.',
            "The FSM's Facing Right is localScale +1 while the art, the Swipe polygon and the run dust"
            ' all put the front at -x and Slam Origin/Burst Rocks at +x; the port faces the sprite where'
            ' the body moves and puts the club, the Swipe and the slam in front.',
            'Hero Solid (the back the Knight can stand on), the thrown club of the corpse, particles,'
            ' audio and camera shakes are not presented.',
            'Walk, Idle, Run, Stop Run, Wake and Stomp Land keep every other frame at half the rate'
            " (same durations), to fit Crossroads_21's 256 KiB actor bank.",
        ],
    }
