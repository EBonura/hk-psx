import struct
import sys
import unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'));sys.path.insert(0,str(ROOT/'tests'))
from scene_bank import with_texture_attributes, build_scene,verify,resize_static,manifest,shared_planes,joint_refine,texel_words,compact_resident,bootstrap_atlases,texture_flags
from test_region_delta import room
from region_delta import textures,layout
from alpha_covers import record


def source_room(moved=False):
    raw=bytearray(room(moved=moved));blob=textures(raw)[0]
    struct.pack_into('<I',raw,36,1);struct.pack_into('<I',raw,52,12)
    raw+=record(*struct.unpack_from('<HH',blob),blob[4:36],blob[36:]);struct.pack_into('<I',raw,32,32)
    header=bytearray(raw[:40]);struct.pack_into('<I',header,16,1);struct.pack_into('<I',header,20,1);struct.pack_into('<I',header,24,1);struct.pack_into('<I',header,28,1)
    draw=struct.pack('<HHI8i4B',0,0,65536,*([0]*8),128,128,128,0)
    frame=struct.pack('<I4i',1,0,0,65536,65536)
    clip=struct.pack('<4I',0,1,60*65536,0);edge=struct.pack('<4i',0,0,65536,0)
    return bytes(header)+raw[40:72]+draw+frame+clip+edge+raw[72:]

