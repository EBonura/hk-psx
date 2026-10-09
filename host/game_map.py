"""The quick map: Cornifer's rough rooms, the mapped rooms and their pins.

The source's map is one prefab in resources.assets, `Game_Map`, carrying a
`GameMap` component and one child per map area (`Crossroads`, `Town_Tutorial`,
...). Every room is a child of its area named after its scene, drawn as one
SpriteRenderer at 100 pixels per unit. Two rules decide what a room shows:

* `GameMap.SetupMap` switches a room on when `scenesMapped` holds its name (or
  `mapAllRooms`) and `hasQuill` is set. A room that is serialized active is on
  from the start: those are the rooms Cornifer charted.
* `RoughMapRoom.OnEnable` swaps a charted room's rough `_Cornifer` sprite for
  its `fullSprite` once `scenesMapped` holds it.

An area is drawn once its map bool is set (`mapDirtmouth` is a new-save
default; `mapCrossroads` is what Cornifer sells), and the quick map shows only
the area the Knight stands in (`Quick Map` FSM, `Check Area`). `scenesMapped`
grows from `scenesVisited` in `PlayerData.UpdateGameMap`, only with the quill
and only for scenes whose area map is owned; Cornifer's purchase and a bench
rest call it. A `pin_bench` child is drawn while `hasPinBench` is set, and the
Knight marker (`Compass Icon`) while charm 2, Wayward Compass, is worn;
`GameMap.PositionCompass` places it inside the room sprite at the hero's
fraction of the scene's tilemap.

This cook reads that prefab for the areas the slice reaches, scales every room
sprite to the quick map's size, quantizes them to one 4bpp palette and packs
them into the one 256x256 texture page `hk_cache::residency::MAP_PAGE`
reserves for the map, with the marker and the bench pin under a second
palette, and the Snail Shaman's frames and the spell orb (host/shaman.py
`art`) under a third and fourth: the page's last row holds the four palettes.
The largest scale in SCALES at which all of it fits is used. It writes data/game_map.rs, the streamed blob data/game-map.bin with
its manifest data/game-map.rs, and a report with
the packing and every refusal to .hkpsx/game-map.json.

Not cooked: the `_b` room variants (no scene carries their name, so SetupMap
never switches them on), the Next Area arrows and labels, the grub, shop,
stag, tram and cocoon pins, the world map pane and its panning, and the map
animations on the Knight (`Map Open`, `Map Idle`, `Map Walk`).
"""
import hashlib, json, struct
from PIL import Image
from source import Source, ROOT, dump
from cook import native_sprite, tk_sprite
import language

GAME_MAP = 'Game_Map'
# Area objects under Game_Map, in guest order, with the PlayerData bool that
# owns each (`GameMap.QuickMap*`, `Quick Map` FSM) and its `Map Zones` key.
AREAS = [
    ('Town_Tutorial', 'mapDirtmouth', 'TOWN'),
    ('Crossroads', 'mapCrossroads', 'CROSSROADS'),
]
# Pixels per map unit on the PS1 screen (x100). The source quick map shows the
# Game Map at scale 1.55 under a HUD camera 16.2 units tall; 240 lines over
# that is 23 pixels per unit. Larger keeps the one-pixel line art legible; the
# cook takes the first of SCALES at which the page holds everything.
SCALE = 0.28
# Tried in order until the map and the Mound's art fit the page's 255 texel rows.
SCALES = (0.28, 0.27, 0.26, 0.25, 0.24, 0.23, 0.22, 0.21, 0.20)
PAGE = 256
# The map page, in VRAM halfwords, kept equal to shared/hk-cache/src/residency.rs
# MAP_PAGE by tests/test_game_map.py. Its last row holds the palettes, four
# 16-entry CLUTs across the page's 64 halfwords, so texels get rows 0..254.
PAGE_XY = (896, 256)
CLUT_ROW = PAGE - 1
CLUT_XY = [(PAGE_XY[0] + 16 * i, PAGE_XY[1] + CLUT_ROW) for i in range(4)]
OUTPUT_RS = ROOT / 'data/game_map.rs'
OUTPUT_BIN = ROOT / 'data/game-map.bin'
MANIFEST = ROOT / 'data/game-map.rs'
SHAMAN_RS = ROOT / 'data/shaman_art.rs'
REPORT = ROOT / '.hkpsx/game-map.json'


