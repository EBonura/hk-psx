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
        self.assertEqual(regions[86]['camera_bounds'][0],10)
        self.assertEqual(regions[87]['camera_bounds'][2],48)
        self.assertEqual(regions[98]['camera_bounds'],[40,8,96,42])
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
        self.assertEqual(regions[699:723],greenpath)
        self.assertEqual(greenpath[0]['activation_bounds'],[-2,-5,22,11])
        self.assertEqual(greenpath[-1]['activation_bounds'],[166,27,172,31])

    def test_greenpath_records_the_two_layouts_the_grid_could_not_hold(self):
        """Fungus1_02 and Fungus1_19 carry the boxes their cook chose, not a grid.

        Both hold a 24x16 view that cooks 6 pages against STATIC_PAGE_BUDGET 5,
        so the plain grid is not a layout either scene can be admitted at and
        the packs that priced them were laid out by halving the offending view.
        Those boxes are recorded in `quality.MEASURED_VIEW_LAYOUTS` rather than
        derived, which makes them the one thing in the catalogue that a later
        cooker could silently disagree with. This is what notices: the recorded
        layout has to be what the catalogue lays out, it has to cover the
        envelope with no hole and no overlap, and it has to still be finer than
        the grid it replaces.
        """
        from quality import MEASURED_VIEW_LAYOUTS,grid_layout
        regions=fixed_regions()
        self.assertEqual(set(MEASURED_VIEW_LAYOUTS),{'Fungus1_02','Fungus1_19'})
        for name,boxes in MEASURED_VIEW_LAYOUTS.items():
            scene=next(s for s in SCENES if s['scene_name']==name)
            laid=[r['activation_bounds'] for r in regions if r['scene_name']==name]
            self.assertEqual(laid,[list(b) for b in boxes],name)
            self.assertGreater(len(boxes),len(grid_layout(scene['runtime_bounds'])),name)
            x0,y0,x1,y1=scene['runtime_bounds']
            self.assertEqual(sum((b[2]-b[0])*(b[3]-b[1]) for b in boxes),(x1-x0)*(y1-y0),name)
            for x in range(x0,x1):
                for y in range(y0,y1):
                    self.assertTrue(any(b[0]<=x+.5<=b[2] and b[1]<=y+.5<=b[3] for b in boxes),
                                    f'{name} leaves ({x}, {y}) in no view')

    def test_greenpath_stays_inside_the_catalogue_chunk_bound(self):
        """`world.generate` refuses above 1024 chunks, and that is what limits
        how much of Greenpath a batch can admit. It is the region catalogue and
        not RAM: the twenty-four Greenpath scenes that cook on the plain grid
        would need 404 more views against the 301 free at build 159."""
        self.assertLessEqual(len(fixed_regions()),1024)

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


class CookReuseTests(unittest.TestCase):
    """A region's reuse survives the provenance list growing, not a file changing."""
    def test_growth_keeps_a_cook_and_a_changed_input_drops_it(self):
        import hashlib,json,tempfile
        from regions import cook_cache_key,cached_cook,record_cook,inputs_digest
        region={'scene_id':3,'activation_bounds':[0,0,1,1]}
        with tempfile.TemporaryDirectory() as temp:
            d=Path(temp)
            for name in ('scene.json','unsupported.json'):(d/name).write_text('{}')
            (d/'room.hk').write_bytes(b'pack')
            cooked={'pack_sha256':hashlib.sha256(b'pack').hexdigest()}
            first={'code':'c','inputs':{'a':'1','b':'2'}}
            # A pack cooked before inputs were recorded is kept once, and rewritten.
            legacy=cook_cache_key(region,first)['legacy']
            (d/'cook-cache.json').write_text(json.dumps({'key':legacy,'cooked':cooked}))
            self.assertEqual(cached_cook(d,cook_cache_key(region,first)),cooked)
            self.assertIn('input_names',json.loads((d/'cook-cache.json').read_text()))
            grown={'code':'c','inputs':{'a':'1','b':'2','new':'3'}}
            self.assertEqual(cached_cook(d,cook_cache_key(region,grown)),cooked)
            self.assertIsNone(cached_cook(d,cook_cache_key(region,{'code':'c','inputs':{'a':'1','b':'9','new':'3'}})))
            self.assertIsNone(cached_cook(d,cook_cache_key(region,{'code':'other','inputs':grown['inputs']})))

if __name__ == '__main__':
    unittest.main()
