"""Compile the actual bounded occlusion helper and check its pixel partitions."""
import subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class SceneryOcclusionTests(unittest.TestCase):
 def test_native_pixel_partitions(self):
  with tempfile.TemporaryDirectory(prefix='hk-occlusion-') as tmp:
   out=Path(tmp)/'test'
   p=subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'tests/scenery_occlusion_runtime.rs'),'-o',str(out)],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stderr)
   p=subprocess.run([str(out)],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stdout+p.stderr)
