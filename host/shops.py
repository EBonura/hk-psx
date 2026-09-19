"""The economy, shop and station catalog P17 step 1 asks for, from the source.

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

Nothing is cooked and no retail payload is embedded. The report goes to
.hkpsx/shop-catalog.json.

Scope note: of everything here only the Crossroads stag station, Cornifer's map
and the City lift toll are inside the admitted scene list. Sly's shop is a
separate scene (`Room_shop`) that the slice does not contain; it is catalogued
because Dirtmouth carries its door. See `out_of_reach` in the report.
"""
import base64
import json
import mmap
import xml.etree.ElementTree as ElementTree
import language
from source import Source, ROOT, dump
from scene import Scene
from focus import action_fields

# StringEncrypt::.cctor, with Mode=ECB and Padding=PKCS7 from Decrypt/EncryptData.
PRICE_SHEET = 'EN_Prices'
GEO_REPORT = ROOT / '.hkpsx/geo-provenance.json'
ADMITTED = ROOT / '.hkpsx/selected-regions.json'

# ---------------------------------------------------------------- AES-256-ECB
# The language sheets are the only authority on shipped prices and the install
# ships them encrypted, so the decryption lives here rather than a new
# dependency. Decrypt only; nothing in this repo ever writes a sheet back.
_SBOX = bytearray(256)
_INV = bytearray(256)


def _build_sbox():
    p = q = 1
    while True:
        p = p ^ ((p << 1) & 0xff) ^ (0x1b if p & 0x80 else 0)
        q ^= q << 1
        q ^= q << 2
        q ^= q << 4
        q &= 0xff
        if q & 0x80:
            q ^= 0x09
        _SBOX[p] = (q ^ ((q << 1) | (q >> 7)) ^ ((q << 2) | (q >> 6))
                    ^ ((q << 3) | (q >> 5)) ^ ((q << 4) | (q >> 4)) ^ 0x63) & 0xff
        if p == 1:
            break
    _SBOX[0] = 0x63
    for value, substituted in enumerate(_SBOX):
        _INV[substituted] = value


def _mul(a, b):
    """Multiply in GF(2^8) with the AES polynomial."""
    result = 0
    while b:
        if b & 1:
            result ^= a
        a = ((a << 1) ^ 0x1b) & 0xff if a & 0x80 else a << 1
        b >>= 1
    return result


def _schedule(key):
    size = len(key) // 4
    rounds = size + 6
    words = [list(key[4 * i:4 * i + 4]) for i in range(size)]
    rcon = 1
    for i in range(size, 4 * (rounds + 1)):
        word = list(words[i - 1])
        if i % size == 0:
            word = [_SBOX[b] for b in word[1:] + word[:1]]
            word[0] ^= rcon
            rcon = _mul(rcon, 2)
        elif size > 6 and i % size == 4:
            word = [_SBOX[b] for b in word]
        words.append([words[i - size][j] ^ word[j] for j in range(4)])
    return words, rounds


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
    scenes = json.loads(ADMITTED.read_text())['scenes']
    return {s['file']: s['scene_name'] for s in scenes}


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
            'notes': [
                'Every ShopMenuStock in the game lives in a scene the admitted list '
                'does not contain, Sly included. Dirtmouth carries his door and the '
                'shop interior is a separate scene.',
                'The only station in the slice is Crossroads_47. Its toll can be paid '
                'and the station opened, but the only destination inside the slice is '
                'Crossroads_47 itself, which Current Location Check cancels. Every '
                'stop that would move the player, Dirtmouth included, is outside it.',
                'Nail upgrades, charm notches, charm repair, the relic dealer, map and '
                'quill and pin purchases and the bank all live in unadmitted scenes. '
                'Cornifer selling the Crossroads map is the one map purchase in reach.',
                'Charms cannot be delivered yet: the charm branch increments '
                'charmsOwned and raises a charm message, and P16 owns that inventory.',
                'The stag menu never hides Dirtmouth, so a player who opens the '
                'Crossroads station is offered a stop the slice does not contain. '
                'Forest Of Bones and Palace Grounds are dead events on Stag Control: '
                'no menu row and no scene.',
            ],
        },
        'limitations': [
            'Extraction only. Nothing here is cooked, bound to the guest or given a '
            'runtime, and no purchase can be made in the port today.',
            'Prices come from the decrypted EN_Prices sheet because ShopItemStats::Awake '
            'overwrites the serialized cost with it. Only the English sheet is read; '
            'the other eleven language sheets are not checked for disagreement.',
            'Charm prices are read but no charm can be equipped, so notch cost, '
            'charmsRequired and the Defender\'s Crest dung discount are recorded '
            'and not applied.',
            'Shop UI layout, icons, text, the item list scrolling and the confirm '
            'window are not extracted; only the catalog and the transaction are.',
            'Relic selling (the Selling Shop branch of Confirm Control) is recorded '
            'as a branch but not extracted: no relic dealer scene is admitted.',
            'Mask shard and vessel fragment counting happens inside a prefab the '
            'branch spawns, not in the branch, so only the branch action list is '
            'reported for special types 1 and 3.',
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
    print(summary(collect()))
