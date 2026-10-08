"""Verify production sparse visibility reset and admission against old semantics."""
import subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class DrawVisibilityTests(unittest.TestCase):
 def test_native_state_sequences(self):
  with tempfile.TemporaryDirectory(prefix='hk-visibility-') as tmp:
   out=Path(tmp)/'test'
   p=subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'tests/draw_visibility_runtime.rs'),'-o',str(out)],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stderr)
   p=subprocess.run([str(out)],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stdout+p.stderr)
