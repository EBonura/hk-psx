"""Recognize the verified Windows Zombie Swipe/Walker variant, without spawning it.

This prepares the source contract and shared clip inventory for guest integration.
It does not make movement_supported true or substitute the Crawler controller.
"""
import hashlib
import json
import math
from pathlib import Path
from combat import ticks

# Structural FSM fingerprint (fsm_fingerprint): states, transitions, enabled
# actions and their scalar fields, variables other than Lunge Speed. Every
# Zombie Swipe placement in the catalog shares it; any changed variant requires
# another source audit before recognition.
FSM_SHA256 = '73f11594e0115a43a66c8d1695a65916b81bfe6057c180636eed3330aaad92c2'
# Zombie Leap (Leaper): same Walker, a leap attack instead of the swipe.
LEAP_FSM_SHA256 = '15ecb1c0984cd4955e5412dfae354441eb9a9d19c6b2bff9d7b6fc564dc7fe47'
LEAP_CLIPS = {'Idle': (6, 12, 0), 'Walk': (7, 12, 0), 'Turn': (2, 10, 2), 'Attack': (11, 12, 2), 'Land': (2, 15, 2),
              'Death Air': (1, 30, 6), 'Death Land': (8, 12, 2)}
ASSEMBLY_SHA256 = 'e9048ef6a633970f735e01ec166d3959f610eaea7a88d827d48d67b1e5fb87bd'
ASSEMBLIES = {'Assembly-CSharp.dll': ASSEMBLY_SHA256,
              'PlayMaker.dll': '0ef0e7829d125e1f632c8a189260ec6c6882630be6932c8c7ae032efbc53469a',
              'TeamCherry.TK2D.dll': 'b443474e6cf6eb03debe5346884a51893621cc39a9b2666c160034fbd5783da7',
              'Assembly-CSharp-firstpass.dll': '2c9b97488f3f8d2e29c8af2e3200e378216652904be22c2678eb5d2cdaa95499'}
CLIPS = {'Idle': (6, 10, 0), 'Walk': (7, 10, 0), 'Turn': (2, 12, 2),
         'Attack Anticipate': (5, 12, 2), 'Attack Lunge': (8, 12, 2),
         'Attack Cooldown': (1, 12, 2), 'Fall': (5, 10, 1),
         'Death Air': (1, 30, 6), 'Death Land': (8, 12, 2)}


def walker_parameters(walker, lunge_speed=6.):
    """Reject C# controller variants not represented by hk_sim::runner.

    Walk speed, the pause wait/time ranges and the FSM `Lunge Speed` are the
    placement parameters (Runner 1.5/6, Barger 1.5/14, Hornhead 2.5/9); every
    other Walker field must match the audited Runner.
    """
    expected = dict(rightScale=-1., edgeXAdjuster=0., turnPause=1., turnAfterIdlePercentage=0,
                    pauses=1, idleClip='Idle', walkClip='Walk',
                    turnClip='Turn', ambush=0, startInactive=0, waitForHeroX=0,
                    preventTurn=0, ignoreHoles=0, preventTurningToFaceHero=0,
                    preventScaleChange=0, m_Enabled=1)
    for name, value in expected.items():
        if walker.get(name) != value:
            raise ValueError('unsupported Runner Walker field: ' + name)
    speed = walker.get('walkSpeedR')
    if not isinstance(speed, float) or not 0 < speed <= 16 or walker.get('walkSpeedL') != -speed:
        raise ValueError('unsupported Runner Walker field: walkSpeedL')
    if not isinstance(lunge_speed, float) or not 0 < lunge_speed <= 32:
        raise ValueError('unsupported Runner lunge speed')
    waits = {}
    for key, (low, high) in {'walking': ('pauseWaitMin', 'pauseWaitMax'), 'pause': ('pauseTimeMin', 'pauseTimeMax')}.items():
        values = [walker.get(low), walker.get(high)]
        if any(not isinstance(v, float) or not 0 < v <= 10 for v in values):
            raise ValueError('unsupported Runner Walker field: ' + low)
        # Random.Range accepts either argument order; the guest samples [hi, lo].
        waits[key] = sorted((ticks(values[0]), ticks(values[1])), reverse=True)
    return {'walk_speed': speed, 'lunge_speed': lunge_speed,
            'walk_velocity_q16': [-round(speed * 65536), round(speed * 65536)],
            'lunge_velocity_q16': [-round(lunge_speed * 65536), round(lunge_speed * 65536)],
            'walking_wait_endpoints_ticks': waits['walking'], 'pause_endpoints_ticks': waits['pause'],
            'turn_cooldown_ticks': 60, 'idle_ticks': 15, 'initial_direction': -1}


