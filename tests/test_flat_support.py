"""Static source qualification and exact whole-cell floor union admission."""
import hashlib
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from flat_support import full_floors, static_chain_allowed, generate, rust_catalog
Q=65536
class FlatSupport(unittest.TestCase):
    def proof(self,edges,excluded=set()):return full_floors([72,11,84,27],edges,excluded,16384)
    def test_exact_source_span_includes_body_margins(self):
        p=self.proof([((96*Q,10*Q,64*Q,10*Q),'terrain')])
        self.assertEqual(p[0]['required_interval'],[72*Q-16384,84*Q+16384]);self.assertEqual(p[0]['height'],10*Q)
        self.assertEqual(self.proof([((72*Q,10*Q,84*Q,10*Q),'terrain')]),[])
    def test_contiguous_union_but_no_one_unit_gap_step_or_slope(self):
        first=((64*Q,10*Q,80*Q,10*Q),'a')
        self.assertEqual(len(self.proof([first,((80*Q,10*Q,96*Q,10*Q),'b')])),1)
        for second in [(80*Q+1,10*Q,96*Q,10*Q),(80*Q,10*Q+1,96*Q,10*Q+1),(80*Q,10*Q,96*Q,10*Q+1)]:
            self.assertEqual(self.proof([first,(second,'b')]),[])
    def test_breakable_bridge_cannot_establish_support(self):
        edges=[((64*Q,10*Q,80*Q,10*Q),'a'),((80*Q,10*Q,96*Q,10*Q),'breakable')]
        self.assertEqual(self.proof(edges,{1}),[])
    def test_dynamic_or_disabled_owners_and_ancestors_are_not_static_terrain(self):
        owner={'active':True,'types':['Transform','MeshFilter','MeshRenderer','EdgeCollider2D']}
        parent={'active':True,'types':['Transform']}
        self.assertTrue(static_chain_allowed([owner,parent]))
        for typ in ['Rigidbody2D','PlayMakerFSM','Breakable','Animator']:
            self.assertFalse(static_chain_allowed([{**owner,'types':owner['types']+[typ]},parent]))
            self.assertFalse(static_chain_allowed([owner,{**parent,'types':['Transform',typ]}]))
        self.assertFalse(static_chain_allowed([owner,{**parent,'active':False}]))
        self.assertFalse(static_chain_allowed([{**owner,'active':False},parent]))
    def test_height_catalogue_fails_closed_instead_of_truncating(self):
        with self.assertRaisesRegex(ValueError,'bounded'):
            self.proof([((64*Q,h*Q,96*Q,h*Q),'terrain')for h in range(10,19)])
        with self.assertRaises(ValueError):rust_catalog([list(range(9))])
    def test_pack_hash_and_quantized_provenance_must_match(self):
        with tempfile.TemporaryDirectory()as tmp:
            root=Path(tmp);d=root/'data';(d/'regions/region-001').mkdir(parents=True)
            (d/'params.rs').write_text('PARAMS={half_width:16384,bottom:-91136};')
            b=bytearray(b'HKROOM02'+struct.pack('<6I',0,0,0,0,0,1)+b'\0'*8+struct.pack('<4i',64*Q,10*Q,96*Q,10*Q))
            (d/'one.hk').write_bytes(b);meta={'edges':[{'source':'level6:1','a':[64,10],'b':[96,11]}]}
            (d/'regions/region-001/scene.json').write_text(json.dumps(meta))
            r={'regions':[{'chunk_id':1,'path':'data/one.hk','sha256':'bad','activation_bounds':[72,11,84,27]}]}
            with self.assertRaisesRegex(ValueError,'hash'):generate(r,root,source=object())
            r['regions'][0]['sha256']=hashlib.sha256(b).hexdigest()
            with self.assertRaisesRegex(ValueError,'cooked/source'):generate(r,root,source=object())
if __name__=='__main__':unittest.main()
