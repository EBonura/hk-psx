"""Asset-free checks for mandatory scenery repair packet admission."""
from fractions import Fraction
import unittest
from host.scenery_geometry import (packet_bound,axis_offsets,check_camera_arithmetic,
                                   SAFE_WIDTH,SAFE_HEIGHT)


class SceneryGeometryBudgets(unittest.TestCase):
    def test_coordinate_only_failure_is_budgeted(self):
        # Width900 is below edge rejection, but a partly visible translation can
        # place its right edge beyond signed11. Admission must reserve a split.
        xy=[(0,0),(900*256,0),(0,200*256),(900*256,200*256)]
        bound=packet_bound(xy,48,48)
        self.assertGreater(bound['packets'],1)
        self.assertLessEqual(bound['child_extent_bound'][0],SAFE_WIDTH)

    def test_source_rounding_and_uneven_uv_intervals_are_conservative(self):
        source=[(-370*256+17,-270*256+99),(820*256+249,-201*256+201),
                (-511*256+79,653*256+17),(679*256+12,722*256+250)]
        for w,h in [(48,48),(47,39),(7,5)]:
            bound=packet_bound(source,w,h)
            us=axis_offsets(w-1,bound['divisions']);vs=axis_offsets(h-1,bound['divisions'])
            self.assertEqual(bound['packets'],(len(us)-1)*(len(vs)-1))
            for phase_x in [0,1,127,128,255,256]:
                for phase_y in [0,127,255,256]:
                    projected=[((x-phase_x)//256,-((y-phase_y)//256))for x,y in source]
                    grid=[]
                    for v in vs:
                        row=[]
                        for u in us:
                            weights=[(w-1-u)*(h-1-v),u*(h-1-v),(w-1-u)*v,u*v]
                            row.append(tuple(round(Fraction(sum(p[k]*a for p,a in zip(projected,weights)),(w-1)*(h-1)))for k in range(2)))
                        grid.append(row)
                    for y in range(len(vs)-1):
                        for x in range(len(us)-1):
                            q=[grid[y][x],grid[y][x+1],grid[y+1][x],grid[y+1][x+1]]
                            extent=[max(p[k]for p in q)-min(p[k]for p in q)for k in range(2)]
                            self.assertLessEqual(extent[0],bound['child_extent_bound'][0])
                            self.assertLessEqual(extent[1],bound['child_extent_bound'][1])
                            self.assertLessEqual(extent[0],SAFE_WIDTH)
                            self.assertLessEqual(extent[1],SAFE_HEIGHT)

    def test_single_texel_large_geometry_is_explicitly_unsupported(self):
        with self.assertRaises(ValueError):
            packet_bound([(0,0),(1200*256,0),(0,1000*256),(1200*256,1000*256)],1,1)

    def test_large_camera_product_is_widened_before_shift(self):
        # Real scale/camera range that overflows the old intermediate i32.
        coord,product=check_camera_arithmetic([(0,0)]*4,92868,[72,0,96,20])
        self.assertEqual(product,2282323968)
        self.assertLess(coord,1<<20)

    def test_small_draw_needs_no_expansion(self):
        self.assertEqual(packet_bound([(0,0),(100*256,0),(0,100*256),(100*256,100*256)],1,1)['packets'],1)


if __name__=='__main__':
    unittest.main()
