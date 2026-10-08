#!/usr/bin/env python3
"""Inventory exact managed reads, writes and callers for progression fields."""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FORMAT = 'HKMANAGEDFIELDUSAGE01'
FIELD_OPS = {'ldfld':'read', 'ldflda':'address', 'stfld':'write',
             'ldsfld':'read', 'ldsflda':'address', 'stsfld':'write'}
CALL_OPS = {'call', 'callvirt', 'newobj'}


def full_type_name(row):
    namespace = str(getattr(row, 'TypeNamespace', ''))
    name = str(getattr(row, 'TypeName', ''))
    return f'{namespace}.{name}' if namespace else name


def owner_maps(pe):
    """Return metadata-row identity maps without guessing from token names."""
    field_owners = {}; method_owners = {}
    for typ in pe.net.mdtables.TypeDef.rows:
        owner = full_type_name(typ)
        for ref in typ.FieldList:
            field_owners[id(ref.row)] = owner
        for ref in typ.MethodList:
            method_owners[id(ref.row)] = owner
    return field_owners, method_owners


def token_row(pe, token):
    table = pe.net.mdtables.tables.get(token.table)
    if table is None or token.rid < 1 or token.rid > len(table.rows):
        return None
    return table.rows[token.rid - 1]


def member_parent_name(parent):
    if parent is None or parent.row is None:
        return None
    row = parent.row
    if hasattr(row, 'TypeName'):
        return full_type_name(row)
    return str(getattr(row, 'Name', '')) or None


def method_target(pe, token, method_owners):
    """Resolve MethodDef/MemberRef/MethodSpec operands into stable identities."""
    row = token_row(pe, token)
    if row is None:
        return {'token':f'0x{token.table:02x}:{token.rid}', 'resolved':False}
    if token.table == 0x2b:  # MethodSpec wraps MethodDefOrRef.
        wrapped = getattr(row, 'Method', None)
        wrapped_row = wrapped.row if wrapped is not None else None
        if wrapped_row is None:
            return {'token':f'0x{token.table:02x}:{token.rid}', 'resolved':False}
        table = 0x06 if wrapped_row.__class__.__name__ == 'MethodDefRow' else 0x0a
        rows = pe.net.mdtables.tables[table].rows
        rid = next((i for i, candidate in enumerate(rows, 1)
                    if candidate is wrapped_row), None)
        if rid is None:
            return {'token':f'0x{token.table:02x}:{token.rid}', 'resolved':False}
        from dncil.clr.token import Token
        target = method_target(pe, Token((table << 24) | rid), method_owners)
        target['method_spec_token'] = f'0x{token.table:02x}:{token.rid}'
        return target
    if token.table == 0x06:
        owner = method_owners.get(id(row))
    elif token.table == 0x0a:
        owner = member_parent_name(getattr(row, 'Class', None))
    else:
        owner = None
    return {'token':f'0x{token.table:02x}:{token.rid}', 'resolved':bool(owner),
            'declaring_type':owner, 'method':str(getattr(row, 'Name', ''))}


def completion_fields(completion):
    rules = completion['rules']; fields = set(rules['charm_count']['flags'])
    fields.add(rules['charm_count']['output'])
    for row in rules['boolean_rules']:
        if row['kind'] == 'boolean':
            fields.add(row['field'])
        fields.update(row.get('fields', []))
    fields.update(row['field'] for row in rules['integer_rules'])
    fields.add(rules['soul_vessel_rule']['field'])
    fields.update(row['field'] for row in rules['nested_boolean_rules'])
    return fields


def scan_assembly(assembly, wanted_fields):
    import dnfile
    from dncil.cil.body.reader import read_method_body_from_bytes
    pe = dnfile.dnPE(str(assembly)); tables = pe.net.mdtables.tables
    field_owners, method_owners = owner_maps(pe)
    player_fields = {}
    for rid, row in enumerate(tables[0x04].rows, 1):
        name = str(row.Name)
        if field_owners.get(id(row)) == 'PlayerData' and name in wanted_fields:
            player_fields[rid] = name
    missing = sorted(wanted_fields - set(player_fields.values()))
    if missing:
        raise ValueError(f'Completion fields missing from PlayerData: {missing}')

    methods = []; all_calls = []; method_identities = {}; parse_errors = []
    for rid, row in enumerate(tables[0x06].rows, 1):
        if not row.Rva:
            continue
        try:
            body = read_method_body_from_bytes(pe.get_data(row.Rva, 100000))
        except Exception as error:
            parse_errors.append({'method_token':f'0x06:{rid}',
                'declaring_type':method_owners.get(id(row)), 'method':str(row.Name),
                'error':f'{type(error).__name__}: {error}'})
            continue
        identity = {'method_token':f'0x06:{rid}',
                    'declaring_type':method_owners.get(id(row)), 'method':str(row.Name)}
        method_identities[identity['method_token']] = identity
        accesses = []; calls = []
        for instruction in body.instructions:
            op = instruction.opcode.name
            token = instruction.operand
            if op in FIELD_OPS and getattr(token, 'table', None) == 0x04:
                field = player_fields.get(token.rid)
                if field:
                    accesses.append({'field':field, 'operation':FIELD_OPS[op],
                                     'opcode':op, 'offset':instruction.offset})
            if op in CALL_OPS and hasattr(token, 'table'):
                call = {'opcode':op, 'offset':instruction.offset,
                        **method_target(pe, token, method_owners)}
                calls.append(call)
                if call.get('token','').startswith('0x06:'):
                    all_calls.append({'caller':identity['method_token'],
                                      'callee':call['token'], 'opcode':op,
                                      'offset':instruction.offset})
        if accesses:
            raw = pe.get_data(row.Rva, body.size)
            methods.append({**identity,
                'method_sha256':hashlib.sha256(raw).hexdigest(),
                'accesses':accesses, 'calls':calls})
    callers = defaultdict(list)
    for edge in all_calls:
        caller = method_identities.get(edge['caller'])
        if caller:
            callers[edge['callee']].append({**caller, 'opcode':edge['opcode'],
                                             'offset':edge['offset']})
    for method in methods:
        method['direct_managed_callers'] = sorted(callers[method['method_token']],
            key=lambda row:(row['declaring_type'] or '', row['method'], row['offset']))
    return methods, all_calls, parse_errors


