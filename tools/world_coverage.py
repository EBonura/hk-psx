#!/usr/bin/env python3
"""Summarize verified whole-world import gaps and system scope.

The report is a planning inventory. Role signals are derived from serialized
components and geometry, but do not classify a scene as playable or unused.
"""
import argparse
from collections import Counter
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def require_verified(report):
    coverage = report.get('coverage', {})
    run = report.get('run', {})
    if (run.get('status') != 'verified' or not report.get('inputs_unchanged') or
            not coverage.get('all_scenes_processed') or coverage.get('failed_scenes')):
        raise ValueError('World import is not complete, failure-free and source-verified')


def aggregate_gaps(report):
    gaps = {}
    for scene in report['scenes']:
        for category in ('errors', 'unsupported'):
            for item in scene.get(category, []):
                detail = item.get('error', item.get('reason', 'unspecified'))
                key = (category, item.get('type', 'unknown'), detail)
                row = gaps.setdefault(key, {'category': category, 'type': key[1],
                    'detail': detail, 'instances': 0, 'scene_indices': set(),
                    'example_sources': []})
                row['instances'] += 1
                row['scene_indices'].add(scene['index'])
                source = item.get('source', item.get('id'))
                if source and source not in row['example_sources'] and len(row['example_sources']) < 8:
                    row['example_sources'].append(source)
    rows = []
    for row in gaps.values():
        row['scene_indices'] = sorted(row['scene_indices'])
        row['scene_count'] = len(row['scene_indices'])
        rows.append(row)
    return sorted(rows, key=lambda row: (-row['scene_count'], -row['instances'],
                                         row['category'], row['type'], row['detail']))


ROLE_COMPONENTS = {
    'boss_arena': {'BossSceneController'},
    'cinematic': {'CinematicPlayer', 'CinematicSequence'},
    'bench': {'RestBench'},
    'shop': {'ShopMenuStock'},
    'save_interface': {'SaveSlotButton', 'SaveProfileHealthBar'},
    'menu_interface': {'MenuScreen', 'MenuButtonList', 'MainMenuOptions'},
    'stag_transport': {'SpawnStagMenu', 'StagTravel'},
    'completion_interface': {'GameCompletionScreen'},
}


def role_signals(scene):
    components = scene.get('component_types', {})
    counts = scene.get('counts', {})
    signals = []
    for role, names in ROLE_COMPONENTS.items():
        evidence = {name: components[name] for name in sorted(names) if components.get(name)}
        if evidence:
            signals.append({'role': role, 'evidence': evidence})
    geometry = {key: counts.get(key, 0) for key in
                ('terrain_edges', 'gates', 'camera_locks', 'tilemaps') if counts.get(key, 0)}
    gameplay = {name: components[name] for name in
                ('SceneManager', 'DamageHero', 'HealthManager', 'PlayMakerFSM') if components.get(name)}
    if geometry or any(name in gameplay for name in ('DamageHero', 'HealthManager')):
        signals.append({'role': 'gameplay_candidate', 'evidence': {**geometry, **gameplay}})
    return signals


def transition_summary(graph):
    # host/room_graph.py::finalize already decides what is unresolved, and its
    # list is wider than the edges: a room whose graph scan was unavailable and
    # every per-TransitionPoint read error are in it too, and neither has an
    # edge to rebuild from. Rebuilding this from graph['edges'] dropped both.
    unresolved = graph['unresolved']
    resolutions = Counter(edge['resolution'] for edge in graph['edges'])
    return {'room_count': graph['room_count'], 'edge_count': graph['edge_count'],
            'resolutions': dict(sorted(resolutions.items())),
            'mapped_gate_percent': round(100*resolutions['mapped_gate']/max(1, graph['edge_count']), 3),
            'unresolved_edges': unresolved,
            'unresolved_without_an_edge': sum('resolution' not in row for row in unresolved)}


def system_inventory(report):
    rows = []
    for name, source in report['coverage']['systems'].items():
        scene_indices = sorted(set(source['scene_indices']))
        rows.append({'type': name, 'instances': source['instances'],
                     'scene_count': len(scene_indices), 'scene_indices': scene_indices,
                     'gameplay_support': source.get('gameplay_support', 'not_evaluated')})
    return sorted(rows, key=lambda row: (-row['scene_count'], -row['instances'], row['type']))


def fsm_inventory(report):
    rows = []
    for name, source in report.get('serialized_fsm_actions', {}).items():
        scene_indices = sorted(set(source['scene_indices']))
        rows.append({'action': name, 'instances': source['instances'],
                     'scene_count': len(scene_indices), 'scene_indices': scene_indices,
                     'runtime_support': 'not_evaluated'})
    return sorted(rows, key=lambda row: (-row['scene_count'], -row['instances'], row['action']))


