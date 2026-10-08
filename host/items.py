"""The item, charm, notch, fragment and resource catalog, from the installed source.

P16 step 1 wants the catalog derived rather than listed by hand, and the source
keeps it in four places. PlayerData's `SetupNewPlayerData` carries every field a
new save starts from, so it is the authority on notch costs, starting notches,
mask/vessel counters and the nail counter. The inventory prefab in
resources.assets carries the identity of each item: forty `InvCharmBackboard`
components name the charm PlayerData fields, and the `Build Equipment List` FSM
names every equipment item with its gate and its localization keys. The English
language sheets carry the names and descriptions, AES encrypted with a key that
is read out of `Encryption`'s static constructor rather than embedded here.
Finally the admitted scenes say which of the catalog the shipped slice contains:
each `Shiny Item` resolves through its own `Shiny Control` routing, and the mask
and vessel pickups resolve through the UI prefab that fuses them.

Nothing here is implemented in the guest: the port has no pickup, no inventory
and no charm, so this is a catalog and a scene audit, not a binding. No retail
payload is embedded; the report goes to .hkpsx.
"""
import html, json, re, subprocess
from pathlib import Path
from source import ROOT, dump
from focus import action_fields
from inspect_il import inspect
import language

# PlayerData fields this catalog tracks, by group. Every name is asserted
# against the CIL defaults before use, so a renamed field refuses rather than
# silently dropping out of the scene audit.
TRACKED = {
    'charm': ('charmsOwned', 'charmSlots', 'charmSlotsFilled', 'hasCharm', 'overcharmed', 'canOvercharm'),
    'notch': ('salubraNotch1', 'salubraNotch2', 'salubraNotch3', 'salubraNotch4',
              'notchShroomOgres', 'notchFogCanyon', 'gotGrimmNotch'),
    'mask': ('heartPieces', 'heartPieceCollected', 'heartPieceMax', 'maxHealth', 'maxHealthBase', 'maxHealthCap'),
    'vessel': ('vesselFragments', 'vesselFragmentCollected', 'MPReserveMax'),
    'nail': ('nailSmithUpgrades', 'nailDamage', 'honedNail'),
    'relic': ('trinket1', 'trinket2', 'trinket3', 'trinket4',
              'foundTrinket1', 'foundTrinket2', 'foundTrinket3', 'foundTrinket4'),
    'resource': ('geo', 'simpleKeys', 'rancidEggs', 'ore', 'grubsCollected', 'dreamOrbs'),
}
# Spell levels are P15's, but the admitted slice grants one, so the audit
# reports it rather than pretending the shiny is not there.
SPELL_LEVELS = ('fireballLevel', 'quakeLevel', 'screamLevel')
PD_WRITES = ('SetPlayerDataBool', 'SetPlayerDataInt', 'IncrementPlayerDataInt',
             'PlayerDataIntAdd', 'SetPlayerDataFloat')
CHARMS = 40
_LOAD = {f'ldc.i4.{n}': n for n in range(9)}
_LOAD['ldc.i4.m1'] = -1


def _short(name):
    return name.rsplit('.', 1)[-1]


def _variables(fsm):
    return {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
            for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}


def _scalar(value, variables):
    """A compact PlayMaker scalar, resolved against the instance's variables."""
    if isinstance(value, dict) and 'value' in value and 'useVariable' in value:
        return variables.get(value['name']) if value['useVariable'] else value['value']
    return value


def _text(value):
    """A compact PlayMaker string, or the variable name when it names one."""
    if isinstance(value, dict) and 'value' in value and 'useVariable' in value:
        return value['name'] if value['useVariable'] else value['value']
    return value


def _actions(state, kind):
    d = state['actionData']
    return [action_fields(d, i) for i, n in enumerate(d['actionNames'])
            if _short(n) == kind and d['actionEnabled'][i]]


def _transitions(state):
    out = {}
    for t in state.get('transitions', []):
        event = t['fsmEvent']['name'] if isinstance(t.get('fsmEvent'), dict) else t.get('eventName')
        out[event] = t['toState']
    return out


def _int_switch(state):
    """The one IntSwitch on a state, as {compared int: event}."""
    d = state['actionData']
    index = next(i for i, n in enumerate(d['actionNames'])
                 if _short(n) == 'IntSwitch' and d['actionEnabled'][i])
    fields = list(action_fields(d, index).items())
    compares = [v['value'] for _, v in fields[1:] if isinstance(v, dict) and 'useVariable' in v]
    events = [v for _, v in fields[1:] if isinstance(v, str)]
    assert len(compares) == len(events) and compares, 'IntSwitch no longer pairs compares with events'
    return dict(zip(compares, events))


