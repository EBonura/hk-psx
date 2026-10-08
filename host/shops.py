"""The economy, shop and station catalog, and the stock table the guest links.

P17 wants purchase transactions, shop catalogs and their unlock conditions taken
from the installed game rather than assumed, and the source spreads one purchase
across four places. `ShopMenuStock` on the shop's menu object holds the stock and
picks between a base and an alternate list. `ShopItemStats` on each stock entry
holds the gate, the delivered PlayerData and the special-type code. The shop's
`Confirm Control` PlayMaker FSM is the transaction: it sets the bool, calls
`HeroController.TakeGeo` and then runs one branch per special type. The price is
the trap: `ShopItemStats::Awake` overwrites the serialized `cost` with
`int.Parse(Language.Get(priceConvo, "Prices"))`, so the shipped price lives in an
encrypted language sheet and the serialized field is stale editor data for most
of Sly's stock. Both are reported here and the mismatch is called out.

Stations work the same way. The bell reads its toll from the same `Prices` sheet,
the `Text YN` dialogue box takes the Geo, and `Stag Control` holds the
destination table while `UI List Stag` in resources.assets holds the per
destination PlayerData gate.

`collect` reports, to .hkpsx/shop-catalog.json. `cook` turns Sly's half of that
report into data/shop.rs, the table `game/src/shop.rs` links: fourteen rows with
their sheet price, their localized name and description wrapped to the panel,
the PlayerData each one reads and writes and the delivery branch it runs. The
three rules a purchase obeys, `BuildItemList`, `CanBuy` and the Defender's Crest
discount in `ShopItemStats::OnEnable`, are read out of the shipped CIL and
asserted rather than transcribed, so an install whose rules moved fails the cook
instead of shipping a guest that quietly obeys a different shop.

Scope note: Sly's shop is a separate scene (`Room_shop`) reached through
Dirtmouth's door_sly, and it is now admitted, so the cooked table is reachable.
The other five shop scenes, the relic dealer, the charm smith, the mapper and
Sly's storeroom among them, are not. See `out_of_reach` in the report.
"""
import json
import mmap
import re
import language
from source import Source, ROOT, dump
from scene import Scene
from focus import action_fields

# The sheet `ShopItemStats::Awake` reparses every price from. `host/language.py`
# owns the key derivation and the decryption for every sheet in the install.
PRICE_SHEET = 'EN_Prices'
GEO_REPORT = ROOT / '.hkpsx/geo-provenance.json'
ADMITTED = ROOT / '.hkpsx/selected-regions.json'
SHOP_TABLE = ROOT / 'data/shop.rs'
CATALOG_REPORT = ROOT / '.hkpsx/shop-catalog.json'
ITEM_CATALOG = ROOT / '.hkpsx/item-catalog.json'
# The shop list reuses the charm panel's box, so it wraps against the same width
# and the same cooked glyph advances the guest draws with.
DESC_WIDTH = 264
DESC_LINES = 6
# `ShopItemStats::OnEnable` discounts while charm 10, Defender's Crest, is worn.
DUNG_CHARM = 10


def _reader(fsm):
    """States, variables and typed accessors, as the spell extraction uses."""
    states = {s['name']: s for s in fsm['states']}
    variables = {v['name']: v['value'] for group in fsm['variables'].values()
                 if isinstance(group, list) for v in group
                 if isinstance(v, dict) and 'name' in v and 'value' in v}

    def scalar(value):
        if isinstance(value, dict) and 'value' in value and 'useVariable' in value:
            return variables.get(value['name']) if value['useVariable'] else value['value']
        return value

    def actions(state, kind, objects=False):
        data = states[state]['actionData']
        return [action_fields(data, i, objects) for i, n in enumerate(data['actionNames'])
                if n.rsplit('.', 1)[-1] == kind and data['actionEnabled'][i]]

    def transition(state, event):
        return next((t['toState'] for t in states[state]['transitions']
                     if t['fsmEvent']['name'] == event), None)

    return states, variables, scalar, actions, transition


def _word(value):
    """A compact field as a literal, or `$Name` when it reads an FSM variable."""
    if isinstance(value, dict) and 'useVariable' in value and 'value' in value:
        return '$' + value['name'] if value['useVariable'] else value['value']
    return value


def _target(field):
    """The variable name an fsmOwnerDefault action points at, if any."""
    inner = field.get('gameObject') if isinstance(field, dict) else None
    if isinstance(inner, dict) and inner.get('useVariable'):
        return inner['name']
    return None


def _call_args(action):
    """The FSM variables a CallMethodProper or SendMessage passes as arguments."""
    return [v['variableName'] for k, v in action.items()
            if k.isdigit() and isinstance(v, dict) and v.get('variableName')]


def _int_switch(action):
    """The (compared value, sent event) pairs of an IntSwitch, in order."""
    values = [v['value'] for k, v in action.items()
              if k.isdigit() and isinstance(v, dict) and 'value' in v and not v['useVariable']]
    events = [v for k, v in action.items() if k.isdigit() and isinstance(v, str)]
    if len(values) != len(events) or not values:
        raise ValueError('IntSwitch no longer decodes to matching value and event arrays')
    return list(zip(values, events))


def _contains(path, needle):
    """Whether a serialized file holds a literal, without loading it into memory."""
    with open(path, 'rb') as handle:
        with mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as view:
            return view.find(needle) >= 0


# Written by every interaction that takes control, so never the point of a sink.
CONTROL_BOOLS = ('disablePause', 'isInvincible', 'atMapPrompt')


def _playerdata_writes(actions, scalar=None):
    """Every PlayerData write an FSM state performs, with its literal or variable."""
    writes = []
    for kind, key in (('SetPlayerDataBool', 'boolName'), ('SetPlayerDataInt', 'intName'),
                      ('IncrementPlayerDataInt', 'intName'), ('PlayerDataIntAdd', 'intName')):
        for a in actions(kind):
            field = _word(a[key])
            if not field:
                continue
            entry = {'action': kind, 'field': field}
            if scalar is not None and field.startswith('$'):
                entry['resolves_to'] = scalar(a[key])
            if 'value' in a:
                entry['value'] = _word(a['value'])
            writes.append(entry)
    return writes


def _state_actions(states, name):
    """The enabled action kinds of one state, so a branch is never summarized blind."""
    data = states[name]['actionData']
    return [n.rsplit('.', 1)[-1] for i, n in enumerate(data['actionNames'])
            if data['actionEnabled'][i]]


