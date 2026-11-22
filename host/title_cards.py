"""Area and boss title cards: the source `Area Title Controller`s and `Titles`.

Each admitted scene may carry one `Area Title Controller`. Its FSM variables
say which area it announces (`Area Event`, a key into the `Titles` language
sheet's `<EVENT>_SUPER`, `_MAIN` and `_SUB` lines), whether it waits for the
Knight to enter its trigger box (`Wait for Trigger`), how long it pauses before
the card on a first and on a later visit, and whether it only speaks on a
revisit. The guest shows the card when the Knight enters an area other than
the saved current area (PlayerData `currentArea`, AreaTitleController.Finish).
A sub-area (King's Pass, the Ancestral Mound's shaman temple; the `subArea`
flag of AreaTitleController's area list, read from its constructor's CIL)
always shows its small card and never becomes the current area. Boss names come from the same sheet (`FALSE_KNIGHT_MAIN` and so on) and
are shown by the boss's own code.

The text is drawn with the dialogue font the disc already carries, the main
line from its own card strip at 40 px (hk-cook dialogue-art); a boss card is drawn in small capitals of the TrajanPro-Bold the menus' capitals use. It reads the
source scenes and cooks nothing, so it is not a cooker input. Regenerate with

    .venv/bin/python host/title_cards.py

which writes data/title_cards.rs and .hkpsx/title-cards.json.
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

ONE = 65536
# Boss titles the guest can ask for by name, from the same `Titles` sheet.
BOSSES = ('FALSE_KNIGHT', 'MAWLEK', 'BIGFLY')
# The dialogue font covers printable ASCII; the sheet uses typographic quotes.
ASCII = {'’': "'", '‘': "'", '“': '"', '”': '"', '–': '-', '—': '-'}


def ascii_text(text):
    out = ''.join(ASCII.get(c, c) for c in text)
    if any(not 32 <= ord(c) <= 126 for c in out):
        raise ValueError(f'title {text!r} has characters the dialogue font lacks')
    return out


def survey(source, scenes):
    from scene import Scene
    import activation
    rows, refused = [], []
    for entry in scenes:
        sc = Scene(source, entry['file'])
        found = []
        for sid, (typename, tree) in sorted(sc.objects.items()):
            if typename != 'PlayMakerFSM' or tree['fsm']['name'] != 'Area Title Controller':
                continue
            gid = tree['m_GameObject']['m_PathID']
            if not sc.active(gid):
                continue
            _, variables, _ = activation.instantiate(source, sc.file, tree)
            box = None
            if variables.get('Wait for Trigger'):
                boxes = [t for i, (k, t) in sc.objects.items() if k == 'BoxCollider2D'
                         and t['m_GameObject']['m_PathID'] == gid and t['m_IsTrigger']]
                if len(boxes) != 1:
                    refused.append({'scene': entry['scene_name'], 'reason': 'waits for a trigger it does not have'})
                    continue
                m = sc.world(sc.go_transform[gid]); b = boxes[0]
                cx = m[0][3] + b['m_Offset']['x'] * m[0][0]; cy = m[1][3] + b['m_Offset']['y'] * m[1][1]
                hw = b['m_Size']['x'] * abs(m[0][0]) / 2; hh = b['m_Size']['y'] * abs(m[1][1]) / 2
                box = [cx - hw, cy - hh, cx + hw, cy + hh]
            found.append({'scene': entry['scene_id'], 'scene_name': entry['scene_name'],
                          'event': variables['Area Event'], 'trigger': box,
                          'unvisited_ticks': round(variables['Unvisited Pause'] * 60),
                          'visited_ticks': round(variables['Visited Pause'] * 60),
                          'only_on_revisit': bool(variables['Only On Revisit'])})
        if len(found) > 1:
            raise ValueError(f'{entry["scene_name"]} carries {len(found)} area title controllers')
        rows += found
    return rows, refused


def sub_areas():
    """`{area event: subArea}` from AreaTitleController's constructor."""
    return {event: sub for event, (_, sub) in area_list().items()}


def area_list():
    """`{area event: (areaID, subArea)}` from AreaTitleController's constructor: each area of its list is
    `ldstr NAME`, `ldc.i4 areaID`, `ldc.i4 subArea`, `ldstr visitedBool`, then `newobj`. `areaID` is what
    `Finish` compares PlayerData `currentArea` with."""
    import re
    import language
    from inspect_il import inspect
    text = inspect(language.managed_directory() / 'Assembly-CSharp.dll', {'AreaTitleController'})
    block = text.split('\nAreaTitleController::.ctor ', 1)[1].split('\n\n', 1)[0]
    rows = [line.split(None, 2)[1:] for line in block.splitlines()[1:] if line.strip()]
    def value(row):
        opcode, operand = (row + [''])[:2]
        if not opcode.startswith('ldc.i4'):
            return None
        if opcode.startswith('ldc.i4.') and opcode[7:].isdigit():
            return int(opcode[7:])
        found = re.search(r'-?\d+', operand)
        return int(found.group(0)) if found else None
    flags = {}
    for i in range(len(rows) - 3):
        if rows[i][0] == 'ldstr' and rows[i + 3][0] == 'ldstr':
            area_id, sub = value(rows[i + 1]), value(rows[i + 2])
            if area_id is not None and sub in (0, 1):
                flags[rows[i][1].strip("'")] = (area_id, bool(sub))
    if flags.get('KINGSPASS', (0, None))[1] is not True or flags.get('CROSSROADS') != (2, False):
        raise ValueError(f'AreaTitleController area list not understood: {flags}')
    return flags


