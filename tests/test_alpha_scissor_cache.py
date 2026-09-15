"""Compile the production cache and compare shared/uncached exact scissors."""
import subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class AlphaScissorCacheTests(unittest.TestCase):
 def test_native_cache_parity(self):
  with tempfile.TemporaryDirectory(prefix='hk-alpha-cache-') as tmp:
   out=Path(tmp)/'test'
   p=subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'game/src/alpha_scissor_cache.rs'),'-o',str(out)],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stderr)
   p=subprocess.run([str(out)],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stdout+p.stderr)
