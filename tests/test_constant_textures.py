"""Strict lossless constant-domain admission, remapping and rejection tests."""
import struct,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from cook import Atlas
from constant_textures import STRICT_MASK_PALETTE,constant_texture_replacements,verify_constant_replacement,verify_constant_room
from texture_dedup import deduplicate_room
from region_delta import textures


def room(frame_id=1):
    atlas=Atlas()
    atlas.add_quantized(7,3,STRICT_MASK_PALETTE,bytes([0x11,0x11,0x11,1])*3)
    atlas.add_quantized(8,2,STRICT_MASK_PALETTE,bytes([0x11])*8)
    atlas.add_quantized(8,2,STRICT_MASK_PALETTE,bytes([0x11])*8,True)
    atlas.pack();out=bytearray(b'HKROOM02'+struct.pack('<8I',len(atlas.pages),3,1,1,1,1,len(atlas.stream),atlas.flags))
    for e in atlas.entries:out.extend(struct.pack('<6HI',*e))
    out.extend(struct.pack('<HHI8i4B',0,0,1234,0,0,100,20,-30,100,70,120,128,128,128,1))
    out.extend(struct.pack('<I4i',frame_id,1,2,3,4));out.extend(struct.pack('<4I',0,1,65536,1));out.extend(struct.pack('<4i',1,2,3,4))
    return bytes(out)+b''.join(atlas.palettes)+b''.join(atlas.pages)+atlas.stream

class ConstantTextures(unittest.TestCase):
    def test_only_static_nonframe_constants_and_global_guards(self):
        raw=room();r=constant_texture_replacements(raw)
        self.assertEqual(set(r),{0});self.assertEqual(r[0],struct.pack('<HH',4,4)+STRICT_MASK_PALETTE+bytes([0x11])*8)
        self.assertEqual(constant_texture_replacements(raw,{0}),{})
        with self.assertRaises(ValueError):constant_texture_replacements(raw,{3})
        with self.assertRaises(ValueError):constant_texture_replacements(room(3))
    def test_every_sample_and_original_palette_are_required(self):
        original=textures(room())[0];target=struct.pack('<HH',4,4)+STRICT_MASK_PALETTE+bytes([0x11])*8
        verify_constant_replacement(original,target)
        for at,value in [(36,0x10),(36,0x21),(len(original)-1,0x00),(6,2)]:
            bad=bytearray(original);bad[at]=value
            with self.assertRaises(ValueError):verify_constant_replacement(bytes(bad),target)
        for bad in [target[:-1],target+b'\0',target[:-1]+b'\x02',struct.pack('<HH',2,1)+target[4:36]+b'\x11']:
            with self.assertRaises(ValueError):verify_constant_replacement(original,bad)
    def test_constant_repack_preserves_rotated_geometry_frames_and_records(self):
        raw=room();replacements=constant_texture_replacements(raw);new,record=deduplicate_room(raw,replacements)
        verify_constant_room(raw,new,record['old_to_canonical'],replacements)
        self.assertEqual(textures(new)[record['old_to_canonical'][0]],replacements[0])
        self.assertEqual(constant_texture_replacements(new),{})  #4x4 is already canonical.
        at=40+struct.unpack_from('<I',new,12)[0]*16
        for delta in [8,40,43,44+4,44+20,44+20+16]:
            changed=bytearray(new);changed[at+delta]^=1
            with self.assertRaises(ValueError):verify_constant_room(raw,bytes(changed),record['old_to_canonical'],replacements)
        with self.assertRaises(ValueError):verify_constant_room(raw,new,record['old_to_canonical'],[])  # No silent exemption.
    def test_oversized_draw_keeps_exact_texture(self):
        from scenery_geometry import texture_draws_safe
        raw=room();self.assertTrue(texture_draws_safe(raw,0,4,4))
        giant=bytearray(raw);struct.pack_into('<8i',giant,40+3*16+8,0,0,331*256,0,0,2727*256,331*256,2727*256);giant=bytes(giant)
        self.assertFalse(texture_draws_safe(giant,0,4,4));self.assertTrue(texture_draws_safe(giant,1,4,4))
        self.assertEqual(constant_texture_replacements(giant),{})
        self.assertEqual(set(constant_texture_replacements(raw)),{0})

if __name__=='__main__':unittest.main()
