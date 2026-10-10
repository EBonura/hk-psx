"""Source `BG Control` arena gates, joined to the terrain edges the cook baked.

`host/breakables.py::battle_gate` reads the one decision a gate makes on load:
`Opened` tests `Start Closed`, and a gate that has it set turns its box back on
in `Quick Close` and then waits for the `BG OPEN` its arena sends when the fight
is won. `host/cook.py` bakes every gate's collider as terrain whatever that
verdict says, so the room pack holds an edge for all of them.

This is the join that lets the runtime move them: for every catalogue slot, the
edge indices in that slot's room pack that belong to a gate collider, keyed by
gate. `game/src/battle_gates.rs` excludes the edges of every gate that is
currently open, the way `game/src/great_door.rs` excludes the door's when it is
broken through, and it starts from the gates that are open on load, which is the
complement of `PLACEMENT_CLOSED`. A gate that loads open is therefore terrain
the runtime lifts on the first frame rather than terrain the cook withheld,
which is what lets `BG CLOSE` seal an arena at its ends.

Both directions of that join have to hold. A missing row is now an invisible
wall rather than a gate that cannot close, so `bind` derives the rows from the
cook's own `edge_sources` and never from a second copy of the rule, and `main`
reports any gate the cook baked no terrain for at all.

This reads `data/regions.json` rather than cooking anything, so it is not a
cooker input: changing it must not invalidate a single cooked region. Regenerate
with

    .venv/bin/python host/battle_gates.py

which rewrites `data/battle_gates.rs` and `.hkpsx/battle-gates.json`. The table
is only as current as `data/regions.json`: `host/cook.py` sits above the divider
in `host/cook_inputs.txt`, so a change to which colliders it bakes re-cooks all
693 regions and this has to run again afterwards.
"""
import json
import math
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

import rustsrc  # noqa: E402

# `game/src/battle_gates.rs` keeps the closed set in one u16, which is the whole
# reason the runtime costs a single test before it looks at anything.
MAX_GATES = 16


def edge_scratch_slots():
    """The shared exclusion scratch, read out of the guest rather than repeated.

    `world::State` keeps one bounded list for every scripted exclusion and
    `append_script_edges` asserts on overflow, which on a disc is a crash rather
    than a wrong room. Reading the number here is what turns that into a
    generator refusal, the way host/cook_scripts.py reads SCRIPT_FIELD_SLOTS.
    """
    try:
        return rustsrc.const_int(ROOT / 'game/src/world.rs', 'SCRIPT_EDGE_SLOTS')
    except KeyError:
        raise ValueError('cannot read SCRIPT_EDGE_SLOTS out of game/src/world.rs') from None


def _count(body):
    return len([value for value in body.split(',') if value.strip()])


def neighbour_edges():
    """Exclusions per catalogue slot that the other two controllers already own.

    The Lifeblood cocoons and the Great Door share this scratch, so the budget
    is the union in one slot rather than this table's own rows. Read off the
    generated files the guest actually links, not off a second copy of the rule
    that produced them. Missing files mean those tables have not been cooked
    yet, which is not this generator's problem to diagnose.
    """
    counts = {}
    life = ROOT / 'data/lifeblood.rs'
    if life.is_file():
        # Only the slots that see a cocoon carry a row, so the slot is written.
        for slot, body in re.findall(r'\((\d+),Binding\{off:&\[[^\]]*\],edges:&\[([^\]]*)\]\}\)',
                                     rustsrc.source(life)):
            counts[int(slot)] = counts.get(int(slot), 0) + _count(body)
    door = ROOT / 'data/great_door.rs'
    if door.is_file():
        # Only the slots that see the door carry a row, so the slot is written.
        for slot, body in re.findall(r'\((\d+),Binding\{frames:\[[^\]]*\],edges:&\[([^\]]*)\]\}\)',
                                     rustsrc.source(door)):
            counts[int(slot)] = counts.get(int(slot), 0) + _count(body)
    # The False Knight's broken floor lifts `Break Floor` through the same
    # scratch, and on a won arena it does so with every gate open.
    floor = ROOT / 'data/false_knight_art.rs'
    if floor.is_file():
        table = re.search(r'FK_BREAK_FLOOR_EDGES:\[\(u16,u16\);\d+\]=\[([^\]]*)\]', rustsrc.source(floor))
        for slot, _edge in re.findall(r'\((\d+),(\d+)\)', table.group(1) if table else ''):
            counts[int(slot)] = counts.get(int(slot), 0) + 1
    return counts


