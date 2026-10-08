import subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class SolidOccluderTests(unittest.TestCase):
 def test_native_convex_cache_and_input_bounds(self):
  with tempfile.TemporaryDirectory(prefix='hk-solid-occluder-')as tmp:
   out=Path(tmp)/'run';p=subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'tests/solid_occluder_runtime.rs'),'-o',str(out)],capture_output=True,text=True);self.assertEqual(p.returncode,0,p.stderr)
   p=subprocess.run([str(out)],capture_output=True,text=True);self.assertEqual(p.returncode,0,p.stdout+p.stderr)
