"""Sly's rescue in Room_ruinhouse, for game/src/sly.rs.

Sly (`Sly Dazed` in Room_ruinhouse, off Crossroads_04's door1) is placed, drawn
and given his talk trigger by host/npc_sources.py like any NPC, and marked
scripted there, because what his conversation writes is not a cursor: it is
`slyRescued`, which Dirtmouth's `Check Opened` reads to show his shop and its
door. So this file cooks his pages and asserts the FSM the guest follows:

* `Convo Choice` reads `slyRescued`: unset goes to `Meet`, set to `Repeat`.
* `Meet` sets `slyRescued` as it is entered and speaks SLY_RESCUE_1, then
  `Meet 2` SLY_RESCUE_2 and `Meet 3` SLY_RESCUE_3 (npcs.conversation_chain
  walks the FSM and gives those entries in order).
* `Repeat` speaks SLY_RESCUE_REPEAT (and sets the field again).
* `Active?` reads `slyRescued` when the room loads and destroys him when it is
  set, so a rescued Sly is gone the next time the room loads.

The text is written as one byte string with a line table and a page table, so
that the guest's `sly` room module carries all of it (host/code_modules.py).
"""
import hashlib, json
from source import Source, ROOT, dump
from scene import Scene
from shops import Catalog
from focus import action_fields
from npcs import conversation_chain, playerdata_defaults
from npc_dialogue import font_advances, _pages, MAX_PAGES, MAX_PAGE_GLYPHS, FOOTER, _quote
import language

SCENE = 'Room_ruinhouse'
OBJECT = 'Sly Dazed'
FSM = 'Conversation Control'
FIELD = 'slyRescued'
OUTPUT = ROOT / 'data/sly.rs'
REPORT = ROOT / '.hkpsx/sly.json'
RESCUE = ['SLY_RESCUE_1', 'SLY_RESCUE_2', 'SLY_RESCUE_3']
REPEAT = ['SLY_RESCUE_REPEAT']


def _fsm(sc, gid):
    return next(tree['fsm'] for _cid, (typ, tree) in sc.objects.items()
                if typ == 'PlayMakerFSM' and tree['m_GameObject']['m_PathID'] == gid and tree['fsm']['name'] == FSM)


def _actions(state, kind):
    data = state['actionData']
    return [action_fields(data, i) for i, name in enumerate(data['actionNames'])
            if name.rsplit('.', 1)[-1] == kind and data['actionEnabled'][i]]


def _value(field):
    return field.get('value') if isinstance(field, dict) else field


def _check_active(fsm):
    """`Active?`: GetPlayerDataBool slyRescued into a variable, then BoolTest on it, DESTROY when true."""
    states = {s['name']: s for s in fsm['states']}
    active = states['Active?']
    reads = _actions(active, 'GetPlayerDataBool')
    tests = _actions(active, 'BoolTest')
    if len(reads) != 1 or len(tests) != 1 or _value(reads[0]['boolName']) != FIELD:
        raise ValueError(f'{OBJECT} Active? no longer reads {FIELD} once')
    store = reads[0]['storeValue']
    tested = tests[0]['boolVariable']
    if not (isinstance(store, dict) and isinstance(tested, dict) and store.get('name') == tested.get('name')):
        raise ValueError(f'{OBJECT} Active? tests something other than {FIELD}')
    destroy = next((t['toState'] for t in active['transitions'] if t['fsmEvent']['name'] == tests[0]['isTrue']), None)
    if destroy != 'Destroy' or [a.rsplit('.', 1)[-1] for a in states['Destroy']['actionData']['actionNames']] != ['DestroySelf']:
        raise ValueError(f'{OBJECT} Active? no longer destroys him when {FIELD} is set')


