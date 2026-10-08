"""Unculled host geometry inventory; no PS1 packing or gameplay admission.

Every source instance survives, including disabled and inactive alternatives.
Mesh geometry and serialized camera/gate data are observations, not a claim that
materials, activation scripts, camera rules or collision semantics are playable.
"""
from collections import Counter
import math
from pathlib import Path

from UnityPy.helpers.MeshHelper import MeshHandler
from tilemap_fill import mesh_cells, merge_cells

SCHEMA = 'HKWORLDGEOM02'


class GeometryHierarchy:
    """Scene-local cache, including RectTransform, without retaining Scene objects."""
    def __init__(self, scene):
        self.gos = scene.gos
        self.transforms = {i: t for i, (k, t) in scene.objects.items()
                           if k in ('Transform', 'RectTransform')}
        self.by_go = {t['m_GameObject']['m_PathID']: i for i, t in self.transforms.items()}
        self.matrices = {}
        self.active_cache = {}

    def world(self, ident, visiting=None):
        if ident in self.matrices:
            return self.matrices[ident]
        visiting = set() if visiting is None else visiting
        if ident in visiting:
            raise ValueError('cyclic transform hierarchy')
        visiting.add(ident)
        t = self.transforms[ident]
        q = t['m_LocalRotation']
        x, y, z, w = [q[k] for k in 'xyzw']
        p, scale = t['m_LocalPosition'], t['m_LocalScale']
        matrix = [[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w), p['x']],
                  [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w), p['y']],
                  [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y), p['z']],
                  [0, 0, 0, 1]]
        for row in range(3):
            for col, key in enumerate('xyz'):
                matrix[row][col] *= scale[key]
        parent = t['m_Father']
        if parent.get('m_FileID', 0):
            raise ValueError('external transform parent')
        if parent['m_PathID']:
            a = self.world(parent['m_PathID'], visiting)
            matrix = [[sum(a[i][k]*matrix[k][j] for k in range(4))
                       for j in range(4)] for i in range(4)]
        if not all(math.isfinite(v) for row in matrix for v in row):
            raise ValueError('nonfinite transform')
        visiting.remove(ident)
        self.matrices[ident] = matrix
        return matrix

    def point(self, gid, x=0, y=0, z=0):
        matrix = self.world(self.by_go[gid])
        point = [sum(matrix[i][j]*v for j, v in enumerate((x, y, z, 1)))
                 for i in range(3)]
        if not all(math.isfinite(v) for v in point):
            raise ValueError('nonfinite point')
        return point

    def ancestors(self, gid):
        seen = set()
        while True:
            if gid in seen:
                raise ValueError('cyclic transform hierarchy')
            seen.add(gid)
            yield gid
            t = self.transforms[self.by_go[gid]]
            if t['m_Father'].get('m_FileID', 0):
                raise ValueError('external transform parent')
            father = t['m_Father']['m_PathID']
            if not father:
                break
            gid = self.transforms[father]['m_GameObject']['m_PathID']

    def active(self, gid):
        if gid not in self.active_cache:
            self.active_cache[gid] = all(bool(self.gos[g]['m_IsActive'])
                                         for g in self.ancestors(gid))
        return self.active_cache[gid]


def collider_paths(kind, tree, hierarchy, gid):
    """Preserve source coordinates and slopes; close only closed source shapes."""
    offset = tree['m_Offset']
    if kind == 'BoxCollider2D':
        x, y = tree['m_Size']['x']/2, tree['m_Size']['y']/2
        raw = [[{'x': a, 'y': b} for a, b in [(-x,-y),(x,-y),(x,y),(-x,y)]]]
    elif kind == 'EdgeCollider2D':
        raw = [tree['m_Points']]
    elif kind == 'PolygonCollider2D':
        raw = tree['m_Points']['m_Paths']
    else:
        raise ValueError('boundary extraction not implemented for '+kind)
    paths = [[hierarchy.point(gid, p['x']+offset['x'], p['y']+offset['y'])
              for p in path] for path in raw]
    if kind != 'EdgeCollider2D':
        paths = [path+path[:1] if path and path[-1] != path[0] else path
                 for path in paths]
    return paths


