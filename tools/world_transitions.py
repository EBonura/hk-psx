#!/usr/bin/env python3
"""Inventory serialized PlayMaker scene transitions without executing FSMs.

Literal targets are resolved against the complete BuildSettings catalog. Fields
bound to FSM variables retain their declared initial values and remain dynamic;
an initial value is evidence, not proof of the value used at runtime.
"""
import argparse
from collections import Counter, defaultdict
import gzip
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'host'))
from focus import action_fields

FORMAT = 'HKSCRIPTEDTRANSITIONS01'
ACTION_TYPES = ('BeginSceneTransition', 'LoadLevel')
LOCAL_WRITER_FIELDS = {
    'GetPlayerDataString': ('storeValue', 'player_data_read', 'stringName'),
    'GetStaticVariable': ('storeValue', 'static_variable_read', 'variableName'),
    'GetConstantsValue': ('storeValue', 'constant_read', 'variableName'),
    'GetFsmString': ('storeValue', 'fsm_variable_read', 'variableName'),
    'SetStringValue': ('stringVariable', 'local_assignment', 'stringValue'),
    'BuildString': ('storeResult', 'local_assignment', None),
}


def variable_table(fsm):
    result = defaultdict(list)
    for category, values in (fsm.get('variables') or {}).items():
        if not isinstance(values, list):
            continue
        for value in values:
            if isinstance(value, dict) and value.get('name'):
                result[value['name']].append({'category': category,
                                              'initial_value': value.get('value')})
    return dict(result)


def binding(field, variables):
    if field is None:
        return {'binding': 'missing'}
    if isinstance(field, dict) and 'useVariable' in field:
        if field['useVariable']:
            name = field.get('name', '')
            return {'binding': 'fsm_variable', 'name': name,
                    'declarations': variables.get(name, [])}
        return {'binding': 'literal', 'value': field.get('value')}
    return {'binding': 'literal', 'value': field}


def variable_reference_name(field):
    """Read both FsmString and generic FsmVar variable references."""
    if not isinstance(field, dict) or not field.get('useVariable'):
        return None
    return field.get('name') or field.get('variableName') or None


def literal_value(field):
    if isinstance(field, dict):
        if field.get('useVariable'):
            return None
        if 'value' in field:
            return field.get('value')
        if field.get('type') == 4:
            return field.get('stringValue')
        return None
    return field


def action_evidence(scene, obj, fsm, go, state, index, action, fields):
    return {'source_scene': scene['scene_name'], 'source_file': scene['file'],
            'fsm_source': obj['source'], 'fsm_name': fsm.get('name', ''),
            'game_object_source': (f"{scene['file']}:" + str(
                (obj['data'].get('m_GameObject') or {}).get('m_PathID', ''))),
            'game_object_name': go.get('m_Name'), 'state': state.get('name', ''),
            'action_index': index, 'action': action}


def scan_dataflow(document):
    """Index serialized writers relevant to variable-bound scene transitions."""
    scene = document['scene']
    game_objects = {row['source'].split(':')[-1]: row['data'] for row in document['objects']
                    if row['type'] == 'GameObject'}
    local, external, player_data, static_data = defaultdict(list), defaultdict(list), \
        defaultdict(list), defaultdict(list)
    for obj in document['objects']:
        if obj['type'] != 'PlayMakerFSM':
            continue
        tree = obj['data']; fsm = tree.get('fsm') or {}
        gid = str((tree.get('m_GameObject') or {}).get('m_PathID', ''))
        go = game_objects.get(gid, {})
        for state in fsm.get('states') or []:
            data = state.get('actionData') or {}
            for index, full_name in enumerate(data.get('actionNames') or []):
                action = full_name.rsplit('.', 1)[-1]
                try:
                    fields = action_fields(data, index)
                except Exception:
                    continue
                base = action_evidence(scene, obj, fsm, go, state, index, action, fields)
                spec = LOCAL_WRITER_FIELDS.get(action)
                if spec:
                    output_field, kind, source_field = spec
                    variable = variable_reference_name(fields.get(output_field))
                    if variable:
                        row = {**base, 'kind': kind, 'variable': variable}
                        if source_field:
                            source = fields.get(source_field)
                            row['source_binding'] = binding(source, {})
                            row['source_key_or_value'] = literal_value(source)
                        local[(obj['source'], variable)].append(row)
                if action == 'SetFsmString':
                    variable = literal_value(fields.get('variableName'))
                    target_fsm = literal_value(fields.get('fsmName'))
                    owner = fields.get('gameObject') or {}
                    # ownerOption 0 addresses the action owner's GameObject. Other
                    # targets stay explicit but cannot be joined by serialization alone.
                    target_gid = gid if owner.get('ownerOption') == 0 else None
                    if variable and target_fsm and target_gid:
                        row = {**base, 'kind': 'fsm_string_write', 'variable': variable,
                               'target_fsm': target_fsm,
                               'value': binding(fields.get('setValue'), {})}
                        external[(target_gid, target_fsm, variable)].append(row)
                if action == 'SetPlayerDataString':
                    key = literal_value(fields.get('stringName'))
                    if key:
                        player_data[key].append({**base, 'kind': 'player_data_write',
                            'key': key, 'value': binding(fields.get('value'), {})})
                if action == 'SetStaticVariable':
                    key = literal_value(fields.get('variableName'))
                    if key:
                        static_data[key].append({**base, 'kind': 'static_variable_write',
                            'key': key, 'value': binding(fields.get('setValue'), {})})
    return local, external, player_data, static_data


