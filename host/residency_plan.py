"""Deterministic offline room dependency plans; never an inferred hardware fit.

The primary unit is a sprite fragment; backing atlases remain extraction inputs.
Source image sizes and hypothetical 4bpp estimates are deliberately not costs.
Only explicit cooked_ram_bytes/cooked_vram_bytes participate in budget sums.
"""
import argparse
import hashlib
import json
from pathlib import Path


def _scene_name(room):
    return room.get('scene_name') or Path(room['path']).stem


def _indexed(records):
    result = {}
    for record in records:
        name = _scene_name(record)
        if name in result:
            raise ValueError(f'ambiguous inventory scene name: {name}')
        result[name] = record
    return result


def _cost(texture, field):
    value = texture.get(field)
    if value is not None and (type(value) is not int or value < 0):
        raise ValueError(f'{field} must be a nonnegative integer or null')
    return value


def _resources(inventory):
    """Use fragments when provided, retaining material-only texture dependencies.

    Legacy/synthetic inventories without a sprite catalog use whole texture IDs
    as an explicitly labelled fallback. Even in fragment mode a missing sprite
    record stays in the dependency set and cannot acquire an invented cost.
    """
    if 'sprites' not in inventory:
        return inventory, 'source_textures_fallback', {}
    catalog = dict(inventory['sprites'])
    planned_rooms = []
    material_only = {}
    for room in inventory['rooms']:
        name = _scene_name(room)
        fragments = set(room.get('sprite_ids', []))
        covered = set().union(*(set(catalog.get(sprite, {}).get('backing_texture_ids', []))
                                for sprite in fragments))
        extra = set(room.get('texture_ids', [])) - covered
        ids = set(fragments)
        if extra:
            material_only[name] = sorted(extra)
        for texture_id in extra:
            key = 'texture:' + texture_id
            if key in inventory['sprites']:
                raise ValueError(f'resource ID collision: {key}')
            # These are still explicit dependencies, but a full retail atlas is
            # not a cooked fragment and has no proven allocation by default.
            catalog[key] = dict(inventory.get('textures', {}).get(texture_id, {}),
                                backing_texture_ids=[texture_id], kind='material_texture')
            ids.add(key)
        planned_rooms.append(dict(room, texture_ids=sorted(ids)))
    return dict(inventory, rooms=planned_rooms, textures=catalog), 'sprite_fragments', material_only


def _budget(ids, textures, field, limit):
    if limit is not None and (type(limit) is not int or limit < 0):
        raise ValueError('budget must be a nonnegative integer or null')
    known = 0
    unknown = []
    for texture_id in sorted(ids):
        cost = _cost(textures.get(texture_id, {}), field)
        if cost is None:
            unknown.append(texture_id)
        else:
            known += cost
    status = ('over_budget' if limit is not None and known > limit else
              'unknown' if unknown or limit is None else 'within_budget')
    return {
        'budget_bytes': limit,
        'known_bytes': known,
        'exact_bytes': None if unknown else known,
        'unknown_resource_ids': unknown,
        'status': status,
    }


