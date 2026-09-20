#!/usr/bin/env python3
"""Compare canonical Tutorial/Town cooks with the verified world import.

The canonical region catalogue has two pack layouts: post-packed Tutorial rows
carry ``base_path``, while Town rows point directly at their final pack.  Region
identity and final-pack integrity therefore come from regions-provenance.json;
``base_path`` is an optional optimization detail, not an admission filter.
"""
import argparse
from collections import defaultdict
import gzip
import hashlib
import json
import math
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
from cook import cooked_edge
from scene import ADDITIVE_ID_BASE

SCENES = {'Tutorial_01': 'level6', 'Town': 'level7'}


def file_sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def require_verified_import(report):
    coverage = report.get('coverage', {})
    if (report.get('run', {}).get('status') != 'verified' or
            not report.get('inputs_unchanged') or
            not coverage.get('all_scenes_processed') or
            coverage.get('failed_scenes')):
        raise ValueError('World import is not complete, failure-free and source-verified')


def provenance_map(provenance):
    rows = provenance.get('regions', [])
    result = {}
    for row in rows:
        chunk = row.get('chunk_id')
        if not isinstance(chunk, int) or chunk in result:
            raise ValueError('Invalid or duplicate region provenance chunk_id')
        result[chunk] = row
    return result


def resolve_region(root, region, provenance):
    """Resolve and verify a canonical region without requiring base_path."""
    chunk = region['chunk_id']
    recorded = provenance.get(chunk)
    if recorded is None:
        raise ValueError(f'Region {chunk} has no provenance record')
    if recorded.get('scene_name') != region.get('scene_name'):
        raise ValueError(f'Region {chunk} scene/provenance mismatch')
    if recorded.get('path') != region.get('path'):
        raise ValueError(f'Region {chunk} pack path/provenance mismatch')
    pack = root / recorded['path']
    if not pack.is_file():
        raise ValueError(f'Region {chunk} final pack is missing')
    digest = file_sha256(pack)
    if digest != recorded.get('sha256') or digest != region.get('sha256'):
        raise ValueError(f'Region {chunk} final pack hash mismatch')
    if pack.stat().st_size != recorded.get('bytes') or pack.stat().st_size != region.get('bytes'):
        raise ValueError(f'Region {chunk} final pack size mismatch')

    base = region.get('base_path')
    method = 'base_path'
    if base:
        base_pack = root / base
        if not base_pack.is_file() or file_sha256(base_pack) != region.get('base_sha256'):
            raise ValueError(f'Region {chunk} base pack hash mismatch')
        metadata = base_pack.with_name('scene.json')
    else:
        # The final pack is also the base pack for rows with no post-pack stage.
        # Its metadata remains in the canonical chunk work directory.
        method = 'provenance_chunk_id'
        metadata = root / 'data/regions' / f'region-{chunk:03}' / 'scene.json'
    if not metadata.is_file():
        raise ValueError(f'Region {chunk} canonical scene metadata is missing')
    return {'chunk_id': chunk, 'scene_name': region['scene_name'],
            'resolution': method, 'pack': str(pack.relative_to(root)),
            'pack_sha256': digest, 'metadata': str(metadata.relative_to(root)),
            'metadata_sha256': file_sha256(metadata)}


def point_key(point):
    return tuple(float(value) for value in point)


def quad_key(points):
    # Cooked quads and source quads use the same four corners, but order is not
    # a semantic property for this comparison.
    return tuple(sorted(point_key(point) for point in points))


def stored_edge_key(edge):
    """One already-cooked edge, keyed as host/cook.py wrote it."""
    return edge['source'], tuple(edge['a'][:2]), tuple(edge['b'][:2])


