"""Opaque occluder extraction from synthetic final PSX palette/atlas bytes."""
import random
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from pack_scenes import black_cores,opaque_cores

def bank(rows,palette,u=3,v=5,page=0):
    h=len(rows);w=len(rows[0]);raw=bytearray(64+32768)
    struct.pack_into('<6HI',raw,0,page,u,v,w,h,0,0)
    struct.pack_into('<16H',raw,16,*(palette+[0]*(16-len(palette))))
    for y,row in enumerate(rows):
        for x,index in enumerate(row):
            raw[64+(v+y)*128+(u+x)//2]|=index<<(((u+x)&1)*4)
    return raw,{'textures':1,'sections':{'textures':{'offset':0},'palettes':{'offset':16},'pages':{'offset':64}}}

class OpaqueCores(unittest.TestCase):
    def test_coloured_opaque_texels_expand_core_without_changing_black(self):
        raw,entry=bank([[0]*4+[1]*4 for _ in range(16)],[1,0x421])
        self.assertEqual(black_cores(raw,entry),[[0,0,4,16]])
        self.assertEqual(opaque_cores(raw,entry),[[0,0,8,16]])

    def test_word_semantics_exclude_transparent_and_all_stp_colours(self):
        # Index zero is opaque here. Transparent black and both STP-black and
        # STP-colour rings must not become occluders, even with an opaque centre.
        rows=[[1]*14 for _ in range(14)]
        for y in range(1,13):
            for x in range(1,13):rows[y][x]=2
        for y in range(2,12):
            for x in range(2,12):rows[y][x]=3
        for y in range(3,11):
            for x in range(3,11):rows[y][x]=0
        raw,entry=bank(rows,[0x7fff,0,0x8000,0x8421])
        self.assertEqual(opaque_cores(raw,entry),[[3,3,8,8]])
        self.assertEqual(black_cores(raw,entry),[[0,0,0,0]])

    def test_small_or_streamed_cores_are_not_admitted(self):
        for rows,page in [([[0]*7 for _ in range(9)],0),([[0]*8 for _ in range(8)],65535)]:
            raw,entry=bank(rows,[0x421],page=page)
            self.assertEqual(opaque_cores(raw,entry),[[0,0,0,0]])

    def test_largest_rectangle_matches_independent_exhaustive_search(self):
        rng=random.Random(741)
        for _ in range(12):
            rows=[[rng.randrange(4) for x in range(12)]for y in range(12)]
            for y in range(2,10):
                for x in range(1,9):rows[y][x]=rng.randrange(2)
            raw,entry=bank(rows,[1,0x421,0,0x8421])
            x,y,w,h=opaque_cores(raw,entry)[0]
            self.assertTrue(all(rows[yy][xx]<2 for yy in range(y,y+h)for xx in range(x,x+w)))
            best=0
            for left in range(12):
                for right in range(left+1,13):
                    run=0
                    for row in rows:
                        run=run+1 if all(i<2 for i in row[left:right]) else 0
                        best=max(best,run*(right-left))
            self.assertEqual(w*h,best)

if __name__=='__main__':unittest.main()
