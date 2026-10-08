"""The Snail Shaman of the Ancestral Mound and the Vengeful Spirit he gives.

Crossroads_ShamanTemple is where the slice's first spell is earned, and every
view of it carries the scene's actor bank (Climber, Gruzzers, Baldurs), with
two of the views the Shaman stands in within 7 KB of the 384 KB staging budget.
So the Shaman cannot ride the NPC bank path host/npc_sources.py uses. His
frames and the spell's orb are packed into the quick map's texture page by
host/game_map.py (`art` below) and stream from the disc with the map, so they
cost VRAM that is already the map's and no main RAM.

What the source does, read out of the scene and asserted below:

* `Shaman Meeting` (`Conversation Control`) stands at the foot of the Mound
  while `fireballLevel` is 0. `Convo Choice` switches on PlayerData `shaman`:
  0 says SHAMAN_MEET, then plays `Summon` for 3 s (`Summon Anim`) and in
  `Spell Appear` shakes the camera, sets `shaman` 1 and switches on the
  `Vengeful Spirit` orb; 1 says SHAMAN_SUMMONED_1; 2 says SHAMAN_SUMMONED_2.
  On a scene entry `Check Summoned` moves 1 to 2 and switches the orb on for
  1 and 2.
* The orb (`Vengeful Spirit` FSM) waits for the hero in its trigger, then hides
  the hero and plays `Knight Get Fireball`: 3 s of rumble, 2 s more, a flash
  and a fall (`Get Fireball`), then `Check Fall`: black, the hero lying
  `Prostrate` at (46, 8.2) facing right, `AddMPCharge(100)`, the respawn set
  there, `hasSpell` and `fireballLevel` 1, the game saved, the get-item message
  for the Fireball (PROMPT_FIREBALL, BUTTON_DESC_TAP + GET_FIREBALL_1,
  GET_FIREBALL_2), a 3 s fade back, and control returns on a direction, jump or
  attack (`Prostrate Rise`) with `shaman` 2.
* `Shaman Trapped` sits behind the gate from then on (`Check Active`: gone
  while `fireballLevel` is 0 or once `shaman` reaches 4). Below 3 he says
  SHAMAN_TRAPPED_1 and sets 3; at 3, SHAMAN_TRAPPED_2.

`Shaman Killed Blocker`, the gate and the Mound's own Elder Baldur are the
next step and are not cooked here.
"""
import hashlib, json, math
from PIL import Image
from source import Source, ROOT, dump
from scene import Scene
from shops import Catalog
from cook import tk_sprite, native_sprite, FOCAL, CAM_Z
from materials import quantize_alpha_coverage
from focus import action_parameters
from npc_dialogue import font_advances, _pages, MAX_PAGES, _quote
import language

SCENE = 'Crossroads_ShamanTemple'
MEETING, TRAPPED, ORB = 'Shaman Meeting', 'Shaman Trapped', 'Vengeful Spirit'
CLIPS = ('Idle', 'Summon', 'Sit Idle')
KEYS = {'Meet': 'SHAMAN_MEET', 'Summoned1': 'SHAMAN_SUMMONED_1', 'Summoned2': 'SHAMAN_SUMMONED_2',
        'Trapped1': 'SHAMAN_TRAPPED_1', 'Trapped2': 'SHAMAN_TRAPPED_2'}
STATE_KEYS = {(MEETING, 'Meet'): 'SHAMAN_MEET', (MEETING, 'Summoned 1'): 'SHAMAN_SUMMONED_1',
              (MEETING, 'Summoned 2'): 'SHAMAN_SUMMONED_2', (TRAPPED, 'Trapped 1'): 'SHAMAN_TRAPPED_1',
              (TRAPPED, 'Trapped 2'): 'SHAMAN_TRAPPED_2'}
OUTPUT_RS = ROOT / 'data/shaman.rs'
REPORT = ROOT / '.hkpsx/shaman.json'


