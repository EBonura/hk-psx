"""Read-only, resumable dependency inventory of every Windows BuildSettings scene.

No image pixels are decoded or cooked here. Source atlas sizes describe source
storage, never the PS1 residency of cropped/resampled sprite fragments.
"""
import argparse
from collections import Counter
import gc
import hashlib
import json
import math
from pathlib import Path
import time
from source import ROOT, Source

VERSION = 1
OUTPUT = ROOT / '.hkpsx/room-inventory.json'


def atomic_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + '.tmp')
    temporary.write_text(json.dumps(data, separators=(',', ':')) + '\n')
    temporary.replace(path)


def key(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'))


def pointer_refs(value):
    """Unique non-null serialized PPtrs, without interpreting FSM parameters."""
    refs = set()
    def visit(item):
        if isinstance(item, dict):
            if 'm_FileID' in item and 'm_PathID' in item:
                if item['m_PathID']:
                    refs.add((item['m_FileID'], item['m_PathID']))
                return
            for child in item.values(): visit(child)
        elif isinstance(item, (list, tuple)):
            for child in item: visit(child)
    visit(value)
    return [{'m_FileID': file, 'm_PathID': path} for file, path in sorted(refs)]


class Inventory:
    def __init__(self, report):
        self.report = report
        self.atlases = {}
        self.collections = {}
        self.animations = {}
        self.materials = {}
        self.loaded_files = set(report.get('source_file_names', []))
        self.stream_files = {t['source_stream_file'] for t in report['textures'].values() if t.get('source_stream_file')}

    def texture(self, source, obj):
        sid = source.sid(obj)
        if sid in self.report['textures']:
            return sid
        tree = source.read(obj)
        if obj.type.name != 'Texture2D':
            raise ValueError(f'Non-Texture2D backing dependency: {obj.type.name}')
        width, height = tree['m_Width'], tree['m_Height']
        stream = tree.get('m_StreamData', {})
        if stream.get('path'):
            self.stream_files.add(Path(stream['path']).name)
        self.report['textures'][sid] = {
            'name': tree['m_Name'], 'width': width, 'height': height,
            'source_format': tree['m_TextureFormat'], 'mip_count': tree['m_MipCount'],
            'serialized_bytes': obj.byte_size,
            'source_image_bytes': tree['m_CompleteImageSize'],
            'source_stream_bytes': stream.get('size', 0),
            'source_stream_file': Path(stream.get('path', '')).name,
            'source_stream_offset': stream.get('offset', 0),
            'source_size_4bpp_estimate_bytes': (width * height + 1) // 2,
            'cooked_ram_bytes': None, 'cooked_vram_bytes': None,
        }
        return sid

    def texture_refs(self, source, file, render):
        result = set()
        for name in ('texture', 'alphaTexture'):
            ref = render.get(name)
            if ref and ref.get('m_PathID'):
                result.add(self.texture(source, source.ref(file, ref)))
        for secondary in render.get('secondaryTextures', []):
            ref = secondary.get('texture')
            if ref and ref.get('m_PathID'):
                result.add(self.texture(source, source.ref(file, ref)))
        return sorted(result)

    def sprite(self, source, obj):
        sid = source.sid(obj)
        if sid in self.report['sprites']:
            return sid
        tree = source.read(obj)
        render, file = tree['m_RD'], obj.assets_file
        atlas_id = None
        if tree.get('m_SpriteAtlas', {}).get('m_PathID'):
            atlas = source.ref(file, tree['m_SpriteAtlas'])
            atlas_id = source.sid(atlas)
            if atlas_id not in self.atlases:
                self.atlases[atlas_id] = {key(k): v for k, v in source.read(atlas)['m_RenderDataMap']}
            render = self.atlases[atlas_id][key(tree['m_RenderDataKey'])]
            file = atlas.assets_file
        rect = tree['m_Rect']
        self.report['sprites'][sid] = {
            'kind': 'Sprite', 'name': tree['m_Name'],
            'backing_texture_ids': self.texture_refs(source, file, render),
            'source_dimensions': [rect['width'], rect['height']],
            'packed_rect': render['textureRect'], 'atlas_id': atlas_id,
            'pixels_per_unit': tree['m_PixelsToUnits'],
            'cooked_ram_bytes': None, 'cooked_vram_bytes': None,
        }
        return sid

    def material(self, source, obj):
        sid = source.sid(obj)
        if sid not in self.materials:
            result = []
            tree = source.read(obj)
            for name, value in tree['m_SavedProperties']['m_TexEnvs']:
                ref = value['m_Texture']
                if ref['m_PathID']:
                    backing = source.ref(obj.assets_file, ref)
                    result.append(self.texture(source, backing))
            self.materials[sid] = sorted(set(result))
        return self.materials[sid]

    def collection(self, source, obj):
        sid = source.sid(obj)
        if sid not in self.collections:
            tree = source.read(obj)
            definitions = []
            for index, definition in enumerate(tree['spriteDefinitions']):
                if not definition.get('material', {}).get('m_PathID'):
                    definitions.append(None)
                    continue
                backing = self.material(source, source.ref(obj.assets_file, definition['material']))
                positions = definition.get('positions', [])
                texel = definition.get('texelSize', {})
                bounds = [min(p['x'] for p in positions), min(p['y'] for p in positions),
                          max(p['x'] for p in positions), max(p['y'] for p in positions)] if positions else None
                dimensions = None
                if bounds and texel.get('x') and texel.get('y'):
                    dimensions = [abs((bounds[2]-bounds[0])/texel['x']), abs((bounds[3]-bounds[1])/texel['y'])]
                sprite_id = f'{sid}:sprite:{index}'
                self.report['sprites'][sprite_id] = {
                    'kind': 'tk2d', 'name': definition['name'], 'collection_id': sid,
                    'definition_index': index, 'backing_texture_ids': backing,
                    'source_dimensions': dimensions, 'source_bounds': bounds,
                    'cooked_ram_bytes': None, 'cooked_vram_bytes': None,
                }
                definitions.append(sprite_id)
            self.collections[sid] = definitions
        return self.collections[sid]

    def animation(self, source, obj):
        sid = source.sid(obj)
        if sid not in self.animations:
            tree = source.read(obj)
            sprites = set()
            clips, unresolved = [], []
            for clip in tree['clips']:
                frames = []
                for frame_index, frame in enumerate(clip['frames']):
                    collection_id = None
                    try:
                        collection = source.ref(obj.assets_file, frame['spriteCollection'])
                        collection_id = source.sid(collection)
                        definitions = self.collection(source, collection)
                        index = frame['spriteId']
                        if index < 0 or index >= len(definitions) or definitions[index] is None:
                            raise ValueError(f'Unresolved animation sprite {collection_id}:{index}; collection has {len(definitions)} definitions')
                        sprite = definitions[index]
                        frames.append(sprite); sprites.add(sprite)
                    except Exception as error:
                        # Preserve the frame's position and known source identity;
                        # one malformed or variant-dependent frame must not erase
                        # the other valid dependencies in this animation library.
                        frames.append(None)
                        unresolved.append({'clip': clip['name'], 'frame_index': frame_index,
                            'collection_id': collection_id, 'sprite_index': frame['spriteId'],
                            'source_collection_ref': frame['spriteCollection'], 'error': str(error)})
                clips.append({'name': clip['name'], 'fps': clip['fps'], 'frame_sprite_ids': frames})
            self.report['animations'][sid] = {'clips': clips, 'sprite_ids': sorted(sprites),
                                               'unresolved_frames': unresolved}
            self.animations[sid] = sorted(sprites)
        return sid, self.animations[sid]

    def fsm(self, source, obj, sprites, textures, animations):
        tree = source.read(obj)  # Strict full-length parse, not a schema prefix.
        fsm = tree.get('fsm') or {}
        states = fsm.get('states') or []
        actions = Counter(name for state in states for name in (state.get('actionData') or {}).get('actionNames', []))
        result = {'source_id': source.sid(obj), 'name': fsm.get('name', ''), 'states': len(states),
                  'actions': dict(actions), 'direct_asset_ids': [], 'unresolved_object_refs': [], 'errors': []}
        # Conservative possible dependencies, including disabled states/actions.
        # This is literal object reachability, not execution or dynamic closure.
        for ref in pointer_refs({'fsm': fsm, 'fsmTemplate': tree.get('fsmTemplate')}):
            target_id = None
            try:
                target = source.ref(obj.assets_file, ref)
                target_id = source.sid(target)
                kind = source.typename(target)
                if kind == 'Sprite': sprites.add(self.sprite(source, target))
                elif kind == 'Texture2D': textures.add(self.texture(source, target))
                elif kind == 'Material': textures.update(self.material(source, target))
                elif kind == 'tk2dSpriteCollectionData':
                    sprites.update(sprite for sprite in self.collection(source, target) if sprite)
                elif kind == 'tk2dSpriteAnimation':
                    animation, refs = self.animation(source, target)
                    animations.add(animation); sprites.update(refs)
                else:
                    result['unresolved_object_refs'].append({'source_id': target_id, 'type': kind})
                    continue
                result['direct_asset_ids'].append(target_id)
            except Exception as error:
                result['errors'].append({'ref': ref, 'source_id': target_id, 'error': str(error)})
        return result

    def scan(self, source, scene):
        file = source.file(scene['file'])
        room = dict(scene, scene_name=Path(scene['path']).stem, texture_ids=[], sprite_ids=[],
                    animation_ids=[], sprite_instances=[], unsupported=[], cooked_coverage='unknown',
                    playmaker={'parsed': [], 'parse_errors': []})
        scripts = []
        types = Counter()
        unsupported = {}
        sprite_ids, textures, animations = set(), set(), set()
        def failed(obj, kind, error):
            room['unsupported'].append({'id': source.sid(obj), 'type': kind, 'error': str(error)})
        for obj in list(file.objects.values()):
            kind = obj.type.name
            try:
                if kind == 'MonoBehaviour':
                    kind = source.typename(obj)
                    scripts.append((obj, kind))
                types[kind] += 1
                if kind == 'SpriteRenderer':
                    tree = source.read(obj)
                    if tree['m_Sprite']['m_PathID']:
                        sprite = self.sprite(source, source.ref(file, tree['m_Sprite']))
                        sprite_ids.add(sprite)
                        room['sprite_instances'].append({'source_id': source.sid(obj), 'sprite_id': sprite,
                            'game_object_id': f"{scene['file']}:{tree['m_GameObject']['m_PathID']}",
                            'component_enabled': bool(tree['m_Enabled'])})
                    for ref in tree['m_Materials']:
                        if ref['m_PathID']:
                            textures.update(self.material(source, source.ref(file, ref)))
                elif kind in ('tk2dSprite', 'tk2dSlicedSprite', 'tk2dTiledSprite', 'tk2dClippedSprite'):
                    tree = source.read(obj)
                    definitions = self.collection(source, source.ref(file, tree['collection']))
                    index = tree['_spriteId']
                    if index < 0 or index >= len(definitions) or definitions[index] is None:
                        raise ValueError(f'Invalid tk2d sprite index {index}')
                    sprite = definitions[index]; sprite_ids.add(sprite)
                    room['sprite_instances'].append({'source_id': source.sid(obj), 'sprite_id': sprite,
                        'game_object_id': f"{scene['file']}:{tree['m_GameObject']['m_PathID']}",
                        'component_enabled': bool(tree['m_Enabled'])})
                elif kind == 'tk2dSpriteAnimator':
                    tree = source.read(obj)
                    if tree['library']['m_PathID']:
                        animation, refs = self.animation(source, source.ref(file, tree['library']))
                        animations.add(animation); sprite_ids.update(refs)
                        unresolved = self.report['animations'][animation]['unresolved_frames']
                        if unresolved:
                            room['unsupported'].append({'id': source.sid(obj), 'type': kind,
                                'animation_id': animation, 'unresolved_frame_count': len(unresolved),
                                'error': 'Animation includes unresolved frame references; valid frames retained'})
                elif kind in ('MeshRenderer', 'SkinnedMeshRenderer', 'ParticleSystemRenderer'):
                    tree = source.read(obj)
                    for ref in tree['m_Materials']:
                        if ref['m_PathID']:
                            textures.update(self.material(source, source.ref(file, ref)))
                    if kind != 'MeshRenderer':
                        unsupported.setdefault(kind, []).append(source.sid(obj))
                elif kind == 'PlayMakerFSM':
                    room['playmaker']['parsed'].append(self.fsm(source, obj, sprite_ids, textures, animations))
                    unsupported.setdefault(kind, []).append(source.sid(obj))
                elif obj.type.name == 'MonoBehaviour':
                    unsupported.setdefault(kind, []).append(source.sid(obj))
                elif kind in ('Animator', 'Animation', 'ParticleSystem', 'Terrain'):
                    unsupported.setdefault(kind, []).append(source.sid(obj))
            except Exception as error:
                failed(obj, kind, error)
                if kind == 'PlayMakerFSM':
                    room['playmaker']['parse_errors'].append({'source_id': source.sid(obj), 'error': str(error)})
        for sprite in sprite_ids:
            textures.update(self.report['sprites'][sprite]['backing_texture_ids'])
        room['sprite_ids'] = sorted(sprite_ids)
        room['texture_ids'] = sorted(textures)
        room['animation_ids'] = sorted(animations)
        room['object_types'] = dict(types)
        room['unresolved_behavior_types'] = {name: {'count': len(ids), 'source_ids': ids} for name, ids in sorted(unsupported.items())}
        room['dependency_completeness'] = 'serialized sprite/material/tk2d and direct FSM asset references only; dynamic/prefab dependencies and FSM execution unresolved'
        room['source_texture_bytes'] = sum(self.report['textures'][t]['source_image_bytes'] for t in textures)
        room['source_atlas_4bpp_estimate_bytes'] = sum(self.report['textures'][t]['source_size_4bpp_estimate_bytes'] for t in textures)
        room['cooked_ram_bytes'] = None; room['cooked_vram_bytes'] = None
        try:
            from room_graph import scan_room
            room['graph'] = scan_room(source, file, scene, scripts)
        except Exception as error:
            room['graph'] = {'scene_name': room['scene_name'], 'file': scene['file'], 'index': scene['index'],
                             'transitions': [], 'errors': [{'error': str(error)}], 'unresolved': []}
        self.loaded_files.update(source.files)
        self.loaded_files.update(str(Path(path).relative_to(source.directory)) for path in source.env.files
                                 if Path(path).is_absolute() and Path(path).is_relative_to(source.directory))
        return room


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--limit', type=int, help='Explicit diagnostic subset; never marked complete')
    parser.add_argument('--resume', action='store_true', help='Resume matching source/code fingerprint')
    args = parser.parse_args()
    start = time.monotonic(); source = Source()
    build = next(o for o in source.file('globalgamemanagers').objects.values() if o.type.name == 'BuildSettings')
    scenes = [{'index': i, 'file': f'level{i}', 'path': path} for i, path in enumerate(source.read(build)['scenes'])]
    core_files = [source.directory/'globalgamemanagers', *sorted((source.directory/'Managed').glob('*.dll'))]
    fingerprint = hashlib.sha256()
    for path in [Path(__file__), ROOT/'host/source.py', ROOT/'host/room_graph.py', *core_files]:
        fingerprint.update(hashlib.file_digest(path.open('rb'), 'sha256').digest())
    # Resume is also invalidated by changed serialized files or streamed resources.
    # Final provenance records their content hashes; this cheap startup manifest
    # avoids hashing several GiB for every interrupted diagnostic resume.
    source_stat_manifest = {}
    for path in sorted(source.directory.iterdir()):
        if path.is_file():
            stat = path.stat()
            source_stat_manifest[path.name] = [stat.st_size, stat.st_mtime_ns]
    fingerprint.update(json.dumps(source_stat_manifest, sort_keys=True).encode())
    fingerprint = fingerprint.hexdigest()
    report = {'version': VERSION, 'source_directory': str(source.directory), 'fingerprint': fingerprint,
              'source_stat_manifest': source_stat_manifest, 'build_scene_count': len(scenes), 'complete': False, 'rooms': [], 'textures': {}, 'sprites': {}, 'animations': {},
              'source_file_names': [], 'source_hashes': {}, 'limitations': [
                  'All scene SpriteRenderers are included even when disabled or under inactive parents; this is potential residency, not simultaneous visibility.',
                  'Every frame of each serialized tk2d animation library is included conservatively; runtime clip selection is not proven.',
                  'Dynamic prefab/Resources loads, PlayMaker behavior, Unity animation overrides, particles and custom shaders are not fully resolved.',
                  'Source atlas byte counts are not cooked PS1 residency. Cropping, screen scale, instances, palettes and packing still require each room cooker.',
                  'Current 116 cooked spatial regions have separate packing costs; this all-scene source inventory does not substitute them for whole-scene exact costs.',
                  'Strict PlayMaker parsing and direct asset references do not resolve prefab activation, dynamic string loads or execute FSM behavior.',
              ]}
    if args.resume and OUTPUT.exists():
        previous = json.loads(OUTPUT.read_text())
        if previous.get('fingerprint') != fingerprint:
            raise SystemExit('Resume fingerprint changed; rerun without --resume')
        # A checkpoint is trustworthy only after every consumed source file
        # has a recorded hash, including external texture streams.
        for filename in previous.get('source_file_names', []):
            path = source.directory / filename
            recorded = previous.get('source_hashes', {}).get(filename)
            if not path.is_file() or not recorded:
                raise SystemExit(f'Resume lacks complete source hash coverage: {filename}')
            actual = hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()
            if recorded['bytes'] != path.stat().st_size or recorded['sha256'] != actual:
                raise SystemExit(f'Resume source content changed: {filename}')
        report = previous
    inventory = Inventory(report)
    completed = {r['file'] for r in report['rooms']}
    selected = scenes[:args.limit] if args.limit is not None else scenes
    for scene in selected:
        if scene['file'] in completed:
            continue
        room_start = time.monotonic()
        try:
            room = inventory.scan(source, scene)
        except Exception as error:
            room = dict(scene, scene_name=Path(scene['path']).stem, texture_ids=[], sprite_ids=[], animation_ids=[],
                        unsupported=[{'type': 'scene scan', 'error': str(error)}], scan_failed=True,
                        cooked_ram_bytes=None, cooked_vram_bytes=None, cooked_coverage='unknown')
        report['rooms'].append(room)
        report['source_file_names'] = sorted(inventory.loaded_files | inventory.stream_files)
        report['scanned_scene_count'] = len(report['rooms'])
        for filename in report['source_file_names']:
            path = source.directory / filename
            if filename not in report['source_hashes'] and path.is_file():
                report['source_hashes'][filename] = {'bytes': path.stat().st_size,
                    'sha256': hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()}
        atomic_json(OUTPUT, report)
        print(f"{len(report['rooms'])}/{len(scenes)} {scene['file']} {Path(scene['path']).stem}: "
              f"{len(room['sprite_ids'])} sprites, {len(room['texture_ids'])} textures, "
              f"{len(room['unsupported'])} read errors, {time.monotonic()-room_start:.2f}s", flush=True)
        # Release SerializedFile buffers and their ObjectReaders after each room.
        # Only compact source metadata is retained across rooms.
        del source
        gc.collect()
        source = Source()
    report['rooms'].sort(key=lambda r: r['index'])
    report['complete'] = len(report['rooms']) == len(scenes) and not any(r.get('scan_failed') for r in report['rooms'])
    report['completed_with_unresolved_dependencies'] = report['complete']
    report['elapsed_seconds'] = round(time.monotonic()-start, 3)
    for filename in sorted(set(report['source_file_names']) | {str(p.relative_to(source.directory)) for p in core_files}):
        path = source.directory/filename
        if path.is_file():
            report['source_hashes'][filename] = {'bytes': path.stat().st_size, 'sha256': hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()}
    try:
        from room_graph import finalize
        graph = finalize(report['rooms'])
        atomic_json(ROOT/'.hkpsx/room-graph.json', graph)
    except Exception as error:
        report['graph_finalize_error'] = str(error)
    atomic_json(OUTPUT, report)
    print(f"Inventory saved: {len(report['rooms'])} rooms, {len(report['textures'])} source textures, "
          f"{len(report['sprites'])} sprite fragments; complete={report['complete']}", flush=True)


if __name__ == '__main__':
    main()