def _tree(source, file, gid):
    go = file.objects[gid].read_typetree()
    comps = [source.ref(file, c['component']) for c in go['m_Component']]
    transform = next(c.read_typetree() for c in comps if c.type.name in ('Transform', 'RectTransform'))
    return go, comps, transform


def _children(source, file, transform):
    for ref in transform['m_Children']:
        yield source.ref(file, ref).read_typetree()['m_GameObject']['m_PathID']


def _game_map(source):
    file = source.file('resources.assets')
    found = [pid for pid, o in file.objects.items()
             if o.type.name == 'GameObject' and o.peek_name() == GAME_MAP]
    if len(found) != 1:
        raise ValueError(f'resources.assets holds {len(found)} {GAME_MAP} objects')
    return file, found[0]


def _sprite(source, file, comps):
    renderers = [c for c in comps if c.type.name == 'SpriteRenderer']
    if not renderers:
        return None, None
    tree = renderers[0].read_typetree()
    return source.ref(file, tree['m_Sprite']), tree


def _scaled(image, scale):
    """Downscale on the alpha-weighted average, then a hard edge at half."""
    w = max(1, round(image.width * scale))
    h = max(1, round(image.height * scale))
    premultiplied = image.convert('RGBa').resize((w, h), Image.Resampling.BOX).convert('RGBA')
    premultiplied.putalpha(premultiplied.getchannel('A').point(lambda a: 255 if a >= 96 else 0))
    return premultiplied


def collect(source):
    """Every room, pin and the marker of the areas the slice reaches."""
    file, root = _game_map(source)
    _go, _comps, root_t = _tree(source, file, root)
    by_name = {}
    for gid in _children(source, file, root_t):
        by_name[file.objects[gid].peek_name()] = gid
    areas, rooms, pins = [], [], []
    for area_index, (area_name, owner, zone_key) in enumerate(AREAS):
        gid = by_name[area_name]
        _ago, _ac, at = _tree(source, file, gid)
        origin = at['m_LocalPosition']
        area_rooms = []
        for rid in _children(source, file, at):
            go, comps, tr = _tree(source, file, rid)
            sprite_o, renderer = _sprite(source, file, comps)
            if sprite_o is None:
                continue
            full_o = None
            for c in comps:
                if c.type.name == 'MonoBehaviour' and source.typename(c) == 'RoughMapRoom':
                    full_o = source.ref(file, source.read(c)['fullSprite'])
            lp, ls = tr['m_LocalPosition'], tr['m_LocalScale']
            room = {'name': go['m_Name'], 'area': area_index, 'source': source.sid(file.objects[rid]),
                    'charted': bool(go['m_IsActive']), 'center': [origin['x'] + lp['x'], origin['y'] + lp['y']],
                    'scale': [ls['x'], ls['y']],
                    'rough': source.sid(sprite_o) if full_o is not None else None,
                    'full': source.sid(full_o if full_o is not None else sprite_o),
                    'rough_o': sprite_o if full_o is not None else None,
                    'full_o': full_o if full_o is not None else sprite_o}
            if room['charted'] and full_o is None:
                raise ValueError(f'{room["name"]} is charted but carries no RoughMapRoom')
            for kid in _children(source, file, tr):
                kgo, kcomps, ktr = _tree(source, file, kid)
                if kgo['m_Name'] != 'pin_bench':
                    continue
                pin_o, _ = _sprite(source, file, kcomps)
                kp, ks = ktr['m_LocalPosition'], ktr['m_LocalScale']
                pins.append({'room': room['name'], 'kind': 'bench', 'sprite_o': pin_o,
                             'center': [room['center'][0] + kp['x'] * ls['x'], room['center'][1] + kp['y'] * ls['y']],
                             'scale': ks['x'] * ls['x']})
            area_rooms.append(room)
        areas.append({'name': area_name, 'owner': owner, 'zone_key': zone_key, 'rooms': area_rooms})
        rooms.extend(area_rooms)
    marker = by_name['Compass Icon']
    _mgo, mcomps, mtr = _tree(source, file, marker)
    tk = next(c for c in mcomps if c.type.name == 'MonoBehaviour' and source.typename(c) == 'tk2dSprite')
    tkt = source.read(tk)
    collection_o = source.ref(file, tkt['collection'])
    marker_image, marker_box = tk_sprite(source, collection_o.assets_file, source.read(collection_o), tkt['_spriteId'], {})
    return areas, rooms, pins, {'image': marker_image, 'box': marker_box,
                                'scale': mtr['m_LocalScale']['x'], 'source': source.sid(file.objects[marker])}


