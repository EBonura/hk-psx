#!/usr/bin/env python3
"""Attach serialized progression evidence to every world transition route."""
import argparse
from collections import Counter, defaultdict, deque
import gzip
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'host'))
from focus import action_fields
from world_catalog import player_data_direction, player_data_keys

FORMAT = 'HKWORLDPROGRESSION01'
LOAD_ACTIONS = {'BeginSceneTransition','LoadLevel'}


def state_distances(start, adjacency):
    distances = {start:0}; queue = deque([start])
    while queue:
        state = queue.popleft()
        for target in adjacency.get(state, ()):
            if target not in distances:
                distances[target] = distances[state] + 1
                queue.append(target)
    return distances


def fsm_evidence(document):
    """Index PlayerData actions and graph distance around each load action."""
    result = {}; object_actions = defaultdict(list)
    for obj in document['objects']:
        if obj['type'] != 'PlayMakerFSM':
            continue
        tree = obj['data']; fsm = tree.get('fsm') or {}
        states = fsm.get('states') or []
        adjacency = defaultdict(set); reverse = defaultdict(set)
        pd_actions = []
        for state in states:
            for edge in state.get('transitions') or []:
                target = edge.get('toState')
                if target:
                    adjacency[state.get('name','')].add(target)
                    reverse[target].add(state.get('name',''))
            data = state.get('actionData') or {}
            for index, full_name in enumerate(data.get('actionNames') or []):
                action = full_name.rsplit('.',1)[-1]
                if 'PlayerData' not in action:
                    continue
                try:
                    fields = action_fields(data,index)
                    keys = player_data_keys(action,fields,fsm)
                    error = None
                except Exception as exception:
                    fields = {}; keys = []
                    error = f'{type(exception).__name__}: {exception}'
                pd_actions.append({'state':state.get('name',''), 'action_index':index,
                    'action':action, 'direction':player_data_direction(action),
                    'keys':keys, 'parse_error':error})
        gid = str((tree.get('m_GameObject') or {}).get('m_PathID',''))
        object_actions[gid].extend({**row, 'fsm_source':obj['source'],
                                   'fsm_name':fsm.get('name','')} for row in pd_actions)
        for state in states:
            data = state.get('actionData') or {}
            for index, full_name in enumerate(data.get('actionNames') or []):
                action = full_name.rsplit('.',1)[-1]
                if action not in LOAD_ACTIONS:
                    continue
                name = state.get('name','')
                before = state_distances(name,reverse)
                after = state_distances(name,adjacency)
                conditions = [{**row,'state_hops':before[row['state']]}
                              for row in pd_actions
                              if row['direction']=='consumer' and row['state'] in before]
                postconditions = [{**row,'state_hops':after[row['state']]}
                                  for row in pd_actions
                                  if row['direction']=='producer' and row['state'] in after]
                result[(obj['source'],name,index,action)] = {
                    'same_state_conditions':[row for row in conditions if row['state_hops']==0],
                    'reachable_condition_candidates':conditions,
                    'same_state_postconditions':[row for row in postconditions if row['state_hops']==0],
                    'reachable_postcondition_candidates':postconditions,
                    'evidence_scope':'FSM graph reachability only; events and runtime branch outcomes are not executed'}
    return result, object_actions


def condition_status(evidence):
    if evidence['same_state_conditions']:
        return 'same_state_serialized_condition'
    if evidence['reachable_condition_candidates']:
        return 'fsm_reachable_serialized_candidates'
    return 'no_serialized_playerdata_condition'


def postcondition_status(evidence):
    if evidence['same_state_postconditions']:
        return 'same_state_serialized_postcondition'
    if evidence['reachable_postcondition_candidates']:
        return 'fsm_reachable_serialized_candidates'
    return 'no_serialized_playerdata_postcondition'


def one_way_sources(graph):
    scene_pairs = {(row['source_scene'],row.get('target_scene')) for row in graph['edges']
                   if row.get('target_file')}
    return {row['source_id'] for row in graph['edges'] if row.get('target_file') and
            (row.get('target_scene'),row['source_scene']) not in scene_pairs}


def reference_cases(scripted, gates):
    cases = []
    def add(category, row, detail=None):
        if row is None:
            return
        cases.append({'category':category,
            'source_scene':row['source_scene'], 'source_file':row['source_file'],
            'source':row.get('fsm_source') or row.get('source'),
            'state':row.get('state'), 'detail':detail})
    for resolution in sorted({row['resolution'] for row in scripted}):
        add(f'scripted_resolution:{resolution}',
            next(row for row in scripted if row['resolution']==resolution))
    for status in sorted({row.get('target_dataflow',{}).get('status') for row in scripted}
                         - {None}):
        add(f'dynamic_target:{status}',next(row for row in scripted
            if row.get('target_dataflow',{}).get('status')==status))
    for status in sorted({row['progression']['condition_status'] for row in scripted}):
        add(f'scripted_condition:{status}',next(row for row in scripted
            if row['progression']['condition_status']==status))
    add('transition_point:initially_inactive',next((row for row in gates
        if not row['active_hierarchy'] or not row['enabled']),None))
    add('transition_point:one_way_candidate',next((row for row in gates
        if row['one_way_candidate']),None))
    add('transition_point:same_object_playerdata',next((row for row in gates
        if row['same_object_player_data_actions']),None))
    return cases


