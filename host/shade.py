"""Verify and cook the original Hollow Shade against .hkpsx/shade/CONTRACT.md.

The Shade can appear in any scene the Knight died in, so its art cannot live
in a per-view atlas. Frames stay in linked RAM and reach VRAM through the
existing 64x64 animation slots; only a single 16-colour CLUT is resident.
"""
import hashlib, json, math
from pathlib import Path
from PIL import Image
from source import Source, ROOT, dump
from cook import tk_sprite, FOCAL, CAM_Z
from geo import q16, rust_array
from materials import quantize_alpha_coverage
from focus import action_fields

SHADE = 4541
SHADE_FSM = 21414
DEATH = 6109
DEATH_FSM = 20803
HERO_DEATH_FSM = 21158
LIBRARY = 22801
COLLECTION = 21382
CLUT = (320, 482, 16, 1)
# Free CLUT rows above the dialogue palette at x320.
CLUT_ROWS = 14
# The animation slot caps a single upload; the largest Shade frame is far under.
SLOT_BYTES = 2048
CLIPS = {'Idle': (12, 12., 0), 'Startle': (7, 12., 2), 'Fly': (5, 12., 0), 'TurnToFly': (9, 12., 1),
         'Slash Antic': (6, 12., 2), 'Slash': (2, 24., 2), 'Slash CD': (2, 12., 2),
         'Retreat Start': (7, 16., 2), 'Retreat End': (7, 16., 2),
         'Death Start': (1, 12., 6), 'Death': (12, 16., 2), 'Slash Effect': (2, 24., 2)}
# Clip order the guest indexes; Slash Effect is the child's, and stays last.
ORDER = ['Idle', 'Startle', 'Fly', 'TurnToFly', 'Slash Antic', 'Slash', 'Slash CD',
         'Retreat Start', 'Retreat End', 'Death Start', 'Death', 'Slash Effect']
BODY_SIZE = (0.5, 1.14)
BODY_OFFSET = (0.0, -0.37)
ALERT_RADIUS = 7.27
SLASH_SCALE = 1.2700315713882446
SLASH_LOCAL = (-0.0486297607421875, -0.0116996765136718)
# Fly and Position, the two states the controller reproduces.
FLY_ACTIONS = {
    'ChaseObject': {'speedMax': 4., 'acceleration': .2, 'targetSpread': 0.},
    'ChaseObjectV2': {'speedMax': 4., 'accelerationForce': 8.},
    'WaitRandom': {'timeMin': 1., 'timeMax': 2., 'finishEvent': 'ATTACK'},
    'FaceObject': {'spriteFacesRight': False, 'playNewAnimation': True, 'newAnimationClip': 'TurnToFly'},
    'Tk2dPlayAnimation': {'clipName': 'Fly'},
}
POSITION_ACTIONS = {
    'DistanceFly': {'distance': 3., 'speedMax': 4., 'acceleration': .2, 'targetsHeight': True, 'height': 0.},
    'Wait': {'time': 6., 'finishEvent': 'END'},
}
TRANSITIONS = {
    'Idle': [('ALERT', 'Startle'), ('TOOK DAMAGE', 'Startle'), ('FRIENDLY', 'Friendly Idle')],
    'Startle': [('FINISHED', 'Fly'), ('FRIENDLY', 'Friendly Idle')],
    'Fly': [('ATTACK', 'Quake?'), ('RETREAT', 'Retreat Start')],
    'Position': [('ATTACK', 'Slash Antic'), ('RETREAT', 'Retreat Start'), ('QUAKE', 'Quake Antic'),
                 ('SCREAM', 'Scream Antic'), ('END', 'Quake?')],
    'Slash Antic': [('FINISHED', 'Check Dir')],
    'Check Dir': [('RIGHT', 'Right'), ('LEFT', 'Left')],
    'Right': [('FINISHED', 'Slash')],
    'Left': [('FINISHED', 'Slash')],
    'Slash': [('SLASH', 'Slash Box')],
    'Slash Box': [('FINISHED', 'Slash CD')],
    'Slash CD': [('FINISHED', 'Fly')],
    'Retreat Start': [('FINISHED', 'Retreat')],
    'Retreat': [('FINISHED', 'Retreat End')],
    'Retreat End': [('FINISHED', 'Retreat Reset')],
    'Retreat Reset': [('FINISHED', 'Idle')],
}
# Branches this port cannot reach; the audit proves each is gated on a zero level.
UNREACHABLE = {'Quake?': ('Quake Level', 0), 'Scream?': ('Scream Level', 0), 'Sp Check': ('Fireball Level', 1)}


