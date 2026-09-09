"""Synthetic original fixtures: no retail data in committed tests."""
import unittest,sys,struct
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from PIL import Image
from cook import Atlas, unsupported_sprite_behavior
from scene import Scene
from combat import ticks, transformed_box, generated_params

class CookTests(unittest.TestCase):
    def test_configured_static_page_budget_fails_without_resampling_animation(self):
        atlas=Atlas(max_pages=1)
        for index in range(5):
            atlas.add(Image.new('RGBA',(128,128),(index*32,100,200,255)),128,128)
        with self.assertRaisesRegex(ValueError,'VRAM page budget'):atlas.pack()
        atlas=Atlas(max_pages=0)
        atlas.add(Image.new('RGBA',(61,62),(200,100,30,255)),61,62,streamed=True)
        atlas.pack()
        self.assertEqual(atlas.entries[0][3:5],(61,62))
        self.assertEqual(len(atlas.pages),0)

    def test_palette_transparency_and_vram_bounds(self):
        a=Atlas();im=Image.new('RGBA',(8,4),(0,0,0,255));im.putpixel((0,0),(0,0,0,0));a.add(im,8,4);a.pack()
        page,x,y,w,h,cl,_=a.entries[0]
        data=a.pages[page];self.assertEqual(data[y*128+x//2]&15,0)
        index=data[y*128+x//2]>>4
        self.assertNotEqual(int.from_bytes(a.palettes[cl][index*2:index*2+2],'little'),0)
        self.assertLessEqual(x+w,256);self.assertLessEqual(y+h,256)
    def test_stream_preserves_quantized_pixels_palette_and_dimensions(self):
        image=Image.new('RGBA',(5,3),(31,127,235,255))
        image.putpixel((0,0),(0,0,0,0));image.putpixel((4,2),(245,32,19,100))
        a=Atlas();a.add(image,5,3);a.pack()
        b=Atlas();b.add(image,5,3,streamed=True);b.pack()
        page,x,y,w,h,cl,_=a.entries[0]
        self.assertEqual(b.entries[0],(65535,0,0,5,3,cl,0))
        self.assertEqual(a.palettes,b.palettes);self.assertFalse(b.pages)
        self.assertEqual(len(b.stream),12)
        for yy in range(h):
            for xx in range(w):
                original=(a.pages[page][(y+yy)*128+(x+xx)//2]>>(((x+xx)&1)*4))&15
                streamed=(b.stream[yy*4+xx//2]>>((xx&1)*4))&15
                self.assertEqual(original,streamed)
            self.assertEqual(b.stream[yy*4+2]&240,0);self.assertEqual(b.stream[yy*4+3],0)
    def test_stream_limits_and_image_alignment(self):
        a=Atlas()
        with self.assertRaisesRegex(ValueError,'64x64'):
            a.add(Image.new('RGBA',(65,1)),65,1,streamed=True)
        a.add(Image.new('RGBA',(1,1)),1,1,streamed=True)
        a.add(Image.new('RGBA',(1,1),(255,255,255,255)),1,1,streamed=True);a.pack()
        self.assertEqual([e[6] for e in a.entries],[0,4]);self.assertEqual(len(a.stream),8)
        # Packing is reproducible when called again and does not append pages.
        before=(a.entries.copy(),bytes(a.stream),a.palettes.copy());a.pack()
        self.assertEqual(before,(a.entries,bytes(a.stream),a.palettes))
    def test_exact_quantized_duplicates_share_texture_not_draw_geometry(self):
        a=Atlas();one=Image.new('RGBA',(5,3),(9,16,24,255));two=Image.new('RGBA',(5,3),(10,17,25,255))
        self.assertEqual(a.add(one,5,3),0)
        self.assertEqual(a.add(two,5,3),0)  # Different RGBA, identical final PS1 colors.
        self.assertEqual(a.add(one,5,3,streamed=True),1)
        self.assertEqual(a.request_map,[0,0,1]);a.pack()
        self.assertEqual(len(a.entries),2)
        self.assertNotEqual(a.entries[0][0],a.entries[1][0])
        self.assertEqual(a.quantized[0],a.quantized[1])

    def test_nested_rotation_scale_and_inactive_parent(self):
        s=Scene.__new__(Scene)
        def tr(go,parent,p,scale=(1,1,1),q=(0,0,0,1)):
            return {'m_GameObject':{'m_PathID':go},'m_Father':{'m_PathID':parent},'m_LocalPosition':dict(zip('xyz',p)),'m_LocalScale':dict(zip('xyz',scale)),'m_LocalRotation':dict(zip('xyzw',q))}
        s.transforms={1:tr(10,0,(5,7,0),(2,2,1)),2:tr(20,1,(3,4,0))};s.go_transform={10:1,20:2};s.gos={10:{'m_IsActive':False},20:{'m_IsActive':True}}
        self.assertEqual(s.point(20),(11,15,0));self.assertFalse(s.active(20))
    def test_palette_limit_is_not_silent(self):
        a=Atlas()
        for i in range(641):a.add_quantized(1,1,struct.pack('<16H',0,i+1,*([0]*14)),b'\x01')
        with self.assertRaisesRegex(ValueError,'CLUT budget'):a.pack()
    def test_inverse_remasker_exclusion_is_behavior_specific(self):
        self.assertEqual(unsupported_sprite_behavior('Inverse Remasker','mask_container',['PlayMakerFSM','SpriteRenderer']),'FSM-controlled inverse remasker')
        for name,parent,types in [('rock','mask_container',['PlayMakerFSM']),('Inverse Remasker','scenery',['PlayMakerFSM']),('Inverse Remasker','mask_container',['SpriteRenderer'])]:
            self.assertIsNone(unsupported_sprite_behavior(name,parent,types))
    def test_nail_timing_and_effect_source_transform(self):
        self.assertEqual(ticks(.349999994),21)
        self.assertEqual(ticks(.409999996),25)
        self.assertEqual(ticks(.02),2)
        self.assertEqual(ticks(.1),6)
        nail={'position':{'x':1,'y':-2},'scale':{'x':2,'y':3}}
        self.assertEqual(transformed_box((-2,-1,0,1),nail),(-3,-5,1,1))
    def test_grass_draw_switch_requires_both_source_renderers(self):
        constants={'ATTACK_DURATION':.35,'ATTACK_COOLDOWN_TIME':.41,'ALT_ATTACK_RESET':.5}
        nails=[{'polygon':[(0,0),(1,0),(0,1)]}]*4
        grass=[{'off':10,'on':20,'box':(0,0,1,1)}]
        with self.assertRaisesRegex(ValueError,'renderer omitted'):
            generated_params(constants,.02,nails,grass,[{'source':'level6:10'}])
        result=generated_params(constants,.02,nails,grass,[{'source':'level6:20'},{'source':'level6:10'}])
        self.assertIn('off_draw:1,on_draw:0',result)
        self.assertEqual(grass[0]['on_draw'],0)
        nails[0]={'polygon':[(17,0),(1,0),(0,1)]}
        with self.assertRaisesRegex(ValueError,'Q16 bound'):
            generated_params(constants,.02,nails,grass,[])
        nails[0]={'polygon':[(0,0),(1,0),(0,1)]}
        grass[0]['box']=(0,0,513,1)
        with self.assertRaisesRegex(ValueError,'Q16 bound'):
            generated_params(constants,.02,nails,grass,[])
if __name__=='__main__':unittest.main()