def build(world_catalog, scripted_transitions, graph, component_root):
    fingerprint = world_catalog['source_world_fingerprint']
    if scripted_transitions['source_world_fingerprint'] != fingerprint:
        raise ValueError('Scripted transition catalog belongs to another world import')
    progression = {}; object_actions = defaultdict(list); errors = []
    for scene in world_catalog['scenes']:
        path = component_root/scene['file']/'components.json.gz'
        with gzip.open(path,'rt') as stream:
            document = json.load(stream)
        try:
            found, objects = fsm_evidence(document)
            for key,value in found.items():
                progression[(scene['file'],)+key] = value
            for gid,values in objects.items():
                object_actions[(scene['file'],gid)].extend(values)
        except Exception as error:
            errors.append({'source_file':scene['file'],
                'error':f'{type(error).__name__}: {error}'})

    scripted = []
    for source in scripted_transitions['actions']:
        row = dict(source)
        key = (row['source_file'],row['fsm_source'],row['state'],
               row['action_index'],row['action'])
        evidence = progression.get(key, {'same_state_conditions':[],
            'reachable_condition_candidates':[], 'same_state_postconditions':[],
            'reachable_postcondition_candidates':[],
            'evidence_scope':'FSM source missing; owned by P02'})
        row['progression'] = {**evidence,
            'condition_status':condition_status(evidence),
            'postcondition_status':postcondition_status(evidence),
            'owner_task':'P09 transitions / P13-P14 progression runtime'}
        scripted.append(row)

    one_way = one_way_sources(graph)
    gates = []
    for source in world_catalog['transition_points']:
        row = dict(source); gid = row['game_object'].split(':')[-1]
        actions = object_actions.get((row['source_file'],gid),[])
        row['same_object_player_data_actions'] = actions
        row['activation_condition_status'] = ('same_object_serialized_candidates'
            if actions else 'runtime_or_hierarchy_condition_unresolved')
        row['one_way_candidate'] = row['source'] in one_way
        row['owner_task'] = 'P09 transitions / P13-P14 progression runtime'
        gates.append(row)

    condition_counts = Counter(row['progression']['condition_status'] for row in scripted)
    post_counts = Counter(row['progression']['postcondition_status'] for row in scripted)
    activation_counts = Counter(row['activation_condition_status'] for row in gates)
    return {'format':FORMAT,
        'scope':'Serialized transition conditions and effects; candidates are not executed reachability',
        'source_world_fingerprint':fingerprint,
        'scripted_transition_count':len(scripted),
        'transition_point_count':len(gates),
        'scripted_condition_counts':dict(sorted(condition_counts.items())),
        'scripted_postcondition_counts':dict(sorted(post_counts.items())),
        'transition_point_activation_counts':dict(sorted(activation_counts.items())),
        'one_way_transition_point_candidates':sum(row['one_way_candidate'] for row in gates),
        'scripted_transitions':scripted, 'transition_points':gates,
        'reference_cases':reference_cases(scripted,gates), 'errors':errors,
        'limitations':[
            'FSM graph reachability lists candidate prerequisites and postconditions; it does not prove which event branch executes.',
            'TransitionPoint conditions outside the same GameObject remain explicitly unresolved for the progression runtime owner.',
            'One-way candidates mean no reverse scene pair exists in serialized TransitionPoints; scripted returns may still exist.',
            'Reference cases identify original source objects for later controlled traces; they are not parity claims.',
        ]}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--world-catalog',type=Path,
                        default=ROOT/'.hkpsx/world-import/world-catalog.json')
    parser.add_argument('--scripted',type=Path,
                        default=ROOT/'.hkpsx/world-import/scripted-transitions.json')
    parser.add_argument('--graph',type=Path,
                        default=ROOT/'.hkpsx/world-import/room-graph.json')
    parser.add_argument('--components',type=Path,default=ROOT/'.hkpsx/world-import')
    parser.add_argument('--output',type=Path,
                        default=ROOT/'.hkpsx/world-import/world-progression.json')
    args=parser.parse_args(); output=args.output.resolve()
    if not output.is_relative_to((ROOT/'.hkpsx').resolve()):
        parser.error('Output must be inside .hkpsx')
    result=build(json.loads(args.world_catalog.read_text()),
        json.loads(args.scripted.read_text()),json.loads(args.graph.read_text()),args.components)
    output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({key:result[key] for key in ('scripted_transition_count',
        'transition_point_count','scripted_condition_counts','scripted_postcondition_counts',
        'transition_point_activation_counts','one_way_transition_point_candidates','errors')},
        indent=2))


if __name__=='__main__':
    main()
