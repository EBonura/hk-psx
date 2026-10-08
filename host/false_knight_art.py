"""The False Knight's whole clip set, cooked into its own scene as a postpass.

`host/false_knight.py` binds the six clips the generic actor path could afford
and names them in `art_bindings`; this module replaces that art for the scene
the boss lives in. It cooks every clip `FalseyControl` plays on the body and
the `Hitter` overlay, the `Head` the armour exposes, the `Death Head` that
drops out of it, the empty armour `Body` it leaves behind and the three floor
sprites `Floor Control` swaps in, all from the installed source, at the size
the actor path already projects to. Nothing is resampled below that size.

What makes the whole set fit is where the bytes go, not how many there are:

- Static parts. A frame is split into horizontal bands, each trimmed to the
  texels that are not transparent, and the bands become ordinary static
  textures in the scene's own texture pages. They cost VRAM the scene was not
  using (Crossroads_10's scenery is 11 of the 19 pages) and no RAM at all,
  because a scene's pages are uploaded at the gate and never kept resident.
  A frame's bounding box is 42% texels; the bands keep almost none of the rest.
- Streamed parts. What the pages cannot hold is cut into cells of at most
  64x64, each trimmed the same way, and streamed from the scene arena through
  the shared animation slots exactly as the six clips were. The budget for
  those is the bytes the six clips used to take, so the scene does not grow.

The split is decided by measurement: clips are admitted in the order the
fight shows them, each one static if the scene's pages still pack (the same
MaxRects packer the scene bank uses), streamed if not, and the cook refuses
rather than shrinking art if neither budget holds.

Every part is its own frame record (texture plus its share of the world box),
appended with the scene-wide actor bank to every view of the scene, so every
view carries the same parts in the same order. The guest finds them through one
anchor clip whose first frame is part 0 in whichever view is live; the table
in `data/false_knight_art.rs` says which parts make up which frame of which
clip. The ActorSpec's own clip fields keep existing and point at `Blank`.
"""
import copy
import hashlib
import json
import math
import struct
from pathlib import Path

from source import ROOT
from audit_resident_bank import dense_pack, aligned
from cook import tk_sprite, native_sprite, guest_wrap, FOCAL, CAM_Z, SLOT_PIXELS, MAX_TEXTURE_AXIS
import false_knight as fk

# The clips the guest's `ArtClip` names, in its order. The first block is
# `FalseyControl`'s own, in `hk_sim::false_knight::Clip` order (which is
# `false_knight.CLIP_ORDER`), so a body clip converts by index. The rest are
# played on other objects or by states that clip enum does not carry.
BODY_CLIPS = tuple(fk.CLIP_ORDER)
EXTRA_CLIPS = ('Body', 'Head Idle', 'Head Hit', 'Head Spaz', 'Death Head 1', 'Death Head 2')
# The slam's ground wave is not the boss's own art: `S Attack Recover` spawns a
# pooled `Shockwave Wave`, which spawns a `Shockwave Spurt` every frame it
# moves, and the spurt's own animator plays this clip once. It is cooked into
# the same bank so it rides the same pages.
SPURT_CLIP = 'Shockwave Spurt'
ART_CLIPS = BODY_CLIPS + EXTRA_CLIPS + (SPURT_CLIP,)
# `Floor Control`'s sprites that start inactive, keyed by the state that shows
# them. `Crack` shows both Cracked sprites; `Break` and `Activate` show Broken.
FLOOR_STATES = (('Cracked', ('Cracked 1', 'Cracked 2')), ('Broken', ('Broken',)))
# The two floor sprites both of those states hide, and the armour the arena's
# `Init` destroys unless the fight was already won.
FLOOR_NORMAL = ('Normal 1', 'Normal 2')
ARMOUR_SPRITES = ('body', 'staff_piece')
# `Break Floor`'s three BoxCollider2Ds, all of which `Break` and `Activate`
# switch off with the GameObject.
BREAK_FLOOR = 'Break Floor'

# The order clips are offered to the static pages: what the fight shows most,
# and what shares the frame with the barrels, first. The stagger and the death
# sequence come last because no barrel is summoned during either, so a
# streamed frame there has the slots to itself.
PRIORITY = ('Floor', 'Idle', 'Turn', 'Jump Antic', 'Land', 'Jump', 'Attack Antic', 'Attack',
            'Attack Recover', SPURT_CLIP, 'Rage', 'Jump Attack Up', 'Jump Attack Hit 1', 'Jump Attack Hit 2',
            'Jump Attack Hit 3', 'Run Antic', 'Run', 'Stun Opened', 'Head Idle', 'Head Hit',
            'Head Spaz', 'Body', 'Stun Open', 'Stun Hit', 'Stun Roll', 'Stun Roll End',
            'Stun Recover', 'Death Fall', 'Death Land', 'Death Spaz', 'Death Head 1',
            'Death Head 2', 'Blank')
# pack_scenes.MAX_PAGES is 19 (page 19 is the animation cache's second
# region). The plan packs the scenery it can see beside the parts, which is not
# exactly the set the scene bank packs after similarity dedup and plane
# sharing: planned at 19 the real bank came to 20. One page of margin, and the
# scene bank's own check stays the one that refuses.
SCENE_PAGE_LIMIT = 18
# What streams through the arena. The six clips this replaces streamed 146,340
# bytes; the whole set streams about 30 KiB more once the floor's wide sprites
# and the page margin above take their share, and the scene arena (sized by the
# largest resident scene, which this makes Crossroads_10) pays for it.
STREAM_BYTES_LIMIT = 180 * 1024
# A part costs a 16-byte texture record, a 20-byte frame record, a 20-byte
# alpha cover and a quad a frame, and every view carries every part against
# HKROOM02's 640-record table; the decomposition trades that against texels at
# these rates. Streamed texels are the scarcer budget, so a streamed part is
# allowed to be smaller.
PART_COST = 256
STREAMED_PART_COST = 192
# Streamed parts each bind an animation slot, and the rage keeps up to eight
# barrels on screen beside the boss: twelve is what the six clips' largest
# frame already bound.
MAX_STREAMED_PARTS = 12
MAX_STATIC_PARTS = 16


def _bytes(rect):
    return aligned(rect[2]) // 2 * rect[3]