def fsm_fingerprint(fsm):
    """Structure and scalar parameters of the Zombie Swipe FSM, without the
    per-scene object references and the placement's Lunge Speed."""
    from focus import action_fields
    def scalar(value):
        if isinstance(value, dict):
            return ('VAR:' + value['name']) if value.get('useVariable') else repr(value.get('value', value.get('name')))
        return repr(value)
    summary = {'name': fsm['name'], 'start': fsm['startState'],
               'variables': sorted((v['name'], repr(v['value'])) for group in fsm['variables'].values() if isinstance(group, list)
                                   for v in group if isinstance(v, dict) and 'name' in v and 'value' in v
                                   and not isinstance(v['value'], dict) and v['name'] != 'Lunge Speed'),
               'globals': [(t['fsmEvent']['name'], t['toState']) for t in fsm.get('globalTransitions', [])], 'states': []}
    for state in fsm['states']:
        data = state['actionData']
        actions = []
        for index, name in enumerate(data['actionNames']):
            try:
                fields = action_fields(data, index)
            except ValueError as error:
                fields = {'error': str(error)}
            actions.append((name, int(data['actionEnabled'][index]), sorted((k, scalar(v)) for k, v in fields.items() if k != 'gameObject')))
        summary['states'].append((state['name'], [(t['fsmEvent']['name'], t['toState']) for t in state['transitions']], actions))
    return hashlib.sha256(json.dumps(summary, sort_keys=True, default=str).encode()).hexdigest()


