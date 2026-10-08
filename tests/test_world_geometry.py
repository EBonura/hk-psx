"""Unculled source extraction, exact geometry, and per-component failures."""
import json
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'host'))
from world_geometry import GeometryHierarchy, collider_paths, collider_shape, extract_scene


def ptr(ident):
    return {'m_FileID': 0, 'm_PathID': ident}


def transform(gid, parent=0, pos=(0,0,0), scale=(1,1,1)):
    return {'m_GameObject': ptr(gid), 'm_Father': ptr(parent),
            'm_LocalPosition': dict(zip('xyz',pos)),
            'm_LocalScale': dict(zip('xyz',scale)),
            'm_LocalRotation': dict(zip('xyzw',(0,0,0,1)))}


def fixture():
    gos = {1: {'m_Name':'parent','m_Layer':8,'m_IsActive':True},
           2: {'m_Name':'child','m_Layer':8,'m_IsActive':True}}
    objects = {i: ('GameObject',g) for i,g in gos.items()}
    objects.update({11: ('Transform',transform(1, pos=(10,20,0),scale=(-2,3,1))),
                    12: ('RectTransform',transform(2,11,pos=(1,2,0)))})
    sc = SimpleNamespace(gos=gos, objects=objects, errors=[], file=SimpleNamespace(name='level99'))
    class Source:
        def ref(self, file, ref):
            return SimpleNamespace(path_id=ref['m_PathID'])
        def sid(self, obj):
            return f'assets:{obj.path_id}'
    return Source(),sc


def collider(kind='PolygonCollider2D', enabled=True, trigger=False):
    t = {'m_GameObject':ptr(2),'m_Enabled':enabled,'m_IsTrigger':trigger,
         'm_Offset':{'x':.5,'y':-.5}, 'm_Points':{'m_Paths':[[
             {'x':0,'y':0},{'x':1,'y':0},{'x':.5,'y':1}]]}}
    if kind == 'BoxCollider2D':
        t['m_Size'] = {'x':2,'y':4}
    if kind == 'EdgeCollider2D':
        t['m_Points'] = [{'x':0,'y':0},{'x':1,'y':1}]
    return t