def _hard(image):
    """A hard alpha edge at half, as the room art gets from `_scaled`."""
    image = image.convert('RGBA')
    image.putalpha(image.getchannel('A').point(lambda a: 255 if a >= 128 else 0))
    return image


def _pack(images):
    """Skyline-pack (w, h) images into one 256-wide page, lowest spot first;
    returns ((x, y) each, rows used)."""
    order = sorted(range(len(images)), key=lambda i: (-images[i].height, -images[i].width))
    placed = [None] * len(images)
    sky = [0] * PAGE
    for i in order:
        w, h = images[i].size
        if w > PAGE or h > PAGE:
            raise ValueError(f'map image {w}x{h} exceeds the {PAGE}px page')
        best = None
        for x in range(PAGE - w + 1):
            y = max(sky[x:x + w])
            if best is None or y < best[1]:
                best = (x, y)
        x, y = best
        placed[i] = (x, y)
        for k in range(x, x + w):
            sky[k] = y + h
    rows = max(sky)
    if rows > CLUT_ROW:
        raise ValueError(f'the map needs {rows} rows of the {CLUT_ROW} its page has for texels')
    return placed, rows

def _quantize(images, colours):
    """One shared 4bpp palette over a set of images; index 0 is transparent."""
    opaque = [p[:3] for im in images for p in (im.get_flattened_data() if hasattr(im, 'get_flattened_data') else im.getdata()) if p[3]]
    strip = Image.new('RGB', (len(opaque), 1))
    strip.putdata(opaque)
    q = strip.quantize(colors=colours, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    flat = q.getpalette()[:colours * 3]
    # A set with fewer distinct colours gets a shorter palette.
    colours = min(colours, len(flat) // 3, max(q.tobytes()) + 1)
    flat = flat[:colours * 3]
    palette = [0]
    for k in range(colours):
        r, g, b = flat[k * 3:k * 3 + 3]
        word = (r >> 3) | ((g >> 3) << 5) | ((b >> 3) << 10)
        palette.append(word or 0x8000)
    def index(rgb):
        best = min(range(colours), key=lambda k: sum((a - c) ** 2 for a, c in zip(rgb, flat[k * 3:k * 3 + 3])))
        return best + 1
    cache = {}
    out = []
    for im in images:
        idx = []
        for p in (im.get_flattened_data() if hasattr(im, 'get_flattened_data') else im.getdata()):
            if not p[3]:
                idx.append(0)
                continue
            key = p[:3]
            if key not in cache:
                cache[key] = index(key)
            idx.append(cache[key])
        out.append(idx)
    return palette + [0] * (16 - len(palette)), out


def cook():
    source = Source()
    regions = json.loads((ROOT / 'data/regions.json').read_text())
    scenes = {s['scene_name']: s for s in regions['scenes']}
    areas, rooms, pins, marker = collect(source)
    refused = []
    kept = []
    for room in rooms:
        # SetupMap switches a room on by its name; a room no admitted scene is
        # named after can only ever show if it is charted.
        if room['name'] not in scenes and not room['charted']:
            refused.append({'room': room['name'], 'source': room['source'],
                            'reason': 'no admitted scene carries this name and Cornifer did not chart it'})
            continue
        kept.append(room)
    # The Snail Shaman's frames and the spell orb ride the same page: they are
    # wanted only in the Mound, and a page the map already owns costs them no
    # CLUT rows, no animation slots and no RAM (host/shaman.py).
    import shaman, blocker_roller
    s_images, s_boxes, s_clips, orb_image, orb_box, s_sources = shaman.art(source)
    # And the Elder Baldur's spat Roller, which shares the Shaman's palette.
    r_images, r_boxes, r_clips, r_sources = blocker_roller.art(source)
    bench_pins = [p for p in pins if p['room'] in {r['name'] for r in kept}]
    global SCALE
    for SCALE in SCALES:
        # Images: rough then full per room, then the marker and the bench pin,
        # then the Shaman's frames and the orb.
        images = []
        for room in kept:
            if room['rough_o'] is not None:
                image, _ = native_sprite(room['rough_o'])
                room['rough_image'] = len(images)
                images.append(_scaled(image, SCALE * room['scale'][0]))
            else:
                room['rough_image'] = None
            image, _ = native_sprite(room['full_o'])
            room['full_image'] = len(images)
            images.append(_scaled(image, SCALE * room['scale'][0]))
        room_count = len(images)
        marker_index = len(images)
        images.append(_scaled(marker['image'], SCALE * marker['scale']))
        pin_index = None
        if bench_pins:
            pin_image, _ = native_sprite(bench_pins[0]['sprite_o'])
            pin_index = len(images)
            images.append(_scaled(pin_image, SCALE * bench_pins[0]['scale']))
        icon_end = len(images)
        shaman_base = len(images)
        images.extend(_hard(im) for im in s_images)
        roller_base = len(images)
        images.extend(_hard(im) for im in r_images)
        orb_index = len(images)
        images.append(_hard(orb_image))
        try:
            placed, rows = _pack(images)
            break
        except ValueError as error:
            last = error
    else:
        raise last
    room_palette, room_indices = _quantize(images[:room_count], 15)
    icon_palette, icon_indices = _quantize(images[room_count:icon_end], 15)
    # Shaman and Roller: both grey shells, one palette.
    shaman_palette, shaman_indices = _quantize(images[shaman_base:orb_index], 15)
    orb_palette, orb_indices = _quantize(images[orb_index:], 15)
    indices = room_indices + icon_indices + shaman_indices + orb_indices
    page = bytearray(PAGE // 2 * rows)
    for im, (x0, y0), idx in zip(images, placed, indices):
        for y in range(im.height):
            for x in range(im.width):
                v = idx[y * im.width + x]
                if v:
                    at = (y0 + y) * (PAGE // 2) + (x0 + x) // 2
                    page[at] |= v << (((x0 + x) & 1) * 4)
    # Page rows, then the palette row: rooms, icons, the Shaman, the orb.
    blob = bytes(page) + b''.join(struct.pack('<16H', *p) for p in (room_palette, icon_palette, shaman_palette, orb_palette))
    # The Shaman's frame table, for game/src/shaman.rs.
    q = lambda v: round(v * 65536)
    frame = lambda i, box, clut: 'MapFrame{u:%d,v:%d,w:%d,h:%d,clut:%d,bounds:[%s]}' % (
        placed[i][0], placed[i][1], images[i].width, images[i].height, clut, ','.join(str(q(v)) for v in box))
    SHAMAN_RS.write_text('// Generated by host/game_map.py: the Snail Shaman and the spell orb in the map page.\n'
        'pub const SHAMAN_CLIPS:&[Clip]=&[' + ','.join('Clip{start:%d,count:%d,fps:%d,wrap:%d}' % (
            c['start'], c['count'], c['fps'], c['wrap']) for c in s_clips) + '];\n'
        'pub const SHAMAN_FRAMES:&[MapFrame]=&[' + ','.join(frame(shaman_base + i, box, 2) for i, box in enumerate(s_boxes))
        + ',' + frame(orb_index, orb_box, 3) + '];\n'
        f'pub const ORB_FRAME:usize={len(s_boxes)};\n'
        'pub const ROLLER_CLIPS:&[Clip]=&[' + ','.join('Clip{start:%d,count:%d,fps:%d,wrap:%d}' % (
            c['start'], c['count'], c['fps'], c['wrap']) for c in r_clips) + '];\n'
        'pub const ROLLER_FRAMES:&[MapFrame]=&[' + ','.join(frame(roller_base + i, box, 2) for i, box in enumerate(r_boxes)) + '];\n')
    OUTPUT_BIN.write_bytes(blob)

    def tex(i):
        (x, y), im = placed[i], images[i]
        return f'MapTex{{u:{x},v:{y},w:{im.width},h:{im.height}}}'

    zones = language.sheet(source, 'Map Zones')
    prompts = language.sheet(source, 'Prompts')
    scene_room = {}
    lines = ['// Generated by host/game_map.py from the local Windows source; no retail payload is embedded.',
             f'pub const MAP_ROWS:u16={rows};',
             f'pub const MAP_PAGE_XY:(u16,u16)=({PAGE_XY[0]},{PAGE_XY[1]});',
             'pub const MAP_CLUT_XY:[(u16,u16);4]=[' + ','.join(f'({x},{y})' for x, y in CLUT_XY) + '];',
             'pub static MAP_AREAS:&[MapArea]=&[']
    area_extent = []
    first = 0
    for a_index, area in enumerate(areas):
        members = [r for r in kept if r['area'] == a_index]
        xs, ys = [], []
        for r in members:
            im = images[r['full_image']]
            cx, cy = r['center']
            xs += [cx * SCALE * 100 - im.width / 2, cx * SCALE * 100 + im.width / 2]
            ys += [cy * SCALE * 100 - im.height / 2, cy * SCALE * 100 + im.height / 2]
        box = [min(xs), max(ys), max(xs), min(ys)]  # left, top (map y up), right, bottom
        area_extent.append(box)
        name = zones[area['zone_key']]
        lines.append('MapArea{name:%s,first:%d,count:%d,w:%d,h:%d},' % (
            _quote(name), first, len(members), round(box[2] - box[0]), round(box[1] - box[3])))
        first += len(members)
    lines.append('];')
    lines.append('pub static MAP_ROOMS:&[MapRoom]=&[')
    ordered = [r for a in range(len(areas)) for r in kept if r['area'] == a]
    for i, r in enumerate(ordered):
        box = area_extent[r['area']]
        im = images[r['full_image']]
        # Top-left of the full sprite, in screen pixels from the area's top-left.
        x = r['center'][0] * SCALE * 100 - im.width / 2 - box[0]
        y = box[1] - (r['center'][1] * SCALE * 100 + im.height / 2)
        scene = scenes.get(r['name'])
        if scene:
            scene_room[scene['scene_id']] = i
        rough = 'None' if r['rough_image'] is None else f'Some({tex(r["rough_image"])})'
        # A rough sprite is not always the full one's size: centre it on the room.
        rdx = rdy = 0
        if r['rough_image'] is not None:
            rim = images[r['rough_image']]
            rdx = round((im.width - rim.width) / 2)
            rdy = round((im.height - rim.height) / 2)
        lines.append('MapRoom{scene:%d,area:%d,charted:%s,x:%d,y:%d,rough:%s,rough_dx:%d,rough_dy:%d,full:%s},'
                     % (scene['scene_id'] if scene else 255, r['area'], 'true' if r['charted'] else 'false',
                        round(x), round(y), rough, rdx, rdy, tex(r['full_image'])))
    lines.append('];')
    lines.append('pub static MAP_PINS:&[MapPin]=&[')
    for p in bench_pins:
        room = next(i for i, r in enumerate(ordered) if r['name'] == p['room'])
        box = area_extent[ordered[room]['area']]
        pim = images[pin_index]
        x = p['center'][0] * SCALE * 100 - pim.width / 2 - box[0]
        y = box[1] - (p['center'][1] * SCALE * 100 + pim.height / 2)
        lines.append('MapPin{room:%d,x:%d,y:%d},' % (room, round(x), round(y)))
    lines.append('];')
    lines.append(f'pub const NO_MAP:&str={_quote(prompts["NO_MAP"])};')
    lines.append(f'pub const MAP_MARKER:MapTex={tex(marker_index)};')
    lines.append(f'pub const MAP_BENCH_PIN:MapTex={tex(pin_index)};')
    # Scene id -> (area, room). A scene with no room of its own (a door room such
    # as Room_shop) takes the area and room of the scene its only gate leads to,
    # and the marker stands at that door: GameMap.PositionCompass's inRoom case.
    count = max(s['scene_id'] for s in regions['scenes']) + 1
    by_id = {s['scene_id']: s for s in regions['scenes']}
    scene_rows, doors = [], []
    for sid in range(count):
        s = by_id.get(sid)
        room = scene_room.get(sid)
        door = None
        if room is None and s is not None:
            targets = {g['target_scene'] for g in s['gates'] if g.get('enabled', True)}
            if len(targets) == 1:
                target = scenes.get(next(iter(targets)))
                if target and target['scene_id'] in scene_room:
                    back = [g for g in target['gates'] if g['target_scene'] == s['scene_name']]
                    if len(back) == 1:
                        room = scene_room[target['scene_id']]
                        door = (target, back[0]['position'])
        if room is None:
            scene_rows.append('SceneMap{room:255,bounds:[0,0,1,1],door:None}')
            continue
        home = door[0] if door else s
        b = home['camera_global_bounds']
        door_text = 'None' if door is None else 'Some([%d,%d])' % (round(door[1][0] * 65536), round(door[1][1] * 65536))
        scene_rows.append('SceneMap{room:%d,bounds:[%d,%d,%d,%d],door:%s}' % (
            room, round(b[0] * 65536), round(b[1] * 65536), round(b[2] * 65536), round(b[3] * 65536), door_text))
        if door:
            doors.append({'scene': s['scene_name'], 'via': door[0]['scene_name']})
    lines.append('pub static SCENE_MAP:[SceneMap;%d]=[%s];' % (count, ','.join(scene_rows)))
    OUTPUT_RS.write_text('\n'.join(lines) + '\n')
    # The blob streams from WORLD.PAK before the title like the world one-shot
    # bank (host/build_guest.py AUDIO_BANKS), so it costs no linked RAM; the
    # guest checks the chunk against this pair.
    def fnv(data):
        value=0x811c9dc5
        for byte in data:value=((value^byte)*0x01000193)&0xffffffff
        return value
    MANIFEST.write_text('// Generated by host/game_map.py.\n'
                        f'pub const BANK_BYTES: usize = {len(blob)};\n'
                        f'pub const BANK_CHECKSUM: u32 = {fnv(blob)};\n')
    dump(REPORT, {
        'format': 'HKMAP02', 'scale_px_per_unit': SCALE * 100, 'shaman_frames': len(s_images) + 1,
        'shaman_sources': s_sources, 'roller_sources': r_sources, 'page_rows': rows, 'page_xy': PAGE_XY,
        'clut_xy': CLUT_XY, 'blob_bytes': len(blob), 'rooms': len(ordered), 'images': len(images),
        'texels': sum(im.width * im.height for im in images),
        'areas': [{'name': a['name'], 'owner': a['owner'], 'rooms': [r['name'] for r in ordered if r['area'] == i]}
                  for i, a in enumerate(areas)],
        'bench_pins': [p['room'] for p in bench_pins], 'door_rooms': doors, 'refused': refused,
        'marker_source': marker['source'],
        'rs_sha256': hashlib.sha256(OUTPUT_RS.read_bytes()).hexdigest(),
        'bin_sha256': hashlib.sha256(blob).hexdigest(),
    })
    preview = Image.new('RGBA', (PAGE, rows), (16, 16, 24, 255))
    for im, xy in zip(images, placed):
        preview.alpha_composite(im, xy)
    preview.save(ROOT / '.hkpsx/game-map-page.png')
    print(f'Game map: {len(ordered)} rooms in {len(areas)} areas, {len(bench_pins)} bench pins, '
          f'{rows} of {PAGE} page rows, {len(blob)} bytes', flush=True)


def _quote(text):
    text = text.replace('’', "'")
    if any(not 32 <= ord(c) <= 126 for c in text):
        raise ValueError(f'map text outside the cooked font: {text!r}')
    return '"' + text.replace('\\', '\\\\').replace('"', '\\"') + '"'


if __name__ == '__main__':
    cook()
