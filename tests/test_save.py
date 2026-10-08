"""Compile the real save module against a stub card and run its record round-trip."""
import subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class SaveRecordTests(unittest.TestCase):
    def test_record_round_trip_and_corruption_rejection(self):
        with tempfile.TemporaryDirectory(prefix='hk-save-test-') as temp:
            binary=Path(temp)/'save-tests'
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',str(ROOT/'tests/save_runtime.rs'),'-o',str(binary)],capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