def _near(a, b):
    return abs(float(a) - float(b)) <= 1e-6


def _scalar(value):
    if isinstance(value, dict):
        return value['name'] if value.get('useVariable') else value['value']
    return value


def _components(source, file, gid):
    go = source.read(file.objects[gid])
    out = []
    for c in go['m_Component']:
        obj = source.ref(file, c['component'])
        out.append((source.typename(obj), obj))
    return go, out


def _one(components, name):
    matches = [obj for typename, obj in components if typename == name]
    if len(matches) != 1:
        raise ValueError('expected one ' + name + ' on the Hollow Shade')
    return matches[0]


def _child(source, file, gid, name):
    go = source.read(file.objects[gid])
    transform = source.ref(file, [c['component'] for c in go['m_Component']][0])
    for ref in source.read(transform)['m_Children']:
        child = source.read(source.ref(file, ref))
        target = source.read(file.objects[child['m_GameObject']['m_PathID']])
        if target['m_Name'] == name:
            return child['m_GameObject']['m_PathID'], child
    raise ValueError('missing Hollow Shade child: ' + name)


def _state_actions(fsm, name):
    state = next((s for s in fsm['states'] if s['name'] == name), None)
    if state is None:
        raise ValueError('missing Shade Control state: ' + name)
    return state


def _matches(actual, fields):
    for key, value in fields.items():
        got = _scalar(actual.get(key))
        if isinstance(value, float) and isinstance(got, (int, float)) and not isinstance(got, bool):
            if not _near(got, value):
                return False
        elif got != value:
            return False
    return True


def _check_actions(fsm, state_name, expected):
    """Exactly one enabled action of each name must carry the audited fields.

    A state may hold several actions of one name (Remove Geo writes both
    `geoPool` and `geo`), so the fields select which one is meant.
    """
    data = _state_actions(fsm, state_name)['actionData']
    for action, fields in expected.items():
        hits = [i for i, n in enumerate(data['actionNames'])
                if n.rsplit('.', 1)[-1] == action and data['actionEnabled'][i]
                and _matches(action_fields(data, i), fields)]
        if len(hits) != 1:
            raise ValueError(f'unsupported Shade action set: {state_name}/{action}')