def clip_contract(clips, expected=CLIPS):
    """Every Zombie Swipe library carries the same clip set with the same wrap
    modes; frame counts and rates differ per variant and are cooked as found.
    `Fall` is optional (the controller never plays it)."""
    by_name = {c['name']: c for c in clips}
    if len(by_name) != len(clips) or not (set(expected) - {'Fall'}) <= set(by_name) or not set(by_name) <= set(expected):
        raise ValueError('unsupported Runner animation inventory')
    result = []
    for name, (frames, fps, wrap) in expected.items():
        clip = by_name.get(name)
        if clip is None:
            continue
        frames, fps = len(clip['frames']), clip['fps']
        if clip['wrapMode'] != wrap or frames == 0 or fps <= 0:
            raise ValueError('unsupported Runner animation: ' + name)
        # The Leaper's Attack trigger frame is the launch cue, read by recognize().
        if name != 'Attack' and any(frame.get('triggerEvent', False) for frame in clip['frames']):
            raise ValueError('unsupported Runner frame event: ' + name)
        if not 0 <= clip['loopStart'] < frames:
            raise ValueError('invalid Runner loop start: ' + name)
        result.append({'name': name, 'frames': frames, 'fps': fps, 'wrap_mode': wrap,
                       'loop_start': clip['loopStart'], 'nominal_duration_ticks': frames * 60 // fps})
    return result


def body_contract(rigid):
    """Reject physics variants that the pending actor integration cannot honor."""
    expected = {'m_BodyType': 0, 'm_Simulated': True, 'm_UseAutoMass': False,
                'm_UseFullKinematicContacts': False, 'm_Mass': 1.,
                'm_LinearDamping': 0., 'm_Interpolate': 0,
                'm_SleepingMode': 1, 'm_Constraints': 4,
                'm_Material': {'m_FileID': 0, 'm_PathID': 0},
                'm_IncludeLayers': {'m_Bits': 0}, 'm_ExcludeLayers': {'m_Bits': 0}}
    for name, value in expected.items():
        if rigid.get(name) != value:
            raise ValueError('unsupported Runner rigid body field: ' + name)
    # Hornheads use continuous collision detection; the swept solver covers both.
    if rigid.get('m_CollisionDetection') not in (0, 1):
        raise ValueError('unsupported Runner rigid body field: m_CollisionDetection')
    # Runners fall at gravity scale 1, Leapers at .8; the guest body takes it from Params.
    if rigid.get('m_GravityScale') not in (1., .800000011920929):
        raise ValueError('unsupported Runner rigid body field: m_GravityScale')


def axis_aligned_bounds(matrix, offset, size):
    """Transform all four collider corners, retaining child scaling and offsets."""
    if any(not math.isfinite(float(v)) for row in matrix for v in row):
        raise ValueError('nonfinite Runner collider transform')
    if any(abs(matrix[r][c]) > 1e-6 for r, c in [(0, 1), (1, 0), (0, 2), (1, 2)]):
        raise ValueError('rotated Runner collider unsupported')
    if matrix[0][0] == 0 or matrix[1][1] == 0:
        raise ValueError('degenerate Runner collider transform')
    if any(not math.isfinite(v) for v in [*offset, *size]) or min(size) <= 0:
        raise ValueError('invalid Runner collider extent')
    points = [[matrix[i][3] + matrix[i][i] * (offset[i] + sign[i] * size[i] / 2)
               for i in range(2)] for sign in [(-1, -1), (-1, 1), (1, -1), (1, 1)]]
    return [min(p[0] for p in points), min(p[1] for p in points),
            max(p[0] for p in points), max(p[1] for p in points)]


def body_box(records, who='actor'):
    """The actor's own BoxCollider2D; some placements carry the same box twice."""
    boxes = [d for _, t, d in records if t == 'BoxCollider2D']
    if not 1 <= len(boxes) <= 2 or any((b['m_Size'], b['m_Offset'], b['m_IsTrigger'], b['m_Enabled']) != (boxes[0]['m_Size'], boxes[0]['m_Offset'], boxes[0]['m_IsTrigger'], boxes[0]['m_Enabled']) for b in boxes):
        raise ValueError(f'unsupported {who} body colliders')
    return boxes[0]


def _one(records, kind):
    matches = [(sid, data) for sid, typ, data in records if typ == kind]
    if len(matches) != 1:
        raise ValueError('Runner requires exactly one ' + kind)
    return matches[0]


def driving_fsm(records):
    """The Walker's own FSM, chosen by name.

    A placement may carry a second FSM that drives no movement (`enemy_corpse`,
    a death effect), so the number of PlayMakerFSM components is not the
    contract; exactly one movement FSM is.
    """
    matches = [data for _, typ, data in records
               if typ == 'PlayMakerFSM' and data['fsm']['name'] in ('Zombie Swipe', 'Zombie Leap')]
    if len(matches) != 1:
        raise ValueError('Runner requires exactly one Zombie Swipe or Zombie Leap FSM')
    return matches[0]


def recognize(sc, actor):
    from actors import _component_records
    source = sc.source
    records = _component_records(sc, actor['game_object'])
    walker_id, walker = _one(records, 'Walker')
    fsm_component = driving_fsm(records)
    fsm = fsm_component['fsm']
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v and not isinstance(v['value'], dict)}
    leap = fsm.get('name') == 'Zombie Leap'
    parameters = walker_parameters(walker, 1. if leap else variables.get('Lunge Speed'))
    # The serialized FSM embeds owner references, so the fingerprint covers its
    # structure and scalar parameters (states, transitions, enabled actions and
    # their fields, variables other than Lunge Speed).
    fingerprint = fsm_fingerprint(fsm)
    if not fsm_component['m_Enabled'] or fingerprint != (LEAP_FSM_SHA256 if leap else FSM_SHA256):
        raise ValueError('unverified Runner FSM variant')
    if leap:
        parameters['lunge_speed'] = 0.; parameters['lunge_velocity_q16'] = [0, 0]
        parameters['attack'] = {'kind': 'Leap', 'jump_speed_y': 20., 'jump_x_factor': 1.25, 'idle_time': variables.get('Idle Time')}
        if parameters['attack']['idle_time'] != .5:
            raise ValueError('unsupported Leaper idle time')
    else:
        parameters['attack'] = {'kind': 'Swipe'}
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Runner methods require a fresh source audit: ' + name)
    gid = actor['game_object']; matrix = sc.world(sc.go_transform[gid])
    # Placements are authored facing left, except where the transform carries a
    # plain x mirror, which faces them right. Anything else (a rotation, a
    # non-unit magnitude, a y flip) stays refused: the body offset and the
    # sprite both follow the matrix, so only the sign is free to vary.
    if len(actor['position']) < 3 or abs(actor['position'][2]) > .01:
        raise ValueError('Runner depth differs from guest source plane')
    mirror = -1 if matrix[0][0] < 0 else 1
    if (sc.gos[gid]['m_Layer'] != 11
            or abs(abs(matrix[0][0]) - 1) > 1e-6 or abs(matrix[1][1] - 1) > 1e-6
            or abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6):
        raise ValueError('unsupported Runner layer or initial scale')
    # rightScale is -1, so a mirrored transform starts the walker facing right.
    parameters['initial_direction'] = -mirror
    _, body = _one(records, 'BoxCollider2D')
    _, rigid = _one(records, 'Rigidbody2D')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0:
        raise ValueError('unsupported Runner body collider')
    body_contract(rigid)
    parameters['gravity_scale'] = rigid['m_GravityScale']
    body_bounds = axis_aligned_bounds(matrix, [body['m_Offset'][i] for i in 'xy'], [body['m_Size'][i] for i in 'xy'])
    los_id, los = _one(records, 'LineOfSightDetector')
    def local_ref(ref):
        if ref['m_FileID'] != 0 or not ref['m_PathID']:
            raise ValueError('external or missing Runner sensing component')
        return ref['m_PathID']
    range_id = local_ref(walker['alertRange'])
    if local_ref(walker['lineOfSightDetector']) != los_id or not los['m_Enabled'] or [local_ref(r) for r in los['alertRanges']] != [range_id]:
        raise ValueError('Runner sensing references differ')
    kind, alert = sc.objects[range_id]
    if kind != 'AlertRange' or not alert['m_Enabled']:
        raise ValueError('Runner alert component disabled or changed')
    range_go = local_ref(alert['m_GameObject'])
    if not sc.active(range_go):raise ValueError('Runner alert object inactive')
    _, collider = _one(_component_records(sc, range_go), 'BoxCollider2D')
    if not collider['m_Enabled'] or not collider['m_IsTrigger'] or collider['m_EdgeRadius'] != 0:
        raise ValueError('unsupported Runner alert trigger')
    alert_bounds = axis_aligned_bounds(sc.world(sc.go_transform[range_go]),
        [collider['m_Offset'][i] for i in 'xy'], [collider['m_Size'][i] for i in 'xy'])
    _, animator = _one(records, 'tk2dSpriteAnimator')
    if not animator['m_Enabled'] or animator['isRealtime']:
        raise ValueError('Runner requires enabled scaled-time animation')
    library = source.ref(sc.file, animator['library']); clips = source.read(library)['clips']
    animation = clip_contract(clips, LEAP_CLIPS if leap else CLIPS)
    if leap:
        attack = next(c for c in clips if c['name'] == 'Attack')
        triggers = [i for i, frame in enumerate(attack['frames']) if frame.get('triggerEvent')]
        if len(triggers) != 1:
            raise ValueError('Leaper Attack clip needs exactly one trigger frame')
        parameters['attack']['trigger_ticks'] = round(triggers[0] * 60 / attack['fps'])
    sprite_keys = sorted({(source.sid(source.ref(library.assets_file, frame['spriteCollection'])), frame['spriteId'])
                         for clip in clips for frame in clip['frames']})
    if not sprite_keys:raise ValueError('Runner sprite inventory empty')
    _, audio_source = _one(records, 'AudioSource')
    # Unity6 stores the assigned loop in m_Resource; m_audioClip is null here.
    loop = source.ref(sc.file, audio_source['m_Resource'])
    anticipate = next(state for state in fsm['states'] if state['name'] == 'Anticipate')['actionData']
    chase = [source.ref(sc.file, ref) for ref in anticipate['unityObjectParams']]
    if len(chase) != 2 or any(obj.type.name != 'AudioClip' for obj in [loop, *chase]):
        raise ValueError('Runner audio references differ from the verified FSM')
    if leap:
        # The Leaper's random attack samples are not in the resident Runner bank;
        # the guest plays the Runner chase sounds in their place.
        pass
    audio = {'walk_loop': source.sid(loop), 'chase': [source.sid(obj) for obj in chase],
             'loop': bool(audio_source['Loop']), 'volume': audio_source['m_Volume'],
             'initial_pitch': audio_source['m_Pitch'], 'play_on_awake': bool(audio_source['m_PlayOnAwake'])}
    x, y = actor['position'][:2]
    def relative_q16(bounds):
        result = [round((v - [x, y][i % 2]) * 65536) for i, v in enumerate(bounds)]
        if any(abs(v) > 16 * 65536 for v in result):raise ValueError('Runner local bounds exceed Q16 contract')
        return result
    body_q16, alert_q16 = relative_q16(body_bounds), relative_q16(alert_bounds)
    # The level37 Runner shape is hk_sim::runner_senses::Shape::RUNNER; other
    # placements carry their body and alert boxes through the actor spec.
    if alert_q16[0] != -alert_q16[2] or body_q16[0] >= body_q16[2] or body_q16[1] >= body_q16[3] or alert_q16[1] >= alert_q16[3]:
        raise ValueError('Runner sensing shape is not a mirror-stable box')
    return {'kind': 'ZombieSwipeWalker', 'guest_enabled': False, 'parameters': parameters,
            'walker_source': f'{Path(sc.file.name).name}:{walker_id}', 'fsm_sha256': fingerprint,
            'assembly_sha256': ASSEMBLY_SHA256, 'assemblies_sha256': dict(ASSEMBLIES),
            'library_source': source.sid(library),
            'clips': animation, 'unique_sprite_sources': sprite_keys,
            'audio_sources': audio,
            'body_bounds_q16': body_q16, 'alert_bounds_q16': alert_q16,
            'limitations': ['Guest actor/clip/sensing/audio bindings are not yet enabled.',
                            'Unity component/physics ordering, synchronous Walker reentry and RNG require reference comparison.']}


if __name__ == '__main__':
    import argparse
    from source import Source, ROOT, dump
    from scene import Scene
    from actors import actor_sources
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / '.hkpsx/runner70/source-contract.json')
    args = parser.parse_args()
    source = Source(); scene = Scene(source, 'level37')
    actors = actor_sources(scene)
    runners = [a for a in actors if 'pending_movement_control' in a]
    if len(runners) != 2 or any(a['movement_supported'] for a in runners):
        raise ValueError('Expected two recognized, not yet guest-enabled Crossroads Runners')
    contracts = [a['pending_movement_control'] for a in runners]
    if any(c['library_source'] != contracts[0]['library_source'] or c['clips'] != contracts[0]['clips']
           or c['unique_sprite_sources'] != contracts[0]['unique_sprite_sources'] for c in contracts[1:]):
        raise ValueError('Runner instances no longer share one complete animation inventory')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    dump(args.output, {'actors': runners, 'guest_enabled': False,
                      'source_sha256': {name: hashlib.sha256((source.directory / name).read_bytes()).hexdigest()
                                        for name in ['level37', 'Managed/Assembly-CSharp.dll']},
                      'code_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()})
    print('Recognized two Crossroads Runners; one shared library,34 sprites; guest admission remains disabled.')