def imported_edge_keys(edge, regions):
    """The cooked forms of one imported source segment, over the regions of a scene.

    The clip, the slope refusal and the snap are host/cook.py::cooked_edge, so
    this reproduces nothing: an edge outside every region's collision_bounds, or
    sloped in a region that refuses slopes, is never cooked and must not be
    counted as an extra.
    """
    keys = set()
    for region in regions:
        segment, _ = cooked_edge(edge['a'], edge['b'], region)
        if segment is not None:
            keys.add((edge['source'], tuple(segment[0]), tuple(segment[1])))
    return keys


def merged_origin(source, level):
    """True when the two sides cannot name this object the same way.

    host/cook.py records an edge under `Scene.sid`, which is the file the object
    was really serialized in; host/world_geometry.py records it under the room's
    own file name and the merged id. For an object that came in through an
    additive load those two strings never match, and the import output keeps no
    origin to translate between them. Counting such an edge as both missing and
    extra is what the pogo measurement did, so they are named instead.
    """
    file, _, ident = source.rpartition(':')
    return file != level or (ident.isdigit() and int(ident) >= ADDITIVE_ID_BASE)


def close_tuple(left, right, tolerance=1e-8):
    return len(left) == len(right) and all(math.isclose(a, b, rel_tol=0, abs_tol=tolerance)
                                           for a, b in zip(left, right))


def compare_scene(scene_name, level, geometry, cooked_scenes, regions):
    imported_quads = defaultdict(list)
    for row in geometry.get('sprites', []):
        if row.get('world_quad') is not None:
            imported_quads[row['source']].append(quad_key(row['world_quad']))
    for row in geometry.get('tilemap_fills', []):
        imported_quads[row['source']].extend(quad_key(quad) for quad in row['world_quads'])

    cooked_quads = {}
    cooked_edges = {}
    draw_occurrences = edge_occurrences = 0
    for cooked in cooked_scenes:
        draw_occurrences += len(cooked['draws'])
        edge_occurrences += len(cooked['edges'])
        for draw in cooked['draws']:
            cooked_quads[(draw['source'], quad_key(draw['points']))] = draw
        for edge in cooked['edges']:
            cooked_edges[stored_edge_key(edge)] = edge

    missing_draws = []
    changed_draws = []
    for source, points in cooked_quads:
        candidates = imported_quads.get(source, [])
        if not candidates:
            missing_draws.append(source)
        elif not any(all(close_tuple(a, b) for a, b in zip(points, candidate))
                     for candidate in candidates):
            changed_draws.append(source)

    imported_edges = set()
    merged_imported = set()
    for edge in geometry.get('terrain_edges', []):
        (merged_imported if merged_origin(edge['source'], level) else imported_edges).update(
            imported_edge_keys(edge, regions))
    merged_cooked = {key for key in cooked_edges if merged_origin(key[0], level)}
    missing_edges = sorted(cooked_edges.keys() - imported_edges - merged_cooked)
    extra_edges = sorted(imported_edges - cooked_edges.keys())
    return {'scene_name': scene_name, 'scene_file': level,
            'region_count': len(cooked_scenes),
            'additive_merge_identity_gaps': {
                'cooked_edges': len(merged_cooked), 'imported_edges': len(merged_imported),
                'reason': 'host/cook.py names these under the file they were serialized in and '
                          'host/world_geometry.py under this room and the merged id; the import '
                          'output keeps no origin to translate between them, so they are excluded '
                          'rather than counted as missing and extra at once'},
            'cooked_draw_occurrences': draw_occurrences,
            'cooked_unique_draw_geometry': len(cooked_quads),
            'imported_draw_sources': len(imported_quads),
            'missing_draw_sources': sorted(set(missing_draws)),
            'changed_draw_sources': sorted(set(changed_draws)),
            'cooked_edge_occurrences': edge_occurrences,
            'cooked_unique_edges': len(cooked_edges),
            'imported_edges': len(imported_edges),
            'missing_edges': [list(row) for row in missing_edges],
            'extra_edges': [list(row) for row in extra_edges],
            'explicit_import_gaps': geometry.get('unsupported', []),
            'geometry_errors': geometry.get('errors', []),
            'comparison_passed': not (missing_draws or changed_draws or missing_edges or extra_edges)}