def build_field_index(fields, methods):
    index = {field:{'field':field, 'readers':[], 'writers':[],
                    'address_users':[]} for field in sorted(fields)}
    for method in methods:
        grouped = defaultdict(list)
        for access in method['accesses']:
            grouped[(access['field'], access['operation'])].append(access['offset'])
        identity = {key:method[key] for key in
                    ('method_token','declaring_type','method','method_sha256')}
        for (field, operation), offsets in grouped.items():
            bucket = {'read':'readers','write':'writers','address':'address_users'}[operation]
            index[field][bucket].append({**identity, 'offsets':offsets})
    return [index[field] for field in sorted(index)]


def field_user_call_graph(methods, all_calls):
    """Keep exact calls among methods that directly use completion fields."""
    target_tokens = {row['method_token'] for row in methods}
    return [row for row in all_calls
            if row['caller'] in target_tokens and row['callee'] in target_tokens]


def build(assembly, completion):
    fields = completion_fields(completion)
    methods, all_calls, errors = scan_assembly(assembly, fields)
    field_index = build_field_index(fields, methods)
    serialized = {row['field']:row['serialized_playmaker']
                  for row in completion['player_data_cross_reference']}
    for row in field_index:
        row['serialized_playmaker'] = serialized.get(row['field'])
    counts = Counter()
    for row in field_index:
        for key in ('readers','writers','address_users'):
            if row[key]:
                counts[f'fields_with_{key}'] += 1
        if not row['writers']:
            counts['fields_without_direct_managed_writers'] += 1
    return {'format':FORMAT,
        'scope':'Direct installed Assembly-CSharp CIL access to completion-contributing PlayerData fields',
        'assembly':str(assembly),
        'assembly_sha256':hashlib.sha256(assembly.read_bytes()).hexdigest(),
        'completion_catalog_method_sha256':completion['method_sha256'],
        'field_count':len(fields), 'method_count':len(methods),
        'summary':dict(sorted(counts.items())), 'fields':field_index,
        'methods':methods, 'field_method_edges':sum(len(row['accesses']) for row in methods),
        'field_user_call_edges':field_user_call_graph(methods, all_calls),
        'direct_callers_of_field_users':sum(len(row['direct_managed_callers'])
                                            for row in methods),
        'parse_errors':errors,
        'limitations':[
            'This is exhaustive for direct CIL field operands in Assembly-CSharp; reflection and native/plugin access are not inferred.',
            'Address access is retained separately because ldflda may mutate a nested struct or pass a field by reference.',
            'Serialized PlayMaker counts are joined from the completion catalog; full action evidence remains in world-catalog.json.',
        ]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--completion', type=Path,
                        default=ROOT/'.hkpsx/world-import/completion-catalog.json')
    parser.add_argument('--assembly', type=Path)
    parser.add_argument('--output', type=Path,
                        default=ROOT/'.hkpsx/world-import/managed-field-usage.json')
    args = parser.parse_args()
    if args.assembly is None:
        doctor = json.loads((ROOT/'.hkpsx/doctor.json').read_text())
        args.assembly = Path(doctor['installs'][0]['data_directory'])/'Managed/Assembly-CSharp.dll'
    output = args.output.resolve()
    if not output.is_relative_to((ROOT/'.hkpsx').resolve()):
        parser.error('Output must be inside .hkpsx')
    result = build(args.assembly, json.loads(args.completion.read_text()))
    output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key:result[key] for key in
        ('field_count','method_count','field_method_edges','summary','parse_errors')}, indent=2))


if __name__ == '__main__':
    main()