class Catalog:
    """One Source, with each scene parsed at most once. Scene parsing is slow."""

    def __init__(self, source):
        self.source = source
        self.parsed = {}
        settings = next(o for o in source.file('globalgamemanagers').objects.values()
                        if o.type.name == 'BuildSettings')
        paths = source.read(settings)['scenes']
        self.files = {p.rsplit('/', 1)[-1][:-len('.unity')]: f'level{i}'
                      for i, p in enumerate(paths)}
        if self.files.get('Town') != 'level7':
            raise ValueError('BuildSettings scene order changed')

    def scene(self, name):
        if name not in self.parsed:
            self.parsed[name] = Scene(self.source, self.files[name])
        return self.parsed[name]

    def object(self, scene, name):
        found = [pid for pid, (kind, tree) in scene.objects.items()
                 if kind == 'GameObject' and tree['m_Name'] == name]
        if len(found) != 1:
            raise LookupError(f'{name} is not a single object in this scene: {found}')
        return found[0]

    def parents(self, scene, gid):
        """The ancestor names of an object, outermost first, as its gate context."""
        chain, current = [], gid
        while current in scene.gos:
            chain.append(scene.gos[current]['m_Name'])
            tid = scene.go_transform.get(current)
            father = scene.transforms[tid]['m_Father']['m_PathID'] if tid else 0
            if not father or father not in scene.transforms:
                break
            current = scene.transforms[father]['m_GameObject']['m_PathID']
        return list(reversed(chain))

    def components(self, scene, gid, kind):
        out = []
        for c in scene.gos[gid]['m_Component']:
            cid = c['component']['m_PathID']
            if cid in scene.objects and scene.objects[cid][0] == kind:
                out.append((cid, scene.objects[cid][1]))
        return out

    def fsm(self, scene, gid, name):
        for _, tree in self.components(scene, gid, 'PlayMakerFSM'):
            if tree['fsm']['name'] == name:
                return tree['fsm']
        raise LookupError(f'no {name} FSM on {scene.gos[gid]["m_Name"]}')


# ------------------------------------------------------- resources.assets pass


def resources(source):
    """One pass for the price sheet and the stag menu gate, both persistent."""
    file = source.file('resources.assets')
    # `host/language.py` owns key derivation and decryption for every sheet, so
    # the key is never written down here and cannot go stale against an install.
    table = {name: int(text) for name, text in language.sheet(source, 'Prices').items()}
    if not table:
        raise ValueError('the decrypted price sheet is empty')
    stag_menu = None
    for o in file.objects.values():
        if o.type.name != 'MonoBehaviour' or source.typename(o) != 'PlayMakerFSM':
            continue
        fsm = source.read(o)['fsm']
        if fsm['name'] == 'UI List Stag':
            if stag_menu is not None:
                raise ValueError('more than one UI List Stag menu in resources.assets')
            stag_menu = fsm
    if stag_menu is None:
        raise LookupError('resources.assets lost the stag menu')
    return table, stag_menu


# --------------------------------------------------------------- Sly's shop


def shop_transaction(catalog, scene):
    """The shared purchase: what is paid, what is written, what each type does."""
    found = [tree['fsm'] for kind, tree in scene.objects.values()
             if kind == 'PlayMakerFSM' and tree['fsm']['name'] == 'Confirm Control']
    if len(found) != 1:
        raise LookupError(f'expected one Confirm Control FSM, found {len(found)}')
    confirm = found[0]
    states, variables, scalar, actions, transition = _reader(confirm)

    pay = actions('Deduct Geo and set PD', 'SetPlayerDataBool')
    take = actions('Deduct Geo and set PD', 'CallMethodProper')
    if len(pay) != 1 or _word(pay[0]['boolName']) != '$PD Bool Name' or pay[0]['value']['value'] is not True:
        raise ValueError('the purchase no longer sets the item bool true')
    take = [a for a in take if scalar(a['methodName']) == 'TakeGeo']
    if len(take) != 1 or _call_args(take[0]) != ['Cost']:
        raise ValueError('the purchase no longer pays HeroController.TakeGeo(Cost)')

    switch = actions('Special Type?', 'IntSwitch')
    if len(switch) != 1 or switch[0]['intVariable']['name'] != 'Special Type':
        raise ValueError('the special type dispatch moved')
    types = {}
    for value, event in _int_switch(switch[0]):
        target = transition('Special Type?', event)
        types[value] = {
            'event': event, 'state': target,
            'playerdata': _playerdata_writes(lambda k, s=target: actions(s, k), scalar)
            if target else [],
            # Mask shards and vessel fragments count themselves inside a prefab
            # this branch spawns, so the action list is reported rather than a
            # guess at what the spawned object does.
            'branch_actions': _state_actions(states, target) if target else [],
        }
    if types[0]['state'] != 'Reset' or types[0]['playerdata']:
        raise ValueError('special type 0 is no longer the plain item branch')
    return {'source': 'Confirm Control',
            'pays': 'HeroController.TakeGeo(Cost)',
            'sets': 'PlayerData bool named by the item (ShopItemStats.playerDataBoolName)',
            'order': ['set the item bool', 'take the Geo', 'thank-you animation',
                      'special type branch'],
            'special_types': types}


def shop_stock(catalog, scene, prices, special_types):
    """The stock lists of the one populated ShopMenuStock in a shop scene."""
    stocks = [(pid, tree) for pid, (kind, tree) in scene.objects.items()
              if kind == 'ShopMenuStock' and (tree['stock'] or tree['stockAlt'])]
    if len(stocks) != 1:
        raise LookupError(f'expected one populated ShopMenuStock, found {len(stocks)}')
    pid, stock = stocks[0]
    lists = {}
    for key in ('stock', 'stockInv', 'stockAlt'):
        items = []
        for ref in stock[key]:
            obj = catalog.source.ref(scene.file, ref)
            tree = catalog.source.read(obj)
            stats = None
            for c in tree['m_Component']:
                component = catalog.source.ref(obj.assets_file, c['component'])
                if catalog.source.typename(component) == 'ShopItemStats':
                    stats = catalog.source.read(component)
            if stats is None:
                raise LookupError(f'{tree["m_Name"]} carries no ShopItemStats')
            price_key = stats['priceConvo']
            if price_key not in prices:
                raise LookupError(f'{price_key} is not in the shipped price sheet')
            special = special_types.get(stats['specialType'])
            if special is None:
                raise ValueError(f'special type {stats["specialType"]} has no branch')
            items.append({
                'object': tree['m_Name'],
                'cost': prices[price_key],
                'price_key': price_key,
                'serialized_cost': stats['cost'],
                'cost_is_serialized': prices[price_key] == stats['cost'],
                'sets': stats['playerDataBoolName'],
                'requires': stats['requiredPlayerDataBool'] or None,
                'removed_by': stats['removalPlayerDataBool'] or None,
                'special_type': stats['specialType'],
                'special_effect': special['event'],
                'also_sets': special['playerdata'],
                'notch_cost_bool': stats['notchCostBool'] or None,
                'charms_required': stats['charmsRequired'],
                'dung_discount': bool(stats['dungDiscount']),
                'name_key': stats['nameConvo'],
                'description_key': stats['descConvo'],
            })
        lists[key] = items
    # ShopMenuStock::Start swaps in stockAlt when either bool is set; UpdateStock
    # rechecks only the first. Both are reported so the swap is not guessed.
    return {'source': catalog.source.sid(scene.file.objects[pid]),
            'owner': catalog.parents(scene, stock['m_GameObject']['m_PathID']),
            'alternate_when': [b for b in (stock['altPlayerDataBool'],
                                           stock['altPlayerDataBoolAlt']) if b],
            'alternate_rule': 'Start uses stockAlt when either bool is true; '
                              'UpdateStock rechecks only the first',
            'lists': lists}


