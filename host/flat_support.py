"""Source-qualified flat support through whole residency cells, built from final packs.

This is only a prefetch proof. Missing support retains the old topology policy;
we never infer new collision or terrain from a height sample.
"""
import hashlib
import json
from pathlib import Path
import re
import struct
from collections import defaultdict

ROOT = Path(__file__).resolve().parents[1]
MAX_HEIGHTS = 8  # Explicit bounded guest lookup (a slice, scanned per prefetch); Crossroads regions reach three.
STATIC_OWNER = {'Transform', 'MeshFilter', 'MeshRenderer', 'EdgeCollider2D'}


def static_chain_allowed(chain):
    return bool(chain) and all(item['active'] for item in chain) and \
        {'Transform', 'MeshFilter', 'MeshRenderer', 'EdgeCollider2D'} <= set(chain[0]['types']) and \
        set(chain[0]['types']) <= STATIC_OWNER and \
        all(item['types'] == ['Transform'] for item in chain[1:])


def full_floors(bounds, edges, excluded, half_width):
    """Exact union proof with body-width margins; no gaps, slope or tolerances."""
    left = round(bounds[0] * 65536) - half_width
    right = round(bounds[2] * 65536) + half_width
    groups = defaultdict(list)
    for i, (edge, source) in enumerate(edges):
        x0, y0, x1, y1 = edge
        if i in excluded or x0 == x1 or y0 != y1:
            continue
        groups[y0].append((min(x0, x1), max(x0, x1), i, source))
    result = []
    for height, ranges in sorted(groups.items()):
        end, proof = left, []
        for low, high, index, source in sorted(ranges):
            if high < end:
                continue
            if low > end:
                break
            proof.append({'edge': index, 'source': source, 'segment': [low, height, high, height]})
            end = max(end, high)
            if end >= right:
                result.append({'height': height, 'required_interval': [left, right], 'edges': proof})
                break
    if len(result) > MAX_HEIGHTS:
        raise ValueError('flat support exceeds bounded height catalogue')
    return result


class StaticTerrain:
    def __init__(self, source):
        self.source = source
        self.cache = {}

    def qualify(self, sid):
        if sid in self.cache:
            return self.cache[sid]
        from scene import Scene
        source = self.source
        name, ident = sid.rsplit(':', 1)
        file = source.file(name)
        obj = file.objects[int(ident)]
        raw = source.read(obj)
        record = {'source': sid, 'type': obj.type.name,
                  'sha256': hashlib.sha256(obj.get_raw_data()).hexdigest(), 'qualified': False}
        if obj.type.name != 'EdgeCollider2D' or not raw['m_Enabled'] or raw['m_IsTrigger']:
            self.cache[sid] = (record, set())
            return self.cache[sid]
        go = source.ref(file, raw['m_GameObject'])
        owner = source.read(go)
        if owner['m_Layer'] != 8:
            self.cache[sid] = (record, set())
            return self.cache[sid]
        # Reuse the cooker's exact transform implementation with only this
        # owner/ancestor chain loaded, not a second full-scene extraction.
        scene = Scene.__new__(Scene)
        scene.transforms, scene.go_transform = {}, {}
        chain, seen = [], set()
        while go is not None:
            if go.path_id in seen:
                raise ValueError('static floor transform cycle')
            seen.add(go.path_id)
            data = source.read(go)
            objects = [source.ref(file, c.get('component', c)) for c in data['m_Component']]
            types = [source.typename(o) for o in objects]
            transforms = [o for o in objects if o.type.name == 'Transform']
            if len(transforms) != 1:
                raise ValueError('static floor owner transform count')
            transform = transforms[0]
            value = source.read(transform)
            scene.transforms[transform.path_id] = value
            scene.go_transform[go.path_id] = transform.path_id
            chain.append({'source': source.sid(go), 'name': data['m_Name'],
                          'active': bool(data['m_IsActive']), 'types': types,
                          'transform_sha256': hashlib.sha256(transform.get_raw_data()).hexdigest()})
            parent = value['m_Father']
            go = source.ref(file, source.read(source.ref(file, parent))['m_GameObject']) if parent['m_PathID'] else None
        record['chain'] = chain
        if not static_chain_allowed(chain):
            self.cache[sid] = (record, set())
            return self.cache[sid]
        gid = raw['m_GameObject']['m_PathID']
        offset = raw['m_Offset']
        points = [scene.point(gid, p['x'] + offset['x'], p['y'] + offset['y']) for p in raw['m_Points']]
        segments = set()
        for a, b in zip(points, points[1:]):
            a, b = list(a), list(b)
            # Exact collision quantization already used by host/cook.py.
            if abs(a[0] - b[0]) <= .005:
                b[0] = a[0]
            elif abs(a[1] - b[1]) <= .005:
                b[1] = a[1]
            segments.add(tuple(round(v * 65536) for p in (a, b) for v in p[:2]))
        record['qualified'] = True
        self.cache[sid] = (record, segments)
        return self.cache[sid]