def _writes(state, variables):
    """The PlayerData a state writes, with its FSM variables substituted in."""
    out, d = [], state['actionData']
    for i, n in enumerate(d['actionNames']):
        kind = _short(n)
        if kind not in PD_WRITES or not d['actionEnabled'][i]:
            continue
        f = action_fields(d, i)
        field = _scalar(f.get('boolName') or f.get('intName') or f.get('floatName'), variables)
        amount = f.get('value', f.get('amount'))
        write = {'action': kind, 'field': field,
                 'value': _scalar(amount, variables) if amount is not None else 1}
        # A write fed by a variable carries the variable's serialized value,
        # which is an editor default rather than what the write computes, so
        # name the variable alongside it.
        if isinstance(amount, dict) and amount.get('useVariable'):
            write['value_from'] = amount['name']
        out.append(write)
    return out


def _fsm_by_name(source, file, name):
    for o in file.objects.values():
        if o.type.name != 'MonoBehaviour' or source.typename(o) != 'PlayMakerFSM':
            continue
        fsm = source.read(o)['fsm']
        if fsm['name'] == name:
            return o, fsm
    raise LookupError(f'no {name} FSM in {file.name}')


def playerdata_defaults(assembly):
    """Every PlayerData field a new save starts from, read from SetupNewPlayerData."""
    body = inspect(assembly, {'PlayerData'}).split('\nPlayerData::SetupNewPlayerData')[1].split('\n\n')[0]
    values, pending = {}, None
    for line in body.split('\n'):
        parts = line.split(None, 2)
        if len(parts) < 2:
            continue
        op, operand = parts[1], parts[2].strip() if len(parts) > 2 else ''
        if op in _LOAD:
            pending = _LOAD[op]
        elif op in ('ldc.i4', 'ldc.i4.s'):
            pending = int(operand)
        elif op == 'ldc.r4':
            pending = float(operand)
        elif op == 'ldstr':
            pending = operand.strip("'")
        elif op == 'stfld':
            values[operand] = pending
            pending = None
        elif op != 'ldarg.0':
            # Anything else on the stack means the field is not a plain constant.
            pending = None
    missing = [f'{k}_{n}' for n in range(1, CHARMS + 1)
               for k in ('gotCharm', 'equippedCharm', 'newCharm', 'charmCost') if f'{k}_{n}' not in values]
    assert not missing, f'PlayerData no longer starts {missing}'
    for group in TRACKED.values():
        absent = [f for f in group if f not in values]
        assert not absent, f'PlayerData no longer starts {absent}'
    costs = [values[f'charmCost_{n}'] for n in range(1, CHARMS + 1)]
    assert all(isinstance(c, int) and c >= 1 for c in costs), 'a charm notch cost is not a positive int'
    return values


def _encryption_key(managed=None):
    """Kept for callers; `host/language.py` owns the derivation."""
    return language.encryption_key()


def language_sheet(source, sheet, key=None):
    """Kept for callers; `host/language.py` owns the decryption."""
    return language.sheet(source, sheet)


def charms(source, defaults, ui):
    """The forty charms: id, sprite name, notch cost and PlayerData fields."""
    file = source.file('resources.assets')
    icons = backboards = None
    found = {}
    for o in file.objects.values():
        if o.type.name != 'MonoBehaviour':
            continue
        kind = source.typename(o)
        if kind == 'CharmIconList':
            icons = source.read(o)
        elif kind == 'InvCharmBackboard':
            board = source.read(o)
            found[board['charmNum']] = board
    assert icons is not None, 'the inventory lost its CharmIconList'
    assert sorted(found) == list(range(1, CHARMS + 1)), f'inventory charm backboards are {sorted(found)}'
    # CharmIconList.GetSprite returns spriteList[id] outside its four quest
    # variants, so entry id names charm id and entry 0 is unused.
    sprites = icons['spriteList']
    assert len(sprites) == CHARMS + 1, f'the charm sprite list holds {len(sprites)} entries'
    out = []
    for n in range(1, CHARMS + 1):
        board = found[n]
        assert board['gotCharmString'] == f'gotCharm_{n}' and board['newCharmString'] == f'newCharm_{n}', \
            f'charm {n} no longer names its own PlayerData fields'
        sprite = source.read(source.ref(file, sprites[n]))
        fields = {'owned': f'gotCharm_{n}', 'equipped': f'equippedCharm_{n}',
                  'unseen': f'newCharm_{n}', 'cost': f'charmCost_{n}'}
        if f'brokenCharm_{n}' in defaults:
            fields['broken'] = f'brokenCharm_{n}'
        # A charm keeps one key each, except where the source varies it: the
        # fragile three carry _BROKEN and _G, and Kingsoul (36) an A/B/C set.
        def keyed(kind):
            pattern = re.compile(rf'CHARM_{kind}_{n}(_[A-Z]+)?$')
            return {k: ui[k] for k in sorted(ui) if pattern.fullmatch(k)}
        names = keyed('NAME')
        assert names, f'no localized name for charm {n}'
        out.append({
            'id': n,
            'internal_name': sprite['m_Name'],
            'notch_cost': defaults[f'charmCost_{n}'],
            'names': names,
            'descriptions': keyed('DESC'),
            'playerdata': fields,
        })
    return out


