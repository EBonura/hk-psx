"""Source-free exact grouped opacity, binding and admission tests."""
import random,struct,subprocess,sys,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'host'))
from opaque_groups import group_phases,connected_groups,solid_word_one,select_groups
class OpaqueGroups(unittest.TestCase):
    def test_joint_camera_phases_cover_fractional_member_offsets(self):
        rng=random.Random(918)
        for count in(2,3,4):
            for _ in range(10):
                xy=[]
                for i in range(count):
                    x=rng.randrange(-30000,30000);y=rng.randrange(-30000,30000);w=rng.randrange(1,10000);h=rng.randrange(1,10000);xy +=[x,y,x+w,y,x,y+h,x+w,y+h]
                variants={tuple(v)for v in group_phases(xy)};self.assertLessEqual(len(variants),256)
                for _ in range(100):
                    cx=rng.randrange(-1000000,1000000);cy=rng.randrange(-1000000,1000000);v=[((xy[i]-cx)>>8,-((xy[i+1]-cy)>>8))for i in range(0,len(xy),2)];self.assertIn(tuple((x-v[0][0],y-v[0][1])for x,y in v),variants)
        with self.assertRaises(ValueError):group_phases([0]*8)
    def test_connected_members_require_common_scale_and_bounded_extent(self):
        def box(x,w=16,scale=60693):return{'xy':[x*256,0,(x+w)*256,0,x*256,48*256,(x+w)*256,48*256],'scale':scale}
        self.assertEqual(list(connected_groups({3:box(0),7:box(16),9:box(100,scale=2)})),[(3,7)])
        self.assertEqual(list(connected_groups({3:box(0,704),7:box(16)})),[])
        self.assertEqual(list(connected_groups({3:box(0),7:box(17)})),[])
    def test_only_exact_resident_sentinel_qualifies(self):
        raw=bytearray(32832);sections={'textures':{'offset':0},'palettes':{'offset':16},'pages':{'offset':64}};struct.pack_into('<6HI',raw,0,0,0,0,2,2,0,0);raw[64]=0x11;raw[64+128]=0x11
        for word,want in[(1,True),(0,False),(0x8001,False),(0x8000,False),(2,False)]:struct.pack_into('<H',raw,18,word);self.assertEqual(solid_word_one(raw,sections,0),want)
        struct.pack_into('<H',raw,0,65535);self.assertFalse(solid_word_one(raw,sections,0))
    def test_budget_admits_atomic_groups_and_shares_identical_proofs(self):
        c={'gx':0,'gy':0,'width':1,'height':1,'words':[1]};groups=[{'scene':0,'members':(i*2,i*2+1),'pose':0,'regions':[i+1]}for i in range(2)]
        selected,size,_=select_groups(groups,[c],1,budget=68);self.assertEqual(len(selected),2);self.assertEqual(size,68)
        selected,size,_=select_groups(groups,[c],1,budget=58);self.assertEqual(len(selected),1);self.assertEqual(size,58)
        same=[g|{'regions':[1]}for g in groups];self.assertEqual(len(select_groups(same,[c],1,budget=100,max_region=1)[0]),1)
        self.assertEqual(select_groups(groups,[c],1,budget=57)[0],[])
    def test_native_seam_proof_and_guest_membership_guards(self):
        with tempfile.TemporaryDirectory()as d:
            for name,source in [('cook',ROOT/'host/coverage_groups.rs'),('guest',ROOT/'game/src/opaque_groups.rs')]:
                binary=Path(d)/name;subprocess.run(['rustc','--edition=2021','--test',str(source),'-o',str(binary)],check=True,capture_output=True);subprocess.run([str(binary)],check=True,capture_output=True)
if __name__=='__main__':unittest.main()
