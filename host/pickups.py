"""Chests and pickups: what the slice hands the Knight, and where.

Two source families:

- `Chest Control` (a nail hit with the Knight in `Hero Region` opens it; its
  `Spawn Items` flings the Geo its `Geo Small/Med/Large` name and activates the
  `Shiny Item` under its `Item` child). Tutorial_01's holds Fury of the Fallen;
  Crossroads_10's is the False Knight's 200 Geo.
- `Shiny Control` (UP inside `Inspect Region` takes it; the instance's own
  routing says what it gives, read by `items._shiny_grant`), and the heart and
  vessel pieces (`Heart Container Control`, `Vessel Fragment Control`), taken
  on touch through their circle collider. The `Key Giver`'s shiny (the City
  Crest) waits for `falseKnightDefeated`.

The art rides the scene actor bank, the Great Door's route: it is appended to
every view of its own scene only, so it costs no resident RAM and no linked
bytes, and each group (a chest's two frames, the glint's thirteen) shares one
palette so a view spends one CLUT slot per group. `append_art` runs inside the
region cook (host/regions.py); `main` writes data/pickups.rs from the bound
region report afterwards.
"""
import json, math
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
CHEST_TRANSITIONS = {
    'Idle': [('NAIL HIT', 'Range?')],
    'Range?': [('NAIL HIT', 'Open'), ('FINISHED', 'Idle')],
    'Open': [('FINISHED', 'Spawn Items')],
    'Spawn Items': [('FINISHED', 'Opened')],
}
SHINY_TRANSITIONS = {'Idle': [('START INSPECT', 'Hero Down')]}
PIECE_TRANSITIONS = {'Idle': [('GET', 'Pickup Disabled?')]}
PIECE_FSMS = {'Heart Container Control': 'mask_shard', 'Vessel Fragment Control': 'vessel_fragment'}
GRANTS = {'charm': 'Grant::Charm({})', 'trinket': 'Grant::Trinket({})', 'city_key': 'Grant::CityKey',
          'mask_shard': 'Grant::MaskShard', 'vessel_fragment': 'Grant::VesselFragment',
          'rancid_egg': 'Grant::RancidEgg'}
NO_CHEST = 255
# Every other frame of the glint's thirteen: the False Knight's tightest view
# has 13 texture entries free of 640, and its chest takes two of them.
GLINT_STEP = 2


def _one(records, kind):
    hits = [(cid, d) for cid, t, d in records if t == kind]
    if len(hits) != 1:
        raise ValueError(f'expected one {kind}')
    return hits[0]


def _grant(fsm):
    from items import _shiny_grant
    from props import _variables
    grant = _shiny_grant(fsm, _variables(fsm))
    route, state = grant['route'], grant.get('state', '')
    if route == 'charm' and grant.get('charm_id'):
        return 'charm', grant['charm_id']
    if route == 'trinket' and state.startswith('Trink '):
        return 'trinket', grant['trinket_num']
    if route == 'trinket' and any(w['field'] == 'hasCityKey' for w in grant['writes']):
        return 'city_key', 0
    if route == 'trinket' and state == 'Egg':
        return 'rancid_egg', 0
    return None


def _name(fsm, grant, ui):
    """The line `Get Charm`'s message or the trinket state's `Msg Text` shows."""
    from props import _actions, _scalar
    from title_cards import ascii_text
    kind, value = grant
    if kind == 'charm':
        key = f'CHARM_NAME_{value}'
    else:
        wanted = {'city_key': 'City Key', 'rancid_egg': 'Egg'}.get(kind, f'Trink {value}')
        state = next(s for s in fsm['states'] if s['name'] == wanted)
        keys = [_scalar(a['convName']) for a in _actions(state, 'GetLanguageString')]
        if len(keys) != 1:
            raise ValueError(f'{wanted} no longer names one line')
        key = keys[0]
    if key not in ui:
        raise ValueError(f'UI sheet has no {key}')
    return ascii_text(ui[key])


