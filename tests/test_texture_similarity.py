import struct
import sys
import unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from audit_texture_similarity import words,metrics,exactkey

def texture(data,mode=0):
    return dict(words=np.array(data,dtype=np.uint16),w=len(data[0]),h=len(data),modes=(mode,),black=False,stream=False,covers=())

class TextureSimilarityTests(unittest.TestCase):
    def test_odd_width_palette_permutation_decodes_same_exact_texels(self):
        a=struct.pack('<HH16H',3,1,0,0x801f,0x03e0,*([0]*13))+bytes([0x21,0])
        b=struct.pack('<HH16H',3,1,0,0x03e0,0x801f,*([0]*13))+bytes([0x12,0])
        np.testing.assert_array_equal(words(a),words(b))
        self.assertEqual(words(a).tolist(),[[0x801f,0x03e0,0]])
    def test_transparent_padding_does_not_inflate_score(self):
        a=texture([[31,0]]);b=texture([[0,31]])
        x=texture([[31,0]+[0]*126]);y=texture([[0,31]+[0]*126])
        self.assertEqual(metrics(a,b),metrics(x,y))
        self.assertEqual(metrics(a,b)['coverage_mismatch'],1)
    def test_black_transparency_difference_visible_on_bright_background(self):
        a=texture([[0x8000]],1);b=texture([[0]],1)
        self.assertLess(metrics(a,b)['similarity'],50)
        self.assertEqual(metrics(a,b)['stp_mismatch'],1)
    def test_exact_identity_preserves_stp_even_if_additive_black_is_invisible(self):
        a=texture([[0x8000]]);b=texture([[0]])
        self.assertEqual(metrics(a,b)['similarity'],100)
        self.assertNotEqual(exactkey(a),exactkey(b))
        self.assertEqual(metrics(a,b)['coverage_mismatch'],1)
    def test_one_color_channel_step_has_bounded_nonzero_error(self):
        m=metrics(texture([[0x7fff]]),texture([[0x7ffe]]))
        self.assertGreater(m['similarity'],98)
        self.assertLess(m['similarity'],100)
        self.assertEqual(m['coverage_mismatch'],0)

if __name__=='__main__':unittest.main()