def plan_inventory(inventory, graph, current, ram_budget_bytes=None, vram_budget_bytes=None):
    """Plan current + immediate serialized neighbours without discarding assets.

    Budgets apply to the listed texture dependency union, not total game RAM or
    a physical VRAM packing proof. Unknown inventories and allocations remain
    explicit blockers. No read is scheduled at an invented distance or time.
    """
    original_rooms = _indexed(inventory['rooms'])
    original_textures = inventory.get('textures', {})
    inventory, resource_kind, material_only = _resources(inventory)
    rooms = _indexed(inventory['rooms'])
    nodes = _indexed(graph['rooms'])
    if current not in rooms:
        raise ValueError(f'current scene is not inventoried: {current}')
    if current not in nodes:
        raise ValueError(f'current scene has no graph node: {current}')
    neighbours = sorted(set(nodes[current].get('neighbours', [])) - {current})
    selected = [current] + neighbours
    missing_rooms = [name for name in selected if name not in rooms]
    incomplete_rooms = {name for name, room in rooms.items()
                        if room.get('scan_failed') or room.get('unsupported')
                        or room.get('unresolved_behavior_types')}
    dependencies = {name: set(rooms[name].get('texture_ids', []))
                    for name in selected if name in rooms}
    all_ids = set().union(*dependencies.values())
    textures = inventory.get('textures', {})
    missing_textures = sorted(all_ids - textures.keys())
    current_ids = dependencies[current]

    def budget_for(ids):
        return {
            'ram': _budget(ids, textures, 'cooked_ram_bytes', ram_budget_bytes),
            'vram': _budget(ids, textures, 'cooked_vram_bytes', vram_budget_bytes),
        }

    def window(name):
        node = nodes.get(name)
        names = [name] + sorted(set(node.get('neighbours', [])) - {name}) if node else [name]
        missing = [scene for scene in names if scene not in rooms]
        ids = set().union(*(set(rooms[scene].get('texture_ids', []))
                            for scene in names if scene in rooms))
        incomplete = [scene for scene in names if scene in incomplete_rooms]
        return names, ids, missing, incomplete, node is not None

    def window_budget(ids, incomplete):
        result = budget_for(ids)
        if incomplete:
            for budget in result.values():
                budget['exact_bytes'] = None
                if budget['status'] != 'over_budget':
                    budget['status'] = 'unknown'
        return result

    def admission(budgets):
        return ('admitted_for_listed_resource_costs' if
                all(item['status'] == 'within_budget' for item in budgets.values())
                else 'unadmitted')

    # Unknown neighbour contents can exceed even an otherwise exact known sum.
    budgets = window_budget(all_ids, bool(missing_rooms) or bool(incomplete_rooms.intersection(selected)))
    required_by = {texture_id: [name for name in selected
                               if texture_id in dependencies.get(name, ())]
                   for texture_id in all_ids}
    fetch_order = []
    # Current-room textures are a strict priority tier. Each source texture is
    # emitted once, even when several neighbours require the same atlas.
    for priority, ids in [(0, current_ids), (1, all_ids - current_ids)]:
        for texture_id in sorted(ids):
            record = textures.get(texture_id, {})
            fetch_order.append({
                'resource_id': texture_id,
                'priority': priority,
                'required_by': required_by[texture_id],
                'fetch_point': 'before_current_room_entry' if priority == 0 else 'current_room_entry_prefetch_neighbours',
                'cooked_ram_bytes': _cost(record, 'cooked_ram_bytes'),
                'cooked_vram_bytes': _cost(record, 'cooked_vram_bytes'),
                'backing_texture_ids': sorted(record.get('backing_texture_ids', [])),
            })
    transitions = []
    for neighbour in neighbours:
        target = dependencies.get(neighbour)
        evidence = [edge for edge in graph.get('edges', [])
                    if edge.get('source_scene') == current
                    and edge.get('target_scene') == neighbour]
        evidence.sort(key=lambda edge: (str(edge.get('source_id', '')),
                                       str(edge.get('entry_point', '')),
                                       json.dumps(edge, sort_keys=True)))
        target_names, target_ids, target_missing, target_incomplete, graph_available = window(neighbour)
        target_budgets = window_budget(target_ids, bool(target_missing) or bool(target_incomplete) or not graph_available)
        target_admission = admission(target_budgets)
        transitions.append({
            'target_scene': neighbour,
            'inventory_available': target is not None,
            'keep_resource_ids': sorted(current_ids & target) if target is not None else None,
            'fetch_resource_ids': sorted(target - current_ids) if target is not None else None,
            'source_room_only_resource_ids': sorted(current_ids - target) if target is not None else None,
            'target_budget': budget_for(target) if target is not None else None,
            'target_window_rooms': target_names,
            'target_window_resource_ids': sorted(target_ids),
            'target_window_missing_room_inventories': target_missing,
            'target_window_incomplete_room_inventories': target_incomplete,
            'target_window_graph_available': graph_available,
            'keep_window_resource_ids': sorted(all_ids & target_ids),
            'fetch_window_resource_ids': sorted(target_ids - all_ids),
            'candidate_release_resource_ids': sorted(all_ids - target_ids),
            # Missing target dependencies must never cause a destructive eviction
            # based on an incomplete set difference.
            'release_after_window_change_resource_ids': (
                sorted(all_ids - target_ids) if target_admission != 'unadmitted' else None),
            'target_window_budgets': target_budgets,
            'target_window_admission': target_admission,
            'gate_evidence': evidence,
            'trigger': 'cross serialized gate to target; approach distance and CD deadline unimplemented',
            'fetch_point': 'target_room_entry_prefetch_neighbours',
            'release_rule': 'after source room rendering finishes and target data is verified',
        })
    # Keep a plan per authored directed gate, rather than collapsing several
    # gates to the same target scene. Runtime can therefore retain the source
    # gate identity and its conditional flags while using the same cooked
    # dependency sets. Unknown target content never produces an executable
    # eviction list.
    directed_edges = []
    edges = [edge for edge in graph.get('edges', [])
             if edge.get('source_scene') == current]
    edges.sort(key=lambda edge: (str(edge.get('source_id', '')),
                                 str(edge.get('target_scene', '')),
                                 str(edge.get('entry_point', '')),
                                 json.dumps(edge, sort_keys=True)))
    for edge in edges:
        target_name = edge.get('target_scene')
        target_names, target_ids, target_missing, target_incomplete, graph_available = window(target_name)
        target_known = target_name in dependencies
        target_admission = admission(window_budget(target_ids,
                                                   bool(target_missing) or bool(target_incomplete)
                                                   or not graph_available))
        source_window_ids = all_ids
        directed_edges.append({
            'source_id': edge.get('source_id'),
            'source_scene': current,
            'target_scene': target_name,
            'entry_point': edge.get('entry_point'),
            'resolution': edge.get('resolution'),
            'target_gate_source_ids': sorted(edge.get('target_gate_source_ids', [])),
            'serialized_flags': edge.get('serialized_flags', {}),
            'conditional': bool(edge.get('serialized_flags')) or edge.get('resolution') not in (None, 'mapped_gate'),
            'target_window_rooms': target_names,
            'target_window_resource_ids': sorted(target_ids),
            'target_window_missing_room_inventories': target_missing,
            'target_window_incomplete_room_inventories': target_incomplete,
            'target_window_graph_available': graph_available,
            'target_window_admission': target_admission,
            'priority_tiers': {
                'current_visible': sorted(current_ids),
                'target_visible': sorted((dependencies.get(target_name, set())) - source_window_ids),
                'target_neighbours': sorted(target_ids - set(dependencies.get(target_name, set())) - source_window_ids),
            } if target_known else None,
            'fetch_before_gate_crossing_resource_ids': sorted(target_ids - source_window_ids) if target_known else None,
            'keep_during_transition_resource_ids': sorted(source_window_ids & target_ids) if target_known else None,
            'evict_after_target_admission_resource_ids': (
                sorted(source_window_ids - target_ids) if target_admission != 'unadmitted' else None),
            'fetch_deadline': 'before_gate_crossing',
            'publish_deadline': 'before_target_first_draw',
            'eviction_rule': ('after_target_metadata_geometry_atlas_and_coverage_verify'
                              if target_admission != 'unadmitted' else 'hold_until_target_dependencies_are_complete'),
        })
    unsupported = {name: sorted(rooms[name].get('unsupported', []),
                               key=lambda item: json.dumps(item, sort_keys=True)) for name in selected
                   if name in rooms and rooms[name].get('unsupported')}
    unresolved = [edge for edge in graph.get('unresolved', [])
                  if edge.get('source_scene') in selected]
    unresolved.sort(key=lambda edge: json.dumps(edge, sort_keys=True))
    backing = set().union(*(set(original_rooms[name].get('texture_ids', []))
                           for name in selected if name in original_rooms))
    return {
        'version': 2,
        'current_scene': current,
        'neighbours': neighbours,
        'selected_rooms': selected,
        'planning_unit': resource_kind,
        'backing_texture_union': sorted(backing),
        'missing_backing_texture_records': sorted(backing - original_textures.keys()),
        'material_texture_dependencies': {name: material_only[name] for name in selected if name in material_only},
        'room_resource_ids': {name: sorted(dependencies[name]) for name in selected if name in dependencies},
        'resource_union': sorted(all_ids),
        'fetch_order': fetch_order,
        'transitions': transitions,
        'directed_edges': directed_edges,
        'budget_scope': 'listed resource dependencies with explicit cooked allocation costs only',
        'budgets': budgets,
        'admission': admission(budgets),
        'missing_room_inventories': missing_rooms,
        'incomplete_room_inventories': sorted(incomplete_rooms.intersection(selected)),
        'missing_resource_records': missing_textures,
        'unsupported_room_content': unsupported,
        'unresolved_behavior_counts': {
            name: {kind: detail.get('count', len(detail.get('source_ids', [])))
                   for kind, detail in sorted(rooms[name].get('unresolved_behavior_types', {}).items())}
            for name in selected if name in rooms and rooms[name].get('unresolved_behavior_types')},
        'unresolved_transitions': unresolved,
        'cooked_coverage': {name: rooms[name].get('cooked_coverage', 'unknown')
                            for name in selected if name in rooms},
        'full_room_fit_proven': False,
        'runtime_fetch_implemented': False,
        'limitations': [
            'Serialized neighbours are potential transitions; runtime gates and availability are not evaluated.',
            'Source fragment IDs require cooked-bank mapping before they can drive guest reads.',
            'No unknown dependency is discarded and no source-size estimate is counted as cooked residency.',
            'Audio, other assets, actor state, code, stack, allocator layout and transition overlap still need budgets.',
            'Fetch points name lifecycle events; traversal distance, CD seek timing and guest execution remain unimplemented.',
        ],
    }


