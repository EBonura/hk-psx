#!/usr/bin/env python3
"""Cook imported source scenes into isolated, measured PS1 scene packs.

P06 whole-world slice. The canonical Tutorial/Town cooker, similarity pass and
scene-bank packer run against any BuildSettings scene from the verified world
import, writing only under an ignored output root. A write guard aborts the
cook if any helper touches canonical `data/`, `.hkpsx/` reports or the disc.

Each result is capacity evidence for `tools/world_pack_matrix.py`, not a
playable room: actors, scripts, camera locks and transitions are inventoried,
never implemented, and the camera/activation envelope is a conservative
inventory rectangle derived from source records rather than measured camera
clamping.
"""
import argparse
import hashlib
import json
import math
import os
import re
import shutil
import sys
import tempfile
import time
import traceback
from pathlib import Path

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))

FORMAT = 'HKSCENEPACK01'
# ponytail: left/bottom/right/top activation apron in world units; the bottom
# margin matches the canonical SCENES -5 fall margin, the rest is one tile.
PAD = (1, 5, 1, 1)
VIEW = (24, 16)  # canonical initial_regions stepping
DEFAULT_ROOT = ROOT / '.hkpsx/scene-packs'
WORLD_REPORT = ROOT / '.hkpsx/world-import/report.json'
# The two ignored cook caches, which a pack cook is allowed to fill because both
# are content-addressed pure-function stores rather than reports: an entry either
# run adds is the same bytes the other would have computed, and both write
# through a pid-suffixed temp file so concurrent cooks cannot tear one.
# `texture_dedup` keys on the input pack, the replacements and a hash of the code
# that decides the result; `alpha_covers` keys on the image itself. They have to
# be named here because the guard below denies by default, and without them every
# cook died in the similarity pass on its first cache write, which is why no pack
# under .hkpsx/scene-packs had been recooked since the caches landed.
COOK_CACHES = (ROOT / '.hkpsx/dedup-cache', ROOT / '.hkpsx/alpha-covers-cache.json')
BUDGET_ERRORS = ('budget exceeded', 'pool exceeds', 'cache dimensions', 'bank exceeds', 'slot guest pool')
SCENE_ROOM_LIMIT = 128  # shared/hk-format/src/scene.rs header limit
PACK_RESTARTS = 3
OVER_BUDGET_MARKERS = BUDGET_ERRORS + ('exceeds', 'residency budgets')


def union(rects):
    return [min(r[0] for r in rects), min(r[1] for r in rects),
            max(r[2] for r in rects), max(r[3] for r in rects)]


def envelope(sc, info):
    """Derive activation and camera envelopes from tilemaps, gates and locks."""
    from regions import scene_metadata
    meta = scene_metadata(sc, info)
    tilemaps = []
    for _, (typ, tree) in sc.objects.items():
        if typ != 'tk2dTileMap':
            continue
        gid = tree['m_GameObject']['m_PathID']
        if not sc.active(gid):
            continue
        x, y, _ = sc.point(gid)
        tilemaps.append([x, y, x + tree['width'], y + tree['height']])
    rects = list(tilemaps)
    for gate in meta['gates']:
        x, y, _ = gate['position']
        off = gate['entry_offset']
        rects.append([x, y, x, y])
        rects.append([x + off['x'], y + off['y'], x + off['x'], y + off['y']])
        if 'trigger_bounds' in gate:
            rects.append(gate['trigger_bounds'])
    # Camera-lock trigger boxes are deliberately excluded: source authors size
    # them generously beyond the map, which would cook empty sky/void views.
    if not rects:
        return None, meta
    u = union(rects)
    runtime = [math.floor(u[0]) - PAD[0], math.floor(u[1]) - PAD[1],
               math.ceil(u[2]) + PAD[2], math.ceil(u[3]) + PAD[3]]
    # ponytail: the original camera clamps to tilemap dimensions; without a
    # tilemap the activation rectangle is the only source-derived choice.
    camera = union(tilemaps) if tilemaps else list(runtime)
    return dict(runtime_bounds=runtime, camera_global_bounds=camera, tilemaps=tilemaps), meta


