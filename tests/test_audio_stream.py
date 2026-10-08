"""Run the ambience stream state machine against the cooked bank.

tests/audio_stream_runtime.rs had no runner, so the boundary window silently
went stale when the cave bed's rate changed. This is what catches that.
"""
import os,subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class AudioStreamTests(unittest.TestCase):
    def test_boundary_window_follows_the_cooked_pitch(self):
        with tempfile.TemporaryDirectory(prefix='hk-stream-test-') as temp:
            binary=Path(temp)/'stream-tests'
            env=dict(os.environ,CARGO_MANIFEST_DIR=str(ROOT))
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',
                str(ROOT/'tests/audio_stream_runtime.rs'),'-o',str(binary)],
                env=env,capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
