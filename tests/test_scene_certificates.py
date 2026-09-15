"""Source-free exact proof relocation and hostile metadata admission tests."""
import copy
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from scene_certificates import (ARENA_BYTES,NO_CERT,atlas_fingerprint,build_bundle,
    parse_bundle,verify_equivalence,verify_region_bindings,read_legacy,rust_manifest,cook)


def fixtures():
    certs=[dict(gx=-3,gy=4,width=2,height=2,offset=0),dict(gx=-2,gy=1,width=3,height=2,offset=32),
           dict(gx=7,gy=-9,width=1,height=1,offset=64)]
    tile=dict(certificates=certs,bits=[0b1011,0b101101,1],map=[2,NO_CERT,1,2,1])
    groups=dict(certificates=copy.deepcopy(certs[:2]),bits=[0b1011,0b101101],groups=[
        dict(certificate=1,members=[0,1,NO_CERT,NO_CERT]),
        dict(certificate=0,members=[0,1,3,NO_CERT])])
    return tile,groups

def blob():
    tile,group=fixtures();return build_bundle(3,123,456,5,tile,group)[0]

class SceneCertificates(unittest.TestCase):
    def test_relocation_keeps_shared_pool_references_and_short_prefix_group_order(self):
        tile,group=fixtures();before=copy.deepcopy((tile,group))
        raw,proof=build_bundle(3,123,456,5,tile,group);parsed=parse_bundle(raw)
        self.assertEqual((tile,group),before)
        self.assertEqual(raw,build_bundle(3,123,456,5,tile,group)[0])
        self.assertEqual(parsed['tile']['map'],[1,NO_CERT,0,1,0])
        self.assertEqual(len(parsed['tile']['certificates']),2)
        self.assertEqual([c['offset']for c in parsed['tile']['certificates']],[0,32])
        self.assertEqual(parsed['tile']['bits'],[0b101101,1])
        self.assertEqual(parsed['group']['groups'],group['groups'])
        self.assertEqual((parsed['scene_id'],parsed['scene_raw_fnv'],parsed['atlas_fnv'],parsed['pool_count']),(3,123,456,5))
        self.assertEqual(proof['status'],'PASS');self.assertEqual(proof['pool_bindings'],5)
        self.assertLessEqual(len(raw),ARENA_BYTES)

    def test_empty_certificates_and_groups_still_preserve_complete_pool_map(self):
        empty=dict(certificates=[],bits=[])
        raw,_=build_bundle(0,1,2,4,dict(empty,map=[NO_CERT]*4),dict(empty,groups=[]))
        parsed=parse_bundle(raw)
        self.assertEqual(parsed['tile']['map'],[NO_CERT]*4)
        self.assertEqual(parsed['group']['groups'],[])

    def test_atlas_fingerprint_has_independent_little_endian_ordered_contract(self):
        atlases=[dict(kind=0,first=2,count=1,raw_len=32768,raw_fnv=0xabcdef01),dict(kind=1,first=9,count=3,raw_len=96,raw_fnv=0x98765432)]
        expected=2166136261
        for byte in bytes.fromhex('0000000002000000010000000080000001efcdab0100000009000000030000006000000032547698'):
            expected=((expected^byte)*16777619)&0xffffffff
        self.assertEqual(atlas_fingerprint(atlases),expected)
        self.assertNotEqual(atlas_fingerprint(atlases),atlas_fingerprint(atlases[::-1]))

    def test_rejects_malformed_sections_configuration_and_padding(self):
        original=blob();cases=[original[:79],original+b'\0'*4,b'BADMAGIC'+original[8:]]
        for offset,value in [(20,6),(24,3),(28,1),(32,84),(36,0xffffffff),(40,80),(44,0xffffffff)]:
            bad=bytearray(original);struct.pack_into('<I',bad,offset,value);cases.append(bad)
        # Five u16 pool entries leave two alignment bytes before group certs.
        off,count=struct.unpack_from('<II',original,48)
        bad=bytearray(original);bad[off+count*2]=1;cases.append(bad)
        for bad in cases:
            with self.subTest(size=len(bad)),self.assertRaises(ValueError):parse_bundle(bad)

    def test_rejects_out_of_bounds_ids_bitmap_words_and_group_bindings(self):
        original=blob();tc,tb,mp,gc,gb,gr=[struct.unpack_from('<I',original,32+i*8)[0]for i in range(6)]
        mutations=[('<I',tc+8,1),('<H',tc+4,0),('<I',tb,0),('<I',tb,0xffffffff),('<H',mp,99),
                   ('<H',gr,99),('<H',gr+2,1),('<H',gr+4,4),('<H',gr+6,0),('<H',gr+4,0),
                   ('<H',gr+12,1),('<H',gr+14,0),('<H',gr+16,NO_CERT)]
        for fmt,offset,value in mutations:
            bad=bytearray(original);struct.pack_into(fmt,bad,offset,value)
            with self.subTest(offset=offset,value=value),self.assertRaises(ValueError):parse_bundle(bad)

    def test_exhaustive_comparison_rejects_valid_but_different_bits_and_eligibility(self):
        raw=blob();tile,group=fixtures()
        tilebits=struct.unpack_from('<I',raw,40)[0];poolmap=struct.unpack_from('<I',raw,48)[0]
        for offset,fmt,value in [(tilebits,'<I',0b101111),(poolmap,'<H',NO_CERT)]:
            bad=bytearray(raw);struct.pack_into(fmt,bad,offset,value);parse_bundle(bad)
            with self.assertRaises(ValueError):verify_equivalence(bad,tile,group)

    def test_regional_first_occurrence_mapping_is_exact(self):
        raw=blob();tile,group=fixtures();bank=bytearray(50)
        struct.pack_into('<10I',bank,0,1,5,0,0,0,40,0,0,0,0)
        struct.pack_into('<5H',bank,40,3,1,0,3,4)
        scene=dict(sections={'rooms':{'offset':0}},source_rooms=[dict(chunk_id=1,counts=[0,0,5])])
        expected=[[[0,2],[2,2],[3,2],[4,1]]]
        proof=verify_region_bindings(raw,scene,bank,tile,group,expected)
        self.assertEqual(proof,[dict(chunk_id=1,local_draws=5,certified_draws=4,resolved_groups=2)])
        wrong=copy.deepcopy(expected);wrong[0][0][1]=1
        with self.assertRaises(ValueError):verify_region_bindings(raw,scene,bank,tile,group,wrong)

    def test_format_capacity_is_fixed_and_does_not_truncate_proofs(self):
        tile,group=fixtures();tile['certificates'][1].update(width=1024,height=1024,offset=0)
        tile['bits']=[0xffffffff]*32768;tile['map']=[1]*5
        with self.assertRaisesRegex(ValueError,'arena'):build_bundle(3,1,2,5,tile,dict(certificates=[],bits=[],groups=[]))
        tile,group=fixtures();tile['map'].pop()
        with self.assertRaises(ValueError):build_bundle(3,1,2,5,tile,group)

    def test_manifest_carries_predecode_inventory_and_pack_indices(self):
        entry=dict(scene_id=1,scene_raw_fnv=2,atlas_fnv=3,draw_pool_count=17,chunk_id=37,raw_len=88,raw_fnv=4,stored_len=55,stored_fnv=6)
        text=rust_manifest([entry],17)
        self.assertIn('COVERAGE_ARENA_BYTES:usize=83700;',text)
        self.assertIn('COVERAGE_GROUP_POOL_CAPACITY:usize=17;',text)
        self.assertIn('scene_id:1,scene_raw_fnv:2,atlas_fnv:3,draw_pool_count:17,chunk_id:37',text)

    def test_generated_rust_is_checked_against_report_not_trusted_as_arbitrary_code(self):
        report=dict(certificates=[dict(gx=-2,gy=1,width=3,height=2,offset=0)],bitmap_bytes=4,grid_shift=2,bank_bindings=[[0,NO_CERT]])
        text='''pub const TILE_CERT_SHIFT:u32=2;
pub static TILE_CERTS:&[TileCert]=&[
TileCert{gx:-2,gy:1,width:3,height:2,offset:0},
];
pub static TILE_CERT_BITS:&[u32]=&[
0x0000002d,
];
pub static SCENE_DRAW_CERTS:&[&[u16]]=&[
&[0,65535],
];'''
        self.assertEqual(read_legacy(report,text)['bits'],[45])
        for bad in [text.replace('width:3','width:4'),text.replace('&[0,65535]','&[1,65535]'),text.replace('0x0000002d','include_bytes!("unknown")')]:
            with self.assertRaises(ValueError):read_legacy(report,bad)

    def test_build_rejects_joint_before_any_cook_or_publication(self):
        import build_guest
        with patch.object(build_guest,'scene_manifest')as pack:
            with self.assertRaisesRegex(ValueError,'scene_gate'):build_guest.build(residency='joint')
            pack.assert_not_called()

    def test_stale_source_cannot_publish_bundle_or_manifest(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);(root/'.hkpsx/opaque-tiles').mkdir(parents=True);(root/'.hkpsx/opaque-groups').mkdir()
            reports={'.hkpsx/packed-scenes.json':dict(residency='scene_gate'),
                     '.hkpsx/opaque-tiles/report.json':dict(input_sha256={'data/stale.rs':'bad'},outputs={}),
                     '.hkpsx/opaque-groups/report.json':{},'.hkpsx/ambience.json':{}}
            for path,value in reports.items():(root/path).write_text(json.dumps(value))
            (root/'data').mkdir();(root/'data/stale.rs').write_text('changed')
            with self.assertRaisesRegex(ValueError,'Stale'):cook(root)
            self.assertFalse((root/'data/scene-coverage').exists());self.assertFalse((root/'data/scene_coverage_manifest.rs').exists())

if __name__=='__main__':unittest.main()