def layout(runtime):
    left, bottom, right, top = runtime
    return [[x, y, min(x + VIEW[0], right), min(y + VIEW[1], top)]
            for y in range(bottom, top, VIEW[1]) for x in range(left, right, VIEW[0])]


def split(box):
    """Halve an over-budget view along its longer axis; None when too small."""
    x0, y0, x1, y1 = box
    if max(x1 - x0, y1 - y0) <= 3:
        return None
    if x1 - x0 >= y1 - y0:
        mid = (x0 + x1) // 2
        return [[x0, y0, mid, y1], [mid, y0, x1, y1]]
    mid = (y0 + y1) // 2
    return [[x0, y0, x1, mid], [x0, mid, x1, y1]]


def mandatory_packets(room):
    """Per-view mandatory packet reservation of a cooked HKROOM02 pack.

    Mirrors host/scenery_geometry.generate so an over-budget view is split before
    similarity/packing instead of failing the whole scene at pack time.
    """
    from scenery_geometry import packet_bound, CHILD_CAPACITY
    import struct
    if room[:8] != b'HKROOM02':
        raise ValueError('unsupported room format')
    counts = struct.unpack_from('<6I', room, 8)
    draw_start = 40 + counts[1] * 16
    total = 0
    for index in range(counts[2]):
        pos = draw_start + index * 44
        texture, = struct.unpack_from('<H', room, pos)
        coords = struct.unpack_from('<8i', room, pos + 8)
        _, _, _, w, h = struct.unpack_from('<5H', room, 40 + texture * 16)
        packets = packet_bound(list(zip(coords[::2], coords[1::2])), w, h)['packets']
        if packets > CHILD_CAPACITY:
            raise ValueError(f'draw {index} needs {packets} children > {CHILD_CAPACITY}')
        total += packets
    return total


def failing_region(error):
    """Chunk id named by a pack-time error such as 'region25' or 'Scene region 17'."""
    match = re.search(r'region ?(\d+)', str(error))
    return int(match.group(1)) if match else None


def install_guard(allowed, caches=COOK_CACHES):
    """Abort on any write outside the allowed roots (plus /dev/null and temp)."""
    allowed = [Path(p).resolve() for p in allowed] + [Path(tempfile.gettempdir()).resolve()]
    caches = [Path(p).resolve() for p in caches]

    def permitted(path):
        path = Path(os.fsdecode(path)).resolve()
        if path == Path('/dev/null') or 'target' in path.parts:
            return True
        # A cache file and the `<stem>.<pid>.tmp` it is renamed from.
        if any(path.is_relative_to(c) or (path.parent == c.parent and path.name.startswith(c.stem))
               for c in caches):
            return True
        return any(path.is_relative_to(root) for root in allowed)

    def guard(event, args):
        paths = []
        # `mkdir(parents=True, exist_ok=True)` on a cache reaches for .hkpsx
        # itself first. Creating a directory that is already there changes
        # nothing, so it is the write that never happens rather than one to
        # forgive; every other event still goes through `permitted`.
        if event == 'os.mkdir' and Path(os.fsdecode(args[0])).is_dir():
            return
        if event == 'open':
            path, mode, flags = args
            if isinstance(path, (str, bytes, os.PathLike)) and (
                    (mode and any(c in mode for c in 'wax+'))
                    or flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC | os.O_APPEND)):
                paths = [path]
        elif event in ('os.remove', 'os.rmdir') and len(args) > 1 and args[1] is not None:
            return  # dir_fd-relative entries: only shutil.rmtree of a permitted root uses them here
        elif event in ('os.mkdir', 'os.remove', 'os.rmdir'):
            paths = [args[0]]
        elif event in ('os.rename', 'os.replace'):
            paths = [args[0], args[1]]
        for path in paths:
            if not permitted(path):
                raise RuntimeError('BLOCKED WRITE OUTSIDE SCENE PACK ROOT: ' + str(path))
    sys.addaudithook(guard)


