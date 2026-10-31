"""Asset-free checks for mandatory scenery repair packet admission."""
from fractions import Fraction
import random
import unittest
from host.scenery_geometry import (packet_bound,axis_offsets,check_camera_arithmetic,mandatory_packets,
                                   SAFE_WIDTH,SAFE_HEIGHT)


def runtime_children(v,w,h,divisions):
    """Children `subdivide_in` (game/src/scenery_geometry.rs) keeps at one grid:
    every cell not wholly left of 0, right of 320, above 0 or below 240."""
    us,vs=axis_offsets(w-1,divisions),axis_offsets(h-1,divisions)
    def point(u,t):
        weights=[(w-1-u)*(h-1-t),u*(h-1-t),(w-1-u)*t,u*t]
        return tuple(round(Fraction(sum(p[k]*a for p,a in zip(v,weights)),(w-1)*(h-1)))for k in range(2))
    grid=[[point(u,t)for u in us]for t in vs]
    count=0
    for j in range(len(vs)-1):
        for i in range(len(us)-1):
            c=[grid[j][i],grid[j][i+1],grid[j+1][i],grid[j+1][i+1]]
            outside=(all(p[0]<=0 for p in c)or all(p[0]>=320 for p in c)
                     or all(p[1]<=0 for p in c)or all(p[1]>=240 for p in c))
            count+=not outside
    return count


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

    def test_on_screen_packets_bound_the_children_the_renderer_keeps(self):
        # Axis-aligned quads of any size, either orientation, flipped or not, at
        # random camera offsets and texture sizes: no coarser grid the renderer
        # may stop at (it starts at 2 divisions and doubles) keeps more
        # children than `on_screen`, and the bound is not the whole grid for a
        # long background layer.
        rng=random.Random(7);tightened=0
        for _ in range(400):
            w,h=rng.choice([(128,41),(28,216),(64,64),(7,5),(256,256),(33,200)])
            x0=rng.randint(-3000,1000)*256+rng.randint(0,255);x1=x0+rng.choice([-1,1])*rng.randint(700,4400)*256
            y0=rng.randint(-1500,800)*256+rng.randint(0,255);y1=y0+rng.choice([-1,1])*rng.randint(1,1500)*256
            quad=[(x0,y0),(x1,y0),(x0,y1),(x1,y1)]
            if rng.random()<.5:quad=[(x,y)for y,x in quad]
            try:bound=packet_bound(quad,w,h)
            except ValueError:continue
            on_screen=bound['packets'].on_screen
            self.assertLessEqual(on_screen,bound['packets'])
            tightened+=on_screen<bound['packets']
            for _ in range(6):
                cx,cy=rng.randint(-3000,3000)*256+rng.randint(0,255),rng.randint(-1500,1500)*256+rng.randint(0,255)
                v=[(160+((x-cx)>>8),120-((y-cy)>>8))for x,y in quad]
                n=2
                while n<=bound['divisions']:
                    self.assertLessEqual(runtime_children(v,w,h,n),on_screen,(quad,w,h,n,cx,cy))
                    n*=2
        self.assertGreater(tightened,50)

    def test_a_slanted_quad_keeps_its_whole_grid(self):
        quad=[(0,0),(4000*256,300*256),(-200*256,900*256),(3800*256,1200*256)]
        bound=packet_bound(quad,128,41)
        self.assertGreater(bound['packets'],1)
        self.assertEqual(bound['packets'].on_screen,bound['packets'])

    def test_mandatory_totals_use_on_screen_bounds_unless_asked_for_the_grid(self):
        wide=packet_bound([(-1400*256,700*256),(1700*256,700*256),(-1400*256,-300*256),(1700*256,-300*256)],128,41)['packets']
        self.assertEqual(int(wide),64)
        self.assertLess(mandatory_packets([wide,1],[]),mandatory_packets([wide,1],[],grid=True))
        self.assertEqual(mandatory_packets([wide,1],[],grid=True),65)
        self.assertEqual(mandatory_packets([5,1],[[0,1]]),5)

    def test_small_draw_needs_no_expansion(self):
        self.assertEqual(packet_bound([(0,0),(100*256,0),(0,100*256),(100*256,100*256)],1,1)['packets'],1)


if __name__=='__main__':
    unittest.main()
