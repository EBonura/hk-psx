"""Check exact cached scenery color against the original packet operations."""
import subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class SceneryColorTests(unittest.TestCase):
 def test_native_color_parity(self):
  with tempfile.TemporaryDirectory(prefix='hk-color-') as tmp:
   out=Path(tmp)/'test'
   p=subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'game/src/scenery_color.rs'),'-o',str(out)],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stderr)
   p=subprocess.run([str(out)],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stdout+p.stderr)