def build(root=ROOT, world_dir=None):
    root = Path(root)
    world_dir = Path(world_dir) if world_dir else root / '.hkpsx/world-import'
    import_report = json.loads((world_dir / 'report.json').read_text())
    require_verified_import(import_report)
    regions = json.loads((root / 'data/regions.json').read_text())
    provenance_doc = json.loads((root / '.hkpsx/regions-provenance.json').read_text())
    if not regions.get('complete') or not provenance_doc.get('complete'):
        raise ValueError('Canonical region cook/provenance is incomplete')
    provenance = provenance_map(provenance_doc)

    resolved = []
    scene_metadata = defaultdict(list)
    scene_regions = defaultdict(list)
    inputs = {}
    for region in regions['regions']:
        if region.get('scene_name') not in SCENES:
            continue
        row = resolve_region(root, region, provenance)
        resolved.append(row)
        metadata_path = root / row['metadata']
        scene_metadata[region['scene_name']].append(json.loads(metadata_path.read_text()))
        # cooked_edge reads collision_bounds and allow_slopes off this row.
        scene_regions[region['scene_name']].append(region)
        inputs[row['metadata']] = row['metadata_sha256']
        inputs[row['pack']] = row['pack_sha256']

    scenes = []
    for scene_name, level in SCENES.items():
        result = json.loads((world_dir / level / 'result.json').read_text())
        if result.get('fingerprint') != import_report.get('fingerprint'):
            raise ValueError(f'{level} output belongs to a different world import')
        geometry_path = world_dir / level / result['outputs']['geometry']['path']
        digest = file_sha256(geometry_path)
        if digest != result['outputs']['geometry']['sha256']:
            raise ValueError(f'{level} geometry output hash mismatch')
        inputs[str(geometry_path.relative_to(root))] = digest
        with gzip.open(geometry_path, 'rt') as stream:
            geometry = json.load(stream)
        scenes.append(compare_scene(scene_name, level, geometry, scene_metadata[scene_name],
                                    scene_regions[scene_name]))

    expected = {r['chunk_id'] for r in regions['regions'] if r.get('scene_name') in SCENES}
    observed = {r['chunk_id'] for r in resolved}
    if expected != observed:
        raise ValueError('Not every canonical Tutorial/Town region was compared')
    methods = defaultdict(int)
    for row in resolved:
        methods[row['resolution']] += 1
    passed = all(scene['comparison_passed'] for scene in scenes)
    return {'format': 'HKWORLDREGIONCOMPARE01',
            'scope': 'Canonical Tutorial/Town source geometry comparison; no guest admission claim',
            'world_fingerprint': import_report['fingerprint'],
            'region_count': len(resolved), 'expected_region_count': len(expected),
            'region_resolution_counts': dict(sorted(methods.items())),
            'regions': resolved, 'scenes': scenes, 'inputs': dict(sorted(inputs.items())),
            'comparison_passed': passed,
            'limitations': [
                'Cooked scenery is spatially repeated across resident views; unique source geometry is compared once.',
                'Imported terrain edges go through host/cook.py::cooked_edge before comparison, so the collision-bounds clip, the slope refusal and the near-axis snap are the cooker policy itself rather than a copy of it.',
                'An edge whose object reached the room through an additive load is counted in additive_merge_identity_gaps, not compared: the two sides name it under different files and the import output carries no origin to translate.',
                'Explicit TextMesh/TextMeshPro runtime glyph gaps are retained and are not scenery or terrain mismatches.',
                'This comparison does not establish guest memory fit, rendering parity or gameplay behavior.',
            ]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--world-dir', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    report = build(args.root, args.world_dir)
    output = args.output or args.root / '.hkpsx/world-import/tutorial-town-comparison.json'
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: report[key] for key in
          ('region_count', 'region_resolution_counts', 'comparison_passed')}, sort_keys=True))
    if not report['comparison_passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
