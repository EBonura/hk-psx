"""Report the placed Crossroads actors `actors.py::actor_sources` still refuses.

Every admitted family owns a recognizer (host/runner.py, host/climber.py,
host/vengefly.py, host/gruzzer.py, host/baldur.py, host/aspid.py) and a contract
in docs/. This module is the other half of that pair: for each placement no
recognizer accepts it records where the placement is, which FSM or component
drives it, the serialized constants a controller would need, its animation clips
and the exact error every recognizer raised. It admits nothing, cooks nothing
and writes no guest table.

Values are copied out of the source, never inferred. A parameter this reader
cannot decode is reported as its raw bytes under `unreadable` rather than
guessed at, so a missing constant stays visible instead of becoming a plausible
wrong number.
"""
import argparse
import collections
import json
import struct
from pathlib import Path

from actors import _component_records, actor_sources
from quality import SCENE_TABLE
from regions import fixed_regions
from scene import Scene
from source import ROOT, Source

# The gates in actor_sources that decide which recognizer is even attempted.
# Keeping them here (rather than calling actor_sources' private path) means the
# report names the recognizer that refused and the ones that never ran.
RECOGNIZERS = (
    ('walker_control', lambda sc, gid, kinds, name: True),
    ('climber', lambda sc, gid, kinds, name: 'Climber' in kinds),
    ('vengefly', lambda sc, gid, kinds, name: 'LineOfSightDetector' in kinds and name.startswith('Buzzer')),
    ('gruzzer', lambda sc, gid, kinds, name: name.startswith('Fly') and 'PlayMakerCollisionStay2D' in kinds),
    ('baldur', lambda sc, gid, kinds, name: name.startswith('Roller') and 'LineOfSightDetector' in kinds),
    ('aspid', lambda sc, gid, kinds, name: name.startswith('Spitter') and 'PersonalObjectPool' in kinds),
    ('hatcher_baby', lambda sc, gid, kinds, name: name.startswith('Hatcher Baby') and 'ObjectBounce' in kinds),
    ('hatcher', lambda sc, gid, kinds, name: name.startswith('Hatcher')
        and not name.startswith('Hatcher Baby') and 'LineOfSightDetector' in kinds),
    ('zombie_shield', lambda sc, gid, kinds, name: name.startswith('Zombie Shield') and 'Walker' in kinds),
    ('pigeon', lambda sc, gid, kinds, name: name.startswith('Pigeon')
        and 'EnemyDeathEffectsNoEffect' in kinds),
    ('runner', lambda sc, gid, kinds, name: 'Walker' in kinds),
)
# The recognizer entry point, where it is not `<module>.recognize`.
ENTRY_POINTS = {'hatcher_baby': ('hatcher', 'recognize_baby')}

# Serialized components whose fields a controller would have to honour. Each one
# is reported through component_fields, which keeps the authored numbers.
CONSTANT_COMPONENTS = (
    'Walker', 'Climber', 'Rigidbody2D', 'Recoil', 'DamageHero', 'ObjectBounce',
    'EnemyDeathEffects', 'EnemyDeathEffectsUninfected', 'EnemyDreamnailReaction',
    'LineOfSightDetector', 'PersistentBoolItem', 'FSMActivator', 'SetZ',
    'DeactivateIfPlayerdataTrue', 'DeactivateIfPlayerdataFalse', 'PersonalObjectPool',
)
HEADER_FIELDS = ('m_GameObject', 'm_Script', 'm_Name')
VECTOR_KEYS = ({'x', 'y'}, {'x', 'y', 'z'})
# HealthManager carries a hundred effect references; only the vital numbers and
# the flags generated_actor_specs refuses on are worth reporting.
HEALTH_FIELDS = ('hp', 'enemyType', 'smallGeoDrops', 'mediumGeoDrops', 'largeGeoDrops',
                 'megaFlingGeo', 'invincible', 'invincibleFromDirection', 'hasSpecialDeath',
                 'hasAlternateHitAnimation', 'damageOverride', 'ignoreKillAll')
