"""Cornifer's Crossroads conversation and the map he sells, for game/src/mapper.rs.

Cornifer (Crossroads_33, `Cornifer`) is an NPC the generic chain cook cannot
hold: his `Conversation Control` ends in a yes/no box with a Geo price, and
what he says next depends on the answer and the wallet. So his placement,
art and talk trigger come through host/npc_sources.py like any NPC, and the
conversation itself is this file's table plus the small state machine in
mapper.rs. Every key, the price and the order below are read out of the FSM
and asserted, so an install whose conversation moved fails the cook instead
of shipping a guest that says the wrong thing.

What the FSM does, in the order mapper.rs runs it (`Conversation Control`):

* Talking at all sets `openedMapperShop` (`Open map Shop`), which is what
  opens Iselda's shop in Dirtmouth.
* `Convo Choice`: unmet, `Meet` (sets `metCornifer`, says CORNIFER_MEET);
  already holding this area's map, `Bought Choice` (first time
  CORNIFER_INTRO_1 then CORNIFER_INTRO_2 and sets `corniferIntroduced`, after
  that <AREA>_BOUGHT); spoken this visit, CORNIFER_AGAIN; otherwise
  <AREA>_GREET. The first three that are not `Bought Choice` go on to the box.
* The box (`Send Text`): CORNIFER_PROMPT with `Toll Cost` = the `Prices`
  sheet's MAP_<AREA> times 0.75 (`Set Map Price`). `Yes` is unselectable while
  the wallet is short (`Enough Geo` on the box's `Yes`).
* Yes: the box takes the Geo (`Take Geo`), `Got Map Bool` is set, and on the
  first map ever `hasMap` too with the first-map prompt (GET_MAP_1, HOLD,
  GET_MAP_2 from `Prompts`) until jump, attack, cast or the map button, then
  CORNIFER_ISELDA. A later map skips straight to the end.
* No: `Enough Geo?` says CORNIFER_REFUSE with the Geo in hand and
  CORNIFER_NOT_ENOUGH without it.

Cornifer stays in the Crossroads for the whole slice: his `Check Active`
removes him once `corn_crossroadsLeft` is set, and only meeting him in another
area sets that (`Not At Crossroads`), which the disc cannot reach.
"""
import hashlib, json
from source import Source, ROOT, dump
from scene import Scene
from shops import Catalog
from focus import action_parameters, fsm_variables
from npc_dialogue import font_advances, _pages, MAX_PAGES, _quote
import language

SCENE = 'Crossroads_33'
OBJECT = 'Cornifer'
FSM = 'Conversation Control'
OUTPUT = ROOT / 'data/cornifer.rs'
REPORT = ROOT / '.hkpsx/cornifer.json'
# (state, key) pairs this cook depends on. A key built from `$Area` is written
# with AREA in its place.
STATE_KEYS = {
    'Meet': 'CORNIFER_MEET', 'Again': 'CORNIFER_AGAIN', 'Send Text': 'CORNIFER_PROMPT',
    'Refuse': 'CORNIFER_REFUSE', 'Not Enough': 'CORNIFER_NOT_ENOUGH',
    'Introduce 3': 'CORNIFER_INTRO_1', 'Introduce 2': 'CORNIFER_INTRO_2',
    'Iselda Mention': 'CORNIFER_ISELDA',
}
TRANSITIONS = {
    ('Send Text', 'YES'): 'Geo Pause and GetMap', ('Send Text', 'NO'): 'YN Down',
    ('Enough Geo?', 'ENOUGH'): 'Refuse', ('Enough Geo?', 'NOT ENOUGH'): 'Not Enough',
    ('First Map?', 'YES'): 'Get F Map', ('First Map?', 'NO'): 'Get Map',
    ('Get F Map', 'FINISHED'): 'Map Input', ('Map Input', 'CONVO END'): 'Prompt Down',
    ('Box Up 3', 'FINISHED'): 'Iselda Mention', ('Bought Choice', 'INTRODUCE'): 'Introduce 3',
    ('Bought Choice', 'FINISHED'): 'Area Bought', ('Introduce 3', 'CONVO_FINISH'): 'Introduce 2',
}


def _conversation(sc):
    gid = next(g for g, go in sc.gos.items() if go['m_Name'] == OBJECT)
    fsm = next(tree['fsm'] for _cid, (typ, tree) in sc.objects.items()
               if typ == 'PlayMakerFSM' and tree['m_GameObject']['m_PathID'] == gid and tree['fsm']['name'] == FSM)
    return gid, fsm


def _spoken_keys(state):
    """The literal StartConversation (key, sheet) pairs one state speaks."""
    data = state['actionData']
    out = []
    for i, name in enumerate(data['actionNames']):
        if not name.endswith('CallMethodProper') or not data['actionEnabled'][i]:
            continue
        params = action_parameters(data, i, objects=True)
        method = next(v for k, v in params if k == 'methodName')
        if (method.get('value') if isinstance(method, dict) else method) != 'StartConversation':
            continue
        strings = [v for _k, v in params if isinstance(v, dict) and 'stringValue' in v]
        out.append(tuple(None if s.get('useVariable') else s['stringValue'] for s in strings[:2]))
    return out


def _writes(state):
    data = state['actionData']
    return [action_parameters(data, i)[0][1] for i, name in enumerate(data['actionNames'])
            if name.endswith('SetPlayerDataBool') and data['actionEnabled'][i]]


