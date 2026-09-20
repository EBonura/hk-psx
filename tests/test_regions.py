"""Synthetic coverage checks for the residency grid, without reading assets."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from regions import SCENES,initial_regions,fixed_regions,intersects,split_region,region_spec


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
        from quality import GRID_SCENE_LAYOUTS
        self.assertEqual(len(regions),106+sum(len(b) for b in GRID_SCENE_LAYOUTS.values()))
        self.assertEqual(regions[0],initial_regions()[0])
        self.assertEqual(regions[13]['activation_bounds'],[84.0,11,96,27])
        for scene in SCENES:
            boxes=[r['activation_bounds'] for r in regions if r['scene_id']==scene['scene_id']]
            x0,y0,x1,y1=scene['runtime_bounds']
            for x in range(x0,x1):
                for y in range(y0,y1):
                    self.assertTrue(any(b[0]<=x+.5<=b[2] and b[1]<=y+.5<=b[3] for b in boxes))

    def test_expansion_preserves_ids_and_tutorial_and_opens_town_camera(self):
        from quality import REGION_LAYOUT
        regions=fixed_regions()
        self.assertEqual(len(REGION_LAYOUT),98)
        town=SCENES[1]
        for i,(scene,bounds) in enumerate(REGION_LAYOUT):
            expected=initial_regions()[0] if i==0 else region_spec(SCENES[0] if scene==0 else town,bounds)
            self.assertEqual(regions[i],expected)
        self.assertEqual(regions[98]['activation_bounds'],[48,-5,96,40])
        self.assertEqual(regions[100]['activation_bounds'],[168,-5,216,40])
        self.assertEqual(regions[105]['activation_bounds'],[168,40,216,76])
        self.assertTrue(all(r['scene_id']==1 for r in regions[98:106]))
        # Crossroads_01 follows as scene 2 with the measured 24x16 stepping; IDs 107..126.
        self.assertTrue(all(r['scene_id']==2 for r in regions[106:126]))
        self.assertTrue(all(r['scene_id']>=2 for r in regions[106:]))
        self.assertEqual(len(regions[106:126]),20)
        self.assertEqual(regions[106]['activation_bounds'],[-2,-5,22,11])
        self.assertEqual(regions[125]['activation_bounds'],[94,43,102,49])
        self.assertEqual(regions[106]['camera_bounds'],[0,0,22,13])
        self.assertEqual(regions[86]['camera_bounds'][0],12)
        self.assertEqual(regions[87]['camera_bounds'][2],48)
        self.assertEqual(regions[98]['camera_bounds'],[40,15,96,42])
        clamp=lambda x,r:max(r['camera_bounds'][0],min(r['camera_bounds'][2],x))
        self.assertEqual(clamp(48,regions[87]),clamp(48,regions[98]))
        for left,right,x in ((98,99,96),(99,100,168),(100,101,216),
                             (103,104,96),(104,105,168),(105,102,216)):
            self.assertEqual(clamp(x,regions[left]),clamp(x,regions[right]))
        clamp_y=lambda y,r:max(r['camera_bounds'][1],min(r['camera_bounds'][3],y+2))
        for lower,upper in ((98,103),(99,104),(100,105),(101,102)):
            self.assertEqual(clamp_y(40,regions[lower]),42)
            self.assertEqual(clamp_y(40,regions[lower]),clamp_y(40,regions[upper]))
        self.assertEqual(regions[102]['camera_bounds'],[216,32,258,68])

    @unittest.skipUnless((Path(__file__).resolve().parents[1]/'data/regions.json').is_file(),
                         'run host/regions.py first')
    def test_admitting_a_scene_only_appends_to_the_cooked_catalogue(self):
        """Every cooked chunk id must still mean the same view.

        The catalogue grows by appending, because `fixed_regions` lays the grid
        scenes out in `scene_id` order and `cook_fingerprints` leaves the scene
        table out of the cache key. A row inserted anywhere but the end would
        renumber chunks the cooked packs, the guest tables and the save records
        all address by id, and nothing else would notice.
        """
        import json
        cooked=json.loads((Path(__file__).resolve().parents[1]/'data/regions.json').read_text())['regions']
        planned=fixed_regions()
        self.assertGreaterEqual(len(planned),len(cooked))
        for row in cooked:
            plan=planned[row['chunk_id']-1]
            self.assertEqual((plan['scene_id'],plan['activation_bounds'],plan['camera_bounds']),
                             (row['scene_id'],row['activation_bounds'],row['camera_bounds']),
                             f"chunk {row['chunk_id']} moved")

    def test_greenpath_appends_the_views_its_isolated_pack_measured(self):
        """Scene 46 is the 24 views `tools/cook_scene_pack.py` priced, appended.

        Fungus1_01's numbers were measured on an isolated pack laid out by the
        same 24x16 stepping, so they only carry over if the catalog envelope
        reproduces that pack's views exactly. It does, and this pins it: 24
        views, appended after the Crossroads and the shop as chunks 700 to 723,
        with the first and last boxes the pack cooked.
        """
        regions=fixed_regions()
        greenpath=[r for r in regions if r['scene_name']=='Fungus1_01']
        self.assertEqual(len(greenpath),24)
        self.assertEqual(regions[699:],greenpath)
        self.assertEqual(greenpath[0]['activation_bounds'],[-2,-5,22,11])
        self.assertEqual(greenpath[-1]['activation_bounds'],[166,27,172,31])

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