# The family a placement belongs to, by the first matching source name prefix, so
# the more specific prefix comes first ('Hatcher Baby' before 'Hatcher'). The
# catalog is closed on purpose: a new placement lands in 'unclassified' and stays
# visible rather than being folded into a neighbour.
FAMILY_PREFIXES = (
    ('Hatcher Baby', 'Hatcher Baby'), ('Hatcher', 'Hatcher'), ('Zombie Shield', 'Zombie Shield'),
    ('Zombie Guard', 'Zombie Guard'), ('Zombie Leaper', 'Zombie Leaper'), ('Zombie Runner', 'Zombie Runner'),
    ('Zombie Myla', 'Zombie Myla'), ('Mender Bug', 'Mender Bug'), ('Climber', 'Climber'),
    ('Egg Sac', 'Egg Sac'), ('Blocker', 'Blocker'), ('Giant Fly', 'Giant Fly'), ('Mawlek', 'Mawlek'),
    ('Pigeon', 'Pigeon'),
)


def family(name):
    for prefix, label in FAMILY_PREFIXES:
        if name.startswith(prefix):
            return label
    return 'unclassified'


WIDTHS = {'f': 4, 'i': 4, '?': 1}


def _compact(raw, code, count=1):
    """A PlayMaker compact parameter: `count` values, then useVariable and name."""
    size = WIDTHS[code] * count
    if len(raw) < size + 1:
        raise ValueError('truncated compact FSM parameter')
    body, flag, name = raw[:size], raw[size], raw[size + 1:].decode('utf8')
    if flag:
        return {'variable': name}
    values = list(struct.unpack('<' + code * count, body))
    return values[0] if count == 1 else values


def action_constants(data, index):
    """Every parameter of one action, decoding the kinds this build serializes.

    `focus.action_fields` deliberately reads only the narrow set the recognizers
    match on; a report needs the vectors and enum ordinals too. Anything this
    reader does not know stays raw so a caller cannot mistake a guess for a
    source value.
    """
    start = data['actionStartIndex'][index]
    end = (data['actionStartIndex'][index + 1] if index + 1 < len(data['actionNames'])
           else len(data['paramName']))
    fields, unreadable = {}, {}
    for i in range(start, end):
        name = data['paramName'][i] or str(i)
        kind, position, size = data['paramDataType'][i], data['paramDataPos'][i], data['paramByteDataSize'][i]
        raw = bytes(data['byteData'][position:position + size])
        try:
            if kind == 1 and size == 1:
                value = bool(raw[0])
            elif kind == 7 and size == 4:  # serialized enum ordinal; the enum name is not stored
                value = {'enum_ordinal': struct.unpack('<i', raw)[0]}
            elif kind == 15 and size:
                value = _compact(raw, 'f')
            elif kind == 16 and size:
                value = _compact(raw, 'i')
            elif kind == 17 and size:
                value = _compact(raw, '?')
            elif kind == 23:
                value = raw.decode('utf8')
            elif kind == 28 and size:  # FsmVector3
                value = _compact(raw, 'f', 3)
            elif kind == 37 and size:  # FsmVector2
                value = _compact(raw, 'f', 2)
            elif kind in (18, 20, 21, 31, 39):
                value = data[{18: 'fsmStringParams', 20: 'fsmOwnerDefaultParams', 21: 'functionCallParams',
                              31: 'fsmEventTargetParams', 39: 'fsmVarParams'}[kind]][position]
            elif kind == 19:
                value = data['fsmGameObjectParams'][position]
            elif kind == 24:
                value = data['fsmObjectParams'][position]
            else:
                unreadable[name] = {'kind': kind, 'bytes': raw.hex()}
                continue
        except (ValueError, struct.error, IndexError, UnicodeDecodeError) as error:
            unreadable[name] = {'kind': kind, 'bytes': raw.hex(), 'error': str(error)}
            continue
        fields[name] = value
    return fields, unreadable


def component_fields(tree):
    """A component's authored numbers, without its prefab reference fields.

    EnemyDeathEffects alone carries around forty PPtrs to shared effect prefabs;
    they say nothing about behaviour and would swamp the report. A reference
    field is kept only as its serialized path ID so a follow-up pass can chase
    it, which is what `corpsePrefab` needs.
    """
    fields = {}
    for key, value in tree.items():
        if key in HEADER_FIELDS:
            continue
        if isinstance(value, dict):
            if set(value) in VECTOR_KEYS:
                fields[key] = [value[axis] for axis in sorted(value)]
            elif set(value) == {'m_FileID', 'm_PathID'} and value['m_PathID']:
                fields[key] = {'reference': f"{value['m_FileID']}:{value['m_PathID']}"}
            continue
        if isinstance(value, (bool, int, float, str)):
            fields[key] = value
    return fields


