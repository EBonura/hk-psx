"""Recognize the verified Windows Zombie Swipe/Walker variant, without spawning it.

This prepares the source contract and shared clip inventory for guest integration.
It does not make movement_supported true or substitute the Crawler controller.
"""
import hashlib
import json
import math
from pathlib import Path

# Full serialized FSM fingerprint includes action parameters omitted by narrow
# action decoders (object types, references, disabled actions and event flags).
# Any changed variant requires another source audit before recognition.
FSM_SHA256 = '82bd846bbe92f489c0684645a657a196514dd6f0338bc58103ee3aae51ec4908'
ASSEMBLY_SHA256 = 'e9048ef6a633970f735e01ec166d3959f610eaea7a88d827d48d67b1e5fb87bd'
ASSEMBLIES = {'Assembly-CSharp.dll': ASSEMBLY_SHA256,
              'PlayMaker.dll': '0ef0e7829d125e1f632c8a189260ec6c6882630be6932c8c7ae032efbc53469a',
              'TeamCherry.TK2D.dll': 'b443474e6cf6eb03debe5346884a51893621cc39a9b2666c160034fbd5783da7',
              'Assembly-CSharp-firstpass.dll': '2c9b97488f3f8d2e29c8af2e3200e378216652904be22c2678eb5d2cdaa95499'}
CLIPS = {'Idle': (6, 10, 0), 'Walk': (7, 10, 0), 'Turn': (2, 12, 2),
         'Attack Anticipate': (5, 12, 2), 'Attack Lunge': (8, 12, 2),
         'Attack Cooldown': (1, 12, 2), 'Fall': (5, 10, 1),
         'Death Air': (1, 30, 6), 'Death Land': (8, 12, 2)}


def walker_parameters(walker):
    """Reject C# controller variants not represented by hk_sim::runner."""
    expected = dict(walkSpeedL=-1.5, walkSpeedR=1.5, rightScale=-1.,
                    edgeXAdjuster=0., turnPause=1., turnAfterIdlePercentage=0,
                    pauseWaitMin=4., pauseWaitMax=1.5, pauseTimeMin=2.5,
                    pauseTimeMax=1.5, pauses=1, idleClip='Idle', walkClip='Walk',
                    turnClip='Turn', ambush=0, startInactive=0, waitForHeroX=0,
                    preventTurn=0, ignoreHoles=0, preventTurningToFaceHero=0,
                    preventScaleChange=0, m_Enabled=1)
    for name, value in expected.items():
        if walker.get(name) != value:
            raise ValueError('unsupported Runner Walker field: ' + name)
    return {'walk_velocity_q16': [-98304, 98304], 'lunge_velocity_q16': [-393216, 393216],
            'walking_wait_endpoints_ticks': [240, 90], 'pause_endpoints_ticks': [150, 90],
            'turn_cooldown_ticks': 60, 'idle_ticks': 15, 'initial_direction': -1}


def clip_contract(clips):
    by_name = {c['name']: c for c in clips}
    if len(by_name) != len(clips) or set(by_name) != set(CLIPS):
        raise ValueError('unsupported Runner animation inventory')
    result = []
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = by_name[name]
        if (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Runner animation: ' + name)
        if any(frame.get('triggerEvent', False) for frame in clip['frames']):
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
                'm_LinearDamping': 0., 'm_GravityScale': 1., 'm_Interpolate': 0,
                'm_SleepingMode': 1, 'm_CollisionDetection': 0, 'm_Constraints': 4,
                'm_Material': {'m_FileID': 0, 'm_PathID': 0},
                'm_IncludeLayers': {'m_Bits': 0}, 'm_ExcludeLayers': {'m_Bits': 0}}
    for name, value in expected.items():
        if rigid.get(name) != value:
            raise ValueError('unsupported Runner rigid body field: ' + name)


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


def _one(records, kind):
    matches = [(sid, data) for sid, typ, data in records if typ == kind]
    if len(matches) != 1:
        raise ValueError('Runner requires exactly one ' + kind)
    return matches[0]


def recognize(sc, actor):
    from actors import _component_records
    source = sc.source
    records = _component_records(sc, actor['game_object'])
    walker_id, walker = _one(records, 'Walker')
    parameters = walker_parameters(walker)
    _, fsm_component = _one(records, 'PlayMakerFSM')
    fsm = fsm_component['fsm']
    fingerprint = hashlib.sha256(json.dumps(fsm, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()).hexdigest()
    if not fsm_component['m_Enabled'] or fsm.get('name') != 'Zombie Swipe' or fingerprint != FSM_SHA256:
        raise ValueError('unverified Runner FSM variant')
    for name, expected in ASSEMBLIES.items():
        if hashlib.sha256((source.directory / 'Managed' / name).read_bytes()).hexdigest() != expected:
            raise ValueError('Runner methods require a fresh source audit: ' + name)
    gid = actor['game_object']; matrix = sc.world(sc.go_transform[gid])
    # These two placed instances begin with unit scale facing left. Runtime
    # facing changes must mirror the body offset along with the source sprite.
    if sc.gos[gid]['m_Layer'] != 11 or any(abs(matrix[i][i] - 1) > 1e-6 for i in range(2)):
        raise ValueError('unsupported Runner layer or initial scale')
    _, body = _one(records, 'BoxCollider2D')
    _, rigid = _one(records, 'Rigidbody2D')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0:
        raise ValueError('unsupported Runner body collider')
    body_contract(rigid)
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
    animation = clip_contract(clips)
    sprite_keys = sorted({(source.sid(source.ref(library.assets_file, frame['spriteCollection'])), frame['spriteId'])
                         for clip in clips for frame in clip['frames']})
    if len(sprite_keys) != 34:raise ValueError('Runner shared sprite inventory changed')
    _, audio_source = _one(records, 'AudioSource')
    # Unity6 stores the assigned loop in m_Resource; m_audioClip is null here.
    loop = source.ref(sc.file, audio_source['m_Resource'])
    anticipate = next(state for state in fsm['states'] if state['name'] == 'Anticipate')['actionData']
    chase = [source.ref(sc.file, ref) for ref in anticipate['unityObjectParams']]
    if len(chase) != 2 or any(obj.type.name != 'AudioClip' for obj in [loop, *chase]):
        raise ValueError('Runner audio references differ from the verified FSM')
    audio = {'walk_loop': source.sid(loop), 'chase': [source.sid(obj) for obj in chase],
             'loop': bool(audio_source['Loop']), 'volume': audio_source['m_Volume'],
             'initial_pitch': audio_source['m_Pitch'], 'play_on_awake': bool(audio_source['m_PlayOnAwake'])}
    x, y = actor['position'][:2]
    def relative_q16(bounds):
        result = [round((v - [x, y][i % 2]) * 65536) for i, v in enumerate(bounds)]
        if any(abs(v) > 16 * 65536 for v in result):raise ValueError('Runner local bounds exceed Q16 contract')
        return result
    body_q16, alert_q16 = relative_q16(body_bounds), relative_q16(alert_bounds)
    if body_q16 != [-51200, -94208, 28672, 12288] or alert_q16 != [-365036, -144507, 365036, 12730]:
        raise ValueError('Runner sensing shape differs from the compiled query contract')
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
