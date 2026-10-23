"""Why the cook refuses rigid Breakable fragments, counted over the admitted scenes.

host/effects.py fragment_records stops at the first reason a part cannot be a
solver Spec, and cook.py drops the list, so a refused part looked like a part
that was never authored. This lists every independent reason for every rigid
part (those host/breakables.py rigid_fragment accepts as a fling, plus the two
it refuses as a body) and prints the census. Needs the Windows source and
data/regions.json; `blockers` itself is source-free and unit tested.

    python tools/fragment_audit.py [--json OUT]
"""
import argparse
import collections
import json
import math
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

WHITE = {'r': 1.0, 'g': 1.0, 'b': 1.0, 'a': 1.0}
SOLVER_COLLIDERS = ('BoxCollider2D', 'PolygonCollider2D')


def blockers(components, matrix):
    """Independent reasons the solver cannot take this part, in a fixed order.

    `components` maps type to (id, data) as fragment_records builds it;
    `matrix` is the part's world transform. Empty means the part cooks today.
    A random-start spin (SpinSelf, SimpleRock) overwrites the authored turn in
    its Start, so `rotated` is only a real constraint for the other classes;
    it is still listed, because today's cooker refuses on it for every class.
    """
    found = []
    simple = 'SpinSelfSimple' in components
    spin = next((components[n][1] for n in ('SpinSelf', 'SpinSelfSimple') if n in components), None)
    if simple:
        found.append('spin_simple')
    elif spin is None or spin['spinFactor'] <= 0:
        found.append('no_positive_spin')
    if abs(matrix[0][1]) > 1e-6 or abs(matrix[1][0]) > 1e-6:
        found.append('rotated')
    columns = (math.hypot(matrix[0][0], matrix[1][0]), math.hypot(matrix[0][1], matrix[1][1]))
    if not all(.5 <= c <= 2 for c in columns):
        found.append('scale')
    renderer = components['SpriteRenderer'][1]
    if renderer['m_Color'] != WHITE:
        found.append('tint')
    if renderer['m_FlipX'] or renderer['m_FlipY']:
        found.append('flip')
    kind = next(t for t in components if t.endswith('Collider2D'))
    if kind not in SOLVER_COLLIDERS:
        found.append('circle_collider' if kind == 'CircleCollider2D' else 'collider')
    return found


def census():
    from source import Source
    from scene import Scene
    from breakables import breakable_sources, rigid_fragment, _components
    regions = json.load(open(ROOT / 'data/regions.json'))
    source = Source()
    rows = []
    for entry in regions['scenes']:
        scene = Scene(source, entry['file'])
        for owner in breakable_sources(scene, errors=[], contract=False):
            for part in owner['debris']:
                gid = int(part['game_object'].split(':')[-1])
                try:
                    if rigid_fragment(scene, gid) is None:
                        continue
                    body = None
                except ValueError as error:
                    body = str(error)
                components = {t: (i, d) for i, t, d in _components(scene, gid)}
                found = blockers(components, scene.world(scene.go_transform[gid]))
                classes = tuple(n for n in ('SpinSelf', 'SpinSelfSimple', 'SimpleRock', 'FinishingRigidBody') if n in components)
                rows.append({'scene': entry['scene_name'], 'part': part['game_object'], 'classes': classes,
                             'blockers': ([body] if body else []) + found})
    return rows


class _NoAtlas:
    """Stands in for the cook's atlas: the plan needs piece geometry, not pixels.

    Hands out texture numbers in order and remembers each one's size on the screen (the
    area a region's static pages would carry for it)."""
    def __init__(self):
        self.sizes = []

    def add(self, image, width, height, **kwargs):
        self.sizes.append((width, height))
        return len(self.sizes) - 1


def plan():
    """What the scene bank would hold with the pieces fragment_records now cooks.

    Read-only: runs the new classification over every admitted scene (whole scene for the
    piece list, each region's own owners for its frame map) and encodes the bank twice
    (with and without the pieces) so the metadata growth is the encoder's own number.
    """
    from source import Source
    from scene import Scene
    from breakables import breakable_sources
    import effects
    import world_metadata
    report = json.load(open(ROOT / 'data/regions.json'))
    source = Source()
    result = []
    for entry in report['scenes']:
        scene = Scene(source, entry['file'])
        refusals, atlas, frames = [], _NoAtlas(), []
        pieces = effects.fragment_records(source, scene, atlas, frames, refusals)
        by_owner = collections.defaultdict(list)
        for piece in pieces:
            by_owner[piece['door_source']].append(piece)
        rows = [r for r in report['regions'] if r['scene_id'] == entry['scene_id']]
        grown, plain, most, widest = [], [], 0, 0
        for row in rows:
            bounds = row.get('interaction_bounds', row['activation_bounds'])
            present = [piece for owner in breakable_sources(scene, bounds, errors=[], contract=False)
                       for piece in by_owner.get(owner['source'], [])]
            most = max(most, len(present))
            drawn = {p['frame'] for p in present}
            area = sum(atlas.sizes[frames[f]['texture']][0] * atlas.sizes[frames[f]['texture']][1] for f in drawn)
            widest = max(widest, area)
            grown.append(dict(row, door_debris=present))
            plain.append(dict(row, door_debris=[]))
        scene_record = dict(entry)
        with_pieces = len(world_metadata.encode_scene(scene_record, grown)[0]) if pieces else 0
        without = len(world_metadata.encode_scene(scene_record, plain)[0]) if pieces else 0
        packed = ROOT / f'.hkpsx/world-metadata-packed/scene_{entry["scene_id"]}.hkwm'
        result.append({'scene': entry['scene_name'], 'scene_id': entry['scene_id'], 'pieces': len(pieces),
                       'most_in_a_region': most, 'region_texture_px_at_most': round(widest), 'refused': dict(collections.Counter(r['reason'] for r in refusals)),
                       'classes': dict(collections.Counter(
                           ('random' if p['random_start'] else 'authored') + ('+finishing' if p['finishing'] else '')
                           + ('+tint' if p['tint'] else '') for p in pieces)),
                       'bank_growth_bytes': with_pieces - without,
                       'bank_bytes_today': packed.stat().st_size if packed.is_file() else None})
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--json', type=Path)
    parser.add_argument('--plan', action='store_true', help='piece counts and metadata growth per scene (read-only)')
    args = parser.parse_args()
    if args.plan:
        found = plan()
        for row in found:
            if row['pieces'] or row['refused']:
                print(row)
        print('pieces', sum(r['pieces'] for r in found), 'refused', collections.Counter(
            reason for r in found for reason, n in r['refused'].items() for _ in range(n)))
        grew = max(found, key=lambda r: (r['bank_bytes_today'] or 0) + r['bank_growth_bytes'])
        print('largest bank after:', grew['scene'], (grew['bank_bytes_today'] or 0) + grew['bank_growth_bytes'])
        if args.json:
            args.json.write_text(json.dumps(found, indent=1))
        return
    rows = census()
    print(f'{len(rows)} rigid parts, {sum(1 for r in rows if not r["blockers"])} without a blocker')
    for combo, count in collections.Counter(tuple(r['blockers']) for r in rows).most_common():
        print(f'{count:5d}  {", ".join(combo) or "cooks today"}')
    per = collections.Counter()
    for r in rows:
        for b in r['blockers']:
            per[b] += 1
    print('any part blocked by:', dict(per))
    if args.json:
        args.json.write_text(json.dumps(rows, indent=1))


if __name__ == '__main__':
    main()