def equipment_items(source, ui):
    """Relics, keys and the simple inventory items, from Build Equipment List.

    Each state gates one inventory object on PlayerData and names the title and
    description keys it pushes into the inventory's text FSM, so the state list
    is the equipment catalog rather than a guess at what the inventory shows.
    """
    file = source.file('resources.assets')
    _, fsm = _fsm_by_name(source, file, 'Build Equipment List')
    items = []
    for state in fsm['states']:
        d = state['actionData']
        gates, ints, child, pending = [], {}, None, None
        for i, name in enumerate(d['actionNames']):
            if not d['actionEnabled'][i]:
                continue
            kind, f = _short(name), action_fields(d, i)
            if kind == 'PlayerDataBoolTest':
                must = True if f['isFalse'] and not f['isTrue'] else (
                    False if f['isTrue'] and not f['isFalse'] else None)
                gates.append({'field': _text(f['boolName']), 'must_be': must})
            elif kind == 'GetPlayerDataInt':
                ints[_text(f['storeValue'])] = _text(f['intName'])
            elif kind == 'IntCompare' and _text(f['integer1']) in ints:
                assert f['equal'] and f['lessThan'] and not f['greaterThan'], \
                    'a counted inventory item no longer shows only above its threshold'
                gates.append({'field': ints[_text(f['integer1'])],
                              'must_be': f'above {_scalar(f["integer2"], {})}'})
            elif kind == 'FindChild':
                child = _text(f['childName'])
            elif kind == 'SetFsmString':
                variable, value = _text(f['variableName']), _text(f['setValue'])
                if variable == 'Title Variable Name':
                    pending = {'object': child, 'requires': list(gates),
                               'name_key': value, 'name': ui.get(value)}
                elif variable == 'Desc variable Name' and pending:
                    items.append({'state': state['name'], **pending,
                                  'description_key': value, 'description': ui.get(value)})
                    pending = None
    assert items, 'Build Equipment List no longer names any item'
    return items


def counters(source):
    """Inventory items that show an amount, and the PlayerData int behind each."""
    file = source.file('resources.assets')
    out = []
    for o in file.objects.values():
        if o.type.name != 'MonoBehaviour' or source.typename(o) != 'DisplayItemAmount':
            continue
        display = source.read(o)
        owner = source.read(file.objects[o.parse_monobehaviour_head().m_GameObject.m_PathID])
        out.append({'object': owner['m_Name'], 'field': display['playerDataInt']})
    assert out, 'the inventory lost its DisplayItemAmount counters'
    return sorted(out, key=lambda item: item['field'])