def target_resolution(target, scenes_by_name):
    if target['binding'] == 'literal':
        value = target.get('value')
        if not value:
            return {'resolution': 'empty_target'}
        matches = scenes_by_name.get(value, [])
        if len(matches) == 1:
            return {'resolution': 'mapped_scene', 'target_file': matches[0]['file'],
                    'target_index': matches[0]['index']}
        if not matches:
            folded = [row for name, rows in scenes_by_name.items()
                      if name.casefold() == str(value).casefold() for row in rows]
            result = {'resolution': 'unresolved_scene'}
            if folded:
                result['casefold_candidates'] = [row['file'] for row in folded]
            return result
        return {'resolution': 'ambiguous_scene',
                'candidate_target_files': [match['file'] for match in matches]}
    if target['binding'] == 'fsm_variable':
        initial = sorted({json.dumps(row.get('initial_value'), sort_keys=True)
                          for row in target.get('declarations', [])})
        decoded = [json.loads(value) for value in initial]
        result = {'resolution': 'dynamic_variable', 'initial_values': decoded}
        candidates = []
        for value in decoded:
            candidates.extend(scenes_by_name.get(value, []))
        if candidates:
            result['initial_target_candidates'] = [row['file'] for row in candidates]
        return result
    return {'resolution': 'missing_target'}


def scan_components(document, scenes_by_name, gates_by_scene):
    if document.get('format') != 'HKWORLDCOMP01':
        raise ValueError('Unsupported component document format')
    scene = document['scene']
    game_objects = {row['source'].split(':')[-1]: row['data'] for row in document['objects']
                    if row['type'] == 'GameObject'}
    records, errors = [], []
    for obj in document['objects']:
        if obj['type'] != 'PlayMakerFSM':
            continue
        tree = obj['data']
        fsm = tree.get('fsm') or {}
        variables = variable_table(fsm)
        gid = str((tree.get('m_GameObject') or {}).get('m_PathID', ''))
        go = game_objects.get(gid, {})
        for state in fsm.get('states') or []:
            data = state.get('actionData') or {}
            names = data.get('actionNames') or []
            enabled = data.get('actionEnabled') or []
            for index, full_name in enumerate(names):
                action = full_name.rsplit('.', 1)[-1]
                if action not in ACTION_TYPES:
                    continue
                try:
                    fields = action_fields(data, index)
                    target_field = fields.get('sceneName' if action == 'BeginSceneTransition'
                                              else 'levelName')
                    target = binding(target_field, variables)
                    gate = binding(fields.get('entryGateName'), variables)
                    row = {'source_scene': scene['scene_name'], 'source_file': scene['file'],
                           'fsm_source': obj['source'], 'fsm_name': fsm.get('name', ''),
                           'game_object_source': (f"{scene['file']}:" + gid),
                           'game_object_name': go.get('m_Name'),
                           'fsm_component_enabled': bool(tree.get('m_Enabled', True)),
                           'game_object_active_self': bool(go.get('m_IsActive', True)),
                           'state': state.get('name', ''), 'action_index': index,
                           'action': action,
                           'action_enabled': bool(enabled[index]) if index < len(enabled) else None,
                           'target': target, 'entry_gate': gate,
                           'fields': fields}
                    row.update(target_resolution(target, scenes_by_name))
                    if row['resolution'] == 'mapped_scene' and action == 'BeginSceneTransition':
                        if gate['binding'] == 'literal' and gate.get('value'):
                            matches = gates_by_scene.get((target.get('value'), gate['value']), [])
                            row['target_gate_source_ids'] = matches
                            row['resolution'] = ('mapped_gate' if len(matches) == 1 else
                                                 'mapped_scene_unresolved_gate')
                        elif gate['binding'] == 'fsm_variable':
                            row['resolution'] = 'mapped_scene_dynamic_gate'
                    records.append(row)
                except Exception as error:
                    errors.append({'source_scene': scene['scene_name'],
                                   'source_file': scene['file'],
                                   'fsm_source': obj['source'],
                                   'fsm_name': fsm.get('name', ''),
                                   'state': state.get('name', ''),
                                   'action_index': index, 'action': action,
                                   'error': f'{type(error).__name__}: {error}'})
    return records, errors