def audit(source):
    """Raise unless the live prefab still matches the audited contract."""
    resources = source.file('resources.assets')
    go, components = _components(source, resources, SHADE)
    if go['m_Name'] != 'Hollow Shade' or go.get('m_TagString', '') not in ('', 'Untagged'):
        raise ValueError('changed Hollow Shade identity')
    body = source.read(_one(components, 'BoxCollider2D'))
    if body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or not all(_near(body['m_Size'][k], v) for k, v in zip('xy', BODY_SIZE)) \
            or not all(_near(body['m_Offset'][k], v) for k, v in zip('xy', BODY_OFFSET)):
        raise ValueError('unsupported Hollow Shade body collider')
    rigid = source.read(_one(components, 'Rigidbody2D'))
    if rigid['m_BodyType'] != 0 or rigid['m_GravityScale'] != 0 or rigid['m_LinearDamping'] != 0 \
            or rigid['m_Constraints'] != 4:
        raise ValueError('unsupported Hollow Shade rigid body')
    recoil = source.read(_one(components, 'Recoil'))
    if recoil['freezeInPlace'] or recoil['recoilSpeedBase'] != 15 or not _near(recoil['recoilDuration'], .15) \
            or recoil['preventRecoilUp']:
        raise ValueError('unsupported Hollow Shade recoil variant')
    damage = source.read(_one(components, 'DamageHero'))
    if damage['damageDealt'] != 1 or damage['hazardType'] != 1:
        raise ValueError('unsupported Hollow Shade contact damage')
    health = source.read(_one(components, 'HealthManager'))
    if health['hp'] != 10 or health['smallGeoDrops'] or health['mediumGeoDrops'] or health['largeGeoDrops']:
        raise ValueError('unsupported Hollow Shade health manager')
    alert_gid, _ = _child(source, resources, SHADE, 'Alert Range New')
    alert_go, alert_components = _components(source, resources, alert_gid)
    circle = source.read(_one(alert_components, 'CircleCollider2D'))
    if not alert_go['m_IsActive'] or not circle['m_IsTrigger'] or not _near(circle['m_Radius'], ALERT_RADIUS) \
            or circle['m_Offset'] != {'x': 0., 'y': 0.}:
        raise ValueError('unsupported Hollow Shade alert range')
    slash_gid, slash_transform = _child(source, resources, SHADE, 'Slash')
    slash_go, slash_components = _components(source, resources, slash_gid)
    if slash_go['m_IsActive'] or not _near(slash_transform['m_LocalScale']['x'], SLASH_SCALE) \
            or not all(_near(slash_transform['m_LocalPosition'][k], v) for k, v in zip('xy', SLASH_LOCAL)):
        raise ValueError('unsupported Hollow Shade Slash child placement')
    polygon = source.read(_one(slash_components, 'PolygonCollider2D'))
    if not polygon['m_IsTrigger'] or len(polygon['m_Points']['m_Paths']) != 1:
        raise ValueError('unsupported Hollow Shade Slash collider')
    slash_damage = source.read(_one(slash_components, 'DamageHero'))
    if slash_damage['damageDealt'] != 1 or slash_damage['hazardType'] != 1:
        raise ValueError('unsupported Hollow Shade Slash damage')
    path = [(p['x'] * SLASH_SCALE + SLASH_LOCAL[0], p['y'] + SLASH_LOCAL[1]) for p in polygon['m_Points']['m_Paths'][0]]
    slash_bounds = [min(p[0] for p in path), min(p[1] for p in path),
                    max(p[0] for p in path), max(p[1] for p in path)]
    fsm = source.read(resources.objects[SHADE_FSM])['fsm']
    if fsm['name'] != 'Shade Control' or fsm['startState'] != 'Pause':
        raise ValueError('unsupported Shade Control entry')
    states = {s['name']: s for s in fsm['states']}
    for name, transitions in TRANSITIONS.items():
        if name not in states or [(t['fsmEvent']['name'], t['toState']) for t in states[name]['transitions']] != transitions:
            raise ValueError('unsupported Shade Control transitions: ' + name)
    _check_actions(fsm, 'Fly', FLY_ACTIONS)
    _check_actions(fsm, 'Position', POSITION_ACTIONS)
    _check_actions(fsm, 'Slash Antic', {'Tk2dPlayAnimationWithEvents': {'clipName': 'Slash Antic'},
                                        'DistanceFly': {'distance': 3., 'speedMax': 4., 'acceleration': .2}})
    _check_actions(fsm, 'Slash', {'Wait': {'time': .0829999968409538, 'finishEvent': 'SLASH'}})
    _check_actions(fsm, 'Right', {'SetVelocity2d': {'x': 8.}})
    _check_actions(fsm, 'Left', {'SetVelocity2d': {'x': -8.}})
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}
    if not _near(variables.get('Max Roam', 0), 25.):
        raise ValueError('unsupported Shade Max Roam')
    # Every spell branch must still be gated on a level this port never grants.
    for state, (variable, threshold) in UNREACHABLE.items():
        data = _state_actions(fsm, state)['actionData']
        gates = [action_fields(data, i) for i, n in enumerate(data['actionNames'])
                 if n.rsplit('.', 1)[-1] == 'IntCompare' and data['actionEnabled'][i]
                 and _scalar(action_fields(data, i).get('integer1')) == variable]
        if not gates or not any(_scalar(g.get('integer2')) == threshold for g in gates):
            raise ValueError('Shade spell branch is no longer gated: ' + state)
    death = source.read(resources.objects[DEATH_FSM])['fsm']
    if death['name'] != 'Shade Control' or death['startState'] != 'Death Start':
        raise ValueError('unsupported Hollow Shade Death entry')
    _check_actions(death, 'Death Start', {'SetPlayerDataInt': {'intName': 'geoPool', 'value': 0},
                                          'Wait': {'time': .5, 'finishEvent': 'FINISHED'}})
    hero = source.read(resources.objects[HERO_DEATH_FSM])['fsm']
    _check_actions(hero, 'Remove Geo', {'SetPlayerDataInt': {'intName': 'geo', 'value': 0}})
    _check_actions(hero, 'Drain Soul', {'SetPlayerDataInt': {'intName': 'MPReserve', 'value': 0}})
    library = source.read(resources.objects[LIBRARY])
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name, (frames, fps, wrap) in CLIPS.items():
        clip = by_name.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode']) != (frames, fps, wrap):
            raise ValueError('unsupported Hollow Shade animation: ' + name)
    return by_name, slash_bounds, {'body': list(BODY_SIZE), 'body_offset': list(BODY_OFFSET),
                                   'alert_radius': ALERT_RADIUS, 'hp': health['hp'], 'slash_bounds': slash_bounds}