def survey(source, report):
    """Every admitted scene's `BG Control` gates, in a stable order.

    Ordered by (scene id, source object id) rather than by name: two gates in
    one room share the name `Battle Gate 2` up to a suffix, and the bit a gate
    owns has to survive a rename in the source.
    """
    from scene import Scene
    from breakables import battle_gates
    gates = []
    refused = []
    for meta in sorted(report['scenes'], key=lambda s: s['scene_id']):
        errors = []
        found = battle_gates(Scene(source, meta['file']), errors=errors)
        for error in errors:
            refused.append(dict(error, scene=meta['scene_name']))
        for record in sorted(found, key=lambda r: int(r['source'].split(':')[-1])):
            gates.append({'scene': meta['scene_id'], 'scene_name': meta['scene_name'],
                          'name': record['name'], 'source': record['source'],
                          'collider_source': record['collider_source'],
                          'solid_on_load': record['solid_on_load'],
                          'box': record['box'], 'position': list(record['position'])})
    if refused:
        raise ValueError(f'unreadable BG Control gates: {refused}')
    if len(gates) > MAX_GATES:
        raise ValueError(f'{len(gates)} arena gates exceed the {MAX_GATES}-bit closed set')
    return gates


def bind(gates, report):
    """(catalogue slot, gate, edges) for every gate the cook baked as terrain.

    `edge_sources` is the cook's own join key: it names, per cooked edge, the
    source collider the edge came out of. Reading the answer off the cook's
    output rather than re-deriving it is the whole point: a row that disagrees
    with the pack is an invisible wall now that the open gates are baked too.
    A gate with no row at all means no edge of it survived any region's
    collision-bounds cull, which is the honest answer rather than an empty one.
    """
    owner = {gate['collider_source']: index for index, gate in enumerate(gates)}
    if len(owner) != len(gates):
        raise ValueError('two arena gates share one collider')
    budget = edge_scratch_slots()
    neighbours = neighbour_edges()
    rows = []
    for slot, region in enumerate(report['regions']):
        found = {}
        for edge, source in enumerate(region['edge_sources']):
            index = owner.get(source)
            if index is None:
                continue
            if gates[index]['scene'] != region['scene_id']:
                raise ValueError(f'gate {gates[index]["name"]} bound outside its own scene')
            found.setdefault(index, []).append(edge)
        spent = sum(len(edges) for edges in found.values()) + neighbours.get(slot, 0)
        if spent > budget:
            raise ValueError(f'catalogue slot {slot} ({region["scene_name"]}) needs {spent} '
                             f'scripted edge exclusions against {budget} in game/src/world.rs')
        for index in sorted(found):
            if slot >= 65536:
                raise ValueError('arena gate binding slot exceeds u16')
            rows.append((slot, index, found[index]))
    return rows