def mapped_values(values, scenes_by_name):
    result = []
    for value in values:
        if not isinstance(value, str) or not value:
            continue
        matches = scenes_by_name.get(value, [])
        result.append({'value': value, 'target_files': [row['file'] for row in matches],
                       'resolution': ('mapped_scene' if len(matches) == 1 else
                                      'unresolved_scene' if not matches else 'ambiguous_scene')})
    return result


def variable_dataflow(row, field, dataflow, scenes_by_name=None):
    local, external, player_data, static_data = dataflow
    variable = field.get('name', '')
    local_writers = local.get((row['source_file'], row['fsm_source'], variable), [])
    external_writers = external.get((row['source_file'],
                                     row['game_object_source'].split(':')[-1],
                                     row['fsm_name'], variable), [])
    upstream = []
    for writer in local_writers:
        key = writer.get('source_key_or_value')
        if writer['kind'] == 'player_data_read' and key:
            upstream.append({'kind': 'player_data_writers', 'key': key,
                             'count': len(player_data.get(key, [])),
                             'examples': player_data.get(key, [])[:8]})
        elif writer['kind'] == 'static_variable_read' and key:
            upstream.append({'kind': 'static_variable_writers', 'key': key,
                             'count': len(static_data.get(key, [])),
                             'examples': static_data.get(key, [])[:8]})
    declarations = [entry.get('initial_value') for entry in field.get('declarations', [])]
    literals = list(declarations)
    literals.extend(writer.get('value', {}).get('value') for writer in external_writers
                    if writer.get('value', {}).get('binding') == 'literal')
    if local_writers:
        status = 'runtime_storage_or_constant_read'
    elif external_writers:
        status = 'serialized_branch_override'
    elif any(isinstance(value, str) and value for value in declarations):
        status = 'serialized_default_only'
    else:
        status = 'runtime_or_external_unresolved'
    values = sorted(set(value for value in literals if isinstance(value, str) and value))
    return {'status': status, 'local_writers': local_writers,
        'same_object_fsm_writers': external_writers,
        'upstream_serialized_writers': upstream,
        'serialized_candidates':(mapped_values(values, scenes_by_name)
                                 if scenes_by_name is not None else values),
        'owner_task': ('P09/P22 scene-transition runtime' if
                       status == 'runtime_or_external_unresolved' else None)}


def bind_dynamic_producers(records, dataflow, scenes_by_name, gates_by_scene=None):
    counts = Counter(); gate_counts = Counter()
    gates_by_scene = gates_by_scene or {}
    for row in records:
        if row.get('resolution') == 'dynamic_variable':
            flow = variable_dataflow(row, row['target'], dataflow, scenes_by_name)
            flow['serialized_target_candidates'] = flow.pop('serialized_candidates')
            row['target_dataflow'] = flow
            counts[flow['status']] += 1
        if row.get('entry_gate', {}).get('binding') == 'fsm_variable':
            flow = variable_dataflow(row, row['entry_gate'], dataflow)
            flow['serialized_gate_candidates'] = flow.pop('serialized_candidates')
            row['entry_gate_dataflow'] = flow
            gate_counts[flow['status']] += 1
        if row.get('target_dataflow') and row.get('entry_gate_dataflow'):
            candidates = []
            for target in row['target_dataflow']['serialized_target_candidates']:
                for gate in row['entry_gate_dataflow']['serialized_gate_candidates']:
                    matches = gates_by_scene.get((target['value'], gate), [])
                    candidates.append({'target_scene':target['value'], 'entry_gate':gate,
                        'target_file_candidates':target['target_files'],
                        'target_gate_source_ids':matches,
                        'resolution':('mapped_gate' if len(matches)==1 else
                                      'unresolved_gate' if not matches else 'ambiguous_gate')})
            row['serialized_edge_candidates'] = candidates
    return dict(sorted(counts.items())), dict(sorted(gate_counts.items()))