def collect(sc, ui):
    """The scene's chests and pickups, each with its source images."""
    from actors import _component_records
    from cook import tk_sprite, native_sprite
    from props import _fsm, _states, _variables, _kids, _box_world, _actions, _scalar
    source = sc.source
    textures = {}

    def tk(ref, file=None):
        # A library's frame refs resolve against the library's own file.
        obj = source.ref(file or sc.file, ref[0])
        im, box = tk_sprite(source, obj.assets_file, source.read(obj), ref[1], textures)
        return im, list(box)

    def unity(obj, flip):
        im, box = native_sprite(obj)
        box = list(box)
        if flip:
            im, box = im.transpose(Image.Transpose.FLIP_LEFT_RIGHT), [-box[2], box[1], -box[0], box[3]]
        return im, box

    chests, pickups = [], []
    arena_piece = _arena_hides_piece(sc)
    for gid in sorted(sc.gos):
        if not sc.active(gid):
            continue
        records = _component_records(sc, gid)
        if not any(t == 'PlayMakerFSM' and d['fsm']['name'] == 'Chest Control' for _, t, d in records):
            continue
        fsm = _fsm(records, 'Chest Control')
        states = _states(fsm, CHEST_TRANSITIONS, 'chest')
        variables = _variables(fsm)
        _, box = _one(records, 'BoxCollider2D')
        _, lid = _one(records, 'tk2dSprite')
        kids = _kids(sc, gid)
        if 'Opened' not in kids or 'Item' not in kids:
            raise ValueError('chest without Opened or Item')
        inside = _kids(sc, kids['Opened'])
        layers = []
        for k in ('Back', 'Front'):
            point, origin = sc.point(inside[k]), sc.point(gid)
            if abs(point[0] - origin[0]) > 1e-3 or abs(point[1] - origin[1]) > 1e-3:
                raise ValueError(f'chest {k} is not at the chest origin')
            _, s = _one(_component_records(sc, inside[k]), 'tk2dSprite')
            layers.append(tk((s['collection'], s['_spriteId'])))
        # `Range?` opens on any hit for a chest without a `Hero Region`.
        if 'Hero Region' in kids:
            _, region = _one(_component_records(sc, kids['Hero Region']), 'BoxCollider2D')
            reach = _box_world(sc, kids['Hero Region'], region)
        else:
            reach = [-30000.0, -30000.0, 30000.0, 30000.0]
        flings = _actions(states['Spawn Items'], 'FlingObjectsFromGlobalPool')
        if sorted(_scalar(f['spawnMin']) for f in flings) != ['Geo Large', 'Geo Med', 'Geo Small']:
            raise ValueError('chest payout is no longer three Geo flings')
        shapes = {tuple(float(_scalar(f[k])) for k in ('speedMin', 'speedMax', 'angleMin', 'angleMax')) for f in flings}
        if len(shapes) != 1:
            raise ValueError('chest Geo flings disagree on their throw')
        speed_min, speed_max, angle_min, angle_max = shapes.pop()
        # The lid's `Open` clip, which `Open` plays before `Spawn Items`.
        _, animator = _one(records, 'tk2dSpriteAnimator')
        library_obj = source.ref(sc.file, animator['library'])
        library = source.read(library_obj)
        clip = next((c for c in library['clips'] if c['name'] == 'Open'), None)
        if clip is None:
            raise ValueError('chest without its Open clip')
        opening = [tk((f['spriteCollection'], f['spriteId']), library_obj.assets_file) for f in clip['frames']]
        chests.append({'source': sc.sid(gid), 'position': sc.point(gid)[:2], 'z': sc.point(gid)[2],
                       'opening': opening, 'open_fps': float(clip['fps']),
                       'body': _box_world(sc, gid, box), 'reach': reach,
                       'geo': [int(variables.get(n) or 0) for n in ('Geo Small', 'Geo Med', 'Geo Large')],
                       'speed': [speed_min, speed_max], 'angle': [angle_min, angle_max],
                       'images': [tk((lid['collection'], lid['_spriteId'])), _stack(layers)],
                       'item_root': kids['Item']})
    in_chest = {}
    for local, c in enumerate(chests):
        stack = [c.pop('item_root')]
        while stack:
            g = stack.pop()
            in_chest[g] = local
            stack.extend(_kids(sc, g).values())
    refused = []
    for gid in sorted(sc.gos):
        records = _component_records(sc, gid)
        fsms = {d['fsm']['name']: d['fsm'] for _, t, d in records if t == 'PlayMakerFSM'}
        if 'Shiny Control' in fsms:
            fsm = fsms['Shiny Control']
            parent = sc.transforms[sc.go_transform[gid]]['m_Father']['m_PathID']
            parent_gid = sc.transforms[parent]['m_GameObject']['m_PathID'] if parent else None
            key_giver = parent_gid is not None and sc.gos[parent_gid]['m_Name'] == 'Key Giver'
            chest = in_chest.get(gid)
            # A fresh save activates only these: the rest belong to an NPC's
            # or an event's reward and wait on systems the port does not run.
            if chest is None and not key_giver and not sc.active(gid):
                continue
            grant = _grant(fsm)
            if grant is None:
                refused.append({'source': sc.sid(gid), 'reason': 'grants an item the port has no inventory for'})
                continue
            _states(fsm, SHINY_TRANSITIONS, 'shiny')
            region = _kids(sc, gid).get('Inspect Region') or (_kids(sc, parent_gid).get('Inspect Region') if parent_gid else None)
            if region is None:
                raise ValueError('shiny without its Inspect Region')
            _, reach = _one(_component_records(sc, region), 'BoxCollider2D')
            reach = _box_world(sc, region, reach)
            position = sc.point(gid)[:2]
            fling_from = None
            if _variables(fsm).get('Fling On Start') and chest is not None:
                # `Fling On Start` throws it left or right out of the opened
                # chest to land beside it. Here it rests against the chest's
                # right side, reachable from the floor there; the throw itself
                # is not reproduced.
                body = chests[chest]['body']
                position = [body[2] + 0.75, body[1] + 0.6]
                reach = [body[2], body[1], body[2] + 1.5, body[3]]
                fling_from = [(body[0] + body[2]) / 2, body[3]]
            pickups.append({'source': sc.sid(gid), 'position': position, 'z': sc.point(gid)[2], 'touch': False,
                            'reach': reach, 'grant': grant, 'chest': chest, 'after_false_knight': key_giver,
                            'fling_from': fling_from,
                            'name': _name(fsm, grant, ui), 'images': _glint(source, sc, records)[::GLINT_STEP],
                            'fps': 15.0 / GLINT_STEP,
                            'hide': []})
            continue
        found = [n for n in PIECE_FSMS if n in fsms]
        if not found or not sc.active(gid):
            continue
        fsm = fsms[found[0]]
        _states(fsm, PIECE_TRANSITIONS, found[0])
        number = _variables(fsm).get('Sprite')
        kids = _kids(sc, gid)
        sprite = kids.get(f'Sprite {number}') if isinstance(number, int) else kids.get('Sprite')
        if sprite is None:
            raise ValueError(f'{found[0]} without its sprite')
        srecords = _component_records(sc, sprite)
        renderer_id = next(i for i, t, _ in srecords if t == 'SpriteRenderer')
        _, renderer = _one(srecords, 'SpriteRenderer')
        _, circle = _one(records, 'CircleCollider2D')
        centre = sc.point(gid, circle['m_Offset']['x'], circle['m_Offset']['y'])
        r = circle['m_Radius'] * abs(sc.world(sc.go_transform[gid])[0][0])
        pickups.append({'source': sc.sid(gid), 'position': sc.point(sprite)[:2], 'z': sc.point(sprite)[2], 'touch': True,
                        'reach': [centre[0] - r, centre[1] - r, centre[0] + r, centre[1] + r],
                        'grant': (PIECE_FSMS[found[0]], 0), 'chest': None, 'after_false_knight': False, 'name': '',
                        'after_arena': arena_piece and found[0] == 'Heart Container Control',
                        'fling_from': None,
                        'images': [unity(source.ref(sc.file, renderer['m_Sprite']), bool(renderer['m_FlipX']))],
                        'fps': 1.0, 'hide': [sc.sid(renderer_id)]})
    if sum(p.get('after_arena', False) for p in pickups) > 1:
        raise ValueError('Battle Control hides one Heart Piece by tag; this scene has more')
    return chests, pickups, refused