def decompose(plane, w, h, streamed):
    """The cheapest cut of a frame into trimmed parts, by dynamic programming.

    Parts are bands cut on a column grid that starts at the frame's first
    opaque column, each cell trimmed to its own texels. A streamed cell is at
    most one animation slot; a static one is at most what a texture page can
    address, so art wider than that (the floor) is two or more columns. Each
    part costs its 4bpp texel bytes plus its part cost, and a frame may not
    exceed its part cap. Returns [(x, y, w, h)] in pixel space.
    """
    width = SLOT_PIXELS if streamed else MAX_TEXTURE_AXIS
    xs = [x for y in range(h) for x in range(w) if plane[y * w + x]]
    left = min(xs) if xs else 0
    columns = max(1, -(-(w - left) // width))
    # Per row and column: (first, last) opaque x or None.
    spans = []
    for y in range(h):
        row = plane[y * w:(y + 1) * w]
        cells = []
        for c in range(columns):
            x0 = left + c * width
            x1 = min(w, x0 + width)
            hit = [x for x in range(x0, x1) if row[x]]
            cells.append((hit[0], hit[-1]) if hit else None)
        spans.append(cells)
    tallest = SLOT_PIXELS if streamed else min(h, 256)
    cap = MAX_STREAMED_PARTS if streamed else MAX_STATIC_PARTS
    best = [(0, 0, None)] + [None] * h
    for y1 in range(1, h + 1):
        choice = None
        # Grow the band upward from row y1-1, keeping each column's extents.
        ext = [None] * columns
        for y0 in range(y1 - 1, max(0, y1 - tallest) - 1, -1):
            for c in range(columns):
                span = spans[y0][c]
                if span is None:
                    continue
                e = ext[c]
                ext[c] = [span[0], span[1], y0, y0] if e is None else \
                    [min(e[0], span[0]), max(e[1], span[1]), y0, e[3]]
            if best[y0] is None:
                continue
            pieces = [(e[0], e[2], e[1] - e[0] + 1, e[3] - e[2] + 1) for e in ext if e is not None]
            count = best[y0][1] + len(pieces)
            if count > cap:
                continue
            cost = best[y0][0] + sum(_bytes(p) + (STREAMED_PART_COST if streamed else PART_COST) for p in pieces)
            if choice is None or (cost, count) < choice[:2]:
                choice = (cost, count, (y0, pieces))
        best[y1] = choice
    if best[h] is None:
        raise ValueError(f'a {w}x{h} frame cannot be cut into {cap} parts')
    parts, y = [], h
    while y:
        y0, pieces = best[y][2]
        parts = pieces + parts
        y = y0
    return parts


def _lerp(a, b, at, of):
    return a + (b - a) * at // of


def part_box(box_q16, w, h, rect):
    """A part's share of the frame's world box, in the frame record's order.

    Art row 0 is the top of the box (`b[3]`), as `hk_format::Room::frame_tile`
    has it. Two parts that meet on a pixel row compute that edge from the same
    arguments, so the seam is exact.
    """
    x, y, pw, ph = rect
    b = box_q16
    return [_lerp(b[0], b[2], x, w), _lerp(b[3], b[1], y + ph, h),
            _lerp(b[0], b[2], x + pw, w), _lerp(b[3], b[1], y, h)]


def _children(sc, gid):
    tid = sc.go_transform[gid]
    return {sc.gos[t['m_GameObject']['m_PathID']]['m_Name']: t['m_GameObject']['m_PathID']
            for i, t in sc.transforms.items() if t['m_Father']['m_PathID'] == tid}


def _component(sc, gid, kind):
    from actors import _component_records
    found = [tree for _, k, tree in _component_records(sc, gid) if k == kind]
    if len(found) != 1:
        raise ValueError(f'expected one {kind} on {sc.gos[gid]["m_Name"]}')
    return found[0]


def _local(sc, gid):
    t = sc.transforms[sc.go_transform[gid]]
    return t['m_LocalPosition'], t['m_LocalScale']


def source_art(s, sc, actor):
    """Every art sprite the bank needs, with its size, box and owning clip.

    Returns (sprites, clips, objects): sprites maps a key to (image, box, w, h)
    where box is the world box relative to the object the guest positions it
    by; clips maps an art clip name to its tk2d record and sprite keys; objects
    carries the placement facts the guest needs beside the art.
    """
    fk_gid = actor['game_object']
    library_o = s.ref(sc.file, actor['tk2dSpriteAnimator']['library'])
    library = s.read(library_o)
    matrix = sc.world(sc.go_transform[fk_gid])
    sprite = actor['tk2dSprite']
    scale = abs(matrix[0][0] * sprite['_scale']['x']), abs(matrix[1][1] * sprite['_scale']['y'])
    if abs(scale[0] - scale[1]) > 1e-6:
        raise ValueError('False Knight art assumes a uniform transform scale')
    children = _children(sc, fk_gid)
    head, death_head = children['Head'], children['Death Head']
    (head_pos, head_scale), (dh_pos, dh_scale) = _local(sc, head), _local(sc, death_head)
    for gid in (head, death_head):
        tk = _component(sc, gid, 'tk2dSprite')
        if tk['_color']['a'] != 1.0 or tk['_scale']['x'] != 1.0 or tk['_scale']['y'] != 1.0:
            raise ValueError('False Knight child sprite is tinted or scaled')
    if abs(head_scale['x'] - 1) > 1e-6 or abs(dh_scale['x'] - dh_scale['y']) > 1e-6:
        raise ValueError('False Knight Head/Death Head scale changed')
    # The Head rides the body's transform, so its local offset is folded into
    # its boxes; the Death Head leaves on its own body, so its boxes stay
    # relative to its own transform and the guest carries the offset.
    head_offset = (head_pos['x'] * scale[0], head_pos['y'] * scale[1])
    death_head_scale = scale[0] * dh_scale['x']
    per_clip_scale = {name: (scale[0], (0.0, 0.0)) for name in BODY_CLIPS + ('Body',)}
    for name in ('Head Idle', 'Head Hit', 'Head Spaz'):
        per_clip_scale[name] = (scale[0], head_offset)
    for name in ('Death Head 1', 'Death Head 2'):
        per_clip_scale[name] = (death_head_scale, (0.0, 0.0))
    textures, collections, sprites, clips = {}, {}, {}, {}
    project = FOCAL / -CAM_Z
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    wave = shockwave_source(s, sc, fk_gid)
    per_clip_scale[SPURT_CLIP] = (1.0, (0.0, 0.0))
    for name in ART_CLIPS:
        lib_o = wave['library'] if name == SPURT_CLIP else library_o
        clip = wave['clip'] if name == SPURT_CLIP else by_name[name]
        factor, (ox, oy) = per_clip_scale[name]
        keys = []
        for frame in clip['frames']:
            collection_o = s.ref(lib_o.assets_file, frame['spriteCollection'])
            sid = s.sid(collection_o)
            if sid not in collections:
                collections[sid] = s.read(collection_o)
            key = (sid, frame['spriteId'], round(factor, 6), round(ox, 6), round(oy, 6))
            if key not in sprites:
                image, box = tk_sprite(s, collection_o.assets_file, collections[sid], frame['spriteId'], textures)
                box = (box[0] * factor + ox, box[1] * factor + oy, box[2] * factor + ox, box[3] * factor + oy)
                w = math.ceil((box[2] - box[0]) * project)
                h = math.ceil((box[3] - box[1]) * project)
                if w > MAX_TEXTURE_AXIS or h > MAX_TEXTURE_AXIS:
                    raise ValueError(f'False Knight frame {w}x{h} exceeds the texture axis')
                sprites[key] = (image, box, max(1, w), max(1, h))
            keys.append(key)
        clips[name] = {'record': clip, 'keys': keys}
    # Floor sprites: SpriteRenderers on `FK Floor`, placed relative to it.
    floor_gid = next(gid for gid, go in sc.gos.items() if go['m_Name'] == 'FK Floor'
                     and gid >= 100000)
    floor_children = _children(sc, floor_gid)
    floor_matrix = sc.world(sc.go_transform[floor_gid])
    anchor = (floor_matrix[0][3], floor_matrix[1][3])
    floor_frames = {}
    for state, names in FLOOR_STATES:
        keys = []
        for name in names:
            gid = floor_children[name]
            renderer = _component(sc, gid, 'SpriteRenderer')
            if renderer['m_Color']['a'] != 1.0 or renderer['m_FlipX'] or renderer['m_FlipY']:
                raise ValueError(f'floor sprite {name} is tinted or flipped')
            m = sc.world(sc.go_transform[gid])
            if abs(m[0][1]) > 1e-6 or abs(m[1][0]) > 1e-6:
                raise ValueError(f'floor sprite {name} is rotated')
            image, box = native_sprite(s.ref(sc.file, renderer['m_Sprite']))
            sx, sy = m[0][0], m[1][1]
            box = (m[0][3] - anchor[0] + box[0] * sx, m[1][3] - anchor[1] + box[1] * sy,
                   m[0][3] - anchor[0] + box[2] * sx, m[1][3] - anchor[1] + box[3] * sy)
            w = math.ceil((box[2] - box[0]) * project)
            h = math.ceil((box[3] - box[1]) * project)
            key = ('floor', name)
            if h > MAX_TEXTURE_AXIS:
                raise ValueError(f'floor sprite {name} is {h} texels tall')
            sprites[key] = (image, box, max(1, w), max(1, h))
            keys.append(key)
        floor_frames[state] = keys
    # `Set Head Facing` writes Death Head Speed 4 (or -4 when facing left) and
    # `Blow` launches the Death Head at it; read the value, not the prose.
    control = fk._fsms(sc, fk_gid)['FalseyControl']['fsm']
    speed = fk._variables(control)['Death Head Speed']
    objects = {
        'wave': wave['params'],
        'death_head_speed': speed,
        'death_head_offset': (dh_pos['x'] * scale[0], dh_pos['y'] * scale[1]),
        'floor_anchor': anchor,
        'floor_sources': {name: sc.sid(_component_id(sc, floor_children[name], 'SpriteRenderer'))
                          for name in FLOOR_NORMAL},
        'break_floor_sources': [sc.sid(i) for i in _component_ids(sc, floor_children[BREAK_FLOOR], 'BoxCollider2D')],
    }
    armour = next(gid for gid, go in sc.gos.items() if go['m_Name'] == 'FK Armour' and gid >= 100000)
    armour_children = _children(sc, armour)
    objects['armour_sources'] = [sc.sid(_component_id(sc, armour_children[name], 'SpriteRenderer'))
                                 for name in ARMOUR_SPRITES]
    # The armour's `Tinger` is a TinkEffect box the cook turns into a nail
    # bounce; it goes with the armour.
    tinger = sc.sid(_component_id(sc, armour_children['Tinger'], 'BoxCollider2D'))
    from actors import pogo_sources
    objects['armour_tink'] = [t['bounds'] for t in pogo_sources(sc)['targets'] if t['source'] == tinger]
    if len(objects['armour_tink']) != 1:
        raise ValueError('FK Armour Tinger is no longer one static nail bounce')
    return sprites, clips, floor_frames, objects


def _state(fsm, name):
    found = [state for state in fsm['states'] if state['name'] == name]
    if len(found) != 1:
        raise ValueError(f'{fsm["name"]} has no single state {name}')
    return found[0]


def _actions(state, kind):
    """Enabled actions of one type in a state, as (fields, index)."""
    from focus import action_fields
    data = state['actionData']
    return [(action_fields(data, i, objects=True), i) for i, n in enumerate(data['actionNames'])
            if n.rsplit('.', 1)[-1] == kind and data['actionEnabled'][i]]


def _one(state, kind):
    found = _actions(state, kind)
    if len(found) != 1:
        raise ValueError(f'{state["name"]} has {len(found)} {kind} actions, expected one')
    return found[0][0]


def _scalar(value):
    if not isinstance(value, dict) or value.get('useVariable'):
        raise ValueError(f'expected a literal, got {value!r}')
    return value['value']


def _enum(state, index, name):
    """A PlayMaker enum parameter (FloatOperator.operation), as its int."""
    data = state['actionData']
    start = data['actionStartIndex'][index]
    end = data['actionStartIndex'][index + 1] if index + 1 < len(data['actionNames']) else len(data['paramName'])
    for i in range(start, end):
        if data['paramName'][i] == name:
            pos, size = data['paramDataPos'][i], data['paramByteDataSize'][i]
            return struct.unpack('<i', bytes(data['byteData'][pos:pos + 4]))[0] if size >= 4 else None
    raise ValueError(f'{state["name"]} action lacks {name}')


def _snapshot_name(s, sc, state, index):
    data = state['actionData']
    start = data['actionStartIndex'][index]
    end = data['actionStartIndex'][index + 1] if index + 1 < len(data['actionNames']) else len(data['paramName'])
    for i in range(start, end):
        if data['paramDataType'][i] == 24:
            ref = data['fsmObjectParams'][data['paramDataPos'][i]].get('value')
            if ref and ref.get('m_PathID'):
                return s.read(s.ref(sc.file, ref)).get('m_Name')
    raise ValueError(f'{state["name"]} names no snapshot')


def _prefab_parts(s, go_o):
    """(typename, tree) for every component of a prefab GameObject."""
    go = go_o.read_typetree()
    out = []
    for c in go['m_Component']:
        o = s.ref(go_o.assets_file, c['component'])
        out.append((s.typename(o) if o.type.name == 'MonoBehaviour' else o.type.name, o, s.read(o) if o.type.name == 'MonoBehaviour' else o.read_typetree()))
    return go, out


def _prefab_box(parts):
    boxes = [t for kind, _, t in parts if kind == 'BoxCollider2D']
    if len(boxes) != 1 or not boxes[0]['m_IsTrigger']:
        raise ValueError('expected one trigger BoxCollider2D')
    b = boxes[0]
    ox, oy, w, h = b['m_Offset']['x'], b['m_Offset']['y'], b['m_Size']['x'], b['m_Size']['y']
    return (ox - w / 2, oy - h / 2, ox + w / 2, oy + h / 2)


def shockwave_source(s, sc, fk_gid):
    """`S Attack Recover`'s ground wave, read from the two pooled prefabs.

    `Shockwave Wave` (FSM `shockwave`) starts at `Speed` times the factor in
    `Start Move`, adds twice the set speed every second, and stops on a
    terrain trigger or when its 1.6-unit ground ray finds nothing. Every frame
    it moves it spawns a `Shockwave Spurt`, which is what the player sees and
    what hurts: its `Damage timing` FSM arms the DamageHero after one wait and
    disarms it after the next, and it recycles when its clip ends.
    """
    control = fk._fsms(sc, fk_gid)['FalseyControl']['fsm']
    recover = _state(control, 'S Attack Recover')
    origin = _one(recover, 'SetVector3XYZ')
    spawn = _one(recover, 'SpawnObjectFromGlobalPool')
    set_speed = _one(recover, 'SetFsmFloat')
    if set_speed['variableName'].get('value') != 'Speed' or set_speed['fsmName'].get('value') != 'shockwave':
        raise ValueError('S Attack Recover no longer writes the wave Speed')
    speed = _scalar(set_speed['setValue'])
    wave_o = s.ref(sc.file, spawn['gameObject']['value'])
    wave_go, wave_parts = _prefab_parts(s, wave_o)
    if wave_go['m_Name'] != 'Shockwave Wave':
        raise ValueError(f'S Attack Recover spawns {wave_go["m_Name"]}')
    fsm = next(t['fsm'] for kind, _, t in wave_parts if kind == 'PlayMakerFSM' and t['fsm']['name'] == 'shockwave')
    start = _state(fsm, 'Start Move')
    (operator, op_index), = _actions(start, 'FloatOperator')
    if _enum(start, op_index, 'operation') != 2:
        raise ValueError('Start Move no longer multiplies the incrementer')
    accel_factor = _scalar(operator['float2'])
    start_factor = _scalar(_one(start, 'FloatMultiplyV2')['multiplyBy'])
    move = _state(fsm, 'Move')
    ray = _scalar(_one(move, 'RayCast2d')['distance'])
    _one(move, 'Trigger2dEventLayer')
    spurt_o = None
    data = _state(fsm, 'Right')['actionData']
    for i, n in enumerate(data['actionNames']):
        if n.endswith('SetGameObject') and data['actionEnabled'][i]:
            st = data['actionStartIndex'][i]
            en = data['actionStartIndex'][i + 1] if i + 1 < len(data['actionNames']) else len(data['paramName'])
            for j in range(st, en):
                if data['paramDataType'][j] == 19:
                    v = data['fsmGameObjectParams'][data['paramDataPos'][j]].get('value')
                    if v and v.get('m_PathID'):
                        spurt_o = s.ref(wave_o.assets_file, v)
    if spurt_o is None:
        raise ValueError('the wave no longer names its spurt')
    spurt_go, spurt_parts = _prefab_parts(s, spurt_o)
    if spurt_go['m_Name'] != 'Shockwave Spurt':
        raise ValueError(f'the wave spawns {spurt_go["m_Name"]}')
    timing = next(t['fsm'] for kind, _, t in spurt_parts if kind == 'PlayMakerFSM' and t['fsm']['name'] == 'Damage timing')
    arm = _scalar(_one(_state(timing, 'Wait'), 'Wait')['time'])
    armed = _scalar(_one(_state(timing, 'Activate'), 'Wait')['time'])
    damage = _one(_state(timing, 'Activate'), 'SetDamageHeroAmount')['damageDealt']
    damage = _scalar(damage) if isinstance(damage, dict) else damage
    off = _one(_state(timing, 'Deactivate'), 'SetDamageHeroAmount')['damageDealt']
    if (_scalar(off) if isinstance(off, dict) else off) != 0:
        raise ValueError('the spurt no longer disarms its DamageHero')
    animator = next(t for kind, _, t in spurt_parts if kind == 'tk2dSpriteAnimator')
    library = s.ref(spurt_o.assets_file, animator['library'])
    clip = s.read(library)['clips'][animator['defaultClipId']]
    if clip['name'] != SPURT_CLIP or guest_wrap(clip) != 2:
        raise ValueError('the spurt no longer plays Shockwave Spurt once')
    lifetime = len(clip['frames']) / clip['fps']
    q = lambda v: round(v * 65536)
    t = lambda seconds: round(seconds * 60)
    # `Floor Break` takes the mixer to `Silent`, which is where the fight's
    # music ends; the guest fades its CD-DA over the same time.
    snap_state = _state(control, 'Floor Break')
    (snap, snap_index), = _actions(snap_state, 'TransitionToAudioSnapshot')
    snapshot = _snapshot_name(s, sc, snap_state, snap_index)
    if snapshot != 'Silent':
        raise ValueError(f'Floor Break now transitions to {snapshot}')
    params = {
        'origin_y': q(_scalar(origin['y'])),
        'start_speed': q(speed * start_factor),
        'accel': q(speed * accel_factor),
        'box': [q(v) for v in _prefab_box(wave_parts)],
        'ground_ray': q(ray),
        'spurt_box': [q(v) for v in _prefab_box(spurt_parts)],
        'damage_from': t(arm), 'damage_to': t(arm + armed), 'damage': int(damage),
        'spurt_ticks': t(lifetime),
        'silence_ticks': t(_scalar(snap['transitionTime'])),
        'sources': {'wave': s.sid(wave_o), 'spurt': s.sid(spurt_o), 'library': s.sid(library)},
    }
    return {'library': library, 'clip': clip, 'params': params}


def _component_ids(sc, gid, kind):
    return [c['component']['m_PathID'] for c in sc.gos[gid]['m_Component']
            if c['component']['m_PathID'] in sc.objects and sc.objects[c['component']['m_PathID']][0] == kind]


def _component_id(sc, gid, kind):
    found = _component_ids(sc, gid, kind)
    if len(found) != 1:
        raise ValueError(f'expected one {kind} on {sc.gos[gid]["m_Name"]}')
    return found[0]


def scenery_rects(base_packs):
    """Distinct static textures of a scene's views, as the scene bank packs them."""
    from region_delta import textures
    seen = set()
    rects = []
    for raw in base_packs:
        count = struct.unpack_from('<6I', raw, 8)[1]
        for i, blob in enumerate(textures(raw)):
            if struct.unpack_from('<H', raw, 40 + i * 16)[0] == 65535 or blob in seen:
                continue
            seen.add(blob)
            w, h = struct.unpack_from('<HH', blob)
            rects.append((aligned(w), h))
    return rects


def plan(sprites, clips, floor_frames, scenery, quantize, priority=PRIORITY, page_limit=SCENE_PAGE_LIMIT,
         stream_limit=STREAM_BYTES_LIMIT, label='False Knight'):
    """Decide each sprite's residency and parts. Refuses rather than resizes.

    The defaults are the False Knight's; host/mawlek_art.py passes its own
    order and budgets, and a bank with no floor passes no floor frames."""
    order = []
    for name in priority:
        keys = [k for state in floor_frames.values() for k in state] if name == 'Floor' else clips[name]['keys']
        order.append((name, list(dict.fromkeys(keys))))
    quantized, cut = {}, {}
    static, streamed = [], []
    decisions = []
    stream_bytes = 0
    for name, keys in order:
        fresh = [k for k in keys if k not in cut]
        for k in fresh:
            if k not in quantized:
                image, box, w, h = sprites[k]
                quantized[k] = quantize(image, w, h)
        trial = {k: decompose(quantized[k][2], sprites[k][2], sprites[k][3], False) for k in fresh}
        rects = list(scenery) + [(aligned(r[2]), r[3]) for k in static for r in cut[k][1]] \
            + [(aligned(r[2]), r[3]) for k in fresh for r in trial[k]]
        pages, _ = dense_pack([(w, h, i) for i, (w, h) in enumerate(rects)]) if rects else (0, [])
        if pages <= page_limit:
            for k in fresh:
                cut[k] = (False, trial[k])
                static.append(k)
            decisions.append({'clip': name, 'residency': 'static', 'sprites': len(fresh), 'scene_pages': pages,
                              'texel_bytes': sum(_bytes(r) for k in fresh for r in trial[k])})
            continue
        cells = {k: decompose(quantized[k][2], sprites[k][2], sprites[k][3], True) for k in fresh}
        extra = sum(_bytes(r) for k in fresh for r in cells[k])
        if stream_bytes + extra > stream_limit:
            raise ValueError(f'{label} clip {name} fits neither the {page_limit} scene pages '
                             f'nor the {stream_limit}-byte stream budget ({stream_bytes}+{extra})')
        stream_bytes += extra
        for k in fresh:
            cut[k] = (True, cells[k])
            streamed.append(k)
        decisions.append({'clip': name, 'residency': 'streamed', 'sprites': len(fresh), 'scene_pages': pages,
                          'texel_bytes': extra})
    rects = list(scenery) + [(aligned(r[2]), r[3]) for k in static for r in cut[k][1]]
    final_pages, _ = dense_pack([(w, h, i) for i, (w, h) in enumerate(rects)])
    return quantized, cut, decisions, {'scene_pages': final_pages, 'stream_bytes': stream_bytes,
                                       'static_bytes': sum(_bytes(r) for k in static for r in cut[k][1]),
                                       'static_parts': sum(len(cut[k][1]) for k in static),
                                       'streamed_parts': sum(len(cut[k][1]) for k in streamed),
                                       'sprites': len(cut)}


def append_bank(atlas, frames, clips, sprites, art_clips, floor_frames, quantized, cut, names=ART_CLIPS,
                anchor_name='False Knight parts'):
    """Append every part as a frame record, plus the anchor clip over all of them.

    Returns the table the guest links: per sprite its first part and part count
    (relative to the anchor's first frame), per art clip its sprite sequence.
    `names` is the guest's clip order; the defaults are the False Knight's.
    """
    first = len(frames)
    sprite_index, sprite_rows = {}, []
    ordered = [k for name in names for k in art_clips[name]['keys']] \
        + [k for state in floor_frames.values() for k in state]
    for key in dict.fromkeys(ordered):
        streamed, parts = cut[key]
        _, box, w, h = sprites[key]
        _, palette, plane = quantized[key]
        from cook import Atlas as _Atlas
        box_q16 = [round(v * 65536) for v in box]
        start = len(frames) - first
        for rect in parts:
            x, y, pw, ph = rect
            texture = atlas.add_quantized(pw, ph, palette, _Atlas._pack_plane(plane, w, x, y, pw, ph), streamed)
            frames.append({'texture': texture, 'box': [v / 65536 for v in part_box(box_q16, w, h, rect)],
                           'box_q16': part_box(box_q16, w, h, rect), 'sprite': str(key), 'event': {}})
        sprite_index[key] = len(sprite_rows)
        sprite_rows.append((start, len(parts), int(streamed)))
    anchor = len(clips)
    clips.append({'name': anchor_name, 'start': first, 'count': max(1, len(frames) - first),
                  'fps': 1., 'wrap': 2, 'loopStart': 0})
    clip_rows, sequence = [], []
    for name in names:
        record = art_clips[name]['record']
        clip_rows.append((len(sequence), len(art_clips[name]['keys']), round(record['fps'] * 65536),
                          guest_wrap(record), record.get('loopStart', 0), name))
        sequence += [sprite_index[k] for k in art_clips[name]['keys']]
    floor_rows = [(state, [sprite_index[k] for k in keys]) for state, keys in floor_frames.items()]
    return anchor, sprite_rows, clip_rows, sequence, floor_rows


def rust_table(anchor_clip, sprite_rows, clip_rows, sequence, floor_rows, objects, bindings):
    del bindings  # they go to data/false_knight_floor.rs
    """data/false_knight_art.rs."""
    lines = ['// Generated by host/false_knight_art.py from the installed source; do not edit.',
             '// The False Knight\'s parts: see docs/FALSE_KNIGHT.md.',
             f'pub const FK_ART_ANCHOR_CLIP: u16 = {anchor_clip};',
             '/// Per sprite: first part (frames after the anchor clip\'s first), parts, streamed.',
             f'pub const FK_ART_SPRITES: [(u16, u8, bool); {len(sprite_rows)}] = [']
    lines += [f'    ({a}, {b}, {str(bool(c)).lower()}),' for a, b, c in sprite_rows]
    lines += ['];', '/// Per art clip, in `ArtClip` order: first sequence entry, frames, fps (Q16), wrap, loop start.',
              f'pub const FK_ART_CLIPS: [(u16, u8, u32, u8, u8); {len(clip_rows)}] = [']
    lines += [f'    ({a}, {b}, {c}, {d}, {e}), // {n}' for a, b, c, d, e, n in clip_rows]
    lines += ['];', f'pub const FK_ART_SEQUENCE: [u16; {len(sequence)}] = [' + ', '.join(map(str, sequence)) + '];']
    for state, indices in floor_rows:
        lines.append(f'pub const FK_FLOOR_{state.upper()}: [u16; {len(indices)}] = [' + ', '.join(map(str, indices)) + '];')
    ax, ay = objects['floor_anchor']
    dx, dy = objects['death_head_offset']
    lines += [f'/// `FK Floor`\'s world position, which the floor sprites\' boxes are relative to.',
              f'pub const FK_FLOOR_ANCHOR: [i32; 2] = [{round(ax * 65536)}, {round(ay * 65536)}];',
              f'/// The `Death Head`\'s local position, times the body\'s scale, as authored (facing left).',
              f'pub const FK_DEATH_HEAD_OFFSET: [i32; 2] = [{round(dx * 65536)}, {round(dy * 65536)}];',
              f'/// `Death Head Speed`, Q16 units a second.',
              f'pub const FK_DEATH_HEAD_SPEED: i32 = {round(objects["death_head_speed"] * 65536)};']
    w = objects['wave']
    box = lambda v: '[' + ', '.join(map(str, v)) + ']'
    lines += [f'/// `Shockwave Wave` ({w["sources"]["wave"]}) and its `Shockwave Spurt` ({w["sources"]["spurt"]}).',
              f'/// Spawn height below the body\'s transform, `S Attack Recover`\'s SetVector3XYZ.',
              f'pub const FK_WAVE_ORIGIN_Y: i32 = {w["origin_y"]};',
              f'/// `Start Move`: the set Speed times its factor, then twice the set Speed added a second (Q16).',
              f'pub const FK_WAVE_START_SPEED: i32 = {w["start_speed"]};',
              f'pub const FK_WAVE_ACCEL: i32 = {w["accel"]};',
              f'/// The wave\'s terrain trigger and ground ray, rightward frame, Q16.',
              f'pub const FK_WAVE_BOX: [i32; 4] = {box(w["box"])};',
              f'pub const FK_WAVE_GROUND_RAY: i32 = {w["ground_ray"]};',
              f'/// A spurt\'s DamageHero box, rightward frame, Q16, armed from tick FROM to TO of its life.',
              f'pub const FK_SPURT_BOX: [i32; 4] = {box(w["spurt_box"])};',
              f'pub const FK_SPURT_DAMAGE_FROM: u16 = {w["damage_from"]};',
              f'pub const FK_SPURT_DAMAGE_TO: u16 = {w["damage_to"]};',
              f'pub const FK_SPURT_DAMAGE: u16 = {w["damage"]};',
              f'/// Ticks a spurt lives: its clip, played once, then recycled.',
              f'pub const FK_SPURT_TICKS: u16 = {w["spurt_ticks"]};',
              f'/// `Floor Break`\'s TransitionToAudioSnapshot to `Silent`, in ticks.',
              f'pub const FK_FLOOR_BREAK_SILENCE_TICKS: u16 = {w["silence_ticks"]};']
    return '\n'.join(lines) + '\n'


def rust_bindings(bindings, scene_id):
    """data/false_knight_floor.rs: what `game/src/battle_gates.rs` moves."""
    lines = ['// Generated by host/false_knight_art.py from the cooked region report; do not edit.',
             '// (catalogue slot, index) rows, sorted by slot for a binary search.',
             '/// The guest scene id `Floor Control` and `FK Armour` stand in.',
             f'pub const FK_FLOOR_SCENE: usize = {scene_id};']
    for name, rows in bindings.items():
        lines.append(f'/// {rows["doc"]}')
        if 'boxes' in rows:
            lines.append(f'pub const {name}: [[i32; 4]; {len(rows["boxes"])}] = [' +
                         ', '.join('[' + ', '.join(map(str, b)) + ']' for b in rows['boxes']) + '];')
            continue
        lines.append(f'pub const {name}: [(u16, u16); {len(rows["rows"])}] = [' +
                     ', '.join(f'({a}, {b})' for a, b in rows['rows']) + '];')
    return '\n'.join(lines) + '\n'


def region_bindings(rows, objects):
    """Per catalogue slot, the draws and edges the floor and armour state move."""
    normal, armour, edges = [], [], []
    tink = {tuple(round(v * 65536) for v in box) for box in objects['armour_tink']}
    normal_sources = set(objects['floor_sources'].values())
    armour_sources = set(objects['armour_sources'])
    floor_sources = set(objects['break_floor_sources'])
    for row in rows:
        # The runtime's catalogue slot is the row's index, which the cook
        # numbers from 1 as chunk ids (host/battle_gates.py keys the same way).
        slot = row['chunk_id'] - 1
        draws = json.loads((ROOT / 'data/regions' / f'region-{row["chunk_id"]:03}' / 'scene.json').read_text())['draws']
        for i, d in enumerate(draws):
            if d['source'] in normal_sources:
                normal.append((slot, i))
            if d['source'] in armour_sources:
                armour.append((slot, i))
        for i, source in enumerate(row['edge_sources']):
            if source in floor_sources:
                edges.append((slot, i))
    for table in (normal, armour, edges):
        if any(b > 65535 or a > 65535 for a, b in table):
            raise ValueError('floor binding index exceeds u16')
    return {
        'FK_FLOOR_NORMAL_DRAWS': {'doc': '(slot, draw) of `Normal 1`/`Normal 2`, hidden once the floor cracks.',
                                  'rows': sorted(normal)},
        'FK_ARMOUR_DRAWS': {'doc': '(slot, draw) of `FK Armour`, which `Battle Control` destroys unless the arena was won.',
                            'rows': sorted(armour)},
        'FK_BREAK_FLOOR_EDGES': {'doc': '(slot, edge) of `Break Floor`\'s colliders, lifted once the floor breaks.',
                                 'rows': sorted(edges)},
        'FK_ARMOUR_TINK': {'doc': 'World box of `FK Armour`\'s `Tinger` nail bounce, which goes with the armour.',
                           'boxes': sorted(tink)},
    }


def neutral_actor(actor):
    """The boss as the generic actor bank should cook it: `Blank` for every body
    slot, so only the barrel and a one-pixel clip ride that bank."""
    actor = copy.deepcopy(actor)
    control = actor['movement_control']
    control['art_bindings'] = {kind: 'Blank' for kind in control['art_bindings']}
    actor['limitations'] = [text for text in actor['limitations'] if 'clips are cooked' not in text] + [
        'Every clip FalseyControl plays, the Head, the Death Head, the empty armour and the floor states '
        'are cooked by host/false_knight_art.py into this scene alone; the ActorSpec clip fields point at Blank.']
    return actor


def cook_scene_bank(s, sc, actor, rows, atlas, frames, clips):
    """Append the whole bank to the scene's shared actor atlas.

    `rows` are the scene's report rows (their immutable base packs give the
    scenery the pages already hold). Returns the report entry and a function
    that writes data/false_knight_art.rs once the bank's clip base is known.
    """
    sprites, art_clips, floor_frames, objects = source_art(s, sc, actor)
    base = [(ROOT / 'data/regions' / f'region-{r["chunk_id"]:03}' / 'room.hk').read_bytes() for r in rows]
    scenery = scenery_rects(base)
    quantized, cut, decisions, totals = plan(sprites, art_clips, floor_frames, scenery, atlas._quantize)
    anchor, sprite_rows, clip_rows, sequence, floor_rows = append_bank(
        atlas, frames, clips, sprites, art_clips, floor_frames, quantized, cut)
    bindings = region_bindings(rows, objects)
    report = {'decisions': decisions, **totals, 'art_clips': list(ART_CLIPS),
              'anchor_clip_in_bank': anchor, 'bindings': {k: len(v.get('rows', v.get('boxes', ()))) for k, v in bindings.items()},
              'code_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}

    def write(clip_base):
        for name, text in (('false_knight_art.rs', rust_table(anchor + clip_base, sprite_rows, clip_rows,
                                                               sequence, floor_rows, objects, bindings)),
                           ('false_knight_floor.rs', rust_bindings(bindings, rows[0]['scene_id']))):
            path = ROOT / 'data' / name
            if not path.is_file() or path.read_text() != text:
                path.write_text(text)
        report['anchor_clip'] = anchor + clip_base
        (ROOT / '.hkpsx/false-knight').mkdir(parents=True, exist_ok=True)
        (ROOT / '.hkpsx/false-knight/art.json').write_text(json.dumps(report, indent=2) + '\n')
    return report, write


# --- Gruz Mother (`Giant Fly`, Crossroads_04) --------------------------------
#
# The third boss bank through the same decomposition. host/gruzzer.py admits
# the one `Giant Fly` with the one-frame `Charge` clip in both generic slots;
# this section, below the cook_inputs divider, checks every number
# shared/hk-sim/src/gruz_mother.rs runs against the installed source and cooks
# every clip the body, its corpse and the corpse's burster play into
# Crossroads_04's own bank. data/gruz_art.rs carries the bank's tables and the
# geometry the guest needs beside the art: `Battle Range`, `Hero Damager` and
# the burster's box, all relative to the object they hang off.

# `hk_sim::gruz_mother::Clip` order. All seventeen come from one library
# (the body's, which the corpse and the burster share); `Corpse Fly` is the
# corpse prefab's default `Fly`.
GRUZ_CLIPS = ('Sleep', 'Wake', 'Fly', 'Charge Antic', 'Charge', 'Charge Recover', 'Slam Down', 'Slam Up',
              'Slam End', 'Corpse Fly', 'Death', 'Fall', 'Wiggle', 'Stop', 'Gurgle Once', 'Gurgle Loop', 'Burst')
GRUZ_LIBRARY_CLIP = {name: ('Fly' if name == 'Corpse Fly' else name) for name in GRUZ_CLIPS}
# What the fight shows most goes to the static pages first.
GRUZ_PRIORITY = ('Fly', 'Sleep', 'Charge Antic', 'Charge', 'Charge Recover', 'Slam Down', 'Slam Up', 'Slam End',
                 'Wake', 'Corpse Fly', 'Death', 'Fall', 'Wiggle', 'Stop', 'Gurgle Once', 'Gurgle Loop', 'Burst')
GRUZ_SCENE_PAGE_LIMIT = 18
GRUZ_STREAM_BYTES_LIMIT = 96 * 1024
GRUZ_CORPSE = 'Corpse Big Fly 1'
GRUZ_BURSTER = 'Corpse Big Fly Burster'
# Every number hk_sim::gruz_mother runs, as the source serializes it.
GRUZ_CONTRACT = {
    'HEALTH': 90, 'INVULNERABLE_SECONDS': .25, 'CONTACT_DAMAGE': 1,
    'WAKE_SPEED_Y': 2.5, 'FLY_SECONDS': 1., 'BUZZ_SPEED': 5., 'SUPER_WAIT_SECONDS': (2., 2.8),
    'CHOOSE_MAX': (3, 2), 'CHARGES_IN_A_ROW': 3, 'SLAMS_IN_A_ROW': 2,
    'CHARGE_ANTIC_SECONDS': .75, 'CHARGE_BACK_SPEED': 3., 'CHARGE_SPEED': 26., 'CHARGE_RECOVER_SECONDS': .3,
    'SUPER_END_SECONDS': .5, 'SLAM_ANTIC_SECONDS': .5, 'SLAM_SECONDS': (2.5, 3.), 'SLAM_SPEED': 50.,
    'SLAM_ANGLES_LEFT': (100., 260.), 'SLAM_ANGLES_RIGHT': (80., 280.), 'SLAM_END_SECONDS': .75, 'SLAM_DECEL': .85,
    'CORPSE_SECONDS': (.5, 3., 1.), 'BURSTER_SPEED': (12.5, 20.), 'BURSTER_GRAVITY': 1., 'BURSTER_BOUNCE': (.5, 1.),
    'BURSTER_INIT_SECONDS': .1, 'BURSTER_GEO': 50, 'BURSTER_GEO_FLING': ((15., 30.), (80., 100.), (.75, .75)),
    'BURSTER_SECONDS': (1., .5, 2., 2., 2., 1.9, .16), 'BATTLE_ENEMIES': 7, 'END_WAIT_SECONDS': 2.,
    'SCALE': 1.25,
    'CLIPS': {'Sleep': (9, 12., 0), 'Wake': (4, 10., 2), 'Fly': (8, 12., 0), 'Charge Antic': (4, 12., 2),
              'Charge': (1, 30., 0), 'Charge Recover': (10, 12., 1), 'Slam Down': (2, 8., 2), 'Slam Up': (2, 8., 2),
              'Slam End': (12, 12., 1), 'Death': (4, 20., 3), 'Fall': (3, 12., 2), 'Wiggle': (3, 12., 3),
              'Stop': (3, 12., 2), 'Gurgle Once': (5, 10., 2), 'Gurgle Loop': (4, 10., 0), 'Burst': (7, 12., 2)},
}
# Structural digests (host/false_knight.py `fsm_digest`) of every state machine
# the fight runs, so a change the tables above do not name still refuses.
GRUZ_FSM_SHA256 = {
    'bouncer_control': '18b3af97f5b91be591cfeab5a39a64e6cb5b8495f45a49a5c6500e7a12b8dfcc',
    'Big Fly Control': 'c5bb9452a43ba4a1608efbb9b0bb72db2b42f9b8416c1fa722464d2f1ab5b377',
    'Battle Control': '658e51436bdc380aa83db0e1f02ae68addcbafb50449e8575076da29c29510c5',
    'corpse': 'fc20d5e005b2b3ad17a11971b1104435e146618ad87e53c16c52ac1ba5c71c5c',
    'burster': 'a466f287c7484796c4a86f0d26f29a154f852bc01ab89c8d8ab8e3ba127fe440',
}


def _gruz_value(field):
    if isinstance(field, dict):
        if field.get('useVariable'):
            raise ValueError(f'expected a literal, got the variable {field.get("name")!r}')
        return field['value']
    return field


def _gruz_require(what, got, want):
    if isinstance(want, (tuple, list)):
        ok = len(got) == len(want) and all(abs(float(g) - float(w)) <= 1e-5 for g, w in zip(got, want))
    else:
        ok = abs(float(got) - float(want)) <= 1e-5
    if not ok:
        raise ValueError(f'Gruz Mother {what} is {got}, the guest runs {want}')


def gruz_sources(s, sc):
    """The objects the contract and the art are read from."""
    from actors import _component_records
    import gruzzer
    body = gruzzer.giant_fly(sc)
    if body is None:
        raise ValueError('no Giant Fly in ' + sc.file.name)
    children = _children(sc, body)
    fsms = {name: data['fsm'] for name, data in fk._fsms(sc, body).items()}
    battle = fk._named(sc, 'Battle Scene')
    fsms['Battle Control'] = fk._fsms(sc, battle)['Battle Control']['fsm']
    components = {kind: tree for _, kind, tree in _component_records(sc, body)}
    corpse_o = s.ref(sc.file, components['EnemyDeathEffects']['corpsePrefab'])
    corpse = _prefab(s, corpse_o)
    if corpse[1]['m_Name'] != GRUZ_CORPSE:
        raise ValueError('Gruz Mother corpse prefab is now ' + corpse[1]['m_Name'])
    fsms['corpse'] = _prefab_fsm(corpse, 'corpse')
    blow = fk._states(fsms['corpse'])['Blow']
    burster_ref = [f['gameObject']['value'] for f, _ in _actions(blow, 'CreateObject')
                   if isinstance(f.get('gameObject'), dict)]
    bursters = [b for b in (_prefab(s, s.ref(corpse_o.assets_file, ref)) for ref in burster_ref)
                if b[1]['m_Name'] == GRUZ_BURSTER]
    if len(bursters) != 1:
        raise ValueError('the Gruz Mother corpse no longer blows out one burster')
    burster = bursters[0]
    fsms['burster'] = _prefab_fsm(burster, 'burster')
    return {'body': body, 'children': children, 'fsms': fsms, 'components': components,
            'corpse': corpse, 'burster': burster, 'battle': battle}


def _prefab(s, go_o):
    """(GameObject object, tree, {typename: [(object, tree)]}) of a prefab root."""
    go = go_o.read_typetree()
    parts = {}
    for c in go['m_Component']:
        o = s.ref(go_o.assets_file, c['component'])
        kind = s.typename(o) if o.type.name == 'MonoBehaviour' else o.type.name
        parts.setdefault(kind, []).append((o, s.read(o) if o.type.name == 'MonoBehaviour' else o.read_typetree()))
    return go_o, go, parts


def _prefab_fsm(prefab, name):
    found = [t['fsm'] for _, t in prefab[2].get('PlayMakerFSM', []) if t['fsm']['name'] == name]
    if len(found) != 1:
        raise ValueError(f'{prefab[1]["m_Name"]} lacks its {name} FSM')
    return found[0]


def gruz_check_contract(s, sc, src):
    """Read every number `GRUZ_CONTRACT` names back out of the source."""
    c = GRUZ_CONTRACT
    fsms = src['fsms']
    digests = {name: fk.fsm_digest(fsm) for name, fsm in fsms.items()}
    for name, digest in GRUZ_FSM_SHA256.items():
        if digests.get(name) != digest:
            raise ValueError(f'Gruz Mother FSM {name} changed: {digests.get(name)}')
    comp = src['components']
    health = comp['HealthManager']
    _gruz_require('hp', health['hp'], c['HEALTH'])
    _gruz_require('invulnerableTime', health['invulnerableTime'], c['INVULNERABLE_SECONDS'])
    if any(health[k] for k in ('smallGeoDrops', 'mediumGeoDrops', 'largeGeoDrops')) or health['battleScene']['m_PathID']:
        raise ValueError('Gruz Mother now drops Geo or counts in its arena')
    if 'DamageHero' in comp or 'Recoil' in comp:
        raise ValueError('Gruz Mother body now hurts or recoils on its own')
    damager = src['children']['Hero Damager']
    _gruz_require('Hero Damager', _component(sc, damager, 'DamageHero')['damageDealt'], c['CONTACT_DAMAGE'])
    m = sc.world(sc.go_transform[src['body']])
    _gruz_require('scale', (m[0][0], m[1][1]), (c['SCALE'], c['SCALE']))
    st = fk._states(fsms['Big Fly Control'])
    wait = lambda name: fk._wait_seconds(st[name])
    v = _one(st['Wake'], 'SetVelocity2d')
    # x is PlayMaker's None: it keeps the sleeping body's zero.
    if not (v['x'].get('useVariable') and not v['x'].get('name')):
        raise ValueError('Wake now sets an x velocity')
    _gruz_require('Wake velocity', _gruz_value(v['y']), c['WAKE_SPEED_Y'])
    _gruz_require('Fly', wait('Fly'), c['FLY_SECONDS'])
    r = _one(st['Buzz'], 'RandomFloat')
    _gruz_require('Buzz', (_gruz_value(r['min']), _gruz_value(r['max'])), c['SUPER_WAIT_SECONDS'])
    from focus import action_parameters
    (_, index), = _actions(st['Super Choose'], 'SendRandomEventV2')
    params = [value for _, value in action_parameters(st['Super Choose']['actionData'], index)]
    events = [p for p in params if isinstance(p, str)]
    literals = [_gruz_value(p) for p in params if isinstance(p, dict) and not p.get('useVariable')]
    if events != ['CHARGE', 'SLAM'] or literals != [1.0, 1.0, c['CHOOSE_MAX'][0], c['CHOOSE_MAX'][1]]:
        raise ValueError(f'Super Choose changed: {params}')
    _gruz_require('Charge Antic in a row', _gruz_value(_one(st['Charge Antic'], 'IntCompare')['integer2']),
                  c['CHARGES_IN_A_ROW'])
    _gruz_require('Slam Antic in a row', _gruz_value(_one(st['Slam Antic'], 'IntCompare')['integer2']),
                  c['SLAMS_IN_A_ROW'])
    _gruz_require('Charge Antic', wait('Charge Antic'), c['CHARGE_ANTIC_SECONDS'])
    _gruz_require('Charge back', _gruz_value(_one(st['Charge Antic'], 'SetVelocityAsAngle')['speed']),
                  c['CHARGE_BACK_SPEED'])
    _gruz_require('Charge back angle', _gruz_value(_one(st['Charge Antic'], 'FloatAdd')['add']), 180.)
    _gruz_require('Charge', _gruz_value(_one(st['Charge'], 'SetVelocityAsAngle')['speed']), c['CHARGE_SPEED'])
    for side in 'LRUD':
        name = 'Charge Recover ' + side
        _gruz_require(name, wait(name), c['CHARGE_RECOVER_SECONDS'])
        divides = [_gruz_value(f['divideBy']) for f, _ in _actions(st[name], 'FloatDivide')]
        (mul, _), = _actions(st[name], 'FloatMultiply')
        axis = 'Self Vel X' if side in 'LR' else 'Self Vel Y'
        if divides != [2., 2.] or _gruz_value(mul['multiplyBy']) != -1. or mul['floatVariable']['name'] != axis:
            raise ValueError(f'{name} no longer halves and mirrors {axis}')
    _gruz_require('Recover End', _gruz_value(_one(st['Recover End'], 'SetFloatValue')['floatValue']),
                  c['SUPER_END_SECONDS'])
    _gruz_require('Slam Antic', wait('Slam Antic'), c['SLAM_ANTIC_SECONDS'])
    r = _one(st['Slam Antic'], 'RandomFloat')
    _gruz_require('Slam Time', (_gruz_value(r['min']), _gruz_value(r['max'])), c['SLAM_SECONDS'])
    _gruz_require('Slam Speed', fk._variables(fsms['Big Fly Control'])['Slam Speed'], c['SLAM_SPEED'])
    for name, key in (('Go Left', 'SLAM_ANGLES_LEFT'), ('Turn Left', 'SLAM_ANGLES_LEFT'),
                      ('Go Right', 'SLAM_ANGLES_RIGHT'), ('Turn Right', 'SLAM_ANGLES_RIGHT')):
        values = [_gruz_value(f['floatValue']) for f, _ in _actions(st[name], 'SetFloatValue')]
        _gruz_require(name, values, c[key])
    for name, sign in (('Turn Left', 1), ('Turn Right', -1)):
        _gruz_require(name + ' scale', _gruz_value(_one(st[name], 'SetScale')['x']), sign * c['SCALE'])
    _gruz_require('Slam End', wait('Slam End'), c['SLAM_END_SECONDS'])
    _gruz_require('Slam End decel', _gruz_value(_one(st['Slam End'], 'DecelerateV2')['deceleration']), c['SLAM_DECEL'])
    _gruz_require('Slam End time', _gruz_value(_one(st['Slam End'], 'SetFloatValue')['floatValue']), 0.)
    bouncer = fk._variables(fsms['bouncer_control'])
    _gruz_require('bouncer Speed', bouncer['Speed'], c['BUZZ_SPEED'])
    if not bouncer['Starts Inactive'] or bouncer['Start Up']:
        raise ValueError('bouncer_control no longer starts stopped')
    corpse = fk._states(fsms['corpse'])
    _gruz_require('corpse waits', [fk._wait_seconds(corpse[n]) for n in ('Init', 'Steam', 'Ready')], c['CORPSE_SECONDS'])
    v = _one(corpse['Blow'], 'SetVelocity2d')
    scale_x = _gruz_value(_one(corpse['Blow'], 'FloatMultiply')['multiplyBy'])
    _gruz_require('burster launch', (scale_x * c['SCALE'], _gruz_value(v['y'])), c['BURSTER_SPEED'])
    _, _, corpse_parts = src['corpse']
    if 'Rigidbody2D' in corpse_parts:
        raise ValueError('the Gruz Mother corpse now has a body and would be flung')
    burster = fk._states(fsms['burster'])
    _gruz_require('burster waits', [fk._wait_seconds(burster[n]) for n in
                                    ('Landed', 'Stop Emit', 'Stop', 'Gurg 1', 'Gurg 2', 'Gurg 3', 'Burst')],
                  c['BURSTER_SECONDS'])
    _gruz_require('burster Initiate', fk._wait_seconds(burster['Initiate']), c['BURSTER_INIT_SECONDS'])
    geo = _one(burster['Geo'], 'FlingObjectsFromGlobalPool')
    _gruz_require('Geo count', (_gruz_value(geo['spawnMin']), _gruz_value(geo['spawnMax'])),
                  (c['BURSTER_GEO'], c['BURSTER_GEO']))
    speed, angle, spread = c['BURSTER_GEO_FLING']
    _gruz_require('Geo speed', (_gruz_value(geo['speedMin']), _gruz_value(geo['speedMax'])), speed)
    _gruz_require('Geo angle', (_gruz_value(geo['angleMin']), _gruz_value(geo['angleMax'])), angle)
    _gruz_require('Geo spread', (_gruz_value(geo['originVariationX']), _gruz_value(geo['originVariationY'])), spread)
    _, _, burster_parts = src['burster']
    rb = burster_parts['Rigidbody2D'][0][1]
    bounce = burster_parts['ObjectBounce'][0][1]
    _gruz_require('burster gravity', rb['m_GravityScale'], c['BURSTER_GRAVITY'])
    _gruz_require('burster bounce', (bounce['bounceFactor'], bounce['speedThreshold']), c['BURSTER_BOUNCE'])
    battle = fk._states(fsms['Battle Control'])
    _gruz_require('Battle Enemies', _gruz_value(_one(battle['Start'], 'SetIntValue')['intValue']), c['BATTLE_ENEMIES'])
    _gruz_require('End Wait', fk._wait_seconds(battle['End Wait']), c['END_WAIT_SECONDS'])
    library = s.read(s.ref(sc.file, comp['tk2dSpriteAnimator']['library']))
    by_name = {clip['name']: clip for clip in library['clips'] if clip['name']}
    for name, (frames, fps, wrap) in c['CLIPS'].items():
        clip = by_name[name]
        _gruz_require(name, (len(clip['frames']), clip['fps'], clip['wrapMode']), (frames, fps, wrap))
    return digests


def gruz_rust_constants(path=ROOT / 'shared/hk-sim/src/gruz_mother.rs'):
    """Every `pub const NAME: type = value;` in the Rust module, evaluated."""
    import re
    found = {}
    for match in re.finditer(r'pub const (\w+)\s*:\s*[^=]+=\s*([^;]+);', path.read_text()):
        raw = re.sub(r'//.*', '', match.group(2)).strip()
        try:
            found[match.group(1)] = eval(raw.replace('ONE', '65536'), {'__builtins__': {}}, {})
        except Exception:
            continue
    return found


def gruz_expected_rust():
    """`GRUZ_CONTRACT` in the guest's units, keyed by the Rust constant names."""
    from combat import HIT_EVASION_SECONDS, ticks
    c = GRUZ_CONTRACT
    q = lambda v: round(v * 65536)
    t = lambda pair: [ticks(v) for v in pair]
    clip_ticks = lambda name: ticks(c['CLIPS'][name][0] / c['CLIPS'][name][1])
    angle = math.radians(c['SLAM_ANGLES_RIGHT'][0])
    return {
        'HEALTH': c['HEALTH'], 'INVULNERABLE_TICKS': ticks(HIT_EVASION_SECONDS),
        'CONTACT_DAMAGE': c['CONTACT_DAMAGE'], 'WAKE_SPEED_Y': q(c['WAKE_SPEED_Y']),
        'WAKE_TICKS': clip_ticks('Wake'), 'FLY_TICKS': ticks(c['FLY_SECONDS']), 'BUZZ_SPEED': q(c['BUZZ_SPEED']),
        'SUPER_WAIT_TICKS': t(c['SUPER_WAIT_SECONDS']), 'CHOOSE_MAX': list(c['CHOOSE_MAX']),
        'CHARGES_IN_A_ROW': c['CHARGES_IN_A_ROW'], 'SLAMS_IN_A_ROW': c['SLAMS_IN_A_ROW'],
        # Both antics end on the `Charge Antic` clip's completion before their
        # Wait: the animator's AnimationCompleted delegate `Wake` assigned is
        # never cleared (hk_sim::gruz_mother::CHARGE_ANTIC_TICKS).
        'CHARGE_ANTIC_TICKS': min(ticks(c['CHARGE_ANTIC_SECONDS']), clip_ticks('Charge Antic')),
        'CHARGE_BACK_SPEED': q(c['CHARGE_BACK_SPEED']),
        'CHARGE_SPEED': q(c['CHARGE_SPEED']), 'CHARGE_RECOVER_TICKS': ticks(c['CHARGE_RECOVER_SECONDS']),
        'SUPER_END_TICKS': ticks(c['SUPER_END_SECONDS']),
        'SLAM_ANTIC_TICKS': min(ticks(c['SLAM_ANTIC_SECONDS']), clip_ticks('Charge Antic')),
        'SLAM_TICKS': t(c['SLAM_SECONDS']), 'SLAM_SPEED': q(c['SLAM_SPEED']),
        'SLAM_DIRECTION': [q(math.cos(angle)), q(math.sin(angle))],
        'SLAM_HIT_TICKS': clip_ticks('Slam Down'), 'SLAM_END_TICKS': ticks(c['SLAM_END_SECONDS']),
        # DecelerateV2 runs on FixedUpdate (50 Hz); the guest steps at 60 Hz.
        'SLAM_DECEL': q(c['SLAM_DECEL'] ** (50 / 60)),
        'CORPSE_TICKS': t(c['CORPSE_SECONDS']), 'BURSTER_SPEED': [q(v) for v in c['BURSTER_SPEED']],
        'BURSTER_GRAVITY': q(c['BURSTER_GRAVITY'] * 60), 'BURSTER_BOUNCE': q(c['BURSTER_BOUNCE'][0]),
        'BURSTER_BOUNCE_THRESHOLD': q(c['BURSTER_BOUNCE'][1]),
        'BURSTER_INIT_TICKS': ticks(c['BURSTER_INIT_SECONDS']), 'BURSTER_GEO': c['BURSTER_GEO'],
        'BURSTER_TICKS': t(c['BURSTER_SECONDS']), 'BATTLE_ENEMIES': c['BATTLE_ENEMIES'],
    }


def gruz_check_rust():
    rust = gruz_rust_constants()
    for name, want in gruz_expected_rust().items():
        got = rust.get(name)
        if isinstance(want, list):
            got = list(got) if got is not None else None
        if got != want:
            raise ValueError(f'shared/hk-sim/src/gruz_mother.rs {name} is {got}, the source says {want}')


def gruz_geometry(s, sc, src):
    """`Battle Range`, `Hero Damager` and the burster's box, relative to their
    owner's transform in the authored (left-facing) pose."""
    body = src['body']
    origin = sc.world(sc.go_transform[body])
    ox, oy = origin[0][3], origin[1][3]
    children = src['children']
    damager = children['Hero Damager']
    m = sc.world(sc.go_transform[damager])
    box = _component(sc, damager, 'BoxCollider2D')
    if not box['m_IsTrigger']:
        raise ValueError('Hero Damager is no longer a trigger')
    cx = m[0][3] + box['m_Offset']['x'] * m[0][0] - ox
    cy = m[1][3] + box['m_Offset']['y'] * m[1][1] - oy
    hw, hh = abs(box['m_Size']['x'] * m[0][0]) / 2, abs(box['m_Size']['y'] * m[1][1]) / 2
    damager_box = [cx - hw, cy - hh, cx + hw, cy + hh]
    rng = children['Battle Range']
    m = sc.world(sc.go_transform[rng])
    poly = _component(sc, rng, 'PolygonCollider2D')
    paths = poly['m_Points']['m_Paths']
    if len(paths) != 1 or not 3 <= len(paths[0]) <= 16 or not poly['m_IsTrigger']:
        raise ValueError('Battle Range is no longer one trigger path of at most 16 points')
    off = poly['m_Offset']
    points = [[m[0][3] + (p['x'] + off['x']) * m[0][0] - ox, m[1][3] + (p['y'] + off['y']) * m[1][1] - oy]
              for p in paths[0]]
    _, _, parts = src['burster']
    bbox = parts['BoxCollider2D'][0][1]
    scale = parts['Transform'][0][1]['m_LocalScale']
    bx, by = bbox['m_Offset']['x'] * scale['x'], bbox['m_Offset']['y'] * scale['y']
    bw, bh = bbox['m_Size']['x'] * abs(scale['x']) / 2, bbox['m_Size']['y'] * abs(scale['y']) / 2
    spawn = fk._named(sc, 'Fly Spawn')
    return {'damager_box': damager_box, 'range': points, 'burster_box': [bx - bw, by - bh, bx + bw, by + bh],
            'fly_spawn': sc.point(spawn)[:2]}


def gruz_source_art(s, sc, src):
    """Every sprite of the seventeen clips at the 1.25 scale all three objects
    draw at. PingPong clips (`Death`, `Wiggle`) are cooked as the loop tk2d
    plays: forward, then back without repeating either end."""
    textures, collections, sprites, clips, boxes = {}, {}, {}, {}, {}
    project = FOCAL / -CAM_Z
    comp = src['components']
    lib_o = s.ref(sc.file, comp['tk2dSpriteAnimator']['library'])
    library = s.read(lib_o)
    for prefab in (src['corpse'], src['burster']):
        _, _, parts = prefab
        anim_o, anim = parts['tk2dSpriteAnimator'][0]
        if s.sid(s.ref(anim_o.assets_file, anim['library'])) != s.sid(lib_o):
            raise ValueError(f'{prefab[1]["m_Name"]} no longer shares the Giant Fly library')
        if parts['tk2dSprite'][0][1]['_color'] != {'r': 1.0, 'g': 1.0, 'b': 1.0, 'a': 1.0}:
            raise ValueError(f'{prefab[1]["m_Name"]} is tinted')
    tk = comp['tk2dSprite']
    if tk['_color'] != {'r': 1.0, 'g': 1.0, 'b': 1.0, 'a': 1.0} or tk['_scale']['x'] != 1 or tk['_scale']['y'] != 1:
        raise ValueError('Giant Fly sprite is tinted or scaled')
    scale = GRUZ_CONTRACT['SCALE']
    by_name = {c['name']: c for c in library['clips'] if c['name']}
    for name in GRUZ_CLIPS:
        clip = by_name[GRUZ_LIBRARY_CLIP[name]]
        keys = []
        for frame in clip['frames']:
            collection_o = s.ref(lib_o.assets_file, frame['spriteCollection'])
            sid = s.sid(collection_o)
            if sid not in collections:
                collections[sid] = s.read(collection_o)
            key = (sid, frame['spriteId'], scale)
            if key not in sprites:
                image, box = tk_sprite(s, collection_o.assets_file, collections[sid], frame['spriteId'], textures)
                box = tuple(v * scale for v in box)
                w = math.ceil((box[2] - box[0]) * project)
                h = math.ceil((box[3] - box[1]) * project)
                if w > MAX_TEXTURE_AXIS or h > MAX_TEXTURE_AXIS:
                    raise ValueError(f'Gruz Mother {name} frame {w}x{h} exceeds the texture axis')
                sprites[key] = (image, box, max(1, w), max(1, h))
                # tk2d writes a Box sprite's own collider into the object's
                # BoxCollider2D whenever the frame changes (colliderType 2:
                # vertices are the centre and the half extents); an Unset
                # sprite (0) leaves whatever box the last one wrote.
                definition = collections[sid]['spriteDefinitions'][frame['spriteId']]
                kind = definition['colliderType']
                if kind == 2:
                    (cx, cy), (hx, hy) = [(v['x'], v['y']) for v in definition['colliderVertices'][:2]]
                    boxes[key] = [(cx - hx) * scale, (cy - hy) * scale, (cx + hx) * scale, (cy + hy) * scale]
                elif kind == 0:
                    boxes[key] = None
                else:
                    raise ValueError(f'Gruz Mother {name} sprite has collider type {kind}')
            keys.append(key)
        record = clip
        if clip['wrapMode'] == 3:
            # tk2d PingPong over n frames is the 2n-2 loop 0..n-1..1.
            keys = keys + keys[-2:0:-1]
            record = dict(clip, wrapMode=0, frames=[None] * len(keys), loopStart=0)
        clips[name] = {'record': record, 'keys': keys}
    return sprites, clips, boxes


def gruz_rust_table(anchor_clip, sprite_rows, clip_rows, sequence, geo, scene_id, sprite_boxes):
    q = lambda v: round(v * 65536)
    box = lambda v: '[' + ', '.join(str(q(x)) for x in v) + ']'
    points = ', '.join(f'[{q(p[0])}, {q(p[1])}]' for p in geo['range'])
    lines = ['// Generated by host/false_knight_art.py (Gruz Mother) from the installed source; do not edit.',
             '// Gruz Mother\'s parts and geometry: see shared/hk-sim/src/gruz_mother.rs.',
             f'pub const GZ_SCENE: usize = {scene_id};',
             f'pub const GZ_ART_ANCHOR_CLIP: u16 = {anchor_clip};',
             '/// Per sprite: first part (frames after the anchor clip\'s first), parts, streamed.',
             f'pub const GZ_ART_SPRITES: [(u16, u8, bool); {len(sprite_rows)}] = [']
    lines += [f'    ({a}, {b}, {str(bool(c)).lower()}),' for a, b, c in sprite_rows]
    lines += ['];', '/// Per art clip, in `hk_sim::gruz_mother::Clip` order: first sequence entry,',
              '/// frames, fps (Q16), wrap, loop start.',
              f'pub const GZ_ART_CLIPS: [(u16, u8, u32, u8, u8); {len(clip_rows)}] = [']
    lines += [f'    ({a}, {b}, {c}, {d}, {e}), // {n}' for a, b, c, d, e, n in clip_rows]
    lines += ['];', f'pub const GZ_ART_SEQUENCE: [u16; {len(sequence)}] = [' + ', '.join(map(str, sequence)) + '];',
              '/// `Hero Damager`\'s trigger and `Battle Range`\'s polygon, relative to the body, as',
              '/// authored (facing left), Q16; and the burster\'s box relative to its transform.',
              f'pub const GZ_DAMAGER_BOX: [i32; 4] = {box(geo["damager_box"])};',
              f'pub const GZ_RANGE: [[i32; 2]; {len(geo["range"])}] = [{points}];',
              f'pub const GZ_BURSTER_BOX: [i32; 4] = {box(geo["burster_box"])};',
              '/// Per sprite, the collider its tk2d definition writes into the body\'s',
              '/// BoxCollider2D, relative to the object, as authored (facing left), Q16;',
              '/// `[0, 0, 0, 0]` for a sprite that leaves the last one in place.',
              f'pub const GZ_SPRITE_BOX: [[i32; 4]; {len(sprite_boxes)}] = [']
    lines += ['    ' + (box(b) if b is not None else '[0, 0, 0, 0]') + ',' for b in sprite_boxes]
    lines += ['];']
    return '\n'.join(lines) + '\n'


def gruz_cook_scene_bank(s, sc, actor, rows, atlas, frames, clips):
    """Check the contract and append the whole bank to the scene's actor atlas,
    as `cook_scene_bank` does for the False Knight. Returns the report entry and
    a function that writes data/gruz_art.rs once the bank's clip base is known."""
    del actor
    src = gruz_sources(s, sc)
    digests = gruz_check_contract(s, sc, src)
    gruz_check_rust()
    geo = gruz_geometry(s, sc, src)
    sprites, art_clips, boxes = gruz_source_art(s, sc, src)
    # append_bank numbers sprites in this order; the box table follows it.
    sprite_boxes = [boxes[k] for k in dict.fromkeys(k for name in GRUZ_CLIPS for k in art_clips[name]['keys'])]
    base = [(ROOT / 'data/regions' / f'region-{r["chunk_id"]:03}' / 'room.hk').read_bytes() for r in rows]
    scenery = scenery_rects(base)
    quantized, cut, decisions, totals = plan(sprites, art_clips, {}, scenery, atlas._quantize,
                                             priority=GRUZ_PRIORITY, page_limit=GRUZ_SCENE_PAGE_LIMIT,
                                             stream_limit=GRUZ_STREAM_BYTES_LIMIT, label='Gruz Mother')
    anchor, sprite_rows, clip_rows, sequence, _ = append_bank(
        atlas, frames, clips, sprites, art_clips, {}, quantized, cut, names=GRUZ_CLIPS, anchor_name='Gruz Mother parts')
    report = {'decisions': decisions, **totals, 'art_clips': list(GRUZ_CLIPS), 'fsm_sha256': digests,
              'geometry': geo, 'anchor_clip_in_bank': anchor,
              'code_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}

    def write(clip_base):
        text = gruz_rust_table(anchor + clip_base, sprite_rows, clip_rows, sequence, geo, rows[0]['scene_id'], sprite_boxes)
        path = ROOT / 'data/gruz_art.rs'
        if not path.is_file() or path.read_text() != text:
            path.write_text(text)
        report['anchor_clip'] = anchor + clip_base
        (ROOT / '.hkpsx/gruz-mother').mkdir(parents=True, exist_ok=True)
        (ROOT / '.hkpsx/gruz-mother/art.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    return report, write
