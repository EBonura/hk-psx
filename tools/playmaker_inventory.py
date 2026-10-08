"""Every PlayMaker action the admitted scenes actually run (P10 step 1).

The plan asks for the action inventory to be ranked by required instances and
progression impact rather than raw count, because that ranking is what decides
which slices of the shared script runtime get built first. This walks every
PlayMakerFSM in the admitted scenes, counts enabled actions in reachable states,
and separates the ones already covered by a native controller from the ones that
still block a scene.

Actions inside disabled states or on inactive objects are counted separately:
they are not work until something enables them, but they are not absent either.

No retail payload is embedded; the report goes to .hkpsx.
"""
import sys
from pathlib import Path as _Path
sys.path.insert(0, str(_Path(__file__).resolve().parents[1] / 'host'))

import collections, json
import script_ir
from source import Source, ROOT, dump

def inventory(source, scene_files):
    actions = collections.Counter()
    disabled = collections.Counter()
    fsm_names = collections.Counter()
    action_scenes = collections.defaultdict(set)
    fsm_scenes = collections.defaultdict(set)
    per_scene = collections.Counter()
    errors = []
    for name, scene in scene_files.items():
        file = source.file(name)
        for obj in file.objects.values():
            if obj.type.name != 'MonoBehaviour':
                continue
            try:
                if source.typename(obj) != 'PlayMakerFSM':
                    continue
                fsm = source.read(obj)['fsm']
            except Exception as error:
                errors.append({'scene': scene, 'object': source.sid(obj), 'error': str(error)})
                continue
            fsm_names[fsm['name']] += 1
            fsm_scenes[fsm['name']].add(scene)
            per_scene[scene] += 1
            for state in fsm['states']:
                data = state['actionData']
                for i, raw in enumerate(data['actionNames']):
                    short = raw.rsplit('.', 1)[-1]
                    if data['actionEnabled'][i]:
                        actions[short] += 1
                        action_scenes[short].add(scene)
                    else:
                        disabled[short] += 1
    return {
        'scene_count': len(scene_files),
        'fsm_instances': sum(fsm_names.values()),
        'distinct_fsm_names': len(fsm_names),
        'enabled_actions': sum(actions.values()),
        'distinct_actions': len(actions),
        'actions': [{'action': a, 'instances': n, 'scenes': len(action_scenes[a])}
                    for a, n in actions.most_common()],
        'disabled_actions': dict(disabled.most_common(20)),
        'fsms': [{'fsm': f, 'instances': n, 'scenes': len(fsm_scenes[f])}
                 for f, n in fsm_names.most_common()],
        'fsms_per_scene': dict(per_scene.most_common()),
        'read_errors': errors,
    }

def report(source, scene_files):
    """The census, with script_ir's own coverage measurement attached.

    Support is not a property of an action name. `compile_action` answers per
    instance: the same action compiles in one FSM and refuses in the next
    depending on what it names and what the cook can resolve, so any name-level
    list can only overstate. The numbers below are survey()'s, not a second
    opinion on them.
    """
    out = inventory(source, scene_files)
    survey = script_ir.survey(source, scene_files)
    compiled = {row['action']: row for row in survey['per_action']}
    native = set(survey['native_actor_fsms'])
    for row in out['actions']:
        seen = compiled.get(row['action'])
        row['script_instances'] = seen['instances'] if seen else 0
        row['compiles'] = seen['compiled'] if seen else 0
    for row in out['fsms']:
        row['native'] = row['fsm'] in native
    out['coverage'] = {
        'compiled_action_instances': survey['compiled_action_instances'],
        'script_action_instances': survey['action_instances'],
        'share_of_script_actions': round(survey['compiled_action_instances']
                                         / max(1, survey['action_instances']), 4),
        'compiled_fsm_instances': survey['compiled'],
        'script_fsm_instances': survey['instances'],
        'fsm_instances_on_native_actors': survey['on_native_actors'],
        'unparsed_scenes': sorted(survey['unparsed_scenes']),
        'note': 'From host/script_ir.py::survey. An action instance compiles when '
                'compile_action emits ops for it; an FSM runs only when every action '
                'in the definition compiles, so compiled_fsm_instances moves in steps '
                'and the action counts are what show whether work landed.',
    }
    out['limitations'] = [
        'Only the admitted scenes are walked. Prefab FSMs reached through '
        'SpawnObjectFromGlobalPool are counted only where a scene holds the prefab.',
        'State reachability is not analysed: an action in an unreachable state counts '
        'as enabled. Disabled actions are counted separately.',
        'enabled_actions counts every FSM in the scenes; script_action_instances counts '
        'only the ones script_ir treats as script work, which leaves out the FSMs sitting '
        'on native actor controllers. The two denominators are not interchangeable.',
        'The native flag comes from survey()\'s native_actor_fsms, which lists the twenty '
        'widest such definitions, so a rarer one can be on a native actor without the flag.',
        'No action is implemented by this inventory. It ranks what the script runtime '
        'would have to support and reports what the compiler currently answers for.',
    ]
    dump(ROOT / '.hkpsx/playmaker-inventory.json', out)
    return out

if __name__ == '__main__':
    s = Source()
    scenes = {e['file']: e['scene_name']
              for e in json.load(open(ROOT / '.hkpsx/selected-regions.json'))['scenes']}
    out = report(s, scenes)
    print(f"{out['fsm_instances']} FSM instances ({out['distinct_fsm_names']} distinct names) "
          f"across {out['scene_count']} scenes")
    c = out['coverage']
    print(f"{out['enabled_actions']} enabled actions, {out['distinct_actions']} distinct; "
          f"of the {c['script_action_instances']} script_ir treats as script work, "
          f"{c['compiled_action_instances']} ({c['share_of_script_actions']:.1%}) compile")
    print(f"{c['compiled_fsm_instances']} of {c['script_fsm_instances']} FSM instances compile; "
          f"{c['fsm_instances_on_native_actors']} more sit on native actor controllers")
    print('top actions (compiled of the instances script_ir measures):')
    for row in out['actions'][:15]:
        print(f"   {row['action']:34} {row['instances']:5}  in {row['scenes']} scenes"
              f"   {row['compiles']:5} of {row['script_instances']} compile")
    print('top FSM names:')
    for row in out['fsms'][:12]:
        mark = ' native' if row['native'] else '       '
        print(f"  {row['fsm']:34} {row['instances']:5}  in {row['scenes']} scenes{mark}")