def catalog(report_path=WORLD_REPORT):
    report = json.loads(Path(report_path).read_text())
    return {s['scene_name']: s for s in report['scenes']}


def source_sha(source, file):
    with (source.directory / file).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def cook_scene(source, entry, out, log=print, forced_boxes=None, restarts=PACK_RESTARTS):
    """Cook one scene into `out`; returns the summary written to out/summary.json.

    `forced_boxes` replaces the derived view layout when a pack-time failure
    named one view: that view is halved and the whole scene is recooked, since
    the similarity pass forbids partial recooks.
    """
    from source import dump
    from scene import Scene
    from cook import cook
    from regions import region_spec, intersects
    from similarity_dedup import postpack_similarity
    from reveal_masks import reveal_mask_sources, bind_regions
    from world import postpack_checkpoints, postpack_masks
    from pack_scenes import pack_scenes
    from scenery_geometry import CAP, ACTOR_RESERVE
    from regions import over_budget

    started = time.time()
    out = Path(out).resolve()
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    scene_id = entry['index']
    info = dict(scene_id=scene_id, scene_name=entry['scene_name'], file=entry['file'])
    summary = dict(format=FORMAT, playable_room=False, scene_name=entry['scene_name'], file=entry['file'],
                   scene_id=scene_id, source_sha256=source_sha(source, entry['file']),
                   activation_pad=list(PAD), view_step=list(VIEW), output=str(out))
    try:
        sc = Scene(source, entry['file'])
        env, meta = envelope(sc, info)
        summary.update(gates=len(meta['gates']), camera_locks=len(meta['camera_locks']),
                       unsupported_scene_errors=list(sc.errors))
        if env is None:
            summary.update(status='no_envelope', elapsed_seconds=round(time.time() - started, 1))
            dump(out / 'summary.json', summary)
            return summary
        info.update(runtime_bounds=env['runtime_bounds'], camera_global_bounds=env['camera_global_bounds'])
        summary.update(envelope=env['runtime_bounds'], camera_global_bounds=env['camera_global_bounds'],
                       tilemaps=env['tilemaps'])
        boxes = forced_boxes or layout(env['runtime_bounds'])
        summary['pack_restarts'] = PACK_RESTARTS - restarts
        from regions import scene_metadata
        report = {'format': 'HKREGIONS01', 'complete': False, 'initial_chunk_id': 1,
                  'pending_regions': len(boxes), 'regions': [], 'scenes': [scene_metadata(sc, info)],
                  'quality': {'scenery_max_axis': 48, 'animation_sampling': 'unchanged', 'similarity_threshold': 95},
                  'limitations': ['Isolated capacity pack: no playable integration, actor bank or pogo generation.']}
        pixels = {}
        geometry = {}
        grass_indices = {sid: index for index, (sid, (typ, _)) in enumerate(
            item for item in sc.objects.items() if item[1][0] == 'GrassCut')}
        queue = list(boxes)
        splits = 0
        while queue:
            box = queue.pop(0)
            i = len(report['regions']) + 1
            r = region_spec(info, box)
            region_out = out / 'base' / f'region-{i:03}'
            log(f'  cook {i} (+{len(queue)} queued) {box}')
            try:
                c = cook(source, sc, r, region_out, pixels, geometry, write_shared=False, stage_for_similarity=True)
                packets = mandatory_packets((region_out / 'room.hk').read_bytes())
                if packets + ACTOR_RESERVE > CAP:
                    raise ValueError(f'packet budget exceeded: {packets}+{ACTOR_RESERVE} > {CAP}')
                # Similarity only shrinks a view, so the post-similarity runtime
                # budget is checked conservatively on the fresh cook. The rule is
                # `regions.over_budget` rather than a copy of it: this restated it
                # with records gated on the 416-slot CLUT budget and palettes not
                # counted at all, which split views that fit and reported a
                # texture count where a reader expected a CLUT figure.
                if over_budget(c['pages'], c['textures'], c['cluts'], c['pack_bytes']):
                    raise ValueError(f'view budget exceeded: {c["pages"]} pages/{c["textures"]} textures/'
                                     f'{c["cluts"]} cluts/{c["pack_bytes"]} bytes')
            except ValueError as error:
                halves = split(box) if any(text in str(error) for text in BUDGET_ERRORS) else None
                if halves is None:
                    raise
                splits += 1
                queue[:0] = halves
                continue
            path = out / f'chunk_{i}.hk'
            shutil.copyfile(region_out / 'room.hk', path)
            row = dict(r, chunk_id=i, path=str(path), base_path=str(region_out / 'room.hk'),
                       bytes=c['pack_bytes'], sha256=c['pack_sha256'])
            row['mandatory_packets'] = packets
            for k in ['pages', 'textures', 'cluts', 'draws', 'edges', 'stream_bytes', 'animation_bytes', 'alpha_cover_bytes',
                      'format_features', 'grass', 'grass_impact', 'door_debris', 'particle_effects', 'breakables',
                      'actors', 'hazards', 'edge_sources', 'texture_request_to_canonical']:
                row[k] = c[k]
            for grass in row['grass']:  # same state identity as the canonical cook
                grass['state_index'] = grass_indices[int(grass['source'].split(':')[-1])]
                grass['bounds'] = grass['box']
            report['regions'].append(row)
            report['pending_regions'] = len(queue)
        summary['view_splits'] = splits
        report['complete'] = True
        if len(report['regions']) > SCENE_ROOM_LIMIT:
            raise ValueError(f'{len(report["regions"])} views exceeds the HKSCNE room limit of {SCENE_ROOM_LIMIT}')
        for r in report['regions']:
            x0, y0, x1, y1 = r['activation_bounds']
            r['neighbour_chunks'] = [o['chunk_id'] for o in report['regions'] if o is not r
                                     and intersects([x0 - .01, y0 - .01, x1 + .01, y1 + .01], o['activation_bounds'])]
        draws = {r['chunk_id']: [d['source'] for d in json.loads(
            (Path(r['base_path']).with_name('scene.json')).read_text())['draws']] for r in report['regions']}
        postpack_checkpoints(report, source, {scene_id: sc})
        postpack_masks(report, draws)
        room_bytes = lambda: {r['chunk_id']: Path(r['path']).read_bytes() for r in report['regions']}
        masks = {scene_id: reveal_mask_sources(sc)}
        bind_regions(report, masks, draws, room_bytes())
        log('  similarity')
        postpack_similarity(report, root=out)
        postpack_masks(report, draws)
        bind_regions(report, masks, draws, room_bytes())
        dump(out / 'regions.json', report)
        log('  pack scene')
        try:
            packed = pack_scenes(out / 'regions.json', out / 'scenes', out / 'packed-scenes.json',
                                 out / 'scene_manifest.rs', residency='scene_gate', generate_world=False)
        except ValueError as error:
            chunk = failing_region(error)
            row = next((r for r in report['regions'] if r['chunk_id'] == chunk), None)
            halves = split(row['activation_bounds']) if row else None
            if halves is None or restarts <= 0:
                raise
            log(f'  pack failed on view {chunk} ({error}); halving it and recooking the scene')
            boxes = [b for r in report['regions'] for b in ([r['activation_bounds']] if r is not row else halves)]
            return cook_scene(source, entry, out, log, forced_boxes=boxes, restarts=restarts - 1)
        bank = packed['scenes'][0]
        errors = {}
        for r in report['regions']:
            unsupported = json.loads((Path(r['base_path']).with_name('unsupported.json')).read_text())
            for e in unsupported['errors']:
                errors.setdefault(e.get('id') or e.get('source') or json.dumps(e, sort_keys=True), e)
        actors = {a['source']: dict(source=a['source'], name=a.get('name'), movement_supported=a['movement_supported'])
                  for r in report['regions'] for a in r['actors']}
        summary.update(
            status='cooked', regions=len(report['regions']),
            resident_bytes=packed['required_resident_bytes'], resident_stored_bytes=packed['resident_stored_bytes'],
            pages=packed['total_pages'], page_bytes=packed['total_pages'] * 32768,
            palettes=packed['total_palettes'], palette_bytes=packed['total_palettes'] * 32,
            atlas_raw_bytes=packed['atlas_raw_bytes'], atlas_stored_bytes=packed['atlas_stored_bytes'],
            scene_bank_bytes=bank['bytes'], textures=bank['textures'],
            decoder_status=packed['sequential_decoder_validation']['status'],
            geometry_packet_bound=bank.get('geometry_packet_bound', {}).get('status'),
            unsupported_cook_errors=sorted(errors.values(), key=lambda e: json.dumps(e, sort_keys=True)),
            actors=sorted(actors.values(), key=lambda a: a['source']),
            unique_breakables=len({b['source'] for r in report['regions'] for b in r['breakables']}),
            unique_grass=len({g['source'] for r in report['regions'] for g in r['grass']}),
            packed_report=str(out / 'packed-scenes.json'))
    except Exception as ex:  # record, never abort a whole-world batch
        if 'report' in locals():
            dump(out / 'regions-partial.json', report)
        text = str(ex)
        status = 'over_budget' if any(m in text for m in OVER_BUDGET_MARKERS) else 'failed'
        summary.update(status=status, error=repr(ex), traceback=traceback.format_exc(),
                       regions=len(locals().get('report', {}).get('regions', [])))
    summary['elapsed_seconds'] = round(time.time() - started, 1)
    dump(out / 'summary.json', summary)
    return summary