def _literal(value):
    """Keep scalars and vectors; drop the per-scene object references.

    A compact parameter bound to an FSM variable is kept as `{'variable': name}`
    so a reader can tell "this constant lives in the variable table" apart from
    "this constant is missing".
    """
    if isinstance(value, dict):
        if 'variable' in value or 'enum_ordinal' in value:
            return value
        if 'useVariable' in value:
            if value['useVariable']:
                return {'variable': value.get('name', '')}
            inner = value.get('value')
            return inner if isinstance(inner, (bool, int, float, str)) else None
        return None
    if isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, list) and all(isinstance(item, (bool, int, float, str)) for item in value):
        return value
    return None


def fsm_constants(fsm):
    """States, transitions and the literal parameters of every action.

    Disabled actions are kept and marked: several of these FSMs carry a disabled
    action next to the enabled one that replaced it (the Hatcher's spawn counter,
    for instance), and dropping them hides why the live gate differs.
    """
    states = []
    for state in fsm['states']:
        data = state['actionData']
        actions = []
        for index, name in enumerate(data['actionNames']):
            fields, unreadable = action_constants(data, index)
            literals = {key: _literal(value) for key, value in fields.items()}
            entry = {'action': name.rsplit('.', 1)[-1], 'enabled': bool(data['actionEnabled'][index]),
                     'fields': {key: value for key, value in literals.items() if value is not None}}
            if unreadable:
                entry['unreadable'] = unreadable
            actions.append(entry)
        states.append({'state': state['name'],
                       'transitions': [(t['fsmEvent']['name'], t['toState']) for t in state['transitions']],
                       'actions': actions})
    variables = {}
    for group in fsm['variables'].values():
        if not isinstance(group, list):
            continue
        for variable in group:
            if isinstance(variable, dict) and 'name' in variable:
                value = _literal(variable.get('value'))
                if value is not None:
                    variables[variable['name']] = value
    return {'name': fsm['name'], 'start_state': fsm['startState'], 'variables': variables,
            'global_transitions': [(t['fsmEvent']['name'], t['toState']) for t in fsm.get('globalTransitions', [])],
            'states': states}


def refusals(sc, actor):
    """Re-run every actor_sources gate and record what each recognizer said.

    actor_sources keeps one `movement_error`, overwritten by whichever gate ran
    last, so the stored string usually names a recognizer that was never a
    candidate. The per-recognizer list is the honest answer.
    """
    gid = actor['game_object']
    records = _component_records(sc, gid)
    kinds = {kind for _, kind, _ in records}
    name = sc.gos[gid]['m_Name']
    results = []
    for label, gate in RECOGNIZERS:
        if not gate(sc, gid, kinds, name):
            results.append({'recognizer': label, 'attempted': False,
                            'reason': 'component/name gate in actor_sources does not select this recognizer'})
            continue
        try:
            if label == 'walker_control':
                from actors import walker_control as recognize
            else:
                module, entry = ENTRY_POINTS.get(label, (label, 'recognize'))
                recognize = getattr(__import__(module), entry)
            recognize(sc, actor)
            results.append({'recognizer': label, 'attempted': True, 'error': None})
        except (ValueError, KeyError, StopIteration) as error:
            results.append({'recognizer': label, 'attempted': True, 'error': str(error)})
    return results


def clip_table(source, sc, records):
    """Every clip of the placement's animation library, in library order."""
    animators = [tree for _, kind, tree in records if kind == 'tk2dSpriteAnimator']
    if not animators:
        return None
    animator = animators[0]
    library_object = source.ref(sc.file, animator['library'])
    library = source.read(library_object)
    return {'library_source': source.sid(library_object),
            'default_clip_index': animator.get('defaultClipId'),
            'play_automatically': bool(animator.get('playAutomatically')),
            'realtime': bool(animator.get('isRealtime')),
            'clips': [{'index': index, 'name': clip['name'], 'frames': len(clip['frames']),
                       'fps': clip['fps'], 'wrap_mode': clip['wrapMode'], 'loop_start': clip.get('loopStart'),
                       'trigger_frames': [i for i, frame in enumerate(clip['frames']) if frame.get('triggerEvent')]}
                      for index, clip in enumerate(library['clips']) if clip['name']]}