def _arena_hides_piece(sc):
    """Whether the scene's `Battle Control` hides its Heart Piece until the arena
    is won: `PrePause` finds it by the `Heart Piece` tag and deactivates it, and
    `End Wait` (or `Activate`, on a later visit) brings it back. Brooding
    Mawlek's arena is the one that does."""
    from focus import action_fields
    for _, (kind, tree) in sc.objects.items():
        if kind != 'PlayMakerFSM' or tree['fsm']['name'] != 'Battle Control':
            continue
        for state in tree['fsm']['states']:
            if state['name'] != 'PrePause':
                continue
            data = state['actionData']
            fields = [(n.rsplit('.', 1)[-1], action_fields(data, i)) for i, n in enumerate(data['actionNames'])
                      if data['actionEnabled'][i]]
            finds = any(n == 'FindGameObject' and f.get('withTag', {}).get('value') == 'Heart Piece' for n, f in fields)
            hides = any(n == 'ActivateGameObject' and f.get('activate', {}).get('value') is False for n, f in fields)
            if finds and hides:
                return True
    return False


def _stack(layers):
    """tk2d layers drawn back to front at one origin, as one image and box."""
    ppu = max(im.width / (b[2] - b[0]) for im, b in layers)
    u = [min(b[0] for _, b in layers), min(b[1] for _, b in layers),
         max(b[2] for _, b in layers), max(b[3] for _, b in layers)]
    canvas = Image.new('RGBA', (math.ceil((u[2] - u[0]) * ppu), math.ceil((u[3] - u[1]) * ppu)))
    for im, b in layers:
        size = (max(1, round((b[2] - b[0]) * ppu)), max(1, round((b[3] - b[1]) * ppu)))
        canvas.alpha_composite(im.resize(size, Image.Resampling.LANCZOS),
                               (round((b[0] - u[0]) * ppu), round((u[3] - b[3]) * ppu)))
    return canvas, u


