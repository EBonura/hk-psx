"""Resolution experiments preserve source instance geometry and animation texels."""
import struct
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from PIL import Image
from cook import Atlas
from region_delta import layout, textures
from scenery_budget import recook, summarize

class BudgetTests(unittest.TestCase):
    def fixture(self):
        image=Image.new('RGBA',(128,64),(70,90,130,255))
        image.putpixel((0,0),(0,0,0,0))
        atlas=Atlas();atlas.add(image,128,64)
        atlas.add(Image.new('RGBA',(3,2),(240,230,220,255)),3,2,streamed=True);atlas.pack()
        points=[[0,64,0],[128,64,0],[0,0,0],[128,0,0]]
        draw={'source':'scene:1','sprite':'sprites:2','points':points,'scale':1,'z':0,'tint':[128,80,100]}
        raw=bytearray(b'HKROOM02'+struct.pack('<8I',1,2,1,1,1,1,len(atlas.stream),atlas.flags))
        for entry in atlas.entries:raw.extend(struct.pack('<6HI',*entry))
        raw.extend(struct.pack('<HHI8i3Bx',0,0,4096,*[round(p[k]*256)for p in points for k in (0,1)],*draw['tint']))
        raw.extend(struct.pack('<I4i',1,-10,-20,30,40))
        raw.extend(struct.pack('<4I',0,1,12*65536,0));raw.extend(struct.pack('<4i',0,0,65536,0))
        raw.extend(b''.join(atlas.palettes));raw.extend(b''.join(atlas.pages));raw.extend(atlas.stream)
        renderer={'m_Color':{'a':1.0}}
        sprite=SimpleNamespace(read=lambda:SimpleNamespace(image=image))
        source=SimpleNamespace(file=lambda name:SimpleNamespace(objects={1:renderer,2:sprite}),read=lambda obj:obj)
        return bytes(raw),draw,source

    def test_only_static_resolution_changes(self):
        raw,draw,source=self.fixture();packed,report=recook(raw,[draw],source,64,{}, {})
        old,new=textures(raw),textures(packed)
        self.assertEqual(struct.unpack_from('<HH',new[0]),(64,32))
        self.assertEqual(old[1],new[1])
        self.assertEqual(raw[72+2:116],packed[72+2:116])
        self.assertEqual(raw[116+4:layout(raw)[1]],packed[116+4:layout(packed)[1]])
        self.assertEqual(report['draws_with_different_texels'],1)
        self.assertEqual(report['animations_preserved'],1)

    def test_original_cap_reproduces_exact_texture_and_rejects_wrong_draw_provenance(self):
        raw,draw,source=self.fixture();packed,report=recook(raw,[draw],source,128,{}, {})
        self.assertEqual(textures(raw),textures(packed))
        self.assertEqual(packed,raw)
        self.assertEqual(report['draws_with_different_texels'],0)
        draw['tint']=[128,128,128]
        with self.assertRaisesRegex(ValueError,'provenance differs'):recook(raw,[draw],source,64,{}, {})

    def test_three_bank_window_does_not_claim_all_neighbors_resident(self):
        rows=[{'chunk_id':i,'pages':p,'raw_bytes':r,'stored_bytes':10}for i,p,r in [(1,3,100),(2,5,200),(3,6,300),(4,2,400)]]
        regions=[{'chunk_id':1,'neighbour_chunks':[2,3,4]}]
        report=summarize(rows,regions)
        self.assertEqual(report['max_three_raw_bytes'],800)
        self.assertEqual(report['max_three_pages'],14)
        self.assertEqual(report['windows'][0]['all_neighbor_raw_bytes'],1000)

if __name__=='__main__':unittest.main()