def child_shapes(sc, gid):
    """Direct children carrying a collider or an FSM: alert ranges, hit boxes, pools."""
    parent = sc.go_transform[gid]
    result = []
    for tid, transform in sc.transforms.items():
        if transform['m_Father']['m_PathID'] != parent:
            continue
        child = transform['m_GameObject']['m_PathID']
        records = _component_records(sc, child)
        kinds = [kind for _, kind, _ in records]
        if not ({'BoxCollider2D', 'CircleCollider2D', 'PolygonCollider2D', 'PlayMakerFSM'} & set(kinds)):
            continue
        entry = {'name': sc.gos[child]['m_Name'], 'active': sc.active(child),
                 'world_position': list(sc.point(child)),
                 'local_scale': [transform['m_LocalScale'][k] for k in 'xyz'],
                 'local_position': [transform['m_LocalPosition'][k] for k in 'xyz'],
                 'components': sorted(collections.Counter(kinds).items()), 'colliders': [], 'fsms': []}
        for _, kind, tree in records:
            if kind == 'CircleCollider2D':
                entry['colliders'].append({'type': kind, 'radius': tree['m_Radius'],
                                           'offset': [tree['m_Offset'][k] for k in 'xy'],
                                           'trigger': bool(tree['m_IsTrigger']), 'enabled': bool(tree['m_Enabled'])})
            elif kind == 'BoxCollider2D':
                entry['colliders'].append({'type': kind, 'size': [tree['m_Size'][k] for k in 'xy'],
                                           'offset': [tree['m_Offset'][k] for k in 'xy'],
                                           'trigger': bool(tree['m_IsTrigger']), 'enabled': bool(tree['m_Enabled'])})
            elif kind == 'PlayMakerFSM':
                entry['fsms'].append(fsm_constants(tree['fsm']))
        result.append(entry)
    return result


def describe(source, sc, actor, boxes):
    """One unadmitted placement, with everything a controller author would ask for."""
    gid = actor['game_object']
    records = _component_records(sc, gid)
    matrix = sc.world(sc.go_transform[gid])
    x, y = actor['position'][:2]
    inside = [index for index, box in boxes if box[0] <= x <= box[2] and box[1] <= y <= box[3]]
    components = {}
    for _, kind, tree in records:
        if kind in CONSTANT_COMPONENTS:
            components.setdefault(kind, []).append(component_fields(tree))
    bodies = [{'type': kind, 'size': [tree['m_Size'][k] for k in 'xy'] if kind == 'BoxCollider2D' else None,
               'radius': tree.get('m_Radius') if kind == 'CircleCollider2D' else None,
               'offset': [tree['m_Offset'][k] for k in 'xy'], 'edge_radius': tree.get('m_EdgeRadius'),
               'trigger': bool(tree['m_IsTrigger']), 'enabled': bool(tree['m_Enabled'])}
              for _, kind, tree in records
              if kind in ('BoxCollider2D', 'CircleCollider2D', 'PolygonCollider2D')]
    return {
        'family': family(sc.gos[gid]['m_Name']), 'name': sc.gos[gid]['m_Name'], 'source': actor['source'],
        'game_object': gid, 'layer': sc.gos[gid]['m_Layer'], 'world_position': list(actor['position']),
        'world_basis': [[matrix[row][column] for column in range(2)] for row in range(2)],
        'in_packed_regions': inside,
        'health_manager': {key: actor['health_manager'][key] for key in HEALTH_FIELDS
                           if key in actor['health_manager']},
        'contact_damage': actor.get('DamageHero', {}).get('damageDealt'),
        'body_colliders': bodies, 'components': components,
        'component_counts': sorted(collections.Counter(kind for _, kind, _ in records).items()),
        'fsms': [{'source': f'{Path(sc.file.name).name}:{cid}', **fsm_constants(tree['fsm'])}
                 for cid, kind, tree in records if kind == 'PlayMakerFSM'],
        'animation': clip_table(source, sc, records),
        'children': child_shapes(sc, gid),
        'refusals': refusals(sc, actor),
        'stored_movement_error': actor.get('movement_error'),
        'stored_pending_movement_error': actor.get('pending_movement_error'),
    }