def shop_region(catalog, scene):
    """Where the player stands to open the shop, from `Shop Region`'s own FSM.

    `Shop Region` is the counter-side twin of `npc_control`: `Out Of Range`
    waits on the object's trigger collider, `In Range` raises the `Prompt
    Marker` child through `ShowPromptMarker` and listens for UP, and `Shop Up`
    is what finally opens the menu. So the three things the guest needs, the
    trigger box, the marker point and the label, are all readable rather than
    placed by hand, and a moved counter moves them with it.

    The scene id is the catalog's, so a shop scene that is not admitted refuses
    here instead of cooking a trigger into a scene the guest cannot load.
    """
    from quality import SCENE_TABLE
    found = [(tree['m_GameObject']['m_PathID'], tree['fsm']) for kind, tree
             in scene.objects.values()
             if kind == 'PlayMakerFSM' and tree['fsm']['name'] == 'Shop Region']
    if len(found) != 1:
        raise LookupError(f'expected one Shop Region FSM, found {len(found)}')
    gid, fsm = found[0]
    if not scene.active(gid):
        raise ValueError('the Shop Region object starts inactive')
    states = {state['name']: state for state in fsm['states']}
    label = None
    for name in ('In Range',):
        data = states[name]['actionData']
        for i, action in enumerate(data['actionNames']):
            if data['actionEnabled'][i] and action.rsplit('.', 1)[-1] == 'ShowPromptMarker':
                label = _word(action_fields(data, i)['labelName'])
    if not label:
        raise LookupError('Shop Region raises no prompt label')
    boxes = [tree for kind, tree in scene.objects.values()
             if kind == 'BoxCollider2D' and tree['m_GameObject']['m_PathID'] == gid
             and tree['m_Enabled'] and tree['m_IsTrigger']]
    if len(boxes) != 1:
        raise ValueError(f'Shop Region has {len(boxes)} triggers')
    off, size = boxes[0]['m_Offset'], boxes[0]['m_Size']
    corners = [scene.point(gid, x + off['x'], y + off['y'])[:2] for x, y in
               ((-size['x'] / 2, -size['y'] / 2), (size['x'] / 2, size['y'] / 2))]
    marker = [pid for pid, (kind, tree) in scene.objects.items()
              if kind == 'GameObject' and tree['m_Name'] == 'Prompt Marker'
              and scene.transforms[scene.transforms[scene.go_transform[pid]]
                                   ['m_Father']['m_PathID']]['m_GameObject']['m_PathID'] == gid]
    if len(marker) != 1:
        raise LookupError('Shop Region has no single Prompt Marker child')
    point = scene.point(marker[0])
    scene_name = next(name for name, parsed in catalog.parsed.items() if parsed is scene)
    admitted = [row for row in SCENE_TABLE if row['scene_name'] == scene_name]
    if len(admitted) != 1:
        raise LookupError(f'{scene_name} is not in the cooked scene catalog')
    return {
        'scene': scene_name, 'scene_id': admitted[0]['scene_id'], 'prompt': label,
        'bounds': [min(corners[0][0], corners[1][0]), min(corners[0][1], corners[1][1]),
                   max(corners[0][0], corners[1][0]), max(corners[0][1], corners[1][1])],
        'marker': [point[0], point[1]],
        'note': 'Trigger, prompt marker and label only. The hero alignment walk, '
                'the turn, the intro conversation and the shop window art that '
                'Shop Region also drives are not cooked.',
    }


def sly_shop(catalog, prices):
    town = catalog.scene('Town')
    building = catalog.object(town, 'Sly_shop')
    gate = _reader(catalog.fsm(town, building, 'Check Opened'))[3]('Init', 'PlayerDataBoolTest')
    if len(gate) != 1:
        raise ValueError('the Sly_shop building no longer has one open/closed gate')
    door_gid = catalog.object(town, 'door_sly')
    door = catalog.fsm(town, door_gid, 'Door Control')
    _, door_vars, _, _, _ = _reader(door)
    destination = door_vars['New Scene']
    if destination not in catalog.files:
        raise LookupError(f'{destination} is not a build scene')
    shop = catalog.scene(destination)
    transaction = shop_transaction(catalog, shop)
    stock = shop_stock(catalog, shop, prices, transaction['special_types'])
    return {
        'shop_scene': destination,
        'shop_file': catalog.files[destination],
        'region': shop_region(catalog, shop),
        'town_building': {'object': 'Sly_shop',
                          'visible_when': _word(gate[0]['boolName']),
                          'swaps': 'Check Opened enables the open or the closed child'},
        'town_door': {'object': 'door_sly',
                      'parents': catalog.parents(town, door_gid),
                      'to_scene': destination,
                      'entry_gate': door_vars['Entry Gate'],
                      'gated_by': _word(gate[0]['boolName']),
                      'gate_note': 'the door is a child of Sly_shop/open, which '
                                   'Check Opened deactivates while the bool is false'},
        'transaction': transaction,
        'stock': stock,
    }


# ------------------------------------------------------------- the stag station


def stag_menu_gates(fsm):
    """Destination to PlayerData bool, from the menu that hides unopened stops.

    The menu holds each row in a variable that Init binds to a named child, so
    the variable is translated back to the row name the station FSM sends.
    """
    states, _, _, actions, _ = _reader(fsm)
    rows = {}
    for bind in actions('Init', 'FindChild', objects=True):
        stored = _word(bind.get('storeResult'))
        if isinstance(stored, str) and stored.startswith('$'):
            rows[stored[1:]] = _word(bind['childName'])
    gates = {}
    for name in states:
        tests = actions(name, 'PlayerDataBoolTest')
        hides = [a for a in actions(name, 'ActivateGameObject')
                 if a['activate']['value'] is False and _target(a['gameObject'])]
        if len(tests) != 1 or not hides:
            continue
        variable = _target(hides[0]['gameObject'])
        gates[rows.get(variable, variable)] = _word(tests[0]['boolName'])
    if not gates:
        raise ValueError('the stag menu no longer gates its destinations')
    return sorted(rows.values()), gates