def _fragment_rule(source, file, fsm):
    """How many of a pickup make a whole one, from the UI prefab it spawns.

    The pickup only increments its counter. The prefab its `UI` state creates
    compares the counter, resets it and sends the PlayerData the upgrade, so
    the fusing rule lives there and not in the pickup.
    """
    states = {s['name']: s for s in fsm['states']}
    d = states['UI']['actionData']
    index = next(i for i, n in enumerate(d['actionNames']) if _short(n) == 'CreateObject')
    start = d['actionStartIndex'][index]
    assert d['paramName'][start] == 'gameObject' and d['paramDataType'][start] == 19, \
        'the fragment UI is no longer the first CreateObject parameter'
    obj = source.ref(file, d['fsmGameObjectParams'][d['paramDataPos'][start]]['value'])
    prefab, shared = source.read(obj), obj.assets_file
    rule = {'ui_prefab': prefab['m_Name'], 'per_whole': None, 'awards': [], 'resets': None}
    thresholds = []
    for component in prefab['m_Component']:
        part = source.ref(shared, component['component'])
        if source.typename(part) != 'PlayMakerFSM':
            continue
        ui = source.read(part)['fsm']
        for state in ui['states']:
            for a in _actions(state, 'IntCompare'):
                # The fusing test counts a variable against a literal and takes
                # the same branch at and above it. The heal check next to it
                # compares two variables, so the literal tells them apart.
                if (a['integer1']['useVariable'] and not a['integer2']['useVariable']
                        and a['equal'] == a['greaterThan'] and a['equal'] != a['lessThan']):
                    thresholds.append(a['integer2']['value'])
            for a in _actions(state, 'SetPlayerDataInt'):
                if _scalar(a['value'], {}) == 0:
                    rule['resets'] = _text(a['intName'])
            for a in _actions(state, 'SendMessage'):
                call = a['functionCall']
                if call['parameterType'] == 'int':
                    rule['awards'].append({'call': call['FunctionName'],
                                           'amount': call['IntParameter']['value']})
    assert len(thresholds) == 1, f'{rule["ui_prefab"]} has {len(thresholds)} fusing thresholds'
    rule['per_whole'] = thresholds[0]
    assert rule['awards'] and rule['resets'], f'{rule["ui_prefab"]} no longer fuses'
    return rule


def _shiny_grant(fsm, variables):
    """What one `Shiny Item` instance gives, by walking its own routing.

    Shiny Control is one prefab with every item branch in it, so the instance's
    variables pick the branch: `Charm` takes the charm path, the seven big item
    bools take a named ability path, and `Trinket Num` indexes the IntSwitch
    that reaches the relic, key, egg, ore and notch states.
    """
    states = {s['name']: s for s in fsm['states']}
    if variables.get('Charm'):
        grant = {'route': 'charm', 'charm_id': variables.get('Charm ID') or None,
                 'writes': _writes(states['Get Charm'], variables)}
        assert any(w['field'] == 'charmsOwned' for w in grant['writes']), 'Get Charm no longer counts charms'
        return grant
    choice = states['Item Choice']
    for a in _actions(choice, 'BoolTest'):
        if variables.get(a['boolVariable']['name']):
            target = _transitions(choice)[a['isTrue']]
            return {'route': 'big item', 'state': target, 'writes': _writes(states[target], variables)}
    number = variables.get('Trinket Num') or 0
    if number > 0:
        switch = states['Trinket Type']
        event = _int_switch(switch).get(number)
        assert event, f'Trinket Num {number} has no branch'
        target = _transitions(switch)[event]
        return {'route': 'trinket', 'trinket_num': number, 'state': target,
                'writes': _writes(states[target], variables)}
    # Divine hands back a fragile charm whose id her own FSM writes at runtime.
    return {'route': 'assigned at runtime', 'writes': []}


def scene_items(source, scenes, tracked):
    """What of the catalog each admitted scene contains, one pass per scene."""
    needles = re.compile('|'.join(sorted(
        (re.escape(f) if re.match(r'^[A-Za-z]{4,}$', f) else rf'(?<![A-Za-z0-9_]){re.escape(f)}(?![A-Za-z0-9_])')
        for f in tracked)).encode('utf8') + rb'|gotCharm_\d+|equippedCharm_\d+|newCharm_\d+|charmCost_\d+')
    found = {}
    for entry in scenes:
        file = source.file(entry['file'])
        names, transforms = {}, {}
        for o in file.objects.values():
            if o.type.name == 'GameObject':
                names[o.path_id] = source.read(o)
            elif o.type.name in ('Transform', 'RectTransform'):
                transforms[o.path_id] = source.read(o)
        owners = {t['m_GameObject']['m_PathID']: pid for pid, t in transforms.items()}

        def place(gid):
            path, x, y = [], 0.0, 0.0
            t = transforms[owners[gid]]
            while True:
                go = names[t['m_GameObject']['m_PathID']]
                path.append(go['m_Name'] if go['m_IsActive'] else go['m_Name'] + ' [inactive]')
                x += t['m_LocalPosition']['x']
                y += t['m_LocalPosition']['y']
                if not t['m_Father']['m_PathID']:
                    return list(reversed(path)), [round(x, 2), round(y, 2)]
                t = transforms[source.ref(file, t['m_Father']).path_id]

        items = []
        for o in file.objects.values():
            if o.type.name != 'MonoBehaviour' or not needles.search(o.get_raw_data()):
                continue
            if source.typename(o) != 'PlayMakerFSM':
                continue
            fsm = source.read(o)['fsm']
            variables = _variables(fsm)
            if fsm['name'] == 'Shiny Control':
                grants = [_shiny_grant(fsm, variables)]
            else:
                grants = []
                for state in fsm['states']:
                    for write in _writes(state, variables):
                        if write['field'] in tracked:
                            grants.append({'route': fsm['name'], 'state': state['name'], 'writes': [write]})
            if fsm['name'] in ('Heart Container Control', 'Vessel Fragment Control'):
                grants.append({'route': 'fuse', **_fragment_rule(source, file, fsm)})
            if not grants:
                continue
            gid = o.parse_monobehaviour_head().m_GameObject.m_PathID
            path, position = place(gid)
            items.append({'source': source.sid(o), 'fsm': fsm['name'],
                          'path': path, 'position': position, 'grants': grants})
        if items:
            found[entry['scene_name']] = {'file': entry['file'], 'items': items}
    return found