def scene_report(source, row, boxes):
    """Parse one scene once and describe every actor it refuses."""
    sc = Scene(source, row['file'])
    try:
        actors = actor_sources(sc)
    except ValueError as error:
        return {'scene_name': row['scene_name'], 'scene_file': row['file'], 'error': str(error),
                'placements': 0, 'admitted': 0, 'unadmitted': []}
    admitted = [a for a in actors if a['movement_supported']]
    unadmitted = [describe(source, sc, a, boxes) for a in actors if not a['movement_supported']]
    report = {'scene_name': row['scene_name'], 'scene_file': row['file'],
              'placements': len(actors), 'admitted': len(admitted),
              'admitted_kinds': sorted(collections.Counter(a['movement_control']['kind'] for a in admitted).items()),
              'unadmitted': unadmitted}
    del sc
    return report


def remaining_actors(source=None):
    """Whole-catalog report. Each scene in the packed table is parsed once."""
    source = source or Source()
    regions = fixed_regions()
    by_scene = collections.defaultdict(list)
    for index, region in enumerate(regions):
        by_scene[region['scene_file']].append(
            (index, region.get('interaction_bounds', region['activation_bounds'])))
    scenes = [scene_report(source, row, by_scene[row['file']]) for row in SCENE_TABLE]
    families = collections.defaultdict(lambda: {'placed': 0, 'parked': 0, 'scenes': [], 'refusals': set()})
    for scene in scenes:
        for actor in scene['unadmitted']:
            entry = families[actor['family']]
            key = 'placed' if actor['in_packed_regions'] else 'parked'
            entry[key] += 1
            entry['scenes'].append({'scene': scene['scene_name'], 'source': actor['source'],
                                   'name': actor['name'], 'position': [round(v, 3) for v in actor['world_position'][:2]],
                                    'placed': bool(actor['in_packed_regions'])})
            for result in actor['refusals']:
                if result.get('error'):
                    entry['refusals'].add(f"{result['recognizer']}: {result['error']}")
    summary = {name: {**entry, 'refusals': sorted(entry['refusals'])}
               for name, entry in sorted(families.items())}
    return {
        'scene_count': len(scenes), 'region_count': len(regions),
        'placements': sum(scene['placements'] for scene in scenes),
        'admitted': sum(scene['admitted'] for scene in scenes),
        'unadmitted_in_packed_regions': sum(entry['placed'] for entry in summary.values()),
        'unadmitted_parked_outside_regions': sum(entry['parked'] for entry in summary.values()),
        'families': summary, 'scenes': scenes,
        'policy': ['Placement counts use regions.fixed_regions() interaction bounds, the same envelope cook.py '
                   'passes to actor_sources; an actor outside every envelope is parked, not placed.',
                   'Refusals re-run each recognizer behind its actor_sources gate, so a recognizer that was '
                   'never a candidate is reported as not attempted instead of as a failure.',
                   'No constant is derived: a parameter this reader cannot decode is reported as raw bytes.'],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / '.hkpsx/remaining-actors.json')
    parser.add_argument('--family', help='print the full record of one family instead of the summary')
    arguments = parser.parse_args()
    report = remaining_actors()
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(json.dumps(report, indent=1, default=lambda value: list(value)) + '\n')
    print(f"{report['admitted']} of {report['placements']} placed actors admitted across "
          f"{report['scene_count']} scenes; {report['unadmitted_in_packed_regions']} refused inside a packed "
          f"region and {report['unadmitted_parked_outside_regions']} parked outside every region.")
    for name, entry in report['families'].items():
        where = ', '.join(sorted({row['scene'] for row in entry['scenes'] if row['placed']})) or 'none'
        print(f"  {name}: {entry['placed']} placed ({where}), {entry['parked']} parked")
        for reason in entry['refusals']:
            print(f"      {reason}")
    if arguments.family:
        for scene in report['scenes']:
            for actor in scene['unadmitted']:
                if actor['family'] == arguments.family:
                    print(json.dumps(actor, indent=1, default=lambda value: list(value)))
    print('Wrote', arguments.output)


if __name__ == '__main__':
    main()