def _glint(source, sc, records):
    """The Shiny Item's Animator loop: one clip of Unity sprites."""
    from cook import native_sprite
    _, animator = _one(records, 'Animator')
    controller = source.ref(sc.file, animator['m_Controller']).read()
    if len(controller.m_AnimationClips) != 1:
        raise ValueError('shiny glint is no longer one clip')
    clip = controller.m_AnimationClips[0].read()
    if clip.m_SampleRate != 15:
        raise ValueError('shiny glint rate changed')
    out = []
    for pointer in clip.m_ClipBindingConstant.pptrCurveMapping:
        im, box = native_sprite(pointer.deref())
        out.append((im, list(box)))
    return out


NEEDLES = (b'Chest Control', b'Shiny Control', b'Heart Container Control', b'Vessel Fragment Control')


_COLLECTED = {}


def _collected(sc):
    """collect() for an admitted scene, once per scene file; None when it has
    nothing the port places (a byte scan rules most scenes out first)."""
    from quality import SCENE_TABLE
    name = Path(sc.file.name).name
    if name not in _COLLECTED:
        admitted = any(s['file'] == name for s in SCENE_TABLE)
        named = admitted and any(o.type.name == 'MonoBehaviour' and any(n in o.get_raw_data() for n in NEEDLES)
                                 for o in sc.file.objects.values())
        found = None
        if named:
            import language
            chests, pickups, refused = collect(sc, language.sheet(sc.source, 'UI'))
            found = (chests, pickups, refused) if chests or pickups else None
        _COLLECTED[name] = found
    return _COLLECTED[name]


def present(sc):
    """Whether the scene places a chest or a pickup."""
    return _collected(sc) is not None


