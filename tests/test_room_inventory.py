"""Original metadata fixtures; no retail source data is embedded."""
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from room_inventory import Inventory, pointer_refs


class FakeSource:
    def __init__(self, records):
        self.records = records
    def sid(self, obj):
        return f'fixture:{obj.path_id}'
    def read(self, obj):
        return self.records[obj.path_id]
    def ref(self, file, ref):
        return self.obj(ref['m_PathID'])
    def obj(self, index):
        return SimpleNamespace(path_id=index, assets_file=None, byte_size=144,
                               type=SimpleNamespace(name='Texture2D' if index == 1 else 'Fixture'))


def ref(index):
    return {'m_FileID': 0, 'm_PathID': index}


def fixture():
    records = {
        1: {'m_Name': 'atlas', 'm_Width': 128, 'm_Height': 64, 'm_MipCount': 1,
            'm_TextureFormat': 10, 'm_CompleteImageSize': 4096,
            'm_StreamData': {'path': 'fixture.resS', 'size': 4096, 'offset': 32}},
        2: {'m_Name': 'fragment', 'm_Rect': {'width': 5, 'height': 3}, 'm_PixelsToUnits': 64,
            'm_RD': {}, 'm_SpriteAtlas': ref(3), 'm_RenderDataKey': ['guid', 42]},
        3: {'m_RenderDataMap': [(['guid', 42], {'texture': ref(1), 'alphaTexture': ref(0),
             'textureRect': {'x': 2, 'y': 7, 'width': 5, 'height': 3}})]},
        4: {'m_SavedProperties': {'m_TexEnvs': [('_MainTex', {'m_Texture': ref(1)})]}},
        5: {'spriteDefinitions': [{'name': 'pose', 'material': ref(4),
             'positions': [{'x': 0, 'y': 0}, {'x': 2, 'y': 3}], 'texelSize': {'x': .5, 'y': .5}}]},
        6: {'clips': [{'name': 'Idle', 'fps': 12, 'frames': [
             {'spriteCollection': ref(5), 'spriteId': 0}, {'spriteCollection': ref(5), 'spriteId': 0}]}]},
    }
    report = {'textures': {}, 'sprites': {}, 'animations': {}}
    return FakeSource(records), Inventory(report), report


class InventoryTests(unittest.TestCase):
    def test_atlas_backing_is_distinct_from_fragment_cost(self):
        source, inventory, report = fixture()
        sprite = inventory.sprite(source, source.obj(2))
        self.assertEqual(sprite, 'fixture:2')
        self.assertEqual(report['sprites'][sprite]['backing_texture_ids'], ['fixture:1'])
        self.assertEqual(report['sprites'][sprite]['source_dimensions'], [5, 3])
        self.assertEqual(report['textures']['fixture:1']['source_size_4bpp_estimate_bytes'], 4096)
        self.assertIsNone(report['textures']['fixture:1']['cooked_vram_bytes'])
        self.assertIsNone(report['sprites'][sprite]['cooked_vram_bytes'])
        self.assertEqual(inventory.stream_files, {'fixture.resS'})

    def test_animation_keeps_frame_references_but_deduplicates_dependencies(self):
        source, inventory, report = fixture()
        animation, sprites = inventory.animation(source, source.obj(6))
        self.assertEqual(sprites, ['fixture:5:sprite:0'])
        self.assertEqual(report['animations'][animation]['clips'][0]['frame_sprite_ids'], sprites * 2)
        self.assertEqual(report['sprites'][sprites[0]]['source_dimensions'], [4, 6])
        self.assertEqual(len(report['textures']), 1)

    def test_fsm_literal_assets_retain_valid_refs_and_mark_unresolved(self):
        source, inventory, report = fixture()
        source.records[7] = {'fsm': {'name':'Test', 'states': [
            {'actionData': {'actionNames':['SetTexture'], 'objects':[ref(1),ref(1),ref(8),ref(999)]}}]}}
        source.typename = lambda obj: obj.type.name
        original_ref = source.ref
        def resolve(file, value):
            if value['m_PathID'] == 999:raise ValueError('missing fixture reference')
            return original_ref(file,value)
        source.ref = resolve
        sprites,textures,animations=set(),set(),set()
        audit=inventory.fsm(source,source.obj(7),sprites,textures,animations)
        self.assertEqual(textures,{'fixture:1'})
        self.assertEqual(audit['direct_asset_ids'],['fixture:1'])
        self.assertEqual(audit['unresolved_object_refs'],[{'source_id':'fixture:8','type':'Fixture'}])
        self.assertEqual(len(audit['errors']),1)
        self.assertEqual(audit['actions'],{'SetTexture':1})
        self.assertIsNone(report['textures']['fixture:1']['cooked_vram_bytes'])

    def test_pointer_walk_deduplicates_and_ignores_nulls(self):
        self.assertEqual(pointer_refs({'a':[ref(1),ref(0)],'b':(ref(1),)}),[ref(1)])

    def test_invalid_animation_index_is_not_silently_dropped(self):
        source, inventory, _ = fixture()
        source.records[6]['clips'][0]['frames'][0]['spriteId'] = -1
        animation, sprites = inventory.animation(source, source.obj(6))
        self.assertEqual(sprites, ['fixture:5:sprite:0'])
        record = inventory.report['animations'][animation]
        self.assertEqual(record['clips'][0]['frame_sprite_ids'], [None, 'fixture:5:sprite:0'])
        self.assertEqual(len(record['unresolved_frames']), 1)
        self.assertEqual(record['unresolved_frames'][0]['sprite_index'], -1)
        self.assertEqual(record['unresolved_frames'][0]['frame_index'], 0)


if __name__ == '__main__':
    unittest.main()