def stag_station(catalog, prices, menu_fsm):
    town = catalog.scene('Town')
    station = catalog.object(town, 'Stag_station')
    building = _reader(catalog.fsm(town, station, 'Check Opened'))[3]('Init', 'PlayerDataBoolTest')
    if len(building) != 1:
        raise ValueError('the Town station building no longer has one gate')
    door_gid = catalog.object(town, 'door_station')
    _, door_vars, _, _, _ = _reader(catalog.fsm(town, door_gid, 'Door Control'))

    # The only station inside the admitted scenes is the Crossroads one.
    crossroads = catalog.scene('Crossroads_47')
    bell_gid = catalog.object(crossroads, 'Station Bell')
    bell = catalog.fsm(crossroads, bell_gid, 'Stag Bell')
    _, bell_vars, bell_scalar, bell_actions, _ = _reader(bell)
    lookup = bell_actions('Get Price', 'GetLanguageString')
    if len(lookup) != 1 or bell_scalar(lookup[0]['sheetName']) != 'Prices':
        raise ValueError('the bell no longer reads its toll from the Prices sheet')
    price_key = bell_scalar(lookup[0]['convName'])
    if price_key not in prices:
        raise LookupError(f'{price_key} is not in the shipped price sheet')
    opened = bell_vars['PlayerData Bool']
    yes = _playerdata_writes(lambda kind: bell_actions('Yes', kind), bell_scalar)
    if not any(w['field'] == '$PlayerData Bool' for w in yes):
        raise ValueError('paying the bell no longer opens the station')
    toll = bell_actions('Send Text', 'SetFsmInt')
    if len(toll) != 1 or _word(toll[0]['variableName']) != 'Toll Cost':
        raise ValueError('the bell no longer hands its toll to the dialogue box')

    control = catalog.fsm(crossroads, catalog.object(crossroads, 'Stag'), 'Stag Control')
    states, variables, scalar, actions, _ = _reader(control)
    for name in states:
        for call in actions(name, 'CallMethodProper'):
            if scalar(call.get('methodName')) in ('TakeGeo', 'AddGeo'):
                raise ValueError('Stag Control now moves Geo; travel is not free')
    destinations = []
    for t in states['Check Result']['transitions']:
        event = t['fsmEvent']['name']
        state = t['toState']
        if not state:
            destinations.append({'choice': event, 'state': None, 'scene': None,
                                 'position': None, 'note': 'offered by the menu, '
                                 'unbound in this FSM'})
            continue
        scene_set = actions(state, 'SetStringValue')
        position = actions(state, 'SetIntValue')
        if len(scene_set) != 1 or len(position) != 1:
            raise ValueError(f'the {event} destination no longer sets one scene and position')
        destinations.append({'choice': event, 'state': state,
                             'scene': scalar(scene_set[0]['stringValue']),
                             'position': scalar(position[0]['intValue'])})
    rows, gates = stag_menu_gates(menu_fsm)
    # The menu also binds its knight marker; keep only rows the station travels to.
    choices = {d['choice'] for d in destinations}
    rows = [r for r in rows if r in choices or r in gates]
    for destination in destinations:
        destination['menu_row'] = destination['choice'] in rows
        destination['listed_when'] = gates.get(destination['choice'])
    return {
        'town_building': {'object': 'Stag_station',
                          'visible_when': _word(building[0]['boolName']),
                          'door': {'object': 'door_station',
                                   'parents': catalog.parents(town, door_gid),
                                   'to_scene': door_vars['New Scene'],
                                   'entry_gate': door_vars['Entry Gate']}},
        'admitted_station': {
            'scene': 'Crossroads_47',
            'bell': 'Station Bell',
            'unlock_bool': opened,
            'toll': prices[price_key],
            'toll_price_key': price_key,
            'serialized_toll': bell_vars['Toll Cost'],
            'toll_is_serialized': prices[price_key] == bell_vars['Toll Cost'],
            'paid_by': 'Text YN / Dialogue Page Control calls TakeGeo(Toll Cost) '
                       'on YES, then returns YES to the bell',
            'on_paid': yes,
            'station_position': variables['Station Position Number'],
        },
        'travel_cost': 0,
        'travel_cost_evidence': 'no TakeGeo or AddGeo call anywhere in Stag Control',
        'travel_writes': ['stagPosition = the destination position',
                          'nextScene = the destination scene',
                          'travelling = true, then Cinematic_Stag_travel is loaded'],
        'destinations': destinations,
        'destination_gates': gates,
        'menu_rows': rows,
        'ungated_destinations': [d['choice'] for d in destinations
                                 if d['menu_row'] and not d['listed_when']],
        'dead_choices': [d['choice'] for d in destinations if not d['menu_row']],
    }


# -------------------------------------------------------------- Geo in the slice


def admitted_scenes():
    """The scene table, not the last build's snapshot of it.

    `.hkpsx/selected-regions.json` is written by the guest build, which runs
    after this script, so on the build that admits a scene it still holds the
    previous run's set. Reading it here made the Geo economy refuse a rock in
    the scene the same build had just cooked, reporting it as not admitted.
    `host/quality.py`'s SCENE_TABLE is what admission means, and this module
    already reads it elsewhere.
    """
    from quality import SCENE_TABLE
    return {row['file']: row['scene_name'] for row in SCENE_TABLE}


def geo_sources(admitted):
    """Rocks and enemy payouts, from the Geo cook's own source-backed report."""
    if not GEO_REPORT.is_file():
        raise FileNotFoundError('run host/geo.py first; its report is the rock and '
                                'enemy payout evidence')
    report = json.loads(GEO_REPORT.read_text())
    if report['format'] != 'HKGEO01':
        raise ValueError('the Geo report format changed')
    denominations = [c['value'] for c in report['coins']]
    if denominations != [1, 5, 25]:
        raise ValueError('Geo denominations changed')
    per_scene = {}
    for rock in report['rocks']:
        file = rock['source'].split(':')[0]
        if file not in admitted:
            raise ValueError(f'the Geo report covers {file}, which is not admitted')
        entry = per_scene.setdefault(admitted[file], {'rock_geo': 0, 'rocks': 0,
                                                      'enemy_geo': 0, 'enemies': 0})
        entry['rock_geo'] += rock['total']
        entry['rocks'] += 1
    for enemy in report['enemies']:
        file = enemy['source'].split(':')[0]
        if file not in admitted:
            raise ValueError(f'the Geo report covers {file}, which is not admitted')
        entry = per_scene.setdefault(admitted[file], {'rock_geo': 0, 'rocks': 0,
                                                      'enemy_geo': 0, 'enemies': 0})
        value = sum(count * coin for count, coin in zip(enemy['drops'], denominations))
        entry['enemy_geo'] += value
        entry['enemies'] += 1
    uncooked = [{'scene': admitted[r['source'].split(':')[0]], 'source': r['source'],
                 'reason': r['reason']} for r in report['unsupported_rocks']]
    return {
        'denominations': denominations,
        'rock_total': sum(s['rock_geo'] for s in per_scene.values()),
        'rock_count': sum(s['rocks'] for s in per_scene.values()),
        'enemy_total': sum(s['enemy_geo'] for s in per_scene.values()),
        'enemy_count': sum(s['enemies'] for s in per_scene.values()),
        'per_scene': dict(sorted(per_scene.items())),
        'uncooked_rocks': uncooked,
        'evidence': str(GEO_REPORT.relative_to(ROOT)),
    }