def generate(gates, rows, report=None):
    cooked = sorted({index for _, index, _ in rows})
    scene_gates = {}
    for index, gate in enumerate(gates):
        scene_gates.setdefault(gate['scene'], 0)
        scene_gates[gate['scene']] |= 1 << index
    def mask(bits):
        return '0b' + format(bits, '0%db' % max(len(gates), 1))
    lines = [
        '// Generated from the source `BG Control` gates and the cooked region edge',
        '// sources. host/battle_gates.py is the generator.',
        '// Gate order is (scene id, source object id); a gate owns its bit for the',
        '// life of this table, so a rename in the source cannot move it.',
        f'pub const GATES:usize={len(gates)};',
        '/// The collider state `Opened` leaves each gate in on load: set means the',
        '/// gate carries `Start Closed`, turned its box back on in `Quick Close`, and',
        '/// is waiting for a `BG OPEN` its arena has not sent yet.',
        f'pub const PLACEMENT_CLOSED:u16={mask(sum(1 << i for i, g in enumerate(gates) if g["solid_on_load"]))};',
        '/// The gates whose collider reached a room pack as terrain. `host/cook.py`',
        '/// bakes every gate, so this is normally all of them; a gate outside every',
        '/// region\'s collision bounds would be the one exception, and closing it',
        '/// would move nothing because there is no edge of it anywhere to restore.',
        f'pub const COOKED:u16={mask(sum(1 << i for i in cooked))};',
        '/// Guest scene id and the gates a `Battle Scene` there broadcasts to. The',
        "/// source's BG CLOSE/BG OPEN reach every gate in the room, not only the two",
        '/// at the arena ends, so the mask is per scene rather than per arena.',
        'pub static SCENE_GATES:&[(u8,u16)]=&['
        + ''.join(f'({scene},{mask(bits)}),' for scene, bits in sorted(scene_gates.items())) + '];',
        '/// (catalogue slot, gate, that gate\'s terrain edges in the slot\'s room',
        '/// pack), sorted by slot then gate so the runtime can binary search it.',
        'pub static REGIONS:&[(u16,u8,&[u16])]=&['
        + ''.join('({},{},&[{}]),'.format(slot, index, ','.join(map(str, edges)))
                  for slot, index, edges in rows) + '];',
    ]
    lines += generate_art(gates, report)
    return '\n'.join(lines) + '\n'


def generate_art(gates, report):
    """The art half of the table: gate positions, clips and each view's frames."""
    art = [(slot, row['battle_gate_art']) for slot, row in enumerate((report or {}).get('regions', []))
           if row.get('battle_gate_art')]
    names = list(GATE_CLIPS) + [SLAM_EFFECT_CLIP]
    clips = art[0][1]['clips'] if art else [{'name': name, 'fps': 0, 'frames': []} for name in names]
    if [c['name'] for c in clips] != names:
        raise ValueError('gate art clips are not in the runtime order')
    first = art[0][1] if art else {'rects': [], 'boxes': [], 'sheets': 0, 'effect_offset': [0, 0]}
    if any(a['clips'] != clips or a['sprites'] != first['sprites'] or a['rects'] != first['rects']
           or a['sheets'] != first['sheets'] for _, a in art):
        raise ValueError('views disagree about the gate frames')
    if any(c['fps'] != int(c['fps']) or not 0 <= c['fps'] < 256 for c in clips):
        raise ValueError('gate clip rate is not a whole number of frames per second')
    if any(slot >= 65536 or a['frame_base'] + a['sheets'] >= 65536 for slot, a in art):
        raise ValueError('gate art binding exceeds u16')
    q16 = lambda v: round(v * 65536)
    return [
        '/// Each gate\'s transform origin on the gameplay plane (q16 world units).',
        '/// host/battle_gates.py refuses a gate that is rotated, scaled or tinted.',
        'pub static POSITION:[[i32;2];GATES]=['
        + ''.join(f'[{q16(g["position"][0])},{q16(g["position"][1])}],' for g in gates) + '];',
        '/// `BG Opened`, `BG Close 1`, `BG Close 2`, `BG Closed`, `BG Open` and the',
        '/// slam effect\'s `BG Effect`: each clip\'s sprites (indices into SPRITE_RECT),',
        '/// and its rate in whole frames per second.',
        f'pub const CLIP_FRAMES:[&[u8];{len(clips)}]=['
        + ''.join('&[{}],'.format(','.join(map(str, c['frames']))) for c in clips) + '];',
        f'pub const CLIP_FPS:[u8;{len(clips)}]=[' + ''.join(f'{int(c["fps"])},' for c in clips) + '];',
        '/// Each sprite\'s sheet and texel rectangle [sheet, x, y, w, h], and its box',
        '/// around its origin [x0, y0, x1, y1] (q16 units, the effect\'s already scaled).',
        'pub static SPRITE_RECT:&[[u8;5]]=&[' + ''.join('[{}],'.format(','.join(map(str, r))) for r in first['rects']) + '];',
        'pub static SPRITE_BOX:&[[i32;4]]=&[' + ''.join('[{}],'.format(','.join(str(q16(v)) for v in b)) for b in first['boxes']) + '];',
        '/// The sheets one view carries (consecutive frames from its ART entry).',
        f'pub const SHEETS:usize={first["sheets"]};',
        '/// The `Close Effect` child\'s offset from its gate (q16 units).',
        f'pub const EFFECT_OFFSET:[i32;2]=[{q16(first["effect_offset"][0])},{q16(first["effect_offset"][1])}];',
        '/// (catalogue slot, the view\'s first gate sheet frame): every view of a',
        '/// scene with gates carries the sheets in its actor bank, sorted by slot.',
        'pub static ART:&[(u16,u16)]=&[' + ''.join(f'({slot},{a["frame_base"]}),' for slot, a in art) + '];',
    ]