def _obtainable(found, tracked):
    """The fields the admitted scenes actually write, and where."""
    out = {}
    for scene, data in found.items():
        for item in data['items']:
            for grant in item['grants']:
                for write in grant.get('writes', []):
                    field = write['field']
                    if field not in tracked or write['action'] == 'SetPlayerDataFloat':
                        continue
                    where = out.setdefault(field, {'group': tracked[field], 'sites': []})
                    site = {
                        'scene': scene, 'object': item['path'][-1], 'position': item['position'],
                        'fsm': item['fsm'], 'route': grant['route'], 'value': write['value'],
                        'value_from': write.get('value_from'), 'action': write['action'],
                    }
                    # One object often writes the same field from several states
                    # of the same branch; the site is what matters, not the copy.
                    if site not in where['sites']:
                        where['sites'].append(site)
    return dict(sorted(out.items()))


def source_item_values(source, assembly, scenes):
    managed = Path(assembly).parent
    defaults = playerdata_defaults(assembly)
    key = _encryption_key(managed)
    ui = language_sheet(source, 'UI', key)
    prices = language_sheet(source, 'Prices', key)
    catalog = charms(source, defaults, ui)
    equipment = equipment_items(source, ui)
    tracked = {field: group for group, fields in TRACKED.items() for field in fields}
    tracked.update({f'gotCharm_{n}': 'charm' for n in range(1, CHARMS + 1)})
    tracked.update({f'newCharm_{n}': 'charm' for n in range(1, CHARMS + 1)})
    tracked.update({f'charmCost_{n}': 'charm' for n in range(1, CHARMS + 1)})
    tracked.update({field: 'spell' for field in SPELL_LEVELS})
    for item in equipment:
        for gate in item['requires']:
            tracked.setdefault(gate['field'], 'equipment')
    found = scene_items(source, scenes, tracked)
    obtainable = _obtainable(found, tracked)
    for charm in catalog:
        sites = obtainable.get(charm['playerdata']['owned'], {}).get('sites', [])
        charm['in_admitted_scenes'] = [f"{s['scene']}:{s['object']}" for s in sites]
    def prefixed(table, prefix):
        return {k: v for k, v in sorted(table.items()) if k.startswith(prefix)}

    def fuse(counter):
        """The fusing rule a pickup of this counter carries, if the slice has one."""
        return next((g for data in found.values() for item in data['items'] for g in item['grants']
                     if g['route'] == 'fuse' and g['resets'] == counter), None)
    return {
        'charms': catalog,
        'charm_count': len(catalog),
        'notches': {
            'starting_slots': defaults['charmSlots'],
            'playerdata': {'slots': 'charmSlots', 'filled': 'charmSlotsFilled',
                           'overcharmed': 'overcharmed', 'unlocked_overcharm': 'canOvercharm'},
            'one_off_flags': {f: defaults[f] for f in TRACKED['notch']},
            'salubra_prices': prefixed(prices, 'NOTCH_'),
        },
        'fragments': {
            'mask_shard': {
                'counter': 'heartPieces', 'collected_flag': 'heartPieceCollected',
                'capped_flag': 'heartPieceMax', 'starting_masks': defaults['maxHealth'],
                'mask_cap': defaults['maxHealthCap'], 'fuse': fuse('heartPieces'),
                'names': prefixed(ui, 'INV_NAME_HEARTPIECE'),
                'descriptions': prefixed(ui, 'INV_DESC_HEARTPIECE'),
                'shop_prices': prefixed(prices, 'HEARTPIECE_'),
            },
            'vessel_fragment': {
                'counter': 'vesselFragments', 'collected_flag': 'vesselFragmentCollected',
                'starting_reserve': defaults['MPReserveMax'], 'fuse': fuse('vesselFragments'),
                'names': prefixed(ui, 'INV_NAME_SOULORBS'),
                'descriptions': prefixed(ui, 'INV_DESC_SOULORBS'),
                'shop_prices': prefixed(prices, 'SOULPIECE_'),
            },
        },
        'nail': {
            'note': 'Levels and costs only. The damage table belongs to the nail upgrade work.',
            'upgrade_counter': 'nailSmithUpgrades',
            'starting_upgrades': defaults['nailSmithUpgrades'],
            'starting_damage': defaults['nailDamage'],
            'honed_flag': 'honedNail',
            'ore_counter': 'ore',
            'names': {k: ui[k] for k in sorted(ui) if re.fullmatch(r'INV_NAME_NAIL\d', k)},
            'descriptions': {k: ui[k] for k in sorted(ui) if re.fullmatch(r'INV_DESC_NAIL\d', k)},
            'geo_prices': prefixed(prices, 'NAIL_UPGRADE_'),
        },
        'equipment_items': equipment,
        'counters': counters(source),
        'prices_sheet': dict(sorted(prices.items())),
        'admitted_scene_count': len(scenes),
        'scenes_with_items': found,
        'obtainable_in_admitted_scenes': obtainable,
        'limitations': [
            'Nothing in this catalog is implemented: the guest has no pickup, no inventory, '
            'no charm and no notch, so no item can be earned in the port today.',
            'The audit reports what the admitted scenes contain, not what a run can reach. '
            'Chest opening, PersistentBoolItem state, grub counts, conversation gates and '
            'dream warps were read but not simulated, so a listed pickup may still sit behind '
            'a precondition the slice cannot satisfy.',
            'Charm effects are not read here. Each charm is catalogued by id, cost and '
            'PlayerData fields; the equip contract, the overcharm rule and the effects the '
            'port can express are read by host/charms.py, which builds on this catalog and '
            'reports what each remaining charm is missing.',
            'Geo prices are carried as the raw Prices sheet. Binding a price key to a charm or '
            'an item needs the shops ShopItemStats components, and every shop scene is outside '
            'the admitted slice.',
            'Nail upgrade levels carry their geo prices, but the level to Pale Ore binding is in '
            "the Nailsmith's own FSM, which no admitted scene contains.",
            'Kingsoul (36) and Grimmchild (40) change name, description and sprite with quest '
            'progress. Their key sets are listed; the progress variables that pick one are not read.',
        ],
    }