def collider_shape(kind, tree, hierarchy, gid):
    """Return an exact analytic world-space boundary for non-path colliders."""
    if kind != 'CircleCollider2D':
        raise ValueError('analytic boundary extraction not implemented for '+kind)
    offset = tree['m_Offset']
    radius = tree['m_Radius']
    if type(radius) not in (int, float) or not math.isfinite(radius) or radius < 0:
        raise ValueError('invalid circle radius')
    center = hierarchy.point(gid, offset['x'], offset['y'])
    point_x = hierarchy.point(gid, offset['x'] + radius, offset['y'])
    point_y = hierarchy.point(gid, offset['x'], offset['y'] + radius)
    return {
        'kind': 'affine_circle',
        'center': center,
        'axis_x': [value-center[index] for index, value in enumerate(point_x)],
        'axis_y': [value-center[index] for index, value in enumerate(point_y)],
        'parameterization': 'center + axis_x*cos(theta) + axis_y*sin(theta)',
    }


def sprite_geometry(obj):
    sprite = obj.read()
    mesh = MeshHandler(sprite.m_RD, obj.version)
    mesh.process()
    vertices = [list(v) for v in mesh.m_Vertices]
    if not vertices or any(len(v) != 3 or not all(math.isfinite(x) for x in v)
                           for v in vertices):
        raise ValueError('missing or invalid sprite mesh vertices')
    xs, ys = [v[0] for v in vertices], [v[1] for v in vertices]
    return [min(xs), min(ys), max(xs), max(ys)]