def build(report, graph, dependency=None, scripted_transitions=None):
    require_verified(report)
    scenes = []
    roles = Counter()
    for scene in report['scenes']:
        signals = role_signals(scene)
        roles.update(signal['role'] for signal in signals)
        scenes.append({'index': scene['index'], 'file': scene['file'],
            'scene_name': scene['scene_name'], 'import_status': scene['status'],
            'role_signals': signals, 'final_classification': 'requires_source_or_reference_evidence',
            'counts': scene.get('counts', {})})
    dependencies = None
    if dependency is not None:
        dependencies = {'complete': dependency.get('complete'),
            'source_hash_count': len(dependency.get('source_hashes', {})),
            'textures': len(dependency.get('textures', {})),
            'sprites': len(dependency.get('sprites', {})),
            'animations': len(dependency.get('animations', {})),
            'rooms_with_scan_failure': [room['index'] for room in dependency.get('rooms', [])
                                        if room.get('scan_failed')]}
    scripted = None
    if scripted_transitions is not None:
        if scripted_transitions.get('source_world_fingerprint') != report['fingerprint']:
            raise ValueError('Scripted-transition inventory belongs to a different world import')
        scripted = {key: scripted_transitions.get(key) for key in
                    ('serialized_transition_point_edges', 'scripted_action_count',
                     'action_counts', 'resolution_counts',
                     'dynamic_actions_with_initial_scene_candidates',
                     'dynamic_producer_resolution_counts',
                     'dynamic_gate_producer_resolution_counts')}
        scripted['error_count'] = len(scripted_transitions.get('errors', []))
    return {'format': 'HKWORLD_COVERAGE01',
        'scope': 'Source-derived work inventory; not PS1 packing or gameplay completion',
        'source_import': {'fingerprint': report['fingerprint'], 'run': report['run'],
            'coverage': {key:value for key,value in report['coverage'].items()
                         if key not in ('counts', 'systems')},
            'geometry_counts': report['coverage']['counts']},
        'dependency_inventory': dependencies,
        'scene_role_signal_counts': dict(sorted(roles.items())),
        'scenes': scenes, 'transitions': transition_summary(graph),
        'scripted_transitions': scripted,
        'geometry_gaps': aggregate_gaps(report),
        'systems': system_inventory(report), 'fsm_actions': fsm_inventory(report),
        'limitations': [
            'Role signals are serialized evidence, not a final playable/menu/unused classification.',
            'Literal TransitionPoint targets do not include PlayMaker/runtime destination overrides.',
            'Component and action occurrence does not establish guest implementation or behavior parity.',
            'Inactive alternatives remain included; progression-dependent activation is unresolved.',
            'Source dependency counts are not cooked PS1 RAM, VRAM, SPU or disc costs.',
        ]}


def markdown(data):
    gaps = data['geometry_gaps'][:20]
    systems = data['systems'][:30]
    actions = data['fsm_actions'][:30]
    lines = ['# Whole-world source coverage', '',
        'This is a verified source inventory and prioritized work queue. It is not a claim that the world is packed or playable.', '',
        '## Import and graph', '',
        f"- Scenes processed: {data['source_import']['coverage']['processed_scenes']} / {data['source_import']['coverage']['catalog_scenes']}",
        f"- Scene import failures: {data['source_import']['coverage']['failed_scenes']}",
        f"- Serialized transition edges: {data['transitions']['edge_count']}",
        f"- Edges mapped to one destination gate: {data['transitions']['resolutions'].get('mapped_gate', 0)} ({data['transitions']['mapped_gate_percent']}%)"]
    if data.get('scripted_transitions'):
        scripted = data['scripted_transitions']
        lines.extend([
            f"- Serialized PlayMaker scene-load actions: {scripted['scripted_action_count']}",
            f"- Scripted transition parse errors: {scripted['error_count']}",
            f"- Scripted target resolutions: {json.dumps(scripted['resolution_counts'], sort_keys=True)}",
            f"- Dynamic actions with a catalogued initial-scene candidate: {scripted['dynamic_actions_with_initial_scene_candidates']}",
        ])
    lines.extend(['',
        '## Highest-impact extraction gaps', '',
        '| Scenes | Instances | Category | Type | Detail |', '| ---: | ---: | --- | --- | --- |'])
    for row in gaps:
        detail = row['detail'].replace('|', '\\|').replace('\n', ' ')
        lines.append(f"| {row['scene_count']} | {row['instances']} | {row['category']} | {row['type']} | {detail} |")
    lines.extend(['', '## Widest serialized component types', '',
        '| Scenes | Instances | Type | Runtime support |', '| ---: | ---: | --- | --- |'])
    for row in systems:
        lines.append(f"| {row['scene_count']} | {row['instances']} | {row['type']} | {row['gameplay_support']} |")
    lines.extend(['', '## Widest PlayMaker action names', '',
        '| Scenes | Instances | Action | Runtime support |', '| ---: | ---: | --- | --- |'])
    for row in actions:
        lines.append(f"| {row['scene_count']} | {row['instances']} | {row['action']} | {row['runtime_support']} |")
    lines.extend(['', '## Interpretation', '',
        'Fix reusable gap families in descending scene coverage. Classify scenes and scripted transitions with source/reference evidence before excluding or admitting them. Every runtime system still needs cooking, guest binding and gameplay validation.', ''])
    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--world', type=Path, default=ROOT/'.hkpsx/world-import/report.json')
    parser.add_argument('--graph', type=Path, default=ROOT/'.hkpsx/world-import/room-graph.json')
    parser.add_argument('--dependencies', type=Path, default=ROOT/'.hkpsx/room-inventory.json')
    parser.add_argument('--scripted-transitions', type=Path,
                        default=ROOT/'.hkpsx/world-import/scripted-transitions.json')
    parser.add_argument('--output', type=Path, default=ROOT/'.hkpsx/world-import/system-coverage.json')
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to((ROOT/'.hkpsx').resolve()):
        parser.error('Output must be inside .hkpsx')
    dependency = json.loads(args.dependencies.read_text()) if args.dependencies.is_file() else None
    scripted = (json.loads(args.scripted_transitions.read_text())
                if args.scripted_transitions.is_file() else None)
    data = build(json.loads(args.world.read_text()), json.loads(args.graph.read_text()),
                 dependency, scripted)
    output.write_text(json.dumps(data, indent=2) + '\n')
    summary = output.with_suffix('.md')
    summary.write_text(markdown(data))
    print(json.dumps({'scenes':len(data['scenes']), 'gaps':len(data['geometry_gaps']),
        'systems':len(data['systems']), 'fsm_actions':len(data['fsm_actions']),
        'json':str(output), 'summary':str(summary)}, indent=2))


if __name__ == '__main__':
    main()