def geo_sinks(catalog, prices, admitted):
    """Everything in an admitted scene that reads the Prices sheet and takes Geo.

    A scene that never mentions the sheet cannot charge a sheet price, so the
    byte scan is the proof that the parsed scenes are the complete set.
    """
    candidates = sorted(name for file, name in admitted.items()
                        if _contains(catalog.source.directory / file, b'Prices'))
    sinks = []
    for name in candidates:
        scene = catalog.scene(name)
        for kind, tree in scene.objects.values():
            if kind != 'PlayMakerFSM':
                continue
            fsm = tree['fsm']
            states, variables, scalar, actions, _ = _reader(fsm)
            for state in states:
                lookups = [a for a in actions(state, 'GetLanguageString')
                           if scalar(a['sheetName']) == 'Prices']
                if not lookups:
                    continue
                convo = lookups[0]['convName']
                key = scalar(convo)
                if key not in prices and convo.get('useVariable'):
                    # Cornifer builds his key as 'MAP_' plus his area rather than
                    # storing it, so the variable's serialized value is only half.
                    built = [b for b in actions(state, 'BuildString')
                             if _word(b['storeResult']) == '$' + convo['name']]
                    if len(built) == 1:
                        key = ''.join(str(scalar(v)) for k, v in built[0].items()
                                      if k.isdigit())
                if key not in prices:
                    raise LookupError(f'{name} asks the price sheet for {key!r}')
                scale = [scalar(a['multiplyBy']) for a in actions(state, 'FloatMultiply')]
                if len(scale) > 1:
                    raise ValueError(f'{name} scales its price more than once')
                factor = scale[0] if scale else 1.0
                writes, seen = [], set()
                for other in states:
                    for write in _playerdata_writes(lambda k, st=other: actions(st, k),
                                                   scalar):
                        mark = json.dumps(write, sort_keys=True)
                        if write['field'] in CONTROL_BOOLS or mark in seen:
                            continue
                        seen.add(mark)
                        writes.append(write)
                sinks.append({
                    'scene': name,
                    'object': scene.gos[tree['m_GameObject']['m_PathID']]['m_Name'],
                    'fsm': fsm['name'],
                    'price_key': key,
                    'sheet_price': prices[key],
                    'multiplier': factor,
                    'cost': int(prices[key] * factor),
                    'serialized_toll': variables.get('Toll Cost'),
                    'playerdata_writes': writes,
                })
    if not sinks:
        raise ValueError('no admitted scene spends Geo; the scan is wrong')
    return sinks


# ------------------------------------------------- the rules a purchase obeys


def _bodies(types, assembly='Assembly-CSharp.dll'):
    """`{(type, method): [opcode names]}` for the shipped CIL of whole types."""
    from inspect_il import inspect
    text = inspect(language.managed_directory() / assembly, set(types))
    bodies = {}
    for block in text.split('\n\n'):
        head, _, rest = block.strip().partition('\n')
        match = re.fullmatch(r'(\w+)::(\S+) RVA=[0-9a-f]+', head)
        if not match:
            continue
        bodies[(match.group(1), match.group(2))] = [
            line.split(None, 2)[1:] for line in rest.splitlines() if line.strip()]
    return bodies


def _reads(body, kinds=('ldfld', 'callvirt', 'call', 'ldstr')):
    """The named operands one method body touches, as a set.

    `inspect_il` prints a user string as its repr, so the quotes come off here
    and a field name and the literal it is compared against read the same way.
    """
    names = set()
    for opcode, operand in (entry for entry in body if len(entry) == 2):
        if opcode not in kinds:
            continue
        names.add(operand[1:-1] if operand[:1] == "'" and operand[-1:] == "'" else operand)
    return names


def purchase_rules():
    """The three rules a purchase obeys, checked against the shipped CIL.

    `ShopMenuStock::BuildItemList` decides what is on the shelf, `CanBuy`
    decides what can be afforded, and `ShopItemStats::OnEnable` is the only
    price modifier in the shop. Each one is asserted by the fields it reads, so
    an install whose rules moved fails the cook instead of shipping a guest that
    quietly obeys a different shop.
    """
    bodies = _bodies(('ShopMenuStock', 'ShopItemStats'))
    listing = _reads(bodies[('ShopMenuStock', 'BuildItemList')])
    wanted = {'requiredPlayerDataBool', 'playerDataBoolName', 'removalPlayerDataBool', 'GetBool'}
    missing = wanted - listing
    if missing:
        raise ValueError(f'BuildItemList no longer reads {missing}')
    buying = _reads(bodies[('ShopMenuStock', 'CanBuy')])
    if not {'relicNumber', 'geo', 'charmsOwned', 'charmsRequired'} <= buying:
        raise ValueError('CanBuy no longer weighs relic, Geo and charm count')
    enable = bodies[('ShopItemStats', 'OnEnable')]
    fields = _reads(enable)
    if not {'dungDiscount', f'equippedCharm_{DUNG_CHARM}', 'runningCost'} <= fields:
        raise ValueError('the shop discount is no longer Defender\'s Crest on runningCost')
    scale = [float(operand) for opcode, operand in enable
             if opcode == 'ldc.r4' and 0 < float(operand) < 1]
    if len(scale) != 1 or abs(scale[0] - 0.8) > 1e-6:
        raise ValueError(f'the shop discount factor is now {scale}')
    awake = _reads(bodies[('ShopItemStats', 'Awake')], ('ldstr', 'ldfld', 'stfld'))
    if not {'Prices', 'priceConvo', 'cost'} <= awake:
        raise ValueError('Awake no longer reparses the cost from the Prices sheet')
    return {
        'listing': 'BuildItemList lists an item when its playerDataBoolName is unset, its '
                   'requiredPlayerDataBool is empty or set, and its removalPlayerDataBool is '
                   'empty or unset',
        'affording': 'CanBuy passes a relic sale outright, else needs geo >= GetCost() and '
                     'charmsOwned >= charmsRequired',
        'discount_charm': DUNG_CHARM,
        'discount_factor': scale[0],
        'discount_rule': 'OnEnable sets runningCost = (int)(cost * 0.8f) while charm 10 is '
                         'equipped and dungDiscount is set; the cast truncates',
        'price_source': 'Awake overwrites the serialized cost with int.Parse(Language.Get('
                        'priceConvo, "Prices"))',
    }


# ------------------------------------------------------------- the cooked table


SPECIAL_DELIVERY = {
    0: ('Item', []),
    1: ('MaskShard', []),
    2: ('Charm', [('SetPlayerDataBool', 'hasCharm'), ('IncrementPlayerDataInt', 'charmsOwned')]),
    3: ('VesselFragment', []),
    10: ('SimpleKey', [('IncrementPlayerDataInt', 'simpleKeys')]),
    11: ('RancidEgg', [('IncrementPlayerDataInt', 'rancidEggs')]),
}


def _discounted(cost, factor=0.8):
    """`(int)(cost * 0.8f)`, and the integer identity the guest uses instead.

    The guest has no floats, so it divides. The two agree for every shipped
    price because 0.8f rounds up, so the product only ever lands just above the
    exact fifth rather than just below it; the cook checks that rather than
    assuming it.
    """
    from struct import pack, unpack
    single = unpack('<f', pack('<f', factor))[0]
    truncated = int(cost * single)
    if truncated != cost * 4 // 5:
        raise ValueError(f'{cost} discounts to {truncated}, not {cost * 4 // 5}')
    return truncated