# ------------------------------------------------------------------ gate art
#
# A gate is not only a collider. `BG Control` plays a tk2d clip in every state
# it enters, and the sprite is on screen the whole time: the open pose is the
# gate's tip showing under its lintel (`gate_resize0000`, 0.48 units of the
# 4.2), the closed pose is the whole gate. Without the art an open gate was
# simply absent and a closed one was an invisible wall, which is how the False
# Knight's start-closed left gate went missing from the arena (MISSING.md).
#
# The art rides the scene actor bank (host/regions.py postpack_actor_bank), as
# the Great Door's does: every view of a gate's scene carries it, so it streams
# with the room and is there on the first frame of the fade-in.

# The clip each FSM state plays, in the order game/src/battle_gates.rs indexes
# them (`CLIP_OPENED` ...). `Opened` and `Quick Open` play `BG Opened`;
# `Quick Close` and `Double Close` play `BG Closed`; `Close 1` plays `BG Close 1`
# and, when that clip finishes, `Close 2` plays `BG Close 2`; `Open` plays
# `BG Open`.
GATE_CLIPS = ('BG Opened', 'BG Close 1', 'BG Close 2', 'BG Closed', 'BG Open')
# What the runtime assumes of them: one held frame each for the two poses, and
# play-once clips for the three transitions (tk2d wrap modes 6 Single and 2
# Once), so a clip's last frame is the pose it leaves the gate in.
GATE_CLIP_WRAPS = {'BG Opened': 6, 'BG Close 1': 2, 'BG Close 2': 2, 'BG Closed': 2, 'BG Open': 2}


def _gate_parts(source, sc, gid):
    from breakables import _components
    found = {t: c for _, t, c in _components(sc, gid)}
    sprite, animator, renderer = found.get('tk2dSprite'), found.get('tk2dSpriteAnimator'), found.get('MeshRenderer')
    if sprite is None or animator is None or renderer is None:
        raise ValueError('arena gate without its tk2dSprite, animator and renderer')
    if not renderer['m_Enabled']:
        raise ValueError('arena gate renderer is disabled')
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[i][j] - (1 if i == j else 0)) > 1e-6 for i in range(2) for j in range(2)) \
            or any(abs(sprite['_scale'][k] - 1) > 1e-6 for k in 'xy') \
            or any(abs(sprite['_color'][k] - 1) > 1e-6 for k in 'rgba'):
        # Every gate in the shipped scenes is upright, unscaled and untinted;
        # the runtime draws exactly that, so anything else is refused here.
        raise ValueError('arena gate is rotated, scaled or tinted')
    collection_o = source.ref(sc.file, sprite['collection'])
    library_o = source.ref(sc.file, animator['library'])
    return collection_o, library_o