# `Shiny Control`'s Knight clips: `Hero Down` plays Collect Normal 1 and
# waits 0.75 s, `Flash` plays Collect Normal 2 and waits 1.0 s, `Hero Up`
# plays Collect Normal 3 to its end.
KNEEL_CLIPS = ('Collect Normal 1', 'Collect Normal 2', 'Collect Normal 3')
# What a scene's views can take, most first. A view whose texture table is
# full retries the scene one step down (host/regions.py); 0 is no pickups.
# FULL: the lid's Open clip and the whole kneel; OPEN: the lid's clip and
# one held kneeling frame; LITE: the held frame only; BASIC: neither.
LEVEL_FULL, LEVEL_OPEN, LEVEL_LITE, LEVEL_BASIC = 4, 3, 2, 1
# Scenes held below FULL for linked RAM. The scene arena is sized by the
# largest scene plus its own metadata bank (host/pack_scenes.py), and
# Tutorial_01 is that scene: its 69,640-byte bank on top of 345,800 resident
# bytes at FULL set a 415,444-byte arena, 14,792 over the arena without the
# kneel and the lid's clip. At OPEN (the lid's clip, one kneeling frame) it
# sets 407,532 (the lid's four frames are 6,200 of it; LITE sets 401,332).
# Every other pickup scene is under the peak, so its
# extras cost nothing resident.
LEVEL_CAP = {'Tutorial_01': LEVEL_OPEN}
# The lowest level each scene is allowed to land on when its views' texture
# tables fill. Without a floor the retry below is silent: anything new appended
# to a scene bank (the arena gates' art was the first) pushes the chests and
# shinies out a level, or out entirely, and the cook still succeeds. The False
# Knight's views (Crossroads_10) sit at 639 of 640 records with LITE; every
# other pickup scene takes its cap.
LEVEL_FLOOR = {'Crossroads_10': LEVEL_LITE}


def level_floor(scene_name):
    return LEVEL_FLOOR.get(scene_name, LEVEL_CAP.get(scene_name, LEVEL_FULL))


def kneel_clips(source):
    """The Knight's three collect clips: unique images, and per clip its
    frame sequence (indices into the images) and fps."""
    from cook import tk_sprite
    rf = source.file('resources.assets')
    for o in rf.objects.values():
        if o.type.name != 'MonoBehaviour' or source.typename(o) != 'tk2dSpriteAnimation':
            continue
        library = source.read(o)
        clips = {c['name']: c for c in library['clips']}
        if not all(n in clips for n in KNEEL_CLIPS):
            continue
        images, index, textures, out = [], {}, {}, []
        for name in KNEEL_CLIPS:
            clip, seq = clips[name], []
            if clip['wrapMode'] != 2:
                raise ValueError(f'{name} no longer plays once')
            for f in clip['frames']:
                ref = source.ref(o.assets_file, f['spriteCollection'])
                key = (source.sid(ref), f['spriteId'])
                if key not in index:
                    index[key] = len(images)
                    im, box = tk_sprite(source, ref.assets_file, source.read(ref), f['spriteId'], textures)
                    images.append((im, list(box)))
                seq.append(index[key])
            out.append({'name': name, 'frames': seq, 'fps': float(clip['fps'])})
        return images, out
    raise ValueError('Knight collect clips not found')


