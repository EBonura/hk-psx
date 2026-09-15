#!/usr/bin/env python3
"""Extract the installed PlayerData completion calculation into a checklist."""
import argparse
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'host'))
from actors import _literal


def token_name(pe, token):
    table = pe.net.mdtables.tables.get(token.table)
    if table is None:
        return str(token)
    row = table.rows[token.rid - 1]
    return str(getattr(row, 'Name', getattr(row, 'TypeName', token)))


def method_rows(pe, type_name, method_name):
    from dncil.cil.body.reader import read_method_body_from_bytes
    matches = []
    for typ in pe.net.mdtables.TypeDef.rows:
        if str(typ.TypeName) != type_name:
            continue
        for ref in typ.MethodList:
            method = ref.row
            if str(method.Name) == method_name and method.Rva:
                body = read_method_body_from_bytes(pe.get_data(method.Rva, 100000))
                rows = []
                for instruction in body.instructions:
                    field = None
                    if instruction.opcode.name in ('ldfld','ldflda','stfld'):
                        field = token_name(pe, instruction.operand)
                    rows.append({'offset':instruction.offset, 'op':instruction.opcode.name,
                                 'field':field, 'literal':_literal(instruction),
                                 'target':(instruction.operand if instruction.opcode.name.startswith('br')
                                           else None)})
                raw = pe.get_data(method.Rva, body.size)
                matches.append((rows, hashlib.sha256(raw).hexdigest()))
    if len(matches) != 1:
        raise ValueError(f'Expected one {type_name}.{method_name}, got {len(matches)}')
    return matches[0]


def next_accumulator_write(rows, start, accumulator, limit=14):
    for row in rows[start + 1:start + 1 + limit]:
        if row['op'] == 'stfld' and row['field'] == accumulator:
            return True
        if row['op'] == 'ret':
            break
    return False


def next_numeric_literal(rows, start, limit=12):
    for row in rows[start + 1:start + 1 + limit]:
        if row['literal'] is not None:
            return row['literal']
    return None


def extract_conditional_fields(rows, accumulator):
    result = []
    for index, row in enumerate(rows[:-1]):
        if row['op'] != 'ldfld' or row['field'] == accumulator:
            continue
        if index and rows[index-1]['op'] == 'ldflda':
            continue
        branch = rows[index + 1]['op']
        if not branch.startswith('br'):
            continue
        if not next_accumulator_write(rows, index, accumulator):
            continue
        weight = next_numeric_literal(rows, index)
        if weight is not None:
            result.append({'kind':'boolean', 'field':row['field'],
                           'weight':weight, 'branch':branch, 'offset':row['offset']})
    return result


def extract_direct_fields(rows, accumulator):
    result = []
    for index, row in enumerate(rows):
        if row['op'] != 'conv.r4' or not next_accumulator_write(rows, index, accumulator, 3):
            continue
        fields = [candidate['field'] for candidate in rows[max(0,index-4):index]
                  if candidate['op'] == 'ldfld' and candidate['field'] != accumulator]
        if fields:
            kind = 'integer_minus_baseline' if any(
                candidate['op'] == 'sub' for candidate in rows[max(0,index-3):index]) else 'integer_value'
            baseline = None
            if kind == 'integer_minus_baseline':
                literals = [candidate['literal'] for candidate in rows[max(0,index-3):index]
                            if candidate['literal'] is not None]
                baseline = literals[-1] if literals else None
            result.append({'kind':kind, 'field':fields[-1], 'weight_per_unit':1,
                           'baseline':baseline, 'offset':row['offset']})
    return result


def extract_nested_fields(rows, accumulator):
    result = []
    for index, row in enumerate(rows[:-2]):
        if row['op'] != 'ldflda' or rows[index+1]['op'] != 'ldfld':
            continue
        if not rows[index+2]['op'].startswith('br') or not next_accumulator_write(
                rows, index, accumulator):
            continue
        result.append({'kind':'nested_boolean', 'field':row['field'],
                       'member':rows[index+1]['field'],
                       'weight':next_numeric_literal(rows,index), 'offset':row['offset']})
    return result


