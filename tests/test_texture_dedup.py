"""Texture merging preserves palette, metadata, geometry and storage class."""
import struct
import sys
import unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from region_delta import reconstruct, textures, layout
from texture_dedup import deduplicate_room


def room(palette_change=False):
    palette=struct.pack('<16H',0,*range(1,16))
    pixels=b'\x21\x03\x54\x06'
    blob=struct.pack('<HH',3,2)+palette+pixels
    changed=blob[:4]+struct.pack('<16H',0,99,*range(2,16))+pixels if palette_change else blob
    # Same texels at odd and even static atlas positions plus a streamed copy.
    header=b'HKROOM02'+struct.pack('<8I',1,3,2,1,1,1,4,0)
    entries=b''.join(struct.pack('<6HI',*e)for e in [(0,1,0,3,2,0,0),(0,6,0,3,2,1,0),(65535,0,0,3,2,2,0)])
    draws=struct.pack('<HHI8i3Bx',1,0,4096,*range(8),128,100,90)+struct.pack('<HHI8i3Bx',0,1,8192,*range(10,18),255,80,64)
    frame=struct.pack('<I4i',2,-100,-200,300,400)
    rest=struct.pack('<4I',0,1,65536,2)+struct.pack('<4i',0,0,65536,0)
    prefix=header+entries+draws+frame+rest
    return reconstruct(prefix,len(prefix)+3*32+32768+4,[blob,changed,blob])

class DedupTests(unittest.TestCase):
    def test_static_aliases_preserve_draw_geometry_and_stream_class(self):
        raw=room();packed,report=deduplicate_room(raw)
        self.assertEqual(report['old_to_canonical'],[0,0,1])
        self.assertEqual(layout(packed)[0],(1,2,2,1,1,1))
        old_draw=raw[40+3*16:40+3*16+88]
        new_draw=packed[40+2*16:40+2*16+88]
        for i in range(2):self.assertEqual(old_draw[i*44+2:(i+1)*44],new_draw[i*44+2:(i+1)*44])
        old_frame=raw[40+3*16+88:40+3*16+108]
        new_frame=packed[40+2*16+88:40+2*16+108]
        self.assertEqual(old_frame[4:],new_frame[4:])
        self.assertEqual(raw[layout(raw)[1]-32:layout(raw)[1]],packed[layout(packed)[1]-32:layout(packed)[1]])
        self.assertEqual(textures(packed),[textures(raw)[0],textures(raw)[2]])
        self.assertEqual(struct.unpack_from('<H',packed,40+16)[0],65535)

    def test_palette_difference_remains_separate(self):
        raw=room(True);packed,report=deduplicate_room(raw)
        self.assertEqual(report['old_to_canonical'],[0,1,2])
        self.assertEqual(textures(packed),textures(raw))

    def test_explicit_static_trial_replaces_only_selected_texture(self):
        raw=room(True);old=textures(raw)
        packed,report=deduplicate_room(raw,{1:old[0]})
        self.assertEqual(report['old_to_canonical'],[0,0,1])
        self.assertEqual(report['replaced_textures'],[1])
        self.assertEqual(textures(packed),[old[0],old[2]])
        oldat=40+3*16;newat=40+2*16
        for i in range(2):
            self.assertEqual(raw[oldat+i*44+2:oldat+(i+1)*44],packed[newat+i*44+2:newat+(i+1)*44])
        self.assertEqual(raw[oldat+88+4:oldat+108],packed[newat+88+4:newat+108])

    def test_static_trial_rejects_animation_and_bad_indices(self):
        raw=room(True);old=textures(raw)
        with self.assertRaises(ValueError):deduplicate_room(raw,{2:old[1]})
        with self.assertRaises(ValueError):deduplicate_room(raw,{3:old[0]})
        with self.assertRaises(ValueError):deduplicate_room(raw,{-1:old[0]})

    def test_repacking_is_idempotent(self):
        first,_=deduplicate_room(room());second,report=deduplicate_room(first)
        self.assertEqual(first,second)
        self.assertEqual(report['old_to_canonical'],[0,1])

if __name__=='__main__':unittest.main()
