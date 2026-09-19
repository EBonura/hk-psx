"""The nail damage tier table, from PlayerData and the Nailsmith's FSM.

P15 step 3 wants the damage tiers taken from the source instead of the single
hardcoded `PURE_NAIL_DAMAGE` guess in game/src/cheats.rs, and the source splits
the table across three places. `PlayerData::SetupNewPlayerData` holds the
starting `nailDamage`, the Nailsmith's `Conversation Control` FSM in
Room_nailsmith (level16) holds one `Upgrade N` variable per tier and writes the
chosen one into `nailDamage`, and the geo price of each tier is a Prices
language entry the same FSM reads by name. `PlayerData::AddGGPlayerDataOverrides`
writes the fully upgraded value, which is the cross-check that the last tier is
right.

No retail payload is embedded; the report goes to .hkpsx.
"""
from focus import action_fields
from inspect_il import inspect

# The Nailsmith scene. Room_nailsmith is the only BuildSettings scene whose
# FSMs write nailDamage; level4 (Knight_Pickup) only reads it.
NAILSMITH_SCENE = 'level16'
TIERS = 4


def _reader(fsm):
    states = {s['name']: s for s in fsm['states']}
    variables = {v['name']: v['value'] for group in fsm['variables'].values() if isinstance(group, list)
                 for v in group if isinstance(v, dict) and 'name' in v and 'value' in v}
    def scalar(value):
        if isinstance(value, dict) and 'value' in value and 'useVariable' in value:
            return variables.get(value['name']) if value['useVariable'] else value['value']
        return value
    def actions(state, kind):
        d = states[state]['actionData']
        return [action_fields(d, i) for i, n in enumerate(d['actionNames'])
                if n.rsplit('.', 1)[-1] == kind and d['actionEnabled'][i]]
    return states, variables, scalar, actions


def _nailsmith_fsm(source):
    file = source.file(NAILSMITH_SCENE)
    found = []
    for o in file.objects.values():
        if o.type.name != 'MonoBehaviour' or source.typename(o) != 'PlayMakerFSM':
            continue
        fsm = source.read(o)['fsm']
        if fsm['name'] != 'Conversation Control':
            continue
        owner = source.read(source.ref(file, {'m_FileID': 0,
                                              'm_PathID': o.parse_monobehaviour_head().m_GameObject.m_PathID}))
        if owner['m_Name'] == 'Nailsmith':
            found.append((source.sid(o), fsm))
    assert len(found) == 1, f'{NAILSMITH_SCENE} no longer carries one Nailsmith Conversation Control'
    return found[0]


def _int_constants(text, method, fields):
    """The constant each `stfld <field>` in one method stores, from the CIL.

    dnPE gives the method bodies as instruction text; a PlayerData field set to
    a small literal is always `ldc.i4*` then `stfld`, so the previous
    instruction carries the value.
    """
    body = text.split(f'\nPlayerData::{method} ')[1].split('\nPlayerData::')[0]
    lines = [l.split(None, 2) for l in body.splitlines() if l.strip()]
    out = {}
    for i, parts in enumerate(lines):
        if len(parts) < 3 or parts[1] != 'stfld' or parts[2] not in fields:
            continue
        load = lines[i - 1]
        assert load[1].startswith('ldc.i4'), f'{method} sets {parts[2]} from {load[1]}, not a literal'
        value = int(load[2]) if load[2] != 'None' else int(load[1].rsplit('.', 1)[-1])
        out[parts[2]] = value
    missing = sorted(set(fields) - set(out))
    assert not missing, f'PlayerData::{method} no longer sets {missing}'
    return out