def literal_assignments(rows):
    """Return simple literal-to-field stores exactly as encoded in CIL."""
    result = {}
    for index, row in enumerate(rows):
        if row['op'] != 'stfld' or not row['field'] or index == 0:
            continue
        value = rows[index - 1]['literal']
        if value is not None:
            result.setdefault(row['field'], []).append(value)
    return result


def copied_assignments(rows):
    """Recognize `this.destination = this.source` field-copy sequences."""
    result = {}
    for index, row in enumerate(rows):
        if row['op'] != 'stfld' or index < 2:
            continue
        source = rows[index - 1]
        if source['op'] == 'ldfld' and source['field']:
            result[row['field']] = source['field']
    return result


def verify_caps(rules, setup_rows, override_rows):
    setup = literal_assignments(setup_rows)
    override = literal_assignments(override_rows)
    copies = copied_assignments(override_rows)
    expected_literals = {'maxHealthCap':9, 'MPReserveCap':99}
    for field, value in expected_literals.items():
        if setup.get(field) != [value]:
            raise ValueError(f'Installed {field} cap changed: {setup.get(field)}')
    for field, value in {'fireballLevel':2, 'quakeLevel':2, 'screamLevel':2,
                         'nailSmithUpgrades':4}.items():
        if override.get(field) != [value]:
            raise ValueError(f'Installed {field} override changed: {override.get(field)}')
    if copies.get('maxHealthBase') != 'maxHealthCap':
        raise ValueError('Installed maximum-health override no longer copies its cap')
    if copies.get('MPReserveMax') != 'MPReserveCap':
        raise ValueError('Installed SOUL-reserve override no longer copies its cap')

    caps = {'charmsOwned':rules['charm_count']['maximum'],
            'fireballLevel':2, 'quakeLevel':2, 'screamLevel':2,
            'nailSmithUpgrades':4, 'maxHealthBase':9}
    boolean_total = sum(row['weight'] for row in rules['boolean_rules'])
    integer_total = 0
    contributions = []
    for row in rules['integer_rules']:
        maximum = caps[row['field']]
        value = maximum - (row.get('baseline') or 0)
        contribution = value * row['weight_per_unit']
        integer_total += contribution
        contributions.append({'field':row['field'], 'maximum':maximum,
            'baseline':row.get('baseline'), 'maximum_contribution':contribution})
    nested_total = sum(row['weight'] for row in rules['nested_boolean_rules'])
    soul_total = max(row['weight'] for row in rules['soul_vessel_rule']['cases'])
    maximum = boolean_total + integer_total + nested_total + soul_total
    return {'status':'verified_from_installed_cil', 'maximum':maximum,
        'boolean_or_group_contribution':boolean_total,
        'integer_contributions':contributions,
        'nested_godhome_contribution':nested_total,
        'soul_vessel_contribution':soul_total,
        'cap_evidence':{
            'PlayerData.SetupNewPlayerData':expected_literals,
            'PlayerData.AddGGPlayerDataOverrides':{
                'literal_assignments':{field:values[0] for field,values in override.items()
                    if field in ('fireballLevel','quakeLevel','screamLevel','nailSmithUpgrades')},
                'field_copies':{'maxHealthBase':'maxHealthCap',
                                'MPReserveMax':'MPReserveCap'}}}}


def extract_rules(completion_rows, charm_rows):
    conditional = extract_conditional_fields(completion_rows, 'completionPercentage')
    by_name = {row['field']:row for row in conditional}
    if {'killedNightmareGrimm','destroyedNightmareLantern'} <= by_name.keys():
        conditional = [row for row in conditional if row['field'] not in
                       ('killedNightmareGrimm','destroyedNightmareLantern')]
        conditional.append({'kind':'any_boolean',
            'fields':['killedNightmareGrimm','destroyedNightmareLantern'],
            'weight':1.0, 'source_logic':'first true or second true'})
    direct = extract_direct_fields(completion_rows, 'completionPercentage')
    nested = extract_nested_fields(completion_rows, 'completionPercentage')
    charm_flags = [row['field'] for row in extract_conditional_fields(charm_rows, 'charmsOwned')
                   if row['field'].startswith('gotCharm_')]
    royal = any(row['op'] == 'ldfld' and row['field'] == 'royalCharmState'
                for row in charm_rows)
    if len(set(charm_flags)) != 39 or not royal:
        raise ValueError('Installed CountCharms structure changed')
    if not any(row['field'] == 'MPReserveMax' for row in completion_rows):
        raise ValueError('Installed MPReserveMax completion switch missing')
    return {'charm_count':{'kind':'derived_count','output':'charmsOwned',
                'flags':sorted(set(charm_flags), key=lambda value:int(value.rsplit('_',1)[-1])),
                'royal_charm_rule':'royalCharmState > 2', 'maximum':40,
                'weight_per_charm':1},
            'boolean_rules':conditional, 'integer_rules':direct,
            'nested_boolean_rules':nested,
            'soul_vessel_rule':{'field':'MPReserveMax',
                'cases':[{'value':33,'weight':1},{'value':66,'weight':2},
                         {'value':99,'weight':3}]}}


