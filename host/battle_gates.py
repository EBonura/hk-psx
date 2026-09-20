"""Source `BG Control` arena gates, joined to the terrain edges the cook baked.

`host/breakables.py::battle_gate` reads the one decision a gate makes on load:
`Opened` tests `Start Closed`, and a gate that has it set turns its box back on
in `Quick Close` and then waits for the `BG OPEN` its arena sends when the fight
is won. `host/cook.py` bakes every gate's collider as terrain whatever that
verdict says, so the room pack holds an edge for all of them.

This is the join that lets the runtime move them: for every catalogue slot, the
edge indices in that slot's room pack that belong to a gate collider, keyed by
gate. `game/src/battle_gates.rs` excludes the edges of every gate that is
currently open, the way `game/src/great_door.rs` excludes the door's when it is
broken through, and it starts from the gates that are open on load, which is the
complement of `PLACEMENT_CLOSED`. A gate that loads open is therefore terrain
the runtime lifts on the first frame rather than terrain the cook withheld,
which is what lets `BG CLOSE` seal an arena at its ends.

Both directions of that join have to hold. A missing row is now an invisible
wall rather than a gate that cannot close, so `bind` derives the rows from the
cook's own `edge_sources` and never from a second copy of the rule, and `main`
reports any gate the cook baked no terrain for at all.

This reads `data/regions.json` rather than cooking anything, so it is not a
cooker input: changing it must not invalidate a single cooked region. Regenerate
with

    .venv/bin/python host/battle_gates.py

which rewrites `data/battle_gates.rs` and `.hkpsx/battle-gates.json`. The table
is only as current as `data/regions.json`: `host/cook.py` sits above the divider
in `host/cook_inputs.txt`, so a change to which colliders it bakes re-cooks all
693 regions and this has to run again afterwards.
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

# `game/src/battle_gates.rs` keeps the closed set in one u16, which is the whole
# reason the runtime costs a single test before it looks at anything.
MAX_GATES = 16


def edge_scratch_slots():
    """The shared exclusion scratch, read out of the guest rather than repeated.

    `world::State` keeps one bounded list for every scripted exclusion and
    `append_script_edges` asserts on overflow, which on a disc is a crash rather
    than a wrong room. Reading the number here is what turns that into a
    generator refusal, the way host/cook_scripts.py reads SCRIPT_FIELD_SLOTS.
    """
    text = (ROOT / 'game/src/world.rs').read_text()
    found = re.search(r'pub const SCRIPT_EDGE_SLOTS: usize = (\d+);', text)
    if not found:
        raise ValueError('cannot read SCRIPT_EDGE_SLOTS out of game/src/world.rs')
    return int(found.group(1))


def _count(body):
    return len([value for value in body.split(',') if value.strip()])


def neighbour_edges():
    """Exclusions per catalogue slot that the other two controllers already own.

    The Lifeblood cocoons and the Great Door share this scratch, so the budget
    is the union in one slot rather than this table's own rows. Read off the
    generated files the guest actually links, not off a second copy of the rule
    that produced them. Missing files mean those tables have not been cooked
    yet, which is not this generator's problem to diagnose.
    """
    counts = {}
    life = ROOT / 'data/lifeblood.rs'
    if life.is_file():
        # Only the slots that see a cocoon carry a row, so the slot is written.
        for slot, body in re.findall(r'\((\d+),Binding\{off:&\[[^\]]*\],edges:&\[([^\]]*)\]\}\)',
                                     life.read_text()):
            counts[int(slot)] = counts.get(int(slot), 0) + _count(body)
    door = ROOT / 'data/great_door.rs'
    if door.is_file():
        # Only the slots that see the door carry a row, so the slot is written.
        for slot, body in re.findall(r'\((\d+),Binding\{frames:\[[^\]]*\],edges:&\[([^\]]*)\]\}\)',
                                     door.read_text()):
            counts[int(slot)] = counts.get(int(slot), 0) + _count(body)
    return counts


def survey(source, report):
    """Every admitted scene's `BG Control` gates, in a stable order.

    Ordered by (scene id, source object id) rather than by name: two gates in
    one room share the name `Battle Gate 2` up to a suffix, and the bit a gate
    owns has to survive a rename in the source.
    """
    from scene import Scene
    from breakables import battle_gates
    gates = []
    refused = []
    for meta in sorted(report['scenes'], key=lambda s: s['scene_id']):
        errors = []
        found = battle_gates(Scene(source, meta['file']), errors=errors)
        for error in errors:
            refused.append(dict(error, scene=meta['scene_name']))
        for record in sorted(found, key=lambda r: int(r['source'].split(':')[-1])):
            gates.append({'scene': meta['scene_id'], 'scene_name': meta['scene_name'],
                          'name': record['name'], 'source': record['source'],
                          'collider_source': record['collider_source'],
                          'solid_on_load': record['solid_on_load'],
                          'box': record['box']})
    if refused:
        raise ValueError(f'unreadable BG Control gates: {refused}')
    if len(gates) > MAX_GATES:
        raise ValueError(f'{len(gates)} arena gates exceed the {MAX_GATES}-bit closed set')
    return gates


def bind(gates, report):
    """(catalogue slot, gate, edges) for every gate the cook baked as terrain.

    `edge_sources` is the cook's own join key: it names, per cooked edge, the
    source collider the edge came out of. Reading the answer off the cook's
    output rather than re-deriving it is the whole point: a row that disagrees
    with the pack is an invisible wall now that the open gates are baked too.
    A gate with no row at all means no edge of it survived any region's
    collision-bounds cull, which is the honest answer rather than an empty one.
    """
    owner = {gate['collider_source']: index for index, gate in enumerate(gates)}
    if len(owner) != len(gates):
        raise ValueError('two arena gates share one collider')
    budget = edge_scratch_slots()
    neighbours = neighbour_edges()
    rows = []
    for slot, region in enumerate(report['regions']):
        found = {}
        for edge, source in enumerate(region['edge_sources']):
            index = owner.get(source)
            if index is None:
                continue
            if gates[index]['scene'] != region['scene_id']:
                raise ValueError(f'gate {gates[index]["name"]} bound outside its own scene')
            found.setdefault(index, []).append(edge)
        spent = sum(len(edges) for edges in found.values()) + neighbours.get(slot, 0)
        if spent > budget:
            raise ValueError(f'catalogue slot {slot} ({region["scene_name"]}) needs {spent} '
                             f'scripted edge exclusions against {budget} in game/src/world.rs')
        for index in sorted(found):
            if slot >= 65536:
                raise ValueError('arena gate binding slot exceeds u16')
            rows.append((slot, index, found[index]))
    return rows


def generate(gates, rows):
    cooked = sorted({index for _, index, _ in rows})
    scene_gates = {}
    for index, gate in enumerate(gates):
        scene_gates.setdefault(gate['scene'], 0)
        scene_gates[gate['scene']] |= 1 << index
    def mask(bits):
        return '0b' + format(bits, '0%db' % max(len(gates), 1))
    lines = [
        '// Generated from the source `BG Control` gates and the cooked region edge',
        '// sources. host/battle_gates.py is the generator.',
        '// Gate order is (scene id, source object id); a gate owns its bit for the',
        '// life of this table, so a rename in the source cannot move it.',
        f'pub const GATES:usize={len(gates)};',
        '/// The collider state `Opened` leaves each gate in on load: set means the',
        '/// gate carries `Start Closed`, turned its box back on in `Quick Close`, and',
        '/// is waiting for a `BG OPEN` its arena has not sent yet.',
        f'pub const PLACEMENT_CLOSED:u16={mask(sum(1 << i for i, g in enumerate(gates) if g["solid_on_load"]))};',
        '/// The gates whose collider reached a room pack as terrain. `host/cook.py`',
        '/// bakes every gate, so this is normally all of them; a gate outside every',
        '/// region\'s collision bounds would be the one exception, and closing it',
        '/// would move nothing because there is no edge of it anywhere to restore.',
        f'pub const COOKED:u16={mask(sum(1 << i for i in cooked))};',
        '/// Guest scene id and the gates a `Battle Scene` there broadcasts to. The',
        "/// source's BG CLOSE/BG OPEN reach every gate in the room, not only the two",
        '/// at the arena ends, so the mask is per scene rather than per arena.',
        'pub static SCENE_GATES:&[(u8,u16)]=&['
        + ''.join(f'({scene},{mask(bits)}),' for scene, bits in sorted(scene_gates.items())) + '];',
        '/// (catalogue slot, gate, that gate\'s terrain edges in the slot\'s room',
        '/// pack), sorted by slot then gate so the runtime can binary search it.',
        'pub static REGIONS:&[(u16,u8,&[u16])]=&['
        + ''.join('({},{},&[{}]),'.format(slot, index, ','.join(map(str, edges)))
                  for slot, index, edges in rows) + '];',
    ]
    return '\n'.join(lines) + '\n'


def main():
    from source import Source
    report = json.loads((ROOT / 'data/regions.json').read_text())
    gates = survey(Source(), report)
    rows = bind(gates, report)
    (ROOT / 'data/battle_gates.rs').write_text(generate(gates, rows))
    # A gate with no row carries no terrain anywhere, so its arena cannot seal
    # at that end. That is not a refusal, because it is a property of the source
    # placement against the region bounds rather than of this join, but it is
    # the one thing a reader of this table would otherwise have to derive.
    bound = {index for _, index, _ in rows}
    limitations = [
        f'{gates[index]["scene_name"]} {gates[index]["name"]} has no cooked terrain in any'
        ' region, so BG CLOSE cannot seal that end of its arena.'
        for index in range(len(gates)) if index not in bound
    ]
    limitations.append(
        'Gate animation, dust and slam audio are not reproduced; only the collider moves.')
    record = {
        'gates': gates,
        'bindings': [{'slot': slot, 'gate': index, 'name': gates[index]['name'],
                      'scene_name': gates[index]['scene_name'], 'edges': edges}
                     for slot, index, edges in rows],
        'limitations': limitations,
    }
    (ROOT / '.hkpsx/battle-gates.json').write_text(json.dumps(record, indent=2) + '\n')
    cooked = {row['name'] for row in record['bindings']}
    print(f'{len(gates)} arena gates, {len(cooked)} cooked as terrain, '
          f'{len(rows)} region bindings: {sorted(cooked)}', flush=True)
    for line in limitations:
        print(f'  note: {line}', flush=True)


if __name__ == '__main__':
    main()
