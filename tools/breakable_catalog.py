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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fsm', action='store_true', help='also compile the FSM-authored families')
    parser.add_argument('--json', type=Path, help='write the full report here')
    args = parser.parse_args()
    from quality import SCENE_TABLE
    counts, where, text = report(SCENE_TABLE)
    print(text)
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
            'fsm_support': rows}, indent=2))


if __name__ == '__main__':
    main()
