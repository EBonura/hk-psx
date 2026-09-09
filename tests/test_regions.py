"""Synthetic coverage checks for the residency grid, without reading assets."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from regions import SCENES,initial_regions,fixed_regions,intersects,split_region


class RegionTests(unittest.TestCase):
    def test_opening_view_and_first_door_continuation(self):
        regions=initial_regions()
        self.assertEqual(regions[0]['camera_bounds'],[30.0,14.11,58.0,20.0])
        self.assertEqual(regions[1]['activation_bounds'],[48,11,72,27])
        self.assertTrue(intersects(regions[0]['activation_bounds'],regions[1]['activation_bounds']))

    def test_seed_grid_has_no_coverage_holes(self):
        regions=initial_regions()
        for scene in SCENES:
            x0,y0,x1,y1=scene['runtime_bounds']
            boxes=[r['activation_bounds'] for r in regions if r['scene_id']==scene['scene_id']]
            for x in range(x0,x1):
                for y in range(y0,y1):
                    self.assertTrue(any(b[0]<=x+.5<=b[2] and b[1]<=y+.5<=b[3] for b in boxes))

    def test_fixed_quality_layout_preserves_existing_boundaries(self):
        regions=fixed_regions()
        self.assertEqual(len(regions),98)
        self.assertEqual(regions[0],initial_regions()[0])
        self.assertEqual(regions[13]['activation_bounds'],[84.0,11,96,27])
        for scene in SCENES:
            boxes=[r['activation_bounds'] for r in regions if r['scene_id']==scene['scene_id']]
            x0,y0,x1,y1=scene['runtime_bounds']
            for x in range(x0,x1):
                for y in range(y0,y1):
                    self.assertTrue(any(b[0]<=x+.5<=b[2] and b[1]<=y+.5<=b[3] for b in boxes))

    def test_budget_split_preserves_parent_coverage(self):
        parent=initial_regions()[1]
        a,b=split_region(parent)
        original=parent['activation_bounds']
        self.assertTrue(intersects(a['activation_bounds'],b['activation_bounds']))
        for i in [0,1]:
            self.assertEqual(min(a['activation_bounds'][i],b['activation_bounds'][i]),original[i])
            self.assertEqual(max(a['activation_bounds'][i+2],b['activation_bounds'][i+2]),original[i+2])
        area=lambda box:(box[2]-box[0])*(box[3]-box[1])
        self.assertEqual(area(a['activation_bounds'])+area(b['activation_bounds']),area(original))


if __name__=='__main__':
    unittest.main()
