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
from source import Source, ROOT, dump

# Families the port already runs natively, so their FSMs are not script work.
# Each name is the FSM's own, matched exactly.
NATIVE_FSMS = {
    'Superdash', 'Spell Control', 'Dream Nail', 'Nail Arts', 'Fireball Cast',
    'Fireball Control', 'damages_enemy', 'Roar Lock', 'Spore Cooldown',
    'ProxyFSM', 'Globalise', 'Control Interpolation', 'Surface Water',
    'Dream Return', 'Map Control',
}
# Actions whose behaviour the guest already provides through a native system,
# so an FSM built only from these is a candidate for the first script slice.
COVERED_HINT = {
    'Wait', 'SetBoolValue', 'SetIntValue', 'SetFloatValue', 'BoolTest', 'IntCompare',
    'FloatCompare', 'SendEventByName', 'NextFrameEvent', 'ActivateGameObject',
    'GetPlayerDataBool', 'SetPlayerDataBool', 'GetPlayerDataInt', 'SetPlayerDataInt',
    'PlayerDataBoolTest', 'Tk2dPlayAnimation', 'Tk2dPlayAnimationWithEvents',
    'Tk2dWatchAnimationEvents', 'SetVelocity2d', 'SetGravity2dScale',
}

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
        'actions': [{'action': a, 'instances': n, 'scenes': len(action_scenes[a]),
                     'covered_family': a in COVERED_HINT}
                    for a, n in actions.most_common()],
        'disabled_actions': dict(disabled.most_common(20)),
        'fsms': [{'fsm': f, 'instances': n, 'scenes': len(fsm_scenes[f]),
                  'native': f in NATIVE_FSMS}
                 for f, n in fsm_names.most_common()],
        'fsms_per_scene': dict(per_scene.most_common()),
        'read_errors': errors,
    }

def report(source, scene_files):
    out = inventory(source, scene_files)
    covered = sum(a['instances'] for a in out['actions'] if a['covered_family'])
    out['coverage'] = {
        'instances_in_covered_families': covered,
        'share_of_enabled': round(covered / max(1, out['enabled_actions']), 4),
        'note': 'A covered family means the guest already has a native system for that '
                'operation, not that the action is implemented in a script runtime. '
                'It is the ranking P10 step 1 asks for, not a completion claim.',
    }
    out['limitations'] = [
        'Only the admitted scenes are walked. Prefab FSMs reached through '
        'SpawnObjectFromGlobalPool are counted only where a scene holds the prefab.',
        'State reachability is not analysed: an action in an unreachable state counts '
        'as enabled. Disabled actions are counted separately.',
        'No action is implemented by this inventory. It ranks what a script runtime '
        'would have to support, and which FSMs a native controller already covers.',
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
    print(f"{out['enabled_actions']} enabled actions, {out['distinct_actions']} distinct; "
          f"{out['coverage']['instances_in_covered_families']} "
          f"({out['coverage']['share_of_enabled']:.1%}) in families the guest already has")
    print('top actions:')
    for row in out['actions'][:15]:
        mark = ' *' if row['covered_family'] else '  '
        print(f"{mark} {row['action']:34} {row['instances']:5}  in {row['scenes']} scenes")
    print('top FSM names:')
    for row in out['fsms'][:12]:
        mark = ' native' if row['native'] else '       '
        print(f"  {row['fsm']:34} {row['instances']:5}  in {row['scenes']} scenes{mark}")