def read_summaries(root=DEFAULT_ROOT):
    root = Path(root)
    result = {}
    for path in sorted(root.glob('*/summary.json')):
        s = json.loads(path.read_text())
        if s.get('format') == FORMAT:
            result[s['scene_name']] = dict(s, summary_path=str(path))
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('scenes', nargs='*', help='scene names from the world import catalog')
    p.add_argument('--all', action='store_true', help='cook every processed catalog scene')
    p.add_argument('--output-root', type=Path, default=DEFAULT_ROOT)
    p.add_argument('--world', type=Path, default=WORLD_REPORT)
    p.add_argument('--force', action='store_true', help='recook scenes whose source is unchanged')
    p.add_argument('--shard', default='0/1', metavar='K/N',
                   help='cook only every Nth scene starting at K, so N processes can share --all')
    a = p.parse_args()
    shard, shards = (int(v) for v in a.shard.split('/'))
    if not 0 <= shard < shards:
        p.error('--shard must be K/N with 0 <= K < N')
    if not a.scenes and not a.all:
        p.error('name scenes or pass --all')
    from source import Source
    entries = catalog(a.world)
    missing = [s for s in a.scenes if s not in entries]
    if missing:
        p.error('unknown scenes: ' + ', '.join(missing))
    names = sorted(entries) if a.all else a.scenes
    names = names[shard::shards]
    root = a.output_root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    install_guard([root])
    source = Source()
    done = read_summaries(root)
    for index, name in enumerate(names, 1):
        entry = entries[name]
        previous = done.get(name)
        if previous and not a.force and previous.get('source_sha256') == source_sha(source, entry['file']) \
                and previous.get('status') in ('cooked', 'over_budget', 'no_envelope'):
            print(f'[{index}/{len(names)}] {name}: kept {previous["status"]}', flush=True)
            continue
        print(f'[{index}/{len(names)}] {name} ({entry["file"]})', flush=True)
        s = cook_scene(source, entry, root / name, log=lambda m: print(m, flush=True))
        brief = {k: s.get(k) for k in ('status', 'regions', 'resident_bytes', 'pages', 'palettes', 'decoder_status', 'error')
                 if s.get(k) is not None}
        print(f'[{index}/{len(names)}] {name}: {json.dumps(brief)}', flush=True)


if __name__ == '__main__':
    main()
