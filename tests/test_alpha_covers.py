"""Exact PSX colour support, disjoint covers and lossless cooker integration."""
import struct
import sys
import unittest
from pathlib import Path
import random
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from alpha_covers import cover,record,support
from cook import Atlas
from region_delta import apply_delta,encode_delta,textures
from texture_dedup import deduplicate_room


def packed(atlas):
    return (b'HKROOM02'+struct.pack('<8I',len(atlas.pages),len(atlas.entries),0,0,0,0,len(atlas.stream),atlas.flags)
            +b''.join(struct.pack('<6HI',*e) for e in atlas.entries)
            +b''.join(atlas.palettes)+b''.join(atlas.pages)+atlas.stream)

class AlphaCoverTests(unittest.TestCase):
    def assert_cover(self,mask):
        rects=cover(mask);self.assertLessEqual(len(rects),4)
        count=[[0]*len(mask[0]) for _ in mask]
        for x,y,r,b in rects:
            self.assertTrue(0<=x<r<=len(mask[0]) and 0<=y<b<=len(mask))
            for yy in range(y,b):
                for xx in range(x,r):count[yy][xx]+=1
        self.assertTrue(all(n<=1 for row in count for n in row))
        self.assertTrue(all(count[y][x]==1 for y,row in enumerate(mask) for x,on in enumerate(row) if on))
        self.assertEqual(rects,cover(mask))
        return rects
    def test_full_256_dimensions_and_empty(self):
        palette=struct.pack('<16H',0,0x8000,*([0]*14))
        self.assertEqual(record(256,256,palette,b'\x11'*32768),bytes([1,0,0,0,0,0,255,255])+bytes(12))
        self.assertEqual(record(1,1,palette,b'\x00'),bytes(20))
    def test_actual_colour_word_and_odd_padding(self):
        palette=struct.pack('<16H',1,0,0x8000,*([0]*13))
        self.assertEqual(support(3,2,palette,bytes([0x10,0x12,0x11,0xF1])),[[True,False,True],[False,False,False]])
        # Index zero can be visible; a nonzero index may still be transparent.
        self.assertEqual(record(3,2,palette,bytes([0x10,0x12,0x11,0xF1]))[0],2)
    def test_guillotine_four_islands_and_random_support(self):
        mask=[[False]*12 for _ in range(12)]
        for y,x in [(0,0),(0,10),(10,0),(10,10)]:
            for yy in range(y,y+2):mask[yy][x:x+2]=[True,True]
        self.assertEqual(len(self.assert_cover(mask)),4)
        rng=random.Random(703)
        for size in [(1,1),(3,9),(19,13)]:
            for density in [0,0.1,0.7,1]:self.assert_cover([[rng.random()<density for _ in range(size[1])] for _ in range(size[0])])
    def test_appending_metadata_changes_no_pixels_palettes_or_animation(self):
        palette=struct.pack('<16H',0,*range(1,16))
        atlases=[]
        for enabled in [False,True]:
            a=Atlas(alpha_covers=enabled)
            for streamed in [False,True,False]:a.add_quantized(3,2,palette,b'\x21\x03\x54\x06',streamed)
            a.pack();atlases.append(a)
        old,new=atlases
        self.assertEqual(new.pages,old.pages);self.assertEqual(new.palettes,old.palettes)
        self.assertEqual(new.stream[:len(old.stream)],old.stream)
        self.assertEqual(new.animation_bytes,len(old.stream));self.assertEqual(new.alpha_cover_bytes,20)
        self.assertEqual(new.entries[1],old.entries[1])
        self.assertEqual(new.entries[0][:6],old.entries[0][:6]);self.assertEqual(new.entries[0][6],len(old.stream))
        raw=packed(new);legacy=packed(old)
        self.assertEqual(textures(raw),textures(legacy))
        first,report=deduplicate_room(raw);second,_=deduplicate_room(first)
        self.assertEqual(first,raw);self.assertEqual(second,first);self.assertEqual(report['alpha_cover_bytes'],20)
        self.assertEqual(apply_delta(legacy,encode_delta(legacy,raw)),raw)
        self.assertEqual(apply_delta(b'',encode_delta(b'',raw)),raw)
    def test_malformed_host_input(self):
        for args in [(0,1,bytes(32),b''),(257,1,bytes(32),bytes(129)),(1,1,bytes(30),b'\0'),(3,1,bytes(32),b'\0')]:
            with self.assertRaises(ValueError):record(*args)

if __name__=='__main__':unittest.main()
