"""What the 45 admitted scenes actually contain of P12's breakable families.

The package plan lists ten families from the whole game. This measures which of
them the admitted catalogue holds, and in what form, so the work is sized
against the world the port ships rather than against the list.

Families are recognized by the components an instance carries, never by its
scene or object name: the same barrel appears under a dozen names and the same
name appears with different parts. Reads the cached per-scene component
inventories under `.hkpsx/world-import/<file>/components.json.gz`, so it runs
without the Windows source. `--fsm` additionally compiles the PlayMaker-authored
families through host/script_ir.py, which does need the source.

`--terrain` asks the question that turned out to matter more than the counts:
which FSM definitions own a collider host/world_geometry.py bakes into terrain?
Those are the instances where the serialized state and the state the original
loads with can disagree, and the cook reads the serialized one.

`--recognize` runs the compiled per-family subsets in host/breakables.py over
the catalogue and reports what each admits and what it refuses, with the source
reason. It needs the Windows source.
"""
import argparse
import collections
import gzip
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

# A family is a component signature plus, where the behaviour lives in a
# PlayMaker FSM rather than a C# component, the authored object name that
# selects that FSM. Naming is only ever a selector for an FSM-authored family,
# because those carry no distinguishing component of their own.
COMPONENT_FAMILIES = {
    'breakable': ('Breakable',),
    'infected_vine': ('BreakableInfectedVine',),
    'grass': ('GrassCut',),
    'town_grass': ('TownGrass',),
    'grass_sprite': ('GrassSpriteBehaviour',),
    'geo_rock': ('GeoRock',),
    'pogo_target': ('TinkEffect',),
    'persistent_flag': ('PersistentBoolItem',),
    'persistent_counter': ('PersistentIntItem',),
    'stalactite': ('StalactiteControl',),
    'lifeblood_cocoon': ('HealthCocoon',),
    'bounce_shroom': ('BounceShroom',),
    'simple_rock': ('SimpleRock',),
    'pushable_rubble': ('PushableRubble',),
    'debris_piece': ('DebrisPiece',),
    'pole_top': ('BreakablePoleTopLand',),
    'pole_simple': ('BreakablePoleSimple',),
    'transition_point': ('TransitionPoint',),
    'playerdata_gate': ('DeactivateIfPlayerdataTrue', 'DeactivateIfPlayerdataFalse',
                        'DeactivateInDarknessWithoutLantern'),
}
FSM_FAMILIES = {
    'hidden_wall': r'^Breakable Wall',
    'cracked_floor': r'^Break Floor',
    'ability_floor': r'^Quake Floor',
    'multi_hit_gate': r'^Battle Gate',
    'toll_gate': r'^Toll Gate$|^Toll Gate \d',
    'persistent_switch': r'^Toll Gate Switch$|^Gate Switch$',
    'ability_barrier': r'^Dream Gate Set Lock',
    'secret_mask': r'^Secret Mask|^Remasker|^Inverse Remasker|^Mask Bottom|^Mask \d',
    'inspect_sign': r'Sign Post$',
}
# The plan's list, against what the admitted catalogue holds. A family named
# here with no entry is absent from these 45 scenes, which is a result, not a
# gap: the port cannot implement a contract it has no instance of.
PLAN_FAMILIES = {
    'grass': ('grass', 'town_grass', 'grass_sprite'),
    'pots': (),
    'signs': ('inspect_sign',),
    'doors': ('transition_point',),
    'multi-hit gates': ('multi_hit_gate', 'toll_gate'),
    'Geo rocks': ('geo_rock',),
    'hidden walls': ('hidden_wall',),
    'cracked floors': ('cracked_floor',),
    'ability-only barriers': ('ability_barrier', 'ability_floor', 'playerdata_gate'),
    'persistent switches': ('persistent_switch', 'persistent_flag', 'persistent_counter'),
}


def scene_inventory(file):
    path = ROOT / '.hkpsx/world-import' / file / 'components.json.gz'
    return json.loads(gzip.open(path).read())


