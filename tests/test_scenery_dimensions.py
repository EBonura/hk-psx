"""Capped sampling keys must be independent of region/renderer iteration order."""
import math,sys,unittest
from pathlib import Path
from PIL import Image
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from cook import Atlas,scenery_dimensions,scenery_texture
class SceneryDimensions(unittest.TestCase):
    def test_float_ceil_overflow_is_capped_and_extreme_aspects_are_nonzero(self):
        # Actual IEEE rounding gives48.00000000000001 for this projected span.
        w=86.08782345942052;self.assertGreater(math.ceil(w*(48/w)),48)
        self.assertEqual(scenery_dimensions(w,23,48),(48,13))
        self.assertEqual(scenery_dimensions(.001,1e8,48),(1,48))
        self.assertEqual(scenery_dimensions(12,9,48),(12,9))
        for w,h in [(54.418772064244344,11.507937207229098),(154.021,88.781),(1e8,.001)]:
            for cap in (46,48,128):self.assertLessEqual(max(scenery_dimensions(w,h,cap)),cap)
        for dims in [(0,1,48),(1,float('inf'),48),(float('nan'),2,48),(1,2,0),(1,2,253)]:
            with self.assertRaises(ValueError):scenery_dimensions(*dims)
    def test_same_old_cache_bucket_with_different_targets_stays_separate(self):
        a=(54.9,11.1);b=(54.1,11.9)
        self.assertEqual(tuple(map(math.ceil,a)),tuple(map(math.ceil,b)))
        self.assertNotEqual(scenery_dimensions(*a,48),scenery_dimensions(*b,48))
        image=Image.new('RGBA',(31,19));image.putdata([(x*8,y*12,(x+y)*5,255)for y in range(19)for x in range(31)])
        def run(order,shared):
            atlas=Atlas();local={};out={}
            for name,dim in order:
                tid,pixels=scenery_texture(atlas,local,shared,'sprite:1',lambda:image,*dim,1.,48)
                out[name]=atlas.quantized[tid]
                self.assertEqual(pixels.size,tuple(out[name][:2]))
            return out
        forward=[('a',a),('b',b)];reverse=list(reversed(forward));shared={}
        self.assertEqual(run(forward,shared),run(reverse,shared))
        self.assertEqual(run(forward,{}),run(reverse,{}))
        self.assertEqual(len(shared),2)
    def test_different_projection_same_final_size_reuses_single_source_sample(self):
        atlas=Atlas();local={};shared={};calls=[]
        def source():calls.append(1);return Image.new('RGBA',(80,40),(200,30,80,255))
        first,_=scenery_texture(atlas,local,shared,'sprite:2',source,100,50,1.,48)
        second,_=scenery_texture(atlas,local,shared,'sprite:2',source,200,100,1.,48)
        self.assertEqual(first,second);self.assertEqual(len(calls),1);self.assertEqual(len(local),1)
        # A second region has its own Atlas but receives the same size/pixels.
        other=Atlas();tid,_=scenery_texture(other,{},shared,'sprite:2',source,300,150,1.,48)
        self.assertEqual(atlas.quantized[first],other.quantized[tid]);self.assertEqual(len(calls),1)
        scenery_texture(other,{},shared,'sprite:2',source,100,50,.5,48)
        self.assertEqual(len(calls),2)
        # Exact source alphas sharing the same8bit bucket cannot share pixels.
        scenery_texture(other,{},shared,'sprite:2',source,100,50,.5001,48)
        self.assertEqual(len(calls),3)
if __name__=='__main__':unittest.main()
