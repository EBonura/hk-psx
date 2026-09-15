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
    for scene in catalog:
        path = component_root/scene['file']/'components.json.gz'
        with gzip.open(path, 'rt') as stream:
            document = json.load(stream)
        found, failed = scan_components(document, scenes_by_name, gates_by_scene)
        records.extend(found); errors.extend(failed)
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
            'actions': records, 'errors': errors,
            'limitations': [
                'Variable-bound destinations retain declarations and initial values only.',
                'FSM state reachability, event conditions and action execution are unresolved.',
                'Scripted actions and TransitionPoints may describe the same logical traversal.',
                'LoadLevel actions have no destination gate in their serialized action payload.',
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
