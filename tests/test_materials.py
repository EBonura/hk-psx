"""Source-free material admission and opaque/soft black regression fixtures."""
import copy,struct,sys,unittest
from pathlib import Path
from PIL import Image
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from materials import premultiplied_sprite,black_pixels
from cook import Atlas

class MaterialsTests(unittest.TestCase):
    def fixture(self):
        blend={k:{'val':v} for k,v in {'srcBlend':1,'destBlend':10,'srcBlendAlpha':1,'destBlendAlpha':10,'blendOp':0,'blendOpAlpha':0,'colMask':15}.items()}
        shader={'m_ParsedForm':{'m_Name':'Sprites/Lit','m_SubShaders':[{'m_Passes':[{'m_State':{'rtBlend0':blend}}]}]}}
        material={'m_SavedProperties':{'m_Floats':[['_EnableExternalAlpha',0]],'m_Colors':[['_Color',dict.fromkeys('rgba',1)]]}}
        return material,shader
    def test_source_blend_not_object_name_or_black_palette_alone(self):
        material,shader=self.fixture();self.assertTrue(premultiplied_sprite(material,shader))
        for key in ['srcBlend','destBlend','srcBlendAlpha','destBlendAlpha','blendOp','blendOpAlpha','colMask']:
            changed=copy.deepcopy(shader);changed['m_ParsedForm']['m_SubShaders'][0]['m_Passes'][0]['m_State']['rtBlend0'][key]['val']+=1
            self.assertFalse(premultiplied_sprite(material,changed))
        shader['m_ParsedForm']['m_Name']='Particles/Additive';self.assertFalse(premultiplied_sprite(material,shader))
    def test_external_alpha_tint_emission_and_unknown_shader_rejected(self):
        for patch in [{'m_Floats':[['_EnableExternalAlpha',1]]},{'m_Colors':[['_Color',dict.fromkeys('rgba',.5)]]},{'m_Colors':[['_EmissionColor',dict.fromkeys('rgba',1)]]},{'m_TexEnvs':[['_AlphaTex',{'m_Texture':{'m_PathID':22}}]]}]:
            m,s=self.fixture();m['m_SavedProperties'].update(patch);self.assertFalse(premultiplied_sprite(m,s))
        m,s=self.fixture();s['m_ParsedForm']['m_SubShaders']=[];self.assertFalse(premultiplied_sprite(m,s))
    def test_existing_opaque_and_soft_texels_are_preserved(self):
        im=Image.new('RGBA',(16,1));im.putdata([(0,0,0,a)for a in range(0,256,17)])
        atlas=Atlas();atlas.add(im,16,1);_,_,pal,pixels=atlas.quantized[0]
        self.assertTrue(black_pixels(im,pal))
        palette=struct.unpack('<16H',pal)
        samples=[palette[(pixels[x//2]>>((x&1)*4))&15] for x in range(16)]
        self.assertEqual(samples[0],0);self.assertEqual(samples[-1],1)
        self.assertIn(0x8000,samples)
        changed=im.copy();changed.putpixel((7,0),(1,0,0,128));self.assertFalse(black_pixels(changed,pal))
        self.assertFalse(black_pixels(Image.new('RGBA',(1,1)),pal))
        self.assertFalse(black_pixels(im,bytes(32)))


    def test_source_black_does_not_need_an_opaque_core_to_darken(self):
        from materials import binary_black_palette,black_scenery_palette
        for alpha in (96,213,255):
            im=Image.new('RGBA',(8,8),(0,0,0,alpha));atlas=Atlas();atlas.add(im,8,8)
            palette=atlas.quantized[0][2]
            self.assertTrue(black_scenery_palette(palette))
            self.assertTrue(black_pixels(im,palette))
            if alpha<224:self.assertNotIn(1,struct.unpack('<16H',palette))
        soft=struct.pack('<16H',0,*([0x8000]*15))
        self.assertFalse(binary_black_palette(soft)) # Dynamic fade contract unchanged.
        self.assertFalse(black_scenery_palette(bytes(32)))
        colored=Image.new('RGBA',(8,8),(1,0,0,213));self.assertFalse(black_pixels(colored,soft))

class AlphaCoverageTests(unittest.TestCase):
    def test_ordered_lifetime_coverage_has_bounded_constant_field_error(self):
        from materials import alpha_coverage
        for alpha in range(256):
            for opacity in (0,32,64,96,128):
                levels=[alpha_coverage(alpha,opacity,x,y)for y in range(4)for x in range(4)]
                self.assertTrue(set(levels)<={0,1,2})
                self.assertLessEqual(abs(sum(levels)/32-alpha*opacity/(255*128)),1/64+1e-12)
                self.assertEqual(levels,[alpha_coverage(alpha,opacity,x+4,y+8)for y in range(4)for x in range(4)])
        self.assertEqual(alpha_coverage(255,128,0,0),2)
        self.assertEqual(alpha_coverage(255,64,0,0),1)
        self.assertEqual(alpha_coverage(0,128,0,0),0)
    def test_rgb_is_straight_and_coverage_classes_survive_palette_quantization(self):
        from materials import quantize_alpha_coverage
        im=Image.new('RGBA',(4,4),(255,0,0,255))
        for opacity,expected in [(128,31),(64,0x801f)]:
            w,h,pal,pixels=quantize_alpha_coverage(im,opacity);words=struct.unpack('<16H',pal)
            self.assertEqual((w,h),(4,4));self.assertEqual(words[pixels[0]&15],expected)
            self.assertEqual((w,h,pal,pixels),quantize_alpha_coverage(im,opacity))
        _,_,pal,pixels=quantize_alpha_coverage(im,96);words=struct.unpack('<16H',pal)
        selected=[words[(pixels[i//2]>>((i&1)*4))&15]for i in range(16)]
        self.assertEqual(selected.count(31),8);self.assertEqual(selected.count(0x801f),8)
        _,_,pal,pixels=quantize_alpha_coverage(im,0);self.assertEqual(pal,bytes(32));self.assertEqual(pixels,bytes(8))
    def test_odd_row_padding_and_invalid_input_fail_explicitly(self):
        from materials import quantize_alpha_coverage
        im=Image.new('RGBA',(5,3),(70,120,250,255));w,h,pal,pixels=quantize_alpha_coverage(im,96)
        self.assertEqual(len(pixels),9)
        for row in range(3):self.assertEqual(pixels[row*3+2]&240,0)
        for opacity in [-1,129]:
            with self.assertRaises(ValueError):quantize_alpha_coverage(im,opacity)
        with self.assertRaises(ValueError):quantize_alpha_coverage(Image.new('RGBA',(257,1)),128)

if __name__=='__main__':unittest.main()