def shop_rows(report, ui, advances):
    """Sly's stock as cooked guest rows, with the flags they read and write.

    The port has no general PlayerData store, so every non-charm bool the shop
    reads gets a slot in a bitmask this module owns. `gotCharm_N` is the one
    exception: `game/src/charms.rs` already holds that set, so a charm row
    points at the charm rather than at a second copy of its bool.
    """
    from read_points import wrap_page
    stock = report['sly_shop']['stock']
    special_types = report['sly_shop']['transaction']['special_types']
    fragments = json.loads(ITEM_CATALOG.read_text())['fragments']
    order, rows = [], {}
    slots = []

    def flag(name):
        """A PlayerData bool as the guest reads it: a charm, or a cooked slot."""
        charm = re.fullmatch(r'gotCharm_(\d+)', name)
        if charm:
            return f'Flag::Charm({int(charm.group(1))})'
        if name not in slots:
            slots.append(name)
        return f'Flag::Slot({slots.index(name)})'

    for name in stock['alternate_when']:
        flag(name)
    for key in ('stock', 'stockAlt'):
        for item in stock['lists'][key]:
            if item['object'] in rows:
                continue
            order.append(item['object'])
            rows[item['object']] = item
    cooked = []
    for name in order:
        item = rows[name]
        kind, writes = SPECIAL_DELIVERY.get(item['special_type'], (None, None))
        if kind is None:
            raise ValueError(f'{name} has unsupported special type {item["special_type"]}')
        branch = [(w['action'], w['field']) for w in
                  special_types[str(item['special_type'])]['playerdata']]
        if branch != writes:
            raise ValueError(f'special type {item["special_type"]} now writes {branch}')
        if item['charms_required'] and kind != 'Charm':
            raise ValueError(f'{name} gates on charms without being one')
        if kind == 'Charm':
            charm = re.fullmatch(r'gotCharm_(\d+)', item['sets'])
            if not charm or item['notch_cost_bool'] != f'charmCost_{charm.group(1)}':
                raise ValueError(f'{name} is a charm row whose charm cannot be named')
            delivery = f'Delivery::Charm({int(charm.group(1))})'
        else:
            delivery = f'Delivery::{kind}'
        title = ui[item['name_key']].replace('’', "'").replace('‘', "'")
        if not all(32 <= ord(c) <= 126 for c in title):
            raise ValueError(f'{name} has an undrawable title {title!r}')
        pages = wrap_page(ui[item['description_key']], advances, DESC_WIDTH, DESC_LINES)
        if len(pages) != 1:
            raise ValueError(f'{name} needs {len(pages)} description panels')
        cooked.append({
            'object': name, 'name': title, 'lines': pages[0], 'cost': item['cost'],
            'discounted': _discounted(item['cost']),
            'sets': flag(item['sets']),
            'requires': flag(item['requires']) if item['requires'] else 'None',
            'removed_by': flag(item['removed_by']) if item['removed_by'] else 'None',
            'delivery': delivery, 'charms_required': item['charms_required'],
            'dung_discount': bool(item['dung_discount']),
            'price_key': item['price_key'], 'sets_field': item['sets'],
        })
    lists = {key: [order.index(i['object']) for i in stock['lists'][key]]
             for key in ('stock', 'stockAlt')}
    return {
        'items': cooked, 'slots': slots, 'lists': lists,
        'region': report['sly_shop']['region'],
        'alternate_when': [flag(n) for n in stock['alternate_when']],
        'mask_shards_per_mask': fragments['mask_shard']['fuse']['per_whole'],
        'mask_cap': fragments['mask_shard']['mask_cap'],
        'starting_masks': fragments['mask_shard']['starting_masks'],
        'vessel_fragments_per_vessel': fragments['vessel_fragment']['fuse']['per_whole'],
        'soul_per_vessel': fragments['vessel_fragment']['fuse']['awards'][0]['amount'],
    }


def _q16(value):
    """World units as the guest reads them, the same Q16 host/world.py emits."""
    if not -512 < value < 512:
        raise ValueError('shop world coordinate outside Q16 bounds')
    return round(value * 65536)


def generated_shop_rs(table, rules):
    """The guest table. `Item`, `Flag` and `Delivery` live in game/src/shop.rs."""
    def quoted(value):
        if '"' in value or '\\' in value:
            raise ValueError(f'unquotable cooked text {value!r}')
        return '"' + value + '"'
    if len(table['slots']) > 32:
        raise ValueError(f'{len(table["slots"])} shop bools exceed the cooked bitmask')
    entries = []
    for row in table['items']:
        lines = ','.join(quoted(line) for line in row['lines'])
        entries.append(
            'Item{name:%s,lines:&[%s],cost:%d,sets:%s,requires:%s,removed_by:%s,'
            'delivery:%s,charms_required:%d,dung_discount:%s}' % (
                quoted(row['name']), lines, row['cost'], row['sets'],
                f"Some({row['requires']})" if row['requires'] != 'None' else 'None',
                f"Some({row['removed_by']})" if row['removed_by'] != 'None' else 'None',
                row['delivery'], row['charms_required'],
                'true' if row['dung_discount'] else 'false'))
    array = lambda values: '&[' + ','.join(str(v) for v in values) + ']'
    region = table['region']
    return '\n'.join([
        "// Generated Sly's shop; prices and text from the installed language sheets.",
        f"/// `Shop Region` in {region['scene']}: the strip of floor in front of the",
        '/// counter its `Out Of Range`/`In Range` pair watches, the `Prompt Marker`',
        '/// child `ShowPromptMarker` raises over it and the label it raises. Q16 world',
        '/// units, the same units world::Gate and the NPC talk triggers use.',
        f"pub const SHOP_SCENE:usize={region['scene_id']};",
        'pub const SHOP_REGION:[i32;4]=[' + ','.join(str(_q16(v)) for v in region['bounds']) + '];',
        'pub const SHOP_MARKER:[i32;2]=[' + ','.join(str(_q16(v)) for v in region['marker']) + '];',
        f"pub const SHOP_PROMPT:&str={quoted(region['prompt'])};",
        f"pub const ITEM_COUNT:usize={len(entries)};",
        '/// The PlayerData bools the shop reads that are not `gotCharm_N`, in the',
        '/// order `Flag::Slot` indexes them.',
        f"pub const FLAG_COUNT:usize={len(table['slots'])};",
        '/// Their source names, which only the tests and the report need, so the',
        '/// guest never links a quarter of a kilobyte of strings it cannot draw.',
        f"#[cfg(test)] pub const FLAG_NAMES:[&str;FLAG_COUNT]=[" +
        ','.join(quoted(name) for name in table['slots']) + '];',
        '/// `ShopMenuStock::Start` swaps in the alternate list when either bool is set.',
        'pub const ALTERNATE_WHEN:&[Flag]=&[' + ','.join(table['alternate_when']) + '];',
        '/// Source stock order; the alternate list is what Sly sells once he has his key.',
        'pub const BASE_STOCK:&[u8]=' + array(table['lists']['stock']) + ';',
        'pub const ALTERNATE_STOCK:&[u8]=' + array(table['lists']['stockAlt']) + ';',
        f"/// `{rules['discount_rule']}`.",
        f"pub const DISCOUNT_CHARM:usize={rules['discount_charm']};",
        '/// `Heart Container Control` fuses this many shards into one mask.',
        f"pub const SHARDS_PER_MASK:u8={table['mask_shards_per_mask']};",
        f"pub const MASK_CAP:u8={table['mask_cap']};",
        f"pub const STARTING_MASKS:u8={table['starting_masks']};",
        '/// `Vessel Fragment Control` fuses this many fragments into one vessel.',
        f"pub const FRAGMENTS_PER_VESSEL:u8={table['vessel_fragments_per_vessel']};",
        f"pub const SOUL_PER_VESSEL:u16={table['soul_per_vessel']};",
        f'pub const ITEMS:[Item;ITEM_COUNT]=[' + ','.join(entries) + '];',
    ]) + '\n'


