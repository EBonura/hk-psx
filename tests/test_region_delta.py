import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'host'))
from region_delta import apply_delta, compressed, encode_delta, reconstruct, textures


def room(change=False, moved=False):
    # Two palette orders, odd atlas X, odd width, padded streaming stride.
    prefix = b'HKROOM02'+struct.pack('<8I',1,2,0,0,0,0,12,0)
    prefix += struct.pack('<6HI',0,7 if moved else 1,3,3,2,1,0)
    prefix += struct.pack('<6HI',65535,0,0,5,3,0,0)
    size = len(prefix)+64+32768+12
    first = struct.pack('<HH',3,2)+bytes(range(32))+bytes([0x21,3,0x54,6])
    second = struct.pack('<HH',5,3)+bytes(reversed(range(32)))+bytes([0x98,0x76,5])*3
    if change:
        second = second[:4]+b'\x55'+second[5:]
    return reconstruct(prefix, size, [first,second])


class RegionDeltaTests(unittest.TestCase):
    def test_relocated_atlas_is_exact_shared_texture(self):
        old,new=room(),room(moved=True)
        self.assertEqual(textures(old),textures(new))
        delta=encode_delta(old,new)
        self.assertEqual(struct.unpack_from('<I',delta,20)[0],0)
        self.assertEqual(apply_delta(old,compressed(delta)),new)

    def test_palette_difference_requires_new_blob(self):
        old,new=room(),room(change=True)
        delta=encode_delta(old,new)
        self.assertEqual(struct.unpack_from('<I',delta,20)[0],1)
        self.assertEqual(apply_delta(old,delta),new)

    def test_empty_base_roundtrip(self):
        target=room()
        self.assertEqual(apply_delta(b'',compressed(encode_delta(b'',target))),target)

    def test_wrong_base_and_corrupt_target_rejected(self):
        old,new=room(),room(change=True)
        delta=encode_delta(old,new)
        with self.assertRaises(ValueError):apply_delta(new,delta)
        damaged=delta[:-1]+bytes([delta[-1]^1])
        with self.assertRaises(ValueError):apply_delta(old,damaged)


if __name__=='__main__':unittest.main()