def _gid(sc, name):
    found = [g for g, go in sc.gos.items() if go['m_Name'] == name]
    if len(found) != 1:
        raise ValueError(f'{SCENE} holds {len(found)} {name}')
    return found[0]


def _fsm(sc, gid, name):
    return next(tree['fsm'] for _cid, (typ, tree) in sc.objects.items()
                if typ == 'PlayMakerFSM' and tree['m_GameObject']['m_PathID'] == gid and tree['fsm']['name'] == name)


def _box(sc, gid):
    """The object's one enabled trigger box, in world units."""
    boxes = [tree for _cid, (typ, tree) in sc.objects.items()
             if typ == 'BoxCollider2D' and tree['m_GameObject']['m_PathID'] == gid and tree['m_Enabled'] and tree['m_IsTrigger']]
    if len(boxes) != 1:
        raise ValueError(f'{sc.gos[gid]["m_Name"]} has {len(boxes)} trigger boxes')
    off, size = boxes[0]['m_Offset'], boxes[0]['m_Size']
    a = sc.point(gid, off['x'] - size['x'] / 2, off['y'] - size['y'] / 2)
    b = sc.point(gid, off['x'] + size['x'] / 2, off['y'] + size['y'] / 2)
    return [min(a[0], b[0]), min(a[1], b[1]), max(a[0], b[0]), max(a[1], b[1])]


def _child(sc, gid, name):
    transform = sc.transforms[sc.go_transform[gid]]
    for ref in transform['m_Children']:
        kid = sc.transforms[ref['m_PathID']]['m_GameObject']['m_PathID']
        if kid in sc.gos and sc.gos[kid]['m_Name'] == name:
            return kid
    raise ValueError(f'{sc.gos[gid]["m_Name"]} has no {name} child')


def _action(state, kind):
    data = state['actionData']
    return [dict((k or str(i), v) for i, (k, v) in enumerate(action_parameters(data, n)))
            for n, name in enumerate(data['actionNames']) if name.endswith(kind) and data['actionEnabled'][n]]


def _check(sc, meeting, trapped, get_fall):
    """The FSM facts this cook and shaman.rs rely on."""
    for (owner, state), key in STATE_KEYS.items():
        fsm = _fsm(sc, meeting if owner == MEETING else trapped, 'Conversation Control')
        states = {s['name']: s for s in fsm['states']}
        spoken = [a for a in _action(states[state], 'CallMethodProper')
                  if any(isinstance(v, dict) and v.get('stringValue') == key for v in a.values())]
        if not spoken:
            raise ValueError(f'{owner} {state} no longer says {key}')
    choice = {s['name']: s for s in _fsm(sc, meeting, 'Conversation Control')['states']}['Convo Choice']
    switch = choice['actionData']
    index = next(i for i, n in enumerate(switch['actionNames']) if n.endswith('IntSwitch'))
    params = [v for k, v in action_parameters(switch, index) if k is None]
    values = [p['value'] for p in params if isinstance(p, dict)]
    events = [p for p in params if isinstance(p, str)]
    if list(zip(values, events)) != [(0, 'MEET'), (1, 'SUMMONED 1'), (2, 'SUMMONED 2'), (6, 'RETURNED')]:
        raise ValueError(f'Shaman Meeting switches {list(zip(values, events))}')
    fall = {s['name']: s for s in get_fall['states']}
    positions = _action(fall['Black'], 'SetPosition')
    wake = [(p['x']['value'], p['y']['value']) for p in positions
            if isinstance(p.get('x'), dict) and not p['x'].get('useVariable')]
    if wake != [(46.0, 8.199999809265137)]:
        raise ValueError(f'the Knight wakes at {wake}')
    charges = _action(fall['Black'], 'SendMessage')
    def plain(v):
        return v['value'] if isinstance(v, dict) and 'value' in v else v
    mp = [plain(a['functionCall']['IntParameter']) for a in charges if plain(a['functionCall']['FunctionName']) == 'AddMPCharge']
    if mp != [100]:
        raise ValueError(f'waking charges {mp} SOUL')
    writes = {a['intName']['value'] if isinstance(a['intName'], dict) else a['intName']:
              a['value']['value'] if isinstance(a['value'], dict) else a['value']
              for a in _action(fall['Set Respawns'], 'SetPlayerDataInt')}
    if writes.get('fireballLevel') != 1:
        raise ValueError(f'the orb writes {writes}')
    return wake[0], mp[0]


