"""Compile the production trimming policy and run its unit tests."""
import subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class TrimPolicyTests(unittest.TestCase):
 def test_native_policy(self):
  with tempfile.TemporaryDirectory(prefix='hk-trim-policy-') as tmp:
   out=Path(tmp)/'test'
   p=subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'game/src/trim_policy.rs'),'-o',str(out)],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stderr)
   p=subprocess.run([str(out)],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stdout+p.stderr)