def census(scenes):
    """(family -> instances, family -> scenes, breakable name -> count)."""
    wanted = {name: family for family, names in COMPONENT_FAMILIES.items() for name in names}
    patterns = {family: re.compile(rule) for family, rule in FSM_FAMILIES.items()}
    counts = collections.Counter()
    where = collections.defaultdict(collections.Counter)
    for scene in scenes:
        data = scene_inventory(scene['file'])
        types = {record['source'].split(':')[1]: record['type'] for record in data['objects']}
        for record in data['objects']:
            if record['type'] != 'GameObject':
                continue
            name = re.sub(r'\s*\(\d+\)$', '', record['data']['m_Name']).strip()
            present = {types.get(str(part['component']['m_PathID']))
                       for part in record['data']['m_Component']}
            families = {wanted[typ] for typ in present if typ in wanted}
            if 'PlayMakerFSM' in present:
                families |= {family for family, rule in patterns.items() if rule.search(name)}
            for family in families:
                counts[family] += 1
                where[family][scene['scene_name']] += 1
    return counts, where


def report(scenes):
    counts, where = census(scenes)
    lines = [f'{len(scenes)} admitted scenes', '', 'families present:']
    for family, total in counts.most_common():
        lines.append(f'  {total:5d} in {len(where[family]):2d} scenes  {family}')
    lines += ['', "the package plan's list:"]
    for plan, families in PLAN_FAMILIES.items():
        found = [(family, counts[family], len(where[family])) for family in families if counts[family]]
        if not found:
            lines.append(f'  {plan:24s} ABSENT from the admitted catalogue')
            continue
        detail = ', '.join(f'{family} {total} in {rooms} scenes' for family, total, rooms in found)
        lines.append(f'  {plan:24s} {detail}')
    return counts, where, '\n'.join(lines)


def fsm_support(scenes, families):
    """Per FSM-authored instance, the actions host/script_ir.py cannot compile."""
    from source import Source
    from scene import Scene
    import script_ir
    patterns = {family: re.compile(rule) for family, rule in FSM_FAMILIES.items() if family in families}
    source = Source()
    rows = []
    for desc in scenes:
        loaded = Scene(source, desc['file'])
        resolver = script_ir.SceneObjects(loaded)
        for path_id, (typ, data) in sorted(loaded.objects.items()):
            if typ != 'PlayMakerFSM':
                continue
            gid = data['m_GameObject']['m_PathID']
            name = re.sub(r'\s*\(\d+\)$', '', loaded.gos.get(gid, {}).get('m_Name', '')).strip()
            family = next((f for f, rule in patterns.items() if rule.search(name)), None)
            if family is None:
                continue
            fsm = data['fsm']
            missing = script_ir.unsupported_actions(fsm, owner_id=gid, objects=resolver)
            rows.append({'scene': desc['scene_name'], 'object': name, 'family': family,
                         'fsm': fsm['name'], 'source': f"{desc['file']}:{path_id}",
                         'actions': sum(1 for _ in script_ir.enabled_actions(fsm)),
                         'unsupported': sum(missing.values()),
                         'unsupported_actions': dict(missing.most_common())})
    return rows


def scene_geometry(file):
    path = ROOT / '.hkpsx/world-import' / file / 'geometry.json.gz'
    return json.loads(gzip.open(path).read())


# Actions that can put a terrain collider somewhere other than where it was
# serialized: the collider setters PlayMaker declares, the ones that take the
# whole object away, and SetParent, which moves the collider in the world.
# SetProperty is on the list because it can reach any field at all. This is a
# screen, not a proof: an owner with none of these is one whose serialized state
# the cook can trust, and an owner with any of them needs its own recognizer
# before anyone can say which state the room loads with.
COLLIDER_MUTATORS = frozenset({
    'SetCollider', 'SetBoxColliderTrigger', 'SetPolygonCollider', 'SetCircleCollider',
    'SetMeshCollider', 'BoundsBoxCollider', 'BoxColliderOffset',
    'ActivateGameObject', 'ActivateAllChildren', 'DestroySelf', 'DestroyObject',
    'DestroyAllChildren', 'SetParent', 'SetProperty'})


