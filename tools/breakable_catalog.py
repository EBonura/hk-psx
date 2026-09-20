"""What the 46 admitted scenes actually contain of P12's breakable families.

The package plan lists ten families from the whole game. This measures which of
them the admitted catalogue holds, and in what form, so the work is sized
against the world the port ships rather than against the list.

Family sizes are the compiled per-family subsets' own answers. Every family
host/breakables.py has a subset for is counted by running it, which selects by
FSM definition and by the reachability `Scene.active` answers, and every other
component family is counted against that same reachability. Both need the
Windows source. Without it the report falls back to the name census over the
cached per-scene component inventories under
`.hkpsx/world-import/<file>/components.json.gz` and marks every line, because
that census is wider on both counts: it selects an FSM family by the object's
name and it counts objects the source loads switched off. `--fsm` additionally
compiles the PlayMaker-authored families through host/script_ir.py.

The `secret_mask` and `hidden_wall` name rules below are that fallback and
nothing more. Both behaviours are one FSM structure that ships under two
definition names and under object names the rules do not match, so the counts
they give are not the family sizes.

`--terrain` asks the question that turned out to matter more than the counts:
which FSM definitions own a collider host/world_geometry.py bakes into terrain?
Those are the instances where the serialized state and the state the original
loads with can disagree, and the cook reads the serialized one. It then narrows
that list twice, to the unconditional load path and to actions aimed at the
FSM's own object, which is the only combination that can make the cook wrong
before the hero has done anything.

`--recognize` runs the compiled per-family subsets in host/breakables.py and
host/reveal_masks.py over the catalogue and reports what each admits and what it
refuses, with the source reason. It needs the Windows source.
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
from focus import action_fields

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
    # Named for the component, not for a behaviour it does not drive. This read
    # `pogo_target` until the pogo population was re-measured: NailSlash resolves
    # NonBouncer, BigBouncer and BounceShroom and never reads TinkEffect, which
    # answers a nail collider with a shake, a flash and a sound and never touches
    # hero velocity. host/actors.py::pogo_sources owns that population and
    # tools/physics_catalog.py::pogo_coverage reports it from that function's own
    # output; the 166 below is a count of spark owners and nothing more.
    'tink_effect': ('TinkEffect',),
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
    # `Break Wall 2` is the same authored hidden wall under another object name
    # and another definition name; `--recognize` selects it structurally and
    # counts 7 where `^Breakable Wall` alone counted 4.
    'hidden_wall': r'^Breakable Wall|^Break Wall',
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
# here with no entry is absent from these 46 scenes, which is a result, not a
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


# The families host/breakables.py has a compiled subset for, and the name
# `recognized()` files each one under. Wherever a family appears here its size
# is that function's answer, never a name rule: breakables.py selects by FSM
# definition and by the reachability the cooker uses, and the name rules below
# both miss definitions and count objects the source leaves switched off.
RECOGNIZED_FAMILIES = {'multi_hit_gate': 'arena gates', 'hidden_wall': 'hidden walls',
                       'cracked_floor': 'cracked floors', 'infected_vine': 'infected vines',
                       'secret_mask': 'secret masks'}


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


def report(scenes, recognition=None, active=None):
    """The catalogue, with every family host/breakables.py owns taken from it.

    `recognition` is `recognized()`'s result and `active` its per-family counts
    over the reachability `Scene.active` answers. Without the Windows source
    neither exists and the name census stands in, which is a wider number: it
    counts an object the source loads switched off, and it selects an FSM family
    by the object's name rather than by the definition the FSM carries.
    """
    counts, where = census(scenes)
    sourced = set()
    for family, name in RECOGNIZED_FAMILIES.items():
        result = (recognition or {}).get(name)
        if result is None:
            continue
        rows = result['admitted'] + result['refused']
        counts[family] = len(rows)
        where[family] = collections.Counter(row['scene'] for row in rows)
        sourced.add(family)
    if active is not None:
        for family in COMPONENT_FAMILIES:
            if family in sourced:
                continue
            rooms = active.get(family, collections.Counter())
            counts[family] = sum(rooms.values())
            where[family] = rooms
            sourced.add(family)
    for family in [f for f, total in counts.items() if not total]:
        del counts[family]
    lines = [f'{len(scenes)} admitted scenes', '',
             'families present (* is a serialized-instance name census: the source may load it '
             'switched off, and an FSM family is selected by object name):']
    for family, total in counts.most_common():
        mark = ' ' if family in sourced else '*'
        lines.append(f'{mark} {total:5d} in {len(where[family]):2d} scenes  {family}')
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


# The two mutators that reach the owner's children rather than the owner, so
# they cannot turn the owner's own collider off however they run.
CHILD_MUTATORS = frozenset({'ActivateAllChildren', 'DestroyAllChildren'})
# The one that names no target because it always acts on the owner.
IMPLICIT_OWNER_MUTATORS = frozenset({'DestroySelf'})


def owner_load_path_mutators(fsm):
    """Collider mutators aimed at the owner, on the unconditional load path.

    This is the screen `fsm_owned_terrain` wanted and the mutator list alone
    could not give. Two things narrow it, and both are exact rather than
    heuristic. Reachability: only `FINISHED` transitions are followed, so the
    walk covers what runs with no hero, no save state and no other object, which
    is the window the cook's serialized read has to be right in. Target: the
    action has to name Owner, because one that moves a different object cannot
    change this object's terrain collider.

    What is left out is left out on purpose. A state behind `ACTIVATE` needs the
    save to say the room was already opened, which a fresh boot does not, and a
    state behind a hero event needs the hero, which is after the first drawn
    frame. Those are behaviours to reproduce, not places the cook can be wrong.
    """
    states = {state['name']: state for state in fsm['states']}
    seen, stack, found = set(), [fsm['startState']], collections.Counter()
    while stack:
        name = stack.pop()
        if name in seen or name not in states:
            continue
        seen.add(name)
        data = states[name]['actionData']
        for index, raw in enumerate(data['actionNames']):
            action = raw.rsplit('.', 1)[-1]
            if not data['actionEnabled'][index] or action not in COLLIDER_MUTATORS \
                    or action in CHILD_MUTATORS:
                continue
            if action in IMPLICIT_OWNER_MUTATORS:
                found[f'{name}:{action}'] += 1
                continue
            try:
                target = action_fields(data, index).get('gameObject')
            except ValueError:
                # An undecodable target is not evidence of safety.
                found[f'{name}:{action}?'] += 1
                continue
            if isinstance(target, dict) and target.get('ownerOption') == 0:
                found[f'{name}:{action}'] += 1
        for transition in states[name]['transitions']:
            if transition['fsmEvent']['name'] == 'FINISHED':
                stack.append(transition['toState'])
    return found


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
            load = collections.Counter()
            for fsm in fsms:
                load += owner_load_path_mutators(fsm)
            rows.append({'scene': scene['scene_name'],
                         'definition': ' + '.join(sorted(fsm['name'] for fsm in fsms)),
                         'collider': collider['source'], 'type': collider['type'],
                         'edges': edges.get(collider['source'], 0), 'mutators': mutators,
                         'load_path_mutators': sorted(load)})
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


def secret_masks(sc, errors=None):
    """The one-way secret masks host/reveal_masks.py admits, in this scene.

    Wrapped to the same (scene, errors) shape the other recognizers use. The
    reversible reveal shapes are left out: they are not a destruction family and
    they are already reported by the region bindings.
    """
    from reveal_masks import reveal_mask_sources
    found = reveal_mask_sources(sc)
    if errors is not None:
        errors += [{'id': record['source'], 'type': 'secret mask', 'error': record['error']}
                   for record in found['unsupported']]
    return [dict(record, position=[min(p[0] for p in record['trigger']),
                                   min(p[1] for p in record['trigger'])])
            for record in found['controllers'] if record['one_way']]


def secret_mask_fade_contract(scenes):
    """Re-run the region bind, so the report says what actually ships.

    `secret_masks` answers the scene-level question: does this instance decode.
    Whether its whole authored group can fade is a property of the cooked rooms,
    so host/reveal_masks.py decides it at bind time and this repeats that here
    against the packs on disk rather than restating the scene-level count as if
    it were the final one. Nothing is written.
    """
    from source import Source
    from scene import Scene
    from reveal_masks import bind_regions, reveal_mask_sources
    path = ROOT / 'data/regions.json'
    if not path.is_file():
        return None
    report = json.loads(path.read_text())
    source = Source()
    by_scene = {info['scene_id']: reveal_mask_sources(Scene(source, info['file']))
                for info in report['scenes']}
    before = {scene: [record['source'] for record in entry['controllers'] if record['one_way']]
              for scene, entry in by_scene.items()}
    bind_regions(report, by_scene)
    names = {info['scene_id']: info['scene_name'] for info in report['scenes']}
    kept, dropped = [], []
    for scene, entry in by_scene.items():
        for item in entry['unsupported']:
            if item['source'] in before[scene]:
                dropped.append((names[scene], item['name'], item['source'], item['error']))
        kept += [(names[scene], record['name'], record['source'])
                 for record in entry['controllers'] if record['one_way']]
    return kept, dropped


def live_component_families(sc):
    """COMPONENT_FAMILIES instances one scene actually loads switched on.

    `Scene.active` is the reachability the cooker itself asks, so this asks it
    too. The cached component inventory carries only the object's own
    `m_IsActive`, which says nothing about a disabled parent: 148 scenes-wide
    BreakableInfectedVine components are 14 by the self flag and 8 by this.
    """
    wanted = {name: family for family, names in COMPONENT_FAMILIES.items() for name in names}
    counts = collections.Counter()
    for typ, tree in sc.objects.values():
        family = wanted.get(typ)
        if family is None or not tree.get('m_Enabled', 1):
            continue
        gid = tree.get('m_GameObject', {}).get('m_PathID')
        if gid in sc.gos and sc.active(gid):
            counts[family] += 1
    return counts


def recognized(scenes):
    """What the compiled per-family subsets admit and refuse, whole catalogue."""
    from source import Source
    from scene import Scene
    import breakables
    source = Source()
    families = {'arena gates': (breakables.battle_gates, {}),
                'hidden walls': (breakables.hidden_walls, {}),
                'cracked floors': (breakables.cracked_floors, {}),
                'infected vines': (breakables.infected_vines, {}),
                'secret masks': (secret_masks, {})}
    out = {name: {'admitted': [], 'refused': []} for name in families}
    doors = {}
    active = collections.defaultdict(collections.Counter)
    for desc in scenes:
        loaded = Scene(source, desc['file'])
        for name, (recognize, options) in families.items():
            errors = []
            for row in recognize(loaded, errors=errors, **options):
                out[name]['admitted'].append(dict(row, scene=desc['scene_name']))
            out[name]['refused'] += [dict(error, scene=desc['scene_name']) for error in errors]
        for family, total in live_component_families(loaded).items():
            active[family][desc['scene_name']] += total
        if any(row['scene'] == desc['scene_name'] for row in out['arena gates']['admitted']):
            doors[desc['scene_name']] = scene_doors(loaded)
    return out, doors, active


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fsm', action='store_true', help='also compile the FSM-authored families')
    parser.add_argument('--terrain', action='store_true', help='FSM definitions that own terrain colliders')
    parser.add_argument('--recognize', action='store_true',
                        help='also list what each compiled per-family subset admits and refuses')
    parser.add_argument('--json', type=Path, help='write the full report here')
    args = parser.parse_args()
    from quality import SCENE_TABLE
    # The family sizes come from the compiled subsets whenever the Windows
    # source is there to run them. Without it the name census stands in, which
    # is a wider and differently selected number, so the report says which.
    try:
        admitted, doors, active = recognized(SCENE_TABLE)
    except (ImportError, FileNotFoundError, OSError) as error:
        admitted, doors, active = {}, {}, None
        print(f'no Windows source ({error}); every family below is the name census\n')
    counts, where, text = report(SCENE_TABLE, admitted, active)
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
        # The screen above says which FSMs *can* move a collider. This one says
        # which ones do it to their own collider before the hero exists, which is
        # the only window where the cook's serialized read can already be wrong.
        reached = [row for row in terrain if row['load_path_mutators']]
        print(f"\n  narrowing to the unconditional load path and to actions aimed at the FSM's own "
              f"object leaves {len(reached)} of {len(moved)} ({sum(row['edges'] for row in reached)} "
              f"of {sum(row['edges'] for row in moved)} edges):")
        for definition, group in sorted(collections.Counter(
                (row['definition'], ', '.join(sorted({name for row2 in [row]
                                                      for name in row2['load_path_mutators']})))
                for row in reached).items(), key=lambda kv: -kv[1]):
            print(f'  {group:4d} colliders  {definition[0]:34s} {definition[1]}')
    for name, result in (admitted if args.recognize else {}).items():
        inert = [row for row in result['refused'] if row.get('source_inert')]
        # An object the original disables too is not a port gap, and counting it
        # as one has the catalogue reporting work that can never be done.
        line = f"\n{name}: {len(result['admitted'])} admitted, {len(result['refused']) - len(inert)} refused"
        print(line + (f', {len(inert)} inert in the source' if inert else ''))
        for scene in sorted({row['scene'] for row in result['admitted']}):
            rows = sorted((row for row in result['admitted'] if row['scene'] == scene),
                          key=lambda row: row.get('box', row['position'])[0])
            print(f'  {scene}')
            for door in doors.get(scene, []):
                print(f"    door {door['name']:18s} ({door['position'][0]:8.2f},{door['position'][1]:7.2f})"
                      f"   -> {door['target']}")
            for row in rows:
                if 'one_way' in row:
                    print(f"    mask {row['name']:18s} {row['source']:14s} "
                          f"{len(row['renderer_sources']):2d} renderers, {row['fade_ticks']:3d} ticks"
                          + (', authored to chime' if row['plays_sound'] else ''))
                    continue
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
        if name == 'secret masks':
            contract = secret_mask_fade_contract(SCENE_TABLE)
            if contract is None:
                print('  (no data/regions.json, so the whole-group fade contract was not applied)')
            else:
                kept, dropped = contract
                print(f'  the region bind then applies the whole-group fade contract: '
                      f'{len(kept)} of these ship, {len(dropped)} more are refused')
                for scene, mask, source, error in dropped:
                    print(f'  refused {scene:16s} {source:14s} {error}')
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