def texts(source, keys):
    import language
    sheet = language.sheet(source, 'Titles')
    out = []
    for key in keys:
        lines = [ascii_text(sheet.get(f'{key}_{part}', '')) for part in ('SUPER', 'MAIN', 'SUB')]
        if not lines[1]:
            raise ValueError(f'no {key}_MAIN title')
        out.append(lines)
    return out


def check_card_font(lines, boss_lines):
    """Every area main line must be drawable from the glyphs `hk-cook dialogue-art` cooked for the cards
    (host/hk-cook/src/font_strip.rs CARD_MAINS) and every line of a boss card from the boss small capitals
    (BOSS_LINES), which is all the card strip carries."""
    import re
    layout = (ROOT / 'data/font_layout.rs').read_text()
    found = re.search(r'CARD_CHARS: &\[u8\] = b"([^"]*)"', layout)
    if not found:
        raise ValueError('data/font_layout.rs has no CARD_CHARS: run hk-cook dialogue-art first')
    # A boss glyph's key is the source text's letter with bit 7 set, written as a byte escape.
    keys = re.sub(r'\\x([0-9a-f]{2})', lambda m: chr(int(m.group(1), 16)), found[1])
    chars = [c for c in keys if ord(c) < 128]
    boss = [chr(ord(c) & 127) for c in keys if ord(c) >= 128]
    missing = sorted({c for _, main, _ in lines for c in main if c != ' ' and c not in chars})
    if missing:
        raise ValueError(f'the card font has no glyph for {missing}: add the title to CARD_MAINS in '
                         'host/hk-cook/src/font_strip.rs and cook again')
    missing = sorted({c for t in boss_lines for part in t for c in part if c != ' ' and c not in boss})
    if missing:
        raise ValueError(f'the boss card font has no glyph for {missing}: add the title to BOSS_LINES in '
                         'host/hk-cook/src/font_strip.rs and cook again')


def rust(areas, events, lines, area_ids=None):
    """`area_ids`: each event's AreaTitleController `areaID` (0 for a boss title), the encoding of PlayerData
    `currentArea` the guest's title card reads and writes."""
    esc = lambda s: s.replace('\\', '\\\\').replace('"', '\\"')
    out = ['// Generated by host/title_cards.py from the source area title controllers and the Titles sheet.',
           '#[derive(Clone,Copy)] pub struct Area {pub scene:u8,pub title:u8,pub trigger:Option<[i32;4]>,pub unvisited_ticks:u16,pub visited_ticks:u16,pub only_on_revisit:bool,pub sub_area:bool}',
           f'pub static TITLES:[[&str;3];{len(lines)}]=[' + ''.join(
               '[' + ','.join(f'"{esc(x)}"' for x in t) + '],' for t in lines) + '];']
    for i, key in enumerate(events):
        out.append(f'pub const {key}:u8={i};')
    ids = [(area_ids or {}).get(key, 0) for key in events]
    out.append('/// Each title\'s AreaTitleController `areaID`, the value PlayerData `currentArea` holds; 0 for a boss.')
    out.append(f'pub const AREA_IDS:[u8;{len(events)}]=[{",".join(map(str, ids))}];')
    out.append(f'pub static AREAS:[Area;{len(areas)}]=[')
    for a in areas:
        trig = 'None' if a['trigger'] is None else 'Some([' + ','.join(str(round(v * ONE)) for v in a['trigger']) + '])'
        out.append(f'Area{{scene:{a["scene"]},title:{events.index(a["event"])},trigger:{trig},'
                   f'unvisited_ticks:{a["unvisited_ticks"]},visited_ticks:{a["visited_ticks"]},'
                   f'only_on_revisit:{str(a["only_on_revisit"]).lower()},'
                   f'sub_area:{str(a.get("sub_area", False)).lower()}}},')
    out.append('];')
    return '\n'.join(out) + '\n'


def main():
    from source import Source
    from quality import SCENE_TABLE
    source = Source()
    areas, refused = survey(source, SCENE_TABLE)
    listed = area_list()
    for a in areas:
        if a['event'] not in listed:
            raise ValueError(f'area event {a["event"]} is not in AreaTitleController\'s list')
        a['area_id'], a['sub_area'] = listed[a['event']]
        if not a['sub_area'] and not 0 < a['area_id'] < 256:
            raise ValueError(f'area {a["event"]} has areaID {a["area_id"]}, which the guest cannot tell from none')
    events = sorted({a['event'] for a in areas}) + [b for b in BOSSES]
    lines = texts(source, events)
    check_card_font(lines[:len(events) - len(BOSSES)], lines[len(events) - len(BOSSES):])
    ids = {a['event']: a['area_id'] for a in areas}
    (ROOT / 'data/title_cards.rs').write_text(rust(areas, events, lines, ids))
    (ROOT / '.hkpsx/title-cards.json').write_text(json.dumps({'areas': areas, 'refused': refused,
        'titles': dict(zip(events, lines)),
        'limitations': ['Drawn in the dialogue font, an area title\'s main line in its own 40 px card strip and a boss title in ',
                        'two sizes of capitals; the source area title\'s Trajan face, its ornaments and its exact timing ',
                        'curves are not presented.']}, indent=2) + '\n')
    print(f'{len(areas)} area title controllers, {len(events)} titles: '
          + ', '.join(' '.join(x for x in l if x) for l in lines), flush=True)


if __name__ == '__main__':
    main()