def source_nail_upgrade_values(source, assembly):
    text = inspect(assembly, {'PlayerData'})
    fresh = _int_constants(text, 'SetupNewPlayerData', {'nailDamage', 'nailSmithUpgrades'})
    maxed = _int_constants(text, 'AddGGPlayerDataOverrides', {'nailDamage', 'nailSmithUpgrades', 'honedNail'})
    assert fresh['nailSmithUpgrades'] == 0, 'a new save no longer starts with no Nailsmith upgrades'
    assert maxed['nailSmithUpgrades'] == TIERS, f'the Godhome override no longer buys {TIERS} upgrades'
    assert maxed['honedNail'] == 1, 'the Godhome override no longer hones the nail'

    sid, fsm = _nailsmith_fsm(source)
    states, variables, scalar, actions = _reader(fsm)

    # One `Upgrade N` variable per tier, picked by the `Upgrade N` state, whose
    # price comes from the Prices sheet rather than from the FSM. The sheet
    # reader is imported here so this module stays cheap to import.
    from read_points import language_sheet
    prices, sheet = language_sheet(source, 'EN_Prices')
    tiers = []
    for tier in range(1, TIERS + 1):
        state = f'Upgrade {tier}'
        chosen = [a for a in actions(state, 'SetIntValue')
                  if a['intVariable']['name'] == 'New Damage']
        assert len(chosen) == 1, f'{state} no longer picks one New Damage'
        assert chosen[0]['intValue']['name'] == f'Upgrade {tier}', f'{state} reads the wrong variable'
        price_key = actions(state, 'GetLanguageString')
        assert len(price_key) == 1 and scalar(price_key[0]['sheetName']) == 'Prices', f'{state} price sheet moved'
        key = scalar(price_key[0]['convName'])
        assert key in prices, f'the Prices sheet no longer carries {key}'
        stored = actions(state, 'ConvertStringToInt')
        assert len(stored) == 1 and stored[0]['intVariable']['name'] == 'Upgrade Cost', f'{state} cost target moved'
        tiers.append({'tier': tier, 'damage': scalar(chosen[0]['intValue']),
                      'geo': int(prices[key]), 'price_key': key})

    damages = [fresh['nailDamage']] + [t['damage'] for t in tiers]
    assert damages == sorted(damages) and len(set(damages)) == len(damages), f'the tiers no longer rise: {damages}'
    assert damages[-1] == maxed['nailDamage'], (
        f'the last Nailsmith tier {damages[-1]} disagrees with the Godhome override {maxed["nailDamage"]}')

    # The one state that writes the tier: it pays with ore, hones the nail and
    # counts the upgrade, so `nailSmithUpgrades` is the index into the table.
    write = actions('Upgrade', 'SetPlayerDataInt')
    assert len(write) == 1 and scalar(write[0]['intName']) == 'nailDamage', 'the Upgrade state stopped writing nailDamage'
    assert write[0]['value']['name'] == 'New Damage', 'the Upgrade state no longer writes the chosen tier'
    honed = actions('Upgrade', 'SetPlayerDataBool')
    assert len(honed) == 1 and scalar(honed[0]['boolName']) == 'honedNail' and scalar(honed[0]['value'])
    counted = actions('Upgrade', 'IncrementPlayerDataInt')
    assert len(counted) == 1 and scalar(counted[0]['intName']) == 'nailSmithUpgrades', 'the upgrade count moved'

    # The offer and the price both switch on nailSmithUpgrades, so the table is
    # indexed by it and nothing else.
    for state in ('Offer Type', 'Price Check'):
        switch = actions(state, 'IntSwitch')
        assert len(switch) == 1 and switch[0]['intVariable']['name'] == 'Upgrades Completed', f'{state} switch moved'
        compares = sorted(v['value'] for k, v in switch[0].items()
                          if k.isdigit() and isinstance(v, dict) and 'useVariable' in v)
        assert compares == list(range(TIERS)), f'{state} no longer branches on 0..{TIERS - 1}: {compares}'
    read = actions('Offer Type', 'GetPlayerDataInt')
    assert any(scalar(a['intName']) == 'nailSmithUpgrades' for a in read), 'the offer no longer reads nailSmithUpgrades'

    # Ore is spent from tier 2 on: each `Offer N` sets its own decrement.
    ore = {}
    for tier in range(1, TIERS + 1):
        decrement = [a for a in actions(f'Offer {tier}', 'SetIntValue')
                     if a['intVariable']['name'] == 'Ore Decrement']
        assert len(decrement) == 1, f'Offer {tier} no longer sets one Ore Decrement'
        ore[tier] = -scalar(decrement[0]['intValue'])
    assert ore == {1: 0, 2: 1, 3: 2, 4: 3}, f'the ore cost per tier changed: {ore}'

    for t in tiers:
        t['ore'] = ore[t['tier']]

    return {
        'source': {'playerdata': 'Assembly-CSharp.dll PlayerData', 'fsm': sid,
                   'scene': NAILSMITH_SCENE, 'prices_sheet': sheet},
        'base_damage': fresh['nailDamage'],
        'tiers': tiers,
        'damage_by_upgrades': damages,
        'godhome_override': maxed['nailDamage'],
        'gate': 'nailSmithUpgrades indexes the table; honedNail becomes true on the first upgrade',
        'limitations': [
            'The Nailsmith himself is not cooked: no admitted scene is Room_nailsmith, so the '
            'tiers are only reachable through the Cheats rows until the NPC exists.',
            'Geo and Pale Ore are read as the price of each tier but neither currency is spent '
            'by the port yet.',
            'Charm-modified nail damage (Fragile/Unbreakable Strength, charm 25) is not applied, '
            'since charms do not exist yet.',
            'The Nailsmith cinematic, his death branch and the nailsmithCliff state are not read.',
        ],
    }


def generated_nail_upgrade_params(values):
    table = values['damage_by_upgrades']
    out = (f'pub const NAIL_DAMAGE_TIERS: [u16; {len(table)}] = '
           + '[' + ','.join(str(v) for v in table) + '];\n')
    out += f'pub const NAIL_DAMAGE_BASE: u16 = {values["base_damage"]};\n'
    out += f'pub const NAIL_DAMAGE_MAX: u16 = {table[-1]};\n'
    return out


if __name__ == '__main__':
    import json
    from pathlib import Path
    from source import ROOT, Source, dump
    s = Source()
    data = Path(json.load(open(ROOT / '.hkpsx/doctor.json'))['installs'][0]['data_directory'])
    out = source_nail_upgrade_values(s, data / 'Managed' / 'Assembly-CSharp.dll')
    dump(ROOT / '.hkpsx/nail-upgrades.json', out)
    print(f'nailDamage by nailSmithUpgrades: {out["damage_by_upgrades"]}')
    for t in out['tiers']:
        print(f'  tier {t["tier"]}: damage {t["damage"]}, {t["geo"]} geo ({t["price_key"]}), {t["ore"]} ore')
    print(generated_nail_upgrade_params(out), end='')