def append_art(source, sc, atlas, frames):
    """Append the gate clips' frames to a scene actor bank, or None without gates.

    Every gate in the catalogue shares one collection and one animation library
    (refused otherwise), so a scene appends the frames once, whatever its gate
    count, and the gates differ only in position and state.
    """
    from breakables import battle_gates
    from cook import tk_sprite, FOCAL, CAM_Z
    gates = battle_gates(sc)
    if not gates:
        return None
    parts = [_gate_parts(source, sc, g['gid']) for g in gates]
    if len({(source.sid(c), source.sid(l)) for c, l in parts}) != 1:
        raise ValueError('arena gates in one scene use different art')
    collection_o, library_o = parts[0]
    collection, library = source.read(collection_o), source.read(library_o)
    clips = {c['name']: c for c in library['clips']}
    order, table = [], []
    for name in GATE_CLIPS:
        clip = clips.get(name)
        if clip is None:
            raise ValueError(f'arena gate library has no {name!r}')
        if clip['wrapMode'] != GATE_CLIP_WRAPS[name] or (clip['wrapMode'] == 6 and len(clip['frames']) != 1):
            raise ValueError(f'arena gate clip {name!r} is no longer a held pose or a play-once clip')
        ids = []
        for frame in clip['frames']:
            if source.ref(library_o.assets_file, frame['spriteCollection']).path_id != collection_o.path_id:
                raise ValueError(f'arena gate clip {name!r} draws from another collection')
            if frame['spriteId'] not in order:
                order.append(frame['spriteId'])
            ids.append(order.index(frame['spriteId']))
        table.append({'name': name, 'fps': clip['fps'], 'frames': ids})
    scale = FOCAL / -CAM_Z
    if any(abs(g['position'][2]) > 0.01 for g in gates):
        raise ValueError('arena gate off the gameplay plane')
    effect = slam_effect(source, sc, gates, library_o, collection_o, clips)
    # The flash's three frames are 30-31 by 36-38 texels at full density, so
    # they need two sheets; at 0.8 they share one. The False Knight's views have
    # room for two gate texture records and no more (host/pickups.py
    # LEVEL_FLOOR refused three), and the flash is on screen for a quarter of
    # a second, so it is cooked at 0.8 texels per pixel and drawn full size.
    sprites = [(index, 1.0, 1.0) for index in order] \
        + [(index, effect['scale'], SLAM_EFFECT_DENSITY) for index in effect['sprites']]
    sheets, rects, boxes = gate_sheets(source, collection_o, collection, sprites, scale)
    # A handful of streamed sheets rather than a frame per sprite: one texture
    # record per sheet per view, and one animation slot per sheet on screen.
    # The False Knight's views sit at 637 of the 640 records hk-format allows,
    # so a frame per sprite pushed its chest and City Crest out of the bank
    # (host/pickups.py LEVEL_FLOOR now refuses that).
    first = len(frames)
    for i, sheet in enumerate(sheets):
        texture = atlas.add(sheet, sheet.width, sheet.height, streamed=True)
        frames.append({'texture': texture, 'box': [0, 0, 1, 1], 'event': {},
                       'sprite': f'{source.sid(collection_o)}:gate-sheet-{i}'})
    effect_clip = {'name': 'BG Effect', 'fps': effect['fps'],
                   'frames': [len(order) + i for i in range(len(effect['sprites']))]}
    return {'scene': Path(sc.file.name).name, 'first': first, 'sheets': len(sheets),
            'sprites': order + effect['sprites'], 'clips': table + [effect_clip],
            'rects': rects, 'boxes': boxes, 'effect_offset': effect['offset'],
            'collection': source.sid(collection_o), 'library': source.sid(library_o)}


# `Close 2` restarts the gate's `Close Effect` child on frame 0 of its clip,
# `BG Effect` (4 frames at 12 fps, played once), and turns its renderer on; the
# clip's last sprite, `gate_effect0003`, is 0.02 units square, so the flash is
# the first three and nothing after them.
SLAM_EFFECT_CHILD = 'Close Effect'
SLAM_EFFECT_CLIP = 'BG Effect'
SLAM_EFFECT_EMPTY = 0.05
SLAM_EFFECT_DENSITY = 0.8


