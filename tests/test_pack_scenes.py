"""Resident scene allocation and O(1) region mapping contracts, source-free."""
import hashlib,json,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from pack_scenes import allocate_scenes,manifest,decoder_command,pack_scenes,validate_geometry

def entry(scene,size,pages,palettes,chunks):
    return dict(scene_id=scene,bytes=size,stored_bytes=size//2,pages=pages,palettes=palettes,
                stored_fnv=13,raw_fnv=17,stored_path=f'scene{scene}.hlzc',raw_path=f'scene{scene}.hk',source_rooms=[{'chunk_id':c}for c in chunks])
class PackScenes(unittest.TestCase):
    def test_largest_scene_uses_full_arena_then_immutable_aligned_prefix(self):
        out=allocate_scenes([entry(1,116756,2,141,[2]),entry(0,929692,18,1065,[1,3])])
        self.assertEqual([e['scene_id']for e in out],[0,1])
        self.assertEqual([(e['ram_offset'],e['arena_capacity'])for e in out],[(0,1179648),(929692,249956)])
        self.assertEqual([(e['page_base'],e['palette_base'])for e in out],[(0,0),(18,1065)])
        self.assertEqual([e['chunk_id']for e in out],[1,2])
        odd=allocate_scenes([entry(0,1001,1,1,[1]),entry(1,8,1,1,[2])],arena_bytes=1024)
        self.assertEqual(odd[1]['ram_offset'],1004)
    def test_aggregate_limits_reject_individually_valid_banks(self):
        for a,b,arena in [(entry(0,600,10,10,[1]),entry(1,600,10,10,[2]),1000),
                          (entry(0,400,11,10,[1]),entry(1,400,10,10,[2]),1000),
                          (entry(0,400,10,900,[1]),entry(1,400,10,800,[2]),1000)]:
            with self.assertRaises(ValueError):allocate_scenes([a,b],arena)
        with self.assertRaises(ValueError):allocate_scenes([])
        with self.assertRaises(ValueError):allocate_scenes([entry(0,100,1,1,[1])]*2)
    def test_region_mapping_uses_source_local_order_not_contiguous_scene_assumption(self):
        scenes=allocate_scenes([entry(1,400,1,1,[2]),entry(0,800,2,3,[1,3])])
        r={'scenes':scenes,'arena_bytes':1179648,'raw_resident_bytes':1200,'total_pages':3,'total_palettes':4}
        m={'regions':[{'chunk_id':1,'scene_id':0},{'chunk_id':2,'scene_id':1},{'chunk_id':3,'scene_id':0}]}
        text=manifest(r,m);self.assertIn('(0,0),\n(1,0),\n(0,1),',text);self.assertIn('SCENE_REGIONS:&[usize]=&[0,1]',text)
        command=decoder_command(r);self.assertEqual(command[-5:],['1179648','scene0.hlzc','scene0.hk','scene1.hlzc','scene1.hk'])
        m['regions'][1]['scene_id']=0
        with self.assertRaises(ValueError):manifest(r,m)
    def test_serialized_scene_draws_receive_packet_bound_checks(self):
        sys.path.insert(0,str(Path(__file__).resolve().parent))
        from test_scene_bank import source_room
        from scene_bank import build_scene
        bank,entry=build_scene(0,[(1,source_room())],share_pixel_planes=True)
        metadata={'regions':[{'chunk_id':1,'camera_bounds':[0,0,1,1]}]}
        proof=validate_geometry(bank,entry,metadata)
        self.assertEqual(proof['regions'][0]['mandatory_packets'],1)
        # Serialize a too-large quad directly into the otherwise valid Scene.
        import struct
        damaged=bytearray(bank);at=entry['sections']['draws']['offset']
        struct.pack_into('<8i',damaged,at+8,0,0,20000*256,0,0,20000*256,20000*256,20000*256)
        with self.assertRaises(ValueError):validate_geometry(damaged,entry,metadata)
    def test_stale_source_rejected_before_output_publication(self):
        with tempfile.TemporaryDirectory()as d:
            d=Path(d);src=d/'room.hk';src.write_bytes(b'stale')
            meta=d/'regions.json';meta.write_text(json.dumps({'complete':True,'regions':[{'scene_id':0,'chunk_id':1,'path':str(src),'sha256':hashlib.sha256(b'expected').hexdigest()}]}))
            with patch('world.generate'):
                with self.assertRaises(ValueError):pack_scenes(meta,d/'banks',d/'report.json',d/'manifest.rs')
            self.assertFalse((d/'banks').exists());self.assertFalse((d/'manifest.rs').exists())
if __name__=='__main__':unittest.main()