def append_art(source, sc, atlas, frames, level=LEVEL_FULL):
    """Append the scene's chest and pickup frames to its actor bank.

    Returns the scene's record with frame indices relative to this bank's
    first appended frame, or None for a scene without any. `level` trims the
    extras for a view short of texture entries (LEVEL_*).
    """
    from cook import FOCAL, CAM_Z
    from quality import SCENE_TABLE
    import copy
    found = _collected(sc)
    if found is None:
        return None
    chests, pickups, refused = copy.deepcopy(found)
    names = {s['file']: s['scene_name'] for s in SCENE_TABLE}
    level = min(level, LEVEL_CAP.get(names[Path(sc.file.name).name], LEVEL_FULL))
    base = len(frames)

    def add(group, z, name):
        scale = FOCAL / (z - CAM_Z)
        firsts = atlas.add_frames_shared([(im, (b[2] - b[0]) * scale, (b[3] - b[1]) * scale) for im, b in group])
        out = []
        for texture, (_, b) in zip(firsts, group):
            out.append(len(frames) - base)
            frames.append({'texture': texture, 'box': b, 'sprite': name, 'event': {}})
        return out
    # Each is drawn at its own depth, as scenery is: its frames are sized and
    # projected with the same FOCAL/(z-CAM_Z).
    for c in chests:
        opening = c.pop('opening')
        c['images'] += opening if level >= LEVEL_OPEN else []
    for item in chests + pickups:
        item['frames'] = add(item.pop('images'), item['z'], item['source'])
        item['scale'] = round(FOCAL / (item.pop('z') - CAM_Z) * 4096)
    kneel = None
    if level >= LEVEL_LITE and any(not p['touch'] for p in pickups):
        images, clips = kneel_clips(source)
        if level < LEVEL_FULL:
            # Collect Normal 1's last frame, the kneel itself, for the whole time.
            held = clips[0]['frames'][-1]
            images, clips = [images[held]], [dict(c, frames=[0] * len(c['frames'])) for c in clips]
        kneel = {'frames': add(images, 0.0, 'Knight collect'), 'clips': clips}
    return {'scene_name': names[Path(sc.file.name).name], 'chests': chests, 'pickups': pickups, 'refused': refused,
            'kneel': kneel, 'level': level}


def bind(record, row, frame_base):
    """One view's binding: where its bank's pickup frames start, and the cooked
    draws of pickups this module draws itself."""
    if record is None:
        return None
    scene = json.loads((ROOT / f'data/regions/region-{row["chunk_id"]:03}/scene.json').read_text())
    hide = {s for p in record['pickups'] for s in p['hide']}
    return dict(record, frame_base=frame_base,
                hide=[i for i, d in enumerate(scene['draws']) if d['source'] in hide])