def cook():
    """Write data/shop.rs from the catalog report, the price sheet and the CIL."""
    from charms import advances
    if not CATALOG_REPORT.is_file():
        raise FileNotFoundError('run host/shops.py collect first; the catalog is its report')
    report = json.loads(CATALOG_REPORT.read_text())
    if report['format'] != 'HKSHOP01':
        raise ValueError('the shop catalog format changed')
    rules = purchase_rules()
    ui = language.sheet(Source(), 'UI')
    table = shop_rows(report, ui, advances())
    SHOP_TABLE.write_text(generated_shop_rs(table, rules))
    dump(ROOT / '.hkpsx/shop-table.json', {'format': 'HKSHOPTABLE01', 'rules': rules, **table})
    return table, rules


# ------------------------------------------------------------------- the report


def out_of_reach(catalog, admitted):
    """Shops, stations and services P17 owns that the admitted slice cannot show."""
    shops = []
    for name, file in sorted(catalog.files.items()):
        path = catalog.source.directory / file
        if path.is_file() and _contains(path, b'ShopMenuStock'):
            shops.append({'scene': name, 'file': file, 'admitted': file in admitted})
    if not shops:
        raise LookupError('no scene carries a ShopMenuStock')
    return shops


def admission_cost(name):
    """What admitting one shop scene would cost, from its measured capacity pack.

    `tools/cook_scene_pack.py` already cooked every BuildSettings scene into
    `.hkpsx/scene-packs`, under a write guard, so the budget answer for a
    candidate scene is a measurement that exists rather than an estimate. The
    numbers that decide it are the per-view ones: the 416-slot CLUT budget is
    per view and is the binding constraint on this port.
    """
    root = ROOT / '.hkpsx/scene-packs' / name
    if not (root / 'summary.json').is_file():
        return {'scene': name, 'measured': False,
                'reason': f'cook it first: tools/cook_scene_pack.py {name}'}
    summary = json.loads((root / 'summary.json').read_text())
    if summary.get('status') != 'cooked':
        return {'scene': name, 'measured': True, 'status': summary.get('status'),
                'error': summary.get('error')}
    from pathlib import Path
    from quality import TEXTURE_BUDGET, STATIC_PAGE_BUDGET, ROOM_BYTE_BUDGET
    from cook import MAX_ROOM_TEXTURES
    from texture_dedup import clut_count
    views = json.loads((root / 'regions.json').read_text())['regions']
    # TEXTURE_BUDGET counts distinct palettes, so the number reported against it
    # has to be the palette count. Reporting the texture-record count beside it
    # is how Room_shop's 87 slots were written into host/quality.py as 90. A
    # pack cooked before cook_scene_pack recorded `cluts` is measured, not
    # assumed, the same way tools/texture_headroom.py does it.
    cluts = {v['chunk_id']: v['cluts'] if 'cluts' in v else clut_count(Path(v['path']).read_bytes())
             for v in views}
    worst = max(views, key=lambda v: cluts[v['chunk_id']])
    return {
        'scene': name, 'measured': True, 'status': 'cooked',
        'file': summary['file'], 'views': len(views),
        'runtime_bounds': summary['envelope'],
        'camera_global_bounds': summary['camera_global_bounds'],
        'worst_view_cluts': cluts[worst['chunk_id']], 'texture_budget': TEXTURE_BUDGET,
        'worst_view_textures': max(v['textures'] for v in views),
        'texture_table_limit': MAX_ROOM_TEXTURES,
        'worst_view_pages': max(v['pages'] for v in views), 'page_budget': STATIC_PAGE_BUDGET,
        'worst_view_bytes': max(v['bytes'] for v in views), 'room_byte_budget': ROOM_BYTE_BUDGET,
        'scene_bank_bytes': summary['scene_bank_bytes'],
        'resident_bytes': summary['resident_bytes'],
        'decoder': summary['decoder_status'], 'packet_bound': summary['geometry_packet_bound'],
        'view_splits': summary['view_splits'],
        'gates': [{'name': g['name'], 'target_scene': g['target_scene'],
                   'entry_point': g['entry_point']}
                  for g in json.loads((root / 'regions.json').read_text())['scenes'][0]['gates']],
        'cook_seconds': summary['elapsed_seconds'],
    }


