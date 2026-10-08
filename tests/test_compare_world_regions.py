"""Tests for canonical-region/world-import comparison."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from compare_world_regions import compare_scene, provenance_map, resolve_region

# compare_scene runs imported edges through host/cook.py::cooked_edge, which
# reads these two keys off the region row.
REGION = {'collision_bounds': (-10, -10, 10, 10), 'allow_slopes': True}


class CompareWorldRegionsTests(unittest.TestCase):
    def test_missing_base_path_resolves_through_provenance_chunk(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pack = root / 'data/regions/chunk_87.hk'
            metadata = root / 'data/regions/region-087/scene.json'
            pack.parent.mkdir(parents=True); metadata.parent.mkdir(parents=True)
            pack.write_bytes(b'pack'); metadata.write_text('{}')
            digest = hashlib.sha256(b'pack').hexdigest()
            region = {'chunk_id':87, 'scene_name':'Town',
                      'path':'data/regions/chunk_87.hk', 'bytes':4, 'sha256':digest}
            provenance = provenance_map({'regions':[dict(region)]})
            row = resolve_region(root, region, provenance)
            self.assertEqual(row['resolution'], 'provenance_chunk_id')
            self.assertEqual(row['metadata'], 'data/regions/region-087/scene.json')

    def test_provenance_hash_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); pack = root / 'pack.hk'; pack.write_bytes(b'pack')
            region = {'chunk_id':1, 'scene_name':'Town', 'path':'pack.hk',
                      'bytes':4, 'sha256':'bad'}
            with self.assertRaisesRegex(ValueError, 'hash mismatch'):
                resolve_region(root, region, {1:dict(region)})

    def test_draws_tilemap_quads_and_snapped_edges_match(self):
        # The cooked edge is stored the way host/cook.py wrote it, already
        # snapped; the imported one still carries its authored near-axis y.
        cooked = {'draws':[
            {'source':'level1:10','points':[[0,0,0],[0,1,0],[1,0,0],[1,1,0]]},
            {'source':'level1:20','points':[[2,0,0],[2,1,0],[3,0,0],[3,1,0]],
             'tilemap_rect':[0,0,1,1]}],
            'edges':[{'source':'level1:30','a':[0,2],'b':[3,2]}]}
        geometry = {'sprites':[{'source':'level1:10',
                    'world_quad':[[1,1,0],[1,0,0],[0,1,0],[0,0,0]]}],
            'tilemap_fills':[{'source':'level1:20',
                    'world_quads':[[[2,0,0],[2,1,0],[3,0,0],[3,1,0]]]}],
            'terrain_edges':[{'source':'level1:30','a':[0,2,0],'b':[3,2.003,0]}],
            'unsupported':[], 'errors':[]}
        result = compare_scene('Town', 'level1', geometry, [cooked], [REGION])
        self.assertTrue(result['comparison_passed'])
        self.assertEqual(result['cooked_unique_draw_geometry'], 2)

    def test_edges_the_cook_refuses_are_not_extras(self):
        """An imported edge the cook would never store is not a mismatch."""
        geometry = {'sprites':[], 'tilemap_fills':[], 'unsupported':[], 'errors':[],
            'terrain_edges':[
                # Wholly outside the region's collision bounds.
                {'source':'level1:31','a':[400,400,0],'b':[500,400,0]},
                # Sloped, in a region that refuses slopes.
                {'source':'level1:32','a':[0,0,0],'b':[4,9,0]}]}
        result = compare_scene('Town', 'level1', geometry, [{'draws':[],'edges':[]}],
                               [dict(REGION, allow_slopes=False)])
        self.assertEqual(result['extra_edges'], [])
        self.assertTrue(result['comparison_passed'])

    def test_additively_merged_edges_are_named_not_double_counted(self):
        """The two sides cannot name a merged object the same way, so neither
        side's edge becomes a missing/extra pair."""
        cooked = {'draws':[], 'edges':[{'source':'level7:30','a':[0,2],'b':[3,2]}]}
        geometry = {'sprites':[], 'tilemap_fills':[], 'unsupported':[], 'errors':[],
            'terrain_edges':[{'source':'level1:100030','a':[0,2,0],'b':[3,2,0]}]}
        result = compare_scene('Town', 'level1', geometry, [cooked], [REGION])
        self.assertEqual(result['missing_edges'], [])
        self.assertEqual(result['additive_merge_identity_gaps']['cooked_edges'], 1)
        self.assertEqual(result['additive_merge_identity_gaps']['imported_edges'], 1)


if __name__ == '__main__':
    unittest.main()