def cook():
    source = Source()
    catalog = Catalog(source)
    regions = json.loads((ROOT / 'data/regions.json').read_text())
    scene = next(s for s in regions['scenes'] if s['scene_name'] == SCENE)
    npc = next(n for r in regions['regions'] if r['scene_id'] == scene['scene_id']
               for n in r.get('npcs', []) if n['name'] == OBJECT)
    sc = Scene(source, catalog.files[SCENE])
    gid = npc['game_object']
    if sc.gos[gid]['m_Name'] != OBJECT:
        raise ValueError(f'the cooked {OBJECT} placement is not the conversation owner')
    fsm = _fsm(sc, gid)
    _check_active(fsm)
    defaults = playerdata_defaults(source.directory / 'Managed/Assembly-CSharp.dll')
    if defaults.get(FIELD) not in (0, False):
        raise ValueError(f'{FIELD} is not unset on a new save')
    chain = conversation_chain(source, sc, gid, defaults)
    keys = [[e['key'] for e in c['entries']] for c in chain]
    if keys != [RESCUE, REPEAT] or any(e['sheet'] != 'Sly' for c in chain for e in c['entries']):
        raise ValueError(f'{OBJECT} no longer speaks {RESCUE} then {REPEAT}: {keys}')
    first = chain[0]['entries'][0]
    if [w['field'] for w in first['writes']] != [FIELD] or first['writes'][0]['value'] is not True:
        raise ValueError(f'{OBJECT} Meet no longer sets {FIELD} as it is entered')
    if [w['field'] for c in chain for w in c['writes']] != [FIELD, FIELD]:
        raise ValueError(f'{OBJECT} writes more than {FIELD}')
    if not chain[1]['terminal']:
        raise ValueError(f'{OBJECT} Repeat is not the conversation he settles on')
    words = language.sheet(source, 'Sly')
    advances = font_advances(source)
    pages = []
    for key in RESCUE + REPEAT:
        pages.extend((key, page) for page in _pages(words[key], advances))
    rescue_pages = sum(1 for key, _ in pages if key in RESCUE)
    for key, page in pages:
        if sum(len(line.replace(' ', '')) for line in page) + len(FOOTER) > MAX_PAGE_GLYPHS:
            raise ValueError(f'{key} exceeds the panel glyph budget')
    if not 1 <= rescue_pages <= 3 * MAX_PAGES or len(pages) - rescue_pages > MAX_PAGES:
        raise ValueError(f'Sly needs {rescue_pages} and {len(pages) - rescue_pages} pages')
    text, line_starts, page_starts = '', [], []
    for _key, page in pages:
        page_starts.append(len(line_starts))
        for line in page:
            _quote(line)  # printable ASCII only: the guest reads the bytes as a str
            line_starts.append(len(text))
            text += line
    line_starts.append(len(text))
    page_starts.append(len(line_starts) - 1)
    if len(text) >= 1 << 16 or len(line_starts) >= 256 or len(pages) >= 255:
        raise ValueError('Sly text tables outgrew their index types')
    max_lines = max(len(page) for _key, page in pages)
    blob = text.replace('\\', '\\\\').replace('"', '\\"')
    lines = ['// Generated by host/sly.py from the local Windows source; no retail payload is embedded.',
             f'pub const SLY_SCENE:usize={scene["scene_id"]};',
             f'pub const SLY_SOURCE:u32={gid};',
             f'pub const SLY_LABEL:&str={_quote(npc["prompt"])};',
             'pub const SLY_MARKER:[i32;2]=[%d,%d];' % (round(npc['marker'][0] * 65536), round(npc['marker'][1] * 65536)),
             f'/// Pages 0 to {rescue_pages - 1} are `Meet`\'s, the rest `Repeat`\'s.',
             f'pub const SLY_RESCUE_PAGES:u8={rescue_pages};',
             f'pub const SLY_PAGE_COUNT:u8={len(pages)};',
             f'pub const MAX_LINES:usize={max_lines};',
             f'pub static SLY_TEXT:[u8;{len(text)}]=*b"{blob}";',
             f'pub static SLY_LINES:[u16;{len(line_starts)}]=[{",".join(map(str, line_starts))}];',
             f'pub static SLY_PAGES:[u8;{len(page_starts)}]=[{",".join(map(str, page_starts))}];']
    OUTPUT.write_text('\n'.join(lines) + '\n')
    dump(REPORT, {
        'scene': SCENE, 'scene_id': scene['scene_id'], 'object': OBJECT, 'source_id': gid,
        'field': FIELD, 'rescue': RESCUE, 'repeat': REPEAT,
        'pages': len(pages), 'rescue_pages': rescue_pages, 'text_bytes': len(text),
        'output_sha256': hashlib.sha256(OUTPUT.read_bytes()).hexdigest(),
        'limitations': ['He stays on Dazed: Meet 2\'s Wake clip, the steam and the conversation sounds are not cooked',
                        'The three Meet entries run as one page list; the source closes and reopens the box between them',
                        'The dream dialogue is not cooked'],
    })
    print(f'Sly: {rescue_pages} rescue pages, {len(pages) - rescue_pages} repeat pages, {len(text)} text bytes',
          flush=True)


if __name__ == '__main__':
    cook()