def slam_effect(source, sc, gates, library_o, collection_o, clips):
    """The slam flash every gate shares: its sprites, scale, rate and offset."""
    from breakables import _components
    found = set()
    for g in gates:
        transform = sc.transforms[sc.go_transform[g['gid']]]
        children = [sc.transforms[c['m_PathID']]['m_GameObject']['m_PathID'] for c in transform['m_Children']]
        child = [c for c in children if sc.gos[c]['m_Name'] == SLAM_EFFECT_CHILD]
        if len(child) != 1:
            raise ValueError(f'arena gate {g["name"]} has {len(child)} {SLAM_EFFECT_CHILD!r} children')
        local = sc.transforms[sc.go_transform[child[0]]]
        comps = {t: c for _, t, c in _components(sc, child[0])}
        if source.sid(source.ref(sc.file, comps['tk2dSpriteAnimator']['library'])) != source.sid(library_o):
            raise ValueError('slam effect animates from another library')
        if any(abs(comps['tk2dSprite']['_scale'][k] - 1) > 1e-6 for k in 'xy') \
                or abs(local['m_LocalScale']['x'] - local['m_LocalScale']['y']) > 1e-6 \
                or abs(local['m_LocalRotation']['z']) > 1e-6:
            raise ValueError('slam effect is scaled unevenly or rotated')
        found.add((round(local['m_LocalPosition']['x'], 4), round(local['m_LocalPosition']['y'], 4),
                   round(local['m_LocalScale']['x'], 4)))
    if len(found) != 1:
        raise ValueError(f'arena gates place their slam effect differently: {found}')
    x, y, k = found.pop()
    clip = clips[SLAM_EFFECT_CLIP]
    if clip['wrapMode'] != 2:
        raise ValueError('slam effect clip is no longer play-once')
    collection = source.read(collection_o)
    sprites = []
    for frame in clip['frames']:
        d = collection['spriteDefinitions'][frame['spriteId']]
        xs = [v['x'] for v in d['positions']]
        if (max(xs) - min(xs)) * k < SLAM_EFFECT_EMPTY:
            break
        sprites.append(frame['spriteId'])
    return {'sprites': sprites, 'scale': k, 'offset': [x, y], 'fps': clip['fps']}


# Transparent texels between packed sprites: twice the quantizer's edge reach,
# so the texels either sprite's edge classification looks at are all its own
# side of the gap. (Two effect sprites 30 wide then share one 64-texel sheet.)
def _sheet_gap():
    from quantize import EDGE_REACH
    return EDGE_REACH * 2


