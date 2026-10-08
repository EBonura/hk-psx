"""Exact source-cell coverage and fail-closed black tilemap admission."""
import random,sys,unittest
from pathlib import Path
from PIL import Image
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from tilemap_fill import mesh_cells,merge_cells,opaque_sample_support,BLACK_PALETTE,BLACK_PIXELS,append_tilemap_fills


def fixture(cells,reflected=False):
    vertices=[];triangles=[]
    for x,y in cells:
        n=len(vertices);vertices.extend([(x,y,0.),(x+1,y,0.),(x,y+1,0.),(x+1,y+1,0.)])
        triangles.extend([(n,n+2,n+1),(n+2,n+3,n+1)]if not reflected else[(n+1,n+3,n),(n+3,n+2,n)])
    return vertices,triangles,[(1.,1.,1.,1.)]*len(vertices)


class TilemapFillTests(unittest.TestCase):
    def test_mesh_proof_handles_both_diagonals_and_preserves_holes(self):
        cells={(x,y)for x in range(9)for y in range(7)}-{(x,y)for x in range(2,6)for y in range(2,5)}
        for reflected in (False,True):self.assertEqual(mesh_cells(*fixture(sorted(cells),reflected)),cells)
        rects=merge_cells(cells);restored={(x,y)for x0,y0,x1,y1 in rects for y in range(y0,y1)for x in range(x0,x1)}
        self.assertEqual(restored,cells);self.assertEqual(sum((r[2]-r[0])*(r[3]-r[1])for r in rects),len(cells))
        self.assertNotIn((3,3),restored)
    def test_random_cell_cover_is_exact_order_independent_and_disjoint(self):
        rng=random.Random(93)
        for _ in range(100):
            cells={(x,y)for x in range(-8,9)for y in range(-8,9)if rng.random()<.65}
            rects=merge_cells(cells);order=list(cells);rng.shuffle(order)
            self.assertEqual(rects,merge_cells(order))
            restored=[]
            for x0,y0,x1,y1 in rects:restored.extend((x,y)for y in range(y0,y1)for x in range(x0,x1))
            self.assertEqual(len(restored),len(set(restored)));self.assertEqual(set(restored),cells)
    def test_missing_duplicate_or_non_cell_triangles_rejected(self):
        v,t,c=fixture([(0,0)])
        for triangles in [t[:1],t+[t[0]],[t[0],t[0]],[(0,1,2),(0,1,99)]]:
            with self.assertRaises(ValueError):mesh_cells(v,triangles,c)
        for index,value in [(0,(.1,0,0)),(0,(0,0,1)),(3,(2,1,0))]:
            vv=v[:];vv[index]=value
            with self.assertRaises(ValueError):mesh_cells(vv,t,c)
        cc=c[:];cc[0]=(1,1,1,.5)
        with self.assertRaises(ValueError):mesh_cells(v,t,cc)
    def test_bilinear_support_includes_boundary_and_requires_opaque_black(self):
        image=Image.new('RGBA',(128,128));image.paste((0,0,0,255),(0,60,68,128))
        uv=[(.015632812,.015632812),(.51561719,.51561719)]
        bounds=opaque_sample_support(image,uv);self.assertEqual(bounds,(1,61,67,127))
        for color in [(0,0,0,254),(1,0,0,255)]:
            bad=image.copy();bad.putpixel((1,61),color)
            with self.assertRaises(ValueError):opaque_sample_support(bad,uv)
        with self.assertRaises(ValueError):opaque_sample_support(image,[(float('nan'),0)])
    def test_source_culled_fill_keeps_order_and_uses_one_exact_texture(self):
        class Atlas:
            def __init__(self):self.calls=[]
            def add_quantized(self,*args):self.calls.append(args);return 7
        class Scene:pass
        sc=Scene();sc._tilemap_fills=[{'source':'level6:1','mesh_source':'level6:2','name':'source mesh','layer':4,'order':9,'material':{'mode':1,'texture_source':'atlas:1'},'cell_count':2,'source_triangle_count':4,'rectangle_count':2,
            'points':[[[0,1,0],[1,1,0],[0,0,0],[1,0,0]],[[99,1,0],[100,1,0],[99,0,0],[100,0,0]]]}]
        atlas=Atlas();draws=[];report=append_tilemap_fills(sc,atlas,draws,{},(0,1),(0,1),600,-40)
        self.assertEqual(len(draws),1);self.assertEqual(atlas.calls,[(4,4,BLACK_PALETTE,BLACK_PIXELS)])
        self.assertEqual((draws[0]['layer'],draws[0]['order'],draws[0]['texture']),(4,9,7));self.assertEqual(report['region_draws'],1)

if __name__=='__main__':unittest.main()
