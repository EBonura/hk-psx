import unittest,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from particles import curve_value,gradient_alpha,generated_style,generated_emitter
class ParticleSourceTests(unittest.TestCase):
    def curve(self,values):
        return {'m_Curve':[dict(time=t,value=v,inSlope=s,outSlope=s,weightedMode=0)for t,v,s in values]}
    def test_hermite_endpoints_and_source_slopes_are_not_linearized(self):
        c=self.curve([(0,0,0),(1,1,0)])
        self.assertEqual(curve_value(c,0),0);self.assertEqual(curve_value(c,1),1)
        self.assertAlmostEqual(curve_value(c,.25),.15625);self.assertEqual(curve_value(c,.5),.5)
        linear=self.curve([(0,0,1),(1,1,1)])
        for i in range(65):self.assertAlmostEqual(curve_value(linear,i/64),i/64)
    def test_unsupported_weighted_curves_fail_explicitly(self):
        c=self.curve([(0,0,1),(1,1,1)]);c['m_Curve'][0]['weightedMode']=1
        with self.assertRaisesRegex(ValueError,'weighted'):curve_value(c,.5)
    def test_source_alpha_uses_alpha_times_not_rgb_times(self):
        g={'m_Mode':0,'m_NumAlphaKeys':3,'atime0':0,'atime1':55127,'atime2':65535,
           'key0':{'a':1},'key1':{'a':1},'key2':{'a':0}}
        self.assertEqual(gradient_alpha(g,.8),1);self.assertEqual(gradient_alpha(g,1),0)
        self.assertAlmostEqual(gradient_alpha(g,(55127+65535)/2/65535),.5)
    def test_emitter_keeps_complete_shape_basis_and_source_identity(self):
        e={'state':5,'source':12289,'origin':[1,2,3],'basis':[[0,10,0],[0,0,20],[30,0,0]],'direction':[0,65536,0]}
        result=generated_emitter(e)
        self.assertIn('state:5,source:12289',result);self.assertIn('basis:[[0,10,0],[0,0,20],[30,0,0]]',result)
    def test_generated_style_keeps_emission_and_lifetime_curves(self):
        s={'life':[42,78],'speed':[3,30],'size':[7,9],'force':[-8,-4],'dampen':19661,'rotation':200,'count':25,'duration':6,'uv_scale':65529,
           'colors':[[128]*3]*2,'curves':[{'size':65536,'alpha':[255,0]}]}
        result=generated_style(s)
        self.assertIn('life:[42,78]',result);self.assertIn('count:25,duration:6',result)
        self.assertIn('alpha:[255,0]',result)