class WorldGeometryTests(unittest.TestCase):
    def test_nested_reflection_and_rect_transform(self):
        _,sc = fixture()
        h = GeometryHierarchy(sc)
        self.assertEqual(h.point(2,1,1), [6,29,0])
        self.assertTrue(h.active(2))
        sc.gos[1]['m_IsActive'] = False
        self.assertFalse(GeometryHierarchy(sc).active(2))

    def test_cyclic_and_missing_parents_fail_explicitly(self):
        _,sc = fixture()
        sc.objects[11][1]['m_Father'] = ptr(12)
        h = GeometryHierarchy(sc)
        with self.assertRaisesRegex(ValueError,'cyclic'):
            h.point(2)
        with self.assertRaisesRegex(ValueError,'cyclic'):
            h.active(2)
        sc.objects[11][1]['m_Father'] = ptr(999)
        with self.assertRaises(KeyError):
            GeometryHierarchy(sc).point(2)

    def test_collider_offset_slope_and_closed_paths_preserved(self):
        _,sc = fixture()
        h = GeometryHierarchy(sc)
        p = collider_paths('PolygonCollider2D',collider(),h,2)[0]
        self.assertEqual(p, [[7,24.5,0],[5,24.5,0],[6,27.5,0],[7,24.5,0]])
        e = collider_paths('EdgeCollider2D',collider('EdgeCollider2D'),h,2)[0]
        self.assertEqual(e, [[7,24.5,0],[5,27.5,0]])
        box = collider_paths('BoxCollider2D',collider('BoxCollider2D'),h,2)[0]
        self.assertEqual(len(box),5)
        self.assertEqual(box[0],box[-1])

    def test_circle_boundary_preserves_exact_affine_transform(self):
        _,sc = fixture()
        tree = dict(collider(), m_Radius=2)
        shape = collider_shape('CircleCollider2D', tree, GeometryHierarchy(sc), 2)
        self.assertEqual(shape['kind'], 'affine_circle')
        self.assertEqual(shape['center'], [7,24.5,0])
        self.assertEqual(shape['axis_x'], [-4,0,0])
        self.assertEqual(shape['axis_y'], [0,6,0])
        tree['m_Radius'] = -1
        with self.assertRaisesRegex(ValueError, 'invalid circle radius'):
            collider_shape('CircleCollider2D', tree, GeometryHierarchy(sc), 2)

    def test_inactive_and_trigger_colliders_retained_without_terrain(self):
        s,sc = fixture()
        sc.objects[21] = ('PolygonCollider2D',collider())
        sc.objects[22] = ('EdgeCollider2D',collider('EdgeCollider2D',trigger=True))
        sc.objects[23] = ('BoxCollider2D',collider('BoxCollider2D',enabled=False))
        r = extract_scene(s,sc,{'scene_name':'test'})
        self.assertEqual(len(r['colliders']),3)
        self.assertEqual(len(r['terrain_edges']),3)
        self.assertEqual(r['terrain_edges'][1]['b'],[6,27.5,0])
        sc.gos[1]['m_IsActive'] = False
        r = extract_scene(s,sc,{})
        self.assertEqual(len(r['colliders']),3)
        self.assertEqual(r['terrain_edges'],[])
        self.assertTrue(all(not c['active_hierarchy'] for c in r['colliders']))

    def test_sprite_reference_shared_geometry_and_disabled_alternatives(self):
        s,sc = fixture()
        sprite = {'m_GameObject':ptr(2),'m_Enabled':False,'m_Sprite':ptr(80),
                  'm_FlipX':True,'m_FlipY':False,'m_DrawMode':0,'m_Materials':[ptr(81)]}
        sc.objects[31] = ('SpriteRenderer',sprite)
        sc.objects[32] = ('SpriteRenderer',dict(sprite,m_Enabled=True))
        with patch('world_geometry.sprite_geometry',return_value=[-1,-2,1,2]) as geom:
            r = extract_scene(s,sc,{})
        self.assertEqual(geom.call_count,1)
        self.assertEqual(r['counts']['unique_sprites'],1)
        self.assertEqual(r['counts']['sprite_quads'],2)
        self.assertFalse(r['sprites'][0]['enabled'])
        self.assertEqual(r['sprites'][0]['world_quad'],[[6,32,0],[10,32,0],[6,20,0],[10,20,0]])
        json.dumps(r,allow_nan=False)

    def test_component_failure_keeps_other_geometry_and_failed_row(self):
        s,sc = fixture()
        sc.objects[31] = ('SpriteRenderer',{'m_GameObject':ptr(2),'m_Sprite':ptr(80)})
        sc.objects[21] = ('PolygonCollider2D',collider())
        sc.errors = [{'id':'level99:40','type':'Broken','error':'original parse failure'}]
        with patch('world_geometry.sprite_geometry',side_effect=ValueError('bad sprite')):
            r = extract_scene(s,sc,{})
        self.assertEqual(len(r['errors']),2)
        self.assertEqual(len(r['sprites']),1)
        self.assertNotIn('world_quad',r['sprites'][0])
        self.assertEqual(len(r['terrain_edges']),3)
        self.assertFalse(r['geometry_extraction_complete'])
        self.assertFalse(r['gameplay_ready'])

    def test_sliced_sprite_and_exact_circle_boundary_are_distinguished(self):
        s,sc = fixture()
        sc.objects[31] = ('SpriteRenderer',{'m_GameObject':ptr(2),'m_Sprite':ptr(80),'m_DrawMode':1})
        sc.objects[22] = ('CircleCollider2D',dict(collider(),m_Radius=2))
        r = extract_scene(s,sc,{})
        self.assertEqual(len(r['unsupported']),1)
        self.assertEqual(r['counts']['sprite_quads'],0)
        self.assertNotIn('world_paths',r['colliders'][0])
        self.assertEqual(r['colliders'][0]['world_shape']['center'], [7,24.5,0])
        self.assertEqual(r['counts']['analytic_collider_shapes'], 1)
        self.assertEqual(r['terrain_shapes'], [{'source':'level99:22',
            'shape':r['colliders'][0]['world_shape']}])

    def test_camera_gate_metadata_keeps_disabled_and_exact_fields(self):
        s,sc = fixture()
        t = {'m_GameObject':ptr(2),'m_Enabled':False,'targetScene':'Other',
             'entryPoint':'left1','entryOffset':{'x':2,'y':3},'nonHazardGate':True}
        sc.objects[21] = ('BoxCollider2D',collider('BoxCollider2D'))
        sc.objects[41] = ('TransitionPoint',t)
        sc.objects[42] = ('CameraLockArea',dict(t,cameraXMin=1.5))
        r = extract_scene(s,sc,{})
        gate = r['gates'][0]
        self.assertEqual(gate['target_scene'],'Other')
        self.assertFalse(gate['enabled'])
        self.assertEqual(gate['colliders'],['level99:21'])
        self.assertTrue(gate['serialized']['nonHazardGate'])
        self.assertEqual(r['camera_locks'][0]['serialized']['cameraXMin'],1.5)

    def test_tk2d_default_frame_keeps_indices_uvs_scale_and_inactive_state(self):
        s,sc = fixture()
        sc.objects[61] = ('tk2dSprite',{'m_GameObject':ptr(2),'m_Enabled':False,
            'collection':ptr(82),'_spriteId':0,'_color':{'r':1,'g':1,'b':1,'a':.5},
            '_scale':{'x':-2,'y':.5,'z':1}})
        sc.objects[62] = ('MeshRenderer',{'m_GameObject':ptr(2),'m_Enabled':True})
        sc.objects[63] = ('MeshFilter',{'m_GameObject':ptr(2),'m_Mesh':ptr(0)})
        definition = {'name':'source frame','positions':[{'x':0,'y':0,'z':0},
            {'x':1,'y':0,'z':0},{'x':0,'y':1,'z':0}],
            'indices':[0,2,1],'uvs':[{'x':0,'y':0},{'x':.5,'y':0},{'x':0,'y':.5}],
            'material':ptr(81)}
        oldref = s.ref
        def ref(file,pointer):
            obj = oldref(file,pointer)
            obj.assets_file = sc.file
            return obj
        s.ref = ref
        s.read = lambda obj: {'spriteDefinitions':[definition]}
        r = extract_scene(s,sc,{})
        row = r['tk2d_sprites'][0]
        self.assertFalse(row['enabled'])
        self.assertEqual(row['world_vertices'],[[8,26,0],[12,26,0],[8,27.5,0]])
        self.assertEqual(row['triangles'],[[0,2,1]])
        self.assertEqual(row['uv0'],[[0,0],[.5,0],[0,.5]])
        self.assertEqual(r['counts']['tk2d_frames'],1)
        self.assertEqual(r['counts']['generated_mesh_aliases'],1)
        self.assertEqual(r['meshes'][0]['geometry_status'],
                         'generated_by_extracted_tk2d_source')
        self.assertEqual(r['meshes'][0]['generated_geometry_sources'],['level99:61'])
        self.assertEqual(r['unsupported'],[])
        sc.objects[61][1]['_spriteId'] = -1
        r = extract_scene(s,sc,{})
        self.assertEqual(r['counts']['tk2d_frames'],0)
        self.assertEqual(r['counts']['generated_mesh_aliases'],0)
        self.assertEqual(r['meshes'][0]['geometry_status'],
                         'runtime_generated_geometry_unresolved')
        self.assertEqual(len(r['errors']),1)

    def test_omitted_native_mesh_and_unowned_null_mesh_are_retained(self):
        s,sc = fixture()
        sc.file.objects = {91:SimpleNamespace(path_id=91,type=SimpleNamespace(name='Mesh'))}
        sc.objects[52] = ('MeshRenderer',{'m_GameObject':ptr(2),'m_Enabled':True})
        sc.objects[53] = ('MeshFilter',{'m_GameObject':ptr(2),'m_Mesh':ptr(0)})
        r = extract_scene(s,sc,{})
        self.assertEqual(len(r['omitted_source_objects']),1)
        self.assertEqual(len(r['unsupported']),1)
        self.assertIsNone(r['meshes'][0]['mesh'])
        self.assertFalse(r['geometry_extraction_complete'])
        self.assertEqual(r['errors'],[])

    def test_particle_parameters_renderer_and_dependencies_are_preserved(self):
        s,sc = fixture()
        sc.objects[71] = ('ParticleSystem',{'m_GameObject':ptr(2),'lengthInSec':2.5,
            'looping':1,'playOnAwake':0,'EmissionModule':{'enabled':1},
            'SubModule':{'subEmitters':[ptr(90)]}})
        sc.objects[72] = ('ParticleSystemRenderer',{'m_GameObject':ptr(2),
            'm_Enabled':True,'m_Materials':[ptr(81)],'m_RenderMode':4,
            'm_SortingLayer':3,'m_SortingOrder':7,'m_Mesh':ptr(82),
            'm_Mesh1':ptr(0),'m_Mesh2':ptr(0),'m_Mesh3':ptr(0)})
        r = extract_scene(s,sc,{})
        self.assertEqual(r['counts']['particle_systems'],1)
        self.assertEqual(r['counts']['particle_renderers'],1)
        particle=r['particle_systems'][0]
        self.assertEqual((particle['source_duration'],particle['source_looping'],
                          particle['source_play_on_awake']),(2.5,True,False))
        self.assertEqual(particle['serialized']['SubModule']['subEmitters'],[ptr(90)])
        renderer=r['particle_renderers'][0]
        self.assertEqual(renderer['materials'],['assets:81'])
        self.assertEqual(renderer['mesh_dependencies'],['assets:82'])
        self.assertEqual((renderer['render_mode'],renderer['sorting_layer'],
                          renderer['sorting_order']),(4,3,7))
        self.assertEqual(r['unsupported'],[])

    def test_runtime_text_mesh_is_bound_to_its_serialized_generator(self):
        s,sc = fixture()
        sc.objects[71] = ('TextMeshPro',{'m_GameObject':ptr(2),'m_Enabled':True,
            'm_text':'Source text','m_fontSize':12})
        sc.objects[72] = ('MeshRenderer',{'m_GameObject':ptr(2),'m_Enabled':True})
        sc.objects[73] = ('MeshFilter',{'m_GameObject':ptr(2),'m_Mesh':ptr(0)})
        r = extract_scene(s,sc,{})
        self.assertEqual(r['counts']['text_mesh_generators'],1)
        self.assertEqual(r['meshes'][0]['geometry_status'],
                         'generated_by_serialized_text_source_unshaped')
        self.assertEqual(r['meshes'][0]['generator_sources'],['level99:71'])
        self.assertEqual(r['unsupported'],[{'source':'level99:72','type':'TextMeshPro',
            'reason':'runtime glyph shaping and mesh geometry not extracted'}])

    def test_trail_curve_renderer_and_material_dependencies_are_preserved(self):
        s,sc = fixture()
        sc.objects[71] = ('TrailRenderer',{'m_GameObject':ptr(2),'m_Enabled':True,
            'm_Materials':[ptr(81)],'m_Time':.5,'m_MinVertexDistance':.125,
            'm_Autodestruct':1,'m_Emitting':0,'m_SortingLayer':2,'m_SortingOrder':4,
            'm_Parameters':{'widthCurve':{'m_Curve':[{'time':0,'value':1}]}}})
        r = extract_scene(s,sc,{})
        self.assertEqual(r['counts']['trail_renderers'],1)
        trail=r['trail_renderers'][0]
        self.assertEqual(trail['materials'],['assets:81'])
        self.assertEqual((trail['source_duration'],trail['min_vertex_distance']),(.5,.125))
        self.assertEqual((trail['autodestruct'],trail['emitting']),(True,False))
        self.assertEqual(trail['serialized']['m_Parameters']['widthCurve']['m_Curve'][0],
                         {'time':0,'value':1})
        self.assertEqual(r['unsupported'],[])

    def test_null_tk2d_collection_resolves_from_unique_animator_sprite_id(self):
        s,sc = fixture()
        sc.objects[61] = ('tk2dSprite',{'m_GameObject':ptr(2),'m_Enabled':True,
            'collection':ptr(0),'_spriteId':7,'_color':{'r':1,'g':1,'b':1,'a':1},
            '_scale':{'x':1,'y':1,'z':1}})
        sc.objects[62] = ('tk2dSpriteAnimator',{'m_GameObject':ptr(2),'m_Enabled':True,
            'library':ptr(90),'defaultClipId':0})
        definition = {'name':'effect','positions':[{'x':0,'y':0,'z':0},
            {'x':1,'y':0,'z':0},{'x':0,'y':1,'z':0}], 'indices':[0,2,1],
            'uvs':[{'x':0,'y':0},{'x':1,'y':0},{'x':0,'y':1}], 'material':ptr(81)}
        oldref=s.ref
        def ref(file,pointer):
            obj=oldref(file,pointer);obj.assets_file=sc.file;return obj
        s.ref=ref
        s.read=lambda obj: ({'clips':[{'name':'fx','frames':[
            {'spriteId':7,'spriteCollection':ptr(82)}]}]} if obj.path_id==90
            else {'spriteDefinitions':[None]*7+[definition]})
        r=extract_scene(s,sc,{})
        sprite=r['tk2d_sprites'][0]
        self.assertEqual(sprite['collection'],'assets:82')
        self.assertEqual(sprite['animation_library'],'assets:90')
        self.assertEqual(sprite['collection_resolution'],
                         'unique sprite id in animator library')
        self.assertEqual(r['counts']['tk2d_frames'],1)
        self.assertEqual(r['unsupported'],[])

    def test_legacy_text_renderer_without_mesh_filter_is_explicit_generator(self):
        s,sc = fixture()
        sc.objects[71] = ('TextMesh',{'m_GameObject':ptr(2),'m_Enabled':True,
            'm_Text':'123'})
        sc.objects[72] = ('MeshRenderer',{'m_GameObject':ptr(2),'m_Enabled':True,
            'm_Materials':[ptr(81)]})
        r=extract_scene(s,sc,{})
        self.assertEqual(r['errors'],[])
        self.assertEqual(r['counts']['legacy_text_mesh_generators'],1)
        self.assertEqual(r['meshes'][0]['generator_sources'],['level99:71'])
        self.assertEqual(r['unsupported'],[{'source':'level99:72','type':'TextMesh',
            'reason':'runtime glyph shaping and mesh geometry not extracted'}])

    def test_tilemap_mesh_preserved_when_cell_proof_fails(self):
        s,sc = fixture()
        sc.objects[51] = ('tk2dTileMap',{'m_GameObject':ptr(1),'m_Enabled':True,'renderData':ptr(2)})
        sc.objects[52] = ('MeshRenderer',{'m_GameObject':ptr(2),'m_Enabled':False,'m_Materials':[ptr(81)]})
        sc.objects[53] = ('MeshFilter',{'m_GameObject':ptr(2),'m_Mesh':ptr(82)})
        oldref = s.ref
        def ref(file, pointer):
            obj = oldref(file,pointer)
            obj.read = lambda: None
            return obj
        s.ref = ref
        reader = SimpleNamespace(process=lambda:None, m_Vertices=[(0,0,0),(1,0,0),(0,1,0)],
            m_Colors=[(1.,1.,1.,1.)]*3,m_UV0=[(0,0)]*3,get_triangles=lambda:[[(0,1,2)]])
        with patch('world_geometry.MeshHandler',return_value=reader):
            r = extract_scene(s,sc,{})
        self.assertEqual(len(r['meshes']),1)
        self.assertFalse(r['meshes'][0]['enabled'])
        self.assertEqual(r['meshes'][0]['tilemap_sources'],['level99:51'])
        self.assertEqual(r['tilemap_fills'],[])
        self.assertTrue(r['unsupported'][0]['raw_mesh_preserved'])
        reader.m_Vertices += [(1,1,0)]
        reader.m_Colors += [(1.,1.,1.,1.)]
        reader.get_triangles = lambda:[[(0,1,2),(1,2,3)]]
        with patch('world_geometry.MeshHandler',return_value=reader):
            r = extract_scene(s,sc,{})
        self.assertEqual(r['tilemap_fills'][0]['cell_count'],1)
        self.assertFalse(r['tilemap_fills'][0]['material_verified_opaque_black'])

if __name__ == '__main__':
    unittest.main()
