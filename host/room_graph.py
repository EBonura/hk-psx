"""Literal Windows TransitionPoint topology, shared with the room inventory scan.

This is serialized potential adjacency, not a claim that every exit is currently
accessible. Ability/progression checks, PlayMaker scene loads and runtime target
overrides are not interpreted. No missing edge is invented from room naming.
"""
import argparse
import json
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def scene_name(info):
    return info.get('scene_name') or Path(info['path'].replace('\\', '/')).stem


def scan_room(source, serialized_file, scene_info, script_objects=None):
    """Read only TransitionPoint payloads and their referenced native objects.

    `script_objects` is the inventory's already discovered (object, class-name)
    sequence, so assemblies and Mono headers need not be inspected a second time.
    All parse failures remain explicit in the returned local evidence.
    """
    result = {'scene_name': scene_name(scene_info), 'file': scene_info['file'],
              'index': scene_info['index'], 'transitions': [], 'errors': [],
              'unresolved': [], 'playmaker_fsm_count': 0}
    if script_objects is None:
        script_objects = []
        for obj in serialized_file.objects.values():
            if obj.type.name != 'MonoBehaviour':
                continue
            try:
                script_objects.append((obj, source.typename(obj)))
            except Exception as error:
                result['errors'].append({'source_id': source.sid(obj),
                                         'stage': 'class_name', 'error': str(error)})
    native = {}

    def read_native(obj):
        key = source.sid(obj)
        if key not in native:
            native[key] = source.read(obj)
        return native[key]

    def transform(go_obj, go):
        for component in go['m_Component']:
            obj = source.ref(go_obj.assets_file, component.get('component', component))
            if obj.type.name in ('Transform', 'RectTransform'):
                return obj, read_native(obj)
        return None, None

    def active(go_obj, go, visited):
        sid = source.sid(go_obj)
        if sid in visited:
            raise ValueError('cycle in source Transform hierarchy')
        if not go['m_IsActive']:
            return False
        visited.add(sid)
        t_obj, t = transform(go_obj, go)
        if t is None or not t['m_Father']['m_PathID']:
            return True
        parent_t_obj = source.ref(t_obj.assets_file, t['m_Father'])
        parent_t = read_native(parent_t_obj)
        parent_go_obj = source.ref(parent_t_obj.assets_file, parent_t['m_GameObject'])
        return active(parent_go_obj, read_native(parent_go_obj), visited)

    for obj, typename in script_objects:
        if typename == 'PlayMakerFSM':
            result['playmaker_fsm_count'] += 1
        if typename != 'TransitionPoint':
            continue
        sid = source.sid(obj)
        try:
            tree = source.read(obj)
            go_obj = source.ref(obj.assets_file, tree['m_GameObject'])
            go = read_native(go_obj)
            transition = {
                'source_id': sid, 'game_object_id': source.sid(go_obj),
                'gate_name': go['m_Name'],
                'target_scene': tree['targetScene'], 'entry_point': tree['entryPoint'],
                'component_enabled': bool(tree['m_Enabled']),
                'active_in_hierarchy': None,
                'entry_offset': tree.get('entryOffset'),
                'serialized_flags': {key: tree[key] for key in (
                    'isADoor', 'dontWalkOutOfDoor', 'entryDelay', 'alwaysEnterRight',
                    'alwaysEnterLeft', 'hardLandOnExit', 'alwaysUnloadUnusedAssets',
                    'nonHazardGate', 'sceneLoadVisualization', 'customFade', 'forceWaitFetch'
                ) if key in tree},
                'custom_fade_fsm': tree.get('customFadeFSM'),
            }
            result['transitions'].append(transition)
            # A hierarchy error does not erase an otherwise observed target.
            try:
                transition['active_in_hierarchy'] = active(go_obj, go, set())
                t_obj, t = transform(go_obj, go)
                if t is not None:
                    transition['transform_id'] = source.sid(t_obj)
                    transition['local_position'] = t['m_LocalPosition']
            except Exception as error:
                result['errors'].append({'source_id': sid, 'stage': 'hierarchy',
                                         'error': str(error)})
            if not tree['targetScene']:
                result['unresolved'].append({'source_id': sid, 'reason': 'empty targetScene'})
        except Exception as error:
            result['errors'].append({'source_id': sid, 'stage': 'TransitionPoint',
                                     'error': str(error)})
    return result