def rust(metadata):
    from geo import q16, rust_array
    from cook import FOCAL, CAM_Z
    b4 = lambda b: rust_array([q16(v) for v in b])
    rows = [(slot, r['scene_id'], r['pickups']) for slot, r in enumerate(metadata['regions']) if r.get('pickups')]
    scenes = {}
    for _, scene_id, record in rows:
        scenes.setdefault(scene_id, record)
    lines = ['// Generated by host/pickups.py from the source chests, shinies and pieces.',
             'pub const CHESTS:&[Chest]=&[']
    def coins(geo):
        # The same Geo in fewer coins: every five small coins become one
        # medium (1 and 5 Geo). The False Knight's 50/10/4 is 64 coins, the
        # whole coin pool at once, and their first flight took one tick past
        # the 16 VBlanks the input queue holds (a QueueFull panic on the
        # emulator); 0/20/4 is 24 coins and peaks at 5.
        small, medium, large = geo
        return [small % 5, medium + small // 5, large]
    for scene_id, record in sorted(scenes.items()):
        for local, c in enumerate(record['chests']):
            lines.append(f'Chest{{scene:{scene_id},local:{local},position:{b4(c["position"])},closed:{c["frames"][0]},'
                         f'opened:{c["frames"][1]},body:{b4(c["body"])},reach:{b4(c["reach"])},geo:{rust_array(coins(c["geo"]))},'
                         f'speed:{b4(c["speed"])},angle:{b4(c["angle"])},scale:{c["scale"]},'
                         f'open_first:{c["frames"][2] if len(c["frames"]) > 2 else 0},open_count:{len(c["frames"]) - 2},'
                         f'open_fps:{round(c.get("open_fps", 12.0) * 256)}}},')
    lines.append('];\npub const PICKUPS:&[Pickup]=&[')
    for scene_id, record in sorted(scenes.items()):
        for i, p in enumerate(record['pickups']):
            kind, value = p['grant']
            chest = NO_CHEST if p['chest'] is None else p['chest']
            lines.append(f'Pickup{{scene:{scene_id},local:{len(record["chests"]) + i},position:{b4(p["position"])},'
                         f'reach:{b4(p["reach"])},touch:{str(p["touch"]).lower()},grant:{GRANTS[kind].format(value)},'
                         f'chest:{chest},after_false_knight:{str(p["after_false_knight"]).lower()},'
                         f'after_arena:{str(p.get("after_arena", False)).lower()},'
                         f'first:{p["frames"][0]},count:{len(p["frames"])},fps:{round(p["fps"] * 256)},scale:{p["scale"]},'
                         f'fling:{"Some(" + b4(p["fling_from"]) + ")" if p.get("fling_from") else "None"},'
                         f'name:{json.dumps(p["name"])}}},')
    # Runs of consecutive views whose rooms put the pickup frames at the same
    # index: most of a scene's views share one.
    runs = []
    for slot, _, r in rows:
        if runs and runs[-1][1] == slot - 1 and runs[-1][2] == r['frame_base']:
            runs[-1][1] = slot
        else:
            runs.append([slot, slot, r['frame_base']])
    lines.append('];')
    for scene_id, record in scenes.items():
        # game/src/pickups.rs MAX_CHESTS and MAX_PICKUPS.
        if len(record['chests']) > 2 or len(record['pickups']) > 4:
            raise ValueError(f'scene {scene_id} places more chests or pickups than the guest seats')
    # The Knight's kneel: per scene, its first frame (relative to the pickup
    # frames) and whether it holds one frame (LITE); the clips once, globally.
    kneels = [(scene_id, r['kneel']) for scene_id, r in sorted(scenes.items()) if r.get('kneel')]
    lines.append('/// Scenes that kneel to a shiny: (scene, first kneel frame, one held frame).')
    lines.append('pub static KNEELS:&[(u16,u16,bool)]=&[' + ','.join(
        f'({sid},{k["frames"][0]},{str(len(k["frames"]) == 1).lower()})' for sid, k in kneels) + '];')
    full = next((k['clips'] for _, k in kneels if len(k['frames']) > 1), None)
    if full is None:
        full = [{'frames': [0], 'fps': 12.0}] * 3
    lines.append('/// Collect Normal 1, 2, 3: (frames into the scene kneel block, fps x256).')
    lines.append('pub static KNEEL_CLIPS:[(&[u8],u32);3]=[' + ','.join(
        f'(&{rust_array(c["frames"])},{round(c["fps"] * 256)})' for c in full) + '];')
    lines.append('/// Views of those scenes: (first slot, last slot, first pickup frame in their rooms).')
    lines.append('pub static BASES:&[(u16,u16,u16)]=&[' + ','.join(f'({a},{b},{c})' for a, b, c in runs) + '];')
    lines.append('/// Views whose cooked draws show a pickup this module draws itself.')
    lines.append('pub static HIDE:&[(u16,&[u16])]=&[' + ','.join(f'({slot},&{rust_array(r["hide"])})' for slot, _, r in rows if r['hide']) + '];')
    return '\n'.join(lines) + '\n'


def main():
    metadata = json.loads((ROOT / 'data/regions.json').read_text())
    text = rust(metadata)
    (ROOT / 'data/pickups.rs').write_text(text)
    report = {r['scene_name']: {k: r['pickups'][k] for k in ('chests', 'pickups', 'refused', 'level')}
              for r in metadata['regions'] if r.get('pickups')}
    report['refused_scenes'] = metadata.get('refused_pickups', [])
    (ROOT / '.hkpsx/pickups.json').write_text(json.dumps(report, indent=2) + '\n')
    scenes = [v for k, v in report.items() if k != 'refused_scenes']
    print(f'Pickups: {sum(len(v["chests"]) for v in scenes)} chests, {sum(len(v["pickups"]) for v in scenes)} '
          f'pickups in {len(scenes)} scenes; levels {sorted({k: v["level"] for k, v in report.items() if k != "refused_scenes"}.items())}; '
          f'step-downs {[(r["scene"], r["level"]) for r in report["refused_scenes"]]}')


if __name__ == '__main__':
    main()