def cook():
    source = Source()
    catalog = Catalog(source)
    regions = json.loads((ROOT / 'data/regions.json').read_text())
    scene = next(s for s in regions['scenes'] if s['scene_name'] == SCENE)
    npc = next(n for r in regions['regions'] for n in r.get('npcs', []) if n['name'] == OBJECT)
    sc = Scene(source, catalog.files[SCENE])
    gid, fsm = _conversation(sc)
    if npc['game_object'] != gid:
        raise ValueError('the cooked Cornifer placement is not the conversation owner')
    states = {s['name']: s for s in fsm['states']}
    for (state, event), target in TRANSITIONS.items():
        found = next((t['toState'] for t in states[state]['transitions'] if t['fsmEvent']['name'] == event), None)
        if found != target:
            raise ValueError(f'Cornifer {state} {event} goes to {found}, not {target}')
    for state, key in STATE_KEYS.items():
        spoken = _spoken_keys(states[state])
        if [k for k, _ in spoken] != [key] or spoken[0][1] != 'Cornifer':
            raise ValueError(f'Cornifer {state} no longer speaks Cornifer/{key}: {spoken}')
    variables = fsm_variables(fsm)
    area = variables['Area']
    if variables['Got Map Bool'] != f'map{area.title()}' or variables['Left Bool'] != f'corn_{area.lower()}Left':
        raise ValueError(f'Cornifer sells {variables["Got Map Bool"]} in area {area}')
    for state, field in [('Meet', 'metCornifer'), ('Open map Shop', 'openedMapperShop'),
                         ('Get F Map', 'hasMap'), ('Introduce 3', 'corniferIntroduced')]:
        if field not in [w.get('value') if isinstance(w, dict) else w for w in _writes(states[state])]:
            raise ValueError(f'Cornifer {state} no longer sets {field}')
    prices = language.sheet(source, 'Prices')
    sheet_price = int(prices[f'MAP_{area}'])
    # Set Map Price: ConvertIntToFloat, FloatMultiply 0.75, ConvertFloatToInt.
    multiply = next(action_parameters(states['Set Map Price']['actionData'], i)
                    for i, n in enumerate(states['Set Map Price']['actionData']['actionNames'])
                    if n.endswith('FloatMultiply'))
    factor = next(v for k, v in multiply if k == 'multiplyBy')
    factor = factor['value'] if isinstance(factor, dict) else factor
    if abs(factor - 0.75) > 1e-6:
        raise ValueError(f'Cornifer discounts by {factor}, not 0.75')
    price = int(sheet_price * factor)
    words = language.sheet(source, 'Cornifer')
    prompts = language.sheet(source, 'Prompts')
    advances = font_advances(source)
    keys = dict(STATE_KEYS, Greet=f'{area}_GREET', Bought=f'{area}_BOUGHT')
    pages = {}
    for name, key in keys.items():
        pages[key] = _pages(words[key], advances)
        if not 1 <= len(pages[key]) <= MAX_PAGES:
            raise ValueError(f'Cornifer {key} needs {len(pages[key])} pages')

    def rs_pages(key):
        return '&[' + ','.join('&[' + ','.join(_quote(line) for line in page) + ']' for page in pages[key]) + ']'

    lines = ['// Generated by host/cornifer.py from the local Windows source; no retail payload is embedded.',
             f'pub const CORNIFER_SCENE:usize={scene["scene_id"]};',
             f'pub const CORNIFER_SOURCE:u32={gid};',
             f'pub const CORNIFER_LABEL:&str={_quote(npc["prompt"])};',
             'pub const CORNIFER_MARKER:[i32;2]=[%d,%d];' % (round(npc['marker'][0] * 65536), round(npc['marker'][1] * 65536)),
             f'pub const CORNIFER_PRICE:u32={price};',
             f'pub static MEET:&[&[&str]]={rs_pages("CORNIFER_MEET")};',
             f'pub static GREET:&[&[&str]]={rs_pages(keys["Greet"])};',
             f'pub static AGAIN:&[&[&str]]={rs_pages("CORNIFER_AGAIN")};',
             f'pub static PROMPT:&[&[&str]]={rs_pages("CORNIFER_PROMPT")};',
             f'pub static REFUSE:&[&[&str]]={rs_pages("CORNIFER_REFUSE")};',
             f'pub static NOT_ENOUGH:&[&[&str]]={rs_pages("CORNIFER_NOT_ENOUGH")};',
             f'pub static BOUGHT:&[&[&str]]={rs_pages(keys["Bought"])};',
             f'pub static INTRO_1:&[&[&str]]={rs_pages("CORNIFER_INTRO_1")};',
             f'pub static INTRO_2:&[&[&str]]={rs_pages("CORNIFER_INTRO_2")};',
             f'pub static ISELDA:&[&[&str]]={rs_pages("CORNIFER_ISELDA")};',
             f'pub const GET_MAP_1:&str={_quote(prompts["GET_MAP_1"])};',
             f'pub const GET_MAP_2:&str={_quote(prompts["GET_MAP_2"])};',
             f'pub const HOLD:&str={_quote(prompts["BUTTON_DESC_HOLD"])};']
    OUTPUT.write_text('\n'.join(lines) + '\n')
    dump(REPORT, {'format': 'HKCORN01', 'scene': SCENE, 'scene_id': scene['scene_id'], 'source': npc['source'],
                  'area': area, 'sheet_price': sheet_price, 'multiplier': factor, 'price': price,
                  'keys': keys, 'pages': {k: len(v) for k, v in pages.items()},
                  'sha256': hashlib.sha256(OUTPUT.read_bytes()).hexdigest(),
                  'not_reproduced': ['the name title card (Title Display)', 'Talk R/L End clips and the hero look',
                                     'the Hum loop and the voice clips', 'the Map Get Msg banner on a second map']})
    print(f'Cornifer: {area} map for {price} Geo, {sum(len(v) for v in pages.values())} pages', flush=True)


if __name__ == '__main__':
    cook()
