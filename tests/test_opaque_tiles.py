"""Source-free guards for host opaque tile certificates and phase admission."""
import random,struct,subprocess,sys,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'host'))
from opaque_tiles import mutable_draws,phase_variants,legal_source,read_certificates,write_changed

class OpaqueTiles(unittest.TestCase):
    def test_all_mutation_bindings_are_excluded_including_remote_and_reveal(self):
        row={'draws':12,'grass':[{'off_draw':0,'on_draw':1}],
             'breakables':[{'off_draws':[2],'on_draws':[3],'mask_fades':[{'draw_indices':[4]}]}],
             'remote_mask_bindings':[{'fade':{'draw_indices':[5]}}],
             'reveal_mask_bindings':[{'draw':6}]}
        geo={'bindings':[[{'off':[7]}]]};life={'bindings':[{'off':[8]}]}
        self.assertEqual(mutable_draws({'regions':[row]},geo,life),[set(range(9))])
        row['reveal_mask_bindings'][0]['draw']=12
        with self.assertRaises(ValueError):mutable_draws({'regions':[row]},geo,life)

    def test_phase_set_contains_projection_for_negative_coordinates_and_parallax(self):
        rng=random.Random(6142)
        for _ in range(30):
            xy=[rng.randrange(-200000,200000)for _ in range(8)]
            actual={tuple(v)for v in phase_variants(xy)}
            self.assertLessEqual(len(actual),16)
            for _ in range(200):
                camera=[rng.randrange(-10000000,10000000)for _ in range(2)];scale=rng.randrange(1,200000)
                cx,cy=[(c*scale)>>12 for c in camera]
                v=[(((xy[i]-cx)>>8),-((xy[i+1]-cy)>>8))for i in range(0,8,2)]
                relative=tuple((x-v[0][0],y-v[0][1])for x,y in v)
                self.assertIn(relative,actual)

    def test_legal_extent_rejects_oversized_originals(self):
        self.assertTrue(legal_source([0,0,703*256,0,0,511*256,703*256,511*256]))
        self.assertFalse(legal_source([0,0,703*256+1,0,0,511*256,703*256+1,511*256]))

    def test_output_rejects_padding_and_empty_payload_aliases(self):
        header=b'HKTCOT01'+struct.pack('<I',1)
        valid=header+struct.pack('<IhhHHII',0,-2,3,1,1,1,1)
        self.assertEqual(read_certificates(valid,1)[0]['words'],(1,))
        for payload in [valid+b'x',valid[:-4]+struct.pack('<I',2),valid[:-4]+struct.pack('<I',0)]:
            with self.assertRaises(ValueError):read_certificates(payload,1)

    def test_identical_generation_preserves_mtime(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'generated.rs';self.assertTrue(write_changed(p,'abc'));stamp=p.stat().st_mtime_ns
            self.assertFalse(write_changed(p,'abc'));self.assertEqual(stamp,p.stat().st_mtime_ns)

    def test_native_opaque_soft_and_phase_intersection(self):
        with tempfile.TemporaryDirectory()as d:
            binary=Path(d)/'coverage-tests'
            subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'host/coverage_cert.rs'),'-o',str(binary)],check=True,capture_output=True)
            subprocess.run([str(binary)],check=True,capture_output=True)

if __name__=='__main__':unittest.main()