def plan_all(inventory, graph, ram_budget_bytes=None, vram_budget_bytes=None):
    """Precompute each inventoried scene's current+neighbours window."""
    rooms = _indexed(inventory['rooms'])
    nodes = _indexed(graph['rooms'])
    return {
        'version': 2,
        'source_inventory_complete': inventory.get('complete'),
        'plans': {name: plan_inventory(inventory, graph, name, ram_budget_bytes, vram_budget_bytes)
                  for name in sorted(rooms) if name in nodes},
        'missing_graph_nodes': sorted(rooms.keys() - nodes.keys()),
        'full_game_fit_proven': False,
        'runtime_fetch_implemented': False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inventory', type=Path, required=True)
    parser.add_argument('--graph', type=Path, required=True)
    parser.add_argument('--current', help='one scene; omit to precompute every inventoried scene')
    parser.add_argument('--ram-budget', type=int)
    parser.add_argument('--vram-budget', type=int)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    inventory_bytes = args.inventory.read_bytes()
    graph_bytes = args.graph.read_bytes()
    inventory = json.loads(inventory_bytes)
    graph = json.loads(graph_bytes)
    plan = (plan_inventory(inventory, graph, args.current, args.ram_budget, args.vram_budget)
            if args.current else plan_all(inventory, graph, args.ram_budget, args.vram_budget))
    plan['inputs'] = {
        'inventory': {'path': str(args.inventory.resolve()), 'sha256': hashlib.sha256(inventory_bytes).hexdigest()},
        'graph': {'path': str(args.graph.resolve()), 'sha256': hashlib.sha256(graph_bytes).hexdigest()},
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(plan, indent=2, sort_keys=True) + '\n')
    if args.current:
        print(f'{args.current}: {len(plan["selected_rooms"])} rooms, '
              f'{len(plan["resource_union"])} unique resource dependencies; '
              f'RAM {plan["budgets"]["ram"]["status"]}, VRAM {plan["budgets"]["vram"]["status"]}')
    else:
        print(f'Precomputed {len(plan["plans"])} room windows; full-game fit unproven.')


if __name__ == '__main__':
    main()