if __name__ == '__main__':
    from source import Source
    source = Source()
    data = Path(json.load(open(ROOT / '.hkpsx/doctor.json'))['installs'][0]['data_directory'])
    scenes = json.load(open(ROOT / '.hkpsx/selected-regions.json'))['scenes']
    values = source_item_values(source, data / 'Managed' / 'Assembly-CSharp.dll', scenes)
    dump(ROOT / '.hkpsx/item-catalog.json', values)
    print(f"{values['charm_count']} charms, notch costs {min(c['notch_cost'] for c in values['charms'])}"
          f" to {max(c['notch_cost'] for c in values['charms'])}, {values['notches']['starting_slots']} starting notches")
    print(f"{len(values['equipment_items'])} equipment items, {len(values['counters'])} counted items")
    for kind in ('mask_shard', 'vessel_fragment'):
        rule = values['fragments'][kind]['fuse']
        award = ', '.join(f"{a['call']}({a['amount']})" for a in rule['awards']) if rule else 'not in the slice'
        print(f"  {kind}: {rule['per_whole'] if rule else '?'} per whole, {award}")
    print(f"items in {len(values['scenes_with_items'])} of {values['admitted_scene_count']} admitted scenes")
    for field, where in values['obtainable_in_admitted_scenes'].items():
        sites = '; '.join(f"{s['scene']} {s['object']} [{s['action']} {s['value_from'] or s['value']}]"
                          for s in where['sites'])
        print(f"  [{where['group']}] {field}: {sites}")