class SceneBankTests(unittest.TestCase):
    def test_compact_scene_keeps_runtime_records_and_complete_atlas_chunks(self):
        raw,e=build_scene(0,[(1,source_room()),(9,source_room(True))])
        compact=compact_resident(raw);old=struct.unpack_from('<10I',raw,64);new=struct.unpack_from('<10I',compact,64)
        self.assertEqual(compact[:8],b'HKSCNE02');self.assertEqual(compact[8:48],raw[8:48])
        self.assertEqual(new[5],new[6]);self.assertEqual(new[6],new[7])
        self.assertEqual(len(raw)-len(compact),e['pages']*32768+e['palettes']*32)
        for i,size in enumerate((16,44,20,16,16)):
            n=e['sections'][('textures','draws','frames','clips','edges')[i]]['bytes']
            self.assertEqual(compact[new[i]:new[i]+n],raw[old[i]:old[i]+n])
        self.assertEqual(compact[new[7]:new[8]],raw[old[7]:old[8]])
        self.assertEqual(compact[new[9]:],raw[old[9]:])
        for i in range(2):
            self.assertEqual(compact[new[8]+i*40:new[8]+i*40+20],raw[old[8]+i*40:old[8]+i*40+20])
            for j in range(4):
                a=struct.unpack_from('<I',raw,old[8]+i*40+20+j*4)[0];b=struct.unpack_from('<I',compact,new[8]+i*40+20+j*4)[0]
                self.assertEqual(a-b,len(raw)-len(compact))
        chunks=bootstrap_atlases(raw,e)
        for kind,section in((0,'pages'),(1,'palettes')):
            selected=[(s,b)for s,b in chunks if s['kind']==kind];stride=32768 if kind==0 else 32
            self.assertEqual(b''.join(b for _,b in selected),raw[e['sections'][section]['offset']:e['sections'][section]['offset']+e['sections'][section]['bytes']])
            for s,b in selected:self.assertEqual(len(b),s['count']*stride);self.assertLessEqual(len(b),32768)
        with self.assertRaises(ValueError):compact_resident(compact)
        with self.assertRaises(ValueError):compact_resident(raw[:-1])
        # Attributes append after the reference arrays, one 12-byte record per texture.
        textures=struct.unpack_from('<I',compact,16)[0]
        black=[[i,0,4,4] for i in range(textures)];opaque=[[0,i,8,8] for i in range(textures)]
        flags=[0]*((textures+15)//16*4);flags[0]|=3
        with_attributes=with_texture_attributes(compact,flags,black,opaque)
        start=struct.unpack_from('<I',with_attributes,104)[0]
        self.assertEqual(start,(len(compact)+3)//4*4);self.assertEqual(len(with_attributes),struct.unpack_from('<I',with_attributes,48)[0])
        self.assertEqual(list(with_attributes[start:start+12]),[3,0,0,0,0,0,4,4,0,0,8,8])
        with self.assertRaises(ValueError):with_texture_attributes(with_attributes,flags,black,opaque)
        with self.assertRaises(ValueError):with_texture_attributes(compact,flags,black[:-1],opaque)

    def test_cooked_texture_flags_match_binary_palette_and_sampled_word(self):
        raw,e=build_scene(0,[(1,source_room())]);raw=bytearray(raw);s=e['sections']
        raw[s['pages']['offset']:s['pages']['offset']+s['pages']['bytes']]=bytes([0x11])*s['pages']['bytes']
        for i in range(e['palettes']):struct.pack_into('<16H',raw,s['palettes']['offset']+i*32,0,1,*([0x8000]*14))
        for word,want in((1,3),(2,0),(0x8001,0),(0,0)):
            for i in range(e['palettes']):struct.pack_into('<H',raw,s['palettes']['offset']+i*32+2,word)
            flags=texture_flags(raw,e)
            for i in range(e['textures']):
                streamed=struct.unpack_from('<H',raw,s['textures']['offset']+i*16)[0]==65535
                self.assertEqual((flags[i//4]>>((i%4)*2))&3,0 if streamed else want)

    def test_moved_atlas_and_palette_orders_keep_all_local_record_semantics(self):
        inputs=[(1,source_room()),(9,source_room(True))]
        payload,report=build_scene(0,inputs)
        self.assertTrue(verify(payload,inputs));self.assertEqual(report['textures'],2)
        self.assertEqual(report['pools'],{'draws':1,'frames':1,'clips':1,'edges':1})
        self.assertEqual(report['palettes'],2)
    def test_record_reference_and_unused_texel_corruption_is_rejected(self):
        inputs=[(1,source_room())];payload,report=build_scene(0,inputs)
        damaged=bytearray(payload);at=report['sections']['refs']['offset'];struct.pack_into('<H',damaged,at,65535)
        with self.assertRaises(ValueError):verify(damaged,inputs)
        damaged=bytearray(payload);at=report['sections']['pages']['offset'];damaged[at]^=1
        with self.assertRaises(ValueError):verify(damaged,inputs)
    def test_word_exact_alias_ignores_unused_palette_but_preserves_sampled_words(self):
        first=source_room();second=bytearray(first);prefix=layout(first)[1]
        palette=struct.unpack_from('<H',first,50)[0]
        second[prefix+palette*32+30]^=0x10
        inputs=[(1,first),(2,bytes(second))]
        old,old_report=build_scene(0,inputs)
        new,new_report=build_scene(0,inputs,dedup_mode='texel_words')
        self.assertEqual(old_report['textures'],3);self.assertEqual(new_report['textures'],2)
        self.assertTrue(verify(new,inputs,dedup_mode='texel_words'))
        # Change a sampled palette word: it must no longer alias.
        second=bytearray(first);second[prefix+palette*32+2]^=0x10
        _,report=build_scene(0,[(1,first),(2,bytes(second))],dedup_mode='texel_words')
        self.assertEqual(report['textures'],3)

    def test_atlas_and_header_limits_are_explicit(self):
        with self.assertRaises(ValueError):build_scene(0,[(1,source_room())],page_limit=0)
        payload,_=build_scene(0,[(1,source_room())]);bad=bytearray(payload);bad[104]=1
        with self.assertRaises(ValueError):verify(bad,[(1,source_room())])
    def test_nearest_changes_only_static48_axis_and_preserves_palette_and_masks(self):
        palette=struct.pack('<16H',*range(16));blob=struct.pack('<HH',48,1)+palette+bytes([0x21]*24)
        out=resize_static(blob,46)
        self.assertEqual(struct.unpack_from('<HH',out),(46,1));self.assertEqual(out[4:36],palette)
        self.assertEqual(resize_static(blob,48),blob)
        black=blob[:4]+struct.pack('<16H',0,1,*([0x8000]*14))+blob[36:]
        self.assertEqual(resize_static(black,44),black)
        small=struct.pack('<HH',20,1)+palette+bytes(10);self.assertEqual(resize_static(small,44),small)
    def test_joint_plane_refinement_preserves_colour_merging_and_nonzero_index_zero(self):
        def blob(indices,palette):return b'\0'+struct.pack('<HH',4,1)+struct.pack('<16H',*(palette+[0]*(16-len(palette))))+bytes([indices[0]|indices[1]<<4,indices[2]|indices[3]<<4])
        # A has two colour groups, B has three crossing A's partition. Palette
        # indexzero must be allowed to contain a nontransparent word.
        a=blob([1,1,2,2],[0,31,0x8000]);b=blob([1,2,2,3],[0,992,0,31744])
        planes,mapping,proof=shared_planes([a,b],True)
        self.assertEqual(len(planes),1);self.assertEqual(len(proof),1)
        w,h,pixels=planes[0]
        for i,source in enumerate([a,b]):
            _,palette=mapping[i]
            rebuilt=struct.pack('<HH',w,h)+palette+pixels
            self.assertEqual(texel_words(rebuilt),texel_words(source[1:]))
        self.assertNotEqual(struct.unpack_from('<H',mapping[0][1])[0],0)
    def test_joint_refinement_rejects_seventeenth_state_and_leaves_masks_exact(self):
        self.assertIsNone(joint_refine(bytes(range(16))+bytes([0]),list(range(16))+[17]))
        black=struct.pack('<HH',4,1)+struct.pack('<16H',0,1,*([0x8000]*14))+bytes([0x21,0x10])
        _,mapping,proof=shared_planes([b'\0'+black,b'\0'+black],True)
        self.assertEqual(proof,[]);self.assertNotEqual(mapping[0][0],mapping[1][0])
        self.assertEqual(mapping[0][1],black[4:36])

    def test_shared_plane_verifier_keeps_mask_and_stream_unused_clut_bytes_exact(self):
        raw=bytearray(source_room());prefix=layout(raw)[1];palette=struct.unpack_from('<H',raw,50)[0]
        raw[prefix+palette*32:prefix+(palette+1)*32]=struct.pack('<16H',0,1,*([0x8000]*14))
        inputs=[(1,bytes(raw))];bank,report=build_scene(0,inputs,share_pixel_planes=True)
        for texture in (0,1): # Static strict-mask and streamed animation.
            damaged=bytearray(bank);ta=report['sections']['textures']['offset']+texture*16
            clut=struct.unpack_from('<H',damaged,ta+10)[0]
            damaged[report['sections']['palettes']['offset']+clut*32+30]^=0x10
            with self.assertRaises(ValueError):verify(damaged,inputs,share_pixel_planes=True)

    def test_global_to_scene_local_mapping_handles_noncontiguous_scene_regions(self):
        result={'scenes':[{'scene_id':0,'stored_bytes':10,'stored_fnv':1,'bytes':20,'raw_fnv':2,'source_rooms':[{'chunk_id':1},{'chunk_id':3}]},
          {'scene_id':1,'stored_bytes':11,'stored_fnv':3,'bytes':24,'raw_fnv':4,'source_rooms':[{'chunk_id':2}]}]}
        source={'regions':[{'chunk_id':1},{'chunk_id':2},{'chunk_id':3}]}
        self.assertIn('(0,0),\n(1,0),\n(0,1),',manifest(result,source))

if __name__=='__main__':unittest.main()


def tiled_room():
    """Two streamed tiles of one 2x1 frame (grid on the first) and a lone streamed
    texture byte-identical to the second tile; one frame names the tile run."""
    palette=struct.pack('<16H',0,*range(1,16));tile=lambda v:bytes([v|v<<4])*(32*64)
    blobs=[(64,64,palette,tile(3)),(64,64,palette,tile(5)),(64,64,palette,tile(5))]
    records=b'';stream=b''
    for i,(w,h,p,pixels) in enumerate(blobs):
        grid=(2,1) if i==0 else (0,0)
        records+=struct.pack('<6HI',65535,*grid,w,h,i,len(stream));stream+=pixels
    frame=struct.pack('<I4i',0,0,0,65536,65536);clip=struct.pack('<4I',0,1,60*65536,0)
    header=b'HKROOM02'+struct.pack('<6I',0,3,0,1,1,0)+struct.pack('<2I',len(stream),0)
    return header+records+frame+clip+b''.join(p for _,_,p,_ in blobs)+stream


class TileGridTests(unittest.TestCase):
    def test_tiled_frames_keep_their_grid_and_a_consecutive_run(self):
        raw,e=build_scene(0,[(1,tiled_room()),(2,tiled_room())])
        self.assertTrue(verify(raw,[(1,tiled_room()),(2,tiled_room())]))
        off=struct.unpack_from('<10I',raw,64);nt=struct.unpack_from('<I',raw,16)[0]
        head=struct.unpack_from('<I',raw,off[2])[0]
        page,u,v=struct.unpack_from('<3H',raw,off[0]+head*16)
        self.assertEqual((page,u,v),(65535,2,1))
        self.assertEqual(struct.unpack_from('<3H',raw,off[0]+(head+1)*16),(65535,0,0))
        # The run is stored once for both rooms, and the lone twin of tile 1 is a
        # record of its own rather than an alias into the run.
        self.assertEqual(nt,3)
        self.assertEqual([m for m in (r['texture_map'] for r in e['source_rooms'])],[[0,1,2],[0,1,2]])