def build(world, graph, component_root):
    coverage = world.get('coverage', {})
    if (world.get('run', {}).get('status') != 'verified' or
            not world.get('inputs_unchanged') or coverage.get('failed_scenes') or
            not coverage.get('all_scenes_processed')):
        raise ValueError('World import must be complete and source-verified')
    catalog = [{'index': row['index'], 'file': row['file'], 'scene_name': row['scene_name']}
               for row in world['scenes']]
    scenes_by_name = defaultdict(list)
    for row in catalog:
        scenes_by_name[row['scene_name']].append(row)
    gates_by_scene = defaultdict(list)
    for edge in graph['edges']:
        gates_by_scene[(edge['source_scene'], edge['gate_name'])].append(edge['source_id'])
    records, errors = [], []
    local = defaultdict(list); external = defaultdict(list)
    player_data = defaultdict(list); static_data = defaultdict(list)
    for scene in catalog:
        path = component_root/scene['file']/'components.json.gz'
        with gzip.open(path, 'rt') as stream:
            document = json.load(stream)
        found, failed = scan_components(document, scenes_by_name, gates_by_scene)
        records.extend(found); errors.extend(failed)
        dl, de, dp, ds = scan_dataflow(document)
        for (fsm_source, variable), values in dl.items():
            local[(scene['file'], fsm_source, variable)].extend(values)
        for (gid, fsm_name, variable), values in de.items():
            external[(scene['file'], gid, fsm_name, variable)].extend(values)
        for key, values in dp.items():
            player_data[key].extend(values)
        for key, values in ds.items():
            static_data[key].extend(values)
    producer_counts, gate_producer_counts = bind_dynamic_producers(records,
        (local, external, player_data, static_data), scenes_by_name, gates_by_scene)
    resolutions = Counter(row['resolution'] for row in records)
    actions = Counter(row['action'] for row in records)
    dynamic_initial_candidates = sum(bool(row.get('initial_target_candidates'))
                                     for row in records
                                     if row['resolution'] == 'dynamic_variable')
    return {'format': FORMAT,
            'scope': 'Serialized PlayMaker scene-load actions; FSM execution not evaluated',
            'source_world_fingerprint': world['fingerprint'],
            'scene_count': len(catalog),
            'serialized_transition_point_edges': graph['edge_count'],
            'scripted_action_count': len(records),
            'action_counts': dict(sorted(actions.items())),
            'resolution_counts': dict(sorted(resolutions.items())),
            'dynamic_actions_with_initial_scene_candidates': dynamic_initial_candidates,
            'dynamic_producer_resolution_counts': producer_counts,
            'dynamic_gate_producer_resolution_counts': gate_producer_counts,
            'player_data_scene_value_keys': {key:len(values) for key,values in sorted(player_data.items())
                                             if 'scene' in key.casefold()},
            'static_scene_value_keys': {key:len(values) for key,values in sorted(static_data.items())
                                        if 'scene' in key.casefold()},
            'actions': records, 'errors': errors,
            'limitations': [
                'Variable-bound destinations retain declarations and serialized producer chains; runtime values remain dynamic.',
                'FSM state reachability, event conditions and action execution are unresolved.',
                'Scripted actions and TransitionPoints may describe the same logical traversal.',
                'LoadLevel actions have no destination gate in their serialized action payload.',
                'Runtime/API-injected values have an owning implementation task instead of an invented destination.',
            ]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--world', type=Path, default=ROOT/'.hkpsx/world-import/report.json')
    parser.add_argument('--graph', type=Path, default=ROOT/'.hkpsx/world-import/room-graph.json')
    parser.add_argument('--components', type=Path, default=ROOT/'.hkpsx/world-import')
    parser.add_argument('--output', type=Path,
                        default=ROOT/'.hkpsx/world-import/scripted-transitions.json')
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to((ROOT/'.hkpsx').resolve()):
        parser.error('Output must be inside .hkpsx')
    data = build(json.loads(args.world.read_text()), json.loads(args.graph.read_text()),
                 args.components)
    output.write_text(json.dumps(data, indent=2) + '\n')
    print(json.dumps({key: data[key] for key in
                      ('scene_count', 'serialized_transition_point_edges',
                       'scripted_action_count', 'action_counts', 'resolution_counts')}, indent=2))


if __name__ == '__main__':
    main()