def attach_player_data(rules, world_catalog):
    index = {row['key']:row for row in world_catalog['player_data']['keys']}
    fields = set(rules['charm_count']['flags'])
    fields.update(row['field'] for row in rules['boolean_rules'] if row['kind']=='boolean')
    for row in rules['boolean_rules']:
        fields.update(row.get('fields', []))
    fields.update(row['field'] for row in rules['integer_rules'])
    fields.update(row['field'] for row in rules['nested_boolean_rules'])
    fields.add(rules['soul_vessel_rule']['field'])
    result = []
    for field in sorted(fields):
        source = index.get(field)
        result.append({'field':field, 'serialized_playmaker':({
            key:source[key] for key in ('producers','consumers','unknown','scene_count',
                                       'source_files','assigned_literals')}
            if source else None),
            'managed_code_producers_consumers':'requires P13/P14 IL call graph'})
    return result


def build(assembly, world_catalog):
    import dnfile
    pe = dnfile.dnPE(str(assembly))
    completion, completion_hash = method_rows(pe, 'PlayerData', 'CountGameCompletion')
    charms, charms_hash = method_rows(pe, 'PlayerData', 'CountCharms')
    setup, setup_hash = method_rows(pe, 'PlayerData', 'SetupNewPlayerData')
    override, override_hash = method_rows(pe, 'PlayerData', 'AddGGPlayerDataOverrides')
    rules = extract_rules(completion, charms)
    maximum = verify_caps(rules, setup, override)
    return {'format':'HKCOMPLETIONCATALOG01',
        'scope':'Installed PlayerData completion calculation; source checklist, not guest implementation',
        'assembly':str(assembly), 'assembly_sha256':hashlib.sha256(assembly.read_bytes()).hexdigest(),
        'method_sha256':{'PlayerData.CountGameCompletion':completion_hash,
                         'PlayerData.CountCharms':charms_hash,
                         'PlayerData.SetupNewPlayerData':setup_hash,
                         'PlayerData.AddGGPlayerDataOverrides':override_hash},
        'rules':rules, 'player_data_cross_reference':attach_player_data(rules,world_catalog),
        'maximum_verification':maximum,
        'limitations':['Managed-code producer/consumer access is emitted by managed_field_usage.py.',
                       'This report does not grant completion items or implement save state.']}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--world-catalog',type=Path,
                        default=ROOT/'.hkpsx/world-import/world-catalog.json')
    parser.add_argument('--assembly',type=Path)
    parser.add_argument('--output',type=Path,
                        default=ROOT/'.hkpsx/world-import/completion-catalog.json')
    args=parser.parse_args()
    if args.assembly is None:
        doctor=json.loads((ROOT/'.hkpsx/doctor.json').read_text())
        source=Path(doctor['installs'][0]['data_directory'])
        args.assembly=source/'Managed/Assembly-CSharp.dll'
    output=args.output.resolve()
    if not output.is_relative_to((ROOT/'.hkpsx').resolve()):
        parser.error('Output must be inside .hkpsx')
    result=build(args.assembly,json.loads(args.world_catalog.read_text()))
    output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({'boolean_rules':len(result['rules']['boolean_rules']),
        'integer_rules':len(result['rules']['integer_rules']),
        'nested_boolean_rules':len(result['rules']['nested_boolean_rules']),
        'charm_maximum':result['rules']['charm_count']['maximum'],
        'maximum_verification':result['maximum_verification']['status'],
        'maximum':result['maximum_verification']['maximum']},indent=2))


if __name__=='__main__':
    main()