def enabled_action_names(fsm):
    for state in fsm['states']:
        data = state['actionData']
        for index, raw in enumerate(data['actionNames']):
            if data['actionEnabled'][index]:
                yield raw.rsplit('.', 1)[-1]


def fsm_owned_terrain(scenes):
    """Per FSM definition, the terrain colliders and edges its object owns.

    host/world_geometry.py makes terrain from a collider's serialized enabled /
    trigger / layer, which is the state the editor left. An FSM that moves its
    own collider on load therefore reaches the guest as whatever it was saved
    as, and the difference is a wall that is there or is not. This is the list
    of places that can happen, measured rather than guessed.
    """
    rows = []
    for scene in scenes:
        data = scene_inventory(scene['file'])
        geometry = scene_geometry(scene['file'])
        definitions = collections.defaultdict(list)
        owner_of = {}
        for record in data['objects']:
            if record['type'] == 'PlayMakerFSM':
                definitions[record['data']['m_GameObject']['m_PathID']].append(record['data']['fsm'])
            elif record['type'] == 'GameObject':
                gid = int(record['source'].split(':')[1])
                for part in record['data']['m_Component']:
                    owner_of[part['component']['m_PathID']] = gid
        edges = collections.Counter(edge['source'] for edge in geometry['terrain_edges'])
        for collider in geometry['colliders']:
            if not collider.get('terrain_eligible'):
                continue
            fsms = definitions.get(owner_of.get(int(collider['source'].split(':')[1])))
            if not fsms:
                continue
            mutators = sorted({name for fsm in fsms for name in enabled_action_names(fsm)}
                              & COLLIDER_MUTATORS)
            rows.append({'scene': scene['scene_name'],
                         'definition': ' + '.join(sorted(fsm['name'] for fsm in fsms)),
                         'collider': collider['source'], 'type': collider['type'],
                         'edges': edges.get(collider['source'], 0), 'mutators': mutators})
    return rows


def scene_doors(sc):
    """Active transition points with a destination.

    A gate verdict only means something read against the room's ways in and out,
    so the gate report prints these next to it: a gate between two doors is the
    difference between a room and two rooms.
    """
    found = []
    for _, (typ, tree) in sorted(sc.objects.items()):
        if typ != 'TransitionPoint' or not tree.get('targetScene'):
            continue
        gid = tree['m_GameObject']['m_PathID']
        if gid in sc.gos and sc.active(gid):
            found.append({'name': sc.gos[gid]['m_Name'], 'position': sc.point(gid),
                          'target': tree['targetScene']})
    return found