def _frames(source, sc, gid):
    from npc_sources import _component
    animator = _component(sc, gid, 'tk2dSpriteAnimator')
    sprite = _component(sc, gid, 'tk2dSprite')
    library_o = source.ref(sc.file, animator['library'])
    library = source.read(library_o)
    matrix = sc.world(sc.go_transform[gid])
    sx, sy = abs(matrix[0][0] * sprite['_scale']['x']), abs(matrix[1][1] * sprite['_scale']['y'])
    if matrix[0][0] <= 0:
        raise ValueError(f'{sc.gos[gid]["m_Name"]} is mirrored')
    scale = FOCAL / -CAM_Z
    textures, images, boxes, clips = {}, [], [], []
    for name in CLIPS:
        clip = next(c for c in library['clips'] if c['name'] == name)
        start = len(images)
        for frame in clip['frames']:
            col = source.ref(library_o.assets_file, frame['spriteCollection'])
            image, box = tk_sprite(source, col.assets_file, source.read(col), frame['spriteId'], textures)
            box = (box[0] * sx, box[1] * sy, box[2] * sx, box[3] * sy)
            dims = (max(1, math.ceil((box[2] - box[0]) * scale)), max(1, math.ceil((box[3] - box[1]) * scale)))
            images.append(image.resize(dims, Image.Resampling.LANCZOS))
            boxes.append(box)
        clips.append(dict(name=name, start=start, count=len(clip['frames']), fps=int(clip['fps']),
                          wrap=0 if clip['wrapMode'] == 0 else 2, loop=clip.get('loopStart', 0)))
    return images, boxes, clips, source.sid(library_o)


def art(source):
    """The Shaman's frames (Idle, Summon, Sit Idle, in that order) and the
    orb's one frame, scaled to the screen, for host/game_map.py to pack into
    the map page. Returns (images, boxes, clips, orb_image, orb_box, sources)."""
    catalog = Catalog(source)
    sc = Scene(source, catalog.files[SCENE])
    images, boxes, clips, library = _frames(source, sc, _gid(sc, MEETING))
    renderer = next(tree for _cid, (typ, tree) in sc.objects.items()
                    if typ == 'SpriteRenderer' and tree['m_GameObject']['m_PathID'] == _gid(sc, ORB))
    orb_o = source.ref(sc.file, renderer['m_Sprite'])
    orb_image, orb_box = native_sprite(orb_o)
    scale = FOCAL / -CAM_Z
    orb_image = orb_image.resize((max(1, math.ceil((orb_box[2] - orb_box[0]) * scale)),
                                  max(1, math.ceil((orb_box[3] - orb_box[1]) * scale))), Image.Resampling.LANCZOS)
    return images, boxes, clips, orb_image, orb_box, {'library': library, 'orb': source.sid(orb_o)}