def cook():
    source = Source()
    by_name, slash_bounds, facts = audit(source)
    resources = source.file('resources.assets')
    scale = FOCAL / -CAM_Z
    textures = {}
    images = []
    boxes = []
    art_sources = []

    def sprite(collection_ref, index):
        obj = source.ref(resources, collection_ref)
        im, box = tk_sprite(source, obj.assets_file, source.read(obj), index, textures)
        dims = tuple(max(1, math.ceil((box[i + 2] - box[i]) * scale)) for i in (0, 1))
        images.append(im.resize(dims, Image.Resampling.LANCZOS))
        boxes.append(list(box))
        art_sources.append(f'{source.sid(obj)}:{index}')
        return len(images) - 1

    clips = []
    for name in ORDER:
        clip = by_name[name]
        start = len(images)
        for frame in clip['frames']:
            sprite(frame['spriteCollection'], frame['spriteId'])
        clips.append(dict(name=name, start=start, count=len(clip['frames']),
                          fps=int(clip['fps']), wrap=clip['wrapMode']))
    # 53 frames exceed one 256x256 quantizer sheet, so clips are grouped into
    # sheets of at most that size, each with its own CLUT. Grouping by clip
    # keeps every frame of one animation on a single palette.
    def shelf(sizes, limit=256):
        """Shelf packing; None when this set cannot fit one sheet."""
        order = sorted(range(len(sizes)), key=lambda i: -sizes[i][1])
        positions = [None] * len(sizes)
        x = y = row = 0
        for i in order:
            w, h = sizes[i]
            if w > limit or h > limit:
                return None
            if x + w > limit:
                x = 0
                y += row
                row = 0
            if y + h > limit:
                return None
            positions[i] = (x, y)
            x += w
            row = max(row, h)
        return positions

    groups = []
    current = []
    for clip in clips:
        candidate = current + list(range(clip['start'], clip['start'] + clip['count']))
        if shelf([images[i].size for i in candidate]) is None:
            if not current:
                raise ValueError('a single Hollow Shade clip does not fit one sheet')
            groups.append(current)
            current = list(range(clip['start'], clip['start'] + clip['count']))
        else:
            current = candidate
    if current:
        groups.append(current)
    palettes = []
    blob = bytearray()
    frames = [None] * len(images)
    sheets = []
    for clut_index, group in enumerate(groups):
        sizes = [images[i].size for i in group]
        positions = shelf(sizes)
        sheet = Image.new('RGBA', (256, 256))
        for i, origin in zip(group, positions):
            sheet.paste(images[i], origin)
        sw, sh, palette, packed = quantize_alpha_coverage(sheet, 128)
        palettes.append(palette)
        sheets.append(sheet)
        for i, (x0, y0) in zip(group, positions):
            im = images[i]
            stride = (im.width + 3) // 4 * 2
            texels = bytearray(stride * im.height)
            for y in range(im.height):
                for x in range(im.width):
                    b = packed[(y + y0) * ((sw + 1) // 2) + (x + x0) // 2]
                    n = (b >> (((x + x0) & 1) * 4)) & 15
                    texels[y * stride + x // 2] |= n << ((x & 1) * 4)
            if len(texels) > SLOT_BYTES:
                raise ValueError('Hollow Shade frame exceeds one animation slot')
            frames[i] = dict(offset=len(blob), width=im.width, height=im.height,
                             bounds=boxes[i], clut=clut_index)
            blob.extend(texels)
    if len(palettes) > CLUT_ROWS:
        raise ValueError('Hollow Shade palettes exceed the free CLUT rows')
    palette = b''.join(palettes)
    payload = bytes(palette) + bytes(blob)
    rust = ['// Generated source Hollow Shade art; no embedded retail source dump.',
            f'pub const CLUT_RECT:(u16,u16,u16,u16)=({CLUT[0]},{CLUT[1]},{CLUT[2]},{CLUT[3]});',
            f'pub const PALETTE_BYTES:usize={len(palette)};',
            f'pub const PALETTE_COUNT:usize={len(palettes)};',
            f'pub const SHADE_HP:u16={facts["hp"]};',
            'pub const SLASH_BOUNDS:[i32;4]=' + rust_array([q16(v) for v in slash_bounds]) + ';',
            'pub const BODY_BOUNDS:[i32;4]=' + rust_array([q16(v) for v in (
                BODY_OFFSET[0] - BODY_SIZE[0] / 2, BODY_OFFSET[1] - BODY_SIZE[1] / 2,
                BODY_OFFSET[0] + BODY_SIZE[0] / 2, BODY_OFFSET[1] + BODY_SIZE[1] / 2)]) + ';',
            f'pub const ALERT_RADIUS:i32={q16(ALERT_RADIUS)};',
            'pub const SHADE_CLIPS:&[Clip]=&[' + ','.join(
                'Clip{' + ','.join(f'{k}:{c[k]}' for k in ('start', 'count', 'fps', 'wrap')) + '}'
                for c in clips) + '];',
            'pub const SHADE_FRAMES:&[Frame]=&[']
    for f in frames:
        rust.append('Frame{' + f'offset:{f["offset"]},width:{f["width"]},height:{f["height"]},clut:{f["clut"]},'
                    + 'bounds:' + rust_array([q16(v) for v in f['bounds']]) + '},')
    rust.append('];')
    for i, name in enumerate(ORDER):
        rust.append(f'pub const CLIP_{name.upper().replace(" ", "_")}:usize={i};')
    text = '\n'.join(rust) + '\n'
    (ROOT / 'data/shade.rs').write_text(text)
    (ROOT / 'data/shade.hk').write_bytes(payload)
    report = {'frames': len(frames), 'bytes': len(payload), 'palette_bytes': len(palette),
              'palettes': len(palettes), 'clip_groups': [[clips[c]['name'] for c in range(len(clips))
                  if clips[c]['start'] in g] for g in groups],
              'clut_rect': list(CLUT), 'clips': clips, 'source_art': art_sources,
              'largest_frame_bytes': max((f['width'] + 3) // 4 * 2 * f['height'] for f in frames),
              'facts': facts, 'image_dimensions': [im.size for im in images],
              'limitations': [
                  'Frames reach VRAM through the existing 64x64 animation slots; only the 16-colour CLUT is resident.',
                  'Every particle system, the death orbs, the Soul Orb HUD event, the light effect and all Shade audio are not presented.',
                  'The Slash polygon becomes its axis-aligned bounds and the retreat iTween path a straight interpolation.',
                  'The Fireball, Quake, Scream, Friendly, Lake and Jar branches are unreachable at this port levels and are not implemented.'],
              'payload_sha256': hashlib.sha256(payload).hexdigest(),
              'rust_sha256': hashlib.sha256(text.encode()).hexdigest(),
              'source_sha256': {name: hashlib.sha256((source.directory / name).read_bytes()).hexdigest()
                                for name in list(source.files) + ['Managed/Assembly-CSharp.dll']},
              'tool_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    dump(ROOT / '.hkpsx/shade-provenance.json', report)
    Path(ROOT / '.hkpsx/shade').mkdir(parents=True, exist_ok=True)
    for i, sheet in enumerate(sheets):
        sheet.save(ROOT / f'.hkpsx/shade/art{i}.png')
    print(f'Hollow Shade: {len(frames)} frames, {len(palettes)} palettes, {len(payload)} bytes, '
          f'largest {report["largest_frame_bytes"]}')
    return report


if __name__ == '__main__':
    cook()
