"""Compile the real NPC conversation state module and run its own tests."""
import subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class NpcStateTests(unittest.TestCase):
    def test_cursor_and_panel_state(self):
        with tempfile.TemporaryDirectory(prefix='hk-npc-state-test-') as temp:
            binary=Path(temp)/'npc-state-tests'
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',
                                     str(ROOT/'tests/npc_state_runtime.rs'),'-o',str(binary)],
                                    capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