def collect():
    source = Source()
    catalog = Catalog(source)
    prices, menu = resources(source)
    admitted = admitted_scenes()
    sly = sly_shop(catalog, prices)
    stag = stag_station(catalog, prices, menu)
    sinks = geo_sinks(catalog, prices, admitted)
    sources = geo_sources(admitted)
    shops = out_of_reach(catalog, admitted)
    unreachable = [d for d in stag['destinations']
                   if d['scene'] and d['scene'] not in admitted.values()]
    report = {
        'format': 'HKSHOP01',
        'price_sheet': PRICE_SHEET,
        'price_sheet_entries': len(prices),
        'admitted_scenes': len(admitted),
        'scenes_parsed': sorted(catalog.parsed),
        'sly_shop': sly,
        'stag_station': stag,
        'geo_sources': sources,
        'geo_sinks': sinks,
        'out_of_reach': {
            'shop_scenes': shops,
            'admitted_shop_scenes': [s['scene'] for s in shops if s['admitted']],
            'stag_destinations_outside_slice': [d['choice'] for d in unreachable],
            'admission_cost': [admission_cost(s['scene']) for s in shops if not s['admitted']],
            # None on a fresh save: door_sly is a child of Sly_shop/open, which
            # Dirtmouth's `Check Opened` turns off until slyRescued, and the
            # rescue room is not on the disc (host/activation.py).
            'town_shop_door': next((g for g in
                                    next(s for s in json.loads(ADMITTED.read_text())['scenes']
                                         if s['scene_name'] == 'Town')['gates']
                                    if g['name'] == 'door_sly'), None),
            'notes': [
                'Room_shop is admitted, but on a fresh save its way in is shut: '
                'Dirtmouth\'s Check Opened hides Sly_shop/open, door_sly with it, until '
                'slyRescued, which Room_ruinhouse sets and which is not on the disc.',
                'Its way in is Dirtmouth\'s door_sly, whose serialized targetScene is '
                'empty because the Door Control FSM holds the destination (Room_shop, '
                'entry gate left1). regions.door_destination reads that FSM, so the '
                'gate cooks, and Room_shop\'s own left1 targets Town/door_sly for the '
                'way back. Both directions are in data/regions.rs.',
                'The only station in the slice is Crossroads_47. Its toll can be paid '
                'and the station opened, but the only destination inside the slice is '
                'Crossroads_47 itself, which Current Location Check cancels. Every '
                'stop that would move the player, Dirtmouth included, is outside it.',
                'Nail upgrades, charm notches, charm repair, the relic dealer, map and '
                'quill and pin purchases and the bank all live in unadmitted scenes. '
                'Cornifer selling the Crossroads map is the one map purchase in reach.',
                'Charms can be delivered: the charm branch writes the same gotCharm_N '
                'bool game/src/charms.rs owns, so a cooked charm row hands the charm '
                'straight to the inventory the pause screen equips from.',
                'The stag menu never hides Dirtmouth, so a player who opens the '
                'Crossroads station is offered a stop the slice does not contain. '
                'Forest Of Bones and Palace Grounds are dead events on Stag Control: '
                'no menu row and no scene.',
            ],
        },
        'limitations': [
            'The purchase is reachable: Room_shop is admitted, data/shop.rs carries '
            'the stock and the Shop Region trigger, and game/src/shop.rs runs the '
            'transaction from it. What the source also runs from that trigger and '
            'this does not: the hero alignment walk and turn, Sly\'s intro '
            'conversation, the shop window art and the confirm sub-window.',
            'The stag station, Cornifer and the City lift are catalogued only. Their '
            'Geo is still taken by no code in the port.',
            'Prices come from the decrypted EN_Prices sheet because ShopItemStats::Awake '
            'overwrites the serialized cost with it. Only the English sheet is read; '
            'the other eleven language sheets are not checked for disagreement.',
            'The Defender\'s Crest discount is implemented and unreachable: no row in '
            'Sly\'s stock sets dungDiscount, so only the relic dealer would use it.',
            'Shop UI layout and icons are not extracted. The cooked rows carry text '
            'and price and the guest draws them in the shared dialogue panel, which '
            'is the port\'s own layout rather than the source\'s scrolling list.',
            'Relic selling (the Selling Shop branch of Confirm Control) is recorded '
            'as a branch but not extracted: no relic dealer scene is admitted.',
            'Mask shard and vessel fragment counting happens inside a prefab the '
            'branch spawns, not in the branch, so the guest counts them from the '
            'fuse rules in the item catalog rather than from the branch itself.',
            'A purchase survives a quit: save::Save carries the shop bools and the '
            'counters as of HKS4, and shop::State::restore refuses a record that '
            'contradicts itself rather than half-decoding it. What it does not '
            'carry is a reserve vessel on the HUD: the fuse raises the SOUL ceiling '
            'and the orb is drawn against the source\'s own 99, so a reserve reads '
            'as a full vessel with no vessels of its own beside it.',
            'Cornifer scales his sheet price by 0.75 and converts back to an int. '
            'The Crossroads product is exact, so which way PlayMaker rounds a '
            'fractional map price is untested here.',
            'The Geo source totals come from the Geo cook report rather than a '
            'second scene pass, and they count what the source places, not what '
            'the guest can currently reach or kill.',
            'Map ownership, explored-room tracking, markers, pins and the journal, '
            'which are the rest of P17, are not touched here.',
        ],
    }
    dump(ROOT / '.hkpsx/shop-catalog.json', report)
    return report


def summary(report):
    lines = []
    sly = report['sly_shop']
    stock = sly['stock']
    lines.append(f"Sly's shop lives in {sly['shop_scene']} ({sly['shop_file']}), reached "
                 f"through Dirtmouth's door_sly, which only exists while "
                 f"{sly['town_door']['gated_by']} is true.")
    lines.append(f"  stock swaps to stockAlt when any of "
                 f"{', '.join(stock['alternate_when'])} is set.")
    for name, items in stock['lists'].items():
        if not items:
            continue
        lines.append(f"  {name} ({len(items)} items):")
        for item in items:
            gate = f", needs {item['requires']}" if item['requires'] else ''
            stale = '' if item['cost_is_serialized'] else f" (serialized {item['serialized_cost']}, stale)"
            lines.append(f"    {item['cost']:>5} Geo  {item['object']:<28} "
                         f"sets {item['sets']}{gate}{stale}")
    stag = report['stag_station']
    admitted_station = stag['admitted_station']
    lines.append('')
    lines.append(f"Stag: the only station in the slice is {admitted_station['scene']}. "
                 f"Ringing the bell costs {admitted_station['toll']} Geo "
                 f"({admitted_station['toll_price_key']}) and sets "
                 f"{admitted_station['unlock_bool']}. Travel itself costs "
                 f"{stag['travel_cost']}.")
    lines.append(f"  Dirtmouth's own station building appears when "
                 f"{stag['town_building']['visible_when']} is true and its door leads "
                 f"to {stag['town_building']['door']['to_scene']}.")
    for d in stag['destinations']:
        where = d['scene'] or 'unbound in Stag Control'
        gate = d['listed_when'] or ('always listed' if d['menu_row'] else 'no menu row')
        lines.append(f"    {d['choice']:<18} -> {where:<26} "
                     f"position {d['position']}, {gate}")
    geo = report['geo_sources']
    lines.append('')
    lines.append(f"Geo in the admitted scenes: {geo['rock_total']} from "
                 f"{geo['rock_count']} rocks and {geo['enemy_total']} from "
                 f"{geo['enemy_count']} enemy payouts, {geo['rock_total'] + geo['enemy_total']} "
                 f"per clear (enemies respawn, rocks do not).")
    for sink in report['geo_sinks']:
        scale = '' if sink['multiplier'] == 1.0 else ' x' + str(sink['multiplier'])
        lines.append(f"  spends: {sink['scene']:<16} {sink['object']:<20} "
                     f"{sink['cost']:>5} Geo  ({sink['price_key']}{scale})")
    lines.append('')
    lines.append('Out of reach: ' + ', '.join(
        s['scene'] for s in report['out_of_reach']['shop_scenes'] if not s['admitted']))
    return '\n'.join(lines)


if __name__ == '__main__':
    import sys
    if 'collect' in sys.argv[1:] or not sys.argv[1:]:
        print(summary(collect()))
    if 'cook' in sys.argv[1:] or not sys.argv[1:]:
        table, rules = cook()
        total = sum(row['cost'] for row in table['items'])
        print(f"\nCooked {len(table['items'])} rows into {SHOP_TABLE.relative_to(ROOT)}: "
              f"{total} Geo of stock across {len(table['slots'])} PlayerData bools, "
              f"discounting {rules['discount_factor']:.1f} on charm {rules['discount_charm']}.")
        print('  ' + '\n  '.join(f"{row['cost']:>5} Geo  {row['name']:<18} "
                                 f"{row['delivery']:<26} sets {row['sets_field']}"
                                 for row in table['items']))