def gate_sheets(source, collection_o, collection, sprites, scale, slot=64):
    """Sprites at gameplay-plane scale, packed into as few slot-sized sheets as fit.

    `sprites` is [(sprite id, world scale, texel density)]: the box is scaled by
    the first, the texels by both. Columns, tallest first; a column
    takes further sprites below while they fit, a sheet takes further columns
    while they fit, and a new sheet starts when neither does. Returns the sheets
    and, per sprite, [sheet, x, y, w, h] and its world box (scaled).
    """
    from PIL import Image
    from cook import tk_sprite
    textures, sized = {}, []
    for index, k, density in sprites:
        image, box = tk_sprite(source, collection_o.assets_file, collection, index, textures)
        box = [v * k for v in box]
        w = max(1, math.ceil((box[2] - box[0]) * scale * density))
        h = max(1, math.ceil((box[3] - box[1]) * scale * density))
        if w > slot or h > slot:
            raise ValueError(f'arena gate sprite {index} is {w}x{h}, past one {slot}x{slot} slot')
        sized.append((image.convert('RGBA').resize((w, h), Image.Resampling.LANCZOS), box))
    gap = _sheet_gap()
    order = sorted(range(len(sized)), key=lambda i: (-sized[i][0].height, i))

    def columns():
        """Columns of tall sprites, each taking more below while they fit."""
        sheets, placed = [], {}
        for i in order:
            im = sized[i][0]
            spot = None
            for n, cols in enumerate(sheets):
                col = next((c for c in cols if c[2] + im.height <= slot and im.width <= c[1]), None)
                if col is None:
                    x = cols[-1][0] + cols[-1][1] + gap
                    if x + im.width <= slot:
                        col = [x, im.width, 0]
                        cols.append(col)
                if col is not None:
                    spot = (n, col)
                    break
            if spot is None:
                sheets.append([[0, im.width, 0]])
                spot = (len(sheets) - 1, sheets[-1][0])
            n, col = spot
            placed[i] = (n, col[0], col[2])
            col[2] += im.height + gap
        return len(sheets), placed

    def shelves():
        """Rows of small sprites, left to right, a new row when one is full."""
        sheets, placed = [], {}      # per sheet: [y of the current row, its height, next x]
        for i in order:
            im = sized[i][0]
            spot = None
            for n, row in enumerate(sheets):
                if row[2] + im.width <= slot and im.height <= row[1]:
                    spot = n
                    break
                y = row[0] + row[1] + gap
                if y + im.height <= slot:
                    row[:] = [y, im.height, 0]
                    spot = n
                    break
            if spot is None:
                sheets.append([0, im.height, 0])
                spot = len(sheets) - 1
            row = sheets[spot]
            placed[i] = (spot, row[2], row[0])
            row[2] += im.width + gap
        return len(sheets), placed

    count, placed = min(columns(), shelves(), key=lambda r: r[0])
    sheets = [None] * count
    images = []
    for n in range(len(sheets)):
        mine = [i for i in placed if placed[i][0] == n]
        width = max(placed[i][1] + sized[i][0].width for i in mine)
        height = max(placed[i][2] + sized[i][0].height for i in mine)
        sheet = Image.new('RGBA', (width, height), (0, 0, 0, 0))
        for i in mine:
            sheet.paste(sized[i][0], placed[i][1:])
        images.append(sheet)
    rects = [[placed[i][0], placed[i][1], placed[i][2], im.width, im.height] for i, (im, _) in enumerate(sized)]
    return images, rects, [box for _, box in sized]


def bind_art(record, row, frame_base):
    """A view's own frame index of the first gate frame, for the generated table."""
    if record is None:
        return None
    return {'frame_base': frame_base + record['first'], 'sprites': record['sprites'],
            'clips': record['clips'], 'rects': record['rects'], 'boxes': record['boxes'],
            'sheets': record['sheets'], 'effect_offset': record['effect_offset'],
            'collection': record['collection'], 'library': record['library']}


def main():
    from source import Source
    report = json.loads((ROOT / 'data/regions.json').read_text())
    gates = survey(Source(), report)
    rows = bind(gates, report)
    (ROOT / 'data/battle_gates.rs').write_text(generate(gates, rows, report))
    # A gate with no row carries no terrain anywhere, so its arena cannot seal
    # at that end. That is not a refusal, because it is a property of the source
    # placement against the region bounds rather than of this join, but it is
    # the one thing a reader of this table would otherwise have to derive.
    bound = {index for _, index, _ in rows}
    limitations = [
        f'{gates[index]["scene_name"]} {gates[index]["name"]} has no cooked terrain in any'
        ' region, so BG CLOSE cannot seal that end of its arena.'
        for index in range(len(gates)) if index not in bound
    ]
    limitations.append(
        'The gate sprite and its five clips are drawn; the slam effect, dust and camera '
        'shake of Close 2 and the raise dust of Open are not reproduced.')
    record = {
        'gates': gates,
        'bindings': [{'slot': slot, 'gate': index, 'name': gates[index]['name'],
                      'scene_name': gates[index]['scene_name'], 'edges': edges}
                     for slot, index, edges in rows],
        'limitations': limitations,
    }
    (ROOT / '.hkpsx/battle-gates.json').write_text(json.dumps(record, indent=2) + '\n')
    cooked = {row['name'] for row in record['bindings']}
    print(f'{len(gates)} arena gates, {len(cooked)} cooked as terrain, '
          f'{len(rows)} region bindings: {sorted(cooked)}', flush=True)
    for line in limitations:
        print(f'  note: {line}', flush=True)


if __name__ == '__main__':
    main()
