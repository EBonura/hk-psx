"""Source-free tests of break-particle curves, art mapping and guarded allocation."""
import math,sys,unittest
from pathlib import Path
from types import SimpleNamespace
from PIL import Image
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
import break_effects as b
class BreakEffectsTests(unittest.TestCase):
    def curve(self,points):return {'m_Curve':[dict(time=t,value=v,inSlope=d,outSlope=d,weightedMode=0)for t,v,d in points]}
    def test_two_curves_preserve_independent_source_slopes_and_reverse_ranges(self):
        c={'minMaxState':2,'minScalar':2,'scalar':4,'minCurve':self.curve([(0,-1,0),(1,0,0)]),'maxCurve':self.curve([(0,1,0),(1,0,0)])}
        self.assertEqual(b.values(c,0),[-2,4]);self.assertEqual(b.values(c,1),[0,0]);self.assertEqual(b.values(c,.5),[-1,2])
        self.assertEqual(b.values({'minMaxState':3,'minScalar':2,'scalar':1}),[2,1])
        c['maxCurve']['m_Curve'][0]['weightedMode']=1
        with self.assertRaisesRegex(ValueError,'weighted'):b.values(c,.5)
    def test_fixed_point_rejects_unbounded_or_nonfinite_source(self):
        for x in [math.inf,math.nan,32768,-32768]:
            with self.assertRaises(ValueError):b.q(x)
        self.assertEqual(b.q(-.5),-32768)
    def test_every_original_cell_and_three_alpha_variants_have_disjoint_legal_art(self):
        image=Image.new('RGBA',(36,144));image.putdata([(255,70*(y//36),50,(0,128,255)[x%3])for y in range(144)for x in range(36)])
        obj=SimpleNamespace(read=lambda:SimpleNamespace(image=image))
        styles=[{'texture':'original:1','cells':4,'size':[32768,52429]}]
        blob,uploads,art,frames,_=b.art_bank(styles,{'original:1':obj})
        self.assertEqual(len(art),12);self.assertEqual(len(frames['original:1']),4)
        self.assertEqual(sorted(i for row in frames['original:1']for i in row),list(range(12)))
        cells=set()
        for u in uploads:
            self.assertTrue(any(x<=u['x'] and y<=u['y'] and u['x']+u['w']<=x+w and u['y']+u['h']<=y+h for x,y,w,h in b.VRAM_RECTS))
            xy={(x,y)for x in range(u['x'],u['x']+u['w'])for y in range(u['y'],u['y']+u['h'])}
            self.assertFalse(cells&xy);cells|=xy
        self.assertEqual(len(cells)*2,len(blob))
        for a in art:self.assertLessEqual(a['u']+a['w'],256);self.assertLessEqual(a['v']+a['h'],256)
        self.assertEqual((blob,uploads,art,frames),b.art_bank(styles,{'original:1':obj})[:4])
    def test_mixed_source_sheet_layout_is_rejected(self):
        obj=SimpleNamespace(read=lambda:SimpleNamespace(image=Image.new('RGBA',(36,144))))
        with self.assertRaisesRegex(ValueError,'mixed sheet'):
            b.art_bank([{'texture':'original:1','cells':4,'size':[1,1]},{'texture':'original:1','cells':3,'size':[1,1]}],{'original:1':obj})
    def test_particle_track_rounding_and_explicit_unrepresentable_fallback(self):
        style=dict(shape=10,cells=1,count=128,life=[60,120],speed=[3,99],size=[65536,65536],rotation=[0,23592959],radius_scale=65536,
                   samples=[dict(size=[65536,65536],spin=[-33554432,33554431],alpha=[255,0])for _ in range(33)])
        emitters=[dict(style=0,source=8726),dict(style=0,source=4)]
        tracks,bases,fallback=b.particle_tracks([style],emitters)
        self.assertEqual((len(tracks),bases,fallback),(256,[0,128],[]))
        for track in tracks:
            self.assertEqual(len(track),33)
            for geometry,spin_alpha in track:
                self.assertEqual(geometry,32768|(32768<<16))
                spin=(spin_alpha&0x3ffffff)-((spin_alpha>>25&1)<<26)
                self.assertTrue(-33554432<=spin<=33554431)
                self.assertTrue(0<=spin_alpha>>26<=3)
        for key,value in [('size',[131072,131072]),('size',[-65536,-65536]),('radius_scale',3*65536),
                          ('samples',[dict(size=[65536,65536],spin=[33554432,33554432],alpha=[255,255])for _ in range(33)])]:
            wider=style|{key:value}
            self.assertEqual(b.particle_tracks([wider],emitters),([],[65535,65535],[0,1]))
        mixed=b.particle_tracks([style,style|{'size':[131072,131072]}],[emitters[0],dict(style=1,source=3),emitters[1]])
        self.assertEqual((len(mixed[0]),mixed[1],mixed[2]),(256,[0,65535,128],[1]))

if __name__=='__main__':unittest.main()