def generate(report, root=ROOT, source=None):
    root = Path(root)
    params = (root / 'data/params.rs').read_text().split('};', 1)[0]
    def value(name):
        match = re.search(r'\b' + name + r':\s*(-?\d+)', params)
        if not match:
            raise ValueError('missing source movement dimension ' + name)
        return int(match[1])
    half_width, bottom = value('half_width'), value('bottom')
    if half_width <= 0 or bottom >= 0:
        raise ValueError('invalid source player support dimensions')
    if source is None:
        from source import Source
        source = Source()
    terrain = StaticTerrain(source)
    owned = {sid for region in report['regions'] for b in region.get('breakables', [])
             for sid in b.get('disabled_collider_sources', [])}
    results = []
    for region in report['regions']:
        path = root / region['path']
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if data[:8] != b'HKROOM02' or digest != region['sha256']:
            raise ValueError('flat support room hash/format mismatch')
        counts = struct.unpack_from('<6I', data, 8)
        start = 40 + counts[1] * 16 + counts[2] * 44 + counts[3] * 20 + counts[4] * 16
        metadata_path = root / f'data/regions/region-{region["chunk_id"]:03}/scene.json'
        metadata = json.loads(metadata_path.read_text())['edges']
        if len(metadata) != counts[5] or start + counts[5] * 16 > len(data):
            raise ValueError('flat support edge bounds mismatch')
        excluded = {i for b in region.get('breakables', []) for i in b['edge_indices']}
        edges = []
        for i, edge in enumerate(metadata):
            raw = struct.unpack_from('<4i', data, start + i * 16)
            if raw != tuple(round(v * 65536) for p in (edge['a'], edge['b']) for v in p):
                raise ValueError('flat support cooked/source edge mismatch')
            sid = edge['source']
            edges.append((raw, sid))
            if sid in owned:
                excluded.add(i)
            if i in excluded or raw[0] == raw[2] or raw[1] != raw[3]:
                continue
            record, source_edges = terrain.qualify(sid)
            if not record['qualified']:
                excluded.add(i)
            elif raw not in source_edges:
                raise ValueError('flat support original source edge mismatch: ' + sid)
        floors = full_floors(region['activation_bounds'], edges, excluded, half_width)
        results.append({'region': region['chunk_id'], 'pack_sha256': digest,
                        'scene_metadata_sha256': hashlib.sha256(metadata_path.read_bytes()).hexdigest(),
                        'excluded_edge_indices': sorted(excluded), 'floors': floors})
    provenance = {'half_width': half_width, 'player_bottom': bottom, 'max_heights': MAX_HEIGHTS,
                  'params_sha256': hashlib.sha256((root / 'data/params.rs').read_bytes()).hexdigest(),
                  'code_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  'regions': results, 'source_qualification': [p[0] for p in terrain.cache.values()]}
    directory = root / '.hkpsx'
    directory.mkdir(exist_ok=True)
    (directory / 'flat-support.json').write_text(json.dumps(provenance, indent=2))
    return [[f['height'] for f in r['floors']] for r in results]


def rust_catalog(floors):
    if any(len(f) > MAX_HEIGHTS for f in floors):
        raise ValueError('flat support catalogue bounds')
    return 'const FLAT_FLOOR_HEIGHTS: &[&[i32]] = &[\n' + \
        ''.join('    &[' + ','.join(map(str, f)) + '],\n' for f in floors) + '];'