def finalize(rooms):
    """Resolve exact scene and gate names after the inventory's one shared pass."""
    by_name = defaultdict(list)
    for room in rooms:
        by_name[scene_name(room)].append(room)
    nodes, edges, unresolved = [], [], []
    for room in rooms:
        name = scene_name(room)
        graph = room.get('graph', {})
        node = {key: room[key] for key in ('index', 'file', 'path')}
        node.update(scene_name=name, transition_count=len(graph.get('transitions', [])),
                    neighbours=[], playmaker_fsm_count=graph.get('playmaker_fsm_count', 0))
        if not graph:
            unresolved.append({'source_scene': name, 'source_file': room['file'],
                               'reason': 'room graph scan unavailable'})
        for error in graph.get('errors', []):
            unresolved.append({'source_scene': name, 'source_file': room['file'], **error})
        for transition in graph.get('transitions', []):
            edge = dict(transition, source_scene=name, source_file=room['file'])
            targets = by_name.get(edge['target_scene'], [])
            if not edge['target_scene']:
                edge['resolution'] = 'empty_target'
            elif not targets:
                edge['resolution'] = 'unresolved_scene'
            elif len(targets) != 1:
                edge['resolution'] = 'ambiguous_scene'
                edge['candidate_target_files'] = [target['file'] for target in targets]
            else:
                target = targets[0]
                edge.update(target_file=target['file'], target_index=target['index'])
                # Inactive gates still supply serialized preload candidates;
                # progression availability is deliberately not inferred here.
                node['neighbours'].append(edge['target_scene'])
                gates = [gate['source_id'] for gate in target.get('graph', {}).get('transitions', [])
                         if gate['gate_name'] == edge['entry_point']]
                edge['target_gate_source_ids'] = gates
                edge['resolution'] = ('mapped_gate' if len(gates) == 1
                                      else 'mapped_scene_unresolved_gate')
                if len(gates) > 1:
                    edge['gate_resolution_reason'] = 'ambiguous gate name'
                elif not gates:
                    edge['gate_resolution_reason'] = 'no matching TransitionPoint GameObject name'
            edges.append(edge)
            if edge['resolution'] != 'mapped_gate':
                unresolved.append(edge.copy())
        node['neighbours'] = sorted(set(node['neighbours']))
        nodes.append(node)
    return {
        'format': 'HKROOMGRAPH01',
        'source': 'Windows Unity BuildSettings and serialized TransitionPoint components',
        'coverage': 'Potential adjacency from literal targetScene and entryPoint only',
        'limitations': [
            'No inference of absent neighbours from scene names or map positions',
            'Progression, ability gates and runtime target overrides are not resolved',
            'PlayMaker FSM scene loads, scripted warps and other transition systems are not decoded',
            'Inactive or disabled TransitionPoints remain potential preload candidates with their flags',
        ],
        'room_count': len(nodes), 'edge_count': len(edges),
        'mapped_gate_edges': sum(edge['resolution'] == 'mapped_gate' for edge in edges),
        'mapped_scene_edges': sum('target_file' in edge for edge in edges),
        'rooms': nodes, 'edges': edges, 'unresolved': unresolved,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inventory', type=Path, default=ROOT/'.hkpsx/room-inventory.json')
    parser.add_argument('--out', type=Path, default=ROOT/'.hkpsx/room-graph.json')
    args = parser.parse_args()
    inventory = json.loads(args.inventory.read_text())
    graph = finalize(inventory['rooms'])
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(graph, indent=2))
    print(f"{graph['room_count']} rooms, {graph['edge_count']} literal transition edges; {args.out}")


if __name__ == '__main__':
    main()
