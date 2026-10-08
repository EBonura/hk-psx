import copy
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'host'))
from similarity_dedup import postpack_similarity,sha,compose_row,select_replacements,verify_records,packet_protected,reservation_guard,draw_packets
from region_delta import reconstruct,textures,layout
from test_texture_match_groups import tex


def room(colour):
    prefix=bytearray(b'HKROOM02'+struct.pack('<8I',1,3,2,1,0,1,60,1))
    for i in range(3):prefix+=struct.pack('<6HI',0,i*2,0,2,2,i,i*20)
    for texture in (0,2):prefix+=struct.pack('<HHI8i4B',texture,0,65536,*([0]*8),128,128,128,0)
    prefix+=struct.pack('<I4i',1,0,0,65536,65536);prefix+=struct.pack('<4i',0,0,65536,0)
    blobs=[struct.pack('<HH16H',2,2,0,c,*([0]*14))+bytes([0x11,0x11])for c in (colour,colour-2,colour-4)]
    return reconstruct(prefix,len(prefix)+3*32+32768+60,blobs)


class SimilarityDedupTests(unittest.TestCase):
    def test_complete_postpass_changes_only_unprotected_scenery_and_is_idempotent(self):
        with tempfile.TemporaryDirectory()as folder:
            root=Path(folder);rows=[];before={}
            for chunk,colour in ((1,31),(2,30)):
                raw=room(colour);path=f'data/regions/chunk_{chunk}.hk';(root/path).parent.mkdir(parents=True,exist_ok=True);(root/path).write_bytes(raw);before[chunk]=raw
                rows.append({'chunk_id':chunk,'scene_id':0,'path':path,'sha256':sha(raw),'bytes':len(raw),'textures':3,'draws':2,
                    'texture_request_to_canonical':[0,1,2,0],'texture_deduplication':{'old_to_canonical':[0,1,2],'textures_before':3},
                    'reveal_mask_bindings':[{'draw':1}],'breakables':[]})
            report={'complete':True,'initial_chunk_id':1,'regions':rows}
            self.assertTrue(postpack_similarity(report,root));self.assertEqual(report['similarity_dedup_policy']['removed_global_textures'],1)
            for row in report['regions']:
                raw=(root/row['path']).read_bytes();mp=row['similarity_deduplication']['input_to_final']
                self.assertEqual(textures(raw)[mp[1]],textures(before[row['chunk_id']])[1])
                self.assertEqual(textures(raw)[mp[2]],textures(before[row['chunk_id']])[2])
                verify_records(before[row['chunk_id']],raw,mp,{1,2})
            self.assertEqual((root/'data/room.hk').read_bytes(),(root/rows[0]['path']).read_bytes())
            snapshot=copy.deepcopy(report);self.assertFalse(postpack_similarity(report,root));self.assertEqual(report,snapshot)
            proof=json.loads((root/'.hkpsx/similarity-dedup.json').read_text());self.assertEqual(len(proof['mapping']),1)
    def test_composes_actor_base_map_before_original_request_map(self):
        raw=room(31);dedup={'old_to_canonical':[2,0,1],'replaced_textures':[],'alpha_cover_bytes':60,'animation_bytes':0}
        row={'texture_deduplication':{'old_to_canonical':[1,2,0,1],'textures_before':4},'texture_request_to_canonical':[2,0,3]}
        final=compose_row(row,raw,raw,dedup)
        self.assertEqual(final['texture_deduplication']['old_to_canonical'],[0,1,2,0])
        self.assertEqual(final['texture_request_to_canonical'],[2,0,0])
    def test_protected_representative_is_excluded_and_no_similarity_chain(self):
        items=[tex(i,[[31-i]])for i in range(4)]
        mapping,matches,groups=select_replacements(items,{1})
        self.assertNotIn(1,mapping);self.assertNotIn(1,mapping.values())
        self.assertFalse(set(mapping)&set(mapping.values()))
        for pair in matches['candidates']:self.assertTrue(pair['a']in mapping or pair['b']in mapping)
    def test_geometry_and_protected_frame_changes_are_detected(self):
        raw=room(31);bad=bytearray(raw);bad[40+3*16+8]^=1
        with self.assertRaises(ValueError):verify_records(raw,bad,[0,1,2],{1,2})
        c,p,ps,ss=layout(raw);bad=bytearray(raw);bad[p+32+2]^=1
        with self.assertRaises(ValueError):verify_records(raw,bad,[0,1,2],{1,2})
    def test_constant_mask_collapse_is_verified_and_frame_identity_is_global(self):
        from constant_textures import STRICT_MASK_PALETTE
        def masked(raw,frame_mask=False):
            c,p,_,_=layout(raw);blobs=textures(raw)
            blobs[0]=blobs[2]=struct.pack('<HH',2,2)+STRICT_MASK_PALETTE+b'\x11\x11'
            prefix=bytearray(raw[:p])
            if frame_mask:struct.pack_into('<I',prefix,40+c[1]*16+c[2]*44,2)
            return reconstruct(prefix,len(raw),blobs)
        for shared_frame in (False,True):
            with self.subTest(shared_frame=shared_frame),tempfile.TemporaryDirectory()as folder:
                root=Path(folder);rows=[];original={}
                for chunk in (1,2):
                    raw=masked(room(31),shared_frame and chunk==2)
                    path=f'data/chunk{chunk}.hk';(root/path).parent.mkdir(exist_ok=True);(root/path).write_bytes(raw)
                    original[chunk]=raw
                    rows.append(dict(chunk_id=chunk,path=path,sha256=sha(raw),reveal_mask_bindings=[{'draw':1}]))
                report=dict(complete=True,regions=rows)
                postpack_similarity(report,root)
                for row in report['regions']:
                    record=row['similarity_deduplication'];raw=(root/row['path']).read_bytes();mp=record['input_to_final']
                    self.assertEqual(record['lossless_constant_indices'],[]if shared_frame else[0,2])
                    expected=textures(original[row['chunk_id']])[2]if shared_frame else struct.pack('<HH',4,4)+STRICT_MASK_PALETTE+b'\x11'*8
                    self.assertEqual(textures(raw)[mp[2]],expected)
                    verify_records(original[row['chunk_id']],raw,mp,{1,2},record['lossless_constant_indices'])
    def test_constant_exemption_cannot_hide_nonconstant_or_frame_changes(self):
        raw=room(31)
        with self.assertRaises(ValueError):verify_records(raw,raw,[0,1,2],{2},{2})
        with self.assertRaises(ValueError):verify_records(raw,raw,[0,1,2],{1},{1})

    def test_smaller_representative_cannot_break_a_large_draw(self):
        raw=room(31);giant=bytearray(raw);struct.pack_into('<8i',giant,40+3*16+8,0,0,331*256,0,0,2727*256,331*256,2727*256)
        items=[{'id':0,'w':2,'h':2,'references':[[1,0]]},{'id':1,'w':4,'h':4,'references':[[2,0]]}]
        self.assertEqual(packet_protected({0:1},items,{1:bytes(giant)}),[0])
        self.assertEqual(packet_protected({0:1},items,{1:raw}),[])

    def test_reservation_guard_drops_replacements_that_overflow_a_region(self):
        def minimal(texture_size,draws,side):
            data=bytearray(b'HKROOM02'+struct.pack('<6I',0,1,draws,0,0,0)+struct.pack('<2I',0,0))
            data+=struct.pack('<6HI',0,0,0,texture_size,texture_size,0,0)
            for _ in range(draws):data+=struct.pack('<Hh',0,0)+struct.pack('<i',65536)+struct.pack('<8i',0,0,side,0,0,side,side,side)+struct.pack('<I',0)
            return bytes(data)+bytes(32)
        big=minimal(64,50,1000*256);small=minimal(64,50,100*256)
        self.assertGreater(draw_packets(big,{0:(8,8)}),768);self.assertLessEqual(draw_packets(big),768)
        items=[{'id':0,'w':64,'h':64},{'id':1,'w':8,'h':8}]
        rows=[{'chunk_id':1}];local={1:[0]}
        mapping={0:1};constants={1:{}}
        self.assertEqual(reservation_guard(rows,{1:big},local,items,mapping,constants),[{'chunk_id':1,'texture':0,'global_id':0}])
        self.assertEqual(mapping,{})
        mapping={0:1};self.assertEqual(reservation_guard(rows,{1:small},local,items,mapping,{1:{}}),[]);self.assertEqual(mapping,{0:1})

    def test_partial_cook_is_not_published(self):
        self.assertFalse(postpack_similarity({'complete':False}))

if __name__=='__main__':unittest.main()