def recognized(scenes):
    """What the compiled per-family subsets admit and refuse, whole catalogue."""
    from source import Source
    from scene import Scene
    import breakables
    source = Source()
    families = {'arena gates': (breakables.battle_gates, {}),
                'infected vines': (breakables.infected_vines, {})}
    out = {name: {'admitted': [], 'refused': []} for name in families}
    doors = {}
    for desc in scenes:
        loaded = Scene(source, desc['file'])
        for name, (recognize, options) in families.items():
            errors = []
            for row in recognize(loaded, errors=errors, **options):
                out[name]['admitted'].append(dict(row, scene=desc['scene_name']))
            out[name]['refused'] += [dict(error, scene=desc['scene_name']) for error in errors]
        if any(row['scene'] == desc['scene_name'] for row in out['arena gates']['admitted']):
            doors[desc['scene_name']] = scene_doors(loaded)
    return out, doors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fsm', action='store_true', help='also compile the FSM-authored families')
    parser.add_argument('--terrain', action='store_true', help='FSM definitions that own terrain colliders')
    parser.add_argument('--recognize', action='store_true', help='run the compiled per-family subsets')
    parser.add_argument('--json', type=Path, help='write the full report here')
    args = parser.parse_args()
    from quality import SCENE_TABLE
    counts, where, text = report(SCENE_TABLE)
    print(text)
    terrain = fsm_owned_terrain(SCENE_TABLE) if args.terrain else []
    if terrain:
        moved = [row for row in terrain if row['mutators']]
        print(f"\n{len(terrain)} terrain colliders in the cooked world belong to a PlayMaker FSM, "
              f"{sum(row['edges'] for row in terrain)} edges.")
        print(f"  {len(moved)} of them ({sum(row['edges'] for row in moved)} edges) belong to an FSM "
              'that can move a collider, so the serialized state the cook reads may not be the '
              'state the room loads with:')
        by_definition = collections.defaultdict(list)
        for row in terrain:
            by_definition[row['definition']].append(row)
        for definition, group in sorted(by_definition.items(), key=lambda kv: (-len(kv[1]), kv[0])):
            rooms = len({row['scene'] for row in group})
            mutators = sorted({name for row in group for name in row['mutators']})
            print(f"  {len(group):4d} colliders {sum(row['edges'] for row in group):5d} edges "
                  f"in {rooms:2d} scenes  {definition:34s} "
                  + (', '.join(mutators) if mutators else 'moves no collider'))
    admitted, doors = recognized(SCENE_TABLE) if args.recognize else ({}, {})
    for name, result in admitted.items():
        print(f"\n{name}: {len(result['admitted'])} admitted, {len(result['refused'])} refused")
        for scene in sorted({row['scene'] for row in result['admitted']}):
            rows = sorted((row for row in result['admitted'] if row['scene'] == scene),
                          key=lambda row: row.get('box', row['position'])[0])
            print(f'  {scene}')
            for door in doors.get(scene, []):
                print(f"    door {door['name']:18s} ({door['position'][0]:8.2f},{door['position'][1]:7.2f})"
                      f"   -> {door['target']}")
            for row in rows:
                if 'solid_on_load' not in row:
                    print(f"    {row['name']:18s} {row['source']}")
                    continue
                print(f"    gate {row['name']:18s} x {row['box'][0]:8.2f}..{row['box'][2]:8.2f}"
                      f" y {row['box'][1]:7.2f}..{row['box'][3]:7.2f} {row['collider_source']:14s} "
                      + ('closed in the source and here' if row['solid_on_load']
                         else 'OPEN in the source, terrain here'))
        for error in result['refused']:
            print(f"  refused {error['scene']:16s} {error['id']:14s} {error['error']}")
        solid = [row for row in result['admitted'] if row.get('solid_on_load') is False]
        if solid:
            print(f"  {len(solid)} of these are terrain in the cooked world and open in the source; "
                  f"their colliders are " + ', '.join(row['collider_source'] for row in solid))
    rows = fsm_support(SCENE_TABLE, FSM_FAMILIES) if args.fsm else []
    if rows:
        print('\nPlayMaker-authored families against the current script IR:')
        blocked = [row for row in rows if row['unsupported']]
        print(f'  {len(rows)} instances, {len(blocked)} with at least one action the IR refuses')
        for family in sorted({row['family'] for row in rows}):
            group = [row for row in rows if row['family'] == family]
            worst = max(group, key=lambda row: row['unsupported'])
            print(f"  {family:18s} {len(group):3d} instances, worst {worst['object']} "
                  f"({worst['unsupported']}/{worst['actions']} actions refused)")
    if args.json:
        args.json.write_text(json.dumps({
            'scenes': [scene['scene_name'] for scene in SCENE_TABLE],
            'families': {family: {'instances': counts[family], 'scenes': dict(where[family])}
                         for family in counts},
            'plan_families': {plan: list(families) for plan, families in PLAN_FAMILIES.items()},
            'fsm_owned_terrain': terrain, 'recognized': admitted,
            'fsm_support': rows}, indent=2, default=str))


if __name__ == '__main__':
    main()
