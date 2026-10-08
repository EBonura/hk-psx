"""The admitted Blockers' `Terrain Block` edges, so the runtime can lift them.

A Blocker (Elder Baldur) sits on a `Terrain Block` child: a layer 8 box the
world cook bakes into the room pack as ordinary terrain (`host/blocker.py`
checks its footprint and leaves it to that cook). In the source the whole
Blocker GameObject is destroyed on death, block included, and its
`PersistentBoolItem` keeps it destroyed. The port used to keep the block, so the
Crossroads_11_alt tunnel to Greenpath stayed shut after the kill.

This is the same join `host/battle_gates.py` makes for arena gates: per
catalogue slot, the edge indices in that slot's room pack whose source is the
block's collider, keyed by Blocker. `game/src/blocker_terrain.rs` excludes them
once the Blocker is dead. It reads `data/regions.json` and the source scene and
cooks nothing, so it is not a cooker input; regenerate with

    .venv/bin/python host/blocker_terrain.py
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

# `game/src/blocker_terrain.rs` keeps the dead set in one u8.
MAX_BLOCKERS = 8


def blockers(report):
    """Every admitted Blocker placement: (scene id, scene file, source, game object)."""
    found = {}
    for region in report['regions']:
        for actor in region['actors']:
            if actor.get('name') != 'Blocker' or not actor.get('movement_supported'):
                continue
            key = (region['scene_id'], actor['source'])
            found.setdefault(key, (region['scene_id'], region['scene_file'], actor['source'],
                                   actor['game_object'], actor['spec_source_id']))
    rows = sorted(found.values(), key=lambda r: (r[0], int(r[2].split(':')[-1])))
    if len(rows) > MAX_BLOCKERS:
        raise ValueError(f'{len(rows)} Blockers exceed the {MAX_BLOCKERS}-bit dead set')
    return rows


def terrain_colliders(source, rows):
    """The `Terrain Block` child's collider sid for each Blocker."""
    from scene import Scene
    from blocker import _children
    out = []
    for scene_id, scene_file, actor_source, game_object, _ in rows:
        sc = Scene(source, scene_file)
        children = _children(sc, game_object)
        if 'Terrain Block' not in children:
            raise ValueError(f'{actor_source} has no Terrain Block child')
        gid, _ = children['Terrain Block']
        colliders = []
        for component in sc.gos[gid]['m_Component']:
            ref = component['component']
            obj = sc.file.objects.get(ref['m_PathID']) if ref['m_FileID'] == 0 else None
            if obj is not None and obj.type.name == 'BoxCollider2D':
                colliders.append(f'{scene_file}:{ref["m_PathID"]}')
        if len(colliders) != 1:
            raise ValueError(f'{actor_source} Terrain Block carries {len(colliders)} box colliders')
        out.append(colliders[0])
    return out


def bind(rows, colliders, report):
    """(catalogue slot, blocker, edges) for every region a block reached."""
    from battle_gates import edge_scratch_slots, neighbour_edges
    owner = {collider: index for index, collider in enumerate(colliders)}
    gates = {}
    gate_rs = ROOT / 'data/battle_gates.rs'
    if gate_rs.is_file():
        import re
        for slot, body in re.findall(r'\((\d+),\d+,&\[([^\]]*)\]\)', gate_rs.read_text()):
            gates[int(slot)] = gates.get(int(slot), 0) + len([v for v in body.split(',') if v.strip()])
    budget = edge_scratch_slots()
    neighbours = neighbour_edges()
    out = []
    for slot, region in enumerate(report['regions']):
        found = {}
        for edge, source in enumerate(region['edge_sources']):
            index = owner.get(source)
            if index is not None:
                if rows[index][0] != region['scene_id']:
                    raise ValueError('a Blocker terrain edge outside its own scene')
                found.setdefault(index, []).append(edge)
        spent = sum(len(e) for e in found.values()) + neighbours.get(slot, 0) + gates.get(slot, 0)
        if found and spent > budget:
            raise ValueError(f'catalogue slot {slot} ({region["scene_name"]}) needs {spent} scripted '
                             f'edge exclusions against {budget} in game/src/world.rs')
        for index in sorted(found):
            out.append((slot, index, found[index]))
    return out


def generate(rows, bindings):
    lines = [
        '// Generated from the admitted Blockers and the cooked region edge sources.',
        '// host/blocker_terrain.py is the generator.',
        f'pub const BLOCKERS:usize={len(rows)};',
        '/// (guest scene id, actor source id) per Blocker; the index is its bit in',
        '/// the dead set and its persist local id.',
        'pub static SOURCES:&[(u16,u32)]=&[' + ''.join(f'({r[0]},{r[4]}),' for r in rows) + '];',
        '/// (catalogue slot, Blocker, its Terrain Block edges in that slot\'s pack),',
        '/// sorted by slot so the runtime can binary search it.',
        'pub static REGIONS:&[(u16,u8,&[u16])]=&['
        + ''.join('({},{},&[{}]),'.format(s, i, ','.join(map(str, e))) for s, i, e in bindings) + '];',
    ]
    return '\n'.join(lines) + '\n'


def main():
    from source import Source
    report = json.loads((ROOT / 'data/regions.json').read_text())
    rows = blockers(report)
    colliders = terrain_colliders(Source(), rows)
    bindings = bind(rows, colliders, report)
    (ROOT / 'data/blocker_terrain.rs').write_text(generate(rows, bindings))
    record = {'blockers': [{'scene': r[0], 'source': r[2], 'terrain_collider': c} for r, c in zip(rows, colliders)],
              'bindings': [{'slot': s, 'blocker': i, 'edges': e} for s, i, e in bindings]}
    (ROOT / '.hkpsx/blocker-terrain.json').write_text(json.dumps(record, indent=2) + '\n')
    print(f'{len(rows)} Blockers, {len(bindings)} region bindings: '
          + ', '.join(f'{r[2]} block {c}' for r, c in zip(rows, colliders)), flush=True)


if __name__ == '__main__':
    main()