def cook():
    source = Source()
    catalog = Catalog(source)
    regions = json.loads((ROOT / 'data/regions.json').read_text())
    scene = next(s for s in regions['scenes'] if s['scene_name'] == SCENE)
    sc = Scene(source, catalog.files[SCENE])
    meeting, trapped, orb = _gid(sc, MEETING), _gid(sc, TRAPPED), _gid(sc, ORB)
    getter = _gid(sc, 'Knight Get Fireball')
    fall_owner = _child(sc, getter, 'Knight Cutscene Animator')
    wake, soul = _check(sc, meeting, trapped, _fsm(sc, fall_owner, 'Check Fall'))
    words = language.sheet(source, 'Shaman')
    prompts = language.sheet(source, 'Prompts')
    advances = font_advances(source)
    pages = {name: _pages(words[key], advances) for name, key in KEYS.items()}
    for name, p in pages.items():
        if not 1 <= len(p) <= MAX_PAGES:
            raise ValueError(f'Shaman {name} needs {len(p)} pages')
    control = {n: next(tree for _cid, (typ, tree) in sc.objects.items() if typ == 'PlayMakerFSM'
                       and tree['m_GameObject']['m_PathID'] == g and tree['fsm']['name'] == 'npc_control')
               for n, g in ((MEETING, meeting), (TRAPPED, trapped))}
    from focus import fsm_variables
    labels = {n: fsm_variables(f['fsm'])['Prompt Name'] for n, f in control.items()}

    def q16(v):
        return round(v * 65536)

    def point(g):
        p = sc.point(g)
        return f'[{q16(p[0])},{q16(p[1])}]'

    def rect(r):
        return '[' + ','.join(str(q16(v)) for v in r) + ']'

    def rs_pages(name):
        return '&[' + ','.join('&[' + ','.join(_quote(line) for line in page) + ']' for page in pages[name]) + ']'

    fire_2 = prompts['GET_FIREBALL_2'].split('<br>')
    lines = ['// Generated by host/shaman.py from the local Windows source; no retail payload is embedded.',
             f'pub const SHAMAN_SCENE:usize={scene["scene_id"]};',
             f'pub const MEETING_AT:[i32;2]={point(meeting)};',
             f'pub const MEETING_TRIGGER:[i32;4]={rect(_box(sc, meeting))};',
             f'pub const MEETING_MARKER:[i32;2]={point(_child(sc, meeting, "Prompt Marker"))};',
             f'pub const TRAPPED_AT:[i32;2]={point(trapped)};',
             f'pub const TRAPPED_TRIGGER:[i32;4]={rect(_box(sc, trapped))};',
             f'pub const TRAPPED_MARKER:[i32;2]={point(_child(sc, trapped, "Prompt Marker"))};',
             f'pub const ORB_AT:[i32;2]={point(orb)};',
             f'pub const ORB_TRIGGER:[i32;4]={rect(_box(sc, orb))};',
             f'pub const WAKE_AT:[i32;2]=[{q16(wake[0])},{q16(wake[1])}];',
             f'pub const WAKE_SOUL:u16={soul};',
             f'pub const MEETING_LABEL:&str={_quote(labels[MEETING])};',
             f'pub const TRAPPED_LABEL:&str={_quote(labels[TRAPPED])};',
             ]
    for name in KEYS:
        lines.append(f'pub static {name.upper()}:&[&[&str]]={rs_pages(name)};')
    lines += [f'pub const SPELL_NAME:&str={_quote(prompts["PROMPT_FIREBALL"])};',
              f'pub const TAP:&str={_quote(prompts["BUTTON_DESC_TAP"])};',
              f'pub const SPELL_LINE:&str={_quote(prompts["GET_FIREBALL_1"])};',
              'pub const SPELL_NOTE:&[&str]=&[' + ','.join(_quote(t.strip()) for t in fire_2) + '];']
    OUTPUT_RS.write_text('\n'.join(lines) + '\n')
    dump(REPORT, {'format': 'HKSHAMAN02', 'scene_id': scene['scene_id'], 'wake': wake, 'soul': soul,
                  'art': 'packed into the quick map page by host/game_map.py (data/shaman_art.rs)',
                  'pages': {k: len(v) for k, v in pages.items()},
                  'sha256': hashlib.sha256(OUTPUT_RS.read_bytes()).hexdigest(),
                  'not_reproduced': ['the Talk clips (the Shaman idles while speaking)', 'the summon particles and the orb idle particles',
                                     'the Knight Get Fireball cutscene knight and its orbs, flash and wave',
                                     'the gate, Shaman Killed Blocker and the Mound Elder Baldur']})
    print(f'Shaman: wakes at {wake} with {soul} SOUL, {sum(len(v) for v in pages.values())} pages', flush=True)


if __name__ == '__main__':
    cook()
