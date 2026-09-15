"""Host import integrity and coverage semantics using synthetic data only."""
import gzip
import json
from pathlib import Path
import sys
import tempfile
import unittest
from types import SimpleNamespace
sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'host'))
from world_import import (atomic_json, catalog, sha, source_json, summarize,
                          valid_checkpoint, verify_inputs, write_gzip)


class ImportTests(unittest.TestCase):
    def test_catalog_retains_non_gameplay_scenes_and_windows_paths(self):
        build = SimpleNamespace(type=SimpleNamespace(name='BuildSettings'))
        source = SimpleNamespace(file=lambda _: SimpleNamespace(objects={1:build}),
            read=lambda _: {'scenes':['Assets\\Scenes\\Menu.unity', 'Assets/Scenes/Room.unity']})
        self.assertEqual(catalog(source), [
            {'index':0,'file':'level0','path':'Assets\\Scenes\\Menu.unity','scene_name':'Menu'},
            {'index':1,'file':'level1','path':'Assets/Scenes/Room.unity','scene_name':'Room'}])

    def test_partial_and_subset_never_claim_resolved_world(self):
        rows = [{'index':0, 'status':'imported', 'component_types':{'Enemy':2}, 'counts':{'sprites':3}},
                {'index':1, 'status':'partial', 'component_types':{'Enemy':1}}]
        summary = summarize(rows, 3)
        self.assertFalse(summary['all_scenes_processed'])
        self.assertFalse(summary['all_geometry_resolved'])
        self.assertEqual(summary['systems']['Enemy']['instances'], 3)
        self.assertEqual(summary['systems']['Enemy']['gameplay_support'], 'not_evaluated')
        summary = summarize(rows, 2)
        self.assertTrue(summary['all_scenes_processed'])
        self.assertFalse(summary['all_geometry_resolved'])
        self.assertEqual(summary['ps1_packing'], 'not_run')
        self.assertEqual(summary['gameplay_validation'], 'not_run')

    def test_checkpoints_reject_modified_output_and_changed_code(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            item = write_gzip(folder/'geometry.json.gz', {'value':1})
            row = {'fingerprint':'abc', 'status':'partial','outputs':{'geometry':item}}
            path = folder/'result.json'
            atomic_json(path,row)
            self.assertEqual(valid_checkpoint(path,'abc'),row)
            self.assertIsNone(valid_checkpoint(path,'def'))
            (folder/item['path']).write_bytes(b'broken')
            self.assertIsNone(valid_checkpoint(path,'abc'))

    def test_failed_checkpoint_is_retried(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'result.json'
            atomic_json(path, {'fingerprint':'abc','status':'failed','outputs':{}})
            self.assertIsNone(valid_checkpoint(path,'abc'))

    def test_input_content_hash_detects_same_length_change(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); path = root/'level0'; path.write_bytes(b'abc')
            recorded = {'level0':{'bytes':3,'sha256':sha(path)}}
            verify_inputs(root, recorded)
            path.write_bytes(b'def')
            with self.assertRaisesRegex(ValueError, 'fingerprint changed'):
                verify_inputs(root, recorded)

    def test_serialized_binary_and_nonfinite_values_are_lossless_tagged_json(self):
        value = source_json({'bytes':b'\x00\xff', 'range':[float('inf'), float('-inf'), 2.0]})
        self.assertEqual(value['bytes'], {'$source_bytes_base64':'AP8='})
        self.assertEqual(value['range'], [{'$source_float':'inf'}, {'$source_float':'-inf'}, 2.0])
        json.dumps(value, allow_nan=False)

    def test_gzip_output_is_reproducible(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'geometry.json.gz'
            first = write_gzip(path, {'points':[[1,2]],'binary':b'abc'})
            second = write_gzip(path, {'points':[[1,2]],'binary':b'abc'})
            self.assertEqual(first, second)
            self.assertEqual(json.loads(gzip.decompress(path.read_bytes()))['points'], [[1,2]])


if __name__ == '__main__':
    unittest.main()
