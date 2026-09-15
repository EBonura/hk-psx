import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from title_music import sector_pad
class TitleTests(unittest.TestCase):
 def test_cdda_sector_alignment_retains_full_stereo_samples(self):
  for size in [4,2352,2356,14429520]:
   data=bytes([0x12,0x34,0x56,0x78])*(size//4);out=sector_pad(data)
   self.assertEqual(out[:size],data);self.assertEqual(len(out)%2352,0)
   self.assertEqual(out[size:],bytes((-size)%2352));self.assertLess(len(out)-size,2352)
  for data in [b'',b'x',bytes(6)]:
   with self.assertRaises(ValueError):sector_pad(data)