def extract_scene(source, scene, scene_info):
    """Return JSON data with sprites/colliders/terrain_edges/meshes/cameras/gates.

    errors are per-component read/transform/geometry failures. unsupported records
    are known features not interpreted by this exporter. Counts and completion
    concern host extraction only. No guest readiness or material equivalence.
    """
    filename = Path(scene.file.name).name
    result = dict(schema=SCHEMA, scene=dict(scene_info), sprites=[], colliders=[],
                  terrain_edges=[], terrain_shapes=[], meshes=[], tk2d_sprites=[], tilemap_fills=[], cameras=[],
                  camera_locks=[], gates=[], tilemaps=[], particle_systems=[],
                  particle_renderers=[], trail_renderers=[], errors=list(scene.errors),
                  unsupported=[], omitted_source_objects=[], gameplay_ready=False)
    # Scene intentionally skips costly native mesh/particle type-tree reads.
    # Keep their provenance visible even when they are absent from objects.
    for obj in scene.file.objects.values() if hasattr(scene.file, 'objects') else []:
        if obj.path_id not in scene.objects and obj.type.name == 'Mesh':
            item = {'source': source.sid(obj), 'type': obj.type.name,
                    'reason': 'native type omitted from Scene type-tree inventory'}
            result['omitted_source_objects'].append(item)
    hierarchy = GeometryHierarchy(scene)
    components = {}
    for ident, (kind, tree) in scene.objects.items():
        gid = tree.get('m_GameObject', {}).get('m_PathID')
        components.setdefault(gid, []).append((ident, kind, tree))

    def sid(ident):
        return f'{filename}:{ident}'

    def reference(ref, file=None):
        if not ref.get('m_PathID'):
            return None
        file = scene.file if file is None else file
        try:
            return source.sid(source.ref(file, ref))
        except (AttributeError, FileNotFoundError):
            # Unity builtin resources may have no serialized file on disk. Their
            # pointer identity remains useful even though geometry is unresolved.
            if not ref.get('m_FileID', 0):
                raise
            external = Path(file.externals[ref['m_FileID']-1].path).name
            return f'{external}:{ref["m_PathID"]}'

    def base(ident, kind, tree):
        gid = tree['m_GameObject']['m_PathID']
        go = scene.gos[gid]
        return {'source': sid(ident), 'type': kind, 'game_object': sid(gid),
                'name': go['m_Name'], 'layer': go['m_Layer'],
                'enabled': bool(tree.get('m_Enabled', True)),
                'active_self': bool(go['m_IsActive']),
                'active_hierarchy': hierarchy.active(gid),
                'position': hierarchy.point(gid)}

    def error(ident, kind, exc):
        result['errors'].append({'source': sid(ident), 'type': kind,
                                 'error': f'{type(exc).__name__}: {exc}'})

    tilemap_roots = {}
    for ident, (kind, tree) in scene.objects.items():
        if kind != 'tk2dTileMap':
            continue
        try:
            row = base(ident, kind, tree)
            row['render_data'] = reference(tree['renderData'])
            row['width'] = tree.get('width')
            row['height'] = tree.get('height')
            result['tilemaps'].append(row)
            ref = tree['renderData']
            if ref['m_PathID']:
                if ref.get('m_FileID', 0):
                    raise ValueError('external tilemap renderData hierarchy')
                tilemap_roots.setdefault(ref['m_PathID'], []).append(row['source'])
        except Exception as exc:
            error(ident, kind, exc)

    collection_cache = {}
    sprite_cache = {}
    mesh_cache = {}
    for ident, (kind, tree) in scene.objects.items():
        relevant = (kind in ('SpriteRenderer', 'tk2dSprite') or kind.endswith('Collider2D') or
                    kind in ('MeshRenderer', 'ParticleSystem', 'ParticleSystemRenderer',
                             'TrailRenderer', 'Camera', 'CameraLockArea', 'TransitionPoint'))
        if not relevant:
            if kind in ('SkinnedMeshRenderer', 'LineRenderer',
                        'tk2dSlicedSprite', 'tk2dTiledSprite'):
                result['unsupported'].append({'source': sid(ident), 'type': kind,
                                              'reason': 'renderer geometry not extracted'})
            continue
        try:
            row = base(ident, kind, tree)
            gid = tree['m_GameObject']['m_PathID']
            if kind == 'SpriteRenderer':
                row.update(sprite=reference(tree['m_Sprite']),
                           materials=[reference(r) for r in tree.get('m_Materials', [])],
                           color=tree.get('m_Color'), sorting_layer=tree.get('m_SortingLayer'),
                           sorting_order=tree.get('m_SortingOrder'),
                           draw_mode=tree.get('m_DrawMode', 0),
                           mask_interaction=tree.get('m_MaskInteraction', 0),
                           flip_x=bool(tree.get('m_FlipX')), flip_y=bool(tree.get('m_FlipY')))
                result['sprites'].append(row)
                if row['sprite'] is None:
                    continue
                if row['draw_mode'] != 0:
                    result['unsupported'].append({'source': sid(ident), 'type': kind,
                                                  'reason': 'sliced/tiled sprite geometry'})
                    continue
                if row['sprite'] not in sprite_cache:
                    sprite_cache[row['sprite']] = sprite_geometry(source.ref(scene.file, tree['m_Sprite']))
                x0, y0, x1, y1 = sprite_cache[row['sprite']]
                if row['flip_x']:
                    x0, x1 = -x0, -x1
                if row['flip_y']:
                    y0, y1 = -y0, -y1
                row['world_quad'] = [hierarchy.point(gid, x, y) for x,y in
                                     [(x0,y1),(x1,y1),(x0,y0),(x1,y0)]]
                row['quad_semantics'] = 'tight sprite mesh bounds; original alpha/mask/material required'
            elif kind == 'tk2dSprite':
                collection_ref = tree['collection']
                collection_file = scene.file
                index = tree['_spriteId']
                if not collection_ref.get('m_PathID'):
                    animators = [t for _, k, t in components[gid]
                                 if k == 'tk2dSpriteAnimator' and
                                 t.get('library', {}).get('m_PathID')]
                    if len(animators) == 1:
                        animation_obj = source.ref(scene.file, animators[0]['library'])
                        animation = source.read(animation_obj)
                        candidates = {(frame['spriteCollection']['m_FileID'],
                                       frame['spriteCollection']['m_PathID'])
                                      for clip in animation.get('clips', [])
                                      for frame in clip.get('frames', [])
                                      if frame.get('spriteId') == index and
                                      frame.get('spriteCollection', {}).get('m_PathID')}
                        if len(candidates) == 1:
                            file_id, path_id = candidates.pop()
                            collection_ref = {'m_FileID': file_id, 'm_PathID': path_id}
                            collection_file = animation_obj.assets_file
                            row['collection_resolution'] = 'unique sprite id in animator library'
                            row['animation_library'] = source.sid(animation_obj)
                row.update(collection=reference(collection_ref, collection_file), frame=index,
                           color=tree['_color'], scale=tree['_scale'])
                result['tk2d_sprites'].append(row)
                if row['collection'] is None:
                    result['unsupported'].append({'source': sid(ident), 'type': kind,
                                                  'reason': 'null serialized sprite collection'})
                    continue
                if row['collection'] not in collection_cache:
                    obj = source.ref(collection_file, collection_ref)
                    collection_cache[row['collection']] = (obj.assets_file, source.read(obj))
                collection_file, collection = collection_cache[row['collection']]
                if type(index) is not int or not 0 <= index < len(collection['spriteDefinitions']):
                    raise ValueError('tk2d sprite frame outside source collection')
                definition = collection['spriteDefinitions'][index]
                positions, indices, uvs = definition['positions'], definition['indices'], definition['uvs']
                if (not positions or len(uvs) != len(positions) or len(indices) % 3 or
                        any(type(i) is not int or not 0 <= i < len(positions) for i in indices)):
                    raise ValueError('invalid tk2d mesh positions, UVs or triangle indices')
                scale = row['scale']
                row.update(frame_name=definition['name'],
                    material=reference(definition['material'], collection_file),
                    world_vertices=[hierarchy.point(gid, *(p[k]*scale[k] for k in 'xyz'))
                                    for p in positions],
                    triangles=[indices[i:i+3] for i in range(0,len(indices),3)],
                    uv0=[[uv['x'],uv['y']] for uv in uvs],
                    frame_semantics='serialized default frame; runtime animation and activation not evaluated')
            elif kind.endswith('Collider2D'):
                row.update(trigger=bool(tree.get('m_IsTrigger')), offset=tree.get('m_Offset'),
                           material=reference(tree.get('m_PhysicsMaterial2D', {})),
                           used_by_composite=tree.get('m_UsedByComposite'),
                           composite_operation=tree.get('m_CompositeOperation'),
                           edge_radius=tree.get('m_EdgeRadius'), size=tree.get('m_Size'),
                           radius=tree.get('m_Radius'))
                row['terrain_eligible'] = (row['layer'] == 8 and row['enabled'] and
                                           row['active_hierarchy'] and not row['trigger'])
                result['colliders'].append(row)
                if kind == 'CircleCollider2D':
                    row['world_shape'] = collider_shape(kind, tree, hierarchy, gid)
                    if row['terrain_eligible']:
                        result['terrain_shapes'].append({'source': sid(ident),
                                                         'shape': row['world_shape']})
                    continue
                if kind not in ('BoxCollider2D', 'EdgeCollider2D', 'PolygonCollider2D'):
                    result['unsupported'].append({'source': sid(ident), 'type': kind,
                                                  'reason': 'collider boundary not extracted'})
                    continue
                row['world_paths'] = collider_paths(kind, tree, hierarchy, gid)
                if row['terrain_eligible']:
                    for path in row['world_paths']:
                        for a, b in zip(path, path[1:]):
                            result['terrain_edges'].append({'source': sid(ident), 'a': a, 'b': b})
            elif kind == 'TrailRenderer':
                row.update(materials=[reference(r) for r in tree.get('m_Materials', [])],
                           source_duration=tree.get('m_Time'),
                           min_vertex_distance=tree.get('m_MinVertexDistance'),
                           autodestruct=bool(tree.get('m_Autodestruct')),
                           emitting=bool(tree.get('m_Emitting')),
                           sorting_layer=tree.get('m_SortingLayer'),
                           sorting_order=tree.get('m_SortingOrder'))
                row['serialized'] = {k:v for k,v in tree.items()
                                     if k not in ('m_GameObject','m_Materials')}
                result['trail_renderers'].append(row)
            elif kind == 'ParticleSystem':
                row['serialized'] = {k:v for k,v in tree.items() if k != 'm_GameObject'}
                row['source_duration'] = tree.get('lengthInSec')
                row['source_looping'] = bool(tree.get('looping'))
                row['source_play_on_awake'] = bool(tree.get('playOnAwake'))
                result['particle_systems'].append(row)
            elif kind == 'ParticleSystemRenderer':
                row.update(materials=[reference(r) for r in tree.get('m_Materials', [])],
                           render_mode=tree.get('m_RenderMode'),
                           sorting_layer=tree.get('m_SortingLayer'),
                           sorting_order=tree.get('m_SortingOrder'),
                           mesh_dependencies=[reference(tree.get(name, {})) for name in
                               ('m_Mesh','m_Mesh1','m_Mesh2','m_Mesh3')
                               if tree.get(name, {}).get('m_PathID')])
                row['serialized'] = {k:v for k,v in tree.items()
                                     if k not in ('m_GameObject','m_Materials')}
                result['particle_renderers'].append(row)
            elif kind == 'MeshRenderer':
                filters = [t for _, k, t in components[gid] if k == 'MeshFilter']
                result['meshes'].append(row)
                if len(filters) != 1:
                    legacy_text = [sid(i) for i, k, _ in components[gid] if k == 'TextMesh']
                    if not filters and legacy_text:
                        row.update(mesh=None,
                                   materials=[reference(r) for r in tree.get('m_Materials', [])],
                                   sorting_layer=tree.get('m_SortingLayer'),
                                   sorting_order=tree.get('m_SortingOrder'),
                                   geometry_status='generated_by_serialized_legacy_text_source_unshaped',
                                   generator_sources=legacy_text)
                        result['unsupported'].append({'source': sid(ident), 'type': 'TextMesh',
                            'reason': 'runtime glyph shaping and mesh geometry not extracted'})
                        continue
                    raise ValueError('MeshRenderer must have exactly one MeshFilter')
                row.update(mesh=reference(filters[0]['m_Mesh']),
                           materials=[reference(r) for r in tree.get('m_Materials', [])],
                           sorting_layer=tree.get('m_SortingLayer'), sorting_order=tree.get('m_SortingOrder'))
                if row['mesh'] is None:
                    row['geometry_status'] = 'runtime_generated_mesh_pending_owner_resolution'
                    continue
                try:
                    obj = source.ref(scene.file, filters[0]['m_Mesh'])
                except (AttributeError, FileNotFoundError) as exc:
                    row['geometry_status'] = 'unresolved source reference'
                    raise ValueError('mesh source unavailable: '+row['mesh']) from exc
                mesh_sid = source.sid(obj)
                if mesh_sid not in mesh_cache:
                    reader = MeshHandler(obj.read())
                    reader.process()
                    mesh_cache[mesh_sid] = reader
                reader = mesh_cache[mesh_sid]
                vertices = [list(v) for v in reader.m_Vertices]
                triangles = [[list(t) for t in group] for group in reader.get_triangles()]
                row.update(mesh=mesh_sid, materials=[reference(r) for r in tree.get('m_Materials', [])],
                           sorting_layer=tree.get('m_SortingLayer'), sorting_order=tree.get('m_SortingOrder'),
                           world_vertices=[hierarchy.point(gid, *v) for v in vertices],
                           triangle_groups=triangles,
                           uv0=[list(v) for v in (reader.m_UV0 or [])],
                           colors=[list(v) for v in (reader.m_Colors or [])])
                owners = [owner for ancestor in hierarchy.ancestors(gid)
                          for owner in tilemap_roots.get(ancestor, [])]
                row['tilemap_sources'] = owners
                if owners:
                    try:
                        if len(triangles) != 1:
                            raise ValueError('multiple tilemap submeshes')
                        cells = mesh_cells(vertices, triangles[0], reader.m_Colors or [])
                        rectangles = merge_cells(cells)
                        result['tilemap_fills'].append({'source': sid(ident), 'mesh': mesh_sid,
                            'tilemap_sources': owners, 'cell_count': len(cells),
                            'rectangles': rectangles, 'world_quads': [[hierarchy.point(gid,x,y)
                                for x,y in [(x0,y1),(x1,y1),(x0,y0),(x1,y0)]]
                                for x0,y0,x1,y1 in rectangles],
                            'material_verified_opaque_black': False})
                    except Exception as exc:
                        result['unsupported'].append({'source': sid(ident), 'type': 'tilemap cell merge',
                                                      'reason': str(exc), 'raw_mesh_preserved': True})
            else:
                row['serialized'] = {k:v for k,v in tree.items()
                                     if k not in ('m_GameObject','m_Script','m_EditorClassIdentifier','m_Name')}
                row['colliders'] = [sid(i) for i,k,t in components[gid] if k.endswith('Collider2D')]
                if kind == 'TransitionPoint':
                    row.update(target_scene=tree.get('targetScene'), entry_point=tree.get('entryPoint'),
                               entry_offset=tree.get('entryOffset'))
                    result['gates'].append(row)
                elif kind == 'CameraLockArea':
                    result['camera_locks'].append(row)
                else:
                    result['cameras'].append(row)
        except Exception as exc:
            error(ident, kind, exc)

    # tk2d owns a runtime-generated MeshFilter as an implementation detail. Keep
    # the renderer metadata, but point it at the already extracted canonical
    # sprite definition instead of treating the duplicate mesh as missing.
    tk2d_geometry = {}
    for sprite in result['tk2d_sprites']:
        if 'world_vertices' in sprite:
            tk2d_geometry.setdefault(sprite['game_object'], []).append(sprite['source'])
    for mesh in result['meshes']:
        if mesh.get('geometry_status') != 'runtime_generated_mesh_pending_owner_resolution':
            continue
        owners = tk2d_geometry.get(mesh['game_object'], [])
        if owners:
            mesh['geometry_status'] = 'generated_by_extracted_tk2d_source'
            mesh['generated_geometry_sources'] = owners
        else:
            gid = int(mesh['game_object'].rsplit(':', 1)[-1])
            text_sources = [sid(ident) for ancestor in hierarchy.ancestors(gid)
                            for ident, kind, _ in components.get(ancestor, [])
                            if kind == 'TextMeshPro']
            if text_sources:
                mesh['geometry_status'] = 'generated_by_serialized_text_source_unshaped'
                mesh['generator_sources'] = text_sources
                result['unsupported'].append({'source': mesh['source'], 'type': 'TextMeshPro',
                    'reason': 'runtime glyph shaping and mesh geometry not extracted'})
            else:
                mesh['geometry_status'] = 'runtime_generated_geometry_unresolved'
                result['unsupported'].append({'source': mesh['source'], 'type': mesh['type'],
                    'reason': 'null serialized mesh; runtime-generated geometry not evaluated'})
    result['counts'] = {key: len(result[key]) for key in
                        ('sprites','tk2d_sprites','colliders','terrain_edges','terrain_shapes','meshes','tilemap_fills',
                         'cameras','camera_locks','gates','tilemaps','particle_systems',
                         'particle_renderers','trail_renderers','errors','unsupported',
                         'omitted_source_objects')}
    result['counts']['tk2d_frames'] = sum('world_vertices' in r for r in result['tk2d_sprites'])
    result['counts']['world_meshes'] = sum('world_vertices' in r for r in result['meshes'])
    result['counts']['sprite_quads'] = sum('world_quad' in r for r in result['sprites'])
    result['counts']['analytic_collider_shapes'] = sum('world_shape' in r
                                                        for r in result['colliders'])
    result['counts']['generated_mesh_aliases'] = sum(
        r.get('geometry_status') == 'generated_by_extracted_tk2d_source'
        for r in result['meshes'])
    result['counts']['text_mesh_generators'] = sum(
        r.get('geometry_status') == 'generated_by_serialized_text_source_unshaped'
        for r in result['meshes'])
    result['counts']['legacy_text_mesh_generators'] = sum(
        r.get('geometry_status') == 'generated_by_serialized_legacy_text_source_unshaped'
        for r in result['meshes'])
    result['counts']['unique_sprites'] = len({r['sprite'] for r in result['sprites'] if r['sprite']})
    result['component_counts'] = dict(sorted(Counter(k for k,t in scene.objects.values()).items()))
    result['geometry_extraction_complete'] = not result['errors'] and not result['unsupported']
    result['limitations'] = ['Host source geometry only; no guest memory/cooking admission.',
        'Serialized initial activation is retained; scripts, animation and dynamic spawns are not evaluated.',
        'Source mesh materials, masks, lighting and collider physics semantics require separate system support.',
        'RectTransform positions use serialized local transforms; runtime canvas layout is not evaluated.']
    return result
